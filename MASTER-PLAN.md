# Assura Master Plan

> Actionable task list for building the Assura compiler from current state
> to v1.0. Each task has a checkbox, estimated effort, dependencies, and
> enough detail for an AI agent to pick up and execute without prior context.
>
> **How to use**: Read top to bottom. Pick the next `[ ]` task whose
> `depends-on` tasks are all `[x]`. Complete it. Mark it `[x]`. Commit
> MASTER-PLAN.md with the change. Continue.
>
> **Parallelization**: Tasks with the same `depends-on` can run in
> parallel. The dependency graph is documented at the end of this file.

---

## Phase 0: Foundation (Target: compiler parses, type-checks, generates Rust)

### 0.1 Parser Hardening (1 week)

> The parser exists and handles the demo files. These tasks make it
> production-quality with tests and full grammar coverage.

- [x] **T001**: Lexer with logos (50+ keywords, operators, literals)
  - Crate: `assura-parser`, file: `src/lexer.rs`
  - Done: 233 lines, all demos parse

- [x] **T002**: Parser with chumsky 0.9 (contracts, services, types, enums, externs, fns, generics)
  - Crate: `assura-parser`, file: `src/parser.rs`
  - Done: ~620 lines, handles 12 edge cases

- [x] **T003**: AST types (SourceFile, ContractDecl, ServiceDecl, TypeDef, etc.)
  - Crate: `assura-parser`, file: `src/ast.rs`
  - Done: 173 lines

- [x] **T004**: CLI with --ast, --tokens, error reporting via ariadne
  - Crate: `assura-cli`, file: `src/main.rs`
  - Done: ~300 lines

- [ ] **T005**: Add snapshot tests for parser (insta crate)
  - Depends on: T001-T004
  - Crate: `assura-parser`
  - Add `insta` to dev-dependencies
  - For each file in `demos/` and `tests/fixtures/`:
    - Parse it, serialize AST to debug format, snapshot it
  - Create `tests/fixtures/` directory with targeted test cases:
    - `empty.assura`: empty file
    - `imports_only.assura`: just imports
    - `contract_minimal.assura`: simplest valid contract
    - `all_clause_kinds.assura`: every clause kind (requires, ensures, effects, etc.)
    - `nested_types.assura`: refinement types, generic types, bounded type params
    - `service_full.assura`: service with states, operations, queries
  - Run: `cargo test --workspace` must pass

- [ ] **T006**: Add error recovery test cases
  - Depends on: T005
  - Create `tests/fixtures/errors/` directory with invalid .assura files:
    - `missing_brace.assura`: unclosed block
    - `bad_token.assura`: invalid characters
    - `duplicate_clause.assura`: contract with same clause twice
  - Each file has a `// EXPECT ERROR` comment
  - Test that parser produces errors (not panics) with meaningful messages
  - Test that `parse_recovery()` returns partial AST + error list

- [ ] **T007**: Expand lexer to cover full spec keywords
  - Depends on: T001
  - Read Appendix A of `docs/SPECIFICATION.md` for all ~199 keywords
  - Add missing keyword tokens to `lexer.rs`
  - Current lexer has ~50 keywords; spec defines ~199
  - Group additions by category (verification, types, effects, etc.)
  - Update `keyword_or_ident()` in parser.rs for any new keywords
    that can appear in identifier position

- [ ] **T008**: Add expression parser
  - Depends on: T002
  - Currently, clause bodies and type bodies are collected as raw tokens
  - Implement a proper expression AST and parser for predicates:
    - Binary ops: `+`, `-`, `*`, `/`, `%`, `==`, `!=`, `<`, `>`, `<=`, `>=`, `&&`, `||`
    - Unary ops: `!`, `-`
    - Field access: `a.b.c`
    - Function calls: `f(x, y)`
    - Quantifiers: `forall x in S: P`, `exists x in S: P`
    - `old(expr)` for postconditions
    - `result` keyword in ensures clauses
    - Conditional: `if P then E1 else E2`
    - Set/list comprehensions
  - Add `Expr` enum to `ast.rs`
  - Replace `Vec<String>` token lists in `Clause.tokens` with `Expr`
  - Spec reference: Sections 1.4-1.7 (Predicate, Expr, Term, Atom)
  - All existing demo files must still parse after this change

### 0.2 Name Resolution (2 weeks)

> Build a symbol table. Resolve all names. Detect errors.

