# Assura Compiler - Agent Instructions

## LLM / agent: read this first (ergonomics map)

Skim this section before changing compiler code. It encodes the patterns that
prevent the most common agent mistakes (orphan checkers, hand-rolled verify,
wrong test helpers, wrong Unknown policy).

### Canonical entry points (do not re-implement)

| Goal | Use this | Not this |
|------|----------|----------|
| Parse only | `assura_parser::parse` / `parse_unwrap` | hand-built CST |
| Full pipeline (no SMT) | `assura_pipeline::compile` | copy-paste resolve+type_check in callers |
| Full pipeline + SMT + codegen | `assura_pipeline::compile_full` | ad-hoc `Verifier` chains in CLI/LSP/MCP |
| SMT on already-typed file | `assura_pipeline::verify_typed(&typed, path, &config)` | `Verifier::new(...).parallel().with_decrease_checks()` outside smt/pipeline |
| Tests (ok / err / codes) | `assura_test_support::*` (`typecheck_ok`, `typecheck_err`, `verify_ok`, `verify_strict_ok`, `expect_error_codes`) | hand-roll parse→resolve→type_check in every test |
| Walk all decls | `assura_ast::walk_decls` + `DeclVisitor` | copy 20-arm `match Decl` blocks |
| New type checker | implement `run_*_checks` in `crates/assura-types/src/checks/`, register in `CHECKER_PIPELINE` in `pipeline.rs` | struct + unit tests only (orphan / dead code) |
| Known SMT limitation? | `assura_smt::is_known_smt_limitation(reason)` or `KNOWN_SMT_LIMITATION_MARKER` | open-code `"not yet encoded in SMT"` with a different string |

### Fast agent commands (prefer over full workspace test)

```bash
# Static anti-pattern greps (Verifier::new, Type::Unknown ==, pipeline size)
bash scripts/agent-guards.sh

# fmt + guards + clippy on key crates + one demo
bash scripts/agent-preflight.sh
bash scripts/agent-preflight.sh assura-types assura-smt   # subset

# Decl variant touch list (grep sites; then cargo build for non-exhaustive)
bash scripts/check-decl-variant.sh

# Targeted compile/test (agent tools with short timeouts)
cargo check -p assura-types --locked
cargo test -p assura-types --locked --lib
cargo clippy -p assura-types --lib --locked -- -D warnings
```

Full `cargo test --workspace --locked` is for session end / pre-push, not every edit.

### Pipeline invariants agents must respect

1. **`CompilationOutput.has_errors`** reflects parse / resolve / type only.
   SMT counterexamples live in `output.verification` and do **not** set `has_errors`.
2. **Success checks for verify:**
   - lenient (tests/MCP): `assura_pipeline::verification_succeeded`
   - strict (`// MUST VERIFY` style): `assura_pipeline::verification_strict_succeeded`
3. **`Unknown` with marker** `"not yet encoded in SMT"` is a **warning** in CLI
   (exit 0), not a hard failure. Do not invent alternate marker strings when
   emitting `VerificationResult::Unknown`.
4. **`Type::Error` vs `Type::Unknown`**: always use `ty.is_indeterminate()`,
   never `ty == Type::Unknown` (misses `Error` and causes cascade false positives).
5. **`codegen_ok` in assura-codegen tests**: do not call `assura_test_support::codegen_ok`
   from inside `assura-codegen` (dependency type mismatch). Use `typecheck_ok` then
   local `codegen(&typed)`.
6. **New `run_*_checks`**: add to `CHECKER_PIPELINE` in the same PR; breadth test
   and `agent-guards.sh` will flag a too-small registry.

### Crate map (where to edit)

```
assura-ast        # Expr, Decl, DeclVisitor, features registry
assura-parser     # lexer, CST, grammar, lower -> AST
assura-resolve    # names, scopes
assura-types      # Layer 0: TypeChecker, checks/*, pipeline.rs CHECKER_PIPELINE
assura-smt        # Layer 1-3: Verifier, IR, Z3/CVC5 backends, result.rs
assura-codegen    # Rust source generation
assura-pipeline   # compile / compile_full / verify_typed (canonical glue)
assura-config     # CompilerConfig, VerifyOptions (incl. for_tests())
assura-test-support  # test helpers only (dev-dep from other crates)
assura-cli / lsp / server / mcp  # frontends; must call pipeline, not re-chain passes
```

### Adding a `Decl` variant

High blast radius (17+ match sites). Run `bash scripts/check-decl-variant.sh`,
then `cargo build` and fix every non-exhaustive match. Full checklist is in
**Adding a New Decl Variant** below.

### Spec and tasks

- Language source of truth: `docs/SPECIFICATION.md` (grep; do not read all 11k lines).
- Actionable work: `MASTER-PLAN.md` (acceptance tests are mandatory, not optional).
- Coverage of 50 verification features: `bash scripts/verify-task.sh SEC.1` (example).

---

## Grok Filename Bug Workaround

This file is named `AGENTS.md` in git. Grok scans for project rules
in this order: `Agents.md`, `Claude.md`, `CLAUDE.md`, `CLAUDE.local.md`,
`AGENT.md`, `AGENTS.md`. On macOS case-insensitive APFS, the pattern
`Agents.md` matches the actual file `AGENTS.md` first, so the
system-reminder reports the path as `Agents.md` instead of the real
filesystem name `AGENTS.md`. This causes `search_replace` and
`read_file` to use the wrong casing.

**Workaround**: Always use `AGENTS.md` (all caps) when editing or
reading this file, regardless of what the system-reminder header says.
The git-tracked name is `AGENTS.md`. Do not rename it.

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

1. **If the session involves code changes**:
   - **Inside an agent tool / Grok CLI** (or any environment with short command timeouts):
     Use fast targeted commands. Never run the full suite if it will time out.
     Preferred: `bash scripts/agent-preflight.sh` (or subset), then
     `cargo check -p <crate> --locked`, `cargo test -p <crate> --locked --lib`.
     Run `bash scripts/agent-guards.sh` after touching verify/checkers/types.
   - **On a local developer machine**:
     Run `cargo test --workspace --locked` in the background while reading the task.
     Do not block on it; start working and check the result before committing.
   **If the session is read-only** (code review, analysis, questions):
   Skip the test suite entirely. Reading code does not require a green
   build first.
