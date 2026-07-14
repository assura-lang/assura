# Assura 0.4.0

Assura is a contract-first language: you write what the program must do,
SMT solvers check it, and the toolchain generates Rust you can build and run.

**0.4.0 is the release where everyday ensures prove more often, onboarding
stops requiring hand-written IR, and check-rust models a much wider slice
of real Rust bodies.** Install from crates.io, follow the guide, and get
**Verified** on contracts that used to stall on Unknown or body_not_modeled.

## Highlights

- **Synthesis-first contracts:** `assura check` synthesizes implementation
  bodies in memory for many ensures shapes (identity, nested arith, bounds,
  And chains, abs/min/max/clamp/signum free or method form, fields, length,
  Bool logic, if/match/let). You often get **Verified** without writing a
  `.ir` file.
- **Multi-ensures that pick a real body:** prefers `result == e` over
  if/match and pure bounds; combines multi-clause bounds with a lower-bound
  witness; residual clauses are named under `check -v` and in JSON.
- **Dual track for residuals:** offline `assura build --write-ir`, then
  `--auto-implement` (heuristics first, LLM only for leftovers).
- **check-rust body proof surface:** checked/wrapping/saturating arithmetic,
  bit ops, rotates, powers, unwrap_or peels, and more, so annotated Rust
  is proven against the body instead of skipped as body_not_modeled.
- **Flagship demos stay green:** `libwebp-huffman` and `taint-tracking`
  check with no A05102 noise; SHOWCASE demos are labeled for first runs.

## What's new

### Contracts that verify without hand IR

`assura check` auto-synthesizes analyzable ensures when no co-located IR
exists:

| Family | Examples |
|--------|----------|
| Equality | `result == x`, nested arith, `-x` |
| Builtins | free or method `abs` / `min` / `max` / `clamp` / `signum` |
| Bounds | `result >= e`, And chains, multi-clause bounds (prefer lower witness) |
| Multi-ensures | Prefer `result == e` when mixed with bounds or if/match |
| Structure | fields, tuples, `result == xs.length()`, Bool logic, if/match/let |

Use the residual ladder when a shape is not synthesizable:

1. Simplify ensures toward the table above, or  
2. `assura build file.assura --write-ir` (offline heuristic IR), or  
3. `assura build file.assura --auto-implement` (offline first, then LLM).

`result.length() >= 0` on extern / unconstrained result verifies via length
axioms without a body (common SEC.1 pattern).

Docs: [GETTING-STARTED.md](https://github.com/assura-lang/assura/blob/main/docs/GETTING-STARTED.md),
[demos/README.md](https://github.com/assura-lang/assura/blob/main/demos/README.md).

### Multi-ensures and transparency

When a contract has several ensures clauses:

- The planner prefers a top-level **`result == e`** body over if/match and
  pure bound witnesses.
- Pure bound-only multi-ensures share one **lower-bound** witness.
- Unplannable clauses no longer block a later equality plan.
- **`assura check -v`** prints the body driver and residual ensures
  (`not body driver`).
- **`assura check --json`** exposes the same surface under
  `file_info.ir` (`colocated`, `synthesized`, `synth_notes`) for agents.

### Dual-track build path

| Command | Role |
|---------|------|
| `assura check` | In-memory synthesis + SMT |
| `assura build --write-ir` | Persist analyzable IR next to source (no API key) |
| `assura build --auto-implement` | Offline heuristics first, then LLM for residuals |
| `assura build --bin` | Runnable `main` for the primary contract |

`--auto-implement` no longer requires the model for shapes the offline
planner already understands.

### check-rust: prove against real Rust bodies

A large expansion of body encoding so `@requires` / `@ensures` on Rust
are checked against the implementation, not left as body_not_modeled:

- **Arithmetic peels:** `checked_*`, `wrapping_*`, `saturating_*`,
  `overflowing_*` (including `.0` / `.1` and `unwrap_or` / `unwrap_or_default`)
- **Bit and shift surface:** and/or/xor, not, rotates, leading/trailing
  zeros and ones, reverse/swap bytes, const and variable masks
- **Powers and roots:** small `pow` / `wrapping_pow` / `checked_pow`,
  `isqrt`, `ilog2` / `ilog10`, `next_power_of_two`
- **Control flow:** if/match/let folding, distribution over binary ops,
  identity match guards
- **Widths:** u8 through u64/i64 paths, NonZero divisors for rem_euclid /
  div_ceil, u128/i128 bounds where supported

Wrong bodies still produce **counterexamples**. Division by a zero-including
path divisor is refused rather than silently modeled.

### Agent and CLI polish

- **Pure JSON** across check, build, fmt, ir, check-rust, init, audit,
  infer, and related commands (machine output without human noise).
- **Vacuous success** is marked in human and JSON (`file_info.vacuous`) so
  empty or requires-only files are not mistaken for full proof.
- **MCP / REPL** failures report structured JSON when requested.
- **`assura fmt`** accepts directories of `.assura` files.
- **Project-mode check** runs SMT verify, not only types.

### Demos and first green run

Prefer SHOWCASE demos for install smoke:

```bash
assura check demos/heartbleed.assura
assura check demos/showcase-echo.assura
assura check demos/libwebp-huffman.assura   # zero-warning on 0.4.0
assura check demos/taint-tracking.assura   # read_blob length ensures verified
assura check demos --showcase-only
```

EXPECT FAIL demos remain intentional (audit / attack models). See
[demos/README.md](https://github.com/assura-lang/assura/blob/main/demos/README.md).

## Try it

```bash
cargo install assura --locked

# Tiny contract path (see GETTING-STARTED.md for full copy-paste files)
assura check ShowcaseEcho.assura
assura build ShowcaseEcho.assura --write-ir --bin --output generated
cd generated && cargo test && cargo run -- 42
```

From a monorepo clone: `bash scripts/smoke-getting-started.sh`.

Embedders: **`assura-pipeline`** on crates.io (`compile` / `compile_full` /
`verify_typed`). Ops notes: [docs/CRATES-IO.md](https://github.com/assura-lang/assura/blob/main/docs/CRATES-IO.md).

## Upgrading from 0.3.0

```bash
cargo install assura --locked --force
# or pin crates to 0.4.0 in Cargo.toml for assura-pipeline and friends
```

- Re-run demos you used as smoke tests; libwebp and taint should now report
  **check passed (no errors)** without determinism / length skips.
- Prefer synthesizable ensures (table above) before hand IR.
- For CI, keep `assura check --strict` if you want Unknown limitations to fail.
- check-rust: more bodies verify; review any previous body_not_modeled items
  that may now prove or CE against the real body.

No intentional breaking CLI renames in 0.4.0. Pre-1.0 APIs may still evolve
in minor releases.

## Links

- Guide: [docs/GETTING-STARTED.md](https://github.com/assura-lang/assura/blob/main/docs/GETTING-STARTED.md)
- Demos: [demos/README.md](https://github.com/assura-lang/assura/blob/main/demos/README.md)
- Compare: [v0.3.0...v0.4.0](https://github.com/assura-lang/assura/compare/v0.3.0...v0.4.0)
- Install / crates: [docs/CRATES-IO.md](https://github.com/assura-lang/assura/blob/main/docs/CRATES-IO.md)

Dual-licensed MIT OR Apache-2.0.
