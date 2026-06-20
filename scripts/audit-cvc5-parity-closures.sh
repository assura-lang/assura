#!/usr/bin/env bash
# Audit closed cvc5-parity issues by re-running their acceptance test commands.
# Usage: scripts/audit-cvc5-parity-closures.sh [--dry-run] [--since DATE] [--native]
set -euo pipefail

REPO="${REPO:-assura-lang/assura}"
DRY_RUN=0
SINCE=""
NATIVE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) DRY_RUN=1; shift ;;
    --since) SINCE="$2"; shift 2 ;;
    --native) NATIVE=1; shift ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

echo "# CVC5 Parity Closure Audit"
echo ""

QUERY='.[] | {number, title, body}'
if [[ -n "$SINCE" ]]; then
  # gh does not support --since for issues; filter client-side
  ISSUES=$(gh issue list --repo "$REPO" --state closed --label cvc5-parity \
    --json number,title,body,closedAt --limit 100 \
    --jq ".[] | select(.closedAt >= \"$SINCE\") | {number, title, body}")
else
  ISSUES=$(gh issue list --repo "$REPO" --state closed --label cvc5-parity \
    --json number,title,body --limit 100 --jq "$QUERY")
fi

PASS=0
FAIL=0
SKIP=0

echo "$ISSUES" | jq -r '.number' 2>/dev/null | while read -r NUM; do
  TITLE=$(echo "$ISSUES" | jq -r "select(.number == $NUM) | .title")
  BODY=$(echo "$ISSUES" | jq -r "select(.number == $NUM) | .body")
  echo "## #$NUM: $TITLE"

  # Extract cargo test commands from issue body
  CMDS=$(echo "$BODY" | grep -oE 'cargo test[^`\n]*' 2>/dev/null || true)
  if [[ -z "$CMDS" ]]; then
    echo "  No cargo test commands found in body"
    echo ""
    continue
  fi

  echo "$CMDS" | while read -r CMD; do
    IS_NATIVE=0
    if echo "$CMD" | grep -q "cvc5-verify"; then
      IS_NATIVE=1
    fi

    if [[ $IS_NATIVE -eq 1 && $NATIVE -eq 0 ]]; then
      echo "  SKIP (native): $CMD"
      continue
    fi

    if [[ $DRY_RUN -eq 1 ]]; then
      echo "  DRY-RUN: $CMD"
      continue
    fi

    echo "  RUN: $CMD"
    if eval "$CMD" >/dev/null 2>&1; then
      echo "  PASS"
    else
      echo "  FAIL"
    fi
  done
  echo ""
done

echo "Audit complete."