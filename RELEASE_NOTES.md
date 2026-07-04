# Assura 0.1.0

**First public release** of Assura: a contract-first language that type-checks and verifies what code should do, then generates Rust.

This cut ships the **core library stack** on [crates.io](https://crates.io) and **CLI installers** on this GitHub Release.

## Highlights

- **End-to-end pipeline:** parse → name resolution → Layer 0 type-checking (domain checkers) → SMT verification → Rust codegen
- **SMT-backed contracts:** Z3 is the default solver path and is pulled in automatically for usual builds; CVC5 is available as an optional native path for portfolio-style work
- **AI-native workflow:** write contracts, verify, generate Rust; demos under `demos/` model real vulnerability patterns
- **Embeddable compiler:** depend on **`assura-pipeline`** from crates.io for compile/verify in your own tools
- **Dual license:** MIT OR Apache-2.0

Pre-1.0 means the language and library APIs can still evolve with the ecosystem, the same as any early Rust or formal-tools project. That is normal for a first public version, not a disclaimer about verification being second-class.

## Install the CLI

Prefer the **prebuilt binaries and installers** attached to this GitHub Release (cargo-dist).

The `assura` CLI package is not on crates.io yet (it still pulls in monorepo frontends such as LSP/MCP that are not part of this library publish). From a full checkout:

```bash
cargo install --path crates/assura-cli
```

**Solvers:**

- **Z3 (default):** included for normal builds via the `z3` crate `gh-release` feature. No separate Z3 install for the usual path.
- **CVC5 (optional):** enable with the `cvc5-verify` feature. The Rust `cvc5` bindings can link static CVC5; when a local source link is awkward, use prebuilts:

```bash
bash scripts/setup-cvc5.sh
# export the printed CVC5_LIB_DIR and CVC5_INCLUDE_DIR
cargo build -p assura-smt --features cvc5-verify
```

Default installers and default features are Z3-first so most users get a working verify path immediately. CVC5 remains an opt-in power path (see CONTRIBUTING.md).

## Use as a library (crates.io)

Public embed surface: **`assura-pipeline`**.

```toml
assura-pipeline = "0.1.0"
```

Published graph (dependency order):

`assura-ast` → `assura-config` → `assura-diagnostics` → `assura-macros` → `assura-runtime` → `assura-parser` → `assura-fmt` → `assura-stdlib` → `assura-resolve` → `assura-types` → `assura-codegen` → `assura-smt` → **`assura-pipeline`**

This release focuses on that **compiler library stack**. The CLI binary ships here via GitHub Releases; product frontends (`assura-lsp`, `assura-mcp`, `assura-server`, …) and internal `assura-test-support` stay out of crates.io for now so the first registry publish stays clean and usable. More on that split: [docs/CRATES-IO.md](https://github.com/assura-lang/assura/blob/main/docs/CRATES-IO.md).

## Try it

```bash
# after installing the CLI from this release
assura check demos/libwebp-huffman.assura
assura check demos/libwebp-huffman.assura --verbose
```

## Documentation

- [README](https://github.com/assura-lang/assura#readme)
- [Tutorial](https://github.com/assura-lang/assura/blob/main/docs/TUTORIAL.md)
- [crates.io / release process](https://github.com/assura-lang/assura/blob/main/docs/CRATES-IO.md)
- [Contributing](https://github.com/assura-lang/assura/blob/main/CONTRIBUTING.md)

## After this tag

Remove the temporary `release-as: 0.1.0` pin in release-please config once this release has landed (tracked in #784) so later versions follow normal conventional-commit bumps.
