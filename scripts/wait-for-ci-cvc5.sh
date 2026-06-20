#!/usr/bin/env bash
# Wait for CI cvc5 job to complete on a given commit.
# Usage: scripts/wait-for-ci-cvc5.sh <commit-sha>
set -euo pipefail

COMMIT="${1:-HEAD}"
REPO="${REPO:-assura-lang/assura}"
TIMEOUT="${TIMEOUT:-600}"

SHA=$(git rev-parse "$COMMIT" 2>/dev/null || echo "$COMMIT")
SHORT=$(echo "$SHA" | cut -c1-7)

echo "Waiting for CI cvc5 job on $SHORT (timeout: ${TIMEOUT}s)..."

# Find the workflow run for this commit
elapsed=0
while true; do
  RUN_ID=$(gh run list --repo "$REPO" --workflow ci.yml --commit "$SHA" \
    --json databaseId,status --jq '.[0].databaseId // empty' 2>/dev/null || true)

  if [[ -n "$RUN_ID" ]]; then
    break
  fi

  if (( elapsed >= TIMEOUT )); then
    echo "ERROR: No CI run found for $SHORT after ${TIMEOUT}s"
    exit 1
  fi

  sleep 15
  elapsed=$((elapsed + 15))
done

echo "Found run $RUN_ID, watching..."

# Watch the run until completion
gh run watch "$RUN_ID" --repo "$REPO" --exit-status 2>/dev/null && {
  # Check cvc5 job specifically
  CVC5_RESULT=$(gh run view "$RUN_ID" --repo "$REPO" --json jobs \
    --jq '.jobs[] | select(.name | test("cvc5"; "i")) | .conclusion' 2>/dev/null || echo "unknown")

  if [[ "$CVC5_RESULT" == "success" ]]; then
    echo "cvc5 job: SUCCESS"
    echo "Run: https://github.com/$REPO/actions/runs/$RUN_ID"
    exit 0
  else
    echo "cvc5 job: $CVC5_RESULT"
    echo "Run: https://github.com/$REPO/actions/runs/$RUN_ID"
    exit 1
  fi
} || {
  echo "CI run $RUN_ID failed or timed out"
  echo "Run: https://github.com/$REPO/actions/runs/$RUN_ID"
  exit 1
}