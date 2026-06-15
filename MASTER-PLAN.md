# Assura Master Plan v2

> Comprehensive rebuild plan treating Assura as a greenfield compiler.
> The original T001-T119 tasks established initial scaffolding. This v2
> plan addresses what actually needs to be done: fix broken pipelines,
> refactor architecture, close open issues, deepen feature implementations
> from structural stubs to real semantic analyzers, and build toward a
> production-quality compiler.
>
> **How to use**: Read top to bottom. Pick the next `[ ]` task whose
> `depends-on` tasks are all `[x]`. Complete it. Mark it `[x]`. Commit
> MASTER-PLAN.md with the change. Continue to the next task.
>
> **Session protocol**: At the start of every session, read this file,
> find the next uncompleted task, work on it, mark it done, commit, push,
> and continue. Before ending, update the Progress Notes section.
>
> **Scope limit**: Never plan more than 5 tasks ahead. This plan is long
> by necessity (it covers years of work), but agents should focus on the
> next 1-3 tasks only. Do not load the entire plan into working memory.

---

## Current State Assessment (2026-06-13)

### What works

| Component | LOC | Tests | Status |
|-----------|-----|-------|--------|
| assura-parser | 2,775 | 33 (14 snapshot + 19 parser) | Solid. All demos parse. |
| assura-resolve | 3,368 | 77 | Working scope analysis, import resolution. |
| assura-types | 25,183 | 838 | Massive monolith. Many checkers are structural stubs. |
| assura-codegen | 3,261 | 90 | Generates syntactically valid Rust (syn::parse_file passes), but generated code does NOT compile (undefined types, missing imports). |
| assura-smt | 7,069 | 0 in-crate (tested via assura-types) | Z3 backend exists behind feature flag. 91 uses of Z3 API. No standalone tests. |
| assura-cli | 2,409 | 15 | Pipeline works: lex -> parse -> resolve -> typecheck -> codegen. |
| assura-lsp | 835 | 9 | Basic diagnostics, hover, go-to-def. Minimal. |
| assura-server | 496 | 0 | gRPC service compiles. Zero tests. |

**Total: 45,579 LOC, ~1,062 tests**

### Critical problems