- [ ] **T009**: Create `assura-resolve` crate
  - Depends on: T008
  - New crate: `crates/assura-resolve/`
  - Cargo.toml: depends on `assura-parser`
  - Exports: `resolve(SourceFile) -> Result<ResolvedFile, Vec<ResolutionError>>`
  - Data structures:
    - `SymbolTable`: maps names to definitions (type, span, visibility)
    - `Scope`: nested scopes (module > service > operation > block)
    - `Symbol`: enum of TypeDef, ContractDef, ServiceDef, FnDef, EnumDef, etc.

- [ ] **T010**: Implement scope analysis
  - Depends on: T009
  - Walk the AST top-down, building scopes:
    - Module scope: all top-level declarations
    - Service scope: types, operations, queries, invariants
    - Contract scope: clauses, nested types/fns
    - Function scope: parameters, local bindings
  - Detect and report:
    - A02001: Undefined name
    - A02002: Ambiguous name (multiple imports)
    - A02003: Duplicate definition
    - A02004: Visibility violation (accessing non-pub field)
  - Spec reference: Section 8.1 (Module System)

- [ ] **T011**: Implement import resolution
  - Depends on: T010
  - Resolve `import a.b.c` to the corresponding module
  - Support: `import a.b.c`, `import a.b.c as alias`, `import a.b { X, Y }`
  - Module paths correspond to file paths (Section 8.1)
  - Detect A02005: circular imports (topological sort)
  - For now, multi-file resolution can use a simple in-memory file map
    (no need for actual filesystem resolution yet)

- [ ] **T012**: Resolve type references
  - Depends on: T010
  - Every type name in the AST must resolve to a TypeDef, EnumDef,
    or built-in type
  - Built-in types (hardcoded in symbol table):
    Int, Nat, Float, Bool, String, Bytes, Unit, Never,
    List<T>, Map<K,V>, Set<T>, Option<T>, Result<T,E>
  - Generic type parameter resolution: `T` in `Contract<T>` resolves
    to the type parameter, not a concrete type
  - Detect A02001 for unknown type names

### 0.3 Type Checker - Layer 0 (3 weeks)

> Check types without SMT. Structural checks only.

- [ ] **T013**: Create `assura-types` crate
  - Depends on: T012
  - New crate: `crates/assura-types/`
  - Depends on: `assura-parser`, `assura-resolve`
  - Core data structures:
    - `Type`: enum representing all Assura types (base, generic, refined, function, etc.)
    - `TypeEnv`: typing environment (maps names to types)
    - `TypeError`: structured error with code, span, message
  - Entry point: `type_check(ResolvedFile) -> Result<TypedFile, Vec<TypeError>>`

- [ ] **T014**: Implement base type checking
  - Depends on: T013
  - Type-check expressions against expected types:
    - Integer literals -> Int (or Nat if non-negative)
    - Float literals -> Float
    - String literals -> String
    - Boolean literals -> Bool
    - Variable references -> look up in TypeEnv
    - Binary operations: check operand types match, determine result type
    - Comparison operations: operands same type, result Bool
    - Logical operations: operands Bool, result Bool
  - Emit A03001 (type mismatch), A03002 (argument count mismatch)
  - Spec reference: Sections 2.1-2.2

- [ ] **T015**: Implement generic type instantiation
  - Depends on: T014
  - Check `List<Int>`, `Map<String, Int>`, `Option<Bool>`, etc.
  - Verify type argument count matches type parameter count
  - Substitute type parameters in the body
  - Emit A03003 (wrong number of type arguments)

- [ ] **T016**: Implement field access and function call type checking
  - Depends on: T014
  - Field access `x.field`: look up field in struct type, return field type
  - Function call `f(args)`: check argument types against parameter types,
    return the function's return type
  - Method-style calls on services: `service.operation(args)`
  - Emit A03004 (unknown field), A03005 (not callable)

