# Assura 0.3.0

Assura is a contract-first language: you write what the program must do, the
compiler and SMT solvers check it, and the toolchain generates Rust you can
build and run.

**0.3.0 is the release where install, check, prove, and run line up.** You can
install the real CLI from crates.io, follow one simple getting-started path,
and get real **Verified** results on result-bearing contracts without fighting
the toolchain first.

## Highlights

- **`cargo install assura --locked` installs the real compiler CLI** (and
  related frontends co-publish with the library stack).
- **A single documented path:** write a tiny contract, `assura check`,
  optional IR, `assura build`, then `cargo test` or `cargo run`.
- **Smarter proofs for everyday ensures:** identity, arithmetic, and call
  shapes verify more often; co-located IR and offline `--write-ir` close the
  loop into generated Rust.
- **Labeled demos and clearer check UX:** showcase files for first green
  runs; expected-fail demos are labeled so audit files do not look like a
  broken install.

## What's new

### Install the CLI from crates.io

You can install Assura the way most Rust tools install:

```bash
cargo install assura --locked
assura --help
```

GitHub Release installers (cargo-dist) remain available for prebuilt
binaries. Embedders still use **`assura-pipeline`** on crates.io as the
public library entry (`compile` / `compile_full` / `verify_typed`).

See [docs/CRATES-IO.md](https://github.com/assura-lang/assura/blob/main/docs/CRATES-IO.md)
and the [FAQ](https://github.com/assura-lang/assura/blob/main/docs/FAQ.md).

### Contract to check to build to run

New guide: **[docs/GETTING-STARTED.md](https://github.com/assura-lang/assura/blob/main/docs/GETTING-STARTED.md)**
(linked from the README and tutorial).

Typical flow:

```bash
# Check (often Verified on synthesizable result ensures)
assura check ShowcaseEcho.assura

# Persist IR + inject bodies + optional binary
assura build ShowcaseEcho.assura --write-ir --bin --output generated
cd generated && cargo test
cargo run -- 42
```

Repo smoke (from a clone): `bash scripts/smoke-getting-started.sh`.

### Result postconditions that actually prove

`ensures { result == ... }` is first-class, not a dead end:

- **In-memory synthesis on check:** for synthesizable shapes (identity,
  simple arithmetic, known if/match/call patterns), `assura check` builds
  an implementation body in memory when co-located IR is missing, so you
  often get **Verified** without writing a `.ir` file first.
- **Offline IR without an LLM:** `assura build --write-ir` writes co-located
  `{ContractName}.ir` from the same heuristics and injects it into generated
  Rust.
- **Runnable binary:** `assura build --bin` emits a small `main` for the
  primary contract so you can `cargo run` a demo end-to-end.
- **Clear refusal:** unanalyzable shapes (for example bare `result > 0`
  with no body) stay **Unknown** with guidance, not a silent counterexample
  and not a fake identity proof.

### Call-shaped contracts and multi-contract files

- **Call IR:** same-file pure helpers (including multi-arg arithmetic
  shapes) get non-identity sibling IR instead of empty plumbing stubs.
- **Ensures equating:** `result == double(x)` lines up with the helper's
  functional ensures under Z3 and CVC5 when the helper is in-file and pure.
- **Multi-contract `verify_ir`:** matches the IR module name to a contract,
  or asks you to name the contract instead of silently validating against
  the first declaration only.

### Demos you can trust for a first green run

Demos are labeled **SHOWCASE** (must-pass) vs **EXPECT FAIL** (intentional
counterexamples / audit models). Prefer:

- `demos/heartbleed.assura`
- `demos/showcase-echo.assura` (result-bearing path)
- other SHOWCASE entries in [demos/README.md](https://github.com/assura-lang/assura/blob/main/demos/README.md)

Directory check can filter demos:

```bash
assura check demos --showcase-only
```

### Fixed-width integers that compare correctly

Signed fixed-width types (`I8`, `I32`, and friends) use proper signed
bitvector ordering in Z3 and CVC5, so comparisons and bounds on fixed-width
values match what you wrote in the contract.

### Check and build flags that match real workflows

| Flag | What it does |
|------|----------------|
| `assura check --strict` | Treat SMT Unknown (including known limitations) as failure (great for CI) |
| `assura check --showcase-only` | In a directory, only files marked SHOWCASE |
| `assura build --write-ir` | Write co-located heuristic IR (no LLM) |
| `assura build --bin` | Add a runnable binary target for the primary contract |
| `assura build --auto-implement` | LLM path when configured (still optional) |

## Try it in under a minute

```bash
cargo install assura --locked

mkdir hello-assura && cd hello-assura
cat > ShowcaseEcho.assura << 'EOF'
contract ShowcaseEcho {
  input(x: Int)
  output(result: Int)
  ensures { result == x }
}
EOF

assura check ShowcaseEcho.assura
# Expect: ensures ... verified

assura build ShowcaseEcho.assura --write-ir --bin --output generated
cd generated && cargo run -- 7
# Expect: 7
```

From a monorepo clone without installing:

```bash
cargo run -q --bin assura -- check demos/showcase-echo.assura
bash scripts/smoke-getting-started.sh
```

## After you upgrade

1. Prefer **crates.io 0.3.0** (`cargo install assura --locked` or
   `assura-pipeline = "0.3"`) over tracking unreleased `main` unless you need
   unreleased work.
2. Re-run `assura check` on your contracts. Synthesizable result ensures may
   flip from Unknown/skipped to **Verified** without new files.
3. For generated code and `cargo run`, use co-located IR or
   `assura build --write-ir` so implementations land in the crate (check
   synthesis alone does not write disk IR).
4. For CI gates that mean "no Unknowns," use `assura check --strict`.
5. Start new users on SHOWCASE demos and
   [GETTING-STARTED](https://github.com/assura-lang/assura/blob/main/docs/GETTING-STARTED.md),
   not `*-audit.assura` files.

## Full history

Machine-generated commit list:
[CHANGELOG.md](https://github.com/assura-lang/assura/blob/main/CHANGELOG.md)
(and the release-please section for 0.3.0 on the release PR).

Notable product work in this window includes co-publish of the CLI (#845),
getting-started and call/IR onboarding (#868 to #870), CVC5 equating parity
(#871), and in-memory IR synthesis on check (#872), plus fixed-width signed
ordering and multi-contract IR selection fixes.
