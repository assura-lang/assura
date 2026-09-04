#!/usr/bin/env bash
# Fast checks for agent sessions. Prefer this over full `cargo test --workspace`.
#
# Usage:
#   bash scripts/preflight.sh              # types + pipeline + config + ast + test-support + smt lib + CLI bin
#   bash scripts/preflight.sh assura-types  # one crate only
#   bash scripts/preflight.sh assura-types assura-smt
#   bash scripts/preflight.sh --json        # structured JSON output
#   bash scripts/preflight.sh --json assura-types
#
# Also runs cargo deny check when cargo-deny is on PATH (skip + one-line note if missing).
#
# Related scaffolds (print-only, not run here):
#   bash scripts/new-checker.sh <name> [--category <stem>]
#   bash scripts/new-decl.sh <Variant>
set -euo pipefail
cd "$(dirname "$0")/.."

json_mode=false
if [[ "${1:-}" == "--json" ]]; then
  json_mode=true
  shift
fi

if [[ $# -gt 0 ]]; then
  crates=("$@")
else
  crates=(assura-types assura-pipeline assura-config assura-ast assura-test-support assura-smt)
fi

# ── JSON accumulation ────────────────────────────────────────────────────────
_jdata=$(mktemp)
trap 'rm -f "$_jdata"' EXIT

jstep() {
  local name="$1" status="$2" detail="${3:-}"
  printf '%s\t%s\t%s\n' "$name" "$status" "$detail" >> "$_jdata"
}

run_step() {
  local name="$1"; shift
  $json_mode || echo "== preflight: $name =="
  if "$@" 2>&1; then
    jstep "$name" "ok"
  else
    jstep "$name" "fail" "$*"
    if $json_mode; then
      # Emit JSON before exiting
      emit_json 1
    fi
    exit 1
  fi
}

emit_json() {
  local exit_code="${1:-0}"
  python3 - "$_jdata" "$exit_code" << 'PYEOF'
import json, sys
steps = []
with open(sys.argv[1]) as f:
    for line in f:
        parts = line.rstrip('\n').split('\t')
        if len(parts) >= 2:
            step = {'name': parts[0], 'status': parts[1]}
            if len(parts) >= 3 and parts[2]:
                step['detail'] = parts[2]
            steps.append(step)
ok = sum(1 for s in steps if s['status'] == 'ok')
fail = sum(1 for s in steps if s['status'] == 'fail')
print(json.dumps({
    'script': 'preflight',
    'steps': steps,
    'summary': {'ok': ok, 'fail': fail},
    'exit_code': int(sys.argv[2])
}, indent=2))
PYEOF
}

run_step "fmt check" cargo fmt --all -- --check

if $json_mode; then
  run_step "guards" bash scripts/guards.sh --json
else
  run_step "guards" bash scripts/guards.sh
fi

# Fast: publish set/order only (full cargo package is CI cargo-package job).
run_step "publish-plan" bash scripts/check-publish-plan.sh

# Match CI lint-fast: cargo deny when the binary is present. Do not install it here.
if command -v cargo-deny >/dev/null 2>&1; then
  run_step "cargo deny" cargo deny check
else
  $json_mode || echo "preflight: skip cargo deny (cargo-deny not on PATH)"
  jstep "cargo deny" "skip" "cargo-deny not on PATH"
fi

clippy_crate() {
  local crate="$1"
  if [[ "$crate" == "assura" ]]; then
    cargo clippy --bin assura --locked -- -D warnings
    return
  fi
  # Keep stderr on both attempts. --lib fails for bin-only crates.
  if ! cargo clippy -p "$crate" --lib --locked -- -D warnings; then
    cargo clippy -p "$crate" --locked -- -D warnings
  fi
}

for crate in "${crates[@]}"; do
  run_step "clippy $crate" clippy_crate "$crate"
done

# Always sanity-check the binary if not explicitly listed
if [[ " ${crates[*]} " != *" assura "* ]]; then
  run_step "clippy --bin assura" cargo clippy --bin assura --locked -- -D warnings
fi

run_step "demo check" cargo run -q --bin assura -- check demos/libwebp-huffman.assura

if $json_mode; then
  emit_json 0
else
  echo "preflight: OK"
fi
