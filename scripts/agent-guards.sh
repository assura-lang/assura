#!/usr/bin/env bash
# Static greps that catch common agent mistakes. Exit non-zero on violations.
# Runs in CI (clippy job) and via scripts/agent-preflight.sh.
set -euo pipefail
cd "$(dirname "$0")/.."
fail=0

warn() { echo "agent-guards WARN: $*" >&2; }
die()  { echo "agent-guards FAIL: $*" >&2; fail=1; }

# ---------------------------------------------------------------------------
# 1) Verifier::new outside allowed crates (production code only)
#    Allowed: assura-smt (implementation), assura-pipeline (canonical entry)
# ---------------------------------------------------------------------------
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
  fi
done < <(rg -n 'Verifier::new\s*\(' crates --glob '*.rs' 2>/dev/null || true)

# ---------------------------------------------------------------------------
# 2) Type::Unknown direct equality (should use is_indeterminate())
# ---------------------------------------------------------------------------
while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  file="${line%%:*}"
  case "$file" in
    *tests*|*test*.rs) continue ;;
    crates/assura-types/src/types.rs) continue ;; # definition / methods
  esac
  die "direct Type::Unknown compare (use ty.is_indeterminate()): $line"
done < <(rg -n '==\s*Type::Unknown|Type::Unknown\s*==' crates --glob '*.rs' 2>/dev/null || true)

# ---------------------------------------------------------------------------
# 3) CHECKER_PIPELINE breadth
# ---------------------------------------------------------------------------
pipeline_count=$(rg -c 'CheckerDispatch::' crates/assura-types/src/pipeline.rs 2>/dev/null | tail -1 || echo 0)
run_checks_files=$(rg -l 'pub\(crate\) fn run_.*_checks' crates/assura-types/src/checks 2>/dev/null | wc -l | tr -d ' ')
if [[ "${pipeline_count:-0}" -lt 50 ]]; then
  die "CHECKER_PIPELINE looks too small ($pipeline_count CheckerDispatch refs)"
fi
echo "agent-guards: CHECKER_PIPELINE refs=$pipeline_count run_*_checks files≈$run_checks_files"

# ---------------------------------------------------------------------------
# 4) Orphan run_*_checks: defined but never referenced from pipeline or peers
#    Internal helpers (called by another run_* in checks/, even same file) are OK.
# ---------------------------------------------------------------------------
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
  die "  fix: add CheckerDispatch::Source($fn) (or Env/Symbols/…) in crates/assura-types/src/pipeline.rs"
  die "       or call it from an existing run_*_checks entry point (see run_info_flow_checks → run_dependent_type_checks)"
done < <(rg -n 'pub\(crate\) fn run_[a-z0-9_]*_checks' \
  crates/assura-types/src/checks \
  crates/assura-types/src/generics.rs \
  2>/dev/null || true)

