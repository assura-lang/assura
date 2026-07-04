#!/usr/bin/env bash
# Print per-crate LOC and #[test] counts for MASTER-PLAN.md refresh.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
python3 - <<'PY'
from pathlib import Path
import re

print("| Crate | LOC | Tests |")
print("|-------|-----|-------|")
total_loc = total_tests = 0
for crate_dir in sorted(Path("crates").iterdir()):
    if not crate_dir.is_dir():
        continue
    loc = tests = 0
    for p in crate_dir.rglob("*.rs"):
        if "target" in p.parts:
            continue
        text = p.read_text(errors="ignore")
        loc += text.count("\n") + (1 if text and not text.endswith("\n") else 0)
        tests += len(re.findall(r"#\[(?:tokio::)?test\]", text))
    total_loc += loc
    total_tests += tests
    print(f"| {crate_dir.name} | {loc:,} | {tests:,} |")
print(f"| **Total** | **{total_loc:,}** | **{total_tests:,}** |")
PY
