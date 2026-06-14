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

- [ ] **R001**: Fix codegen to produce compilable Rust for all demo files
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

- [ ] **R002**: Fix codegen module structure for multi-contract files
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

- [ ] **R003**: Split assura-types into domain modules (file-level)
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

- [ ] **R004**: Deduplicate raw-token param/type extraction (issue #5)
  - Depends on: none
  - Three crates independently parse `name: Type` pairs from raw tokens:
    - `assura-types/src/lib.rs`: `register_input_clause_params()`
    - `assura-codegen/src/lib.rs`: `extract_input_params()`
    - `assura-resolve/src/lib.rs`: `extract_input_param_names()`
  - Extract to `assura-parser::ast::parse_clause_params()` with a
    shared `ParsedParam { name: String, ty: Vec<Token> }` return type
  - Update all three call sites
  - Close issue #5 with `Closes #5` in commit message

- [ ] **R005**: Preserve refinement predicates during type parsing (issue #6)
  - Depends on: none
  - When `parse_type_tokens` encounters `{ x: Int | x > 0 }`, the
    refinement predicate is lost. Downstream sees `Type::Int` instead
    of `Type::Refined { base: Int, predicate: "x > 0" }`
  - Fix `parse_type_tokens()` in assura-types to detect the `{`, parse
    the base type, detect `|`, and capture the predicate tokens
  - This is critical for SMT: without the predicate, Z3 cannot verify
    refinement subtyping
  - Close issue #6

- [ ] **R006**: Improve parser error messages with expected tokens (issue #7)
  - Depends on: none
  - chumsky 0.9's `Simple` error has an `expected()` method returning
    the set of expected tokens. The CLI currently ignores this.
  - Update error rendering in `assura-cli/src/main.rs` to show:
    `error[A01001]: unexpected 'foo', expected one of: '{', 'requires'`
  - Close issue #7

- [ ] **R007**: Add integration tests for CLI build --output flag (issue #8)
  - Depends on: R001
  - Tests:
    - Custom output directory is created and receives files
    - Default `generated/` works when `--output` omitted
    - Error on invalid output path
  - Close issue #8

- [ ] **R008**: Convert wildcard catch-alls to explicit match arms (issue #9)
  - Depends on: none
  - Locations:
    - `assura-codegen/src/lib.rs`, `generate_service`: two `_ => {}` arms
    - `assura-resolve/src/lib.rs`: similar catch-alls
  - Replace with explicit variant lists so rustc warns on new variants
  - Close issue #9

### R.4 Fix Pipeline Integrity

- [ ] **R009**: Show per-clause verification results in CLI output
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

- [ ] **R010**: Make Z3 feature enabled by default in workspace builds
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

- [ ] **R011**: Add standalone tests to assura-smt
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

- [ ] **R012**: Fix `assura build` to verify generated code compiles
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

- [ ] **R013**: Eliminate raw token fallback in clause bodies
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

- [ ] **S001**: Implement real termination checking (not just measure parsing)
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

- [ ] **S002**: Implement real effect inference (not just declared-vs-used)
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

- [ ] **S003**: Implement real information flow tracking
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

- [ ] **S004**: Implement real context splitting for linear types
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

- [ ] **S005**: Wire per-clause SMT verification into the CLI pipeline
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

- [ ] **S006**: Implement counterexample display in CLI
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

- [ ] **S007**: Implement Layer 2 real quantifier verification
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

- [ ] **S008**: Generate compilable Rust for service declarations
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

- [ ] **S009**: Generate proptest/quickcheck tests from contracts
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

- [ ] **A001**: Design and implement a HIR between AST and type checker
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

- [ ] **A002**: Implement filesystem-based module resolution
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

- [ ] **A003**: Implement `assura.toml` project configuration
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

- [ ] **A004**: Implement structured error types across all crates
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

- [ ] **I001**: Implement CVC5 fallback solver (GitHub issue #1)
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

- [ ] **I002**: Add performance profiling and benchmarks (GitHub issue #2)
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

- [ ] **I003**: Implement WASM compilation target (GitHub issue #3)
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

- [ ] **I004**: Set up cargo-fuzz for the parser
  - Depends on: none
  - The parser should never panic on any input. Fuzzing finds inputs
    that cause panics or infinite loops.
  - Create `fuzz/` directory with:
    - `fuzz_targets/parse.rs`: feed random bytes to `assura_parser::parse()`
    - `fuzz_targets/lex.rs`: feed random bytes to the lexer
  - Run for at least 10 minutes. Fix any panics found.
  - Add CI job: run fuzzing for 60 seconds on each PR

- [ ] **I005**: Set up cargo-fuzz for the type checker
  - Depends on: I004
  - Fuzz the pipeline: parse -> resolve -> type_check
  - The type checker should never panic, only return errors
  - Use structured fuzzing: generate random ASTs that are syntactically
    valid but may be type-incorrect

---

## Phase T: Testing (Comprehensive Test Coverage)

### T.1 Missing Test Coverage

- [ ] **T201**: Add tests to assura-server (currently 0)
  - Depends on: none
  - The gRPC server has zero tests despite 496 lines of code
  - Add tests for:
    - `Check` RPC: valid source returns success, invalid returns errors
    - `Build` RPC: valid source returns generated files
    - `Explain` RPC: valid error code returns description
    - `Health` RPC: returns "serving"
    - `CheckStream` RPC: streams events for a valid source
  - Use `tonic`'s test utilities (in-process channel, no network)

- [ ] **T202**: Expand LSP tests (currently 9)
  - Depends on: none
  - The LSP needs tests for:
    - `textDocument/completion`: keyword completions, type completions
    - `textDocument/hover`: hover on type names, function names
    - `textDocument/definition`: go to definition for symbols
    - `textDocument/documentSymbol`: contract/service/function symbols
    - `textDocument/diagnostic`: parse errors, type errors, resolution errors
    - Incremental edits: edit a document, verify diagnostics update
  - Target: 30+ tests for LSP

- [ ] **T203**: Add negative test suite (MUST REJECT files)
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

- [ ] **T204**: Add positive test suite (MUST COMPILE files)
  - Depends on: R001
  - Create `tests/fixtures/must_compile/` with valid .assura files:
    - `simple_contract.assura` with `// MUST COMPILE`
    - `generic_contract.assura` with `// MUST COMPILE`
    - `service_with_state.assura` with `// MUST COMPILE`
    - `effects_declared.assura` with `// MUST COMPILE`
  - Test harness runs full pipeline AND verifies generated Rust compiles
  - Target: 15+ positive test files

- [ ] **T205**: Add end-to-end round-trip tests
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

- [ ] **T206**: Add tree-sitter grammar tests
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

- [ ] **T207**: VS Code extension test infrastructure
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

- [ ] **E001**: Set up cargo-dist for binary releases
  - Depends on: none
  - Configure `cargo-dist` in workspace `Cargo.toml`
  - Generate release binaries for: Linux x86_64, macOS x86_64, macOS
    aarch64, Windows x86_64
  - GitHub Action: on tag push, build and upload release artifacts
  - This gives users `assura` binaries without building from source

- [ ] **E002**: Set up Homebrew tap
  - Depends on: E001
  - Create `assura-lang/homebrew-tap` repository
  - Generate Homebrew formula from cargo-dist output
  - Users can: `brew install assura-lang/tap/assura`

- [ ] **E003**: Publish to crates.io
  - Depends on: E001
  - The `assura` name is already claimed on crates.io
  - Publish all workspace crates in dependency order:
    `assura-parser` -> `assura-resolve` -> `assura-types` ->
    `assura-smt` -> `assura-codegen` -> `assura-cli`
  - Set up CI: publish on tag push (after cargo-dist)

### E.2 CI Hardening

- [ ] **E004**: Add CI jobs for editors
  - Depends on: T206
  - Add GitHub Action jobs:
    - `tree-sitter test` for the grammar
    - `npm run compile` for the VS Code extension
    - `npm test` for VS Code extension tests (if T207 is done)

- [ ] **E005**: Add CI job for generated code validation
  - Depends on: R001
  - CI should run `assura build` on all demo files and then
    `cargo check` on each generated project
  - This prevents regressions where codegen produces invalid Rust
  - Install Z3 and Rust stable in the CI job

- [ ] **E006**: Add security scanning (CodeQL, cargo-audit)
  - Depends on: none
  - Add `.github/workflows/security.yml`:
    - `cargo audit` for known vulnerable dependencies
    - CodeQL analysis for Rust
    - Dependabot configuration for automated dependency updates

### E.3 Documentation

- [ ] **E007**: Write getting-started tutorial
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

- [ ] **E008**: Write compiler internals documentation
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

- [ ] **P001**: Implement `--verbose` and `--quiet` modes
  - Depends on: R009
  - `--verbose`: show timing information, Z3 solver statistics,
    intermediate results
  - `--quiet`: suppress all output except errors
  - Default: current behavior (summary line per file)

- [ ] **P002**: Implement `--watch` mode
  - Depends on: none
  - `assura check --watch .` watches for file changes and re-runs
    the pipeline incrementally
  - Use `notify` crate for filesystem watching
  - Re-parse only changed files, re-resolve affected modules,
    re-typecheck affected contracts

- [ ] **P003**: Implement `assura fmt` command
  - Depends on: none
  - Format `.assura` source files with consistent style
  - Use the parser's AST to re-emit formatted source
  - Enforce: consistent indentation, brace placement, clause ordering
  - This is important for code review and collaboration

### P.2 Advanced Codegen

- [ ] **P004**: Generate Rust with proper error handling
  - Depends on: R001
  - Currently all function bodies are `todo!("implementation provided by AI agent")`
  - When an implementation is provided (via IR or inline), generate
    proper error handling:
    - `Result<T, E>` return types
    - `?` operator for propagation
    - Custom error types from contract error clauses

- [ ] **P005**: Implement the Implementation IR parser (Section 4)
  - Depends on: A001
  - The IR is what AI agents generate (not the contract language)
  - Parse IR text format into HIR
  - Validate IR against the contract it implements
  - Generate Rust from validated IR
  - This completes the AI-in-the-loop workflow:
    contract -> AI generates IR -> compiler verifies -> Rust output

### P.3 Performance

- [ ] **P006**: Implement verification caching
  - Depends on: S005
  - Hash each contract (AST + implementation) as a cache key
  - Store verification results in `.assura-cache/`
  - On re-run, skip re-verification for unchanged contracts
  - Invalidate cache when the contract or its dependencies change
  - This is critical for large projects: SMT queries are expensive

- [ ] **P007**: Implement parallel SMT queries
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
