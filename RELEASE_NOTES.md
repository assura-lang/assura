# Assura 0.4.1

Patch release focused on **solver timeouts that actually stick**, **cleaner
agent JSON when a path is wrong**, and **docs for first-time and launch
workflows**. Install the same way as 0.4.0.

## Highlights

Longer `verify.timeout` values now apply to Z3 and CVC5 (shell and native),
so hard contracts are less likely to die early under a short internal budget.
`assura check --json` on a missing file returns the same report shape as a
normal check, which makes automation easier to write.

## Verification and CLI

- **Z3 and CVC5 honor `verify.timeout`.** Raising the timeout in `assura.toml`
  or on the CLI is respected by Z3 clause solvers and by CVC5 shell and native
  paths (including `tlimit`), not only by a subset of the stack
  ([#1384](https://github.com/assura-lang/assura/pull/1384),
  [#1409](https://github.com/assura-lang/assura/pull/1409)).
- **Missing file under `--json` matches the success envelope.**
  `assura check --json /path/missing.assura` emits an object with
  `diagnostics`, `file_info`, `layer`, and `verification` (code `A01000`),
  not a bare diagnostic array ([#1449](https://github.com/assura-lang/assura/pull/1449)).
- **Parser dependency upgrade (rowan 0.17).** No Assura language changes;
  the CST stack is on the current rowan major
  ([#1451](https://github.com/assura-lang/assura/pull/1451)).

## Docs and onboarding

- Launch playbooks, SMT portfolio note, Codespaces try path, demo GIF, and
  install path clarity for crates.io vs GitHub Releases
  ([#1406](https://github.com/assura-lang/assura/pull/1406),
  [#1407](https://github.com/assura-lang/assura/pull/1407),
  [#1432](https://github.com/assura-lang/assura/pull/1432)).
- Getting started is linked from the mdBook nav; error-code index and
  agent-oriented docs continue to track high-traffic codes
  ([#1434](https://github.com/assura-lang/assura/pull/1434),
  [#1437](https://github.com/assura-lang/assura/pull/1437)).

## Dependencies

- Routine crate and Actions bumps (including MCP stack on rmcp 3.x for the
  published server surface). Prefer `cargo install assura --locked` so the
  lockfile matches what we tested.

## Upgrading

```bash
cargo install assura --locked --force
# or pin libraries:
# assura-pipeline = "0.4.1"
```

No Assura source changes are required for 0.4.0 projects. If you set a long
`verify.timeout`, re-run a slow contract with CVC5 or portfolio to confirm
the full budget is used.

## Full changelog

https://github.com/assura-lang/assura/compare/v0.4.0...v0.4.1