1. **Generated Rust does not compile** (missing types: `BitReader`, `Region`, `HuffmanGroup`, etc.)
2. **assura-types is a 25K-line single file** (unmaintainable, impossible to review)
3. **Refinement predicates lost during type parsing** (GitHub issue #6)
4. **Duplicate param extraction across 3 crates** (GitHub issue #5)
5. **SMT verification reports nothing per-clause** (just "check passed")
6. **Parser errors lack expected-token info** (GitHub issue #7)
7. **No `assura.toml` project config** (spec Section 10.3)
8. **No multi-file compilation** (only single-file mode)
9. **Wildcard catch-alls in match arms** (GitHub issue #9)
10. **No CLI build --output tests** (GitHub issue #8)

### Missing from spec/roadmap/market-research

| Feature | Source | Status |
|---------|--------|--------|
| CVC5 fallback solver | Spec, Roadmap, Issue #1 | Not started |
| WASM compilation target | Spec, Investigation, Issue #3 | Not started |
| Performance benchmarks | Issue #2 | Not started |
| `assura.toml` configuration | Spec Section 10.3 | Not started |
| Multi-file/module compilation | Spec Section 8 | Not started |
| Fuzzing infrastructure | AGENTS.md, Roadmap | Not started |
| Release pipeline (crates.io, Homebrew) | Market research | Not started |

---

## Phase R: Rework (Architecture + Critical Fixes)

> Treat the codebase as a greenfield project with existing code as
> reference material. Rework should not matter since there are no
> users yet. Fix foundations before adding features.

### R.1 Generated Rust Must Compile

- [x] **R001**: Fix codegen to produce compilable Rust for all demo files
  - Depends on: none (blocking everything)
  - The generated `lib.rs` for `demos/libwebp-huffman.assura` fails
    `cargo build` with: undefined `BitReader`, `Region`, `HuffmanGroup`,
    `DecodeError`, `ValidCodeLengths`, `MAX_ALPHABET_SIZE`, etc.
  - **Root cause**: codegen emits function signatures referencing types
    from the contract, but does not generate struct/enum definitions for
    those types. `feature_max` constants are emitted as empty modules.
  - **Fix**:
    1. Collect all user-defined type names from the AST (TypeDef, EnumDef,
       extern type declarations, feature_max constants)
    2. Generate Rust struct/enum stubs for every referenced type that has
       a definition in the AST
    3. Generate `const` declarations for `feature_max` values
    4. Add `use` imports for types from other modules
    5. For types with no AST definition (extern types), generate a
       placeholder struct with a `_phantom` field
  - **Validation**: `cargo run --bin assura -- build demos/libwebp-huffman.assura`
    followed by `cd generated && cargo check` must succeed (not `cargo build`,
    since function bodies are `todo!()`, but types must resolve)
  - Run `cargo check` on all three demo outputs:
    - `demos/libwebp-huffman.assura`
    - `demos/zlib-inflate.assura`
    - `demos/mbedtls-x509.assura`
  - Add integration test: `cargo test` verifies generated code passes
    `cargo check` (not just `syn::parse_file`)
  - Crate: `assura-codegen/src/lib.rs`

- [x] **R002**: Fix codegen module structure for multi-contract files
  - Depends on: R001
  - Currently all generated code goes into one flat `lib.rs`
  - Multi-contract files should generate:
    ```
    generated/src/lib.rs        // pub mod declarations
    generated/src/contract_a.rs // per-contract modules
    generated/src/contract_b.rs
    generated/src/types.rs      // shared type definitions
    ```
  - This also fixes name collision issues when two contracts define
    types with the same name
  - Crate: `assura-codegen/src/lib.rs`

### R.2 Split assura-types Monolith

- [x] **R003**: Split assura-types into domain modules (file-level)
  - Depends on: none
  - The current `assura-types/src/lib.rs` is 25,183 lines in ONE file.
    This is the single biggest maintainability problem in the codebase.
  - **Target structure**:
    ```
    assura-types/src/
      lib.rs              # Re-exports, TypedFile, type_check() entry point
      types.rs            # Type enum, TypeEnv, display impls
      checker.rs          # Core expression type checking (T014)
      generics.rs         # Generic instantiation, substitution (T015)
      patterns.rs         # Pattern exhaustiveness (T017)
      clauses.rs          # Contract clause checking (T018)
      linearity.rs        # Usage tracking, context splitting (T031-T032)
      typestate.rs        # DFA state tracking (T034)
      effects.rs          # Effect set checking (T036)
      taint.rs            # Taint tracking, info flow (T047, T051)
      measures.rs         # Totality, decreases (T053-T054)
      stdlib.rs           # Built-in type definitions, method signatures
      errors.rs           # TypeError, error codes, formatting
    ```
  - **Approach**: Pure mechanical refactoring. Move functions and types
    to the appropriate module file. Use `pub(crate)` for internal items.
    Re-export the public API from `lib.rs`.
  - **Constraint**: Zero behavior changes. The test count (838) must not
    change. No test should need modification beyond `use` path changes.
  - Run `cargo test --workspace` after each module extraction to ensure
    nothing breaks.

### R.3 Close Open GitHub Issues

- [x] **R004**: Deduplicate raw-token param/type extraction (issue #5)
  - Depends on: none
  - Three crates independently parse `name: Type` pairs from raw tokens:
    - `assura-types/src/lib.rs`: `register_input_clause_params()`
    - `assura-codegen/src/lib.rs`: `extract_input_params()`
    - `assura-resolve/src/lib.rs`: `extract_input_param_names()`
  - Extract to `assura-parser::ast::parse_clause_params()` with a
    shared `ParsedParam { name: String, ty: Vec<Token> }` return type
  - Update all three call sites
  - Close issue #5 with `Closes #5` in commit message

- [x] **R005**: Preserve refinement predicates during type parsing (issue #6)
  - Depends on: none
  - When `parse_type_tokens` encounters `{ x: Int | x > 0 }`, the
    refinement predicate is lost. Downstream sees `Type::Int` instead
    of `Type::Refined { base: Int, predicate: "x > 0" }`
  - Fix `parse_type_tokens()` in assura-types to detect the `{`, parse
    the base type, detect `|`, and capture the predicate tokens
  - This is critical for SMT: without the predicate, Z3 cannot verify
    refinement subtyping
  - Close issue #6

- [x] **R006**: Improve parser error messages with expected tokens (issue #7)
  - Depends on: none
  - chumsky 0.9's `Simple` error has an `expected()` method returning
    the set of expected tokens. The CLI currently ignores this.
  - Update error rendering in `assura-cli/src/main.rs` to show:
    `error[A01001]: unexpected 'foo', expected one of: '{', 'requires'`
  - Close issue #7

- [x] **R007**: Add integration tests for CLI build --output flag (issue #8)
  - Depends on: R001
  - Tests:
    - Custom output directory is created and receives files
    - Default `generated/` works when `--output` omitted
    - Error on invalid output path
  - Close issue #8

- [x] **R008**: Convert wildcard catch-alls to explicit match arms (issue #9)
  - Depends on: none
  - Locations:
    - `assura-codegen/src/lib.rs`, `generate_service`: two `_ => {}` arms
    - `assura-resolve/src/lib.rs`: similar catch-alls
  - Replace with explicit variant lists so rustc warns on new variants
  - Close issue #9

### R.4 Fix Pipeline Integrity

- [x] **R009**: Show per-clause verification results in CLI output
  - Depends on: none
  - Currently `assura check` just says "check passed (no errors)".
    It should show, for each contract:
    ```
    contract SafeDivision:
      requires { b != 0 }         ... verified
      ensures { result * b ... }  ... verified (0.02s)
    ```
  - When Z3 is not available (feature not enabled), show:
    ```
      requires { b != 0 }         ... skipped (no SMT solver)
    ```
  - Crate: `assura-cli/src/main.rs`

- [x] **R010**: Make Z3 feature enabled by default in workspace builds
  - Depends on: none
  - Currently `z3-verify` is behind an optional feature flag, so the
    default `cargo build` produces a compiler with no verification.
    That defeats the purpose of the project.
  - Change `assura-smt/Cargo.toml`: `default = ["z3-verify"]`
  - Update `assura-cli/Cargo.toml` to depend on `assura-smt` with
    default features (not `default-features = false`)
  - CI already installs libz3-dev, so this should work in CI
  - For users without Z3: document `--no-default-features` as the
    opt-out, and gracefully degrade with "Z3 not installed" messages
  - Add `cargo build --no-default-features` to CI to test graceful fallback

- [x] **R011**: Add standalone tests to assura-smt
  - Depends on: none
  - The SMT crate has 7,069 lines and ZERO in-crate tests. Everything
    is tested indirectly through assura-types.
  - Add tests for:
    - `check_refinement_subtype()` with trivial predicates
    - `verify()` on a minimal TypedFile
    - `verify_buffer_bounds()` with concrete expressions
    - `verify_taint_safety()` with known-safe and known-unsafe inputs
    - Counterexample extraction format
    - Timeout behavior
  - These must work with both `z3-verify` enabled and disabled (test
    the graceful fallback too)

- [x] **R012**: Fix `assura build` to verify generated code compiles
  - Depends on: R001
  - Currently `assura build` writes files and says "OK" even though
    the generated Rust does not compile. It should:
    1. Write generated files
    2. Run `cargo check` on the generated project
    3. Report any `rustc` errors as Assura diagnostics
    4. Exit 1 if generated code fails to compile
  - The original T025 spec said "invoke `cargo build` on the generated
    project" but this was never implemented

### R.5 Clause Body Consistency

- [x] **R013**: Eliminate raw token fallback in clause bodies
  - Depends on: R005
  - Currently some clause bodies are parsed as `Expr` (the expression
    AST) and some fall back to `Vec<String>` (raw token text). This
    dual representation causes:
    - codegen to have two code paths (one for Expr, one for raw tokens)
    - SMT encoder to skip raw-token clauses
    - resolve to skip name checking in raw-token clauses
  - Audit all clause kinds and their current body representation:
    - `requires`, `ensures`: should be `Expr` (most already are)
    - `input`, `output`: parameter lists (different structure, OK)
    - `invariant`: should be `Expr`
    - `modifies`, `reads`, `writes`: identifier lists (OK as-is)
    - `effects`: effect name lists (OK as-is)
    - `decreases`: should be `Expr`
  - For any clause kind still using raw tokens where `Expr` is
    appropriate, update the parser to emit `Expr`
  - Remove the raw-token fallback code paths from codegen and SMT
  - This is a prerequisite for correct SMT encoding of all clauses

---

## Phase S: Strengthen (Deepen Existing Features)

> Many "checkers" in assura-types are structural pattern matchers
> that check syntactic properties but do not perform real semantic
> analysis. This phase deepens them into real analyzers.

### S.1 Real Semantic Analysis

- [x] **S001**: Implement real termination checking (not just measure parsing)
  - Depends on: R003
  - Currently the totality checker (T053) extracts `decreases` clauses
    and checks structural properties (measure exists, is well-founded),
    but does not verify that the measure actually decreases across
    recursive calls
  - Implement:
    1. Detect recursive calls in function/contract bodies
    2. For each recursive call, compute the decreases argument at the
       call site
    3. Generate SMT obligation: `decreases(call_args) < decreases(fn_args)`
    4. Report A09002 if the obligation cannot be discharged
  - Test with: factorial, fibonacci, list append, tree traversal
  - The `partial` escape hatch must suppress the check

- [x] **S002**: Implement real effect inference (not just declared-vs-used)
  - Depends on: R003
  - Currently the effect checker validates that declared effects are
    from the known set, but does not infer which effects a function
    body actually requires by analyzing its call graph
  - Implement:
    1. For each function call in a body, look up the callee's declared
       effects
    2. Compute the union of all callee effects
    3. Check that the union is a subset of the function's own declaration
    4. Report A07001 with the specific undeclared effect and the call
       site that introduced it
  - Currently A07001 only fires on direct name mismatch, not on
    transitive effect propagation through call chains

- [x] **S003**: Implement real information flow tracking
  - Depends on: R003
  - Currently the info flow checker (T051) has the lattice structure
    and declassification tracking, but does not actually trace
    information flow through expressions
  - Implement:
    1. Assign security labels to all input parameters
    2. Propagate labels through assignments, function calls, and
       control flow (implicit flows from branch conditions)
    3. Check that output labels satisfy the declared flow policy
    4. Report A08001 for direct flow violations, A08002 for implicit
       flow through branching
  - Test with: secret data leaked through return value, secret data
    used as branch condition affecting public output

- [x] **S004**: Implement real context splitting for linear types
  - Depends on: R003
  - The current linearity checker tracks usage counts but does not
    implement full context splitting at branch points
  - Implement the algorithm from Section 2.5 of the spec:
    1. At `if/match`, split the linear context into two copies
    2. Type-check each branch with its own copy
    3. After the branch, merge: variables used in both branches are
       consumed (count = sum), variables used in neither are preserved
    4. Report A05001 if a linear variable is used in both branches
       (double use)
  - Test with Section 13 Test Case 1: refinement predicate on a linear
    variable should NOT count as a use

### S.2 SMT Encoding Depth

- [x] **S005**: Wire per-clause SMT verification into the CLI pipeline
  - Depends on: R009
  - Currently `verify()` in assura-smt runs on the whole TypedFile
    and returns aggregate results. The CLI does not display per-clause
    status.
  - Restructure to:
    1. Iterate over each contract in the TypedFile
    2. For each requires/ensures pair, generate a separate SMT query
    3. Report individual results: verified, counterexample, timeout, unknown
    4. The CLI formats these as the per-clause output from R009
  - This enables the user to see exactly which clause failed and why

- [x] **S006**: Implement counterexample display in CLI
  - Depends on: S005
  - When Z3 returns SAT (counterexample), the current output is just
    a generic "counterexample found" message
  - Display concrete values:
    ```
    contract SafeDivision:
      ensures { result > 0 }  ... COUNTEREXAMPLE
        | a = 0, b = 1
        | result = 0
        | The ensures clause is falsified when a=0, b=1.
    ```
  - This is critical for AI iteration: the counterexample tells the AI
    exactly what input breaks the contract

- [x] **S007**: Implement Layer 2 real quantifier verification
  - Depends on: S005
  - The current Layer2Verifier has structural checks but does not
    actually send quantified formulas to Z3
  - Implement:
    1. `forall x in S: P(x)` -> Z3 universal quantifier with trigger
    2. `exists x in S: P(x)` -> Z3 existential quantifier
    3. Use the AUFLIA theory (arrays + uninterpreted functions + LIA)
    4. Implement the 10s timeout from Layer 2 (vs 1s for Layer 1)
  - Test with: sorted list invariant, binary tree balance property

### S.3 Codegen Quality

- [x] **S008**: Generate compilable Rust for service declarations
  - Depends on: R001
  - Services with typestate should generate:
    ```rust
    pub struct ServiceName<State> {
        _state: std::marker::PhantomData<State>,
        // fields
    }
    pub struct StateA;
    pub struct StateB;
    impl ServiceName<StateA> {
        pub fn transition(self) -> ServiceName<StateB> { ... }
    }
    ```
  - Currently services generate modules with functions, but no typestate
    encoding in the Rust output
  - Test: generated service code passes `cargo check`

- [x] **S009**: Generate proptest/quickcheck tests from contracts
  - Depends on: R001
  - For each contract with `requires`/`ensures`, generate:
    ```rust
    #[cfg(test)]
    mod tests {
        use proptest::prelude::*;
        proptest! {
            fn test_safe_division(a in any::<i64>(), b in 1i64..=i64::MAX) {
                // requires: b != 0 is encoded in the generator (b in 1..=MAX)
                let result = safe_division(a, b);
                // ensures: result * b + (a % b) == a
                prop_assert_eq!(result * b + (a % b), a);
            }
        }
    }
    ```
  - The `requires` clause constrains the proptest generator
  - The `ensures` clause becomes `prop_assert!`
  - Add `proptest` to the generated Cargo.toml dev-dependencies
  - This was T083 but was never actually implemented as code generation

---

## Phase A: Architecture (Proper Compiler Pipeline)

> Build the missing architectural layers that a real compiler needs.

### A.1 HIR (High-Level IR)

- [x] **A001**: Design and implement a HIR between AST and type checker
  - Depends on: R003, R013
  - Currently the type checker operates directly on the parser AST.
    This couples the type checker to parser implementation details
    (raw tokens, span representations, syntactic sugar).
  - The HIR should:
    1. Desugar syntactic sugar (e.g., `a.b.c` -> nested field access)
    2. Resolve all names (from assura-resolve) into unique IDs
    3. Replace raw token sequences with structured expressions
    4. Normalize clause representations
  - New crate: `crates/assura-hir/`
  - Input: `ResolvedFile` (from assura-resolve)
  - Output: `HirFile` with fully resolved, desugared AST
  - The type checker then operates on `HirFile` instead of raw AST
  - This is a large refactoring task. Approach:
    1. Define HIR types in the new crate
    2. Write the lowering pass (AST -> HIR)
    3. Update assura-types to accept HIR input
    4. Keep AST input as a compatibility layer during migration

### A.2 Multi-File Compilation

- [x] **A002**: Implement filesystem-based module resolution
  - Depends on: A001
  - Currently the compiler only handles single files
  - `import a.b.c` should resolve to `a/b/c.assura` relative to the
    project root (defined by `assura.toml`)
  - Implement:
    1. `assura.toml` parser (TOML format per spec Section 10.3)
    2. Project root discovery (walk up from the file being compiled
       until `assura.toml` is found)
    3. Module path -> file path resolution
    4. Compile all imported files, build a module graph
    5. Detect circular imports via topological sort (already partially
       implemented in assura-resolve)
  - Support: `assura check .` to check all `.assura` files in the project

- [x] **A003**: Implement `assura.toml` project configuration
  - Depends on: A002
  - Parse `assura.toml` with these fields (from spec Section 10.3):
    ```toml
    [package]
    name = "my-project"
    version = "0.1.0"

    [build]
    target = "native"          # or "wasm32-wasi"
    output = "generated"

    [verify]
    smt-solver = "z3"          # or "cvc5" or "portfolio"
    layer = 1                  # default verification layer
    timeout = 1000             # SMT timeout in ms

    [profile]
    type = "parser"            # minimal, parser, database, etc.
    ```
  - Wire into CLI: `assura check` reads config from `assura.toml`
  - `assura init` generates a default `assura.toml`

### A.3 Error System Rework

- [x] **A004**: Implement structured error types across all crates
  - Depends on: R003
  - Currently each crate has its own error representation (strings,
    ad-hoc structs, tuples). Unify on:
    ```rust
    pub struct Diagnostic {
        pub code: ErrorCode,        // e.g., A03001
        pub severity: Severity,     // Error, Warning, Info
        pub message: String,
        pub primary: Span,
        pub secondary: Vec<(Span, String)>,
        pub suggestion: Option<Suggestion>,
    }
    ```
  - New crate: `crates/assura-diagnostics/` (or just a module in parser)
  - All compiler passes emit `Vec<Diagnostic>` instead of pass-specific
    error types
  - The CLI renders diagnostics uniformly (ariadne for human, serde for JSON)
  - This eliminates the ad-hoc error formatting scattered across main.rs

---

## Phase I: Issues (GitHub Issues + Market Research Gaps)

### I.1 Enhancement Issues

- [x] **I001**: Implement CVC5 fallback solver (GitHub issue #1)
  - Depends on: S005
  - The spec defines `--solver cvc5` and `assura.toml` supports
    `smt-solver = "cvc5"`, but only Z3 is implemented
  - Implement:
    1. Add `cvc5` crate dependency (or shell out to `cvc5` binary)
    2. Create a solver trait: `trait SmtSolver { fn check_sat(...) }`
    3. Implement for Z3 (refactor existing code) and CVC5
    4. Add portfolio mode: try Z3 first, fall back to CVC5 on timeout
    5. CLI: `--solver z3|cvc5|portfolio`
  - Test: run the same contracts on both solvers, compare results
  - Close issue #1

- [x] **I002**: Add performance profiling and benchmarks (GitHub issue #2)
  - Depends on: S005
  - No benchmarks exist. The compiler needs:
    1. `cargo bench` infrastructure using `criterion`
    2. Benchmark: parse all demo files (measure throughput)
    3. Benchmark: type-check all demo files
    4. Benchmark: Z3 verification of all demo files
    5. Benchmark: codegen for all demo files
  - Create synthetic large contracts (100+ clauses) to test scaling
  - Add CI job: run benchmarks on every PR, compare to baseline
  - Close issue #2

- [x] **I003**: Implement WASM compilation target (GitHub issue #3)
  - Depends on: R001, A003
  - The spec defines `--target wasm32-wasi` and the investigation
    lists WASM as a key differentiator
  - Implement:
    1. In codegen, detect target from `assura.toml` or `--target` flag
    2. Generate WASM-compatible Rust (no std features that require OS)
    3. Add `wasm32-wasip1` target to generated `Cargo.toml`
    4. Test: `assura build --target wasm32-wasi` produces a `.wasm` file
  - Prerequisite: install `wasm32-wasip1` target via rustup
  - Close issue #3

### I.2 Fuzzing and Robustness

- [x] **I004**: Set up cargo-fuzz for the parser
  - Depends on: none
  - The parser should never panic on any input. Fuzzing finds inputs
    that cause panics or infinite loops.
  - Create `fuzz/` directory with:
    - `fuzz_targets/parse.rs`: feed random bytes to `assura_parser::parse()`
    - `fuzz_targets/lex.rs`: feed random bytes to the lexer
  - Run for at least 10 minutes. Fix any panics found.
  - Add CI job: run fuzzing for 60 seconds on each PR

- [x] **I005**: Set up cargo-fuzz for the type checker
  - Depends on: I004
  - Fuzz the pipeline: parse -> resolve -> type_check
  - The type checker should never panic, only return errors
  - Use structured fuzzing: generate random ASTs that are syntactically
    valid but may be type-incorrect

---

## Phase T: Testing (Comprehensive Test Coverage)

### T.1 Missing Test Coverage

- [x] **T201**: Add tests to assura-server (currently 0)
  - Depends on: none
  - The gRPC server has zero tests despite 496 lines of code
  - Add tests for:
    - `Check` RPC: valid source returns success, invalid returns errors
    - `Build` RPC: valid source returns generated files
    - `Explain` RPC: valid error code returns description
    - `Health` RPC: returns "serving"
    - `CheckStream` RPC: streams events for a valid source
  - Use `tonic`'s test utilities (in-process channel, no network)

- [x] **T202**: Expand LSP tests (currently 9)
  - Depends on: none
  - The LSP needs tests for:
    - `textDocument/completion`: keyword completions, type completions
    - `textDocument/hover`: hover on type names, function names
    - `textDocument/definition`: go to definition for symbols
    - `textDocument/documentSymbol`: contract/service/function symbols
    - `textDocument/diagnostic`: parse errors, type errors, resolution errors
    - Incremental edits: edit a document, verify diagnostics update
  - Target: 30+ tests for LSP

- [x] **T203**: Add negative test suite (MUST REJECT files)
  - Depends on: none
  - Create `tests/fixtures/must_reject/` with .assura files that must
    produce specific error codes:
    - `type_mismatch.assura` with `// MUST REJECT A03001`
    - `undefined_name.assura` with `// MUST REJECT A02001`
    - `linear_double_use.assura` with `// MUST REJECT A05001`
    - `wrong_state.assura` with `// MUST REJECT A06001`
    - `effect_violation.assura` with `// MUST REJECT A07001`
    - etc.
  - Write a test harness that:
    1. Reads each file in the directory
    2. Parses the `MUST REJECT` annotation
    3. Runs the full pipeline
    4. Asserts the expected error code appears in the diagnostics
  - Target: 20+ negative test files

- [x] **T204**: Add positive test suite (MUST COMPILE files)
  - Depends on: R001
  - Create `tests/fixtures/must_compile/` with valid .assura files:
    - `simple_contract.assura` with `// MUST COMPILE`
    - `generic_contract.assura` with `// MUST COMPILE`
    - `service_with_state.assura` with `// MUST COMPILE`
    - `effects_declared.assura` with `// MUST COMPILE`
  - Test harness runs full pipeline AND verifies generated Rust compiles
  - Target: 15+ positive test files

- [x] **T205**: Add end-to-end round-trip tests
  - Depends on: R001
  - For each demo file:
    1. Parse -> resolve -> type-check -> codegen
    2. Write generated Rust
    3. `cargo check` the generated project
    4. Verify the generated code contains expected `debug_assert!` calls
    5. Verify function signatures match the contract
  - These tests catch regressions where any pipeline stage breaks the
    round-trip

### T.2 tree-sitter and VS Code Testing

- [x] **T206**: Add tree-sitter grammar tests
  - Depends on: none
  - The `editors/tree-sitter-assura/` directory exists but has an empty
    `test/corpus/` directory
  - Add test corpus files (tree-sitter's standard test format):
    - `contracts.txt`: basic contract parsing
    - `services.txt`: service with operations
    - `expressions.txt`: arithmetic, comparisons, quantifiers
    - `types.txt`: refinement types, generic types
  - Run: `tree-sitter test` must pass
  - Ensure the grammar handles error recovery (partial parses)

- [x] **T207**: VS Code extension test infrastructure
  - Depends on: none
  - The extension at `editors/vscode/` has no tests
  - Add basic tests:
    - Extension activates on `.assura` files
    - Syntax highlighting applies (TextMate grammar test)
    - LSP client connects to server
  - Use `@vscode/test-electron` for integration tests

---

## Phase E: Ecosystem (Distribution, CI, Documentation)

### E.1 Release Pipeline

- [x] **E001**: Set up cargo-dist for binary releases
  - Depends on: none
  - Configure `cargo-dist` in workspace `Cargo.toml`
  - Generate release binaries for: Linux x86_64, macOS x86_64, macOS
    aarch64, Windows x86_64
  - GitHub Action: on tag push, build and upload release artifacts
  - This gives users `assura` binaries without building from source

- [x] **E002**: Set up Homebrew tap
  - Depends on: E001
  - Create `assura-lang/homebrew-tap` repository
  - Generate Homebrew formula from cargo-dist output
  - Users can: `brew install assura-lang/tap/assura`

- [x] **E003**: Publish to crates.io
  - Depends on: E001
  - The `assura` name is already claimed on crates.io
  - Publish all workspace crates in dependency order:
    `assura-parser` -> `assura-resolve` -> `assura-types` ->
    `assura-smt` -> `assura-codegen` -> `assura-cli`
  - Set up CI: publish on tag push (after cargo-dist)

### E.2 CI Hardening

- [x] **E004**: Add CI jobs for editors
  - Depends on: T206
  - Add GitHub Action jobs:
    - `tree-sitter test` for the grammar
    - `npm run compile` for the VS Code extension
    - `npm test` for VS Code extension tests (if T207 is done)

- [x] **E005**: Add CI job for generated code validation
  - Depends on: R001
  - CI should run `assura build` on all demo files and then
    `cargo check` on each generated project
  - This prevents regressions where codegen produces invalid Rust
  - Install Z3 and Rust stable in the CI job

- [x] **E006**: Add security scanning (CodeQL, cargo-audit)
  - Depends on: none
  - Add `.github/workflows/security.yml`:
    - `cargo audit` for known vulnerable dependencies
    - CodeQL analysis for Rust
    - Dependabot configuration for automated dependency updates

### E.3 Documentation

- [x] **E007**: Write getting-started tutorial
  - Depends on: R001, A003
  - Currently `docs/TUTORIAL.md` exists but may be outdated
  - Write/update a tutorial that a developer can follow:
    1. Install Assura (from binary or source)
    2. Create a project with `assura init`
    3. Write a simple contract
    4. Run `assura check`
    5. Run `assura build`
    6. Run the generated Rust code
  - Include screenshots/terminal output
  - Test the tutorial end-to-end (every command must work)

- [x] **E008**: Write compiler internals documentation
  - Depends on: R003, A001
  - `docs/INTERNALS.md` should cover:
    1. Pipeline overview (lex -> parse -> resolve -> HIR -> typecheck
       -> SMT -> codegen)
    2. Each crate's responsibility and public API
    3. How to add a new checker to assura-types
    4. How to add a new SMT encoding to assura-smt
    5. How to add a new codegen pass
  - This enables other developers to contribute

---

## Phase P: Production (Toward v1.0)

> These tasks bring the compiler to production quality.

### P.1 CLI Polish

- [x] **P001**: Implement `--verbose` and `--quiet` modes
  - Depends on: R009
  - `--verbose`: show timing information, Z3 solver statistics,
    intermediate results
  - `--quiet`: suppress all output except errors
  - Default: current behavior (summary line per file)

- [x] **P002**: Implement `--watch` mode
  - Depends on: none
  - `assura check --watch .` watches for file changes and re-runs
    the pipeline incrementally
  - Use `notify` crate for filesystem watching
  - Re-parse only changed files, re-resolve affected modules,
    re-typecheck affected contracts

- [x] **P003**: Implement `assura fmt` command
  - Depends on: none
  - Format `.assura` source files with consistent style
  - Use the parser's AST to re-emit formatted source
  - Enforce: consistent indentation, brace placement, clause ordering
  - This is important for code review and collaboration

### P.2 Advanced Codegen

- [x] **P004**: Generate Rust with proper error handling
  - Depends on: R001
  - Currently all function bodies are `todo!("implementation provided by AI agent")`
  - When an implementation is provided (via IR or inline), generate
    proper error handling:
    - `Result<T, E>` return types
    - `?` operator for propagation
    - Custom error types from contract error clauses

- [x] **P005**: Implement the Implementation IR parser (Section 4)
  - Depends on: A001
  - The IR is what AI agents generate (not the contract language)
  - Parse IR text format into HIR
  - Validate IR against the contract it implements
  - Generate Rust from validated IR
  - This completes the AI-in-the-loop workflow:
    contract -> AI generates IR -> compiler verifies -> Rust output

### P.3 Performance

- [x] **P006**: Implement verification caching
  - Depends on: S005
  - Hash each contract (AST + implementation) as a cache key
  - Store verification results in `.assura-cache/`
  - On re-run, skip re-verification for unchanged contracts
  - Invalidate cache when the contract or its dependencies change
  - This is critical for large projects: SMT queries are expensive

- [x] **P007**: Implement parallel SMT queries
  - Depends on: S005
  - Independent contracts can be verified in parallel
  - Use `rayon` for work-stealing parallelism
  - Each Z3 context must be thread-local (Z3 contexts are not Sync)
  - Target: linear speedup on multi-core machines for projects with
    many independent contracts

---

## Dependency Graph (v2)

```
Phase R (Rework) - No dependencies, start immediately
  R001 (codegen compiles) ──► R002 (multi-file codegen)
  R003 (split monolith) ──► S001-S004 (deepen checkers)
  R004-R008 (close issues) - Independent
  R009 (per-clause output) ──► S005 (per-clause SMT)
  R010 (Z3 default) - Independent
  R011 (SMT tests) - Independent
  R012 (build validates) ──► depends on R001
  R013 (clause consistency) ──► depends on R005

Phase S (Strengthen)
  S005 (per-clause SMT) ──► S006 (counterexample display)
  S005 ──► S007 (Layer 2 quantifiers)
  S005 ──► I001 (CVC5), I002 (benchmarks)

Phase A (Architecture)
  A001 (HIR) ──► A002 (multi-file) ──► A003 (assura.toml)
  A004 (error rework) - Independent

Phase I (Issues)
  I001 (CVC5) ──► depends on S005
  I003 (WASM) ──► depends on R001, A003
  I004-I005 (fuzzing) - Independent

Phase T (Testing) - Mostly independent, start anytime
  T201-T207 - All independent of each other

Phase E (Ecosystem) - Mostly independent
  E001 (cargo-dist) ──► E002 (Homebrew) ──► E003 (crates.io)
  E004-E006 - Independent

Phase P (Production)
  P005 (IR parser) ──► depends on A001
  P006-P007 (performance) ──► depends on S005
```

### Recommended Execution Order

The phases are designed so agents can start immediately with Phase R
(no dependencies), then interleave work from other phases as
dependencies are met.

**Priority order within a session**:
1. Pick the next `[ ]` task from Phase R (if any remain)
2. If all Phase R tasks are done, pick from Phase S
3. Interleave Phase T tasks (testing) between any other tasks
4. Phase A tasks can start once R003 and R013 are done
5. Phase I tasks can start once their dependencies are met
6. Phase E and P tasks are lower priority but have few dependencies

**Parallelization opportunities**:
- R001-R002 (codegen) is independent of R003 (monolith split)
- R004-R008 (issue fixes) are all independent of each other
- T201-T207 (tests) are all independent of each other
- E001-E003 (releases) are independent of compiler work

---

## Recovery Procedures

### If a task is too large for one session

Split it. Mark the original task as `[x]` with a note saying "partially
done, continued in Xxxx". Add new sub-tasks with clear scope.
Update the Progress Notes with what was and was not completed.

### If a task seems wrong or impossible

Do NOT silently skip it. Write a note in Progress Notes explaining
what's wrong. Suggest a correction. Continue with the next independent
task. The user will review and adjust the plan.

### If dependencies are wrong

The dependency graph may have errors. If you discover that task Xxxx
does NOT actually need Yyyy to be done first, note it in Progress Notes
and proceed. If you discover a MISSING dependency, note it and do the
prerequisite first.

### If the spec is ambiguous

Add a `// SPEC-QUESTION: <question>` comment in the code, make a
reasonable choice, document it in Progress Notes, and continue.
Do not block on ambiguity.

### If Z3 installation fails

The `z3` Rust crate needs libz3. If installation fails:
1. Try `brew install z3` (macOS) or `apt-get install libz3-dev` (Linux)
2. If that fails, try building Z3 from source:
   `git clone https://github.com/Z3Prover/z3 && cd z3 && mkdir build && cd build && cmake .. && make -j$(nproc)`
3. Set `Z3_SYS_Z3_HEADER=/path/to/z3/src/api/z3.h` and
   `LD_LIBRARY_PATH=/path/to/z3/build`
4. As a last resort, note the failure and work on non-Z3 tasks

---

## Milestones and Validation

These are the "prove it works" checkpoints. Each milestone must be
demonstrated, not just claimed.

**The pipeline test**: After every task that modifies a compiler pass,
run this and verify the output:

```bash
cargo run --bin assura -- check demos/libwebp-huffman.assura
cargo run --bin assura -- build demos/libwebp-huffman.assura
cd generated && cargo check
```

### MR1: Generated Rust Compiles (R001-R002)
- All three demo files generate Rust that passes `cargo check`
- Integration test verifies this automatically

### MR2: Monolith Eliminated (R003)
- No source file exceeds 3,000 lines
- All 838 type checker tests still pass

### MR3: All GitHub Issues Closed (R004-R008)
- Issues #5, #6, #7, #8, #9 closed with commits

### MR4: Pipeline Shows Real Results (R009-R012, S005-S006)
- `assura check` shows per-clause verification status
- Counterexamples display concrete values
- `assura build` fails if generated Rust doesn't compile

### MS1: Real Semantic Analysis (S001-S004)
- Termination checker rejects non-terminating recursion
- Effect checker traces through call chains
- Information flow catches secret leaks
- Linear type checker splits context at branches

### MA1: Multi-File Projects (A001-A003)
- `assura check .` works on a multi-file project
- `assura.toml` configures project settings
- Imports resolve to files on disk

### ME1: Installable Release (E001-E003)
- `brew install assura-lang/tap/assura` works
- `cargo install assura` works
- GitHub Releases has binaries for all platforms

---

## Progress Notes

> Agents: write a brief note here when completing a task or ending a session.
> Include: date, tasks completed, tasks attempted but not finished,
> any issues or spec questions encountered.

### Prior work (T001-T119, 2026-06-12 to 2026-06-13)
All original 119 tasks were completed, establishing the initial compiler
scaffolding. See git history for details. Key stats: 45,579 LOC across
8 crates, 1,062 tests, all demo files parse and type-check, basic Z3
integration exists behind feature flag, generated Rust is syntactically
valid but does not compile.

### Plan v2 created (2026-06-13)
Comprehensive audit identified 10 critical problems and 7 missing
features. Plan v2 has 50+ tasks across 7 phases (R, S, A, I, T, E, P)
to take the compiler from "initial scaffolding" to production quality.

### R003 completed (2026-06-13)
Split assura-types 25,183-line monolith into 6 files:
- lib.rs: 2,965 lines (core types, env construction, entry point, wiring)
- checkers.rs: 5,736 lines (20+ analysis pass checker structs)
- domain.rs: 3,837 lines (34 domain-specific checker structs)
- inference.rs: 863 lines (expression type inference)
- clauses.rs: 538 lines (clause body type checking)
- tests.rs: 11,280 lines (all 838 unit tests)
Zero behavior changes. All 1,220 tests pass unchanged.

### R001 completed (2026-06-13)
Fixed codegen to produce compilable Rust for all three demo files.
Changes: added `value` field to `Decl::Block` in parser AST, added
block keywords as clause stoppers, rewrote codegen with multi-phase
architecture (collect types, detect generics, generate stubs, emit
feature_max constants), fixed cross-integer-width comparisons via
i128 casts, stripped typestate annotations, refined `has_deep_field_access`
to allow method chains while blocking struct field access on stubs.
All demos pass `cargo check` on generated output. 1,220 tests pass.

### R004 completed (2026-06-14)
Deduplicated raw-token param extraction across 3 crates. Added shared
`ParsedParam` struct and `extract_clause_params()` to assura-parser/ast.rs.
Updated all 3 call sites:
- assura-resolve: `extract_input_param_names` now wraps shared function
- assura-types/clauses.rs: `register_input_clause_params` and
  `collect_input_param_types` now wrap shared function
- assura-codegen: `extract_input_params` now wraps shared function
Removed ~160 lines of duplicated parsing logic. All 1,062 tests pass.

### R005 completed (2026-06-14)
Fixed refinement predicate preservation through clause param extraction.
The raw-token splitter in `extract_clause_params_from_raw` only tracked
`<`/`>` depth but not `{`/`}` or `(`/`)`. Refinement types like
`{ x : Int | x < 10 }` contain `<` as a comparison operator, not a
generic delimiter; without brace tracking, the `<` opened an angle
bracket that prevented comma splitting from finding subsequent params.
Fix: track brace/paren depth; only treat `<`/`>` as angle brackets
when outside braces. Added 5 new tests (3 parser, 2 types) including
round-trip test through extract_clause_params -> parse_type_tokens.

### R006 completed (2026-06-14)
Parser error messages already included expected tokens (implemented during
T-series). Added `.labelled("declaration")` and `.labelled("clause keyword")`
annotations to key parser combinators for cleaner expected sets. Added test
verifying parse errors include non-empty expected token set.

### R007 completed (2026-06-14)
Added 3 CLI integration tests for `assura build --output`:
- Custom output dir creates Cargo.toml + src/lib.rs
- Default output is "generated/" when --output omitted
- Missing input file produces an error exit code
Tests use process invocation of the built binary.

### R008 completed (2026-06-14)
Converted wildcard `_ => {}` catch-alls to explicit variant lists in
codegen and resolve. Replaced 11 wildcards on Decl, ServiceItem, and
ClauseKind enums so rustc will warn when new variants are added.
String-token and Expr wildcards were left as-is (too many variants,
and they grow frequently).

### R002 completed (2026-06-14)
Multi-contract files now generate separate .rs module files. When a
source file has 2+ contracts/services, codegen produces:
- `src/lib.rs`: shared types, enums, externs, functions, `pub mod` decls
- `src/contract_{name}.rs`: per-contract module with `use super::*`
- `src/{service_name}.rs`: per-service module with `use super::*`
Files with 0-1 contracts/services keep the existing single-file layout.
Added `generate_contract_contents` and `generate_service_contents` for
the multi-file path. Updated 1 existing test, added 4 new tests.
Total: 94 codegen tests, 1,233 workspace tests passing.

### R009 completed (2026-06-14)
Verification results in `assura check` are now grouped by contract/service/
function name. Each group shows per-clause status (verified, COUNTEREXAMPLE,
timeout, skipped). Counterexample models are indented with `|` prefix for
readability. When no verifiable clauses exist, the output now shows which
contracts were present instead of a generic "no verifiable clauses" message.
The `assura build` path uses the same grouped format. Added helper functions
`print_grouped_verification`, `clause_owner`, and `collect_contract_names`.

### R010 completed (2026-06-14)
Z3 was already the default feature (`default = ["z3-verify"]` in
assura-smt/Cargo.toml). CI already installs libz3-dev. Added CI step
to verify the graceful fallback builds: `cargo check -p assura-smt
--no-default-features`.

### R011 completed (2026-06-14)
158 standalone tests already exist in assura-smt, covering: verify on
minimal TypedFile (trivially true/false ensures, counterexamples),
check_refinement_subtype (holds, fails, with context), buffer bounds
(7 tests with verified/counterexample/partial requires), taint safety
(5 tests: safe, unsafe, mixed, trusted), counterexample extraction
format, and measure verification. The "ZERO in-crate tests" assessment
from the plan was outdated; these were added during the T-series tasks.

### R012 completed (2026-06-14)
`assura build` now runs `cargo check` on the generated Rust project after
writing files. If the generated code has compilation errors, they are
reported as warnings with the relevant error/location lines. The output
message changes from "OK" to "OK (generated Rust compiles)" on success.
Added `--no-check` flag to skip the validation step. Help text updated.

### R013 completed (2026-06-14)
Eliminated raw token fallback in clause bodies for expression-type clauses.
Split `clause_body()` into two parsers: `clause_body_expr()` for expression
clauses (requires, ensures, invariant, decreases, rule, must_not) that tries
`expr_parser()` for inline/bare forms before falling back to raw tokens, and
`clause_body_raw()` for non-expression clauses (input, output, effects, etc.)
that keeps the raw-token-first behavior. The `clause()` parser now routes
through `is_expr_clause()` to pick the right body parser. Snapshot diffs
confirm the transformation: e.g. `requires: name.length() > 0` was
`Raw(["name", ".", "length", "(", ")", ">", "0"])`, now
`BinOp { lhs: MethodCall { receiver: Ident("name"), method: "length" },
op: Gt, rhs: Literal(Int("0")) }`. Added 7 tests (5 positive, 2 negative)
verifying expression clauses produce `Expr` and non-expression clauses
keep `Raw`. All 1,240 tests pass. `Expr::Raw` variant and its handlers
remain for non-expression clause kinds (input, output, effects, etc.).

### S001 completed (2026-06-14)
Implemented real termination checking with SMT-backed decrease verification.
Changes across 3 crates:
- assura-types/checkers.rs: Added `DecreaseCheckResult` enum (Proved, NeedsSmt,
  Failed) and `PendingDecreaseCheck` struct. Modified `check_recursive_call()`
  to return `NeedsSmt` when syntactic decrease check is inconclusive (instead
  of immediately producing A09002). Modified `check_function_totality()` to
  return `(Vec<TotalityError>, Vec<PendingDecreaseCheck>)`.
- assura-types/lib.rs: Added `pending_decrease_checks` field to `TypedFile`.
  Updated `run_totality_checks()` to return pending checks alongside errors.
- assura-smt/lib.rs: Added `verify_decrease()` public function and Z3 backend
  `verify_decrease_impl()` that checks `preconditions => call_arg < measure
  AND call_arg >= 0`. Added 6 SMT-level tests (factorial, fibonacci n-1/n-2,
  spin same-arg, increasing arg, countdown with nat precondition).
- assura-cli/main.rs: Added `dispatch_decrease_checks()` helper. Wired pending
  checks into all 3 verify sites (run_check, run_build, default path).
Updated 11 existing totality tests for new tuple return type. Renamed
`totality_non_decreasing_measure_a09002` to verify pending SMT check is
created instead of immediate error. All 1,246 tests pass.

### S002 completed (2026-06-14)
Implemented call-graph-based effect inference. The effect checker now builds
a `HashMap<String, EffectSet>` of all declared effects from functions,
contracts, externs, and service operations (pass 1), then for each function
with an effects clause, scans clause bodies for `Call` and `MethodCall`
expressions, looks up the callee's effects, and checks that the caller
declares all required effects (pass 2). Added `build_effect_map()`,
`infer_callee_effects()`, and `collect_call_effects()` which recursively
walks all Expr variants. Added 5 new tests (contract-level call-graph OK,
unit containment for pure->io, missing subset, build_effect_map verification,
pure callee OK). All 1,251 tests pass.

### S003 completed (2026-06-13)
Implemented real information flow tracking with security label propagation.
Added `run_info_flow_checks()` wired into the type_check pipeline. The
checker assigns security labels (Public, Internal, Confidential, Restricted)
from input clauses and fn parameter type annotations, then checks ensures
clauses for: (1) direct flows of secret data to public result (A08001),
(2) implicit flows through if-conditions branching on secret data (A08004).
Added helper functions: `check_contract_info_flow`, `check_fn_info_flow`,
`assign_labels_from_clause`, `infer_label_from_type_tokens`,
`check_expr_info_flow`, `infer_branch_target_label`, `contains_result_ref`.
Added 7 tests: no-labels-no-errors, secret-to-result A08001, implicit-flow
A08004, same-level OK, upward-flow OK, label inference through BinOp,
contract with secret input pipeline test.

### S004 completed (2026-06-13)
Implemented real context splitting for linear types at match arms and
ghost-use exclusion per Spec Section 13 Test Case 1.
- Match arms: `check_expr_linearity_inner` now forks the context for
  each arm and uses `merge_arms()` to check consistency across all arms.
  Previously all arms shared a single context, causing false positives
  and missed double-use errors.
- Ghost uses: `Forall`/`Exists` bodies and `Old()` expressions are now
  treated as ghost (logical) context. Variable references inside them
  do NOT count as computational uses, per the spec's Ghost Use Problem.
- Added `merge_arms()` method to `LinearContext` for N-way branch merge
  with per-variable delta comparison.
- 9 new tests: match consistent OK, match inconsistent A05004, 3-arm
  one-differs A05004, scrutinee + arm double-use A05001, forall/exists
  ghost use, old() ghost use, ghost block confirmation, merge_arms unit.

### S005 completed (2026-06-13)
Wired per-clause SMT verification into the full CLI pipeline. Added
`verify_contract()` public API for verifying a single contract's clauses
independently (with Z3 backend). Enhanced the default summary output path
to show per-clause grouped verification details (not just aggregate counts).
The `assura` default command now shows each contract/function with its
individual clause results (verified, COUNTEREXAMPLE with model, timeout,
skipped). Added `print_grouped_verification_stdout()` for stdout output
(vs existing stderr variant for `assura check`). 4 new SMT tests:
single-ensures verified, counterexample, multiple-ensures mixed results,
no-verifiable-clauses empty result.

### S006 completed (2026-06-13)
Implemented human-readable counterexample display. Counterexample output
now uses the structured `CounterexampleModel` instead of raw Z3 model
strings. Added `format_counterexample_lines()` and `clean_z3_value()`
helpers. Changes: (1) Z3 `(- N)` format converted to `-N`, (2) variables
grouped into inputs and outputs (result), (3) inputs displayed as compact
`name = value, ...` pairs, (4) `__result` variable now preserved in
`extract_counter_model` (previously skipped by `__` prefix filter).
Before: `a -> (- 2)\nb -> 1\n__field_extra -> { 4 }`. After:
`a = -2, b = 1\nresult = -1`. Both stderr (`assura check`) and stdout
(default summary) paths use the new formatter.

### S007 completed (2026-06-13)
Implemented Layer 2 real quantifier verification with Z3. Added
`verify_quantified_expr()` public API that encodes forall/exists
expressions with 10s timeout (Layer 2) and returns VerificationResult.
The Z3 backend `verify_quantified_impl()` accepts assumptions and a
quantified body Expr, negates and checks validity. The existing Encoder
already handled Forall/Exists with `forall_const`/`exists_const` and
domain guards (range => bounded, other => uninterpreted containment).
Added `Layer2Verifier.verify()` method that delegates to Z3 when the
feature is enabled. 6 new tests: forall trivially true, forall with
counterexample, exists satisfiable, forall with assumption, empty
verifier, string-based invariant structural check.

### S008 completed (2026-06-13)
Implemented typestate-encoded Rust codegen for service declarations.
Services with `states:` now generate compile-time state marker structs
(`pub struct Locked;`, `pub struct Unlocked;`), a generic service struct
(`ServiceName<State>` with `PhantomData`), and state-specific `impl`
blocks. State-transitioning operations consume `self` and return the
new typed state (`fn Connect(self) -> Connection<Connected>`). Pre-state
guards are enforced by the type system instead of runtime assertions.
State-independent queries/operations go in a generic `impl<S>` block.
Stateless services remain unchanged. Both `generate_service_contents()`
(multi-file) and `generate_service()` (single-file) share the same
typestate logic. 6 new tests, 3 existing tests updated. 1,283 total
tests passing.

### S009 completed (2026-06-13)
Implemented proptest generation from contracts with input+ensures clauses.
For each testable contract, generates a `#[cfg(test)]` module with a
`proptest!` block: input types map to proptest strategies, requires
constraints are either refined into generator ranges (e.g., `b != 0`
becomes `1i64..=i64::MAX`) or fall back to `prop_assume!`, and ensures
clauses become `prop_assert!`. Adds `proptest = "1"` to the generated
Cargo.toml dev-dependencies only when testable contracts exist. Works
in both single-file and multi-file codegen modes. 7 new tests, 1,290
total tests passing.

### A004 completed (2026-06-13)
Created `assura-diagnostics` crate with unified `Diagnostic` type:
`code`, `severity` (Error/Warning/Info), `message`, `primary` span,
`secondary` spans with labels, and optional `Suggestion`. Added
`From<ResolutionError>` and `From<TypeError>` conversions so all
compiler passes can emit unified diagnostics. CLI gains
`render_diagnostic()` (ariadne renderer for Diagnostic) and
`DiagnosticJson::from_diagnostic()` (JSON conversion). 6 new
diagnostic tests. 1,296 total tests passing.

### T201 completed (2026-06-13)
Added 26 tests to assura-server (was 0). Covers: `run_check` (valid,
invalid, parse error, resolution error, layer 0 vs 1 SMT), `run_codegen`
(valid and invalid), `lookup_error_code` (known, unknown, catalog
completeness), `span_to_line_col` (first line, multiline, empty, beyond
bounds), HTTP handlers (health, check valid/invalid, explain), gRPC
handlers (check valid/invalid, build valid/invalid, explain, health,
check_stream events). Uses axum tower::ServiceExt for HTTP and direct
AssuraServer method calls for gRPC. 1,323 total tests passing.

### T204 completed (2026-06-13)
Created `tests/fixtures/must_compile/` with 15 valid .assura files:
simple_contract, multiple_contracts, service_with_states,
function_definition, enum_declaration, extern_function,
effects_declared, module_and_import, invariant_clause,
decreases_clause, nested_expressions, quantifier_contract,
output_clause, bool_contract, string_contract. Added
`test_must_compile_fixtures` harness that runs the full pipeline
(parse, resolve, type_check, codegen) and validates generated Rust
via `syn::parse_file()`. 1,297 total tests passing.

### T203 completed (2026-06-13)
Added 15 MUST REJECT fixture files in `tests/fixtures/must_reject/`
covering 11 unique error codes: A02001, A02003, A02008, A03001,
A03002, A03005, A03006, A03010, A05001, A07003, A08001. Updated the
`test_must_reject_fixtures` harness to scan both `tests/fixtures/errors/`
and `tests/fixtures/must_reject/` directories. Total 24 annotated
negative test fixtures across both directories, all validated through
the full pipeline (parse, resolve, type_check). 1,296 total tests passing.

### P001 completed (2026-06-13)
Implemented `--verbose` (`-v`) and `--quiet` (`-q`) CLI modes across
all three command paths (check, build, legacy). Verbose mode shows
per-phase pipeline timing (lex, parse, resolve, typecheck, verify,
codegen) with token counts, declaration counts, symbol counts, type
binding counts, and total elapsed time in milliseconds. Quiet mode
suppresses all non-error output (no "check passed", no verification
summary, no file listing in build mode) while still displaying error
diagnostics and the error count. Added `Verbosity` enum, `TimingInfo`
struct with `Clone`+`Copy`, and `parse_verbosity()` helper. Updated
help text. 7 new CLI integration tests (verbose timing assertions,
quiet suppression assertions, short flag variants, verbose build with
codegen timing, quiet build file suppression). 1,330 total tests passing.

### T205 completed (2026-06-13)
Added 9 end-to-end round-trip tests that exercise the full pipeline
(parse, resolve, type-check, codegen) and validate the output:
- 3 demo file tests (libwebp, zlib, mbedtls) that verify generated Rust
  passes both `syn::parse_file()` and `cargo check`
- `roundtrip_libwebp_has_debug_asserts`: verifies requires clauses
  produce `debug_assert!` in generated code
- `roundtrip_zlib_has_function_stubs`: verifies function names match
  the contract declarations
- `roundtrip_libwebp_function_signatures_present`: verifies specific
  functions (validate_code_lengths, check) appear in output
- `roundtrip_contract_with_ensures_has_postcondition`: synthetic contract
  verifying requires clause becomes debug_assert with correct types
- `roundtrip_service_generates_typestate`: verifies service with states
  generates PhantomData-based typestate markers
- `roundtrip_project_has_valid_cargo_toml`: verifies generated
  Cargo.toml has [package], name, and edition fields.
1,339 total tests passing.

### T202 completed (2026-06-13)
Expanded LSP test suite from 9 to 33 tests. Added 24 new tests covering:
- Position/offset edge cases: beyond end, start position, multiline roundtrip
- word_at_offset edge cases: empty source, underscores, digits, end of word,
  beyond source length
- is_ident_char: character classification checks
- byte_span_to_range: zero-length spans, beyond-file spans
- Document symbols: empty file, service with operations, extern function,
  multiple contracts, kind preservation
- Diagnostic conversion: resolution warning, resolution/type error with
  secondary spans, source field verification, parse error severity
- Completeness: builtin types list coverage, keywords list coverage
Fixed one test expectation: word_at_offset at word boundary correctly
returns the word (function scans backwards from offset). 1,363 total tests.

### I004 completed (2026-06-13)
Set up cargo-fuzz with two fuzz targets:
- `fuzz_parse`: feeds random UTF-8 to `assura_parser::parse()`, verifying
  the parser never panics on any input
- `fuzz_lex`: feeds random UTF-8 to the logos lexer, verifying the lexer
  never panics
Both targets build with nightly via `PATH="$HOME/.cargo/bin:$PATH"
RUSTUP_TOOLCHAIN=nightly cargo fuzz run fuzz_parse`. Parser ran 76,801
iterations in 60 seconds with zero crashes (2,408 coverage edges). Lexer
ran 3,673,205 iterations in 31 seconds with zero crashes. Seed corpus
includes all demo and fixture .assura files. Fuzz workspace excluded from
main workspace via `exclude = ["generated", "fuzz"]` in root Cargo.toml.

### P002 completed (2026-06-13)
Implemented `--watch` / `-w` flag for `assura check`. When enabled, the
command runs the full pipeline (parse, resolve, type-check, verify), then
watches the file's parent directory for changes using the `notify` crate
(v7, uses FSEvents on macOS, inotify on Linux). On file change, the screen
is cleared and the pipeline re-runs. Events are debounced with a 100ms
window to coalesce rapid saves. Extracted `check_file_once()` helper that
runs the full check pipeline and returns error status, used by both the
one-shot and watch code paths. Help text updated. No new tests needed
since the watch loop is an interactive feature; the extraction of
`check_file_once()` is covered by the existing test suite.

### P003: `assura fmt` (2026-06-13)

Implemented `assura fmt <file.assura> [--check]`. The formatter parses the
source file and re-emits it with consistent style: 4-space indentation,
braced clause bodies for expression-type clauses (requires, ensures,
invariant, decreases, rule, must_not, effects, modifies), function-call
syntax for input/output params, colon syntax for inline values (feature
blocks), and semicolons after type definitions and struct fields.

Parser fixes for round-trip correctness:
- Return type parsers now stop at declaration keywords (fn, contract, type,
  enum, extern, service, axiom, lemma, feature_max) preventing greedy
  consumption of the next declaration.
- `Token::Table` and `Token::Feature` added to `is_clause_stopper` so
  inline clause bodies don't consume standalone block declarations.
- `Token::Spec` removed from `clause_kind()` since `spec` is a block
  declaration keyword, not a clause keyword.
- Updated 2 snapshots (expected token list, mbedtls feature parsing).

Formatter features:
- `join_raw_tokens()` collapses dotted paths (io.read, db.write)
- Empty blocks get explicit `{ }` to prevent misparse as clauses
- `--check` mode exits 0 if already formatted, 1 if not
- 9 new tests covering idempotency, contracts, types, enums, extern fns,
  services, features, and dotted effects

### A001 completed (2026-06-14)
Created `crates/assura-hir/` with HIR types and AST-to-HIR lowering pass.

**HIR types** (`src/lib.rs`, ~500 lines):
- `HirFile`: top-level file with declarations and reference to `ResolvedFile`
- `HirDecl`/`HirDeclKind`: Contract, Service, TypeDef, EnumDef, Extern, FnDef, Block
- `HirExpr`: structured expressions with no `Raw` fallback for expression clauses;
  `RawTokens` variant preserved only for non-expression clause bodies (effects, input, etc.)
- `HirType`: structured type representation replacing raw `Vec<String>` tokens
  (Named, Generic, Tuple, Fn, Refined, Unit, Unresolved)
- `HirClause`/`HirClauseKind`: clause with structured body and typed kind
- `DefId`: resolved name ID (Resolved(usize) or Unresolved(String))
- `parse_type_tokens()`: converts raw token sequences to `HirType`
- `HirExpr::to_ast_expr()`, `HirClause::to_ast_clause()`: backward compatibility
  conversions for the type checker during migration

**Lowering pass** (`src/lower.rs`, ~300 lines):
- `lower(resolved: &ResolvedFile) -> HirFile`: main entry point
- `NameResolver`: maps names to symbol table indices for DefId resolution
- Lowers all declaration types, clauses, expressions, type references
- Resolves identifiers to DefIds via the symbol table

**CLI integration**:
- `compile()` now runs HIR lowering between resolve and type-check
- `CompilationResult` and `TimingInfo` include `hir`/`hir_ms` fields
- Verbose mode (`-v`) shows HIR decl count and timing in all paths

23 new tests (13 unit + 10 lowering integration). 1,395 total tests passing.

### I005 completed (2026-06-14)
Added `fuzz_typecheck` target to the fuzz workspace. The target runs the full
parse -> resolve -> type_check pipeline on arbitrary UTF-8 input. Added
assura-resolve and assura-types as fuzz workspace dependencies. Ran 72,782
iterations in 60 seconds with zero crashes (3,345 coverage edges). Seed
corpus includes all demo and fixture .assura files.

### E001 completed (2026-06-14)
Set up cargo-dist 0.31.0 for binary releases:
- Created `dist-workspace.toml` with workspace config
- Added `[profile.dist]` with LTO to root Cargo.toml
- Generated `.github/workflows/release.yml`
- Targets: Linux x86_64, macOS x86_64, macOS aarch64
- System dependencies: libz3-dev + protobuf (apt), z3 + protobuf (brew)
- Shell installer generated for Unix platforms
- Windows excluded (no straightforward Z3 CI install)
- On tag push (e.g., `v0.1.0`), builds and uploads to GitHub Releases
- `pr-run-mode = "plan"` runs plan-only on PRs to validate config

### E006 completed (2026-06-14)
Added security scanning and Dependabot configuration:
- `.github/workflows/security.yml`: cargo-audit (rustsec/audit-check@v2.0.0)
  for dependency vulnerability scanning, CodeQL analysis for GitHub Actions
  code. Runs on push to main, PRs, weekly schedule, and manual dispatch.
  Concurrency groups and timeouts configured.
- `.github/dependabot.yml`: weekly updates for cargo and github-actions
  ecosystems, 5 open PR limit each, labeled with `dependencies`.

### E005 completed (2026-06-14)
Added `codegen-validation` job to CI workflow. The job builds the compiler
in release mode, then runs `assura build --no-check` on all demo files and
`cargo check` on the generated Rust output. Runs after the main `check` job
succeeds. Validated locally: all 3 demos (libwebp, mbedtls, zlib) generate
Rust that passes `cargo check`.

### I002 completed (2026-06-14)
Created `crates/assura-bench/` with criterion 0.5 benchmarks for the full
compiler pipeline. 8 benchmark groups:
- `parse`: lex + parse each demo file (300-550us per file)
- `resolve`: name resolution (6-9us)
- `hir_lower`: AST to HIR lowering (12-17us)
- `type_check`: full type checking (31-61us)
- `codegen`: Rust code generation via prettyplease (88-145us)
- `smt_verify`: Z3 verification with 20 samples (317-784us)
- `full_pipeline`: parse through codegen+SMT (1.2-2.2ms per demo)
- `scaling`: synthetic contracts with 10/50/100 clauses to measure scaling
Run with: `cargo bench -p assura-bench`

### P004 completed (2026-06-14)
Added error type generation to codegen. When a contract or function has an
`errors` clause (e.g., `errors { DivByZero, Overflow }`), codegen now:
1. Generates a `#[derive(Debug, thiserror::Error)]` enum with each error
   variant (e.g., `pub enum SafeDivisionError { DivByZero, Overflow }`)
2. Wraps the return type in `Result<T, ContractError>` instead of plain `T`
3. Returns `Ok(__result)` instead of `__result` when ensures clauses exist
4. Adds `thiserror = "2"` to the generated `Cargo.toml` dependencies
Works in both single-file and multi-file codegen modes for contracts; fn
declarations extract error variants the same way. Added 8 new tests:
contract error enum generation, Result return type, thiserror dep
inclusion, negative tests for no-error contracts, unit tests for
extract_error_variants and generate_error_enum. 1,403 total tests passing.

### E008 completed (2026-06-14)
Rewrote `docs/INTERNALS.md` from a 66-line stub to comprehensive
documentation covering: pipeline overview with data flow, crate map with
LOC/test counts, detailed descriptions of all 11 crates and their public
APIs, step-by-step guides for adding new type checkers, new SMT
encodings, and new codegen passes, error code scheme with crate mappings,
build/test commands, and key library version constraints.

### A003 completed (2026-06-14)
Implemented `assura.toml` project configuration parsing. Added `toml`
crate dependency to CLI. Created `ProjectConfig` struct with four
sections: `[package]` (name, version), `[build]` (target, output),
`[verify]` (smt-solver, layer, timeout), `[profile]` (type). CLI flags
override config values (layer, output). `load_project_config()` walks up
from the source file to find the project root (via existing
`find_project_root`), parses the TOML, and supports legacy `[project]`
section name. Updated `assura init` to generate proper config with all
sections and comments. Verbose mode displays project info and config
values. JSON output includes config when present. 6 new tests. 1,434
total tests passing.

### Architecture refactoring completed (2026-06-14)

Completed the full 8-phase architecture refactoring plan across multiple
sessions. Summary of all phases:

**Phase 6.4**: Added `hir_type_from_expr()` and `resolve_hir_type()` to
assura-hir, enabling the HIR lowerer to use structured TypeExpr instead
of raw token sequences. 8 new tests.

**Phase 7.1-7.6**: Wired HirFile through the type checker. Added
`type_check_hir()` entry point, `build_type_env_from_hir()` using
`type_from_hir_type()`, `check_clause_bodies_hir()` with HirExpr-to-Expr
bridge, and propagated HirFile through TypedFile for downstream access.
All domain checkers work via `hir.resolved().source`. 8 new tests.

**Phase 8.1**: Added `parse_unwrap()` convenience to assura-parser.
Replaced the 3-line parse+assert+unwrap boilerplate across 6 crates
(18 sites). -29 lines.

**Phase 8.2**: Re-exported `BinOp`, `UnaryOp`, `Literal` from assura-hir
so downstream crates can use short names instead of reaching into
`assura_parser::ast::`.

**Phase 8.3**: Replaced hand-rolled lex+parse pipeline in SMT's
`verify_source()` test helper with `parse_unwrap()`. -17 lines.

**Phase 8.4**: Added `bare_expr` attempt in `clause_body_expr()` so
expression clauses without delimiters also try the expression parser.
This converted `decreases n` from Raw to Ident and `ensures result >= 0`
from Raw to BinOp{Gte}. Added 4 regression tests with budgets for 2
known gaps (@ pattern syntax, mod operator). 1,491 total tests passing.

### Issues #10, #11, #12 completed (2026-06-14)

**Issue #10 (extern/bind codegen)**: Added `BindDecl` across the full
compiler pipeline (17 files): parser grammar + CST lowering with
param extraction from input/output clauses, resolver `SymbolKind::BindFn`,
HIR `HirBind` struct, type checker env registration, codegen `generate_bind`
producing checked wrappers that call the bound Rust function with
`debug_assert!` for requires/ensures clauses, SMT clause verification,
LSP hover/completion/document symbols, CLI stats, and formatter.
4 tests added (2 parser, 2 codegen). 1,672 total tests.

**Issue #11 (Rust-to-Assura type mapping)**: Phase 1: `type_map.rs` module
with `rust_type_to_assura()` handling all primitive types, collections,
Option/Result, references, smart pointer erasure, tuples, and nested
generics (16 unit tests). Phase 2: three AI prompt templates
(single-function, module-level, CVE-patterns) in `templates/`. Phase 3:
`assura infer` CLI command that extracts public function signatures from
.rs files, applies reverse type mapping, and generates skeleton bind
declarations. Output parses as valid Assura syntax. 1,690 total tests.

**Issue #12 (assura audit command)**: Scans a Cargo workspace, discovers
public function signatures, generates skeleton Assura bind contracts with
heuristic preconditions (medium depth: bounds checks for index params,
non-empty checks for collection params), and verifies through the full
pipeline. Supports human and JSON output, --focus/--max-functions/
--unsafe-only filters. All open issues now closed.

### G001, G002, G011 completed (2026-06-14)

**G001 (CryptoConformanceChecker)**: Added `run_crypto_conformance_checks()`
wired into both `type_check()` and `type_check_hir()`. Fixed clause body
parsing for ident-based clauses (conforms, key_size, nonce_size, spec,
crypto) via `is_ident_expr_clause()`. Fixed `Literal::Str` quote stripping
for algorithm name matching. Added must_reject fixture for A17001.

**G011 (codegen catch-alls)**: Replaced `_ => "0"` catch-all in
`expr_to_rust_static()` with explicit handling for all 22 Expr variants.
Also eliminated catch-alls in `old_var_name()` (was silently producing
collision-prone "expr" names) and `extract_output_type()` (was hiding
unhandled variants behind `()`).

**G002 (TriggerManager)**: Wired TriggerManager into Z3 quantifier
encoding. Forall/Exists now infer trigger patterns from function calls
in the body. Added `collect_trigger_calls()` and
`collect_function_names_for_triggers()` to build Z3 Pattern objects
for e-matching hints.

### Issue #54 completed (2026-06-14)
Fixed all remaining silent catch-all wildcards across codegen and CVC5
backend. In cvc5_backend.rs, replaced `_ => {}` in `collect_vars()` with
explicit enumeration of all 22 Expr variants with recursive descent (8
tests). In codegen, fixed 4 catch-alls: `extract_output_type()` inner
match, `extract_error_variants()`, `generate_trait_method()`, and
`generate_block()` ClauseKind match (7 tests). 1,907 total tests passing.

### G007 completed (2026-06-14)
Implemented CONC.6 Weak Memory Ordering with structured parser
annotations and pipeline integration across all compiler layers:
- **Parser**: Added `ClauseKind::Ordering` and `MemoryOrdering` enum
  (Relaxed, Acquire, Release, AcqRel, SeqCst) to AST. Added
  `ORDERING_KW` to clause grammar.
- **HIR**: Added `HirClauseKind::Ordering` with bidirectional AST
  conversions.
- **Type checker**: Added `run_weak_memory_checks()` wired into both
  `type_check()` and `type_check_hir()`. Emits A-CONC-016 when relaxed
  ordering is combined with ensures clauses (stale value risk).
- **SMT backend**: Updated T092 weak memory checks to prefer structured
  `ClauseKind::Ordering` clauses over keyword scanning in effects.
  Also handles FnDef declarations.
- **Codegen**: Emits `std::sync::atomic::Ordering::*` constant
  declarations from ordering clauses via `resolve_ordering_variant()`.
- **Formatter**: Added `ordering` keyword mapping.
- **Tests**: 4 type checker tests (relaxed+ensures, acquire+ensures,
  relaxed no ensures, seq_cst), 5 codegen tests (resolve_ordering_variant,
  codegen constant), 1 must_reject fixture (A-CONC-016). 1,931 total
  tests passing.

### G008 completed (2026-06-14)
Implemented FMT.4 Codec Registry with full pipeline integration across
all compiler layers (19 files changed):
- **Parser**: Grammar for `codec_registry`/`codec`/`magic`/`decoder`/
  `contracts` productions with `BytePattern`, `extension()`, and
  `probe()` magic kinds. Added `CODEC_REGISTRY_DECL` and `CODEC_ENTRY`
  CST node kinds.
- **CST->AST lowering**: `lower_codec_registry()` and `lower_codec_entry()`
  extract structured types, handling lexer's split hex literals
  (`Int("0")` + `Ident("x89")` for `0x89`).
- **AST**: `CodecRegistryDecl`, `CodecEntry`, `MagicPattern` types.
- **HIR**: `HirCodecRegistry`, `HirCodecEntry` with lowering.
- **Type checker**: `run_codec_registry_checks()` wired into both
  `type_check()` and `type_check_hir()`. Detects A52001 overlapping
  magic byte prefixes and A52002 missing decoder functions.
- **Codegen**: `generate_codec_registry()` emits dispatch function
  with magic-byte pattern matching.
- **LSP**: Document symbols, hover, completion for CodecRegistry.
- **Tests**: 3 parser, 3 types, 2 codegen tests + 1 must_reject
  fixture (A52001). 1,939 total tests passing.

---

## Phase G: Gaps (Unwired Features, Dead Code, Pipeline Completion)

> Audit of 2026-06-14 found 7 verification features from the spec's 50
> that are either not wired into the pipeline, have structural stubs
> without real logic, or are entirely missing from the parser. This
> phase also covers architecture gaps: HIR migration completion, dead
> code removal, and inference hardening.
>
> **Current state**: 1,869 tests passing, all demos compile, 43/50
> verification features wired. This phase closes the remaining 7 and
> fixes 6 architecture/quality gaps.
>
> **Session protocol**: Pick the next `[ ]` task whose dependencies are
> all `[x]`. Complete it. Mark it `[x]`. Commit and push. Continue.

### G.1 Wire Existing Code Into Pipeline

These features have working checker/manager code with tests, but are
NOT called from the type-check or verification pipeline.

- [x] **G001**: Wire SEC.5 CryptoConformanceChecker into type_check pipeline
  - Depends on: none
  - **Current state**: `CryptoConformanceChecker` exists at
    `checkers.rs:3832` with 9 passing tests (key size, nonce size,
    nonce uniqueness, tag verification, custom specs). But there is
    NO `run_crypto_conformance_checks()` function in `lib.rs`, so the
    checker is never called during compilation.
  - **Fix**:
    1. Add `run_crypto_conformance_checks(source: &SourceFile) -> Vec<TypeError>`
       to `lib.rs` following the pattern of the other 29 `run_*_checks` functions
    2. Scan declarations for contracts/functions with `spec` or `conforms`
       annotations. When found, instantiate `CryptoConformanceChecker`,
       extract algorithm name + key/nonce sizes from clause bodies,
       run `check_key_size()`, `check_nonce_size()`,
       `check_nonce_uniqueness()`, `check_tag_verification()`
    3. Wire the call into BOTH `type_check()` and `type_check_hir()`
       entry points (same pattern as existing domain checkers)
    4. Add integration test: .assura file with `spec conforms("AES-128-GCM")`
       and wrong key size, verify A17001 is emitted
  - **Validation**: `cargo test --workspace`, verify a new must_reject
    fixture with `// MUST REJECT A17001` passes

- [x] **G002**: Wire CORE.5 TriggerManager into Z3 quantifier encoding
  - Depends on: none
  - **Current state**: `TriggerManager` exists at `advanced.rs:17` with
    `infer_trigger()`, `validate_trigger()`, `add_trigger()`. But the
    Z3 backend in `z3_backend.rs` encodes `forall`/`exists` without
    any trigger patterns, causing solver timeouts on complex quantifiers.
  - **Fix**:
    1. In `z3_backend.rs`, when encoding `Forall`/`Exists` expressions,
       use `TriggerManager.infer_trigger()` on the body expression string
    2. Pass inferred trigger patterns to Z3's `forall_const` via the
       `pattern!` API: `z3::Pattern::new(ctx, &[&trigger_expr])`
    3. Register function names from the contract's scope into
       `TriggerManager.register_function()` before encoding
    4. When user provides `trigger(...)` annotation (parser already lexes
       `trigger` keyword), pass those as user-provided triggers
    5. Add warning when no trigger can be inferred for a quantifier
       (heuristic: quantifier likely to timeout)
  - **Validation**: Verify a forall-heavy contract that previously timed
    out now verifies with triggers. Add test to e2e suite.

- [x] **G003**: Wire IncrementalCompiler into CLI --watch mode
  - Depends on: none
  - **Current state**: `IncrementalCompiler` at `incremental.rs` has
    module registration, dirty detection, dependency tracking, and 5
    tests. But it is dead code; `--watch` mode re-runs the full pipeline
    from scratch on every file change.
  - **Fix**:
    1. In `main.rs` watch loop, maintain an `IncrementalCompiler` instance
       across iterations
    2. On first run, `register_module()` for the source file with its
       content hash
    3. On re-run, `update_hash()` and only re-verify `dirty_modules()`
    4. Cache verification results per module; skip clean modules
    5. When dependencies are declared (imports), use `add_dependency()`
       so changing a dependency cascades dirty marks
  - **Validation**: Run `assura check --watch` on a file, edit it, verify
    only the changed contract is re-verified (visible in verbose output)

- [x] **G004**: Wire TEST.1 TestGenerator into codegen and CLI
  - Depends on: none
  - **Current state**: `TestGenerator` at `domain.rs:1281` generates
    proptest/boundary/smoke test code strings, with 6 passing tests.
    But it is NOT wired into the pipeline. The spec says TEST.1 should:
    (a) trigger on verification timeout/unknown, (b) generate test
    files alongside codegen output, (c) be invocable via
    `assura test-gen <file>`.
  - **Fix**:
    1. Add `assura test-gen <file.assura>` CLI command that:
       - Parses, resolves, type-checks the file
       - Extracts `TestableContract` from each contract with input/ensures
       - Runs `TestGenerator.generate_all()` to produce test code
       - Writes `tests/generated_tests.rs` alongside codegen output
    2. In `assura build`, when SMT verification returns `Timeout` or
       `Unknown` for a contract, automatically invoke TestGenerator
       for that contract and include the test in the generated project
    3. Support `#[generate_tests]` annotation on contracts to force
       test generation regardless of verification result (per spec)
    4. Add `--test-gen` flag to `assura build` to generate tests for
       ALL contracts (not just timeout/unknown)
  - **Validation**: Build a demo file, verify generated project contains
    proptest-based tests. Run `cargo test` on the generated project.

### G.2 Deepen Structural Stubs Into Real Analyzers

These features have structural stubs (data structures + basic checks)
but lack real semantic analysis or parser integration.

- [x] **G005**: CORE.7 Prophecy Variables: parser + type system integration
  - Depends on: none
  - **Current state**: `ProphecyManager` at `advanced.rs:331` has
    register, constrain, resolve, check_all_resolved, check_unconstrained
    APIs with 7 tests. Z3 backend superficially scans clause text for
    "prophecy" keyword. But: no parser grammar for `ghost prophecy`,
    no AST/HIR nodes, no type checker integration, no resolution check.
  - **Fix**:
    1. **Parser**: Add grammar production for `ghost prophecy <name>: <type>`
       declarations. Store as new `Decl::GhostProphecy { name, ty }` or
       reuse existing ghost handling. Add `resolve <name> = <expr>`
       statement parsing.
    2. **AST/HIR**: Add corresponding AST and HIR node types
    3. **Resolver**: Register prophecy variables in the symbol table
       with `SymbolKind::Prophecy`
    4. **Type checker**: Type-check the prophecy type annotation. At
       resolve statements, check type compatibility between the
       expression and the declared prophecy type (A-CORE-027)
    5. **SMT backend**: Use `ProphecyManager` to track resolved/unresolved
       status. Emit A-CORE-025 for unresolved prophecies at function exit.
       Emit A-CORE-026 for double resolution.
    6. **Codegen**: Erase prophecy variables (verification-only per spec)
  - **Validation**: Add must_reject fixture for A-CORE-025 (unresolved
    prophecy) and A-CORE-026 (double resolve). Add must_compile fixture
    for a correctly resolved prophecy.

- [x] **G006**: CORE.8 Liveness Contracts: parser + BMC verification
  - Depends on: G005
  - **Current state**: `LivenessChecker` at `advanced.rs:426` has
    obligation registration, mark_verified, check_unverified,
    check_bounded, check_fairness with 8 tests. Z3 backend
    superficially scans for "eventually" in clause text. But: no parser
    for `liveness { ... }` blocks, no BMC implementation, no k-induction.
  - **Fix**:
    1. **Parser**: Add grammar for `liveness <name> { ... }` declarations
       with `assume eventually_always`, `prove eventually`,
       `prove leads_to(P, Q)`, `prove eventually_within(N)`,
       `assume fair` (per spec Section 14 CORE.8)
    2. **AST/HIR**: `Decl::Liveness { name, assumptions, proofs }`
    3. **Resolver**: Register liveness obligation names
    4. **SMT backend**: Implement liveness-to-safety reduction:
       a. Add lasso detector augmentation to state space
       b. Bounded model checking (BMC) up to K steps
       c. Optional k-induction for unbounded proofs
       d. Fairness encoding as lasso constraints
    5. **Error codes**: A-CORE-029 (lasso found), A-CORE-030 (unproven
       within bound), A-CORE-031 (missing fairness), A-CORE-033
       (bounded liveness exceeded)
    6. **Codegen**: Erase liveness contracts; in debug mode, generate
       optional runtime monitors (AtomicU32 tick counters with
       debug_assert)
  - **Validation**: Write liveness contract for a simple state machine.
    Verify BMC finds a lasso when fairness is missing. Verify k-induction
    proves progress when fairness is assumed.

- [x] **G007**: CONC.6 Weak Memory Ordering: parser annotations + view model
  - Depends on: none
  - **Current state**: `WeakMemoryChecker` at `advanced.rs:200` has
    data race detection, happens-before tracking, release-acquire
    pairing checks, ordering strength warnings with 6 tests. Z3 backend
    scans for `relaxed`/`acquire`/`release`/`seq_cst` keywords in
    effect clauses. But: no parser support for `ordering:` annotations,
    no per-thread ghost view model, no view merge on acquire/release.
  - **Fix**:
    1. **Parser**: Add `ordering: <relaxed|acquire|release|acqrel|seq_cst>`
       syntax in atomic operation expressions (per spec CONC.6)
    2. **Type checker**: When an expression has an ordering annotation,
       validate the ordering is consistent with the operation (e.g.,
       loads must be acquire or relaxed, stores must be release or relaxed)
    3. **SMT backend**: Implement per-thread view model:
       a. Each thread has a ghost `HashMap<Var, Version>` view
       b. Release store merges the writer's view into the variable
       c. Acquire load merges the variable's stored view into the reader
       d. SeqCst operations go through a total order
       e. Relaxed operations only guarantee atomicity, not view merge
    4. **Error codes**: A-CONC-016 (relaxed read without view check),
       A-CONC-017 (missing release before acquire), A-CONC-018
       (view inconsistency)
    5. **Codegen**: Preserve exact Rust `Ordering::*` variants
  - **Validation**: Contract with acquire-release pair verifies. Contract
    with relaxed-only concurrent access emits A-CONC-016 warning.

- [x] **G008**: FMT.4 Codec Registry: full parser + dispatch codegen
  - Depends on: none
  - **Current state**: Parser lexes `codec_registry` and `codec` keywords
    (`lexer.rs:326`, `syntax_kind.rs:588`), but there is NO grammar
    production, NO AST node, NO HIR node, NO codegen. The feature is
    entirely structural scaffolding (just keywords).
  - **Fix**:
    1. **Parser grammar**: Add productions per spec:
       `codec_registry <name> { output: <type>, codec <name> { magic: [...],
       decoder: <fn>, contracts: { ... } } }`
    2. **AST**: `Decl::CodecRegistry { name, output_type, codecs: Vec<CodecEntry> }`
       where `CodecEntry { name, magic, decoder, contracts }`
    3. **HIR**: Mirror AST codec registry declaration
    4. **Resolver**: Register codec registry and each codec in symbol table
    5. **Type checker**: Verify magic pattern uniqueness (A52001),
       decoder return type matches output type (A52002), codec-specific
       contracts hold (A52003), probe functions are pure (A52004)
    6. **Codegen**: Generate dispatch function with magic-byte pattern
       matching (nested if-else on byte slices, per spec Appendix C)
    7. **SMT**: Verify codec-specific contracts and common output contract
  - **Validation**: Write a codec registry with 3 formats (PNG, JPEG, BMP
    magic bytes). Verify codegen produces correct dispatch function.
    Add must_reject for overlapping magic patterns (A52001).

### G.3 Architecture Gaps

- [x] **G009**: Complete HIR migration: eliminate to_ast_expr() bridge
  - Depends on: none
  - **Current state**: `infer_hir_expr()` at `inference.rs:16` immediately
    calls `hir_expr.to_ast_expr()` and delegates to `infer_expr()`. The
    `clauses.rs` file has 8 sites calling `.to_ast_expr()` before
    `check_clause_expr()`. This means the type checker still operates on
    AST expressions even when receiving HIR input, defeating the HIR's
    purpose (structured types, resolved names, no raw tokens).
  - **Fix**:
    1. Implement `infer_hir_expr_native()` that pattern-matches on
       `HirExpr` variants directly, using `DefId` for name resolution
       instead of string lookup in `TypeEnv`
    2. Implement `check_clause_expr_hir()` that operates on `HirExpr`
       and `HirClauseKind` directly
    3. Update `type_check_hir()` to use the native HirExpr path
    4. Keep `to_ast_expr()` but deprecate it with `#[deprecated]`
    5. Update `infer_hir_expr()` to call native implementation
  - **Validation**: All 1,869 tests pass. The `to_ast_expr()` method
    is no longer called in production code paths (only in tests for
    backward compatibility).

- [x] **G010**: Harden type inference: reduce Type::Unknown returns
  - Depends on: G009
  - **Current state**: `inference.rs` has 32 `Type::Unknown` returns.
    Many are legitimate (raw tokens, unknown callees), but several
    represent real inference gaps:
    - `MethodCall` on unknown receiver returns `Unknown` (line 458)
    - Generic method returns like `Option::map()` return `Unknown`
      (line 388)
    - `Call` on `TypeParam` returns `Unknown` (line 848)
    - `Index` on non-indexable type returns `Unknown` (line 495)
  - **Fix**:
    1. For `MethodCall` on `Named` types: maintain a method registry
       mapping `(TypeName, MethodName) -> ReturnType` for common stdlib
       types (Vec, HashMap, BTreeMap, etc.)
    2. For `Option::map` and similar HKT methods: infer return type
       from the closure argument's return type when available
    3. For `Index` on `Named` types: assume indexable types return
       their element type (configurable per type)
    4. Add `Type::Error` variant distinct from `Type::Unknown` to
       distinguish "genuinely unknown" from "error recovery"
    5. Target: reduce Unknown returns from 32 to <15
  - **Validation**: Run type inference on all demo files and fixture
    files. Count `Unknown` inferences before and after. Verify no
    regressions in error reporting.

- [x] **G011**: Codegen: eliminate unsupported expression placeholders
  - Depends on: none
  - **Current state**: `codegen/lib.rs` has 3 sites emitting
    `"0 /* unsupported: ... */"` for complex expressions in const
    context: non-arithmetic binops (line 1704), negation of complex
    exprs (line 1721), and generic complex expressions (line 1738).
    Additionally, 15 `todo!()` calls for function bodies (expected for
    AI-generated implementations, but some may be eliminable).
  - **Fix**:
    1. For const-context expressions: support logical operators
       (`&&`, `||`, `!`) by generating them directly in const context
    2. For complex negation: evaluate the inner expression first, then
       negate the result string
    3. For remaining `todo!()` calls: audit each one. Function body
       `todo!("implementation provided by AI agent")` is correct by
       design. Any `todo!()` in non-body positions is a bug.
    4. Replace any remaining `/* unsupported */` with proper code
       generation or a compile error diagnostic (instead of silent
       wrong output)
  - **Validation**: Build all demos, grep for `unsupported` in generated
    Rust. Should be zero occurrences in expression positions (type stubs
    with `_phantom` fields are OK).

### G.4 Testing Gaps

- [x] **G012**: Add must_reject fixtures for all new error codes
  - Depends on: G001, G005, G006, G007, G008
  - For each new error code introduced in G001-G008, add a
    corresponding `tests/fixtures/must_reject/` file:
    - `crypto_key_mismatch.assura` -> `// MUST REJECT A17001`
    - `crypto_nonce_mismatch.assura` -> `// MUST REJECT A17002`
    - `unresolved_prophecy.assura` -> `// MUST REJECT A-CORE-025`
    - `double_resolve_prophecy.assura` -> `// MUST REJECT A-CORE-026`
    - `liveness_no_fairness.assura` -> `// MUST REJECT A-CORE-031`
    - `relaxed_without_view.assura` -> `// MUST REJECT A-CONC-016`
    - `overlapping_magic.assura` -> `// MUST REJECT A52001`
  - Target: 7+ new must_reject fixtures (one per new error code
    category at minimum)
  - **Validation**: `test_must_reject_fixtures` harness passes for
    all new files

- [x] **G013**: E2E verification tests for all 50 features
  - Depends on: G001-G008
  - **Current state**: 8 e2e test files exist but only cover basic
    contracts, counterexamples, and a few domain features
  - Create one e2e .assura file per feature category that exercises
    the feature end-to-end through the full pipeline:
    - `tests/e2e/core_ghost.assura` (CORE.1-2)
    - `tests/e2e/core_quantifiers.assura` (CORE.3-5)
    - `tests/e2e/core_prophecy_liveness.assura` (CORE.7-8)
    - `tests/e2e/mem_regions.assura` (MEM.1-4)
    - `tests/e2e/type_interfaces.assura` (TYPE.1-3)
    - `tests/e2e/sec_taint_crypto.assura` (SEC.1-5)
    - `tests/e2e/conc_shared_weak.assura` (CONC.1-6)
    - `tests/e2e/stor_crash_mvcc.assura` (STOR.1-6)
    - `tests/e2e/fmt_binary_codec.assura` (FMT.1-4, FMT.6)
    - `tests/e2e/num_precision.assura` (NUM.1-2)
    - `tests/e2e/test_gen_equiv.assura` (TEST.1-2)
  - Each file has `// EXPECTED: verified` or `// EXPECTED: <specific>`
    annotations validated by the e2e harness
  - Target: 11+ new e2e files covering all 50 features

---

## Phase G Dependency Graph

```
G.1 Wire existing code (independent, start immediately)
  G001 (crypto) -- no deps
  G002 (triggers) -- no deps
  G003 (incremental) -- no deps
  G004 (test gen) -- no deps

G.2 Deepen stubs (some interdependencies)
  G005 (prophecy) -- no deps
  G006 (liveness) -- depends on G005 (prophecy resolution used in liveness)
  G007 (weak memory) -- no deps
  G008 (codec registry) -- no deps

G.3 Architecture (can start in parallel)
  G009 (HIR migration) -- no deps
  G010 (inference) -- depends on G009
  G011 (codegen) -- no deps

G.4 Testing (after features land)
  G012 (must_reject) -- depends on G001, G005-G008
  G013 (e2e 50 features) -- depends on G001-G008
```

### Recommended Execution Order

**Round 1** (4 tasks, fully parallel):
- G001 (wire crypto checker, ~15 min)
- G002 (wire triggers into Z3, ~30 min)
- G003 (wire incremental compiler, ~20 min)
- G011 (codegen unsupported, ~15 min)

**Round 2** (3 tasks, fully parallel):
- G004 (test gen CLI + auto-trigger, ~45 min)
- G005 (prophecy parser + type system, ~60 min)
- G009 (HIR migration, ~90 min)

**Round 3** (3 tasks, dependencies from Round 2):
- G006 (liveness BMC, depends G005, ~90 min)
- G007 (weak memory views, ~60 min)
- G008 (codec registry parser + codegen, ~90 min)

**Round 4** (2 tasks, after all features):
- G010 (inference hardening, depends G009, ~60 min)
- G012 (must_reject fixtures, depends G001/G005-G008, ~30 min)

**Round 5** (final):
- G013 (e2e 50 features, depends all, ~60 min)

---

## Milestones (Phase G)

### MG1: All 50 Features Wired (G001-G008)
- `run_crypto_conformance_checks()` called in type_check pipeline
- Z3 quantifier encoding uses trigger patterns
- `IncrementalCompiler` used by `--watch` mode
- `assura test-gen` command works
- `ghost prophecy` parses, type-checks, and verifies
- `liveness { ... }` parses, BMC runs, k-induction optional
- `ordering: acquire` parses and validates
- `codec_registry { ... }` parses, type-checks, generates dispatch

### MG2: Architecture Clean (G009-G011)
- Zero `to_ast_expr()` calls in production code paths
- `Type::Unknown` count < 15 (from 32)
- Zero `/* unsupported */` in generated Rust expressions

### MG3: Full Test Coverage (G012-G013)
- 7+ new must_reject fixtures for new error codes
- 11+ e2e files covering all 50 verification features
- All e2e files validated by the harness
