# Contributing to Assura

Thank you for your interest in Assura! This guide covers everything you
need to set up, build, test, and submit changes.

## Prerequisites

- **Rust 1.85+** (edition 2024)
- **Z3 4.12+** (required for SMT verification; 4.16+ recommended)
- **CVC5** (optional, for portfolio solver mode)

```bash
# macOS
brew install z3
brew install cvc5   # optional

# Ubuntu/Debian
sudo apt-get install -y libz3-dev

# Verify
z3 --version
assura doctor       # checks all dependencies
```

## Getting Started

```bash
git clone https://github.com/assura-lang/assura.git
cd assura
cargo test --workspace
```

If all tests pass, you are ready to contribute.

## Project Structure

The compiler is a Cargo workspace with one crate per pipeline stage:

```
Source (.assura)
  --> assura-parser     Lexer (logos) + recursive-descent parser (rowan CST)
  --> assura-resolve    Name resolution, symbol table, scope analysis
  --> assura-types      Type checking, 50+ domain-specific checkers
  --> assura-smt        Z3 SMT solver integration, verification
  --> assura-codegen    Rust code generation via prettyplease
  --> assura-cli        CLI binary (check, build, init, fmt, explain, ...)
  --> assura-lsp        Language Server Protocol (tower-lsp)
```

Supporting crates: `assura-diagnostics` (error types), `assura-config`
(project configuration), `assura-fmt` (formatter), `assura-pipeline`
(multi-file compilation orchestration), `assura-macros` (`#[contract]`
and `#[trust]` proc macros), `assura-stdlib` (standard library
definitions), `assura-mcp` (MCP server), `assura-rust-analyzer` (Rust
source analysis), `assura-bench` (benchmarks), `assura-server`
(gRPC/HTTP API).

## Development Workflow

### 1. Make your change

Edit the relevant crate. Every compiler pass lives in its own crate
under `crates/`.

### 2. Run the pre-commit gate

Session-end / full gate (matches [AGENTS.md](AGENTS.md) and CI). Use
`--locked` so `Cargo.lock` is not rewritten accidentally:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --locked -- -D warnings
cargo clippy -p assura-smt --features cvc5-verify -- -D warnings
cargo test --workspace --locked
cargo check --no-default-features -p assura-smt
```

While iterating on a single crate (faster, agent-friendly):

```bash
cargo fmt -- <changed files>
cargo clippy -p <crate> --locked -- -D warnings
cargo check -p <crate> --locked
cargo test -p <crate> --locked --lib
```

The `cvc5-verify` clippy pass mirrors the CI `cvc5` job and catches
cfg-gate mistakes in native CVC5 modules that default workspace clippy
skips. The final `cargo check --no-default-features` verifies the no-Z3
build: any code in `assura-smt` that imports Z3 must be behind
`#[cfg(feature = "z3-verify")]` with a fallback.

For local CVC5 on macOS ARM, run `bash scripts/setup-cvc5.sh` and export
the printed `CVC5_LIB_DIR` / `CVC5_INCLUDE_DIR` before the cvc5 clippy/test
commands (source builds often fail under AppleClang).

### 3. Verify demo files still parse

```bash
cargo run --bin assura -- check demos/libwebp-huffman.assura
cargo run --bin assura -- check demos/zlib-inflate.assura
cargo run --bin assura -- check demos/mbedtls-x509.assura
cargo run --bin assura -- check demos/taint-tracking.assura
cargo run --bin assura -- check demos/heartbleed.assura
```

### 4. Commit

Use scoped commit messages:

```
<scope>: <description>
```

| Scope | When to use |
|-------|-------------|
| `parser` | Lexer or parser changes |
| `resolve` | Name resolution |
| `types` | Type checker |
| `smt` | SMT verification |
| `codegen` | Rust code generation |
| `cli` | CLI commands |
| `lsp` | Language server |
| `docs` | Documentation |
| `tests` | Test infrastructure |
| `ci` | CI/CD workflows |
| `deps` | Dependency updates |

## Testing

### Unit tests

Write `#[test]` functions in the same file as the code they test:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_contract() {
        let (ast, errors) = assura_parser::parse("contract Foo { }");
        assert!(errors.is_empty());
        assert!(ast.is_some());
    }
}
```

### Fixture tests

Test `.assura` files live in `tests/fixtures/`:

- `must_compile/` -- valid contracts that must parse and type-check
- `must_reject/` -- invalid contracts annotated with `// MUST REJECT Axxxxx`
- `errors/` -- files with specific parse errors

### End-to-end tests

Full pipeline tests in `tests/e2e/` exercise parsing through verification.

### Demo files

The five files in `demos/` are regression guards. Every PR must not
break them. They model real CVEs (libwebp, zlib, mbedtls, heartbleed,
taint-tracking).

## Adding a New Compiler Pass

When adding a new crate or major feature:

1. Create `crates/assura-{name}/` with workspace-inherited metadata
2. Wire it into the CLI pipeline in `crates/assura-cli/src/main.rs`
3. Add at least one integration test that feeds output from the
   previous pass
4. Verify end-to-end: `cargo run --bin assura -- check demos/libwebp-huffman.assura`

Every new pass must be called from the pipeline. Orphan code (compiles
but is never invoked) is a bug.

## Error Codes

Error codes follow the spec (Appendix D):

| Range | Category |
|-------|----------|
| A01xxx | Syntax errors (parser) |
| A02xxx | Name resolution errors |
| A03xxx | Type errors |
| A05xxx | Linearity errors |
| A06xxx | Typestate errors |
| A07xxx | Effect errors |
| A08xxx | Information flow errors |

Use `assura explain <code>` to look up any error code.

## Code Style

- `cargo fmt` is the formatter; do not deviate
- `cargo clippy -- -D warnings` must pass with zero warnings
- Use `pub(crate)` for internal visibility; `pub` only for cross-crate API
- No `unwrap()` in library code (OK in tests and CLI)
- Every AST node carries a `Span` for error reporting

## Documentation

- [Tutorial](docs/TUTORIAL.md) -- getting started
- [Specification](docs/SPECIFICATION.md) -- full language spec (11,800 lines)
- [Internals](docs/INTERNALS.md) -- architecture and crate details
- [Cookbook](docs/COOKBOOK.md) -- 25 ready-to-copy contract patterns
- [Scenario Guides](docs/SCENARIOS.md) -- practical walkthroughs
- [Roadmap](docs/ROADMAP.md) -- phased development plan

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE),
at your option. By contributing, you agree to license your contribution
under the same terms.