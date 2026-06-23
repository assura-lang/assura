#!/usr/bin/env bash
# Fast checks for agent sessions. Prefer this over full `cargo test --workspace`.
#
# Usage:
#   bash scripts/agent-preflight.sh              # types + pipeline + smt lib + CLI bin
#   bash scripts/agent-preflight.sh assura-types # one crate only
#   bash scripts/agent-preflight.sh assura-types assura-smt
#
# Related scaffolds (print-only, not run here):
#   bash scripts/agent-new-checker.sh <name> [--category <stem>]
#   bash scripts/agent-new-decl.sh <Variant>
set -euo pipefail
cd "$(dirname "$0")/.."

crates=("${@:-assura-types assura-pipeline assura-config assura-ast assura-test-support}")

echo "== agent-preflight: fmt check =="
cargo fmt --all -- --check

echo "== agent-preflight: agent guards =="
bash scripts/agent-guards.sh

for crate in "${crates[@]}"; do
  echo "== agent-preflight: clippy -p $crate =="
  if [[ "$crate" == "assura" ]]; then
    cargo clippy --bin assura --locked -- -D warnings
  else
    cargo clippy -p "$crate" --lib --locked -- -D warnings 2>/dev/null \
      || cargo clippy -p "$crate" --locked -- -D warnings
  fi
done

# Always sanity-check the binary if not explicitly listed
if [[ " ${crates[*]} " != *" assura "* ]]; then
  echo "== agent-preflight: clippy --bin assura =="
  cargo clippy --bin assura --locked -- -D warnings
fi

echo "== agent-preflight: demo check =="
cargo run -q --bin assura -- check demos/libwebp-huffman.assura >/dev/null

echo "agent-preflight: OK"
