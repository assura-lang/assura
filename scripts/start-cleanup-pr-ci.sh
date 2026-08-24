#!/usr/bin/env bash
# Start required checks on a GITHUB_TOKEN-authored cleanup PR.
#
# Dual-token review (GITHUB_TOKEN authors the PR, App approves it) does
# not start CI. GitHub leaves those pull_request runs at action_required
# with zero jobs. This script uses APP_TOKEN to approve those stub runs.
#
# Do not push an empty commit as the App. main-branch-protection has
# require_last_push_approval and dismiss_stale_reviews_on_push. If the
# App is the last pusher, its review is dismissed and it cannot approve
# itself (#1520, same class as #1499).
#
# Usage (from repo root, or any cwd; no checkout required for API path):
#   GH_REPO=assura-lang/assura BRANCH=chore/cleanup-release-notes-vX.Y.Z \
#     APP_TOKEN=... bash scripts/start-cleanup-pr-ci.sh
#   bash scripts/start-cleanup-pr-ci.sh --self-test
set -euo pipefail

self_test() {
  if grep -qE '^empty_commit_as_app\(\)' "$0"; then
    echo "FAIL: empty_commit_as_app function still present"
    exit 1
  fi
  echo "OK: start-cleanup-pr-ci self-test"
}

if [[ "${1:-}" == "--self-test" ]]; then
  self_test
  exit 0
fi

: "${GH_REPO:?set GH_REPO}"
: "${BRANCH:?set BRANCH}"
if [[ -z "${APP_TOKEN:-}" ]]; then
  echo "FAIL: APP_TOKEN unset; cannot approve cleanup CI stubs as App"
  exit 1
fi

export GH_TOKEN="$APP_TOKEN"

echo "PLAN: approve cleanup stub runs as App repo=${GH_REPO} branch=${BRANCH}"

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

approve_stub_runs
echo "DONE: actor=app branch=${BRANCH} (stub approve only)"
