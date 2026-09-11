#!/usr/bin/env bash
# Local tests for verify.sh (no network, no real assura).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
VERIFY="$ROOT/verify.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# Mock assura: exit 0 unless the filename contains fail, print Warning if warn.
mkdir -p "$TMP/bin" "$TMP/src" "$TMP/nested/dir"
cat >"$TMP/bin/assura" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
file="${!#}"
base="$(basename "$file")"
if [[ "$base" == *warn* ]]; then
  echo "Warning: [A05103] not yet encoded in SMT"
fi
if [[ "$base" == *fail* ]]; then
  echo "Counterexample for Demo::ensures"
  exit 1
fi
echo "check passed (no errors)"
exit 0
EOF
chmod +x "$TMP/bin/assura"
export PATH="$TMP/bin:$PATH"

echo "PLAN: verify.sh local tests"
echo "DO: empty glob fails"
if FILES="no-such-dir/*.assura" bash "$VERIFY" --step run >"$TMP/empty.out" 2>&1; then
  fail "empty glob should exit 1"
fi
grep -q "No .assura files matched" "$TMP/empty.out" || fail "missing empty-glob error"

echo "DO: allow-empty succeeds"
if ! ALLOW_EMPTY=true FILES="no-such-dir/*.assura" bash "$VERIFY" --step run >"$TMP/allow.out" 2>&1; then
  fail "allow-empty should exit 0"
fi
grep -q "allow-empty=true" "$TMP/allow.out" || fail "missing allow-empty note"

echo "DO: globstar matches nested file"
touch "$TMP/nested/dir/ok.assura"
(
  cd "$TMP"
  FILES="**/*.assura" bash "$VERIFY" --step run >"$TMP/glob.out" 2>&1
) || fail "globstar run should pass"
grep -q "nested/dir/ok.assura" "$TMP/glob.out" || fail "globstar missed nested file"

echo "DO: failing contract increments errors"
touch "$TMP/src/fail.assura"
if (
  cd "$TMP"
  FILES="src/fail.assura" bash "$VERIFY" --step run >"$TMP/fail.out" 2>&1
); then
  fail "failing contract should exit 1"
fi
grep -q "Counterexample" "$TMP/fail.out" || fail "diagnostics were discarded"
grep -q "Verification failed" "$TMP/fail.out" || fail "missing error annotation"

echo "DO: fail-on-warning treats Warning as error"
touch "$TMP/src/warn.assura"
if (
  cd "$TMP"
  FAIL_ON_WARNING=true FILES="src/warn.assura" bash "$VERIFY" --step run >"$TMP/warn.out" 2>&1
); then
  fail "fail-on-warning should exit 1"
fi
grep -q "fail-on-warning=true" "$TMP/warn.out" || fail "missing warning promotion"

echo "DO: result output is written"
export GITHUB_OUTPUT="$TMP/gh.out"
(
  cd "$TMP"
  FILES="nested/dir/ok.assura" bash "$VERIFY" --step run >"$TMP/out.out" 2>&1
) || fail "ok file should pass"
grep -q "total-contracts=1" "$TMP/gh.out" || fail "missing total-contracts"
grep -q "verified=1" "$TMP/gh.out" || fail "missing verified"
grep -q '"ok":true' "$TMP/gh.out" || fail "missing JSON result"

echo "DONE: verify.sh tests passed"
