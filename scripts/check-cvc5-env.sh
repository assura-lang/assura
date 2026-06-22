#!/usr/bin/env bash
# Check whether native cvc5-verify development is configured.
# Exit 0: CVC5_LIB_DIR and CVC5_INCLUDE_DIR set and directories exist
# Exit 1: missing or invalid env (--require mode)
# Exit 2: missing env on Darwin (advisory, not an error for optional steps)

set -euo pipefail

MODE="${1:-}"

check_env() {
  [[ -n "${CVC5_LIB_DIR:-}" ]] && [[ -d "${CVC5_LIB_DIR}" ]] &&
  [[ -n "${CVC5_INCLUDE_DIR:-}" ]] && [[ -d "${CVC5_INCLUDE_DIR}" ]]
}

print_setup_hint() {
  echo "Native cvc5-verify not configured."
  echo "  bash scripts/setup-cvc5.sh"
  echo "  export CVC5_LIB_DIR=..."
  echo "  export CVC5_INCLUDE_DIR=..."
  echo ""
  echo "Or skip native steps manually when using pre-commit-gate.sh"
  echo "CI cvc5 job is still required before closing cvc5-parity issues (#304)."
}

if check_env; then
  [[ "$MODE" != "--quiet" ]] && echo "CVC5 native env OK (lib=$CVC5_LIB_DIR)"
  exit 0
fi

if [[ "$MODE" == "--quiet" ]]; then
  exit 1
fi

if [[ "$MODE" == "--require" ]]; then
  print_setup_hint
  exit 1
fi

# Default: advisory
print_setup_hint
if [[ "$(uname)" == "Darwin" ]]; then
  exit 2
fi
exit 1