2. Read `MASTER-PLAN.md` to find the next uncompleted task.
3. Check which tasks are marked `[x]` (done) vs `[ ]` (pending).
4. Pick the next task whose dependencies are all `[x]`.
5. Read that task's **Acceptance Tests** section carefully before
   writing any code. Know what "done" looks like before you start.
6. Implement the task.
7. Run every acceptance test command from the task. See each one pass.
8. Before each push and before session end, run manual checks:
   cargo fmt --check, cargo clippy --workspace --locked -- -D warnings,
   cargo check -p <crate> --locked,
   and cargo test --workspace --locked (full) or targeted tests as needed.
   Using `--locked` prevents Cargo from modifying Cargo.lock unnecessarily.
9. Mark the task `[x]` in `MASTER-PLAN.md`. Commit and push.
10. Continue to the next task until the session ends or context runs out.
11. Before the session ends, update the Progress Notes section with
    what was completed and what to do next.

If multiple independent tasks are available (no dependency between them),
work on them in the order listed unless parallelization with subagents
makes sense.

## Repository Structure

```
assura/
  Cargo.toml                  # Workspace root (members = crates/*)
  AGENTS.md                   # This file
  MASTER-PLAN.md              # Actionable task list with dependencies
  crates/
    assura-ast/               # Canonical AST IR: Decl, Expr, Spanned, visitors
      src/ast/mod.rs          # ExprVisitor/ExprFolder, DeclVisitor/DeclFolder
      src/features.rs         # Registry of all 50 verification features
    assura-parser/            # Lexer (logos), parser (rowan CST + lowering)
      src/grammar/            # Recursive descent (items, clauses, expressions, params)
      src/lower/              # CST -> AST lowering
      src/ast.rs              # Re-export of assura-ast (backward compat)
    assura-resolve/           # Name resolution, symbol table, imports, project roots
    assura-types/             # Layer 0 type checker + domain checkers
      src/checkers/           # effects, linear, taint, security/*, etc.
      src/checks/             # run_*_checks entry points wired into pipeline
      src/domain/             # Domain checker structs
      src/pipeline.rs         # TypeChecker orchestration
    assura-smt/               # Layer 1-3 SMT (Z3/CVC5), IR, cache, measures
      src/entry/              # Verifier builder API (apply_options / verify)
      src/z3_backend/         # Z3 encoder + verify
      src/cvc5_backend/       # CVC5 shell + native (feature-gated)
    assura-codegen/           # Rust codegen (prettyplease), multi-file projects
    assura-pipeline/          # CANONICAL compile path: compile / compile_full / verify_typed / run_at
    assura-config/            # ProjectConfig, CompilerConfig, VerifyOptions
    assura-diagnostics/       # Error codes, Diagnostic, ariadne/JSON rendering
    assura-cli/               # Binary: check/build/init/diff/fmt/repl/audit/...
      src/check.rs, build.rs  # Use assura_pipeline (not hand-rolled passes)
      src/shared.rs           # compile / compile_with_config wrappers
    assura-fmt/               # Formatter
    assura-lsp/               # LSP server (tower-lsp)
    assura-mcp/               # MCP server (rmcp) for agent tools
    assura-server/            # gRPC (tonic) + HTTP (axum)
    assura-macros/            # Proc macros
    assura-stdlib/            # Standard library .assura contracts
    assura-rust-analyzer/     # RA integration helpers
    assura-bench/             # Criterion benchmarks
    assura-test-support/      # Shared test helpers (parse_ok, typecheck_ok, verify_ok)
  docs/                       # SPECIFICATION, INVESTIGATION, ROADMAP, TUTORIAL, INTERNALS
  demos/                      # CVE-prevention example contracts
  templates/                  # AI contract-generation prompt templates
  editors/vscode/             # VS Code extension
  tests/fixtures/             # MUST COMPILE / MUST REJECT fixtures
  tests/e2e/                  # End-to-end verification contracts
  scripts/                    # verify-task.sh, setup-cvc5.sh, CI helpers
```

New crates are added as `crates/assura-{name}/`. Every crate uses
workspace-inherited version, edition, license, and repository fields.

### Agent ergonomics (Tier 1 conventions)

**Single pipeline.** All entry points (CLI, LSP, server, MCP, tests) should
go through `assura_pipeline::{compile, compile_full, verify_typed, run_at}`.
Do not re-implement parse -> resolve -> typecheck -> verify in a new crate.

**Verify options.** `assura_config::VerifyOptions` is the source of truth
for solver, timeout, layer, parallel, decrease_checks, and enable_cache.
Pass it via `CompilerConfig` or `Verifier::apply_options(opts)`. Defaults
match historical CLI behavior (`parallel: true`, `decrease_checks: true`).
Use `VerifyOptions::for_tests()` for fast unit tests.

**AST visitors.** Prefer `ExprVisitor`/`ExprFolder` and `DeclVisitor`/
`DeclFolder` in `assura-ast` over open-coding large `match` blocks on
`Expr`/`Decl` in every pass. `Decl::name()`, `Decl::clauses()`, and
`Decl::summary_label()` cover the common accessors.

**Test helpers.** Prefer `assura_test_support::{parse_ok, resolve_ok,
typecheck_ok, compile_ok, verify_ok, codegen_ok}` over duplicating pipeline
shims in each crate's `tests/` module. `assura-types` tests delegate
`resolve_ok` through it; `assura-codegen` tests use `codegen_ok`.

**Domain checkers (`assura-types/src/checks/`).** Prefer helpers in
`checks/mod.rs` (`clauses_contract_fn`, `clauses_contract_fn_block`,
`fn_or_contract_name_clauses`, `runtime_decl_clauses_params`) or
`Decl::clauses()` / `Decl::name()` over open-coding `match &decl.node`
for every contract/fn/block triple.

## Build and Test

```bash
# Build everything
cargo build

# Run the CLI (check subcommand)
cargo run --bin assura -- check demos/libwebp-huffman.assura
cargo run --bin assura -- check demos/libwebp-huffman.assura --verbose
cargo run --bin assura -- check demos/libwebp-huffman.assura --stats

# Run tests
cargo test --workspace --locked

# Check formatting and lints
cargo fmt --check --all
cargo clippy --workspace --locked -- -D warnings
```

Every change must pass `cargo build`, `cargo clippy --workspace --locked -- -D warnings`
before committing.

For full test coverage use `cargo test --workspace --locked` (local machine or end-of-session).
Inside an agent tool with timeouts, use targeted verification instead:
`cargo test -p <crate> --locked --lib`, `cargo check -p <crate> --locked`.

