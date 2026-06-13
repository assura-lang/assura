# Contributing to Assura

Thank you for your interest in contributing to Assura.

## Getting Started

```bash
git clone https://github.com/assura-lang/assura.git
cd assura
cargo build
cargo test --workspace
```

### Prerequisites

- Rust (stable, edition 2024)
- Z3 SMT solver: `brew install z3` (macOS) or `sudo apt-get install -y libz3-dev` (Linux)
- Protobuf compiler: `brew install protobuf` (macOS) or `sudo apt-get install -y protobuf-compiler` (Linux)

## Pre-Commit Gate

Every change must pass before committing:

```bash
cargo fmt --all
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

Verify demo files still parse:

```bash
cargo run -- demos/libwebp-huffman.assura
cargo run -- demos/zlib-inflate.assura
cargo run -- demos/mbedtls-x509.assura
```

## Project Structure

The compiler is a Cargo workspace with 9 crates under `crates/`:

| Crate | Purpose |
|-------|---------|
| `assura-parser` | Lexer (logos), parser (chumsky 0.9), AST |
| `assura-resolve` | Name resolution, symbol table, scopes |
| `assura-types` | Type checker with 50+ domain checkers |
| `assura-smt` | Z3 SMT solver integration (Layer 1-3) |
| `assura-codegen` | Rust code generation |
| `assura-cli` | CLI binary (`assura check/build/init/explain`) |
| `assura-lsp` | Language Server Protocol server |
| `assura-server` | gRPC + HTTP API server |

## Coding Conventions

- Rust edition 2024
- `#[derive(Debug, Clone, PartialEq)]` on AST and data types
- Every AST node carries a `Span` (source location)
- `pub(crate)` for internal visibility, `pub` only for cross-crate API
- No `unwrap()` in library code; OK in CLI and tests
- Write `#[test]` functions in the same file as the code they test

### Pinned Crate Versions

| Crate | Version | Reason |
|-------|---------|--------|
| chumsky | 0.9 | 0.10+ has a completely different API |
| ariadne | 0.4 | 0.5+ changes Report/Label API |

### Error Codes

Errors use structured codes from the spec (format: `Axxxxx`). Each error
includes: code, location, message, optional secondary locations, and
optional suggested fix.

## Commit Messages

Format: `<scope>: <description>`

Scopes: `parser`, `resolve`, `types`, `smt`, `codegen`, `cli`, `docs`,
`tests`, `ci`, `deps`

## License

By contributing, you agree that your contributions will be licensed under
the project's dual MIT OR Apache-2.0 license.