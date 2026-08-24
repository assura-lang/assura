# Assura 0.4.3

Patch release: compound `old()` in SMT now matches the pre-state that
codegen already saved, and a few CLI checks behave as documented.

## Highlights

`ensures { result == old(x + 1) }` is checked against the value of `x`
from before the step, on Z3 and CVC5, not the value after havoc.
`assura check --timeout` is wired through, `assura init` prints the next
commands, and `assura doctor` treats rustc and a standalone `z3` CLI as
optional.

## Verification

- **`old(x + 1)` used the new `x`.** After a body that assigns to `x`,
  SMT still encoded the live expression, so a bad implementation could
  look verified while generated Rust had already snapshotted the real
  old value. Compound `old(...)` now snapshots free names (and fields)
  in the pre-state on Z3, CVC5 SMT-LIB, and CVC5 native. `old(x + 1) ==
  old(x) + 1` verifies; `x == old(x + 0)` under `modifies { x }` is a
  counterexample ([#1514](https://github.com/assura-lang/assura/pull/1514)).

## CLI

- **`assura check --timeout MS` is documented but was ignored.** The
  flag now sets the SMT timeout for check and for watch ([#1504](https://github.com/assura-lang/assura/pull/1504)).
- **`assura init` stopped after creating files.** It now prints `cd
  <dir>` and `assura check contracts/lib.assura` ([#1504](https://github.com/assura-lang/assura/pull/1504)).
- **`assura doctor` treated missing rustc or a standalone `z3` binary
  as a hard failure.** Those tools are optional. `assura check` links
  Z3 through the z3 crate ([#1504](https://github.com/assura-lang/assura/pull/1504)).

## Upgrading

```bash
cargo install assura --locked --force
# libraries:
# assura-pipeline = "0.4.3"
```

Rust **1.87** or newer (workspace `rust-version`). No Assura source
changes are required for 0.4.2 projects. Re-run verify if you use
compound `old()` in ensures.

## Full changelog

https://github.com/assura-lang/assura/compare/v0.4.2...v0.4.3