**Important for changes touching the main executable or cli_integration:**
After edits to cli_integration.rs, temp dir handling, or anything that affects
the `assura` binary build (CARGO_BIN_EXE_assura), always run the *full*
`cargo test --workspace --locked` (not just the targeted integration test) before
committing or declaring done. The targeted test only exercises part of the
suite; the workspace run validates all crates + the complete executable with
every dependency enabled. See issues #328.

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

Keep dependencies up to date. Run `cargo outdated -R` periodically.

| Crate | Version | Notes |
|-------|---------|-------|
| rowan | 0.16 | stable, upgrades OK |
| ariadne | 0.6 | Report::build takes (kind, span) with 2 args; span is (Id, Range) |
| logos | 0.16 | stable, upgrades OK |
| z3 | 0.20 | No lifetime params on AST types; no &ctx first arg; pre-generated FFI bindings |
| sha2 | 0.11 | Uses digest 0.11, high-level API unchanged |
| cvc5 | 0.4 | Native FFI bindings; `Sort` not Copy; `Kind` names differ from SMT-LIB2; requires `features = ["static"]` for static linking |

**rowan 0.16 patterns**: `GreenNodeBuilder`, `SyntaxNode::new_root()`,
`Language` trait on `AssuraLanguage`, `SyntaxKind` enum with `From<u16>`.
The parser uses an events/markers pattern (Open/Close/Advance) with
Pratt parsing for expressions.

**z3 0.20 patterns**: No lifetime params (`Bool`, not `Bool<'ctx>`).
No `&ctx` first arg on constructors (`Int::from_i64(n)`, not
`Int::from_i64(&ctx, n)`). Use `.eq()` not `._eq()`. Context
created via `z3::with_z3_config(&cfg, || { ... })`.

