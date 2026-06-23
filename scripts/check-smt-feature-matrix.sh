#!/usr/bin/env bash
# SMT/CVC5 feature-matrix gate (PR #367 footgun prevention).
#
# Catches issues that pass default `cargo test` but fail the CI "CVC5 native
# tests" job:
#   - imports of shell-only `cvc5_ir_smtlib` outside `not(cvc5-verify)` gates
#   - compile failures under cvc5-verify / no-default-features
#   - ir_parity asserting on the stubbed CVC5 IR body encode path
#
# Usage:
#   bash scripts/check-smt-feature-matrix.sh           # lint + compile matrix
#   bash scripts/check-smt-feature-matrix.sh --lint     # cfg lint only (fast)
#   bash scripts/check-smt-feature-matrix.sh --require-cvc5  # fail if no CVC5 env
#
# When to run: any edit under crates/assura-smt/ (especially cvc5_*, ir_parity,
# ir_encode, ir_lower). Do not report an assura-smt PR green until CI job
# "CVC5 native tests" passes on the latest SHA.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

LINT_ONLY=0
REQUIRE_CVC5=0
for arg in "$@"; do
  case "$arg" in
    --lint) LINT_ONLY=1 ;;
    --require-cvc5) REQUIRE_CVC5=1 ;;
    -h|--help)
      sed -n '2,22p' "$0" | sed 's/^# \?//'
      exit 0
      ;;
  esac
done

RED=$'\033[0;31m'
GRN=$'\033[0;32m'
YLW=$'\033[0;33m'
NC=$'\033[0m'

pass() { echo "${GRN}OK${NC}  $*"; }
warn() { echo "${YLW}WARN${NC} $*"; }
fail() { echo "${RED}FAIL${NC} $*"; }

# Strip // comments and doc comments for crude code-only scans.
code_lines() {
  local f="$1"
  # remove full-line // comments and trailing // comments (simple)
  sed -E 's|//.*$||' "$f" | sed -E 's|^[[:space:]]*///.*$||' | sed -E 's|^[[:space:]]*//!.*$||'
}

