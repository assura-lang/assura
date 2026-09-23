#!/usr/bin/env bash
# Shown when a Codespace / devcontainer is attached. Keep it short and runnable.
set -u

cat <<'BANNER'

  Assura: write what it should do, prove it does.

  Try a counterexample (intentional: this demo models real bugs):

      cargo run -- check demos/zip-crate-audit.assura

  Try a clean proof:

      cargo run -- check demos/heartbleed.assura

  Then write your own:

      cargo run -- init my-project

  Docs: docs/GETTING-STARTED.md   Cheatsheet: docs/CHEATSHEET.md

BANNER

if ! cargo build --locked -p assura --offline >/dev/null 2>&1; then
  echo "  Note: \`cargo build --locked -p assura --offline\` failed."
  echo "  If this is a new container, the first compile has not finished yet"
  echo "  and the commands above will build on demand (a few minutes)."
  echo "  If the project was already built, that failure is a real build error;"
  echo "  rerun the command without \`--offline\` to see it."
  echo
fi
