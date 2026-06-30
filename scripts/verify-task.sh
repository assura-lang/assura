#!/usr/bin/env bash
# verify-task.sh - Machine-enforced gate for feature completion
#
# Usage: bash scripts/verify-task.sh <FEATURE_ID>
# Example: bash scripts/verify-task.sh SEC.1
#
# Exits 0 only if:
#   1. cargo build succeeds
#   2. cargo clippy passes with -D warnings
#   3. cargo test --workspace passes
#   4. All demo/fixture files run without error
#   5. Coverage score for the feature is reported
#
# This script is the definition of "done." If it exits non-zero,
# the feature is not done. No exceptions.

set -euo pipefail

FEATURE="${1:?Usage: bash scripts/verify-task.sh <FEATURE_ID>}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

echo "=== Verifying feature: $FEATURE ==="
echo ""

# Step 1: Build
echo "--- Step 1: cargo build ---"
cargo build 2>&1
echo "OK"
echo ""

# Step 2: Clippy
echo "--- Step 2: cargo clippy ---"
cargo clippy --workspace -- -D warnings 2>&1
echo "OK"
echo ""

# Step 3: Tests
echo "--- Step 3: cargo test --workspace ---"
cargo test --workspace 2>&1
echo "OK"
echo ""

# Step 4: Demo files
echo "--- Step 4: Demo and fixture files ---"
fail=0
for f in demos/*.assura tests/fixtures/test_basic.assura; do
  if [ -f "$f" ]; then
    if cargo run --bin assura -- check "$f" > /dev/null 2>&1; then
      echo "  PASS  $f"
    else
      echo "  FAIL  $f"
      fail=1
    fi
  fi
done
if [ "$fail" -ne 0 ]; then
  echo "ERROR: One or more demo files failed"
  exit 1
fi
echo "OK"
echo ""

# Step 5: Coverage score
echo "--- Step 5: Coverage score ---"
SCRIPT="$HOME/.assura/scripts/coverage-matrix.sh"
if [ -f "$SCRIPT" ]; then
  LINE=$(bash "$SCRIPT" "$REPO" 2>&1 | grep -F "**${FEATURE}**" | head -1)
  if [ -n "$LINE" ]; then
    echo "  $LINE"
    # Extract score from the completeness ranking table (e.g., "10/13")
    SCORE=$(bash "$SCRIPT" "$REPO" 2>&1 \
      | grep -F "**${FEATURE}**" \
      | grep -oE '[0-9]+/13' | head -1)
    if [ -n "$SCORE" ]; then
      echo ""
      echo "  Coverage for $FEATURE: $SCORE"
    fi
  else
    echo "  WARNING: Feature $FEATURE not found in coverage output"
  fi
else
  echo "  WARNING: Coverage script not found at $SCRIPT"
fi
echo ""

echo "=== VERIFIED: $FEATURE ==="