# ---------------------------------------------------------------------------
# 5) Open-coded known-SMT-limitation marker outside assura-smt (production only)
#    Test modules may embed the marker string to assert classification behavior.
# ---------------------------------------------------------------------------
while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  file="${line%%:*}"
  case "$file" in
    crates/assura-smt/*) continue ;;
    *tests*|*test*.rs) continue ;;
  esac
  # Skip lines inside #[cfg(test)] modules (heuristic: after mod tests in same file is hard;
  # allow assura-cli/src/check.rs if classifier is present and only tests use the string)
  if [[ "$file" == crates/assura-cli/src/check.rs ]] \
    && rg -q 'is_known_smt_limitation|assura_smt::is_known_smt_limitation' "$file" 2>/dev/null; then
    # Production check.rs delegates; only test assertions embed the marker string.
    continue
  fi
  die "open-coded SMT limitation marker outside assura-smt: $line"
  die "  fix: emit reasons via assura_smt (KNOWN_SMT_LIMITATION_MARKER), classify with is_known_smt_limitation"
done < <(rg -n 'not yet encoded in SMT' crates --glob '*.rs' 2>/dev/null || true)

# ---------------------------------------------------------------------------
# 6) Ergonomics APIs must exist (sanity)
# ---------------------------------------------------------------------------
if ! rg -q 'trait DeclVisitor' crates/assura-ast/src/ast/mod.rs; then
  die "DeclVisitor missing from assura-ast"
fi
if ! rg -q 'fn verify_typed' crates/assura-pipeline/src/lib.rs; then
  die "verify_typed missing from assura-pipeline"
fi
if ! rg -q 'fn verification_strict_succeeded' crates/assura-pipeline/src/lib.rs; then
  die "verification_strict_succeeded missing from assura-pipeline"
fi
if ! rg -q 'fn is_known_smt_limitation' crates/assura-smt/src/result.rs; then
  die "is_known_smt_limitation missing from assura-smt"
fi
if ! rg -q 'pub const KNOWN_SMT_LIMITATION_MARKER' crates/assura-smt/src/result.rs; then
  die "KNOWN_SMT_LIMITATION_MARKER missing from assura-smt"
fi
if ! rg -q 'pub fn typecheck_err' crates/assura-test-support/src/lib.rs; then
  die "typecheck_err missing from assura-test-support"
fi
if ! rg -q 'const CHECKER_PIPELINE' crates/assura-types/src/pipeline.rs; then
  die "CHECKER_PIPELINE missing from assura-types/src/pipeline.rs"
fi
if [[ ! -f crates/assura-types/src/CHECKER-LAYERS.md ]]; then
  die "CHECKER-LAYERS.md missing (agents need checks/ vs checkers/ vs domain map)"
fi
if [[ ! -x scripts/agent-new-checker.sh ]]; then
  die "scripts/agent-new-checker.sh missing or not executable"
fi
if [[ ! -x scripts/agent-new-decl.sh ]]; then
  die "scripts/agent-new-decl.sh missing or not executable"
fi
if [[ ! -f docs/error-codes-agent.md ]]; then
  die "docs/error-codes-agent.md missing (agent error-code index)"
fi

# ---------------------------------------------------------------------------
# 7) Guard v2 (HARD fail): high-signal SMT methods must appear outside
#    advanced.rs and outside tests. Prevents dead ProphecyManager/TriggerManager
#    APIs that only pass unit tests.
# ---------------------------------------------------------------------------
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
    fi
  fi
done

# ---------------------------------------------------------------------------
# 8) Soft warn: unwrap() in production lib code (not unit-test modules).
#    Excludes test modules / assert helpers so agents do not ignore noisy
#    false positives (e.g. assura-mcp inline #[cfg(test)] blocks).
# ---------------------------------------------------------------------------
unwrap_hits=$(rg -n '\.unwrap\(\)' crates --glob '*.rs' 2>/dev/null \
  | rg -v '_test\.rs|/tests/|benches/|tests_|/test_support' \
  | rg -v 'assert!|assert_eq!|assert_ne!|panic!|unreachable!' \
  | rg -v 'mod tests|cfg\(test\)' \
  | rg 'crates/[^/]+/src/' \
  | head -8 || true)
# Further filter: skip lines that live inside obvious test-only files/modules
# by requiring no "tests::" in path segment after src/ (best-effort).
if [[ -n "$unwrap_hits" ]]; then
  filtered=""
  while IFS= read -r uh; do
    [[ -z "$uh" ]] && continue
    file="${uh%%:*}"
    # Skip if file is entirely under a tests module path pattern
    case "$file" in
      *tests.rs|*_tests.rs) continue ;;
    esac
    # Sample only non-CLI/MCP assert-heavy binaries if still noisy: allow
    # assura-cli/assura-mcp only when line does not look like a test body.
    # assura-mcp/cli often have #[cfg(test)] blocks in lib.rs; skip those crates
  # for the soft warn entirely (noise > signal for agents).
  case "$file" in
    crates/assura-mcp/*|crates/assura-cli/*) continue ;;
  esac
  # Skip doc-comment / string-literal hits about .unwrap() as a pattern name
  if echo "$uh" | rg -q '///|// Pattern|contains\("\.unwrap'; then
    continue
  fi
  filtered+="$uh"$'\n'
  done <<< "$unwrap_hits"
  if [[ -n "${filtered//[$'\n']/}" ]]; then
    warn "unwrap() in production src (sample, not failing; prefer Result in libs):"
    while IFS= read -r uh; do
      [[ -n "$uh" ]] && warn "  $uh"
    done <<< "$filtered"
  fi
fi

# ---------------------------------------------------------------------------
# 9) Soft inform: open-coded match &decl.node counts (agent should prefer
#    DeclVisitor / Decl::name|clauses for new passes). Does not fail CI.
# ---------------------------------------------------------------------------
# Baselines post Priority B / do-soon codegen visitor (phases 1–2 no longer match &decl.node).
for pair in "assura-codegen:7" "assura-smt:12" "assura-resolve:2" "assura-lsp:2"; do
  crate="${pair%%:*}"
  baseline="${pair##*:}"
  # rg exits 1 when no matches; under set -e that would abort the script.
  count=$( { rg -c 'match &decl\.node|match &d\.node' "crates/$crate/src" --glob '*.rs' 2>/dev/null || true; } \
    | awk -F: '{s+=$2} END {print s+0}')
  # Normalize (avoid multi-line / empty arithmetic failures)
  count=$(printf '%s' "${count:-0}" | tr -d '[:space:]')
  if [[ -z "$count" || ! "$count" =~ ^[0-9]+$ ]]; then
    count=0
  fi
  threshold=$((baseline + 3))
  if [[ "$count" -gt "$threshold" ]]; then
    warn "crates/$crate has $count open match &decl.node sites (baseline~$baseline); prefer DeclVisitor for new passes"
  fi
done

if [[ "$fail" -ne 0 ]]; then
  echo "agent-guards: FAILED" >&2
  exit 1
fi
echo "agent-guards: OK"
