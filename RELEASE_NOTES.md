# Assura 0.1.0

**First public release** of Assura: a contract-first language that type-checks and verifies what code should do, then generates Rust.

This release ships the **compiler libraries** on [crates.io](https://crates.io) and **CLI installers** on this GitHub Release.

## Highlights

- **End-to-end pipeline:** parse → name resolution → type-checking with domain checkers → SMT verification → Rust codegen
- **SMT-backed contracts:** Z3 is the default solver and is included for normal builds; CVC5 is available as an optional second solver
- **AI-native workflow:** write contracts, verify them, generate Rust; demos under `demos/` show real vulnerability patterns as contracts
- **Embeddable compiler:** add **`assura-pipeline`** from crates.io to compile and verify from your own tools
- **Dual license:** MIT OR Apache-2.0

## Install the CLI

Use the **prebuilt binaries and installers** attached to this GitHub Release (cargo-dist). That is the supported install path for 0.1.0.

From a full source checkout you can also build the CLI with:

```bash
cargo install --path crates/assura-cli
```

**Solvers:**

- **Z3 (default):** included for normal builds. No separate Z3 install for the usual path.
- **CVC5 (optional):** enable with the `cvc5-verify` feature when you want the second solver. See CONTRIBUTING.md and `scripts/setup-cvc5.sh` if you build that path from source.

## Use as a library

Depend on the public entry point:

```toml
assura-pipeline = "0.1.0"
```

That crate pulls in the published compiler stack (`assura-parser`, `assura-types`, `assura-smt`, and related crates). Details: [docs/CRATES-IO.md](https://github.com/assura-lang/assura/blob/main/docs/CRATES-IO.md).

## Try it

```bash
assura check demos/libwebp-huffman.assura
assura check demos/libwebp-huffman.assura --verbose
```

## Documentation

- [README](https://github.com/assura-lang/assura#readme)
- [Tutorial](https://github.com/assura-lang/assura/blob/main/docs/TUTORIAL.md)
- [crates.io publish notes](https://github.com/assura-lang/assura/blob/main/docs/CRATES-IO.md)
- [Contributing](https://github.com/assura-lang/assura/blob/main/CONTRIBUTING.md)
