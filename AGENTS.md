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
    assura-parser/            # Lexer (logos), parser (rowan CST + lowering), AST
      src/
        lib.rs                # Public parse() entry point
        lexer.rs              # Token definitions, logos derive
        ast.rs                # AST node types
        syntax_kind.rs        # SyntaxKind enum (rowan Language trait)
        cst.rs                # Parser engine, events, GreenNode builder
        lower.rs              # CST -> AST lowering
        grammar/              # Recursive descent grammar
          mod.rs              # source_file, project, module, import
          items.rs            # contract, type, enum, fn, service, extern
          clauses.rs          # requires, ensures, invariant, effects, etc.
          expressions.rs      # Pratt expression parser (8 precedence levels)
          params.rs           # param_list, return_type, type_params
    assura-cli/               # CLI binary (assura check/build/init/explain)
      src/
        main.rs               # Entry point, error reporting (ariadne)
    assura-resolve/           # Name resolution, symbol table, scopes
      src/
        lib.rs
    assura-types/             # Type checker (Layer 0): 50+ domain checkers
      src/
        lib.rs
    assura-smt/               # Z3 SMT integration (Layer 1-3), IR, caching
      src/
        lib.rs
    assura-codegen/           # Rust code generation, backend config
      src/
        lib.rs
    assura-lsp/               # LSP server (tower-lsp)
      src/
        lib.rs
        main.rs
    assura-server/            # gRPC (tonic) + HTTP (axum) API server
      proto/
        assura.proto
      src/
        main.rs
  docs/
    SPECIFICATION.md          # Language specification (source of truth)
    INVESTIGATION.md          # Competitive analysis, architecture decisions
    ROADMAP.md                # High-level phased roadmap
    LANDING.md                # Marketing content
    TUTORIAL.md               # Getting started tutorial
    INTERNALS.md              # Architecture and internals documentation
  demos/                      # Example .assura contract files
    libwebp-huffman.assura    # CVE-2023-4863 prevention demo
    zlib-inflate.assura       # CVE-2022-37434 prevention demo
    mbedtls-x509.assura       # 4 CVSS 9.8 CVE prevention demo
  editors/
    vscode/                   # VS Code extension (TextMate + LSP)
    tree-sitter-assura/       # tree-sitter grammar for editors
  tests/
    fixtures/                 # Test .assura files
      test_basic.assura
    e2e/                      # End-to-end verification test contracts
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
| rowan | 0.16 | stable, upgrades OK |
| ariadne | 0.4 | 0.5+ (different Report/Label API) |
| logos | 0.15 | stable, upgrades OK |

**rowan 0.16 patterns**: `GreenNodeBuilder`, `SyntaxNode::new_root()`,
`Language` trait on `AssuraLanguage`, `SyntaxKind` enum with `From<u16>`.
The parser uses an events/markers pattern (Open/Close/Advance) with
Pratt parsing for expressions.

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

**Pipeline test trap**: Helpers like `codegen_ok` and `type_check_source`
run the FULL compiler pipeline (parse -> resolve -> type check -> codegen).
Test inputs must be valid for ALL phases, not just the phase being tested.
Concretely:

- **Effect names must be from the known set.** The type checker rejects
  unknown effects (A07003). Valid names: `io`, `database`, `logging`,
  `mem`, `net`, `fs`, `rng`, `time`, `alloc`, `diverge`, `random`,
  and dotted sub-effects like `console.read`, `filesystem.write`,
  `network.connect`, `database.read`, `log.info`, etc. Do NOT use
  made-up names like `memory` or `compute`.
- **Type names must be valid.** Use `Int`, `Nat`, `Float`, `Bool`,
  `String`, `Bytes`, `Unit`, or generic types like `List<Int>`.
- **Contracts need at least a `requires` clause** to produce meaningful
  codegen output (a `debug_assert!` to test against).

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
| Parser | rowan 0.16 CST + hand-written recursive descent | Lossless CST, Pratt expressions |
| Error display | ariadne 0.4 | Colored spans |
| SMT solver | Z3 primary (z3 crate), CVC5 fallback | docs/ROADMAP.md |
| Codegen target | Rust source via prettyplease | NOT syn/quote |
| Codegen output | `generated/` dir as Cargo workspace | Section 10.3 of spec |

## Integration Rule: No Orphan Code

**Every new compiler pass must be wired into the CLI pipeline in the
same task that creates it.** Do not create crates that compile but are
never called.

The pipeline is a chain. After each task, verify the chain works
end-to-end by running `cargo run -- demos/libwebp-huffman.assura`:

