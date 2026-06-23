#!/usr/bin/env bash
# Static greps that catch common agent mistakes. Exit non-zero on violations.
set -euo pipefail
cd "$(dirname "$0")/.."
fail=0

warn() { echo "agent-guards WARN: $*" >&2; }
die()  { echo "agent-guards FAIL: $*" >&2; fail=1; }

# 1) Verifier::new outside allowed crates (production code only; tests/benches OK)
#    Allowed: assura-smt (implementation), assura-pipeline (canonical entry)
while IFS= read -r line; do
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

# 2) Type::Unknown direct equality (should use is_indeterminate())
while IFS= read -r line; do
  file="${line%%:*}"
  case "$file" in
    *tests*|*test*.rs) continue ;;
    crates/assura-types/src/types.rs) continue ;; # definition / methods
  esac
  die "direct Type::Unknown compare (use ty.is_indeterminate()): $line"
done < <(rg -n '==\s*Type::Unknown|Type::Unknown\s*==' crates --glob '*.rs' 2>/dev/null || true)

# 3) Reminder: new run_*_checks should appear in CHECKER_PIPELINE (soft: count only)
pipeline_count=$(rg -c 'CheckerDispatch::' crates/assura-types/src/pipeline.rs 2>/dev/null | tail -1 || echo 0)
run_checks=$(rg -l 'pub\(crate\) fn run_.*_checks' crates/assura-types/src/checks 2>/dev/null | wc -l | tr -d ' ')
if [[ "${pipeline_count:-0}" -lt 50 ]]; then
  die "CHECKER_PIPELINE looks too small ($pipeline_count CheckerDispatch refs)"
fi
echo "agent-guards: CHECKER_PIPELINE refs=$pipeline_count run_*_checks files≈$run_checks"

# 4) DeclVisitor / accessors exist (sanity)
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
if ! rg -q 'pub fn typecheck_err' crates/assura-test-support/src/lib.rs; then
  die "typecheck_err missing from assura-test-support"
fi

if [[ "$fail" -ne 0 ]]; then
  echo "agent-guards: FAILED" >&2
  exit 1
fi
echo "agent-guards: OK"
