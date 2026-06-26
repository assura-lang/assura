#!/usr/bin/env bash
# Static greps that catch common agent mistakes. Exit non-zero on violations.
# Runs in CI (clippy job) and via scripts/agent-preflight.sh.
#
# Usage:
#   bash scripts/agent-guards.sh          # human-readable (default)
#   bash scripts/agent-guards.sh --json   # structured JSON output
set -euo pipefail
cd "$(dirname "$0")/.."

json_mode=false
[[ "${1:-}" == "--json" ]] && json_mode=true

fail=0

# ── JSON accumulation ────────────────────────────────────────────────────────
_jdata=$(mktemp)
trap 'rm -f "$_jdata"' EXIT

# Append a section result (tab-separated: S id name status message)
jsec()  { printf 'S\t%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" >> "$_jdata"; }
# Append a finding for section id (tab-separated: F id file detail)
jfind() { printf 'F\t%s\t%s\t%s\n' "$1" "$2" "$3" >> "$_jdata"; }

# ── Human output helpers (suppressed in JSON mode) ───────────────────────────
warn() { $json_mode || echo "agent-guards WARN: $*" >&2; }
die()  { $json_mode || echo "agent-guards FAIL: $*" >&2; fail=1; }

# ---------------------------------------------------------------------------
# 1) Verifier::new outside allowed crates (production code only)
#    Allowed: assura-smt (implementation), assura-pipeline (canonical entry)
# ---------------------------------------------------------------------------
s_fail=0
while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  file="${line%%:*}"
  case "$file" in
    crates/assura-smt/*|crates/assura-pipeline/*) continue ;;
    *tests*|*benches*|*test*.rs) continue ;;
    crates/assura-cli/src/diff/tests.rs) continue ;;
  esac
  if [[ "$file" == crates/*/src/* ]]; then
    die "Verifier::new in production code outside smt/pipeline: $line"
    die "  fix: use assura_pipeline::verify_typed(&typed, path, &config)"
    jfind 1 "$file" "Verifier::new outside smt/pipeline"
    s_fail=1
  fi
done < <(rg -n 'Verifier::new\s*\(' crates --glob '*.rs' 2>/dev/null || true)
if [[ $s_fail -eq 1 ]]; then
  jsec 1 "Verifier::new outside smt/pipeline" "fail" "Verifier::new found in production code outside allowed crates"
else
  jsec 1 "Verifier::new outside smt/pipeline" "ok" "no violations"
fi

# ---------------------------------------------------------------------------
# 2) Type::Unknown direct equality (should use is_indeterminate())
# ---------------------------------------------------------------------------
s_fail=0
while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  file="${line%%:*}"
  case "$file" in
    *tests*|*test*.rs) continue ;;
    crates/assura-types/src/types.rs) continue ;; # definition / methods
  esac
  die "direct Type::Unknown compare (use ty.is_indeterminate()): $line"
  jfind 2 "$file" "direct Type::Unknown == compare"
  s_fail=1
done < <(rg -n '==\s*Type::Unknown|Type::Unknown\s*==' crates --glob '*.rs' 2>/dev/null || true)
if [[ $s_fail -eq 1 ]]; then
  jsec 2 "Type::Unknown direct equality" "fail" "use ty.is_indeterminate() instead of == Type::Unknown"
else
  jsec 2 "Type::Unknown direct equality" "ok" "no violations"
fi

# ---------------------------------------------------------------------------
# 3) CHECKER_PIPELINE breadth
# ---------------------------------------------------------------------------
pipeline_count=$(rg -c 'CheckerDispatch::' crates/assura-types/src/pipeline.rs 2>/dev/null | tail -1 || echo 0)
run_checks_files=$(rg -l 'pub\(crate\) fn run_.*_checks' crates/assura-types/src/checks 2>/dev/null | wc -l | tr -d ' ')
if [[ "${pipeline_count:-0}" -lt 50 ]]; then
  die "CHECKER_PIPELINE looks too small ($pipeline_count CheckerDispatch refs)"
  jsec 3 "CHECKER_PIPELINE breadth" "fail" "only $pipeline_count CheckerDispatch refs (minimum 50)"
else
  $json_mode || echo "agent-guards: CHECKER_PIPELINE refs=$pipeline_count run_*_checks files=$run_checks_files"
  jsec 3 "CHECKER_PIPELINE breadth" "ok" "refs=$pipeline_count, run_*_checks files=$run_checks_files"
fi

# ---------------------------------------------------------------------------
# 4) Orphan run_*_checks: defined but never referenced from pipeline or peers
#    Internal helpers (called by another run_* in checks/, even same file) are OK.
# ---------------------------------------------------------------------------
s_fail=0
while IFS= read -r def_line; do
  [[ -z "$def_line" ]] && continue
  fn=$(echo "$def_line" | sed -n 's/.*fn \(run_[a-z0-9_]*_checks\).*/\1/p')
  [[ -z "$fn" ]] && continue
  def_file="${def_line%%:*}"
  def_lineno="${def_line#*:}"
  def_lineno="${def_lineno%%:*}"

  if rg -q "$fn" crates/assura-types/src/pipeline.rs 2>/dev/null; then
    continue
  fi

  # Any reference that is not exactly the definition line?
  peer_hits=$(rg -n "$fn" crates/assura-types/src/checks crates/assura-types/src/generics.rs 2>/dev/null \
    | grep -v "^${def_file}:${def_lineno}:" \
    || true)
  if [[ -n "$peer_hits" ]]; then
    continue
  fi

  die "orphan run_*_checks (not in CHECKER_PIPELINE and not called by peers): $fn"
  die "  defined at: $def_line"
  die "  fix: add CheckerDispatch::Source($fn) (or Env/Symbols/...) in crates/assura-types/src/pipeline.rs"
  die "       or call it from an existing run_*_checks entry point (see run_info_flow_checks -> run_dependent_type_checks)"
  jfind 4 "$def_file" "orphan $fn"
  s_fail=1
done < <(rg -n 'pub\(crate\) fn run_[a-z0-9_]*_checks' \
  crates/assura-types/src/checks \
  crates/assura-types/src/generics.rs \
  2>/dev/null || true)
if [[ $s_fail -eq 1 ]]; then
  jsec 4 "orphan run_*_checks" "fail" "run_*_checks functions not in CHECKER_PIPELINE and not called by peers"
else
  jsec 4 "orphan run_*_checks" "ok" "no violations"
fi

# ---------------------------------------------------------------------------
# 5) Open-coded known-SMT-limitation marker outside assura-smt (production only)
#    Test modules may embed the marker string to assert classification behavior.
# ---------------------------------------------------------------------------
s_fail=0
while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  file="${line%%:*}"
  case "$file" in
    crates/assura-smt/*) continue ;;
    *tests*|*test*.rs) continue ;;
  esac
  if [[ "$file" == crates/assura-cli/src/check.rs ]] \
    && rg -q 'is_known_smt_limitation|assura_smt::is_known_smt_limitation' "$file" 2>/dev/null; then
    continue
  fi
  die "open-coded SMT limitation marker outside assura-smt: $line"
  die "  fix: emit reasons via assura_smt (KNOWN_SMT_LIMITATION_MARKER), classify with is_known_smt_limitation"
  jfind 5 "$file" "open-coded SMT limitation marker"
  s_fail=1
done < <(rg -n 'not yet encoded in SMT' crates --glob '*.rs' 2>/dev/null || true)
if [[ $s_fail -eq 1 ]]; then
  jsec 5 "open-coded SMT limitation marker" "fail" "marker string found outside assura-smt"
else
  jsec 5 "open-coded SMT limitation marker" "ok" "no violations"
fi

# ---------------------------------------------------------------------------
# 6) Ergonomics APIs must exist (sanity)
# ---------------------------------------------------------------------------
s_fail=0
check_api() {
  local file="$1" pattern="$2" label="$3"
  if ! rg -q "$pattern" "$file" 2>/dev/null; then
    die "$label missing from $file"
    jfind 6 "$file" "$label missing"
    s_fail=1
  fi
}

check_api crates/assura-ast/src/ast/mod.rs 'trait DeclVisitor' "DeclVisitor"
check_api crates/assura-pipeline/src/lib.rs 'fn verify_typed' "verify_typed"
check_api crates/assura-pipeline/src/lib.rs 'fn verification_strict_succeeded' "verification_strict_succeeded"
check_api crates/assura-smt/src/result.rs 'fn is_known_smt_limitation' "is_known_smt_limitation"
check_api crates/assura-smt/src/result.rs 'pub const KNOWN_SMT_LIMITATION_MARKER' "KNOWN_SMT_LIMITATION_MARKER"
check_api crates/assura-test-support/src/lib.rs 'pub fn typecheck_err' "typecheck_err"
check_api crates/assura-types/src/pipeline.rs 'const CHECKER_PIPELINE' "CHECKER_PIPELINE"

if [[ ! -f crates/assura-types/src/CHECKER-LAYERS.md ]]; then
  die "CHECKER-LAYERS.md missing (agents need checks/ vs checkers/ vs domain map)"
  jfind 6 "crates/assura-types/src/CHECKER-LAYERS.md" "missing"
  s_fail=1
fi
for script_path in scripts/agent-new-checker.sh scripts/agent-new-decl.sh; do
  if [[ ! -x "$script_path" ]]; then
    die "$script_path missing or not executable"
    jfind 6 "$script_path" "missing or not executable"
    s_fail=1
  fi
done
if [[ ! -f docs/error-codes-agent.md ]]; then
  die "docs/error-codes-agent.md missing (agent error-code index)"
  jfind 6 "docs/error-codes-agent.md" "missing"
  s_fail=1
fi
if [[ $s_fail -eq 1 ]]; then
  jsec 6 "ergonomics APIs" "fail" "required APIs or files missing"
else
  jsec 6 "ergonomics APIs" "ok" "all required APIs and files present"
fi

# ---------------------------------------------------------------------------
# 7) Guard v2 (HARD fail): high-signal SMT methods must appear outside
#    advanced.rs and outside tests.
# ---------------------------------------------------------------------------
s_fail=0
smt_wire_hard=(
  "ProphecyManager::check_all_resolved|check_all_resolved"
  "ProphecyManager::check_unconstrained|check_unconstrained"
  "TriggerManager::validate_trigger|validate_trigger"
  "validate_quantifier_bounds|validate_quantifier_bounds"
  "dispatch_decrease_checks|dispatch_decrease_checks"
)
for item in "${smt_wire_hard[@]}"; do
  label="${item%%|*}"
  meth="${item##*|}"
  hits=$(rg -n "$meth" crates/assura-smt/src --glob '*.rs' 2>/dev/null \
    | rg -v '_test|tests_|/tests/' \
    | rg -v 'crates/assura-smt/src/advanced.rs' \
    || true)
  if [[ -z "$hits" ]]; then
    def_only=$(rg -n "fn $meth" crates/assura-smt/src --glob '*.rs' 2>/dev/null || true)
    if [[ -n "$def_only" ]]; then
      die "SMT method unwired (only def / tests): $label ($meth)"
      die "  fix: call from assura-smt entry/verify/encoder (or remove dead API)"
      die "  see agent-guards section 7 / AGENTS decision tree"
      jfind 7 "crates/assura-smt" "unwired SMT method: $label"
      s_fail=1
    fi
  fi
done
if [[ $s_fail -eq 1 ]]; then
  jsec 7 "SMT manager wiring" "fail" "SMT methods defined but not wired into entry points"
else
  jsec 7 "SMT manager wiring" "ok" "all SMT manager methods properly wired"
fi

# ---------------------------------------------------------------------------
# 8) Soft warn: unwrap() in production lib code (not unit-test modules).
# ---------------------------------------------------------------------------
s_warn=0
unwrap_hits=$(rg -n '\.unwrap\(\)' crates --glob '*.rs' 2>/dev/null \
  | rg -v '_test\.rs|/tests/|benches/|tests_|/test_support' \
  | rg -v 'assert!|assert_eq!|assert_ne!|panic!|unreachable!' \
  | rg -v 'mod tests|cfg\(test\)' \
  | rg 'crates/[^/]+/src/' \
  | head -8 || true)
if [[ -n "$unwrap_hits" ]]; then
  filtered=""
  while IFS= read -r uh; do
    [[ -z "$uh" ]] && continue
    file="${uh%%:*}"
    case "$file" in
      *tests.rs|*_tests.rs) continue ;;
      crates/assura-mcp/*|crates/assura-cli/*) continue ;;
    esac
    if echo "$uh" | rg -q '///|// Pattern|contains\("\.unwrap'; then
      continue
    fi
    filtered+="$uh"$'\n'
    jfind 8 "$file" "unwrap() in production code"
    s_warn=1
  done <<< "$unwrap_hits"
  if [[ -n "${filtered//[$'\n']/}" ]]; then
    warn "unwrap() in production src (sample, not failing; prefer Result in libs):"
    while IFS= read -r uh; do
      [[ -n "$uh" ]] && warn "  $uh"
    done <<< "$filtered"
  fi
fi
if [[ $s_warn -eq 1 ]]; then
  jsec 8 "unwrap() in production" "warn" "unwrap() found in production library code (not failing)"
else
  jsec 8 "unwrap() in production" "ok" "no unwrap() in production library code"
fi

# ---------------------------------------------------------------------------
# 9) Soft inform: open-coded match &decl.node counts.
# ---------------------------------------------------------------------------
s_warn=0
for pair in "assura-codegen:7" "assura-smt:12" "assura-resolve:2" "assura-lsp:2"; do
  crate="${pair%%:*}"
  baseline="${pair##*:}"
  count=$( { rg -c 'match &decl\.node|match &d\.node' "crates/$crate/src" --glob '*.rs' 2>/dev/null || true; } \
    | awk -F: '{s+=$2} END {print s+0}')
  count=$(printf '%s' "${count:-0}" | tr -d '[:space:]')
  if [[ -z "$count" || ! "$count" =~ ^[0-9]+$ ]]; then
    count=0
  fi
  threshold=$((baseline + 3))
  if [[ "$count" -gt "$threshold" ]]; then
    warn "crates/$crate has $count open match &decl.node sites (baseline~$baseline); prefer DeclVisitor for new passes"
    jfind 9 "crates/$crate" "match &decl.node count=$count exceeds baseline=$baseline"
    s_warn=1
  fi
done
if [[ $s_warn -eq 1 ]]; then
  jsec 9 "match decl.node counts" "warn" "some crates exceed match &decl.node baseline"
else
  jsec 9 "match decl.node counts" "ok" "all crates within baseline"
fi

# ── Final output ─────────────────────────────────────────────────────────────
if $json_mode; then
  python3 - "$_jdata" << 'PYEOF'
import json, sys
sections_map = {}
section_order = []
findings_map = {}

with open(sys.argv[1]) as f:
    for raw in f:
        line = raw.rstrip('\n')
        parts = line.split('\t')
        if parts[0] == 'S' and len(parts) >= 5:
            sid = int(parts[1])
            sections_map[sid] = {
                'id': sid, 'name': parts[2],
                'status': parts[3], 'message': parts[4]
            }
            section_order.append(sid)
        elif parts[0] == 'F' and len(parts) >= 4:
            sid = int(parts[1])
            findings_map.setdefault(sid, []).append(
                {'file': parts[2], 'detail': parts[3]}
            )

result = []
for sid in section_order:
    sec = sections_map[sid]
    sec['findings'] = findings_map.get(sid, [])
    result.append(sec)

ok = sum(1 for s in result if s['status'] == 'ok')
w = sum(1 for s in result if s['status'] == 'warn')
f = sum(1 for s in result if s['status'] == 'fail')

print(json.dumps({
    'script': 'agent-guards',
    'sections': result,
    'summary': {'ok': ok, 'warn': w, 'fail': f},
    'exit_code': 1 if f > 0 else 0
}, indent=2))
PYEOF
fi

if [[ "$fail" -ne 0 ]]; then
  $json_mode || echo "agent-guards: FAILED" >&2
  exit 1
fi
$json_mode || echo "agent-guards: OK"
