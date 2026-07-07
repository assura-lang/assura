#!/usr/bin/env bash
# Smoke the boring path from docs/GETTING-STARTED.md (#866).
# Usage (from repo root): bash scripts/smoke-getting-started.sh
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

cp "$ROOT/demos/showcase-echo.assura" "$ROOT/demos/ShowcaseEcho.ir" "$OUT/"
"${ASSURA_CMD[@]}" check "$OUT/showcase-echo.assura"
"${ASSURA_CMD[@]}" build "$OUT/showcase-echo.assura" --output "$OUT/generated"
(cd "$OUT/generated" && cargo test --quiet)
echo "smoke-getting-started: OK"
