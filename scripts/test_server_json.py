#!/usr/bin/env python3
"""Lock server.json constraints for the official MCP Registry."""
from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SERVER = ROOT / "server.json"
GLAMA = ROOT / "glama.json"
README = ROOT / "crates" / "assura-cli" / "README.md"
# Official MCP Registry rejects body.description longer than this (HTTP 422).
MAX_DESCRIPTION = 100
SERVER_NAME = "io.github.assura-lang/assura"
MARKER = f"mcp-name: {SERVER_NAME}"


def main() -> int:
    data = json.loads(SERVER.read_text(encoding="utf-8"))
    desc = data.get("description")
    if not isinstance(desc, str) or not desc.strip():
        print("server.json description must be a non-empty string", file=sys.stderr)
        return 1
    if len(desc) > MAX_DESCRIPTION:
        print(
            f"server.json description is {len(desc)} chars; "
            f"MCP Registry max is {MAX_DESCRIPTION}",
            file=sys.stderr,
        )
        print(desc, file=sys.stderr)
        return 1
    if "prove" in desc.lower() and "secure" in desc.lower():
        print("server.json description must not claim a security proof", file=sys.stderr)
        return 1
    for word in ("Verified", "Counterexample", "Unknown"):
        if word not in desc:
            print(f"server.json description must name {word}", file=sys.stderr)
            return 1
    name = data.get("name")
    if name != SERVER_NAME:
        print(f"unexpected server.json name: {name!r}", file=sys.stderr)
        return 1
    packages = data.get("packages")
    if not isinstance(packages, list) or len(packages) != 1:
        print("server.json must list exactly the cargo package", file=sys.stderr)
        return 1
    package = packages[0]
    if package.get("registryType") != "cargo" or package.get("identifier") != "assura":
        print(f"unexpected package: {package!r}", file=sys.stderr)
        return 1
    args = package.get("packageArguments") or []
    values = [item.get("value") for item in args if isinstance(item, dict)]
    if values != ["mcp"]:
        print(f"package arguments must be ['mcp'], got {values!r}", file=sys.stderr)
        return 1
    readme = README.read_text(encoding="utf-8")
    if MARKER not in readme:
        print(f"crate README is missing the visible marker {MARKER}", file=sys.stderr)
        return 1
    if f"<!-- {MARKER} -->" in readme or f"<!--{MARKER}-->" in readme:
        print("cargo marker must be visible markdown, not an HTML comment", file=sys.stderr)
        return 1
    glama = json.loads(GLAMA.read_text(encoding="utf-8"))
    maintainers = glama.get("maintainers")
    if maintainers != ["SebTardif"]:
        print(f"glama.json maintainers must be ['SebTardif'], got {maintainers!r}", file=sys.stderr)
        return 1
    print(f"ok: description {len(desc)}/{MAX_DESCRIPTION} chars")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
