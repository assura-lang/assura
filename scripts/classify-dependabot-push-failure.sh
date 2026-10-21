#!/usr/bin/env bash
# Read a git push log from a Dependabot rebase.
# Exit 2 when GitHub rejected a workflow-file update.
# Exit 1 for every other rejection (stale lease, auth, network).
set -euo pipefail

log=${1:?push log path}
if grep -q "without \`workflows\` permission" "$log"; then
  echo "FAIL: GITHUB_TOKEN cannot update workflow files (permissions.workflows: write)"
  exit 2
fi
echo "FAIL: push rejected"
exit 1
