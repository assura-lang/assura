#!/usr/bin/env bash
# Full pre-commit gate — must match AGENTS.md § Pre-Commit Gate.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> cargo fmt --all"
cargo fmt --all

echo "==> cargo clippy --workspace -- -D warnings"
cargo clippy --workspace -- -D warnings

echo "==> cargo clippy -p assura-smt --features cvc5-verify -- -D warnings"
cargo clippy -p assura-smt --features cvc5-verify -- -D warnings

echo "==> cargo test --workspace"
cargo test --workspace

echo "==> cargo check --no-default-features -p assura-smt"
cargo check --no-default-features -p assura-smt

echo "pre-commit gate: OK"