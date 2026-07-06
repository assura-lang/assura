#!/usr/bin/env bash
# Assert the crates.io publish plan matches the co-publish stack (libraries +
# frontends + CLI). See docs/CRATES-IO.md and issue #838.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

plan_line=$(bash scripts/publish-crates.sh --plan-only 2>/dev/null | head -1)
if [[ ! "$plan_line" =~ Publish\ plan\ \(([0-9]+)\ crates\):\ (.*)$ ]]; then
  echo "error: could not parse publish plan line: $plan_line" >&2
  exit 1
fi
count="${BASH_REMATCH[1]}"
# shellcheck disable=SC2206
ORDER=(${BASH_REMATCH[2]})

# Topo order uses ALL path deps (normal + build + dev).
expected=(
  assura-ast assura-config assura-diagnostics assura-runtime
  assura-rust-analyzer assura-parser assura-macros assura-llm assura-fmt
  assura-stdlib assura-resolve assura-types assura-codegen assura-smt
  assura-pipeline assura-lsp assura-mcp assura
)

if [[ "$count" -ne ${#expected[@]} ]] || [[ ${#ORDER[@]} -ne ${#expected[@]} ]]; then
  echo "error: publish plan has count=$count len=${#ORDER[@]}, expected ${#expected[@]}" >&2
  echo "  got: ${ORDER[*]}" >&2
  exit 1
fi

for i in "${!expected[@]}"; do
  if [[ "${ORDER[$i]}" != "${expected[$i]}" ]]; then
    echo "error: plan[$i]=${ORDER[$i]} expected ${expected[$i]}" >&2
    exit 1
  fi
done

for bad in assura-cli assura-test-support assura-server assura-bench; do
  for c in "${ORDER[@]}"; do
    if [[ "$c" == "$bad" ]]; then
      echo "error: publish plan must not include $bad" >&2
      exit 1
    fi
  done
done

echo "publish plan ok (${#ORDER[@]} crates, ends with assura (CLI crate name))"
