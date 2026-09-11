#!/usr/bin/env bash
# Check every demos/*.assura against its header taxonomy.
# EXPECT FAIL files must exit non-zero. All other files must exit 0.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: check-demos.sh --step classify|check
  classify  Require a taxonomy header on every demo (no solver)
  check     Run assura check and assert the expected exit
EOF
}

STEP=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --step)
      STEP="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "FAIL: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$STEP" ]]; then
  usage >&2
  exit 2
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEMOS="$ROOT/demos"

demo_expect_fail() {
  local header="$1"
  printf '%s\n' "$header" | grep -q "EXPECT FAIL"
}

demo_has_header() {
  local header="$1"
  printf '%s\n' "$header" | grep -Eq "SHOWCASE|FEATURE|EXPECT FAIL|CVE-|TYPE\\.|MEM\\.|SEC\\.|CONC\\."
}

step_classify() {
  echo "PLAN: classify demo headers"
  local missing=0
  local count=0
  local expect_fail=0
  shopt -s nullglob
  local demo name header
  for demo in "$DEMOS"/*.assura; do
    name="$(basename "$demo")"
    header="$(head -n 5 "$demo")"
    count=$((count + 1))
    if demo_expect_fail "$header"; then
      expect_fail=$((expect_fail + 1))
      echo "OK: $name EXPECT FAIL"
    elif demo_has_header "$header"; then
      echo "OK: $name must-pass"
    else
      echo "FAIL: $name has no taxonomy/CVE/feature header"
      missing=1
    fi
  done
  if [[ "$count" -eq 0 ]]; then
    echo "FAIL: no demos/*.assura files found"
    exit 1
  fi
  if [[ "$missing" -ne 0 ]]; then
    echo "DONE: classify failed (count=$count)"
    exit 1
  fi
  echo "DONE: classified $count demos ($expect_fail EXPECT FAIL)"
}

step_check() {
  local assura="${ASSURA:-}"
  if [[ -z "$assura" ]]; then
    if command -v assura >/dev/null 2>&1; then
      assura="assura"
    else
      assura="cargo run --locked --bin assura --"
    fi
  fi

  echo "PLAN: check all demo contracts"
  local missing_header=0
  local failed=0
  local checked=0
  shopt -s nullglob
  local demo name header expect_fail rc
  for demo in "$DEMOS"/*.assura; do
    name="$(basename "$demo")"
    header="$(head -n 5 "$demo")"
    expect_fail=0
    if demo_expect_fail "$header"; then
      expect_fail=1
    elif ! demo_has_header "$header"; then
      echo "FAIL: $name has no taxonomy/CVE/feature header"
      missing_header=1
    fi

    echo "DO: $name expect_fail=$expect_fail"
    rc=0
    # shellcheck disable=SC2086
    $assura check "$demo" || rc=$?
    checked=$((checked + 1))
    if [[ "$expect_fail" -eq 1 ]]; then
      if [[ "$rc" -eq 0 ]]; then
        echo "FAIL: $name is EXPECT FAIL but check exited 0"
        failed=1
      else
        echo "OK: $name failed as expected (exit $rc)"
      fi
    else
      if [[ "$rc" -ne 0 ]]; then
        echo "FAIL: $name is must-pass but check exited $rc"
        failed=1
      else
        echo "OK: $name passed"
      fi
    fi
  done

  if [[ "$checked" -eq 0 ]]; then
    echo "FAIL: no demos/*.assura files found"
    exit 1
  fi
  if [[ "$missing_header" -ne 0 || "$failed" -ne 0 ]]; then
    echo "DONE: demo check failed (checked=$checked)"
    exit 1
  fi
  echo "DONE: all $checked demos matched their expected outcome"
}

case "$STEP" in
  classify) step_classify ;;
  check) step_check ;;
  *)
    echo "FAIL: unknown --step $STEP" >&2
    usage >&2
    exit 2
    ;;
esac
