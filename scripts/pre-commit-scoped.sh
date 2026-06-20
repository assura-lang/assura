#!/usr/bin/env bash
# Scoped verification before push — fast path (~1–4 min). Full gate: pre-commit-gate.sh
set -euo pipefail

cd "$(dirname "$0")/.."

find_crate_dir() {
  local name="$1"
  local d toml
  for toml in crates/*/Cargo.toml; do
    if grep -q "^name = \"${name}\"" "$toml" 2>/dev/null; then
      dirname "$toml"
      return 0
    fi
  done
  # Directory name passed (e.g. assura-cli → crates/assura-cli)
  if [[ -f "crates/${name}/Cargo.toml" ]]; then
    echo "crates/${name}"
    return 0
  fi
  return 1
}

resolve_crate_name() {
  local arg="${1:-}"
  if [[ -z "$arg" ]]; then
    arg="$(git diff --name-only HEAD 2>/dev/null | grep '^crates/' | head -1 | cut -d/ -f2 || true)"
    arg="${arg:-assura-smt}"
  fi
  local toml="crates/${arg}/Cargo.toml"
  if [[ -f "$toml" ]]; then
    grep -m1 '^name = ' "$toml" | sed 's/name = "\(.*\)"/\1/'
  else
    echo "$arg"
  fi
}

CRATE="$(resolve_crate_name "${1:-}")"
CRATE_DIR="$(find_crate_dir "$CRATE" || find_crate_dir "${1:-}" || true)"

echo "==> scoped gate (crate: ${CRATE})"

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
  echo "==> SKIP cvc5-verify clippy (no native env; run scripts/setup-cvc5.sh)"
fi

if [[ -n "$CRATE_DIR" && -f "${CRATE_DIR}/src/lib.rs" ]]; then
  echo "==> cargo test -p ${CRATE} --lib"
  cargo test -p "${CRATE}" --lib
else
  echo "==> cargo test -p ${CRATE}"
  cargo test -p "${CRATE}"
fi

echo "scoped pre-commit gate: OK (run scripts/pre-commit-gate.sh before session end)"