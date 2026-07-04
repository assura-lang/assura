# Assura 0.1.0

**First public release.** Experimental: contracts, SMT behavior, and library APIs may change before 1.0.

Assura is a contract-first language that type-checks and verifies specifications, then generates Rust. This release publishes the **core library stack** to crates.io and ships **CLI installers** via GitHub Releases.

## Install the CLI

Prebuilt binaries and installers are on this GitHub Release (cargo-dist). Prefer those over crates.io for the CLI.

The `assura` binary package is **not** published to crates.io yet (it still depends on unpublished frontends such as LSP/MCP). From source:

```bash
cargo install --path crates/assura-cli
```

Z3 is pulled in automatically for default builds (no manual solver install required for the usual path).

## Use as a library (crates.io)

Public embed surface: **`assura-pipeline`** (compile / verify entry points). Dependency order for the published graph:

`assura-ast` → `assura-config` → `assura-diagnostics` → `assura-macros` → `assura-runtime` → `assura-parser` → `assura-fmt` → `assura-stdlib` → `assura-resolve` → `assura-types` → `assura-codegen` → `assura-smt` → **`assura-pipeline`**

```toml
assura-pipeline = "0.1.0"
```

Not published in this release: `assura` (CLI package), `assura-test-support`, and product frontends (`assura-lsp`, `assura-mcp`, `assura-server`, …). Details: [docs/CRATES-IO.md](https://github.com/assura-lang/assura/blob/main/docs/CRATES-IO.md).

## What you get

- Full pipeline: parse → resolve → type-check (Layer 0 checkers) → optional SMT verify (Z3; CVC5 as an optional feature path) → Rust codegen
- CLI: `check`, `build`, `fmt`, `init`, and related developer commands (see README)
- Demos under `demos/` modeling real CVE-style contract patterns
- Dual license: MIT OR Apache-2.0

## Experimental guarantees

- **Not production-certified formal methods.** Treat “verified” results as a strong development aid, not a warranty.
- SMT may report `Unknown` for encodings that are not yet complete; the CLI treats known limitation markers as warnings.
- Library crate APIs may change with minor releases while pre-1.0.

## Documentation

- [README](https://github.com/assura-lang/assura#readme)
- [Tutorial](https://github.com/assura-lang/assura/blob/main/docs/TUTORIAL.md)
- [crates.io packaging / release process](https://github.com/assura-lang/assura/blob/main/docs/CRATES-IO.md)
- [Contributing](https://github.com/assura-lang/assura/blob/main/CONTRIBUTING.md)

## Upgrade / next

After this tag, remove the temporary `release-as: 0.1.0` pin in release-please config (tracked in #784) so later versions follow normal conventional-commit bumps.