```
CLI main.rs
  -> assura-parser::parser::source_file()   # parse
  -> assura-resolve::resolve()              # name resolution (T009+)
  -> assura-types::type_check()             # type checking (T013+)
  -> assura-smt::verify()                   # SMT verification (T038+)
  -> assura-codegen::codegen()              # Rust code generation (T019+)
```

**Concrete rules:**

1. When you create `assura-resolve` (T009), update `assura-cli/src/main.rs`
   to call `resolve()` after parsing. If resolution finds errors, print
   them and exit 1. Verify by running a demo file through the CLI.

2. When you create `assura-types` (T013), update `main.rs` to call
   `type_check()` after resolution. Same pattern.

3. When you create `assura-smt` (T038), update `main.rs` to call
   `verify()` after type checking. Same pattern.

4. When you create `assura-codegen` (T019), update `main.rs` to call
   `codegen()` and write output. Same pattern.

**Validation after every new pass**: Run this and verify the output
changes (new errors reported, new output produced, etc.):

```bash
cargo run -- demos/libwebp-huffman.assura
cargo run -- --ast demos/libwebp-huffman.assura
```

If the output is identical to before you added the pass, the pass is
not wired in. Fix it before marking the task done.

**Test that the passes interact**: Each new pass must have at least one
integration test that feeds the output of the previous pass into the
new pass. Unit tests of the pass in isolation are necessary but not
sufficient. The test must prove the pipeline works, not just the crate.

Example: a `resolve` test must start from a parsed `SourceFile` (not
hand-built AST), and a `type_check` test must start from a resolved
file (not hand-built resolved AST).

## Pre-Commit Gate

Run this exact command before every commit. No exceptions.

```bash
cargo fmt --all && cargo clippy --workspace -- -D warnings && cargo test --workspace
```

If any step fails, fix it before committing. Do not commit with
`--no-verify` or skip tests. If a test is flaky, fix the test.

After committing, verify the commit is clean:

```bash
cargo run -- demos/libwebp-huffman.assura
cargo run -- demos/zlib-inflate.assura
cargo run -- demos/mbedtls-x509.assura
cargo run -- tests/fixtures/test_basic.assura
```

All four files must parse successfully. If a parser change breaks any
demo file, the change is wrong. Fix it before pushing.

## Task Completion Criteria

A task in MASTER-PLAN.md is done when ALL of these are true:

1. The code compiles: `cargo build`
2. All tests pass: `cargo test --workspace`
3. No warnings: `cargo clippy --workspace -- -D warnings`
4. All demo files still parse: run all four
5. New code has tests (unit tests in the same file, integration tests
   in `tests/`)
6. MASTER-PLAN.md is updated: task marked `[x]`, session note added
7. Changes are committed and pushed

Do not mark a task `[x]` if any of these are false.

## Issue Closure Discipline (CRITICAL)

Issues have been closed with zero acceptance criteria checked. This is
unacceptable. The following rules are mandatory:

### Never close an issue with unchecked acceptance criteria

If an issue has checkboxes in its body (e.g., `- [ ] Feature X works`),
every checkbox must be checked (`- [x]`) before closing. If you cannot
complete an acceptance criterion, leave the issue open and comment
explaining what is blocked.

**Mechanical verification before closing any issue:**

```bash
# 1. Verify the project compiles
cargo build

# 2. Verify all tests pass
cargo test --workspace

# 3. Read the issue body and check each criterion
gh issue view <number> --json body --jq '.body' | grep -c '\- \[ \]'
# If this returns > 0, the issue is NOT ready to close

# 4. Only then close
gh issue close <number>
```

### Never close an issue without running the acceptance tests

If an acceptance criterion says "test X exists and passes," you must:
1. Run the specific test: `cargo test test_name`
2. See it pass in the terminal output
3. Only then check the checkbox

Saying "I added the test" is not the same as "the test passes." Run it.

### Module extraction requires per-module test verification

When extracting code from a monolith into separate modules:
1. Count `#[test]` functions in the source BEFORE extraction
2. After extraction, count `#[test]` functions in EACH new module
3. Any module with zero tests is incomplete
4. Every extracted module must have at least one direct test

This rule exists because the SMT module extraction created 10 new files
with zero tests each, leaving all 205 tests in the original lib.rs.

### Never commit code that breaks `cargo build`

This already exists in the Pre-Commit Gate section but was violated.
Restating for emphasis: if `cargo build` fails after your changes,
your changes are wrong. Fix them before committing. No exceptions.

### The build must compile AFTER every commit, not just before

When making a series of commits (e.g., extracting modules one by one),
each individual commit must leave the project in a compilable state.
Run `cargo build` after each `git commit`, not just at the end.

### What "done" means for each issue type

