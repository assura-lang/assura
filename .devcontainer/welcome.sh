#!/usr/bin/env bash
# Shown when a Codespace / devcontainer is attached. Keep it short and runnable.
set -u

cat <<'BANNER'

  Assura — write what it should do, prove it does.

  Try a counterexample (intentional: this demo models real bugs):

      cargo run -- check demos/zip-crate-audit.assura

  Try a clean proof:

      cargo run -- check demos/heartbleed.assura

  Then write your own:

      cargo run -- init my-project

  Docs: docs/GETTING-STARTED.md   Cheatsheet: docs/CHEATSHEET.md

BANNER

if ! cargo build --locked -p assura --offline >/dev/null 2>&1; then
  echo "  Note: first build still running or not yet cached; the commands above"
  echo "  will compile on demand (a few minutes the first time)."
  echo
fi
