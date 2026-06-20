#!/usr/bin/env bash
# Full pre-commit gate — must match AGENTS.md § Pre-Commit Gate.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> cargo fmt --all"
cargo fmt --all

echo "==> cargo clippy --workspace -- -D warnings"
cargo clippy --workspace -- -D warnings

if [[ "${PRE_COMMIT_SKIP_CVC5_NATIVE:-}" == "1" ]]; then
  echo "==> SKIP cvc5-verify clippy (PRE_COMMIT_SKIP_CVC5_NATIVE=1)"
elif bash scripts/check-cvc5-env.sh --quiet 2>/dev/null; then
  echo "==> cargo clippy -p assura-smt --features cvc5-verify -- -D warnings"
  cargo clippy -p assura-smt --features cvc5-verify -- -D warnings
else
  echo "WARNING: cvc5-verify clippy skipped (no native env)"
  echo "  Run: bash scripts/setup-cvc5.sh"
  echo "  CI cvc5 job still required before closing cvc5-parity issues"
fi

echo "==> cargo test --workspace"
cargo test --workspace

echo "==> cargo check --no-default-features -p assura-smt"
cargo check --no-default-features -p assura-smt

echo "pre-commit gate: OK"