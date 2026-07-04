#!/usr/bin/env bash
# Publish workspace crates to crates.io in dependency order.
#
# Fail-closed: any unexpected publish error exits non-zero.
# "already uploaded" for this version is treated as success (idempotent re-runs).
#
# The set of crates and order are derived from the workspace graph:
#   - package.publish is not false
#   - every path dependency (normal/build/dev) on a workspace crate is also
#     publishable (otherwise cargo packaging cannot resolve it from crates.io)
#   - topological order by normal+build path dependencies among that set
#
# Usage (from repo root):
#   CARGO_REGISTRY_TOKEN=... bash scripts/publish-crates.sh
#   bash scripts/publish-crates.sh --dry-run
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DRY_RUN=0
PLAN_ONLY=0
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN=1
elif [[ "${1:-}" == "--plan-only" ]]; then
  PLAN_ONLY=1
  DRY_RUN=1
fi

if [[ "$DRY_RUN" -eq 0 && -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
  echo "error: CARGO_REGISTRY_TOKEN is not set" >&2
  exit 1
fi

mapfile -t ORDER < <(python3 - <<'PY'
"""Compute fail-closed publish plan from the workspace."""
from __future__ import annotations

import sys
import tomllib
from collections import defaultdict, deque
from pathlib import Path

root = Path("crates")
packages: dict[str, dict] = {}

for cargo_toml in sorted(root.glob("*/Cargo.toml")):
    data = tomllib.loads(cargo_toml.read_text())
    pkg = data["package"]
    name = pkg["name"]
    publish = pkg.get("publish", True)
    publishable = publish is not False and publish != []

    # Path deps that affect packaging (all sections).
    all_path: set[str] = set()
    # Path deps that determine publish *order* (runtime/build only).
    order_path: set[str] = set()

    for section in ("dependencies", "build-dependencies", "dev-dependencies"):
        for dep_name, spec in data.get(section, {}).items():
            if not (isinstance(spec, dict) and "path" in spec):
                continue
            all_path.add(dep_name)
            if section != "dev-dependencies":
                order_path.add(dep_name)
            if publishable and "version" not in spec:
                print(
                    f"error: {name} [{section}] depends on {dep_name} via path "
                    f"without version=; add version = \"0.1.0\" before publish",
                    file=sys.stderr,
                )
                sys.exit(2)

    packages[name] = {
        "publishable_flag": publishable,
        "all_path": all_path,
        "order_path": order_path,
    }

workspace_names = set(packages)

candidates = {n for n, i in packages.items() if i["publishable_flag"]}
blocked: dict[str, list[str]] = {}
for name in sorted(candidates):
    bad = sorted(
        d
        for d in packages[name]["all_path"]
        if d in workspace_names and not packages[d]["publishable_flag"]
    )
    if bad:
        blocked[name] = bad

publishable = candidates - set(blocked)

if blocked:
    print(
        "note: excluding packages with path deps on unpublished workspace crates "
        "(cargo packaging cannot resolve them from crates.io):",
        file=sys.stderr,
    )
    for name, bad in blocked.items():
        print(f"  - {name} -> {', '.join(bad)}", file=sys.stderr)

if not publishable:
    print("error: no publishable crates remain after graph filtering", file=sys.stderr)
    sys.exit(1)

# Order edges must include *dev* path deps too: cargo publish resolves
# [dev-dependencies] against crates.io while packaging (even though they are
# not required to build the lib). Omitting them publishes macros before
# runtime and fails with "no matching package named assura-runtime".
graph = {
    n: {d for d in packages[n]["all_path"] if d in publishable} for n in publishable
}
indeg = {n: 0 for n in publishable}
rev: dict[str, set[str]] = defaultdict(set)
for n, deps in graph.items():
    for d in deps:
        rev[d].add(n)
        indeg[n] += 1

queue = deque(sorted(n for n, i in indeg.items() if i == 0))
order: list[str] = []
while queue:
    n = queue.popleft()
    order.append(n)
    for m in sorted(rev[n]):
        indeg[m] -= 1
        if indeg[m] == 0:
            queue.append(m)

if len(order) != len(publishable):
    stuck = sorted(publishable - set(order))
    print(f"error: dependency cycle among publishable crates: {stuck}", file=sys.stderr)
    sys.exit(1)

for name in order:
    print(name)
PY
)

if [[ ${#ORDER[@]} -eq 0 ]]; then
  echo "error: empty publish order" >&2
  exit 1
fi

echo "Publish plan (${#ORDER[@]} crates): ${ORDER[*]}"

if [[ "$PLAN_ONLY" -eq 1 ]]; then
  exit 0
fi

# Returns: 0 new publish, 1 already on index, 2 hard failure (prints error).
# Retries crates.io 429 (new-crate rate limit) with backoff.
publish_one() {
  local crate="$1"
  local args=(-p "$crate" --locked)
  if [[ "$DRY_RUN" -eq 1 ]]; then
    # Local verification often runs with uncommitted packaging fixes.
    args+=(--dry-run --allow-dirty)
  fi

  echo "=== Publishing ${crate} ==="
  local attempt=1
  local max_attempts=8
  local out rc wait_s

  while true; do
    set +e
    out="$(cargo publish "${args[@]}" 2>&1)"
    rc=$?
    set -e
    printf '%s\n' "$out"

    if [[ $rc -eq 0 ]]; then
      return 0
    fi

    if printf '%s\n' "$out" | grep -Eqi 'already (exists|uploaded)|is already uploaded|already exists on crates\.io'; then
      echo "note: ${crate} already published at this version; treating as success"
      return 1
    fi

    # First-time monorepo dry-run: dependents need prior crates on the real
    # crates.io index. Real publishes land earlier crates first, so this must
    # still fail closed when DRY_RUN=0.
    if [[ "$DRY_RUN" -eq 1 ]] && printf '%s\n' "$out" | grep -Eq 'no matching package named `assura'; then
      echo "note: ${crate} dry-run blocked on unpublished workspace deps (expected for first release dry-run); graph preflight already passed"
      return 1
    fi

    if [[ "$DRY_RUN" -eq 0 ]] && printf '%s\n' "$out" | grep -Eqi '429|Too Many Requests|rate.limit|published too many'; then
      if [[ $attempt -ge $max_attempts ]]; then
        echo "error: rate-limited publishing ${crate} after ${max_attempts} attempts" >&2
        return 2
      fi
      # crates.io often returns "try again after <HTTP date>"; default 90s, grow.
      wait_s=$((90 * attempt))
      echo "note: crates.io rate limit on ${crate}; sleeping ${wait_s}s (attempt ${attempt}/${max_attempts})"
      sleep "$wait_s"
      attempt=$((attempt + 1))
      continue
    fi

    echo "error: cargo publish failed for ${crate} (exit ${rc})" >&2
    return 2
  done
}

last="${ORDER[$((${#ORDER[@]} - 1))]}"
for crate in "${ORDER[@]}"; do
  set +e
  publish_one "$crate"
  rc=$?
  set -e
  if [[ $rc -ge 2 ]]; then
    exit 1
  fi
  # Space out *new* crate publishes (crates.io new-crate rate limits).
  # Skip long sleep when the crate was already on the index (rc=1).
  if [[ "$DRY_RUN" -eq 0 && "$crate" != "$last" && $rc -eq 0 ]]; then
    echo "note: waiting 60s before next new crate publish (crates.io rate limits)"
    sleep 60
  fi
done

if [[ "$DRY_RUN" -eq 1 ]]; then
  echo "Dry-run finished for ${#ORDER[@]} crate(s). Leaves fully verified; dependents need a real publish for end-to-end packaging."
else
  echo "All ${#ORDER[@]} crate(s) published successfully."
fi