| Issue type | "Done" means |
|------------|-------------|
| Feature | Code exists, tests pass, acceptance criteria checked, demo works |
| Bug fix | Bug is reproducible before fix, fix applied, test proves it, no regression |
| Refactoring | Before and after produce identical behavior, all tests pass, no new modules with zero tests |
| Tech debt | Each listed item has implementation AND test, all checkboxes checked |

## Z3 Installation

The `assura-smt` crate (T038+) depends on the `z3` Rust crate, which
needs libz3 installed on the system.

```bash
# macOS
brew install z3

# Ubuntu/Debian
sudo apt-get install -y libz3-dev

# Verify
z3 --version
```

The `z3` Rust crate version should be `0.12` (latest stable with good
API). Pin it in Cargo.toml. The crate links against libz3 at build
time; if it can't find it, set `Z3_SYS_Z3_HEADER` and `LD_LIBRARY_PATH`.

For CI (T029), add this to the GitHub Actions workflow:

```yaml
- name: Install Z3
  run: sudo apt-get install -y libz3-dev
```

## Spec Navigation Guide

The spec (`docs/SPECIFICATION.md`) is 11,800 lines. Do NOT read it
all. Use this index to find what you need:

| Topic | Spec Section | What's There |
|-------|-------------|--------------|
| Grammar (EBNF) | Section 1, Appendix A | All 195 productions |
| Keywords | Section 1.1, Appendix A | All ~199 keywords |
| Type system | Sections 2.1-2.9 | Base types, refinement, linear, typestate, effects, info-flow |
| Effect system | Section 3 | Effect rows, hierarchy, handlers |
| Implementation IR | Section 4 | What AI generates (not contract language) |
| SMT encoding | Section 5 | Layer 1-3 strategies, theories, counterexamples |
| Rust codegen | Section 6, Appendix C | Type mapping, contract codegen, Cargo output |
| Error codes | Section 7, Appendix D | All ~278 error codes with descriptions |
| Module system | Section 8 | Imports, paths, visibility |
| Standard library | Section 9 | Built-in types, collection contracts |
| CLI and config | Section 10 | Commands, assura.toml, output modes |
| AI Agent API | Section 11 | gRPC service definition |
| Verification layers | Section 12 | Layer 0-3 boundaries, timeouts |
| Type interactions | Section 13 | 11 test cases for pairwise feature interactions |
| Feature categories | Section 14 | All 50 features: MEM, SEC, TYPE, CONC, FMT, etc. |

When working on a task, read ONLY the spec sections listed in that
task's description. Grep the spec for specific keywords rather than
scrolling.

## Debugging Strategy

When the parser (or any compiler pass) fails on an .assura file:

1. **Binary search on file size**: Comment out the bottom half, see if
   it parses. Narrow to the failing region.
2. **Minimal reproduction**: Extract the smallest .assura snippet that
   triggers the failure. Put it in `tests/fixtures/` as a regression test.
3. **Token dump**: Run `cargo run -- --tokens file.assura` to see what
   the lexer produces. The issue might be a missing keyword token.
4. **AST dump**: Run `cargo run -- --ast file.assura` to see what the
   parser produces (may show partial results with `parse_recovery`).
5. **Fix, test, commit**: Fix the issue, add the minimal reproduction
   as a test, verify all demos still pass, commit.

This binary-search approach was used to find and fix 12 parser edge
cases during initial development. It works.

## Reference Implementations

Study these open-source projects when working on specific phases.
Do not copy code; study patterns and approaches.

