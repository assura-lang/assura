#!/usr/bin/env bash
# Read a git push log from a Dependabot rebase.
# Exit 2 when GitHub rejected a workflow-file update. The rebase job
# retries that push with the App token. Exit 1 for every other rejection.
set -euo pipefail

log=${1:?push log path}
if grep -q "without \`workflows\` permission" "$log"; then
  echo "FAIL: GITHUB_TOKEN cannot update workflow files; retry with the App token"
  exit 2
fi
echo "FAIL: push rejected"
exit 1
