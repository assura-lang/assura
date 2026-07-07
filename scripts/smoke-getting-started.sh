#!/usr/bin/env bash
# Smoke the boring path from docs/GETTING-STARTED.md (#866 / P1-P2).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${TMPDIR:-/tmp}/assura-gs-smoke-$$"
mkdir -p "$OUT"
cleanup() { rm -rf "$OUT"; }
trap cleanup EXIT

if command -v assura >/dev/null 2>&1 && [[ "${USE_CARGO_ASSURA:-}" != "1" ]]; then
  ASSURA_CMD=(assura)
else
  ASSURA_CMD=(cargo run -q --manifest-path "$ROOT/Cargo.toml" --bin assura --)
fi

cp "$ROOT/demos/showcase-echo.assura" "$OUT/"
# Prefer co-located IR from demo when present; also exercise --write-ir.
if [[ -f "$ROOT/demos/ShowcaseEcho.ir" ]]; then
  cp "$ROOT/demos/ShowcaseEcho.ir" "$OUT/"
fi
"${ASSURA_CMD[@]}" check "$OUT/showcase-echo.assura"
"${ASSURA_CMD[@]}" check "$OUT/showcase-echo.assura" --strict
"${ASSURA_CMD[@]}" build "$OUT/showcase-echo.assura" --write-ir --bin --output "$OUT/generated"
(cd "$OUT/generated" && cargo test --quiet)
OUT_VAL=$(cd "$OUT/generated" && cargo run -q -- 9)
[[ "$OUT_VAL" == "9" ]] || { echo "expected cargo run 9, got: $OUT_VAL"; exit 1; }
echo "smoke-getting-started: OK"
