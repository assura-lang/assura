# Assura Compiler - Agent Instructions

## Project Overview

Assura is a contract-first AI-native language that transpiles to Rust.
Users write contracts (what code should do); AI generates implementations;
the compiler proves correctness via Z3/CVC5 SMT solvers; then `rustc`
compiles the generated Rust to native or WASM binaries.

- Full spec: `docs/SPECIFICATION.md` (11,800 lines, 195 EBNF productions,
  50 verification features, ~278 error codes)
- Competitive analysis: `docs/INVESTIGATION.md` (3,200 lines)
- Phased roadmap: `docs/ROADMAP.md` (752 lines)
- **Master plan**: `MASTER-PLAN.md` (the actionable task list, read this
  to know what to build next)

## Session Startup

At the start of every session:

1. Read `MASTER-PLAN.md` to find the next uncompleted task
2. Check which tasks are marked `[x]` (done) vs `[ ]` (pending)
3. Pick the next task whose dependencies are all `[x]`
4. Work on it until complete, then mark it `[x]` in `MASTER-PLAN.md`
5. Commit and push after completing each task
6. Continue to the next task until the session ends or context runs out
7. Before the session ends, update `MASTER-PLAN.md` with what was
   completed and any notes for the next session

If multiple independent tasks are available (no dependency between them),
work on them in the order listed unless parallelization with subagents
makes sense.

## Repository Structure

```
assura/
  Cargo.toml                  # Workspace root
  AGENTS.md                   # This file
  MASTER-PLAN.md              # Actionable task list with dependencies
  crates/
    assura-parser/            # Lexer (logos), parser (chumsky), AST
      src/
        lib.rs
        lexer.rs              # Token definitions, logos derive
        ast.rs                # AST node types
        parser.rs             # chumsky parser combinators
    assura-cli/               # CLI binary (assura check/build/init)
      src/
        main.rs               # Entry point, error reporting (ariadne)
    assura-resolve/           # [future] Name resolution, symbol table
    assura-types/             # [future] Type checker (Layer 0)
    assura-smt/               # [future] Z3/CVC5 integration (Layer 1-3)
    assura-codegen/           # [future] Rust code generation
  docs/
    SPECIFICATION.md          # Language specification (source of truth)
    INVESTIGATION.md          # Competitive analysis, architecture decisions
    ROADMAP.md                # High-level phased roadmap
    LANDING.md                # Marketing content
  demos/                      # Example .assura contract files
    libwebp-huffman.assura    # CVE-2023-4863 prevention demo
    zlib-inflate.assura       # CVE-2022-37434 prevention demo
    mbedtls-x509.assura       # 4 CVSS 9.8 CVE prevention demo
  tests/
    fixtures/                 # Test .assura files
      test_basic.assura
```

New crates are added as `crates/assura-{name}/`. Every crate uses
workspace-inherited version, edition, license, and repository fields.

## Build and Test

```bash
# Build everything
cargo build

# Run the parser CLI
cargo run -- demos/libwebp-huffman.assura
cargo run -- --ast demos/libwebp-huffman.assura
cargo run -- --tokens demos/libwebp-huffman.assura

# Run tests
cargo test --workspace

# Check formatting and lints
cargo fmt --check --all
cargo clippy --workspace -- -D warnings
```

Every change must pass `cargo build`, `cargo test --workspace`,
`cargo clippy --workspace -- -D warnings` before committing.

## Coding Conventions

### Rust

- Edition 2024
- Use `thiserror` for error types (add when needed)
- Use `#[derive(Debug, Clone, PartialEq)]` on AST nodes
- Every AST node carries a `Span` (source location)
- Use `pub(crate)` for internal visibility, `pub` only for cross-crate API
- No `unwrap()` in library code; `unwrap()` is OK in CLI/tests
- Prefer `Result<T, E>` over panics
- Write `#[test]` functions in the same file as the code they test
  (unit tests) or in `tests/` for integration tests

### Crate Versioning (CRITICAL)

