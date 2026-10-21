#!/usr/bin/env bash
# Classify Dependabot rebase push logs without a network or a git remote.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLASSIFY="$ROOT/scripts/classify-dependabot-push-failure.sh"
TMP="$(mktemp)"
trap 'rm -f "$TMP" "$TMP.out"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

echo "PLAN: classify Dependabot rebase push failures"
echo "DO: workflows permission rejection exits 2"
printf '%s\n' \
  " ! [remote rejected] HEAD -> dependabot/github_actions/example (refusing to allow a GitHub App to create or update workflow .github/workflows/scorecard.yml without \`workflows\` permission)" \
  >"$TMP"
set +e
bash "$CLASSIFY" "$TMP" >"$TMP.out" 2>&1
rc=$?
set -e
if [ "$rc" -ne 2 ]; then
  fail "workflows permission log exited ${rc}, expected 2"
fi
grep -q 'retry with the App token' "$TMP.out" || fail "missing App token retry message"

echo "DO: stale lease rejection exits 1"
printf '%s\n' ' ! [rejected] (stale info)' >"$TMP"
set +e
bash "$CLASSIFY" "$TMP" >"$TMP.out" 2>&1
rc=$?
set -e
if [ "$rc" -ne 1 ]; then
  fail "stale lease log exited ${rc}, expected 1"
fi
grep -q 'App token' "$TMP.out" && fail "lease miss was classified as a workflow-file rejection"

echo "OK: classify-dependabot-push-failure"