| Phase | Project | What to Study |
|-------|---------|--------------|
| Parser/AST | [Gleam](https://github.com/gleam-lang/gleam) | Rust compiler that transpiles to Erlang. Parser structure, AST design, codegen to another language. |
| Name resolution | [rust-analyzer](https://github.com/rust-lang/rust-analyzer) | How `hir-def` builds name resolution and scopes. |
| Type checker | [Gleam](https://github.com/gleam-lang/gleam) | Type inference, generic instantiation, error reporting. |
| Z3 encoding | [Verus](https://github.com/verus-lang/verus) | Direct Z3 encoding for Rust verification. Study `source/vir/src/sst_to_air.rs`. |
| Z3 encoding | [Dafny](https://github.com/dafny-lang/dafny) | Boogie-to-Z3 encoding. Study `Source/DafnyCore/Verifier/`. |
| Refinement types | [Liquid Haskell](https://github.com/ucsd-progsys/liquidhaskell) | SMT-based refinement type checking. The original. |
| Linear types | [Rust (rustc)](https://github.com/rust-lang/rust) | Borrow checker is a form of linearity checking. |
| Effect system | [Koka](https://github.com/koka-lang/koka) | Row-polymorphic effects. Study effect inference. |
| Codegen | [Gleam](https://github.com/gleam-lang/gleam) | How Gleam generates Erlang/JavaScript source. |
| LSP | [rust-analyzer](https://github.com/rust-lang/rust-analyzer) | Gold standard for Rust LSP implementation. |

## Adding New Crates

When a task requires a new crate:

1. Create `crates/assura-{name}/`
2. Create `Cargo.toml` using workspace inheritance:
   ```toml
   [package]
   name = "assura-{name}"
   description = "..."
   version.workspace = true
   edition.workspace = true
   license.workspace = true
   repository.workspace = true

   [dependencies]
   assura-parser = { path = "../assura-parser" }
   # other deps
   ```
3. Create `src/lib.rs` with public API
4. The workspace Cargo.toml auto-discovers via `members = ["crates/*"]`
5. Verify: `cargo build` succeeds with the new crate

## Span Propagation

Every compiler pass must propagate source locations. This is critical
for error reporting. The pattern:

- **Parser**: Every AST node is wrapped in `Spanned<T>` which carries
  a `Span = Range<usize>` (byte offsets into the source).
- **Name resolution**: `ResolvedFile` preserves spans from the AST.
  Every `Symbol` in the symbol table stores the span of its definition.
- **Type checker**: `TypeError` includes the span where the error
  occurred, plus optional secondary spans (e.g., "expected type
  declared here" pointing to the type definition).
- **SMT verification**: `VerificationResult` includes the span of the
  contract clause being verified, so counterexamples can point to the
  exact `requires` or `ensures` clause.
- **Codegen**: Generated Rust code includes comments with source
  locations: `// from contract Foo, line 42`.

If you add a new compiler pass and it produces errors without spans,
that's a bug.

## Expression Parser

The expression parser uses Pratt parsing (binding power) implemented
in `grammar/expressions.rs`. It produces `Expr` AST nodes with full
operator precedence.

**Binding power levels** (lowest to highest):

1. `||` (logical or) - BP 1
2. `&&` (logical and) - BP 3
3. `==`, `!=` (equality) - BP 5
4. `<`, `>`, `<=`, `>=` (comparison) - BP 7
5. `+`, `-` (additive) - BP 9
6. `*`, `/`, `%`, `mod` (multiplicative) - BP 11
7. `!`, `-` (unary prefix)
8. `.` field access, `()` function call, `[]` index (postfix)

The expression parser also handles quantifiers (`forall`, `exists`),
`if/then/else`, `old()`, `result`, `match`, and `let` expressions.

**Key files**: `grammar/expressions.rs` (Pratt parser), `ast.rs`
(`Expr` enum with 22 variants), `lower.rs` (CST EXPR nodes to AST).

## Soundness Testing

The type checker and verifier must be **sound**: if the compiler says
"verified," the contract must actually hold. Unsoundness = the compiler
accepts buggy code. This is the worst kind of bug.

**How to test soundness**:

1. **Positive tests** (`// MUST COMPILE`): Valid contracts that must
   type-check and verify. Verify the generated Rust actually compiles.
2. **Negative tests** (`// MUST REJECT Axxxxx`): Invalid contracts that
   must be rejected with a specific error code. If the compiler accepts
   them, that's unsoundness.
3. **Counterexample tests**: Contracts with known counterexamples.
   Verify the counterexample Z3 produces matches the expected one.
4. **Adversarial tests**: Contracts designed to trick the compiler into
   accepting unsound code. These come from Section 13 (type interaction
   test cases) and from known unsoundness bugs in Dafny/Verus.
5. **Fuzzing**: Use `cargo-fuzz` to generate random .assura files.
   The parser should never panic (only return errors). The type checker
   should never crash. The verifier should never produce "verified" for
   a contract that has a counterexample.

**The most common unsoundness sources**:
- Ghost code affecting runtime values (violation of erasure)
- Linear variable used in refinement predicate counted as a use
- Typestate transition not checked on all control flow paths
- Effect containment bypassed via higher-order functions
- Z3 timeout silently treated as "verified" instead of "unknown"

## What NOT To Do

- Do not add features not in SPECIFICATION.md
- Do not upgrade ariadne past 0.4
- Do not use `syn`/`quote` for codegen (they're for proc macros)
- Do not use tree-sitter as the compiler parser (it's error-tolerant,
  the compiler needs exact parses; tree-sitter is for editor support)
- Do not skip tests; every new feature needs test coverage
- Do not commit code that fails `cargo clippy -- -D warnings`
- Do not treat Z3 `Timeout` or `Unknown` as `Verified`; they are
  distinct results that must be reported to the user
- Do not silently swallow errors; every error must have a span and code
- Do not add `#[allow(unused)]` to suppress warnings on code that
  should be used; find and fix the actual issue
- Do not make AST changes without updating all downstream passes;
  if you change `ast.rs`, grep for every usage