These versions are load-bearing. The APIs change between majors.

| Crate | Version | Do NOT upgrade to |
|-------|---------|-------------------|
| chumsky | 0.9 | 0.10+ (completely different API) |
| ariadne | 0.4 | 0.5+ (different Report/Label API) |
| logos | 0.15 | stable, upgrades OK |

**chumsky 0.9 patterns**: `Parser<Token, Output, Error = Simple<Token>>`,
`Stream::from_iter()`, `parse_recovery()`, `filter_map()`,
`separated_by()`, `delimited_by()`, `map_with_span()`.

### Specification Compliance

The language specification is `docs/SPECIFICATION.md`. Every compiler
feature must implement exactly what the spec says:

- Grammar productions from Appendix A
- Type rules from Sections 2-3
- Error codes from Appendix D (format: Axxxxx)
- Verification layers from Section 5
- Codegen rules from Section 6 and Appendix C

When the spec is ambiguous, add a `// SPEC-QUESTION:` comment and
make a reasonable choice. Do not invent features not in the spec.

### Error Handling

Errors use structured codes from the spec:

- A01xxx: Syntax errors (parser)
- A02xxx: Name resolution errors
- A03xxx: Type errors
- A05xxx: Linearity errors
- A06xxx: Typestate errors
- A07xxx: Effect errors
- A08xxx: Information flow errors

Each error includes: code, location, message, optional secondary
locations, optional suggested fix.

Output modes:
- `--human` (default): Rich terminal diagnostics via ariadne
- `--json`: Structured JSON per Section 7.3 of the spec

### Testing Strategy

- **Snapshot tests**: Parse .assura files, serialize AST, compare to
  golden files. Use `insta` crate.
- **Error tests**: .assura files with `// MUST REJECT Axxxxx` annotations
  that must produce the specified error code.
- **Pass tests**: .assura files with `// MUST COMPILE` that must parse
  and type-check without errors.
- **Integration tests**: Each type interaction test case from Section 13
  of the spec.
- **Demo tests**: All files in `demos/` must parse and (eventually)
  verify without errors.

### Commit Messages

Format: `<scope>: <description>`

Scopes: `parser`, `resolve`, `types`, `smt`, `codegen`, `cli`, `docs`,
`tests`, `ci`, `deps`

Examples:
- `parser: handle refinement types in field definitions`
- `resolve: implement symbol table and scope analysis`
- `types: add base type checker for Int, Nat, Float, Bool`
- `smt: initial Z3 bindings and refinement type encoding`
- `codegen: generate debug_assert! from requires clauses`

### License

MIT OR Apache-2.0 (dual license, Rust ecosystem standard).
Both `LICENSE-MIT` and `LICENSE-APACHE` files must exist at repo root.

## Architecture Decisions

These are final. Do not revisit without explicit discussion.

| Decision | Choice | Reference |
|----------|--------|-----------|
| Compiler language | Rust | docs/INVESTIGATION.md |
| Lexer | logos 0.15 | Fast, derive macro |
| Parser | chumsky 0.9 combinators | NOT hand-rolled, NOT 0.10 |
| Error display | ariadne 0.4 | Colored spans |
| SMT solver | Z3 primary (z3 crate), CVC5 fallback | docs/ROADMAP.md |
| Codegen target | Rust source via prettyplease | NOT syn/quote |
| Codegen output | `generated/` dir as Cargo workspace | Section 10.3 of spec |

## What NOT To Do

- Do not add features not in SPECIFICATION.md
- Do not upgrade chumsky past 0.9 or ariadne past 0.4
- Do not use `syn`/`quote` for codegen (they're for proc macros)
- Do not build a hand-rolled parser (chumsky is the decision)
- Do not use tree-sitter as the compiler parser (it's error-tolerant,
  the compiler needs exact parses; tree-sitter is for editor support)
- Do not skip tests; every new feature needs test coverage
- Do not commit code that fails `cargo clippy -- -D warnings`
