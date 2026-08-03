#!/usr/bin/env bash
# Gate for demos/check-rust (issue #1458). Expect prove demos ok; fail demo non-zero.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ -x "${ASSURA_BIN:-}" ]]; then
  BIN="$ASSURA_BIN"
elif [[ -x "$ROOT/target/debug/assura" ]]; then
  BIN="$ROOT/target/debug/assura"
elif command -v assura >/dev/null 2>&1; then
  BIN="$(command -v assura)"
else
  cargo build -p assura --locked -q
  BIN="$ROOT/target/debug/assura"
fi

echo "Using: $BIN"
"$BIN" check-rust "$ROOT/demos/check-rust/ok"
if "$BIN" check-rust "$ROOT/demos/check-rust/fail/clamp_wrong.rs"; then
  echo "FAIL: expected non-zero exit on fail/clamp_wrong.rs" >&2
  exit 1
fi
echo "check-rust demos: OK"
