#!/usr/bin/env bash
# Fail-closed gate: cargo package every publishable workspace crate.
#
# Catches monorepo-only assets (e.g. include_str! paths outside the crate)
# that normal cargo test/clippy of the workspace miss. See issue #814.
#
# Usage (from repo root):
#   bash scripts/check-cargo-package.sh
#   bash scripts/check-cargo-package.sh --list-only   # fast file list only
#
# On co-publish version-bump PRs (workspace version not yet on crates.io),
# full `cargo package` fails with "candidate versions found which didn't
# match" because path deps pin version= to the *new* version that only
# exists locally. In that case we fall back to --list (still catches bad
# package membership) and full package+verify remains the gate once the
# version exists on crates.io (main after release, or re-publish re-runs).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

LIST_ONLY=0
FORCE_FULL=0
if [[ "${1:-}" == "--list-only" ]]; then
  LIST_ONLY=1
elif [[ "${1:-}" == "--full" ]]; then
  FORCE_FULL=1
elif [[ -n "${1:-}" ]]; then
  echo "usage: $0 [--list-only|--full]" >&2
  exit 2
fi

# Fail fast if the expected publish set/order drifts (also run in lint-fast).
bash scripts/check-publish-plan.sh

plan_line=$(bash scripts/publish-crates.sh --plan-only 2>/dev/null | head -1)
if [[ ! "$plan_line" =~ Publish\ plan\ \(([0-9]+)\ crates\):\ (.*)$ ]]; then
  echo "error: could not parse publish plan line: $plan_line" >&2
  exit 1
fi
# shellcheck disable=SC2206
ORDER=(${BASH_REMATCH[2]})

if [[ ${#ORDER[@]} -eq 0 ]]; then
  echo "error: empty publish plan" >&2
  exit 1
fi

# Workspace version from [workspace.package]
WS_VER=$(
  python3 - <<'PY'
import re
from pathlib import Path
cargo = Path("Cargo.toml").read_text()
m = re.search(
    r"(?ms)^\[workspace\.package\]\s*.*?^version\s*=\s*\"([^\"]+)\"",
    cargo,
)
if not m:
    raise SystemExit("could not find [workspace.package] version")
print(m.group(1))
PY
)

if [[ "$LIST_ONLY" -eq 0 && "$FORCE_FULL" -eq 0 ]]; then
  # Full package requires every co-published member (and thus every path+version
  # peer) to already exist on crates.io at WS_VER. If we only probe the first
  # leaf (e.g. assura-ast) after expanding the set (CLI/frontends), mid-graph
  # packages fail packaging with "no matching package named X".
  missing=()
  for crate in "${ORDER[@]}"; do
    code=$(
      curl -sS -o /dev/null -w "%{http_code}" \
        -A "assura-check-cargo-package/1.0 (https://github.com/assura-lang/assura)" \
        "https://crates.io/api/v1/crates/${crate}/${WS_VER}" || echo "000"
    )
    if [[ "$code" != "200" ]]; then
      missing+=("${crate}")
    fi
  done
  if [[ ${#missing[@]} -gt 0 ]]; then
    echo "note: ${#missing[@]} co-publish member(s) at ${WS_VER} not on crates.io yet:"
    printf 'note:   - %s\n' "${missing[@]}"
    echo "note: co-publish cannot full-package interdependent crates (path+version"
    echo "note: deps resolve against the registry). Using --list only; full"
    echo "note: package+verify after all members exist on crates.io."
    LIST_ONLY=1
  fi
fi

mode_label="full package+verify"
if [[ "$LIST_ONLY" -eq 1 ]]; then
  mode_label="--list only"
fi
echo "cargo package gate: ${#ORDER[@]} publishable crates (${mode_label}, workspace ${WS_VER})"

failed=()
for crate in "${ORDER[@]}"; do
  echo "==> cargo package -p ${crate} --locked$([ "$LIST_ONLY" -eq 1 ] && echo ' --list' || true)"
  if [[ "$LIST_ONLY" -eq 1 ]]; then
    if ! cargo package -p "$crate" --locked --list >/dev/null; then
      echo "error: cargo package --list failed for ${crate}" >&2
      failed+=("$crate")
    fi
  else
    # Full package + verify (build extracted tarball). This is the mode that
    # would have caught missing monorepo templates inside assura-smt (#812).
    if ! cargo package -p "$crate" --locked; then
      echo "error: cargo package failed for ${crate}" >&2
      echo "  hint: include_str! / build scripts must only reference files" >&2
      echo "  under crates/${crate}/ so they ship in the package tarball." >&2
      echo "  hint: if this is a pre-publish version bump and deps are not" >&2
      echo "  on crates.io yet, re-run without --full or wait until co-publish." >&2
      failed+=("$crate")
    fi
  fi
done

if [[ ${#failed[@]} -gt 0 ]]; then
  echo "error: cargo package gate failed for: ${failed[*]}" >&2
  exit 1
fi

echo "cargo package gate ok (${#ORDER[@]} crates, ${mode_label})"