- [ ] **T017**: Implement pattern exhaustiveness checking
  - Depends on: T014
  - For match expressions over enum types:
    - Build a pattern matrix (Maranget's algorithm)
    - Check that all variants are covered
    - Report A10001 (non-exhaustive match) with missing variants
  - Spec reference: Section 2.9

- [ ] **T018**: Implement contract clause type checking
  - Depends on: T014
  - `requires` and `ensures` clauses must be Bool-typed expressions
  - `input` clause: declare parameter names and types
  - `output` clause: declare return type
  - `effects` clause: validate effect names against known effects
  - `modifies` clause: validate that named variables exist
  - `old(expr)` in ensures: expr must be valid in the pre-state
  - `result` in ensures: type matches the output type

### 0.4 Rust Code Generation (2 weeks)

> Generate valid Rust source code from type-checked contracts.

- [ ] **T019**: Create `assura-codegen` crate
  - Depends on: T018
  - New crate: `crates/assura-codegen/`
  - Depends on: `assura-parser`, `assura-resolve`, `assura-types`
  - Add `prettyplease` dependency for Rust code formatting
  - Entry point: `codegen(TypedFile) -> GeneratedProject`
  - `GeneratedProject`: Cargo.toml content + Vec<(path, rust_source)>

- [ ] **T020**: Implement type mapping
  - Depends on: T019
  - Assura -> Rust type translations (Section 6.1 of spec):
    - `Int` -> `i64`
    - `Nat` -> `u64`
    - `Float` -> `f64`
    - `Bool` -> `bool`
    - `String` -> `String`
    - `Bytes` -> `Vec<u8>`
    - `Unit` -> `()`
    - `Never` -> `!`
    - `List<T>` -> `Vec<T>`
    - `Map<K,V>` -> `BTreeMap<K,V>`
    - `Set<T>` -> `BTreeSet<T>`
    - `Option<T>` -> `Option<T>`
    - `Result<T,E>` -> `Result<T,E>`
  - Generate newtype wrappers for refined types (Section 6.2):
    `type Pos = { v: Int | v > 0 }` -> `struct Pos(i64);`

- [ ] **T021**: Implement contract codegen
  - Depends on: T020
  - `requires { P }` -> `debug_assert!(P, "requires: P");` at function entry
  - `ensures { Q }` -> `debug_assert!(Q, "ensures: Q");` before return
  - `old(expr)` -> save expr value in a local before the body executes
  - Generate function signatures from input/output clauses
  - Spec reference: Section 6.7

- [ ] **T022**: Implement Cargo project generation
  - Depends on: T021
  - Generate a complete Cargo workspace:
    ```
    generated/
      Cargo.toml          # [package] with dependencies
      src/
        lib.rs            # All generated Rust code
    ```
  - Cargo.toml includes `edition = "2024"` and any needed deps
  - Format all generated .rs files with prettyplease
  - Spec reference: Section 10.3

- [ ] **T023**: Implement struct and enum codegen
  - Depends on: T020
  - Assura `type Foo { field: Int }` -> Rust `struct Foo { field: i64 }`
  - Assura `enum Bar { A, B(Int) }` -> Rust `enum Bar { A, B(i64) }`
  - Generate `#[derive(Debug, Clone, PartialEq)]` on all generated types
  - Handle visibility: `pub field` -> `pub field`

### 0.5 CLI Completion (1 week)

> Wire up the full compilation pipeline in the CLI.

- [ ] **T024**: Implement `assura check` command
  - Depends on: T018
  - Parse -> resolve -> type-check
  - Report all errors with codes and source spans
  - Exit 0 if no errors, exit 1 if errors
  - Support `--json` output mode (structured errors as JSON)
  - Support `--human` output mode (ariadne-formatted, default)

- [ ] **T025**: Implement `assura build` command
  - Depends on: T022, T024
  - Parse -> resolve -> type-check -> codegen
  - Write generated Rust project to `generated/` directory
  - Invoke `cargo build` on the generated project
  - Forward cargo's stdout/stderr
  - Exit with cargo's exit code

- [ ] **T026**: Implement `assura init` command
  - Depends on: T024
  - Create a new Assura project:
    ```
    project-name/
      assura.toml         # Project configuration
      contracts/
        lib.assura        # Starter contract
    ```
  - `assura.toml` format per Section 10.3 of spec
  - Starter contract: SafeDivision example from ROADMAP.md

- [ ] **T027**: Implement `assura explain <error-code>` command
  - Depends on: T024
  - Look up error code in the error catalog
  - Print: description, example code that triggers it, how to fix
  - Spec reference: Section 10.2

### 0.6 Phase 0 Integration (1 week)

- [ ] **T028**: End-to-end test: SafeDivision contract
  - Depends on: T025
  - Write `tests/e2e/safe_division.assura`:
    ```assura
    contract SafeDivision {
      input(a: Int, b: Int)
      output(result: Int)
      requires { b != 0 }
      ensures  { result * b + (a mod b) == a }
      effects  { pure }
    }
    ```
  - `assura check` passes
  - `assura build` generates valid Rust
  - `cargo build` on generated code succeeds
  - Generated code contains `debug_assert!(b != 0)`

- [ ] **T029**: CI setup (GitHub Actions)
  - Depends on: T005
  - `.github/workflows/ci.yml`:
    - Trigger: push to main, pull requests
    - Jobs: `cargo build`, `cargo test --workspace`,
      `cargo clippy --workspace -- -D warnings`, `cargo fmt --check --all`
    - Matrix: stable Rust on ubuntu-latest
    - Cache: `Swatinem/rust-cache`
    - Timeout: 15 minutes

- [ ] **T030**: Add README.md
  - Depends on: T028
  - Project description (from LANDING.md)
  - Quick start (install, init, check, build)
  - Example contract
  - Link to SPECIFICATION.md, ROADMAP.md
  - License badge, CI badge

---

## Phase 1: Alpha - Verification Pipeline (Target: Z3-powered proofs)

### 1.1 Linear Types (2 weeks)

- [ ] **T031**: Implement usage tracking in type checker
  - Depends on: T018
  - Extend `assura-types` with usage grades:
    - Grade 0: erased (ghost)
    - Grade 1: linear (use exactly once)
    - Grade n: exact count
    - Grade omega: unlimited
  - Track how many times each variable is used
  - Spec reference: Section 2.5

- [ ] **T032**: Implement context splitting
  - Depends on: T031
  - At each branch point (if/match), split the linear context
  - Variables used in one branch cannot be used in the other
  - After branches merge, variables used in both branches are consumed
  - Emit A05001-A05005 (linearity errors)

- [ ] **T033**: Test cases for linear types
  - Depends on: T032
  - Test Case 1 from Section 13 of spec:
    refinement predicate on a linear variable should NOT count as a use
  - Test: double-use of linear variable -> A05001
  - Test: unused linear variable -> A05002
  - Test: linear variable correctly used once -> passes

### 1.2 Typestate (1 week)

- [ ] **T034**: Implement typestate checker
  - Depends on: T032 (typestate requires linearity)
  - Build DFA per typestate variable from `states:` declaration
  - Track current state through control flow
  - At each operation call, verify the object is in the required state
  - After operation, transition to the declared next state
  - Emit A06001-A06004 (typestate errors)
  - Spec reference: Section 2.6

- [ ] **T035**: Test cases for typestate
  - Depends on: T034
  - Test: valid state transition sequence -> passes
  - Test: operation called in wrong state -> A06001
  - Test: ambiguous state after diverging branches -> A06004
  - Test: typestate variable must be linear -> A06002

### 1.3 Effect System (1 week)

- [ ] **T036**: Implement effect checker
  - Depends on: T032 (effect system uses linearity for resource effects)
  - Each function declares an effect row: `effects { io, mem }`
  - Function body's actual effects must be subset of declared effects
  - Effect hierarchy: `io` = union of all IO sub-effects (Section 3.6)
  - `pure` = empty effect set
  - Emit A07001-A07005
  - Spec reference: Section 3.5

- [ ] **T037**: Test cases for effects
  - Depends on: T036
  - Test: pure function calling effectful function -> A07001
  - Test: function with correct effect declaration -> passes
  - Test: effect containment across call chain -> A07002

### 1.4 Z3 Integration (3 weeks) -- CRITICAL PATH

> This is the hardest and most important milestone. Everything after
> this depends on having a working SMT solver connection.

- [ ] **T038**: Create `assura-smt` crate with Z3 bindings
  - Depends on: T018
  - New crate: `crates/assura-smt/`
  - Add `z3` crate dependency (Rust bindings to libz3)
  - Set up: solver context, sort declarations, function declarations
  - Implement timeout mechanism: 1s default for Layer 1
  - Entry point: `verify(TypedFile) -> Vec<VerificationResult>`
  - `VerificationResult`: `Verified | Counterexample(Model) | Timeout | Unknown`
  - Spec reference: Section 5.1

- [ ] **T039**: Encode refinement type subtyping as SMT queries
  - Depends on: T038
  - Core encoding (Section 5.2):
    `{v: T | P} <: {v: T | Q}` becomes:
    `(assert P) (assert (not Q)) (check-sat)`
    UNSAT = subtyping holds, SAT = counterexample exists
  - SMT theories for Layer 1:
    - QF_UFLIA: quantifier-free linear integer arithmetic + uninterpreted fns
    - QF_UFLRA: same with reals (float contracts)
    - QF_DT: datatypes (info flow labels, typestate)
  - Start with integer arithmetic predicates only

- [ ] **T040**: Implement counterexample extraction
  - Depends on: T039
  - When Z3 returns SAT, extract the model:
    - Variable names and their concrete values
    - Format as structured JSON (Section 5.3)
  - This is critical for AI iteration: the counterexample tells the AI
    exactly what input breaks the contract

- [ ] **T041**: Wire Z3 into the compilation pipeline
  - Depends on: T039, T024
  - After type checking, run SMT verification on all contracts
  - Report results: verified, counterexample, timeout
  - `assura check` reports verification results alongside type errors
  - `assura check --layer 0` skips SMT (structural checks only)
  - `assura check --layer 1` runs Layer 1 SMT (default)

- [ ] **T042**: Test Z3 integration with simple contracts
  - Depends on: T041
  - Test: `requires { x > 0 } ensures { result > 0 }` with body
    `result = x + 1` -> VERIFIED
  - Test: `requires { true } ensures { result > 0 }` with body
    `result = x` -> COUNTEREXAMPLE (x = 0 or x = -1)
  - Test: contract with timeout -> TIMEOUT result
  - Test: SafeDivision contract -> VERIFIED

### 1.5 CORE Features (3 weeks)

- [ ] **T043**: Implement CORE.1 Ghost code
  - Depends on: T041
  - Ghost variables, functions, and blocks: exist in logic, erased at runtime
  - Enforce: ghost code cannot affect runtime values
  - Ghost functions must be pure
  - Ghost assertions become SMT proof obligations
  - Codegen: completely erased (or debug_assert in debug mode)
  - Error codes: A54001-A54005
  - Spec reference: Section 14.CORE.1

- [ ] **T044**: Implement CORE.2 Lemmas
  - Depends on: T043 (lemmas use ghost infrastructure)
  - Proof functions that generate no runtime code
  - `apply lemma_name(args)` adds the lemma's ensures as an assumption
  - `induction var` generates base case + inductive case
  - Error codes: A55001-A55005
  - Spec reference: Section 14.CORE.2

- [ ] **T045**: Implement CORE.3 Frame conditions
  - Depends on: T041
  - `modifies` clause declares what a function changes
  - Everything not listed is implicitly unchanged
  - Critical for modular verification: without this, the verifier
    must re-prove all invariants after every call
  - Spec reference: Section 14.CORE.3

### 1.6 MEM.1 + SEC.1 (4 weeks) -- THE MVP FEATURES

- [ ] **T046**: Implement MEM.1 Memory region contracts
  - Depends on: T041, T043 (uses ghost regions)
  - Buffer bounds contracts: `requires offset + len <= buf.capacity`
  - Ghost regions tracking valid index ranges
  - SMT encoding of region containment (region_a subset region_b)
  - Error codes for: buffer overread, overwrite, out-of-bounds
  - Spec reference: Section 14.MEM.1
  - Test: parse and verify the libwebp-huffman.assura demo

- [ ] **T047**: Implement SEC.1 Untrusted data taint tracking
  - Depends on: T041, T043
  - Taint labels on data from external sources
  - Taint propagation through operations
  - Taint must be explicitly validated before use in sensitive positions
    (array indices, allocation sizes, SQL queries)
  - Information flow lattice (Section 2.7)
  - Spec reference: Section 14.SEC.1
  - Test: tainted index used without validation -> error

- [ ] **T048**: End-to-end: libwebp CVE-2023-4863 prevention demo
  - Depends on: T046, T047
  - Parse `demos/libwebp-huffman.assura`
  - Type-check it
  - Verify with Z3: prove buffer overflow is impossible
  - Generate Rust code
  - Compile generated Rust
  - This is THE demo that proves Assura works

### 1.7 Phase 1 Polish (1 week)

- [ ] **T049**: Error catalog for Phase 1
  - Depends on: T032, T034, T036, T041
  - Implement all error codes: A01xxx-A08xxx, A10xxx, A11xxx
  - Each error: location, secondary locations, contract reference,
    counterexample (when SMT), suggested fixes with confidence scores
  - `assura explain` works for all Phase 1 error codes

- [ ] **T050**: Section 13 type interaction tests
  - Depends on: T032, T034, T036
  - Implement all 11 test cases from Section 13 of the spec
  - These cover pairwise interactions between:
    refinement, linear, typestate, effects, info-flow, dependent
  - Each test is both a specification and a regression test

---

## Phase 2: Beta - Feature Completeness (Target: all primary features)

### 2.1 Remaining Type System (4 weeks)

> These tasks are independent of each other and can run in parallel.
> All depend on T041 (Z3 integration).

- [ ] **T051**: Information flow checker (A08001-A08005)
  - Depends on: T041
  - Security lattice: Public < Internal < Confidential < Restricted
  - Declassification tracking
  - Purpose labels for GDPR (Section 2.7)

- [ ] **T052**: Dependent types (restricted)
  - Depends on: T041
  - Types depending on Nat, Bool, finite enums
  - `Vec<T, n>` with index arithmetic
  - Index erasure at runtime
  - A03006

- [ ] **T053**: Totality checker (A09001-A09004)
  - Depends on: T041
  - Termination checking via `decreases` measures
  - `partial` escape hatch

- [ ] **T054**: Measures
  - Depends on: T041
  - `len`, `elems`, `keys`, `values`, `size`
  - Encode as uninterpreted functions in SMT

### 2.2 MEM Features (3 weeks, parallelizable)

- [ ] **T055**: MEM.2 Fixed-width integers
  - Depends on: T041
  - Overflow detection, checked_add/checked_mul

- [ ] **T056**: MEM.3 Allocator contracts
  - Depends on: T046
  - Allocation/deallocation pairing, size tracking, arena lifetime

- [ ] **T057**: MEM.4 Circular buffer contracts
  - Depends on: T046
  - Wrap-around indexing, logical-to-physical mapping

### 2.3 SEC Features (4 weeks, parallelizable)

- [ ] **T058**: SEC.2 FFI boundary contracts
  - Depends on: T041
  - extern/bind declarations, trust boundaries

- [ ] **T059**: SEC.3 Constant-time execution
  - Depends on: T047
  - Reject branches on secret data

- [ ] **T060**: SEC.4 Secure erasure
  - Depends on: T032, T047
  - Linear type consumed via zeroize

- [ ] **T061**: SEC.5 Cryptographic conformance
  - Depends on: T041
  - Top-level theorem connecting code to math spec

### 2.4 TYPE Features (3 weeks, parallelizable)

- [ ] **T062**: TYPE.1 Interface contracts
  - Depends on: T041
  - Trait-like contracts, callback re-entrancy restrictions

- [ ] **T063**: TYPE.2 Recursive structural invariants
  - Depends on: T041
  - Tree balance, list sortedness, graph acyclicity

- [ ] **T064**: TYPE.3 Error propagation
  - Depends on: T018
  - `must_propagate` on error types, detect silently swallowed errors

### 2.5 CONC Features (5 weeks, partially parallelizable)

- [ ] **T065**: CONC.1 Shared memory protocols
  - Depends on: T032, T041
  - Per-object access modes, data race detection

- [ ] **T066**: CONC.2 Callback re-entrancy
  - Depends on: T062
  - Prevent re-entrant calls through callback chains

- [ ] **T067**: CONC.3 Determinism contracts
  - Depends on: T041
  - Ban HashMap, ban random, enforce ordering

- [ ] **T068**: CONC.4 Lock ordering
  - Depends on: T041
  - Static lock hierarchy, deadlock prevention

- [ ] **T069**: CONC.5 Temporal deadlines
  - Depends on: T041
  - Bounded response time

### 2.6 FMT Features (5 weeks, parallelizable)

- [ ] **T070**: FMT.1 Binary format contracts
  - Depends on: T041
  - Byte-aligned format contracts

- [ ] **T071**: FMT.2 Bit-level format contracts
  - Depends on: T043, T041
  - Sub-byte parsing, ghost bit cursor

- [ ] **T072**: FMT.3 String encoding contracts
  - Depends on: T041
  - UTF-8/UTF-16 safety

- [ ] **T073**: FMT.4 Codec dispatch
  - Depends on: T070
  - Magic-byte routing

- [ ] **T074**: FMT.5 Checksum integrity
  - Depends on: T041
  - CRC32, Adler-32, SHA verification

- [ ] **T075**: FMT.6 Protocol grammar
  - Depends on: T034, T041
  - RFC conformance, state machine

### 2.7 Layer 2 Verification (2 weeks)

- [ ] **T076**: Implement Layer 2 SMT encoding
  - Depends on: T041
  - Quantified invariants (AUFLIA)
  - Functional correctness (AUFLIA + UF)
  - Termination proofs
  - Serialization roundtrip
  - Timeout: 10s default, configurable

- [ ] **T077**: CORE.4 Axiomatic definitions
  - Depends on: T041
  - Abstract mathematical concepts

- [ ] **T078**: CORE.5 Quantifier triggers
  - Depends on: T076
  - E-matching hints for SMT solver

- [ ] **T079**: CORE.6 Opaque functions
  - Depends on: T041
  - Hide implementation from verifier

### 2.8 Tooling (4 weeks, parallelizable with everything above)

> These do NOT depend on the verification features. They can be built
> in parallel with Phase 2.1-2.7.

- [ ] **T080**: LSP server
  - Depends on: T012, T018
  - New crate: `crates/assura-lsp/`
  - Language Server Protocol implementation
  - Features: completions, go-to-definition, hover, inline diagnostics
  - Use `tower-lsp` crate

- [ ] **T081**: VS Code extension
  - Depends on: T080
  - TextMate grammar for syntax highlighting
  - LSP client configuration
  - Publish to VS Code Marketplace (name already claimed? check)

- [ ] **T082**: tree-sitter grammar
  - Depends on: T007
  - Separate grammar for editor support (NOT the compiler parser)
  - Error-tolerant by design
  - Can be used by neovim, helix, zed, etc.

- [ ] **T083**: TEST.1 Test generation from contracts
  - Depends on: T041
  - Generate property-based tests from requires/ensures
  - Use proptest or quickcheck in generated Rust
  - Generate boundary value tests from refinement predicates

### 2.9 Phase 2 Integration (2 weeks)

- [ ] **T084**: End-to-end: zlib CVE-2022-37434 demo
  - Depends on: T070, T074, T046
  - Parse, verify, codegen `demos/zlib-inflate.assura`

- [ ] **T085**: End-to-end: mbedTLS 4-CVE demo
  - Depends on: T047, T059, T061
  - Parse, verify, codegen `demos/mbedtls-x509.assura`

---

## Phase 3: Advanced Features (Target: research-adjacent capabilities)

### 3.1 STOR Features (6 weeks, parallelizable)

- [ ] **T086**: STOR.1 Crash recovery contracts
  - Depends on: T034, T041

- [ ] **T087**: STOR.2 Page cache contracts
  - Depends on: T034, T041

- [ ] **T088**: STOR.3 MVCC / snapshot isolation
  - Depends on: T041

- [ ] **T089**: STOR.4 Transactional rollback
  - Depends on: T034

- [ ] **T090**: STOR.5 Monotonic state
  - Depends on: T043, T041

- [ ] **T091**: STOR.6 Storage failure model
  - Depends on: T041

### 3.2 Advanced Verification (8 weeks)

> These are the hardest features. Each needs dedicated focus.

- [ ] **T092**: CONC.6 Weak memory ordering
  - Depends on: T065, T076
  - Per-thread ghost views (GPS/RSL approach)
  - Model all 5 C++ memory orderings
  - Effort: 3 weeks. Budget for dead ends.

- [ ] **T093**: CORE.7 Prophecy variables
  - Depends on: T043, T076
  - Ghost state with deferred resolution
  - SMT encoding uses Skolemization
  - Effort: 2 weeks

- [ ] **T094**: CORE.8 Liveness contracts
  - Depends on: T076
  - `eventually`, `leads_to`, `eventually_within`
  - BMC with lasso detection (Layer 2)
  - K-induction (Layer 3)
  - Fairness encoding
  - Effort: 3 weeks

### 3.3 Remaining Features (6 weeks, parallelizable)

- [ ] **T095**: NUM.1 Numerical precision
  - Depends on: T041
- [ ] **T096**: NUM.2 Precomputed table verification
  - Depends on: T041
- [ ] **T097**: PLAT.1 Platform abstraction
  - Depends on: T041
- [ ] **T098**: PLAT.2 Feature flags
  - Depends on: T041
- [ ] **T099**: PLAT.3 Resource limits
  - Depends on: T041
- [ ] **T100**: PERF.1 Unsafe escape with proof
  - Depends on: T041
- [ ] **T101**: PERF.2 Complexity bounds (AARA)
  - Depends on: T076
- [ ] **T102**: TEST.2 Behavioral equivalence
  - Depends on: T041
- [ ] **T103**: TEST.3 Multi-pass refinement
  - Depends on: T041
- [ ] **T104**: MISC.1 Incremental contracts
  - Depends on: T034
- [ ] **T105**: MISC.2 Scoped invariant suspension
  - Depends on: T041

### 3.4 AI Agent API (2 weeks)

- [ ] **T106**: gRPC service implementation
  - Depends on: T041
  - RPCs: Check, Build, Explain, Health, CheckStream
  - JSON-over-HTTP fallback
  - Spec reference: Section 11.2

---

## Phase 4: v1.0 (Target: production release)

### 4.1 Standard Library (3 weeks)

- [ ] **T107**: Core types (Pos, NonNeg, Email, Uuid)
  - Depends on: T041
- [ ] **T108**: Collection contracts (ListOps, sort, filter)
  - Depends on: T041
- [ ] **T109**: CRUD patterns, auth contracts
  - Depends on: T041

### 4.2 Module System Completion (2 weeks)

- [ ] **T110**: Contract composition with `extends`
  - Depends on: T011
- [ ] **T111**: Contract libraries as publishable packages
  - Depends on: T110

### 4.3 IR Format (2 weeks)

- [ ] **T112**: Implementation IR parser (Section 4)
  - Depends on: T008
  - Text format parser
  - Binary (MessagePack) serializer
  - This is what AI agents generate

### 4.4 Performance (3 weeks)

- [ ] **T113**: Verification caching
  - Depends on: T041
  - Hash contract + implementation, skip if unchanged

- [ ] **T114**: Parallel SMT queries
  - Depends on: T041
  - Independent contracts verified in parallel

- [ ] **T115**: Incremental compilation
  - Depends on: T011
  - Only re-check changed modules

### 4.5 Ecosystem (4 weeks)

- [ ] **T116**: GitHub Action (assura-lang/verify-action)
  - Depends on: T025

- [ ] **T117**: Documentation (tutorial, internals, API reference)
  - Depends on: T106

- [ ] **T118**: Showcase builds (full CVE demos with differential testing)
  - Depends on: T048, T084, T085

- [ ] **T119**: Cranelift backend for fast dev builds
  - Depends on: T022

---

## Dependency Graph

```
T001-T004 (parser, DONE)
  │
  ├─► T005 (snapshot tests)
  │     └─► T006 (error tests)
  │     └─► T029 (CI) ──────────────────────────────────┐
  │                                                       │
  ├─► T007 (full keywords)                                │
  │     └─► T082 (tree-sitter) ─► T081 (VS Code ext)     │ PARALLEL
  │                                                       │ TRACK
  ├─► T008 (expression parser)                            │
  │     └─► T009 (resolve crate)                          │
  │           └─► T010 (scope analysis)                   │
  │                 ├─► T011 (imports)                     │
  │                 └─► T012 (type refs)                   │
  │                       └─► T013 (types crate)          │
  │                             └─► T014 (base types)     │
  │                                   ├─► T015 (generics) │
  │                                   ├─► T016 (fields)   │
  │                                   ├─► T017 (patterns) │
  │                                   └─► T018 (clauses)  │
  │                                         │              │
  │                    ┌──────────┬─────────┤              │
  │                    │          │         │              │
  │                    ▼          ▼         ▼              │
  │               T031-T033  T034-T035  T036-T037         │
  │               (linear)   (typestate) (effects)        │
  │                    │          │         │              │
  │                    └──────────┴─────────┘              │
  │                              │                         │
  │                    ┌─────────┤                         │
  │                    │         │                         │
  │                    ▼         ▼                         │
  │               T019-T023  T038-T042                    │
  │               (codegen)  (Z3 / SMT) ◄── CRITICAL     │
  │                    │         │                         │
  │                    ▼         │                         │
  │               T024-T027  T043-T045 (CORE.1-3)        │
  │               (CLI)        │                          │
  │                    │       ▼                           │
  │                    ▼   T046-T047 (MEM.1 + SEC.1)      │
  │               T028       │                            │
  │               (e2e)      ▼                            │
  │                    └─► T048 (libwebp demo) ◄── MVP    │
  │                                                       │
  │               T080-T081 (LSP + VS Code) ◄─────────────┘
  │                                           PARALLEL TRACK
  │
  ▼
Phase 2: T051-T085 (all depend on T041, many parallelizable)
  │
  ▼
Phase 3: T086-T106 (STOR, advanced verification, AI API)
  │
  ▼
Phase 4: T107-T119 (stdlib, perf, ecosystem)
```

### Key Parallelization Opportunities

1. **T029 (CI) + T007 (keywords) + T008 (expressions)**: All depend
   only on T001-T004 (done). Start all three simultaneously.

2. **T082 (tree-sitter) + T080 (LSP)**: Entire editor tooling track
   is independent of the verification pipeline. Can be built by a
   separate person/agent from Month 1.

3. **T019-T023 (codegen) + T038-T042 (Z3)**: Codegen depends on T018,
   Z3 depends on T018. Both can start as soon as type checking works.

4. **Phase 2 features (T051-T079)**: Most features only depend on T041
   (Z3 integration). Once Z3 works, 20+ features can be built in
   parallel by multiple agents.

5. **Phase 3 STOR (T086-T091)**: All 6 features are independent of
   each other once T041 is done.

### Sequential Bottlenecks (cannot parallelize)

1. **T008 -> T009 -> T010 -> T012 -> T013 -> T014 -> T018**: Parser
   expressions through type checking. Each step needs the previous.

2. **T038 -> T039 -> T040 -> T041**: Z3 integration is inherently
   sequential (each step builds on the solver connection).

3. **T031 -> T032 -> T034**: Linear types -> context splitting ->
   typestate. Typestate requires linearity.

---

## Progress Notes

> Agents: write a brief note here when completing a task or ending a session.

### Session 1 (2026-06-12)
- T001-T004 completed in prior sessions (market-research repo)
- Code copied to assura-lang/assura with proper workspace structure
- AGENTS.md and MASTER-PLAN.md created
- Parser verified: all 4 demo/test files parse successfully
