#!/usr/bin/env bash
# Rewrite path-dependency version= pins to match [workspace.package].version.
# Used on release-please PRs so crates.io packaging constraints stay aligned.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

python3 - <<'PY'
from __future__ import annotations

import re
import tomllib
from pathlib import Path

root = Path(".")
data = tomllib.loads((root / "Cargo.toml").read_text())
ver = data["workspace"]["package"]["version"]
print(f"workspace version: {ver}")

# Match version = "x.y.z" on lines that also declare path = (path deps only).
line_re = re.compile(
    r'^(?P<pre>.*\bversion\s*=\s*)"(?P<old>[^"]+)"(?P<post>.*\bpath\s*=\s*".*".*)$'
)
# Also handle path before version on the same line.
line_re_alt = re.compile(
    r'^(?P<pre>.*\bpath\s*=\s*"[^"]+".*\bversion\s*=\s*)"(?P<old>[^"]+)"(?P<post>.*)$'
)

changed_files = 0
for path in sorted((root / "crates").glob("*/Cargo.toml")):
    text = path.read_text()
    out_lines = []
    file_changed = False
    for line in text.splitlines(keepends=True):
        raw = line.rstrip("\n")
        ending = line[len(raw) :]
        m = line_re.match(raw) or line_re_alt.match(raw)
        if m and m.group("old") != ver:
            raw = f'{m.group("pre")}"{ver}"{m.group("post")}'
            file_changed = True
        out_lines.append(raw + ending)
    if file_changed:
        path.write_text("".join(out_lines))
        changed_files += 1
        print(f"updated {path}")

print(f"updated {changed_files} file(s)")
PY
