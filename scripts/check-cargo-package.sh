#!/usr/bin/env bash
# Fail-closed gate: cargo package every publishable workspace crate.
#
# Catches monorepo-only assets (e.g. include_str! paths outside the crate)
# that normal cargo test/clippy of the workspace miss. See issue #814.
#
# Usage (from repo root):
#   bash scripts/check-cargo-package.sh
#   bash scripts/check-cargo-package.sh --list-only   # fast file list only
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

LIST_ONLY=0
if [[ "${1:-}" == "--list-only" ]]; then
  LIST_ONLY=1
elif [[ -n "${1:-}" ]]; then
  echo "usage: $0 [--list-only]" >&2
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

echo "cargo package gate: ${#ORDER[@]} publishable crates"
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
      failed+=("$crate")
    fi
  fi
done

if [[ ${#failed[@]} -gt 0 ]]; then
  echo "error: cargo package gate failed for: ${failed[*]}" >&2
  exit 1
fi

echo "cargo package gate ok (${#ORDER[@]} crates)"
