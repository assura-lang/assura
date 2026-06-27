#!/usr/bin/env bash
# benchmark-llm-verify.sh -- Measure LLM verification success rate
#
# For each contract in tests/fixtures/llm_bench/:
#   1. Generate an IR prompt via `assura ir-prompt`
#   2. Send it to an LLM (or use pre-generated IR files)
#   3. Validate the IR with `assura ir --verify`
#   4. Record pass/fail
#
# Usage:
#   # Dry run (just check contracts parse, list prompts)
#   bash scripts/benchmark-llm-verify.sh --dry-run
#
#   # Run with pre-generated IR files in tests/fixtures/llm_bench/ir/
#   bash scripts/benchmark-llm-verify.sh --ir-dir tests/fixtures/llm_bench/ir
#
#   # Run with live LLM (requires OPENAI_API_KEY or ANTHROPIC_API_KEY)
#   bash scripts/benchmark-llm-verify.sh --live
#
# Output: summary table with pass/fail/error per contract.

set -euo pipefail

BENCH_DIR="tests/fixtures/llm_bench"
IR_DIR=""
DRY_RUN=false
LIVE=false
ASSURA="cargo run --bin assura --"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run)    DRY_RUN=true; shift ;;
        --ir-dir)     IR_DIR="$2"; shift 2 ;;
        --live)       LIVE=true; shift ;;
        *)            echo "Unknown arg: $1"; exit 1 ;;
    esac
done

# Collect benchmark files
files=( "$BENCH_DIR"/*.assura )
total=${#files[@]}
pass=0
fail=0
error=0
skip=0

echo "=== Assura LLM Verification Benchmark ==="
echo "Contracts: $total"
echo ""

printf "%-35s %-12s %-s\n" "Contract" "Status" "Detail"
printf "%-35s %-12s %-s\n" "---" "---" "---"

for f in "${files[@]}"; do
    name=$(basename "$f" .assura)

    # Phase 1: verify contract parses
    if ! $ASSURA check "$f" >/dev/null 2>&1; then
        # Some errors are expected (counterexamples on unconstrained result)
        # Only count A01 parse errors as real errors
        parse_errors=$($ASSURA check "$f" 2>&1 | grep -c 'A01' || true)
        if [[ "$parse_errors" -gt 0 ]]; then
            printf "%-35s %-12s %-s\n" "$name" "ERROR" "parse failure"
            error=$((error + 1))
            continue
        fi
    fi

    if $DRY_RUN; then
        printf "%-35s %-12s %-s\n" "$name" "SKIP" "dry run"
        skip=$((skip + 1))
        continue
    fi

    # Phase 2: find IR file
    ir_file=""
    if [[ -n "$IR_DIR" ]] && [[ -f "$IR_DIR/$name.ir" ]]; then
        ir_file="$IR_DIR/$name.ir"
    elif [[ -n "$IR_DIR" ]]; then
        # Try to match contract name inside the file
        contract_name=$($ASSURA ir-prompt "$f" --list 2>/dev/null | head -1 || echo "")
        if [[ -n "$contract_name" ]] && [[ -f "$IR_DIR/$contract_name.ir" ]]; then
            ir_file="$IR_DIR/$contract_name.ir"
        fi
    fi

    if [[ -z "$ir_file" ]]; then
        if $LIVE; then
            # Generate prompt and send to LLM (placeholder for future integration)
            printf "%-35s %-12s %-s\n" "$name" "SKIP" "live LLM not yet implemented"
            skip=$((skip + 1))
        else
            printf "%-35s %-12s %-s\n" "$name" "SKIP" "no IR file"
            skip=$((skip + 1))
        fi
        continue
    fi

    # Phase 3: verify IR against contract
    verify_output=$($ASSURA ir "$ir_file" --contract "$f" --verify 2>&1 || true)
    if echo "$verify_output" | grep -qi 'verified'; then
        if echo "$verify_output" | grep -qi 'counterexample'; then
            printf "%-35s %-12s %-s\n" "$name" "PARTIAL" "some clauses verified, some failed"
            fail=$((fail + 1))
        else
            printf "%-35s %-12s %-s\n" "$name" "PASS" "all clauses verified"
            pass=$((pass + 1))
        fi
    else
        detail=$(echo "$verify_output" | grep -i 'error\|fail\|counter' | head -1 || echo "unknown")
        printf "%-35s %-12s %-s\n" "$name" "FAIL" "$detail"
        fail=$((fail + 1))
    fi
done

echo ""
echo "=== Summary ==="
echo "Total:   $total"
echo "Pass:    $pass"
echo "Fail:    $fail"
echo "Error:   $error"
echo "Skip:    $skip"
if [[ $((pass + fail)) -gt 0 ]]; then
    rate=$(python3 -c "print(f'{$pass / ($pass + $fail) * 100:.1f}%')")
    echo "Rate:    $rate ($pass / $((pass + fail)) verified)"
fi
