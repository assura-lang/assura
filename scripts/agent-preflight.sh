#!/usr/bin/env bash
# Fast checks for agent sessions. Prefer this over full `cargo test --workspace`.
#
# Usage:
#   bash scripts/agent-preflight.sh              # types + pipeline + smt lib + CLI bin
#   bash scripts/agent-preflight.sh assura-types  # one crate only
#   bash scripts/agent-preflight.sh assura-types assura-smt
#   bash scripts/agent-preflight.sh --json        # structured JSON output
#   bash scripts/agent-preflight.sh --json assura-types
#
# Related scaffolds (print-only, not run here):
#   bash scripts/agent-new-checker.sh <name> [--category <stem>]
#   bash scripts/agent-new-decl.sh <Variant>
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
  crates=(assura-types assura-pipeline assura-config assura-ast assura-test-support)
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
  $json_mode || echo "== agent-preflight: $name =="
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
    'script': 'agent-preflight',
    'steps': steps,
    'summary': {'ok': ok, 'fail': fail},
    'exit_code': int(sys.argv[2])
}, indent=2))
PYEOF
}

run_step "fmt check" cargo fmt --all -- --check

if $json_mode; then
  run_step "agent guards" bash scripts/agent-guards.sh --json
else
  run_step "agent guards" bash scripts/agent-guards.sh
fi

for crate in "${crates[@]}"; do
  if [[ "$crate" == "assura" ]]; then
    run_step "clippy $crate" cargo clippy --bin assura --locked -- -D warnings
  else
    run_step "clippy $crate" bash -c "cargo clippy -p '$crate' --lib --locked -- -D warnings 2>/dev/null || cargo clippy -p '$crate' --locked -- -D warnings"
  fi
done

# Always sanity-check the binary if not explicitly listed
if [[ " ${crates[*]} " != *" assura "* ]]; then
  run_step "clippy --bin assura" cargo clippy --bin assura --locked -- -D warnings
fi

run_step "demo check" cargo run -q --bin assura -- check demos/libwebp-huffman.assura

if $json_mode; then
  emit_json 0
else
  echo "agent-preflight: OK"
fi