# ---------------------------------------------------------------------------
# 1) Static cfg lint: cvc5_ir_smtlib is shell-only (cfg not(cvc5-verify))
# ---------------------------------------------------------------------------
lint_cvc5_ir_smtlib_imports() {
  local errs=0
  local f

  # Files that may reference the module without a per-function gate (mod/reexport
  # sites themselves carry #[cfg(not(feature = "cvc5-verify"))]).
  local allow_files=(
    "crates/assura-smt/src/cvc5_backend/mod.rs"
    "crates/assura-smt/src/lib.rs"
    "crates/assura-smt/src/cvc5_backend/cvc5_ir_smtlib.rs"
    "crates/assura-smt/src/cvc5_backend/cvc5_havoc_assume_smtlib.rs"
  )

  is_allowed_file() {
    local target="$1"
    local a
    for a in "${allow_files[@]}"; do
      [[ "$target" == "$a" ]] && return 0
    done
    return 1
  }

  while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    is_allowed_file "$f" && continue

    # Only care about non-comment references (imports / paths in code).
    if ! code_lines "$f" | grep -q 'cvc5_ir_smtlib'; then
      continue
    fi

    if ! grep -q 'cfg(not(feature = "cvc5-verify"))' "$f"; then
      fail "cvc5_ir_smtlib referenced in $f without any not(cvc5-verify) gate"
      errs=$((errs + 1))
      continue
    fi

    # Flag unconditional module-level use lines (not indented inside a gated fn).
    while IFS= read -r line; do
      lineno="${line%%:*}"
      rest="${line#*:}"
      [[ "$rest" =~ ^[[:space:]]*// ]] && continue
      if [[ "$rest" =~ ^use[[:space:]].*cvc5_ir_smtlib ]] || \
         [[ "$rest" =~ ^pub\(crate\)[[:space:]]+use[[:space:]].*cvc5_ir_smtlib ]]; then
        local window
        window="$(sed -n "$((lineno > 5 ? lineno - 5 : 1)),${lineno}p" "$f")"
        if ! echo "$window" | grep -q 'cfg(not(feature = "cvc5-verify"))'; then
          fail "$f:$lineno module-level cvc5_ir_smtlib import lacks nearby not(cvc5-verify) cfg"
          errs=$((errs + 1))
        fi
      fi
    done < <(grep -n 'cvc5_ir_smtlib' "$f" || true)
  done < <(grep -rl 'cvc5_ir_smtlib' crates/assura-smt --include='*.rs' 2>/dev/null || true)

  # Flag must exist; if stub is re-enabled, ir_parity must not call the no-op path.
  if grep -q 'fn apply_ir_body_constraints_cvc5' \
    crates/assura-smt/src/cvc5_backend/cvc5_ir_native.rs 2>/dev/null; then
    if ! grep -q 'CVC5_IR_BODY_CONSTRAINTS_IS_STUB' \
      crates/assura-smt/src/cvc5_backend/cvc5_ir_native.rs; then
      fail "apply_ir_body_constraints_cvc5 missing CVC5_IR_BODY_CONSTRAINTS_IS_STUB marker"
      errs=$((errs + 1))
    fi
  fi

  if grep -q 'CVC5_IR_BODY_CONSTRAINTS_IS_STUB: bool = true' \
    crates/assura-smt/src/cvc5_backend/cvc5_ir_native.rs 2>/dev/null; then
    if code_lines crates/assura-smt/src/ir_parity.rs | grep -qE \
      'apply_ir_body_constraints_cvc5[[:space:]]*\(|native_ir_output[[:space:]]*\('; then
      fail "ir_parity.rs calls apply_ir_body_constraints_cvc5 / native_ir_output while stub is active"
      fail "  (assert only Z3/shell; mirror ignored tests in cvc5_ir_native)"
      errs=$((errs + 1))
    fi
    warn "CVC5_IR_BODY_CONSTRAINTS_IS_STUB=true (IR body no-op); production CVC5 skips IR axioms"
  fi

  if [[ "$errs" -gt 0 ]]; then
    echo ""
    echo "Fix: gate shell imports with #[cfg(not(feature = \"cvc5-verify\"))]."
    echo "     See crates/assura-smt/src/cvc5_backend/mod.rs and AGENTS.md SMT feature matrix."
    return 1
  fi
  pass "cvc5_ir_smtlib / IR stub cfg lint"
  return 0
}

# ---------------------------------------------------------------------------
# 2) Compile matrix (default, no-default-features, optional cvc5-verify)
# ---------------------------------------------------------------------------
run_compile_matrix() {
  pass "cargo check -p assura-smt --locked --tests (default = z3-verify)"
  cargo check -p assura-smt --locked --tests

  pass "cargo check -p assura-smt --locked --tests --no-default-features"
  cargo check -p assura-smt --locked --tests --no-default-features

  local cvc5_ok=0
  if bash "$ROOT/scripts/check-cvc5-env.sh" --quiet 2>/dev/null; then
    cvc5_ok=1
  elif [[ -d /tmp/cvc5-install/cvc5-macOS-arm64-static/lib ]] && \
       [[ -d /tmp/cvc5-install/cvc5-macOS-arm64-static/include ]]; then
    export CVC5_LIB_DIR=/tmp/cvc5-install/cvc5-macOS-arm64-static/lib
    export CVC5_INCLUDE_DIR=/tmp/cvc5-install/cvc5-macOS-arm64-static/include
    cvc5_ok=1
    warn "using heuristic CVC5 paths under /tmp/cvc5-install (run setup-cvc5.sh for canonical env)"
  fi

  if [[ "$cvc5_ok" -eq 1 ]]; then
    pass "cargo check -p assura-smt --locked --tests --features cvc5-verify"
    cargo check -p assura-smt --locked --tests --features cvc5-verify
  else
    if [[ "$REQUIRE_CVC5" -eq 1 ]]; then
      fail "cvc5-verify compile required but CVC5_LIB_DIR/CVC5_INCLUDE_DIR not set"
      bash "$ROOT/scripts/check-cvc5-env.sh" --require || true
      return 1
    fi
    warn "skipping cvc5-verify compile (no CVC5 env). CI job still required."
    warn "  bash scripts/setup-cvc5.sh && export CVC5_LIB_DIR=... CVC5_INCLUDE_DIR=..."
    warn "  or: bash scripts/check-smt-feature-matrix.sh --require-cvc5"
  fi
}

echo "=== assura-smt feature matrix ==="
lint_cvc5_ir_smtlib_imports

if [[ "$LINT_ONLY" -eq 1 ]]; then
  echo "=== lint only; compile matrix skipped ==="
  exit 0
fi

run_compile_matrix
echo "=== feature matrix OK ==="