**cvc5 0.4 patterns**: `TermManager` is the factory for all sorts,
terms, and operators. `Solver::new(&tm)` borrows the TermManager
(lifetime tied). `Sort` is NOT Copy; call `tm.integer_sort()` each
time instead of reusing a variable across loop iterations. `Kind`
enum names differ from SMT-LIB2: `IntsDivision` (not `IntsDiv`),
`IntsModulus` (not `IntsMod`). Function sorts: `tm.mk_fun_sort()`
(not `mk_function_sort`). Quantifier bound variables:
`tm.mk_var(sort, name)` for bound vars, `tm.mk_const(sort, name)`
for free constants. Wrap bound vars in `VariableList` kind for
`Forall`/`Exists`: `tm.mk_term(Kind::VariableList, &[bound_var])`.
Static linking: `features = ["static"]` on the cvc5 dep in
Cargo.toml links cadical, picpoly, gmp statically.

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
  that must produce the specified error code. The CLI harness
  (`test_must_reject_fixtures` in `assura-cli/src/diff.rs`) scans
  `tests/fixtures/errors/` and `tests/fixtures/must_reject/`. Fixtures may
  include `// BLOCKED: <reason>` (ideally with a GitHub issue number) to
  skip execution; the harness logs skipped paths and fails if the blocked
  count exceeds `MAX_BLOCKED_MUST_REJECT` (currently 0, see #349). Do not
  block a fixture to get CI green without filing an issue.
- **Pass tests**: .assura files with `// MUST COMPILE` that must parse
  and type-check without errors. The CLI harness only scans
  `tests/fixtures/must_compile/` (`test_must_compile_fixtures` in
  `assura-cli/src/diff.rs`) — not arbitrary paths under `tests/fixtures/`.
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
| Lexer | logos 0.16 | Fast, derive macro |
| Parser | rowan 0.16 CST + hand-written recursive descent | Lossless CST, Pratt expressions |
| Error display | ariadne 0.6 | Colored spans |
| SMT solver | Z3 primary (z3 crate), CVC5 fallback | docs/ROADMAP.md |
| Codegen target | Rust source via prettyplease | NOT syn/quote |
| Codegen output | `generated/` dir as Cargo workspace | Section 10.3 of spec |

## Integration Rule: No Orphan Code

**Every new compiler pass must be wired into the shared pipeline in the
same task that creates it.** Do not create crates that compile but are
never called.

The canonical chain lives in `assura-pipeline` (not hand-rolled in CLI/LSP/server):

```
assura_pipeline::compile / compile_full / verify_typed / run_at
  -> assura_parser::parse_full()              # lex + parse
  -> assura_resolve::resolve()                # name resolution
  -> assura_types::TypeChecker::check()       # Layer 0 (+ multi-file via types APIs)
  -> assura_smt::Verifier::apply_options()    # SMT (via verify_typed / compile_full)
  -> assura_codegen::codegen()                # Rust generation (compile_full / build)
```

CLI (`check.rs`, `build.rs`), server, MCP, and LSP should call these
helpers (or thin wrappers in `assura-cli/src/shared.rs`), not rebuild
the chain. Verification options always come from
`assura_config::VerifyOptions` on `CompilerConfig`.

**Validation after every new pass**: Run this and verify the output
changes (new errors reported, new output produced, etc.):

```bash
cargo run --bin assura -- check demos/libwebp-huffman.assura
cargo run --bin assura -- check demos/libwebp-huffman.assura --verbose
```

If the output is identical to before you added the pass, the pass is
not wired in. Fix it before marking the task done.

**Test that the passes interact**: Each new pass must have at least one
integration test that feeds the output of the previous pass into the
new pass. Prefer `assura_test_support` helpers so tests exercise the
real pipeline, not hand-built intermediate structs.

Example: a `resolve` test must start from a parsed `SourceFile` (not
hand-built AST), and a `type_check` test must start from a resolved
file (not hand-built resolved AST).

**This rule applies at BOTH levels, not just top-level passes:**

- **Compiler passes**: new crates must be reachable from `assura-pipeline`
  or a documented entry point (`assura-cli`, `assura-lsp`, `assura-server`,
  `assura-mcp`)
- **Analysis components**: new checker structs in `assura-types` must
  have a corresponding `run_*_checks()` function wired into the type
  checker pipeline. New manager structs in `assura-smt` must be called
  from `Verifier` / verify dispatch.

Verification after adding any new checker or manager struct:

```bash
# Must appear in the entry-point function's call chain
grep -n "StructName\|run_structname_checks" crates/assura-types/src/lib.rs
grep -n "StructName\|run_structname_checks" crates/assura-types/src/pipeline.rs
```

If the struct exists but the grep returns zero matches in the entry
point, the component is dead code. Wire it in before marking the task
done. This was learned when 4 features (`CryptoConformanceChecker`,
`TriggerManager`, `IncrementalCompiler`, `TestGenerator`) shipped with
complete implementations and passing unit tests but were never called
from any pipeline entry point across multiple sessions.

**Check individual methods, not just struct presence.** A struct can
be "wired in" (called from the entry point) while individual public
`check_*()` or `validate_*()` methods on it remain dead code.

After adding a new manager struct with multiple checking methods,
verify each method is called:

```bash
# List all pub fn on the struct
grep -n 'pub fn' crates/assura-smt/src/advanced.rs | grep -i prophecy

# Verify each appears in a call chain from the entry point
grep -rn 'check_all_resolved\|check_unconstrained' crates/assura-smt/src/
```

If a method exists but has zero callers outside its own test module,
wire it in before marking the task done.

This was learned when `ProphecyManager::check_unconstrained()` existed
with passing unit tests but was never called from `verify()` or the
Z3 backend across multiple sessions. Unconstrained prophecy variables
were silently ignored until the method was wired in during #62.

### Pipeline skew (behavioral divergence)

If CLI does X but `compile_full` does Y, agents and users get different
results from the same source. Always fix skew by converging on
`assura-pipeline` + `VerifyOptions`, not by copying more ad-hoc logic
into CLI/server/MCP.

## Pre-Commit Checks

**Verification command hygiene (see #330):**
Never use patterns like `command 2>&1 | tail -N && echo "step: OK"`.
The `tail` always succeeds, so the echo runs even on real failures (this
masked cvc5 clippy and test failures in session bg tasks).
Use `set -euo pipefail`, run commands directly.

Pre-commit scripts have been removed per request. Use direct commands:

**Before each push** (fast):
```bash
cargo fmt -- --check
cargo clippy --workspace --locked -- -D warnings
cargo check -p <crate> --locked
cargo test -p <crate> --locked --lib   # or targeted
```

**Before session end or marking a MASTER-PLAN task `[x]`** (full):
```bash
cargo fmt --all
cargo clippy --workspace --locked -- -D warnings
cargo clippy -p assura-smt --features cvc5-verify -- -D warnings
cargo test --workspace --locked
cargo check --no-default-features -p assura-smt
```

**Command selection by query type (important for agent sessions)**

When the user's question is reflective, audit-style, or meta ("during the session did we learn...", "did we notice an issue but jump over it", "should we update any skill", "what went wrong in the process"):

- Use *only* inspection tools: `read_file`, `grep`, `gh issue/run/view/list`, `git status`, `git diff --name-only`, `git show`, `list_dir`.
- Never launch `cargo`, `cargo run`, `cargo test`, `cargo check`, or any build/test command.

Implementation, reproduction, or "make this green" questions are the only time targeted `cargo ... -p <crate> --locked` commands are appropriate.

This rule was reinforced when unnecessary build commands were executed during a pure reflection question.

**After any change that could affect cli_integration races or the main
executable (see #328), run the full checks + explicitly `cargo test --workspace --locked`
before the end of the session / before pushing the final commit.**

The `cargo clippy -p assura-smt --features cvc5-verify` step mirrors the CI
`cvc5` job and catches cfg-gate violations in native CVC5 modules that the
default workspace clippy build skips.

The final `cargo check --no-default-features` verifies the no-z3 build.
Any code in `assura-smt` that imports from `z3_backend` or `z3` must be
behind `#[cfg(feature = "z3-verify")]` with a fallback. This check has
caught cfg-gate violations twice; do not skip it.

If any step fails, fix it before committing. Do not commit with
`--no-verify` or skip tests. If a test is flaky, fix the test.

After committing, verify the commit is clean:

```bash
cargo run --bin assura -- check demos/libwebp-huffman.assura
cargo run --bin assura -- check demos/zlib-inflate.assura
cargo run --bin assura -- check demos/mbedtls-x509.assura
cargo run --bin assura -- check demos/taint-tracking.assura
cargo run --bin assura -- check demos/heartbleed.assura
cargo run --bin assura -- check tests/fixtures/test_basic.assura
cargo run --bin assura -- check tests/fixtures/test_sec.assura
```

## Feature Verification Gate

After completing work on any of the 50 verification features, run
the verify-task script. It is a machine-enforced gate that checks
build, clippy, tests, demo files, and coverage score.

```bash
bash scripts/verify-task.sh SEC.1
```

If the script exits non-zero, the feature is not done. Fix the issue
before marking the feature complete. "I wrote the code" is not done;
"the script exits 0" is done.

The coverage audit script (`~/.grok/skills/assura-coverage-audit/`)
tracks each feature across 13 compiler layers. After implementing a
feature in a new layer, the coverage score for that feature must
increase. If it does not increase, the implementation is not wired
in correctly.

All files above must parse successfully. If a parser change breaks any
demo file, the change is wrong. Fix it before pushing.

## Task Completion Criteria

A task in MASTER-PLAN.md is done when ALL of these are true:

1. The code compiles: `cargo build`
2. All tests pass: `cargo test --workspace --locked` (on local machine or via full gate).
   Inside an agent tool, targeted tests + `--locked` are acceptable substitutes for the full run.
   **Exception:** changes that touch `cli_integration`, temp-dir code, or the
   main `assura` executable (all crates) require a real full `cargo test --workspace --locked`
   (see #328 and the "Build and Test" section).
3. No warnings: `cargo clippy --workspace -- -D warnings`
4. All demo files still parse: run all four
5. New code has tests (unit tests in the same file, integration tests
   in `tests/`)
6. **Every acceptance test command in the task's "Acceptance Tests"
   block has been run and passed.** This is the most important criterion.
   Each task in MASTER-PLAN.md has a code block with exact commands.
   Run every one. See every one pass. If any fails, the task is not done.
7. MASTER-PLAN.md is updated: task marked `[x]`, session note added
8. Changes are committed and pushed

Do not mark a task `[x]` if any of these are false.

### Acceptance test enforcement (CRITICAL)

The previous plan (v2) had 66 tasks all marked `[x]` but many checkers
were structural stubs (wiring dead-ends returning `Vec::new()`). This
happened because tasks were marked done based on "code compiles" without
verifying the code actually produces correct output.

**New rule**: Every task in MASTER-PLAN.md v3 has an `Acceptance Tests`
section with exact shell commands. These are not suggestions. They are
mandatory verification steps. The mechanical process is:

```
1. Read the task's Acceptance Tests block
2. Run each command in order
3. Verify each command's output matches the expected result
4. If any command fails, the task is NOT done -- fix the code first
5. Only after all commands pass, mark [x] and commit
```

**What counts as "pass":**
- `cargo test -p crate_name test_name` exits 0 with "test result: ok"
- `grep` commands return the expected count (0 or >0 as specified)
- CLI commands exit 0 with expected output
- `cargo test --workspace --locked` exits 0 at the end (the final gate) — or the equivalent targeted tests with `--locked` when running inside an agent tool with timeouts

**What does NOT count:**
- "I wrote the test" (did it pass?)
- "The code compiles" (does it produce correct output?)
- "Similar to another feature that works" (run the specific test)
- "The acceptance test is too strict" (then fix the code, not the test)

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
# (or targeted: `cargo test -p <crate> --lib` when inside agent tool with timeouts)

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

The `z3` Rust crate version is `0.20` (uses pre-generated FFI
bindings, no `bindgen` needed at build time). The crate links against
libz3 at build time; if it can't find it, set `LD_LIBRARY_PATH`.

For CI (T029), add this to the GitHub Actions workflow:

```yaml
- name: Install Z3
  run: sudo apt-get install -y libz3-dev
```

## CVC5 Installation

The `assura-smt` crate optionally depends on the `cvc5` Rust crate
(behind the `cvc5-verify` feature flag). The cvc5 crate uses native
FFI bindings and needs the CVC5 static libraries.

```bash
# macOS (prerequisites for building cvc5)
brew install cmake gmp

# The cvc5 crate with features = ["static"] downloads and builds
# CVC5 from source automatically. No manual install needed for
# the Rust crate build.

# For CI or manual prebuilt setup:
# macOS ARM64
curl -sL "https://github.com/cvc5/cvc5/releases/latest/download/cvc5-macOS-arm64-static.zip" \
  -o /tmp/cvc5.zip
unzip -o /tmp/cvc5.zip -d /tmp/cvc5-install
export CVC5_LIB_DIR=/tmp/cvc5-install/cvc5-macOS-arm64-static/lib
export CVC5_INCLUDE_DIR=/tmp/cvc5-install/cvc5-macOS-arm64-static/include

# Linux x86_64
curl -sL "https://github.com/cvc5/cvc5/releases/latest/download/cvc5-Linux-x86_64-static.zip" \
  -o /tmp/cvc5.zip
unzip -o /tmp/cvc5.zip -d /tmp/cvc5-install
export CVC5_LIB_DIR=/tmp/cvc5-install/cvc5-Linux-x86_64-static/lib
export CVC5_INCLUDE_DIR=/tmp/cvc5-install/cvc5-Linux-x86_64-static/include
```

The `cvc5-verify` feature mirrors the `z3-verify` feature gate. All
CVC5 native code is behind `#[cfg(feature = "cvc5-verify")]` with a
shell-out fallback behind `#[cfg(not(feature = "cvc5-verify"))]`.
Build with CVC5: `cargo build --features cvc5-verify`. Test:
`cargo test -p assura-smt --features cvc5-verify`.

### CVC5 verification gate (issues labelled `cvc5-parity`)

Do not close `cvc5-parity` issues from Z3-only evidence. "CVC5 native
blocked on cvc5-sys build" is not equivalent to "CVC5 parity verified."

**Minimum evidence before closing:**

| Layer | Command | When required |
|-------|---------|---------------|
| Shell parity | `cargo test -p assura-smt -- cvc5_` | Always (default build) |
| Native parity | `cargo test -p assura-smt --features cvc5-verify -- cvc5_` | When native encoding changed |
| Native clippy | `cargo clippy -p assura-smt --features cvc5-verify -- -D warnings` | When touching `cvc5_*` modules |

**CI enforcement:** `.github/workflows/ci.yml` job `cvc5` runs native
clippy + tests with prebuilt static libs (same URLs as below).

**macOS ARM developers:** `cvc5-sys` source builds often fail on AppleClang
(Poly-EP / `gmpxx.h` `-Werror`). Run prebuilt setup **before**
manual commands (pre-commit scripts removed), not only after a failure:

```bash
bash scripts/setup-cvc5.sh
# paste the printed export CVC5_LIB_DIR / CVC5_INCLUDE_DIR lines
cargo test -p assura-smt --features cvc5-verify -- cvc5_
```

If native tests cannot run locally, the issue may still close when CI
`cvc5` job is green on `main` — comment the run URL as evidence. Do not
close `cvc5-parity` issues before that CI job finishes on the closing
commit (#304).

**CI-before-close rule (#304):** Do not close `cvc5-parity` issues
until the CI `cvc5` job is green on the closing commit. If closing
immediately after push, comment "pending CI" and verify once the run
completes. Use `scripts/wait-for-ci-cvc5.sh <sha>` when available.

**`cargo test` filter:** One substring filter per invocation only
(`cargo test -p assura-smt ir_parity` — not `ir_parity ir_lower`).

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
3. **Verbose check**: Run `cargo run --bin assura -- check file.assura --verbose`
   to see timing for each pipeline phase and identify where the failure occurs.
4. **Unit test**: Write a focused `#[test]` that parses the failing snippet
   with `assura_parser::parse()` and inspects the AST/errors directly.
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

### Lowering Helpers (avoid boilerplate)

In `crates/assura-parser/src/lower.rs`, repeated patterns for wrapping
nodes and handling sub-expressions/recovery have been centralized:

- `spanned(node, n)` — use everywhere instead of manual
  `Spanned { node, span: span_of(n) }` or `let sp = span_of(n); Spanned...`.
- `missing_expr()` — the canonical `Spanned::no_span(Expr::Raw(vec![]))`
  for recovery.
- `lower_first_child_expr_or_missing(n)` — "first direct expr-kind child
  or missing_expr()".
- `lower_expr_children(n)` — `Vec<SpExpr>` of all direct expr children
  (lower_arg_list now delegates to this for consistency).
- `apply_binop_chain(base, chain)` — deduplicates the left-associative
  BinOp chain reconstruction used in `lower_bin_expr`.

- Use `cst::is_trivia(k)` (not manual `k == WHITESPACE || k == COMMENT` or matches!) for all trivia skipping in token walks and filters in lower.rs (and call sites). See collect_token_texts and the ~15 sites consolidated for #337.

**Rule**: If the same Spanned construction, child-filter, or recovery
snippet appears in 3+ places in lower.rs (or across lowering functions),
extract a helper and migrate. Grep for remaining duplication:

```bash
grep -n 'Spanned\s*{' crates/assura-parser/src/lower.rs
grep -n 'span_of(n)' crates/assura-parser/src/lower.rs
grep -n 'filter.*is_expr_kind' crates/assura-parser/src/lower.rs
```

The same principle applies project-wide:

- **BinOp**: Use `as_str`/`as_rust_str`/`as_ident` (backed by internal
  `repr()`) + `is_arithmetic()`, `is_comparison()`, `is_logical()`,
  `is_ordering_comparison()`, `is_division_like()`, `is_membership()`.
  Never repeat long `| BinOp::Add | Sub | ...` lists in match guards.
- **ExprFolder** consumers (display, codegen, fmt): Use
  `fold_joined(self, items, sep)`, `fold_arg_list(self, args)`, and
  `literal_to_string(lit)` instead of inlining the collect/join/map and
  literal arms in every impl.

When you introduce a new helper, document it here and in
`~/.grok/skills/assura-contrib/SKILL.md`.

### Parser / CST helpers (for correct spans after trivia capture)

- `bump_delim()` on `Parser` (cst.rs) — bump a delimiter token (`{`, `(`, etc.) and immediately call `bump_trivia()`. This ensures expressions inside braced/parenthesized clause bodies (and similar) receive `text_range()` values that match original source offsets rather than being shifted by leading whitespace or comments. Introduced during the #335 spans + trivia work and the subsequent duplication cleanup pass to eliminate ~20 repeated `bump(); bump_trivia();` sites. Use it (instead of the two-liner) after any manual delimiter open that must expose following trivia to child nodes.

- `body_tokens_inner(p, closer, stoppers)` (grammar/mod.rs) — raw balanced delimiter skipper used for clause bodies, fn/trailing/axiom bodies, generic blocks, type bodies, attr lists, etc. Pass the expected closer (R_BRACE / R_PAREN / R_BRACKET) as the virtual; it uses a stack to handle nesting of mixed delimiters. Updated in #339 to take explicit closer (was always R_BRACE, causing cross-closer theft of outer } when collecting inside ( or [ ).

  **Collector contract** (`body_tokens_inner` + `expect_closer`, #342):
  1. Caller must already have consumed the opening delimiter (or be in a context
     where the virtual closer on the stack is the only thing that needs matching).
  2. On success, the collector stops with `current()` at the matching closer
     (the closer is **not** consumed; the caller must expect it).
  3. On stoppers / EOF / mismatched closer, the collector may leave the parser
     before the closer (e.g. illustrative fn bodies with `validate { } … or return`,
     struct lits, `constant_time { }`, comments/trivia at EOF). That is expected.
  4. Prefer fixing the collector (stoppers, stack discipline, `current_raw`/`bump_raw`)
     when good input systematically fails. Use `expect_closer` as the safety net
     for the known "slightly off" cases above, not as a substitute for correct
     collection.
  5. `expect_closer` only bumps on the error path when not already at the closer;
     on well-formed input it is a single `expect` with no extra tree nodes.
  6. Truly unclosed input still errors: if EOF is reached with no closer in the
     stream, `expect` emits the usual "expected `}`" (or paren/bracket) diagnostic.
  7. Debug builds can assert after collection that when `at(closer)` is false and
     not at EOF, a future collector improvement may be warranted; do not turn that
     into a production-only panic.

- `expect_closer(p, closer)` (grammar/mod.rs) — tolerant sync then strict expect
  after `body_tokens_inner` or item loops that may leave trivia/mixed constructs
  between the last inner token and the outer closer. Use for R_BRACE / R_PAREN /
  R_BRACKET in those paths. Bare `p.expect(R_*)` remains correct when the parser
  is structurally guaranteed to be on the closer (e.g. `old(expr)`, `param_list`,
  arg/index lists after normal expression parsing).

- **`current()` / `current_text()` vs `tokens[pos]`** (params.rs `is_return_type_stopper`,
  #345): `current()` and `current_text()` skip leading trivia at `pos`. Reading
  `tokens.get(p.pos())` while branching on `current() == IDENT` can see WHITESPACE
  text and fail to treat ident clause starters (`catch`, `must_check`, etc.) as
  stoppers, slurping them into return types. Always use `current_text()` when
  matching on `current()` kind. Regression: `return_type_does_not_slurp_catch_clause`
  in `crates/assura-parser/tests/snapshots.rs`.

- `is_trivia(k)` (cst.rs, pub(crate)) — canonical check for WHITESPACE | COMMENT. Use everywhere instead of duplicating the predicate in lower.rs and elsewhere. See #337 consolidation.

- **`current()` / `current_text()` vs `tokens[pos]`** (#345, #348): `current()` and
  `current_text()` skip leading trivia at `pos`. Reading `tokens.get(p.pos())`
  while branching on `current() == IDENT` can see WHITESPACE text and fail to
  treat ident clause starters (`catch`, `must_check`, etc.) as return-type
  stoppers, slurping them into `return_ty`. Always use `current_text()` when
  matching on `current()` kind. Documented on `Parser::pos` / `current_text` in
  `cst.rs`. Grep audit: `tokens.get(p.pos())`, `tokens[p.pos()]` (should be zero
  in grammar code). Fixed in `is_return_type_stopper` (params.rs).

  **Footgun**: In manual token-walking code that does `p.expect(SyntaxKind::L_BRACE); p.bump_delim();` (e.g. `codec_entry` and similar in `grammar/items.rs`), `expect` already bumped the `{`. The extra `bump()` from `bump_delim()` skips the first real token after the brace. This caused "expected `magic`, `decoder`, `contracts`, or `}`" errors and made the codec_registry lower tests fail. Correct pattern for such collectors: `expect(L_BRACE); bump_trivia();` (matching contract_decl, service_decl, etc.). Audit all manual loops inside braces after any span/trivia change.

**eat(COMMA) / list separator trivia footgun (match arms, patterns, etc.)**

In loops that parse comma-separated items (match arms, tuple patterns, etc.):

```rust
while !p.at(R_BRACE) {
    match_arm(p);
    p.eat(SyntaxKind::COMMA);
    p.bump_trivia();   // <-- required
}
```

`eat(COMMA)` consumes the token but does not advance past trivia. `pattern(p)` (and parts of the expression parser) inspect `p.current()` directly to decide `LITERAL_PAT`, `IDENT_PAT`, `WILDCARD_PAT`, etc. Missing `bump_trivia()` causes the next literal/ident to be invisible → `err_and_bump("expected pattern")` → the arm CST has no PAT child.

In lowering (`lower_match_arm`):

```rust
let pattern = n.children().find_map(|c| lower_pattern(&c))
    .unwrap_or(Pattern::Wildcard);
```

Result: second arm silently becomes `Wildcard`. A checker that looks for "no wildcard on unknown scrutinee" (A10002) will see `has_wildcard` and emit nothing. The test `match_unknown_scrutinee_no_wildcard_a10002` (and similar exhaustiveness cases) will fail with "expected A10002, got []".

Same rule applies to any `eat(COMMA)` (or other separator) that sits between calls to `pattern()` or `expr()` inside a list.

Always follow such `eat`s with `bump_trivia()` before the next sub-parser that relies on `current()`.

**Verification checklist after any change to grammar/expressions.rs, lower.rs, or pattern handling**

Before you push:
1. Inspect the *committed* code, not just your working tree:
   `git show HEAD:crates/assura-parser/src/grammar/expressions.rs | sed -n '250,280p'`
2. Run the exact test(s) that would have caught the symptom (targeted only):
   `cargo test -p assura-types match_unknown_scrutinee_no_wildcard_a10002 --locked`
   `cargo test -p assura-types 'match_' --locked`
3. If you temporarily added `eprintln!` of AST or clause bodies for debugging, remove it.
4. After `git push origin main`, immediately check the new run:
   `gh run list --branch main --limit 3`
   and `gh run view <new-run-id>`. Do not assume "it was correct locally."

When a types-level test says "expected error code X, got []", the root cause is frequently a parser/lower decision that produced the wrong AST shape (e.g. unexpected Wildcard).

## Expression Parser

The expression parser uses Pratt parsing (binding power) implemented
in `grammar/expressions.rs`. It produces `Expr` AST nodes with full
operator precedence.

**Trivia rule for lists/arms**: After `p.eat(SyntaxKind::COMMA)` (or similar separators) inside loops that call `match_arm` / `pattern` / `expr`, always follow immediately with `p.bump_trivia()`. See the detailed footgun + verification checklist in the "Parser / CST helpers" section above and the code comment in `match_expr`. Missing it is a common source of "parser accepts it but the AST is wrong (Wildcard instead of Literal)" bugs that only show up in type/exhaustiveness checks.

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

**Operator chain limit**: The Pratt parser enforces
`MAX_BINOP_CHAIN = 256`. After 256 consecutive infix operators at the
same precedence level, the parser emits an error and stops extending
the chain. This prevents stack overflow in downstream recursive AST
walkers (display, resolve, type-check, codegen) which recurse on the
left-leaning `Expr::BinOp` tree. The limit is the primary defense;
`lower_bin_expr` (lower.rs) and `expr_to_string` (display.rs) also
use iterative traversal as defense-in-depth.

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
- Do not change the ariadne major version without updating all
  `Report::build` call sites (the API changed between 0.4 and 0.6)
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

## Adding a New Decl Variant

Adding a new variant to the `Decl` enum (e.g., `Bind`, `Trait`) is a
high-impact change that touches 17+ files across the codebase. Every
match on `Decl` becomes non-exhaustive. Use this checklist:

### Files that need a new match arm

| Crate | File | What to update |
|-------|------|----------------|
| assura-parser | `syntax_kind.rs` | Add `VARIANT_DECL` to `SyntaxKind` |
| assura-parser | `grammar/items.rs` | Add grammar function, wire into `decl()` and recovery sets |
| assura-parser | `ast.rs` | Add variant to `Decl` enum and struct definition |
| assura-parser | `lower.rs` | Add `VARIANT_DECL` match in `lower_decl()` and `lower_variant()` function |
| assura-parser | `display.rs` | Add `Decl::Variant` display arm |
| assura-fmt | `lib.rs` | Add `format_variant()` and import the struct |
| assura-resolve | `lib.rs` | Add to `SymbolKind`, register in symbol table, handle in 4+ match sites |
| assura-types | `lib.rs` | Add to `build_type_env` |
| assura-types | `checkers.rs` | Add match arm in taint checking |
| assura-types | `clauses.rs` | Add match arms in clause body checking |
| assura-codegen | `lib.rs` | Add to 9+ match sites (type collection, generic arity, codegen dispatch) |
| assura-smt | `display.rs` | Add to `collect_contract_names()` and stats counting |
| assura-smt | `z3_backend.rs` | Add to verification dispatch loop |
| assura-lsp | `lib.rs` | Add to hover, completion, and document symbols (3 match sites for SymbolKind, 1 for Decl) |
| assura-cli | `main.rs` | Add to stats counting, REPL eval (line ~907), `extract_decl_summary()` (diff command) |
| assura-mcp | `lib.rs` | Add to declaration listing in `run_check_pipeline()` |

### Common mistakes

1. **Forgetting `parsed_type` on `Param`**: The `Param` struct has a
   mandatory `parsed_type: Option<TypeExpr>` field. When constructing
   params outside the normal `lower_param()` path, you must set it
   (use `try_parse_type_tokens(&ty)` or `None`).

2. **Bind-style declarations with body clauses**: If the new variant
   stores params inside `input(...)`/`output(...)` clauses (like `bind`
   does), you cannot use `lower_param_list()` because there is no
   `PARAM_LIST` CST node. You must extract params from the clause body
   tokens. See `extract_params_from_clause_body()` in `lower.rs`.

3. **SymbolKind propagation**: After adding a new `SymbolKind` variant
   in assura-resolve, grep for all matches on `SymbolKind` in the LSP
   crate (3 sites: hover labels, completion item kinds, completion
   detail strings).

### Verification strategy

After adding the variant, run `cargo build` first. The compiler will
report every non-exhaustive match. Fix them all before running tests.

## Writing Demo Contracts That Z3 Can Verify

All Assura declarations are specification-only (no implementation
bodies). This means `result` and output variables (from `output()`
clauses) are free Z3 variables. Z3 can assign them to any value,
so ensures clauses that reference unconstrained outputs will always
produce counterexamples.

**Rules for verifiable demo contracts:**

1. **Ensures must reference only input variables.** Write ensures
   clauses that are tautologies derivable from the requires clauses.
   Z3 proves these by showing the negation is unsatisfiable.

   ```assura
   # GOOD: ensures references only inputs (x, max)
   requires { x >= 0 }
   requires { x < max }
   ensures  { max > x }

   # BAD: ensures references unconstrained output (result)
   requires { x >= 0 }
   ensures  { result >= 0 }
   ```

2. **Use inline integer literals, not `feature_max` constants.**
   The SMT encoder treats `feature_max` named constants as
   unconstrained Z3 integer variables, not their defined values
   (see #180). Until constant folding is implemented, inline the
   values directly.

   ```assura
   # GOOD: Z3 sees the actual value 3
   requires { 3 + payload_length + padding_length <= record_length }

   # BAD: Z3 treats HEADER_SIZE as unconstrained (could be 0)
   feature_max HEADER_SIZE: Nat = 3
   requires { HEADER_SIZE + payload_length + padding_length <= record_length }
   ```

3. **`.length()` method calls work.** The encoder adds a background
   axiom `length >= 0` for Bytes/String `.length()` calls. So
   `ensures { result.length() >= 0 }` verifies on extern functions
   returning Bytes.

4. **Base new demos on real CVEs.** Each demo should model a real
   vulnerability with the CVE number, CVSS score, root cause, and
   how Assura prevents it. See `demos/heartbleed.assura` (CVE-2014-0160)
   as the template.

**Verification semantics:**
- `ensures`: Z3 does validity checking (assert NOT clause; UNSAT = valid)
- `invariant`: Z3 does satisfiability checking (assert clause; SAT = ok)

This was learned when `demos/taint-tracking.assura` broke CI with
counterexamples from unconstrained output variables, and
`demos/heartbleed.assura` initially failed because `feature_max`
constants were treated as unconstrained by the encoder.

## SMT API Shape

Prefer `assura_pipeline::verify_typed` from outside `assura-smt`. Only construct
`Verifier` directly inside `assura-smt` or the pipeline crate.

Classify `Unknown` reasons with `assura_smt::is_known_smt_limitation` (marker:
`KNOWN_SMT_LIMITATION_MARKER` = `"not yet encoded in SMT"`), not ad-hoc string
checks with different wording.

The `assura_smt::VerificationResult` is an **enum**, not a struct with
result/kind/name fields:

```rust
pub enum VerificationResult {
    Verified { clause_desc: String },
    Counterexample { clause_desc: String, model: String, counter_model: Option<CounterexampleModel> },
    Timeout { clause_desc: String },
    Unknown { clause_desc: String, reason: String },
}
```

`verify()` returns `Vec<VerificationResult>`. There is no `.contract_name`
or `.clause_kind` field. The `clause_desc` is a human-readable string
like `"SafeDivision: ensures"`. Do not pattern-match assuming struct
fields that do not exist.

**Unknown severity classification**: Not all `Unknown` results are errors.
The CLI (`check.rs`) classifies them by reason string:

| Reason contains | Severity | Exit code | Rationale |
|-----------------|----------|-----------|-----------|
| "not yet encoded in SMT" | Warning | 0 | Known compiler limitation, not a verification failure |
| Anything else | Error | 1 | Genuine solver inconclusive (non-linear arithmetic, timeout fallback, etc.) |

When adding new `VerificationResult::Unknown` producers, choose the
reason string carefully. If the reason represents a known limitation
where we intentionally skip verification, include "not yet encoded in
SMT" so the CLI treats it as a warning. If the solver genuinely could
not decide, use a different reason string.

## MCP Server (rmcp 1.7) API Shape

The `assura-mcp` crate uses `rmcp` 1.7 with `server` and `transport-io`
features. The rmcp proc macros generate significant glue code, and the
public API surface is not obvious from docs. These patterns were learned
from 11 build errors during initial implementation.

**Imports**: Use `rmcp::handler::server::wrapper::Parameters`, not
`rmcp::tool::Parameters` (the latter is private and will not compile).

**Tool return type**: Tool functions must return `String`, not
`Result<CallToolResult, McpError>`. The `IntoToolRoute` trait bound
requires it. Serialize your result with `serde_json::to_string_pretty`.

**Tool async**: Tool functions do not need to be `async`. Sync `fn`
satisfies the trait. Use async only if the tool does actual I/O.

**ServerInfo construction**: `ServerInfo` is non-exhaustive; you cannot
use a struct literal. Use the builder:

```rust
ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    .with_instructions("...")
```

**Two macros required**: Both `#[tool_router]` on the tool impl block
AND `#[tool_handler]` on `impl ServerHandler` are needed. Without
`#[tool_handler]`, `list_tools` returns an empty array and `call_tool`
does nothing.

**Dead code warning**: The `tool_router: ToolRouter<Self>` field on the
server struct is read by the `#[tool_handler]` macro at runtime, but
the compiler thinks it is unused. Add `#[expect(dead_code)]` to the
field.

```rust
#[derive(Debug, Clone)]
pub struct AssuraMcpServer {
    #[expect(dead_code)]
    tool_router: ToolRouter<Self>,
}
```

## Type::Error vs Type::Unknown

The `Type` enum has two indeterminate variants. **Never compare
directly against `Type::Unknown`; use `ty.is_indeterminate()` instead.**

| Variant | Meaning | When produced |
|---------|---------|---------------|
| `Unknown` | Genuinely unknown (unresolved ident, missing type args) | Identifier not in env, empty generic params |
| `Error` | Error already reported upstream; suppress cascading | `Expr::Raw`, error-receiver field/method/index/call |

`Type::is_indeterminate()` returns `true` for both. Use it everywhere
you would have written `ty == Type::Unknown`:

- **Clause body checking**: `if !ty.is_indeterminate() && ty != Type::Bool`
- **Type compatibility**: `types_compatible()` treats both as wildcard
- **Numeric leniency**: `is_numeric()` accepts both
- **If/match branch merging**: pick the concrete branch when the other
  is indeterminate

**Why this matters**: Writing `== Type::Unknown` misses `Error`, which
causes cascading false-positive diagnostics. A single typo in a
receiver name would produce one "unknown variable" error followed by
spurious A03005 errors on every field access and method call downstream.

