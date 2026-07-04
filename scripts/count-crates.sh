#!/usr/bin/env bash
# Print per-crate LOC and #[test] counts for MASTER-PLAN.md refresh.
# Only counts Cargo workspace members (respects root Cargo.toml exclude).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
python3 - <<'PY'
from pathlib import Path
import json
import re
import subprocess

meta = json.loads(
    subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        text=True,
    )
)
# Map package name -> manifest dir (absolute)
members = []
for p in meta["packages"]:
    manifest = Path(p["manifest_path"])
    # Only crates under this repo's crates/ or the root binary package
    try:
        rel = manifest.parent.relative_to(Path.cwd())
    except ValueError:
        continue
    members.append((p["name"], Path(rel)))

members.sort(key=lambda x: x[0])

print("| Crate | LOC | Tests |")
print("|-------|-----|-------|")
total_loc = total_tests = 0
for name, crate_dir in members:
    loc = tests = 0
    if not crate_dir.is_dir():
        continue
    for path in crate_dir.rglob("*.rs"):
        if "target" in path.parts:
            continue
        text = path.read_text(errors="ignore")
        loc += text.count("\n") + (1 if text and not text.endswith("\n") else 0)
        tests += len(re.findall(r"#\[(?:tokio::)?test\]", text))
    total_loc += loc
    total_tests += tests
    print(f"| {name} | {loc:,} | {tests:,} |")
print(f"| **Total** | **{total_loc:,}** | **{total_tests:,}** |")
print(f"\n({len(members)} workspace members; excluded dirs under crates/ are skipped)")
PY
