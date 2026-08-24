#!/usr/bin/env bash
# Start required checks on a GITHUB_TOKEN-authored cleanup PR.
#
# Dual-token review (GITHUB_TOKEN authors the PR, App approves it) does
# not start CI. GitHub leaves those pull_request runs at action_required
# with zero jobs. This script uses APP_TOKEN to:
#   1. Approve any stub action_required runs on the branch
#   2. Push an empty commit as the App so synchronize starts real jobs
#
# Usage (from repo root, or any cwd; no checkout required for API path):
#   GH_REPO=assura-lang/assura BRANCH=chore/cleanup-release-notes-vX.Y.Z \
#     APP_TOKEN=... bash scripts/start-cleanup-pr-ci.sh
#   bash scripts/start-cleanup-pr-ci.sh --self-test
set -euo pipefail

self_test() {
  local payload
  payload=$(jq -n --arg msg "ci: start cleanup checks as App" \
    --arg tree "abc" --arg parent "def" \
    '{message:$msg, tree:$tree, parents:[$parent]}')
  echo "$payload" | jq -e '.parents | type == "array" and length == 1' >/dev/null
  echo "$payload" | jq -e '.message == "ci: start cleanup checks as App"' >/dev/null
  echo "OK: start-cleanup-pr-ci self-test"
}

if [[ "${1:-}" == "--self-test" ]]; then
  self_test
  exit 0
fi

: "${GH_REPO:?set GH_REPO}"
: "${BRANCH:?set BRANCH}"
if [[ -z "${APP_TOKEN:-}" ]]; then
  echo "FAIL: APP_TOKEN unset; cannot start cleanup CI as App"
  exit 1
fi

export GH_TOKEN="$APP_TOKEN"

echo "PLAN: start cleanup CI as App repo=${GH_REPO} branch=${BRANCH}"

approve_stub_runs() {
  local ids id
  ids=$(gh run list --repo "${GH_REPO}" --branch "${BRANCH}" --limit 20 \
    --json databaseId,conclusion,status \
    --jq '.[] | select(.conclusion == "action_required") | .databaseId' \
    || true)
  if [[ -z "${ids}" ]]; then
    echo "OK: no action_required stub runs"
    return 0
  fi
  while read -r id; do
    [[ -z "${id}" ]] && continue
    echo "DO: approve stub run ${id}"
    if gh api -X POST "repos/${GH_REPO}/actions/runs/${id}/approve" >/dev/null; then
      echo "OK: approved ${id}"
    else
      echo "FAIL: approve ${id} (continuing)"
    fi
  done <<<"${ids}"
}

empty_commit_as_app() {
  local head_sha tree msg new_sha
  head_sha=$(gh api "repos/${GH_REPO}/git/ref/heads/${BRANCH}" --jq '.object.sha')
  tree=$(gh api "repos/${GH_REPO}/git/commits/${head_sha}" --jq '.tree.sha')
  msg=$(gh api "repos/${GH_REPO}/git/commits/${head_sha}" --jq '.message')
  if [[ "${msg}" == "ci: start cleanup checks as App" ]]; then
    echo "OK: App empty commit already present sha=${head_sha}"
    echo "DONE: actor=app branch=${BRANCH} sha=${head_sha}"
    return 0
  fi
  echo "DO: empty commit as App on ${BRANCH}"
  new_sha=$(
    jq -n --arg msg "ci: start cleanup checks as App" \
      --arg tree "${tree}" --arg parent "${head_sha}" \
      '{message:$msg, tree:$tree, parents:[$parent]}' \
      | gh api "repos/${GH_REPO}/git/commits" --input - --jq '.sha'
  )
  gh api --method PATCH "repos/${GH_REPO}/git/refs/heads/${BRANCH}" \
    -f sha="${new_sha}" >/dev/null
  echo "DONE: actor=app branch=${BRANCH} sha=${new_sha}"
}

approve_stub_runs
empty_commit_as_app
