# Assura 0.4.2

Patch release focused on **stronger `check-rust` body proofs** (mutation and
control flow), **clearer docs for that surface**, and a **tighter diagnostics
catalog** so error codes match what the tools actually emit. Install the same
way as 0.4.1.

## Highlights

`assura check-rust` can model more realistic Rust bodies: `let mut` updates
and mutations across `if` / `match` join points, not only straight-line pure
code. Docs spell out what is supported, with demos and agent-loop guidance.
The published error catalog is smaller and aligned with codes you can actually
hit in practice (~195 entries).

## `check-rust` (annotated Rust)

- **Mutating locals.** Linear SSA tracks `let mut` reassignment so ensures can depend on the value after assignment, not only the initial binding ([#1465](https://github.com/assura-lang/assura/pull/1465)).
- **Control-flow joins.** CFG SSA merges mutations at `if` / `match` joins so branch-local updates are not lost at the join ([#1468](https://github.com/assura-lang/assura/pull/1468)).
- **Demos and agent loop.** Check-rust demos under `demos/check-rust/`, interop notes, and launch alignment for the annotate-check-fix cycle ([#1464](https://github.com/assura-lang/assura/pull/1464)).
- **Supported surface page.** [check-rust supported surface](https://assura-lang.github.io/assura/CHECK-RUST-SURFACE.html) maps what body shapes can be proven today versus `body_not_modeled` ([#1463](https://github.com/assura-lang/assura/pull/1463), [#1467](https://github.com/assura-lang/assura/pull/1467)).
- **COMPARE.** Clearer positioning of check-rust relative to Verus-style full-crate proof ([#1455](https://github.com/assura-lang/assura/pull/1455)).

Example:

```bash
assura check-rust demos/check-rust/ok
assura check-rust src/ --json
```

## Diagnostics and docs

- **Error catalog matches reality.** The diagnostics catalog and SPEC §7.2 list codes that the compiler and CLI actually emit (~195). Unused placeholder codes were dropped so `assura explain` and the [error-code index](https://assura-lang.github.io/assura/error-codes.html) stay useful for debugging ([#1496](https://github.com/assura-lang/assura/pull/1496), [#1495](https://github.com/assura-lang/assura/pull/1495)).
- **High-traffic index.** Expanded agent-oriented index of common codes with phase and start paths ([#1480](https://github.com/assura-lang/assura/pull/1480), [#1484](https://github.com/assura-lang/assura/pull/1484), [#1485](https://github.com/assura-lang/assura/pull/1485), [#1488](https://github.com/assura-lang/assura/pull/1488)).
- **Primary paths.** High-traffic rows point at real implement files, not reserved stubs ([#1488](https://github.com/assura-lang/assura/pull/1488), [#1497](https://github.com/assura-lang/assura/pull/1497)).

## Security and dependencies

- Dependency and Actions updates, including npm advisories under the VS Code extension tree (brace-expansion and related) ([#1478](https://github.com/assura-lang/assura/pull/1478), Dependabot crate and Actions bumps).

## Upgrading

```bash
cargo install assura --locked --force
# libraries:
# assura-pipeline = "0.4.2"
```

No Assura source changes are required for 0.4.1 projects. If you use
`check-rust` with `let mut` or branchy mutation, re-run your suite: more
bodies may verify that previously reported `body_not_modeled` or weak
results.

If you scripted against a removed placeholder error code number, switch to
codes that `assura explain` still documents (or the high-traffic index).

## Contributors

Thanks to external contributors in this release:

- [@amanraj-gith](https://github.com/amanraj-gith) for expanding the high-traffic error-code index ([#1480](https://github.com/assura-lang/assura/pull/1480))

## Full changelog

https://github.com/assura-lang/assura/compare/v0.4.1...v0.4.2
