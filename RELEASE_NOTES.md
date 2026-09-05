# Assura 0.4.4

Patch release: first-run snippets and the VS Code language server match
what `cargo install assura` actually ships, Float contracts generate
valid Rust, and `assura check` on a directory reports the same warnings
as a single file.

## Highlights

Copy-paste SafeDivision now uses `result == a / b`, so the first
`assura check` can verify without an IR sidecar. VS Code starts
`assura lsp`. Project-mode check no longer drops unconstrained-result
warnings, and Float `ensures { result >= 0 }` no longer wraps
`result` in `i128::from`.

## Getting started and editors

- **First-run SafeDivision used a remainder identity.** Without IR,
  that clause counterexamples. Init, README, cookbook, tutorial, and
  agent snippets now use `result == a / b` ([#1561](https://github.com/assura-lang/assura/pull/1561)).
- **VS Code launched `assura-lsp` with no arguments.** `cargo install
  assura` provides `assura lsp`. The extension default is now `assura`
  with args `["lsp"]`. Custom `serverPath` examples still point at a
  standalone `assura-lsp` binary ([#1561](https://github.com/assura-lang/assura/pull/1561)).
- **Tutorial CI ran `assura check` on the wrong path.** It now checks
  `contracts` ([#1561](https://github.com/assura-lang/assura/pull/1561)).

## `assura check` and project mode

- **`assura check <dir>` hid A04008.** Type warnings were not copied
  into the project report, so unconstrained `result` in ensures looked
  clean in a directory and noisy on a single file. Project check now
  copies those warnings and suppresses A04008 after a matching
  `::ensures` is Verified ([#1561](https://github.com/assura-lang/assura/pull/1561), [#1558](https://github.com/assura-lang/assura/pull/1558)).
- **Known SMT skips in a directory were silent.** Project check now
  emits A05102 for encoder gaps and unconstrained-result skips, same
  as a single-file check ([#1555](https://github.com/assura-lang/assura/pull/1555)).
- **Empty or vacuous files still said they passed.** Infer and check
  now report `vacuous: true` when there is nothing to prove, and keep
  exit 0 ([#1528](https://github.com/assura-lang/assura/pull/1528)).
- **`assura infer` treated function lines as 0-based.** Line numbers
  in output are 1-based ([#1544](https://github.com/assura-lang/assura/pull/1544)).

## `assura check-rust`

- **`@modifies xs` was parsed and then dropped.** The synthesized
  contract now includes `modifies { xs }` next to decreases ([#1561](https://github.com/assura-lang/assura/pull/1561)).
- **Empty `@modifies` became `modifies { }` and failed A14001.**
  Comment-only and `{}` annotations are skipped ([#1562](https://github.com/assura-lang/assura/pull/1562)).
- **Any `while` / `for` / `loop` stays `body_not_modeled`.** The
  reason prefix is `loop control flow not modeled`. Mid-block unknown
  calls use `mid-block expression not modeled as assignment/if/match`.
  `@loop_invariant` and `@stub` are not encoded ([#1562](https://github.com/assura-lang/assura/pull/1562)).
- **Co-located IR for the wrong item could be injected.** Check-rust
  now matches IR by module, not the first sibling ([#1540](https://github.com/assura-lang/assura/pull/1540), [#1541](https://github.com/assura-lang/assura/pull/1541)).

## Codegen

- **Float `ensures { result >= 0 }` emitted `i128::from(result)`.**
  `f64` does not implement `Into<i128>`, so the generated crate failed
  to compile. Float `result` and `output()` names skip that wrap ([#1561](https://github.com/assura-lang/assura/pull/1561)).
- **Service operations with Float parameters hit the same wrap.**
  Those params stay `f64` arithmetic ([#1557](https://github.com/assura-lang/assura/pull/1557)).

## JSON, MCP, and LSP

- **Missing-file and other CLI errors under `--json` were a bare
  diagnostic array.** They now use the same envelope as a successful
  check (`diagnostics`, `file_info`, `layer`, `verification`) ([#1528](https://github.com/assura-lang/assura/pull/1528)).
- **`assura fmt --json` on a directory omitted parse errors.** Each
  file object includes `parse_error` when the CST is invalid ([#1550](https://github.com/assura-lang/assura/pull/1550)).
- **MCP tools accepted paths outside the workspace.** Reads are
  limited to the current directory, `.assura` / `.rs` / `.ir`, and a
  size cap. Failures return stable `error_kind` values ([#1528](https://github.com/assura-lang/assura/pull/1528)).
- **`assura_ir_verify` without IR did not name the missing file.**
  The error now includes `ir` / `ir_file` ([#1553](https://github.com/assura-lang/assura/pull/1553)).
- **LSP hover missed nested parameters, and effect completions
  overwrote the preceding ident.** Hover walks nested params. Effect
  suffixes insert after the dot ([#1552](https://github.com/assura-lang/assura/pull/1552)).
- **rust-analyzer line mapping was wrong on CRLF files.** Offsets and
  function lines match the buffer ([#1540](https://github.com/assura-lang/assura/pull/1540), [#1541](https://github.com/assura-lang/assura/pull/1541)).

## Formatter

- **Minified `{...}` stayed on one line.** `assura fmt` expands those
  braces from CST tokens, skips braces inside strings, and keeps
  comment indent before a closing brace ([#1547](https://github.com/assura-lang/assura/pull/1547), [#1548](https://github.com/assura-lang/assura/pull/1548), [#1549](https://github.com/assura-lang/assura/pull/1549)).

## Types and effects

- **`must_not` with an unknown effect name had no Help.** A07003 now
  covers must-not as well as unknown names, and the Help says what to
  do ([#1539](https://github.com/assura-lang/assura/pull/1539), [#1542](https://github.com/assura-lang/assura/pull/1542)).
- **Effects policy and A04008 scope were too wide or too narrow.**
  Policy is enforced on the documented surfaces; A04008 skips only
  `result.length() >= 0` on externs ([#1538](https://github.com/assura-lang/assura/pull/1538), [#1558](https://github.com/assura-lang/assura/pull/1558)).

## Solver

- CVC5 crate pin is 0.4.1 (same 0.4 API). 0.5 and newer stay ignored
  until Assura adopts the lifetime change ([#1536](https://github.com/assura-lang/assura/pull/1536)).

## Upgrading

```bash
cargo install assura --locked --force
# libraries:
# assura-pipeline = "0.4.4"
```

Rust **1.87** or newer (workspace `rust-version`). No Assura source
changes are required for 0.4.3 projects. Rebuild generated Rust if you
have Float `result` or Float service parameters. Re-run `assura check`
on a project directory if you relied on A04008 only appearing in
single-file mode.

## Full changelog

https://github.com/assura-lang/assura/compare/v0.4.3...v0.4.4
