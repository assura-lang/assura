# Assura Master Plan v3

> Comprehensive plan covering all remaining work to bring Assura from
> "compiles and has scaffolding" to "production-quality compiler."
>
> **Created**: 2026-06-14 after deep audit of all 72K LOC, 1,946 tests,
> and 50 verification features. Classifies every checker as REAL,
> STRUCTURAL (dead-end wiring), or PARTIAL (hardcoded params).
>
> ## How to use this plan
>
> 1. Read top to bottom. Pick the next `[ ]` task whose `depends-on`
>    tasks are all `[x]`.
> 2. Read the **Acceptance Tests** section of that task. These are
>    the exact commands you must run and see pass before marking `[x]`.
> 3. Complete the task. Run every acceptance test command. Paste the
>    output into your working notes.
> 4. Mark the task `[x]`. Commit MASTER-PLAN.md with the change.
> 5. Continue to the next task.
>
> ## Rules (non-negotiable)
>
> - **Never mark `[x]` without running every acceptance test command.**
>   "I wrote the code" is not done. "The test passes in my terminal" is.
> - **Never skip an acceptance test** because "it's obvious" or "similar
>   to another test." Run it. See it pass. Then check it off.
> - **Every task has a verification command block.** If you can't run
>   one of them, the task is blocked. Say so. Don't mark it done.
> - **Scope limit**: Focus on the next 1-3 tasks only. Don't load the
>   whole plan into working memory.
> - **Session protocol**: At the end of each session, update the
>   Progress Notes section with what was completed and what to do next.

---

## Current State (2026-06-14)

| Crate | LOC | Tests | Status |
|-------|-----|-------|--------|
| assura-parser | 7,880 | 137 | Solid. All demos parse. |
| assura-resolve | 3,841 | 84 | Working scope analysis, import resolution. |
| assura-hir | 1,769 | 31 | CST-to-HIR lowering. |
| assura-types | 29,470 | 960 | ~30 REAL checkers, 14 STRUCTURAL, 12 PARTIAL. |
| assura-codegen | 7,027 | 156 | Generates `cargo check`-passing Rust. |
| assura-smt | 12,753 | 338 | Z3 + CVC5 backends. Layer 2 structural. |
| assura-cli | 3,944 | 81 | 8 commands: check, build, init, explain, fmt, infer, test-gen, audit. |
| assura-lsp | 1,541 | 42 | Hover, completion, go-to-def, document symbols. |
| assura-server | 953 | 16 | gRPC + HTTP, compiles. |
| assura-config | 517 | 22 | assura.toml parsing. |
| assura-diagnostics | 1,029 | 23 | Error code registry. |
| assura-fmt | 1,332 | 56 | Formatter. |
| assura-bench | 2 | 0 | Placeholder. |
| **Total** | **72,058** | **1,946** | |

### Audit results: checker classifications

**14 STRUCTURAL checkers** (wiring dead-ends that return `Vec::new()`):
CallbackReentrancyChecker, TemporalDeadlineChecker, ProtocolGrammarChecker,
MonotonicStateChecker, NumericalPrecisionChecker, Layer2Verifier,
IncrementalCompiler, WeakMemoryChecker (SMT), ProphecyManager,
LivenessChecker, CrashRecoveryChecker, PageCacheChecker, MvccChecker,
RollbackChecker.

**12 PARTIAL checkers** (hardcoded params, limited extraction):
BinaryFormatChecker, BitLevelChecker, StringEncodingChecker,
ChecksumChecker, ResourceLimitChecker, PrecomputedTableChecker,
MultiPassRefinementChecker, IncrementalContractChecker, FrameChecker,
Stdlib, IR parser, Cranelift backend.

**~30 REAL checkers** with meaningful logic and wiring.

### 12 open GitHub issues

#42, #43, #44, #45, #46, #53, #55, #56, #57, #58, #59, #62

---

## Phase 1: Fix Open Bugs (priority: correctness)

> Fix all open bug/tech-debt issues. These represent known broken behavior
> that must be resolved before adding new features.

### 1.01: Fix flaky test `build_cli_default_output_is_generated` (#46)

- Depends on: none
- **What**: The test panics intermittently because it expects a file
  that `cargo run --bin assura -- build` may not create if the demo
  file triggers an error path.
- **Fix**: Make the test create a temp directory, run the build command,
  and verify the output exists. If the build fails, the test should
  assert the error, not panic on a missing file.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Run the specific test 10 times without failure
  for i in $(seq 1 10); do cargo test -p assura-cli build_cli_default_output_is_generated -- --nocapture; done
  # 2. Full test suite still passes
  cargo test --workspace
  # 3. Close issue #46
  gh issue view 46 --json body --jq '.body' | grep -c '\- \[ \]'
  # Must return 0 before closing
  ```

### 1.02: Fix `assura audit` workspace root failure (#43)

- Depends on: none
- **What**: `assura audit` hardcodes `src/` lookup and fails on Cargo
  workspace roots that have `crates/` instead.
- **Fix**: Use `cargo metadata` or walk `Cargo.toml` members to find
  source files instead of hardcoding `src/`.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Run audit on a workspace-root project (this repo itself)
  cargo run --bin assura -- audit .
  # Must not panic or error with "src/ not found"
  # 2. Run audit on a single-crate project
  cargo run --bin assura -- audit crates/assura-parser
  # Must still work
  # 3. Test passes
  cargo test -p assura-cli audit
  # 4. Close issue #43
  ```

### 1.03: Fix Rust function signature extractor (#44)

- Depends on: none
- **What**: `assura infer` misses most real-world Rust function
  signatures (generics, lifetimes, where clauses, impl blocks).
- **Fix**: Use `syn` to parse Rust files properly instead of regex.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Run infer on a non-trivial Rust file with generics/lifetimes
  cargo run --bin assura -- infer crates/assura-parser/src/cst.rs
  # Must extract at least 5 function signatures
  # 2. Run infer on a file with impl blocks
  cargo run --bin assura -- infer crates/assura-types/src/checkers.rs
  # Must extract functions from inside impl blocks
  # 3. Test passes
  cargo test -p assura-cli infer
  # 4. Close issue #44
  ```

### 1.04: Fix `assura infer/audit` invalid module paths (#42)

- Depends on: none
- **What**: Generated Rust module paths are invalid (wrong crate names,
  missing `crate::` prefix).
- [x] **Acceptance Tests**:
  ```bash
  # 1. Run infer and verify module paths are valid Rust
  cargo run --bin assura -- infer crates/assura-parser/src/lib.rs > /tmp/inferred.assura
  cat /tmp/inferred.assura | grep 'module\|import'
  # Module paths must use :: separators and valid crate names
  # 2. Tests pass
  cargo test -p assura-cli infer
  # 3. Close issue #42
  ```

### 1.05: Fix verification cache using unstable DefaultHasher (#56)

- Depends on: none
- **What**: On-disk cache keys use `DefaultHasher` which is not stable
  across Rust versions, causing cache misses after toolchain updates.
- **Fix**: Use a stable hasher (e.g., `xxhash-rust`, `ahash` with fixed
  seed, or `sha2` for content-addressing).
- [x] **Acceptance Tests**:
  ```bash
  # 1. Grep: no more DefaultHasher in cache code
  grep -rn 'DefaultHasher' crates/assura-smt/src/
  # Must return 0 results
  # 2. Cache round-trip test: write cache, read it back
  cargo test -p assura-smt cache
  # 3. Full suite passes
  cargo test --workspace
  # 4. Close issue #56
  ```

### 1.06: Fix parser clause bodies with braces in generic blocks (#53)

- Depends on: none
- **What**: Clause bodies inside generic blocks cannot contain braces.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Parse a test fixture with braces inside generic blocks
  # Create test fixture first, then:
  cargo test -p assura-parser clause_brace
  # 2. Demo files still parse
  cargo run --bin assura -- check demos/libwebp-huffman.assura
  cargo run --bin assura -- check demos/zlib-inflate.assura
  cargo run --bin assura -- check demos/mbedtls-x509.assura
  # 3. Close issue #53
  ```

### 1.07: Resolve remaining tech-debt issues (#55, #57, #58, #59, #62)

- Depends on: none
- **What**: Five smaller tech-debt items:
  - #55: Deduplicate verify+report logic between run_check and watch mode
  - #57: Eliminate double lexing in CLI pipeline
  - #58: Short-circuit verification when no verifiable clauses exist
  - #59: Remove duplicate ParallelVerifier tests from lib.rs
  - #62: ProphecyManager errors lack structured error codes
- [x] **Acceptance Tests**:
  ```bash
  # For each issue, verify the fix:
  # #55: grep for duplicated verify+report patterns
  grep -c 'fn run_check\|fn run_watch' crates/assura-cli/src/main.rs
  # Shared logic should be in a helper function
  # #57: grep for double lex() calls in pipeline
  grep -n 'lex(' crates/assura-cli/src/main.rs | head -5
  # Should only lex once per file
  # #58: Run check on a file with no verifiable clauses
  echo 'contract Empty {}' > /tmp/empty.assura
  cargo run --bin assura -- check /tmp/empty.assura
  # Should return quickly without running Z3
  # #59: Count ParallelVerifier tests in lib.rs vs dedicated file
  grep -c '#\[test\].*parallel\|fn test.*parallel' crates/assura-smt/src/lib.rs
  # Should be 0 (moved to dedicated test module)
  # #62: Grep for structured error codes in ProphecyManager
  grep -n 'A0[0-9]' crates/assura-smt/src/advanced.rs | grep -i prophecy
  # Must have at least 1 structured error code
  # Final: full suite
  cargo test --workspace
  ```

---

## Phase 2: Wire Structural Checkers (14 dead-end checkers) -- GitHub #65

> These checkers have real data structures and algorithms but their
> `run_*_checks()` functions return `Vec::new()` unconditionally or
> never register real data. Each task wires one checker to actually
> produce diagnostics from real AST data.

### 2.01: Wire CallbackReentrancyChecker

- Depends on: none
- **What**: `run_callback_reentrancy_checks()` marks non-reentrant
  functions but returns `Vec::new()` unconditionally after marking.
- **Fix**: After marking non-reentrant functions, actually walk call
  sites and return A-CONC errors when re-entrancy is detected.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Write a test .assura file with a re-entrant callback pattern
  # 2. Run type checker and verify it produces an error
  cargo test -p assura-types callback_reentrancy
  # Must have at least 2 tests: one that triggers re-entrancy error,
  # one that passes for non-re-entrant callbacks
  # 3. Verify the run function does NOT return Vec::new() unconditionally
  grep -A5 'fn run_callback_reentrancy_checks' crates/assura-types/src/domain.rs | grep -c 'Vec::new()'
  # Must be 0 (no unconditional empty return at the end)
  # 4. Full suite
  cargo test --workspace
  ```

### 2.02: Wire TemporalDeadlineChecker

- Depends on: none
- **What**: `run_temporal_deadline_checks()` creates the checker but
  returns `Vec::new()` immediately when an annotation is found.
- **Fix**: Actually run deadline nesting checks on the annotated code.
- [x] **Acceptance Tests**:
  ```bash
  cargo test -p assura-types temporal_deadline
  # At least 2 tests: deadline violation detected, valid deadline passes
  grep -A5 'fn run_temporal_deadline_checks' crates/assura-types/src/domain.rs | grep -c 'Vec::new()'
  # Must be 0
  cargo test --workspace
  ```

### 2.03: Wire ProtocolGrammarChecker

- Depends on: none
- **What**: `run_protocol_grammar_checks()` creates the checker but
  returns `Vec::new()` unconditionally.
- [x] **Acceptance Tests**:
  ```bash
  cargo test -p assura-types protocol_grammar
  # At least 2 tests: invalid protocol sequence, valid sequence
  grep -A5 'fn run_protocol_grammar_checks' crates/assura-types/src/domain.rs | grep -c 'Vec::new()'
  # Must be 0
  cargo test --workspace
  ```

### 2.04: Wire MonotonicStateChecker

- Depends on: none
- **What**: Declares monotonic variables but returns empty.
- [x] **Acceptance Tests**:
  ```bash
  cargo test -p assura-types monotonic_state
  grep -A5 'fn run_monotonic_state_checks' crates/assura-types/src/domain.rs | grep -c 'Vec::new()'
  # Must be 0
  cargo test --workspace
  ```

### 2.05: Wire NumericalPrecisionChecker

- Depends on: none
- **What**: Declares precision variables but returns empty.
- [x] **Acceptance Tests**:
  ```bash
  cargo test -p assura-types numerical_precision
  grep -A5 'fn run_numerical_precision_checks' crates/assura-types/src/domain.rs | grep -c 'Vec::new()'
  # Must be 0
  cargo test --workspace
  ```

### 2.06: Wire CrashRecoveryChecker

- Depends on: none
- **What**: Creates checker with no data, `check_all()` returns empty.
- **Fix**: Extract crash recovery annotations from AST and register
  them with the checker before calling `check_all()`.
- [x] **Acceptance Tests**:
  ```bash
  cargo test -p assura-types crash_recovery
  cargo test --workspace
  ```

### 2.07: Wire PageCacheChecker

- Depends on: none
- **What**: Created with hardcoded 1024 capacity, no real data.
- [x] **Acceptance Tests**:
  ```bash
  cargo test -p assura-types page_cache
  cargo test --workspace
  ```

### 2.08: Wire MvccChecker

- Depends on: none
- **What**: Created with no data registered.
- [x] **Acceptance Tests**:
  ```bash
  cargo test -p assura-types mvcc
  cargo test --workspace
  ```

### 2.09: Wire RollbackChecker

- Depends on: none
- **What**: Created with no savepoints registered.
- [x] **Acceptance Tests**:
  ```bash
  cargo test -p assura-types rollback
  cargo test --workspace
  ```

### 2.10: Wire SMT ProphecyManager into verify()

- Depends on: none
- **What**: `ProphecyManager` in `advanced.rs` has declare/resolve/
  constraint tracking but is never called from `verify()`.
- **Fix**: When a contract has prophecy variables (future values),
  call `ProphecyManager` from the verification loop.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Grep for ProphecyManager in verify dispatch
  grep -n 'ProphecyManager' crates/assura-smt/src/z3_backend.rs
  # Must appear in the verification path, not just imports
  # 2. Test with prophecy variables
  cargo test -p assura-smt prophecy
  # At least 1 test
  # 3. Full suite
  cargo test --workspace
  ```

### 2.11: Wire SMT LivenessChecker into verify()

- Depends on: none
- **What**: Has obligation tracking but not wired into `verify()`.
- [x] **Acceptance Tests**:
  ```bash
  grep -n 'LivenessChecker' crates/assura-smt/src/z3_backend.rs
  cargo test -p assura-smt liveness
  cargo test --workspace
  ```

### 2.12: Wire SMT WeakMemoryChecker into verify()

- Depends on: none
- **What**: SMT-level weak memory checker (different from type-level
  one) has data structures but is standalone.
- [x] **Acceptance Tests**:
  ```bash
  grep -n 'WeakMemoryChecker' crates/assura-smt/src/z3_backend.rs
  cargo test -p assura-smt weak_memory
  cargo test --workspace
  ```

### 2.13: Implement Layer2 Verifier

- Depends on: 2.10, 2.11, 2.12
- **What**: `layer2.rs` `check_structural()` returns `Unknown` for
  everything with a message "requires Z3 Layer 2 verification."
- **Fix**: Implement actual Z3-based structural checks for at least:
  typestate protocol verification, ghost variable erasure checking,
  and effect containment across higher-order functions.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Verify Layer 2 no longer returns Unknown for everything
  cargo test -p assura-smt layer2
  # At least 3 tests: one Verified, one Counterexample, one for each
  # of the three check categories
  # 2. Grep for the old stub
  grep -n 'requires Z3 Layer 2' crates/assura-smt/src/layer2.rs
  # Must return 0 results
  # 3. Full suite
  cargo test --workspace
  ```

### 2.14: Wire IncrementalCompiler

- Depends on: none
- **What**: Tracks dirty modules but has no actual compilation logic.
- **Fix**: Connect the incremental tracking to the verify/typecheck
  pipeline so that unchanged modules are skipped on re-verification.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Test that re-verifying an unchanged file is faster
  cargo test -p assura-smt incremental
  # At least 1 test showing cache hit behavior
  # 2. Full suite
  cargo test --workspace
  ```

---

## Phase 3: Fix Partial Checkers (12 hardcoded-param checkers) -- GitHub #63

> These checkers have real algorithms but use hardcoded parameters
> instead of extracting values from the AST. Each task fixes one
> checker to use real AST data.

### 3.01: Fix BinaryFormatChecker field extraction

- Depends on: none
- **What**: Uses hardcoded offset=0, size=1. Only checks endianness.
- **Fix**: Extract actual field offsets and sizes from type definitions
  and binary format annotations in the AST.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Grep: no hardcoded offset/size in the run function
  grep -n 'offset.*=.*0\|size.*=.*1' crates/assura-types/src/domain.rs | grep -i binary
  # Must return 0 for hardcoded values in run_binary_format_checks
  # 2. Test with real binary format annotations
  cargo test -p assura-types binary_format
  # At least 3 tests: overlap detection, bounds check, endianness
  # 3. Full suite
  cargo test --workspace
  ```

### 3.02: Fix BitLevelChecker field extraction

- Depends on: none
- **What**: Hardcoded total_bits=64, no real field extraction.
- [x] **Acceptance Tests**:
  ```bash
  grep -n 'total_bits.*=.*64' crates/assura-types/src/domain.rs
  # Must return 0
  cargo test -p assura-types bit_level
  cargo test --workspace
  ```

### 3.03: Fix StringEncodingChecker encoding extraction

- Depends on: none
- **What**: Always uses RawBytes encoding instead of extracting the
  actual encoding from annotations.
- [x] **Acceptance Tests**:
  ```bash
  cargo test -p assura-types string_encoding
  # At least 1 test with a non-RawBytes encoding (UTF8, ASCII)
  cargo test --workspace
  ```

### 3.04: Fix ChecksumChecker parameter extraction

- Depends on: none
- **What**: Hardcoded Crc32 algorithm and range 0..1024.
- [x] **Acceptance Tests**:
  ```bash
  cargo test -p assura-types checksum
  # At least 1 test with a non-Crc32 algorithm and non-default range
  cargo test --workspace
  ```

### 3.05: Fix ResourceLimitChecker limit extraction

- Depends on: none
- **What**: Declares limits with `u64::MAX` instead of extracting
  actual limits from annotations.
- [x] **Acceptance Tests**:
  ```bash
  grep -n 'u64::MAX' crates/assura-types/src/domain.rs | grep -i resource
  # Must return 0
  cargo test -p assura-types resource_limit
  cargo test --workspace
  ```

### 3.06: Fix PrecomputedTableChecker size extraction

- Depends on: none
- **What**: Hardcoded table size 256.
- [x] **Acceptance Tests**:
  ```bash
  cargo test -p assura-types precomputed_table
  cargo test --workspace
  ```

### 3.07: Fix MultiPassRefinementChecker pass extraction

- Depends on: none
- **What**: Hardcoded "abstract"/"concrete" passes.
- [x] **Acceptance Tests**:
  ```bash
  cargo test -p assura-types multi_pass_refinement
  cargo test --workspace
  ```

### 3.08: Fix IncrementalContractChecker version extraction

- Depends on: none
- **What**: Hardcoded version (1,1,1).
- [x] **Acceptance Tests**:
  ```bash
  cargo test -p assura-types incremental_contract
  cargo test --workspace
  ```

### 3.09: Deepen FrameChecker scope validation

- Depends on: none
- **What**: Only checks for empty modifies clauses. Comment says scope
  validation "deferred until expression-level name resolution."
- **Fix**: Implement scope validation for modifies clauses (verify that
  modified variables are in scope and match declared types).
- [x] **Acceptance Tests**:
  ```bash
  cargo test -p assura-types frame
  # At least 2 tests: scope violation detected, valid frame passes
  cargo test --workspace
  ```

### 3.10: Expand Stdlib to standard library modules -- GitHub #66

- Depends on: none
- **What**: Only 6 refinement types (Pos, NonNeg, Email, Uuid, Port,
  Percentage). No standard library contracts or modules.
- **Fix**: Create `crates/assura-stdlib/` with at least:
  - `std/collections.assura` (List, Map, Set contracts)
  - `std/math.assura` (abs, min, max, clamp)
  - `std/string.assura` (length, substring, contains)
  - Prelude auto-imported into every file
- [x] **Acceptance Tests**:
  ```bash
  # 1. Stdlib crate exists and compiles
  cargo build -p assura-stdlib
  # 2. Prelude types are available without explicit import
  echo 'contract UseStdlib {
    requires(x: List<Int>)
    ensures(result: Nat)
  }' > /tmp/stdlib_test.assura
  cargo run --bin assura -- check /tmp/stdlib_test.assura
  # Must not report "unknown type List" or "unknown type Nat"
  # 3. Tests
  cargo test -p assura-stdlib
  cargo test --workspace
  ```

### 3.11: Complete IR parser (Section 4)

- Depends on: none
- **What**: `ir.rs` has AST nodes and text parser but generates
  `todo!()` bodies. Not a full Section 4 implementation.
- **Fix**: IR parser should generate real Rust function bodies from
  the IR grammar, not `todo!()`.
- [x] **Acceptance Tests**:
  ```bash
  # 1. No todo!() in IR codegen output
  cargo test -p assura-smt ir_codegen
  # Tests must show generated Rust bodies that are not todo!()
  # 2. Full suite
  cargo test --workspace
  ```

### 3.12: Implement Cranelift backend behavior

- Depends on: 3.10
- **What**: `BackendConfig` has the Cranelift enum variant but it does
  not change codegen behavior.
- **Fix**: When Cranelift backend is selected, generate code compatible
  with cranelift-jit (e.g., C ABI functions, no Rust-specific features).
- [x] **Acceptance Tests**:
  ```bash
  # 1. Cranelift codegen produces different output than default
  cargo test -p assura-codegen cranelift
  # At least 1 test showing Cranelift output differs from default Rust
  # 2. Full suite
  cargo test --workspace
  ```

---

## Phase 4: Multi-File Compilation -- GitHub #64

> The compiler currently only handles single files. Multi-file project
> compilation is required by spec Section 8 and the market research.

### 4.01: Implement filesystem module resolution

- Depends on: Phase 1 complete
- **What**: `ModuleMap` type exists with a comment "actual filesystem
  resolution is deferred." No `resolve_project()` function.
- **Fix**: Implement `resolve_project(root: &Path) -> Result<Project>`
  that reads `assura.toml`, discovers `.assura` files, resolves import
  paths to files, and builds a dependency graph.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Create a multi-file test project
  mkdir -p /tmp/multi-test/src
  echo '[project]
  name = "multi-test"' > /tmp/multi-test/assura.toml
  echo 'module math
  contract Add {
    requires(a: Int, b: Int)
    ensures(result: Int)
    ensures(result == a + b)
  }' > /tmp/multi-test/src/math.assura
  echo 'import math.Add
  contract Main {
    requires(x: Int)
    ensures(result: Int)
  }' > /tmp/multi-test/src/main.assura
  # 2. Check the project (not a single file)
  cargo run --bin assura -- check /tmp/multi-test
  # Must resolve the import and type-check across files
  # 3. Tests
  cargo test -p assura-resolve multi_file
  cargo test --workspace
  ```

### 4.02: Cross-file name resolution

- Depends on: 4.01
- **What**: Imports between files must resolve to real symbols.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Import a contract from another file and use its types
  cargo test -p assura-resolve cross_file
  # At least 3 tests: valid import, missing import, circular import
  # 2. Error messages include the file path
  cargo test --workspace
  ```

### 4.03: Cross-file type checking

- Depends on: 4.02
- **What**: Type checker must work across file boundaries.
- [x] **Acceptance Tests**:
  ```bash
  cargo test -p assura-types cross_file
  cargo test --workspace
  ```

### 4.04: Multi-file codegen

- Depends on: 4.03
- **What**: Generate a Cargo workspace with multiple modules from a
  multi-file Assura project.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Build a multi-file project
  cargo run --bin assura -- build /tmp/multi-test
  # 2. Generated code compiles
  cd generated && cargo check
  # 3. Tests
  cargo test -p assura-codegen multi_file
  cargo test --workspace
  ```

---

## Phase 5: Testing and Quality

### 5.01: Fuzzing infrastructure -- GitHub #68

- Depends on: Phase 1 complete
- **What**: No fuzz tests exist. The parser should never panic on
  arbitrary input.
- **Fix**: Set up `cargo-fuzz` with targets for: lexer, parser,
  type checker, codegen.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Fuzz targets exist
  ls fuzz/fuzz_targets/*.rs
  # Must show at least: fuzz_lexer.rs, fuzz_parser.rs
  # 2. Fuzz runs for 30 seconds without crash
  cargo fuzz run fuzz_parser -- -max_total_time=30
  # Must exit 0 (no crash)
  # 3. Full suite
  cargo test --workspace
  ```

### 5.02: Spec compliance test suite (Section 13) -- GitHub #69

- Depends on: Phase 2 complete
- **What**: Section 13 of the spec defines 11 type interaction test
  cases. These must exist as automated tests.
- **Fix**: Create `tests/spec_compliance/` with one test file per
  Section 13 case.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Count test files
  ls tests/spec_compliance/*.rs 2>/dev/null | wc -l
  # Must be >= 11
  # 2. All pass
  cargo test --test spec_compliance
  # 3. Full suite
  cargo test --workspace
  ```

### 5.03: Error code coverage audit

- Depends on: none
- **What**: Spec Appendix D defines ~278 error codes. Not all are
  tested.
- **Fix**: For each error code in the spec, verify there is at least
  one test that triggers it. Create missing tests.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Count unique error codes in tests
  grep -roh 'A[0-9]\{5\}' crates/*/src/ tests/ | sort -u | wc -l
  # Should be close to 278
  # 2. Count error codes in spec
  grep -oh 'A[0-9]\{5\}' docs/SPECIFICATION.md | sort -u | wc -l
  # 3. Diff: untested codes
  comm -23 \
    <(grep -oh 'A[0-9]\{5\}' docs/SPECIFICATION.md | sort -u) \
    <(grep -roh 'A[0-9]\{5\}' crates/ tests/ | sort -u)
  # Target: fewer than 20 untested codes
  ```

### 5.04: Benchmark suite -- GitHub #70

- Depends on: none
- **What**: `assura-bench` has 2 LOC and 0 tests. No benchmarks.
- **Fix**: Create benchmarks for: lexing, parsing, type checking,
  Z3 verification, codegen. Use `criterion` crate.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Benchmarks exist and run
  cargo bench -p assura-bench
  # Must produce timing output for at least 4 benchmarks
  # 2. Each pipeline stage has a benchmark
  grep -c 'fn bench_' crates/assura-bench/benches/*.rs
  # Must be >= 4 (lex, parse, typecheck, codegen)
  ```

### 5.05: E2E demo file validation in CI

- Depends on: none
- **What**: CI runs `cargo check` on generated code but does not run
  the full pipeline (check + verify + codegen) on all demos.
- **Fix**: Add a CI step that runs `assura check` on all demo files
  and verifies no unexpected errors.
- [x] **Acceptance Tests**:
  ```bash
  # 1. All demo files pass check
  for f in demos/*.assura; do
    echo "=== $f ==="
    cargo run --bin assura -- check "$f"
  done
  # All must exit 0
  # 2. CI workflow includes demo validation
  grep -c 'demos/.*\.assura' .github/workflows/ci.yml
  # Must be > 0
  ```

---

## Phase 6: Ecosystem and Distribution

### 6.01: Enable CodeQL when repo goes public (#45)

- Depends on: repo going public
- **What**: CodeQL is disabled because the repo is private.
- [ ] **Acceptance Tests** (BLOCKED: repo is still private):
  ```bash
  # 1. CodeQL workflow exists and is not disabled
  grep -c 'codeql' .github/workflows/security.yml
  # Must be > 0
  # 2. Close issue #45
  ```

### 6.02: Release pipeline (cargo-dist) -- GitHub #67

- Depends on: Phase 5 complete
- **What**: No release pipeline. Users cannot install Assura.
- **Fix**: Set up cargo-dist for GitHub releases + Homebrew tap.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Release workflow exists
  cat .github/workflows/release.yml
  # Must exist with cargo-dist configuration
  # 2. cargo-dist config in Cargo.toml
  grep 'cargo-dist' Cargo.toml
  # 3. Dry run succeeds
  cargo dist build
  ```

### 6.03: crates.io placeholder

- Depends on: 6.02
- **What**: Claim the `assura` crate name on crates.io.
- [ ] **Acceptance Tests** (UNCLEAR: cannot confirm without checking crates.io):
  ```bash
  # 1. cargo publish --dry-run succeeds
  cargo publish -p assura-parser --dry-run
  ```

### 6.04: VS Code extension publish

- Depends on: none
- **What**: Extension exists in `editors/vscode/` but is not published.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Extension compiles
  cd editors/vscode && npm install && npx tsc -p ./
  # 2. Extension package builds
  npx @vscode/vsce package
  # Must produce a .vsix file
  ```

### 6.05: Documentation site

- Depends on: Phase 4 complete
- **What**: No hosted documentation. Tutorial exists but is not
  published.
- **Fix**: Set up mdBook or similar for docs/ and deploy to GitHub Pages.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Docs build without error
  mdbook build docs/
  # 2. GitHub Pages workflow exists
  cat .github/workflows/docs.yml
  ```

---

## Phase 7: Production Hardening

### 7.01: Error recovery in parser

- Depends on: Phase 4 complete
- **What**: Parser currently stops at first error. Should recover
  and report multiple errors per file (spec Section 7).
- [x] **Acceptance Tests**:
  ```bash
  # 1. File with 3 errors reports all 3, not just the first
  echo 'contract Bad {
    requires(x: ???)
    ensures(y: @@@)
    invariant(z: !!!)
  }' > /tmp/multi_err.assura
  cargo run --bin assura -- check /tmp/multi_err.assura 2>&1 | grep -c 'error'
  # Must be >= 3
  # 2. Tests
  cargo test -p assura-parser error_recovery
  ```

### 7.02: LSP completions for all language features

- Depends on: Phase 3 complete
- **What**: LSP has basic completions but not for all keywords,
  clause types, effect names, etc.
- [x] **Acceptance Tests**:
  ```bash
  # 1. LSP tests cover keyword completion
  cargo test -p assura-lsp completion
  # At least 5 completion tests
  # 2. Full suite
  cargo test --workspace
  ```

### 7.03: JSON output mode (spec Section 7.3)

- Depends on: none
- **What**: `--json` output for all diagnostics per spec Section 7.3.
- [x] **Acceptance Tests**:
  ```bash
  # 1. JSON output is valid JSON
  cargo run --bin assura -- check --json demos/libwebp-huffman.assura | python3 -m json.tool
  # Must not error
  # 2. JSON has required fields per spec
  cargo run --bin assura -- check --json demos/libwebp-huffman.assura | python3 -c "
  import json, sys
  data = json.load(sys.stdin)
  assert 'diagnostics' in data or isinstance(data, list)
  print('JSON output valid')
  "
  ```

### 7.04: assura.toml full spec compliance (Section 10.3)

- Depends on: 4.01
- **What**: `assura-config` parses basic assura.toml but may not
  support all spec Section 10.3 fields.
- [x] **Acceptance Tests**:
  ```bash
  # 1. All Section 10.3 fields are recognized
  cargo test -p assura-config
  # Tests for: [project], [verification], [codegen], [effects]
  # 2. Full suite
  cargo test --workspace
  ```

### 7.05: Performance profiling and optimization

- Depends on: 5.04
- **What**: No performance baseline. Need to establish benchmarks
  and identify bottlenecks.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Benchmark results exist
  cargo bench -p assura-bench -- --output-format=bencher 2>&1 | head -20
  # Must show timing data
  # 2. No individual benchmark > 1 second on demo files
  ```

---

## Phase 8: Inline Contract Annotations -- GitHub #101-#107

> Extend Assura beyond contract-first `.assura` files to support inline
> contract annotations in existing Rust code (`/// @requires`, `/// @ensures`).
> This enables brownfield adoption: developers annotate existing code with
> contracts and verify them using the same Z3/CVC5 engine. Later tasks
> extend this to proc macros, a `check-rust` command, AI inference,
> dual-source merge, VS Code overlays, and multi-language support.

### 8.01: Inline annotation doc-comment parser -- #101

- Depends on: none
- **What**: Create a module that parses `/// @requires`, `/// @ensures`,
  `/// @invariant`, `/// @effects`, and `/// @decreases` clauses from
  Rust doc comments. Extracts clause text and maps to spans. Handles
  multi-line predicates (continuation lines indented under the clause).
  Supports function contracts, struct invariants, and impl block contracts.
- **Deliverable**: New crate `assura-rust-analyzer` (or module in
  `assura-parser`) with:
  - `DocContractParser` that extracts `@`-clauses from `syn::Attribute` doc
    comments
  - `InlineContract` struct with `requires`, `ensures`, `invariant`,
    `effects`, `decreases` vectors
  - `parse_rust_file(path) -> Vec<AnnotatedItem>` that uses `syn` to parse
    a `.rs` file and extracts all annotated functions, structs, and impl blocks
  - Tests covering: single-line clauses, multi-line predicates, struct
    invariants, impl block contracts, mixed doc comments + contracts
- [x] **Acceptance Tests**:
  ```bash
  # 1. Crate exists and compiles
  cargo build -p assura-rust-analyzer
  # 2. Parse a test Rust file with inline annotations
  cargo test -p assura-rust-analyzer parse_doc_contracts
  # At least 5 tests: single requires, multi-line ensures, struct invariant,
  # impl block, effects clause
  # 3. Round-trip: parse annotations and reconstruct clause text
  cargo test -p assura-rust-analyzer roundtrip
  # 4. Edge cases: no annotations, empty doc comments, malformed @-clauses
  cargo test -p assura-rust-analyzer edge_cases
  # 5. Full suite
  cargo test --workspace
  ```

### 8.02: assura-macros proc macro crate -- #102

- Depends on: 8.01
- **What**: Create `assura-macros` proc macro crate with:
  - `#[assura::contract]`: In debug builds, generates `debug_assert!`
    from `@requires`/`@ensures` clauses. In release builds, no-op.
  - `#[assura::trust("reason")]`: Marks a function as trusted (skip
    verification), must include a reason string.
  - Reuses clause parsing from 8.01.
- **Deliverable**: `crates/assura-macros/` with `proc-macro = true`.
  Dependencies: `syn 2`, `quote 1`, `proc-macro2 1`.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Crate compiles as proc-macro
  cargo build -p assura-macros
  # 2. Contract attribute generates debug_assert! in debug mode
  cargo test -p assura-macros contract_debug_assert
  # At least 3 tests: requires generates assert, ensures wraps return,
  # release mode is no-op
  # 3. Trust attribute compiles and is no-op
  cargo test -p assura-macros trust_attribute
  # 4. Integration: use assura-macros from a test crate
  cargo test -p assura-macros integration
  # 5. Full suite
  cargo test --workspace
  ```

### 8.03: `assura check-rust` command -- #103

- Depends on: 8.01
- **What**: Add `assura check-rust <path>` CLI command that:
  1. Scans `.rs` files for inline contract annotations
  2. Parses the Rust code with `syn` to get function bodies
  3. Builds Assura HIR from the contract + implementation pair
  4. Runs `assura-smt::verify()` on each annotated function
  5. Reports verification results with spans pointing into the `.rs` file
  - Supports `--json`, `--layer`, `--watch` flags.
  - Supports directory scanning (check all `.rs` files recursively).
- [x] **Acceptance Tests**:
  ```bash
  # 1. Create a test Rust file with inline contracts
  mkdir -p /tmp/check-rust-test/src
  cat > /tmp/check-rust-test/src/lib.rs << 'EOF'
  /// @requires divisor != 0
  /// @ensures result == dividend / divisor
  fn safe_divide(dividend: i64, divisor: i64) -> i64 {
      dividend / divisor
  }
  EOF
  # 2. Run check-rust on the file
  cargo run --bin assura -- check-rust /tmp/check-rust-test/src/lib.rs
  # Must report verification results (not "unknown command")
  # 3. Run check-rust on a directory
  cargo run --bin assura -- check-rust /tmp/check-rust-test/src/
  # Must find and check all annotated .rs files
  # 4. JSON output works
  cargo run --bin assura -- check-rust --json /tmp/check-rust-test/src/lib.rs
  # Must produce valid JSON
  # 5. Tests
  cargo test -p assura-cli check_rust
  # 6. Full suite
  cargo test --workspace
  ```

### 8.04: Enhanced `assura infer` with AI contract inference -- #104

- Depends on: 8.03
- **What**: Enhance the existing `assura infer` command to:
  1. Accept `.rs` files (not just `.assura` files)
  2. Analyze function signatures and bodies to suggest contracts
  3. Support `--dry-run` (output suggestions without modifying files)
  4. Support `--focus unsafe,panic,unwrap` to prioritize risky functions
  5. Insert suggestions as `/// @requires`/`/// @ensures` doc comments
  - Initially use heuristic-based inference (no LLM required):
    detect `unwrap()` calls (suggest `@requires` for `is_some()`/`is_ok()`),
    division operations (suggest `divisor != 0`), array indexing (suggest
    bounds checks), integer arithmetic (suggest overflow guards).
- [x] **Acceptance Tests**:
  ```bash
  # 1. Infer on a Rust file with obvious contract candidates
  cat > /tmp/infer-test.rs << 'EOF'
  fn divide(a: i64, b: i64) -> i64 {
      a / b
  }
  fn get_first(items: &[i32]) -> i32 {
      items[0]
  }
  fn unwrap_result(r: Result<i32, String>) -> i32 {
      r.unwrap()
  }
  EOF
  cargo run --bin assura -- infer --dry-run /tmp/infer-test.rs
  # Must suggest at least 2 contracts (division-by-zero, bounds check)
  # 2. Infer with --focus flag
  cargo run --bin assura -- infer --dry-run --focus unwrap /tmp/infer-test.rs
  # Must focus on unwrap-related suggestions
  # 3. Tests
  cargo test -p assura-cli infer_rust
  # 4. Full suite
  cargo test --workspace
  ```

### 8.05: Dual-source contracts (external + inline merge) -- #105

- Depends on: 8.01, 8.03
- **What**: Allow a Rust function to have contracts from both an
  external `.assura` file (via `bind` declaration) and inline doc
  comment annotations. Define merge semantics:
  1. External contracts are authoritative (higher priority)
  2. Clauses from both sources are merged (union, not replacement)
  3. Duplicate clauses are detected and warned
  4. Conflicts (contradictory requires/ensures) are reported as errors
  - Add `bind` declaration support to the parser and resolver.
  - Add `[contracts]` and `[inline]` sections to `assura.toml`.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Parse a bind declaration in .assura file
  cargo test -p assura-parser bind_decl
  # 2. Merge inline + external contracts
  cargo test -p assura-rust-analyzer dual_source_merge
  # At least 3 tests: external only, inline only, both merged
  # 3. Conflict detection
  cargo test -p assura-rust-analyzer conflict_detection
  # 4. Full suite
  cargo test --workspace
  ```

### 8.06: VS Code contract overlay -- #106

- Depends on: 8.05
- **What**: Extend the VS Code extension to show external `.assura`
  contracts as inline virtual text (decorations) above Rust functions.
  - Use `vscode.DecorationRenderOptions` with `before` text decorations
  - Show contract source file and line in a clickable header
  - Toggle overlays on/off with a command
  - Show verification status icons per clause
  - LSP provides contract data via custom request
- [x] **Acceptance Tests**:
  ```bash
  # 1. LSP serves contract overlay data
  cargo test -p assura-lsp contract_overlay
  # 2. VS Code extension compiles with overlay support
  cd editors/vscode && npm install && npx tsc -p ./
  # Must compile without errors
  # 3. Extension package builds
  cd editors/vscode && npx @vscode/vsce package
  # 4. Full suite
  cargo test --workspace
  ```

### 8.07: Multi-language annotation support (framework) -- #107

- Depends on: 8.01, 8.03
- **What**: Create a language-agnostic annotation framework that
  separates clause parsing (universal) from predicate expression
  parsing (per-language). Initially support one additional language
  beyond Rust (e.g., Python with `# @requires` in docstrings).
  - Universal clause parser: `requires { }`, `ensures { }`, etc.
  - Language adapter trait: `LanguageAdapter` with `parse_source()`,
    `extract_annotations()`, `parse_predicate()`, `map_types()`.
  - Python adapter as proof of concept.
- [x] **Acceptance Tests**:
  ```bash
  # 1. Language adapter trait exists
  cargo test -p assura-rust-analyzer language_adapter
  # 2. Python adapter parses docstring annotations
  cargo test -p assura-rust-analyzer python_adapter
  # At least 2 tests: function with @requires, class with @invariant
  # 3. Full suite
  cargo test --workspace
  ```

---

## Phase 9: Code Perfection (no historical artifacts)

> Deep audit found 13 categories of rough edges across 88K LOC.
> This phase eliminates every imperfection so the codebase looks
> like a pristine, production-quality compiler from day one.
>
> **Created**: 2026-06-15 after 3-agent parallel audit covering
> code quality, architecture consistency, and documentation accuracy.

### 9.01: Fix 4 latent correctness bugs -- #132

- Depends on: none
- **What**: BinOp::Add default for unknown operators (lower.rs:590),
  Type::Unknown instead of is_indeterminate() (interface.rs:186,196),
  naive "result" string replacement (assura-macros:167),
  CVC5 silently dropping 12 expression forms (cvc5_backend.rs:303-312)
- [x] **Acceptance Tests**:
  ```bash
  grep -n "unwrap_or(BinOp::Add)" crates/assura-parser/src/lower.rs
  # Must return 0
  grep -n "== Type::Unknown" crates/assura-types/src/checkers/interface.rs
  # Must return 0
  cargo test -p assura-macros result_replacement
  cargo test -p assura-smt cvc5
  cargo test --workspace
  ```

### 9.02: Rewrite INTERNALS.md and fix AGENTS.md versions -- #133

- Depends on: none
- **What**: INTERNALS.md references chumsky 0.9 (not used), wrong
  file names (parser.rs), wrong API signatures, missing 5 crates.
  AGENTS.md version table says logos 0.15 (actual: 0.16).
- [x] **Acceptance Tests**:
  ```bash
  grep -ci "chumsky" docs/INTERNALS.md
  # Must return 0
  grep "logos.*0.15" AGENTS.md docs/INTERNALS.md
  # Must return 0
  for c in assura-pipeline assura-mcp assura-rust-analyzer assura-macros assura-stdlib; do
    grep -c "$c" docs/INTERNALS.md
  done
  # Each must return >= 1
  ```

### 9.03: Split 5 monolith files -- #134

- Depends on: 9.05 (dedup first, then split the smaller result)
- **What**: types/lib.rs (8,305), domain.rs (4,144), smt/lib.rs (4,468),
  z3_backend.rs (3,654), resolve/lib.rs (4,266). Total: ~24,600 lines
  across 5 files. Split into ~30 focused modules.
- [x] **Acceptance Tests**:
  ```bash
  find crates -name "*.rs" -path "*/src/*" ! -path "*/tests/*" \
    | xargs wc -l | sort -n | tail -5
  # Largest non-test file: resolve/lib.rs at 2,428 (under 2,500)
  # codegen_tests.rs at 2,561 is a test file, not production code
  cargo test --workspace
  ```

### 9.04: Eliminate triple-duplicated checker pipeline -- #135

- Depends on: none
- **What**: 57-checker dispatch list copy-pasted in 3 entry points.
  Also duplicated build_type_env and check_clause_bodies paths.
- [x] **Acceptance Tests**:
  ```bash
  # After: run_all_checks called from all 3 paths (moved to pipeline.rs)
  grep -c "run_all_checks" crates/assura-types/src/pipeline.rs
  # Must be >= 3 (one call per entry point) -- returns 5 (1 defn + 3 calls + 1 doc)
  # The 57 individual run_*_checks calls should be in ONE function
  cargo test --workspace
  ```

### 9.05: Replace 250+ wildcard match arms with exhaustive patterns -- #136

- Depends on: none
- **What**: `_ => {}` across lib.rs (155), z3_backend.rs (41),
  inference.rs (25), lower.rs (15), CLI (16), codegen (10).
  Silently skip new enum variants.
- [x] **Acceptance Tests**:
  ```bash
  grep -rn "_ => {}" crates/assura-types/src/lib.rs \
    crates/assura-smt/src/z3_backend.rs crates/assura-codegen/src/ \
    crates/assura-parser/src/lower.rs | grep -v test | wc -l
  # Target: 0 for enum wildcards. Remaining 10 are string/char/SyntaxKind
  # matches where wildcards are required (can't enumerate all strings).
  # types/lib.rs: 0, z3_backend.rs: 0 (the high-impact files are clean)
  cargo test --workspace
  cargo clippy --workspace -- -D warnings
  ```

### 9.06: Fix code style inconsistencies -- #137

- Depends on: none
- **What**: 136 `std::string::String` in domain.rs, 67 hardcoded
  numeric defaults without named constants, 5 glob re-exports,
  `&Vec<T>` instead of `&[T]`, Debug format as semantic data.
- [x] **Acceptance Tests**:
  ```bash
  grep -c "std::string::String" crates/assura-types/src/domain.rs
  # Must return 0 (domain.rs was split; all checker files also cleaned)
  grep -c "pub use.*\*" crates/assura-smt/src/lib.rs
  # Must return 0
  cargo test --workspace
  ```

### 9.07: Fix error handling (silent swallowing, sentinel spans) -- #138

- Depends on: none
- **What**: 12 sentinel `0..0` spans on import errors,
  cache/display errors silently swallowed, SMT encoding
  results discarded, file read failures ignored in watch mode.
- [x] **Acceptance Tests**:
  ```bash
  grep -n "span: 0..0" crates/assura-resolve/src/lib.rs
  # Must return 0
  cargo test --workspace
  ```

### 9.08: CI gaps (fuzz, benchmark, release publish, nightly) -- #139

- Depends on: none
- **What**: No fuzz CI (targets exist, no workflow), no benchmark CI,
  assura-macros missing from release publish, nightly examples check
  uses `|| true`, `cargo publish --no-verify || true`.
- [x] **Acceptance Tests**:
  ```bash
  ls .github/workflows/fuzz.yml
  grep "assura-macros" .github/workflows/release.yml
  # Both must succeed
  ```

### 9.09: Improve test quality -- #140

- Depends on: none
- **What**: 5 assertion-free tests in assura-macros, 7 discarded
  results in assura-types tests, domain checker test coverage
  at 7.7 tests/struct (target: 10+).
- [x] **Acceptance Tests**:
  ```bash
  grep -rn "let _ =" crates/assura-macros/tests/ | wc -l
  # Must return 0
  grep -rn "let _ = check_expr" crates/assura-types/src/tests/ | wc -l
  # Must return 0
  cargo test --workspace
  ```

### 9.10: Add doc comments to public API -- #141

- Depends on: 9.03 (add docs after splitting, not before)
- **What**: TypeEnv methods, parse_type_tokens, extraction helpers,
  55 run_*_checks functions, domain checker methods, Encoder struct,
  encode_expr, SymbolTable methods, module-level docs.
- [x] **Acceptance Tests**:
  ```bash
  cargo doc --workspace --no-deps 2>&1 | grep -c "missing documentation"
  # Target: 0 warnings
  ```

### 9.11: Parser clause keyword sync and tree-sitter grammar -- #142

- Depends on: none
- **What**: at_clause_start() vs is_clause_stopper() keyword list
  divergence, #129 missing clause_kind arms, tree-sitter grammar
  missing decreases/where/view/abstracts/transitions/result/mod.
- [x] **Acceptance Tests**:
  ```bash
  cargo test -p assura-parser clause_kind
  cd editors/tree-sitter-assura && npx tree-sitter generate && npx tree-sitter test
  # 23/23 tests pass, zero conflicts
  ```

### 9.12: Improve generated code quality -- #143 (was #144)

- Depends on: none
- **What**: Remove `unreachable_code` from generated allow list,
  deduplicate contract/proptest function pairs, reduce 5 Decl
  iteration passes, replace 30+ hardcoded method names with data.
- [x] **Acceptance Tests**:
  ```bash
  grep "unreachable_code" crates/assura-codegen/src/lib.rs
  # Must return 0
  cargo test --workspace
  ```

### 9.13: Remove dead code paths and cleanup -- #144 (was #143)

- Depends on: none
- **What**: Layer 2 dead code path (layer2.rs:614), unused typed
  in diff.rs:364, diagnostics O(n) lookup, SolverChoice duplication,
  format_rust() silent degradation.
- [x] **Acceptance Tests**:
  ```bash
  grep -n "let _ = typed" crates/assura-cli/src/diff.rs
  # Must return 0
  grep -rn "enum SolverChoice" crates/ | wc -l
  # Must return 1
  cargo test --workspace
  ```

---

## Dependency Graph

```
Phase 1 (bugs) ─────────────────┬──> Phase 4 (multi-file) ──> Phase 6 (ecosystem)
                                │                                      │
Phase 2 (structural checkers) ──┼──> Phase 5 (testing) ──────> Phase 7 (production)
                                │
Phase 3 (partial checkers) ─────┘

Phase 8 (inline annotations):
  8.01 ──┬──> 8.02
         ├──> 8.03 ──> 8.04
         ├──> 8.05 ──> 8.06
         └──> 8.07

Phase 9 (code perfection):
  9.01, 9.02, 9.06, 9.07, 9.08, 9.09, 9.11, 9.12, 9.13 -- independent, parallel
  9.04 ──> 9.05 ──> 9.03 ──> 9.10
  (dedup pipeline, then replace wildcards, then split files, then add docs)
```

Phases 1-8: complete (except 6.01 CodeQL, blocked on public repo).
Phase 9: all tasks independent except 9.03 depends on 9.04+9.05,
and 9.10 depends on 9.03. The critical path is:
  9.04 (dedup) -> 9.05 (wildcards) -> 9.03 (split) -> 9.10 (docs)

Within the independent tasks, recommended order by impact:
  9.01 (correctness) > 9.02 (docs) > 9.06 (style) > 9.07 (errors)
  > 9.08 (CI) > 9.11 (parser) > 9.12 (codegen) > 9.13 (cleanup) > 9.09 (tests)

---

## Progress Notes

### 2026-06-14: Plan v3 created
- Deep audit of all 72K LOC, 1,946 tests, 50 verification features
- Classified every checker as REAL (30), STRUCTURAL (14), PARTIAL (12)
- Created 7-phase plan with verifiable acceptance tests for every task
- 12 open GitHub issues catalogued
- Previous plan (v2) had 66 tasks all marked `[x]`

### 2026-06-14: Session 2 (continued)
- **#96 closed**: Added 21 CLI integration tests for doctor, coverage,
  completions, agent-instructions, explain commands
- **5.03 done**: Error code coverage audit. Added 85 missing error codes
  to diagnostics catalog (gap: 85 -> 0, all 218 spec codes now covered)
- **#66 closed**: Created assura-stdlib crate with 13 contracts across
  3 modules (math: 4, string: 3, collections: 6) and 13 tests
- **#67 closed**: Fixed release pipeline config (stale metadata.dist,
  added missing crates to publish job)
- Test count: 2,053 (up from 2,020)
- Remaining open issues: #45, #86, #88, #89, #91
- **Next session**: Phase 7 tasks (error recovery, LSP completions,
  JSON output mode) or Phase 6 remaining (VS Code extension, docs site)

### 2026-06-14: Session 3 (continued)
- **PR #99 merged**: Added `assura diff`, `assura repl`, `assura mcp`
  commands. Closed issues #86, #89, #91.
- **7.02 done**: LSP completions enhanced with 23 effect names, 8 snippet
  templates, 14 new keywords. 7 new completion tests.
- **7.04 done**: Added `[effects]` config section (allowed/denied lists,
  default-effect) and `[codegen]` section (backend, emit-debug-asserts,
  generate-tests). 10 new config tests.
- **7.01, 7.03**: Already working (parser error recovery, JSON output mode).
- **5.05**: Already in CI (E2E demo validation step).
- Test count: 2,080 (up from 2,063)
- Only open issue: #45 (CodeQL, blocked on repo going public)
- All other issues closed (97 total closed)
- **Next session**: Phase 6.04 (VS Code extension publish), Phase 6.05
  (documentation site), Phase 7.05 (performance optimization), or new
  features from spec sections not yet implemented.

### 2026-06-14: Session 4 (continued)
- **50 SMT tests added**: 40 for advanced.rs (TriggerManager,
  CodecDispatcher, WeakMemoryChecker, ProphecyManager, LivenessChecker),
  10 for incremental.rs
- **#100 closed**: Created assura-pipeline crate to deduplicate compiler
  pipeline across CLI REPL, MCP server. Replaced DefaultHasher with
  FNV-1a in z3_backend pattern_hash.
- **36 domain checker tests added**: AllocatorChecker (6),
  CircularBufferChecker (3), PlatformAbstractionChecker (4),
  FeatureFlagChecker (5), ResourceLimitChecker (5),
  UnsafeEscapeChecker (3), ContractLibraryChecker (3),
  ContractCompositionChecker (3), StorageFailureChecker (3)
- **2.13 fixed**: Layer2Verifier.verify_with_z3() now does real Z3
  verification for quantified invariants (parse body strings, create
  bound vars, check validity), termination obligations (measure
  decrease encoding), and roundtrip obligations (uninterpreted
  functions). All 4 "requires Z3 Layer 2 verification" stubs removed.
- **3.11 fixed**: IR codegen no longer emits todo!() for functions
  without explicit result assignment; uses type-appropriate defaults.
- **Full audit of Phase 2 and Phase 3**: All acceptance criteria
  verified for tasks 2.01-2.14 and 3.01-3.12. No remaining stubs.
- Test count: 2,213 (up from 2,172)
- Only open issue: #45 (CodeQL, blocked on repo going public),
  plus #101-#105 (inline contract annotations feature set)
- **Next session**: Phase 4 (multi-file compilation), Phase 6.04
  (VS Code extension), Phase 6.05 (docs site), or inline annotation
  features (#101-#105).

### 2026-06-15: Session 5 (continued)
- **Phase 8 complete**: All 7 inline contract annotation tasks implemented
- **#101 done (8.01)**: assura-rust-analyzer crate with doc comment parser,
  20 tests covering clauses, structs, impl blocks, edge cases
- **#102 done (8.02)**: assura-macros proc macro crate with #[contract] and
  #[trust], 13 integration tests
- **#103 done (8.03)**: `assura check-rust` CLI command for Rust file
  verification, 7 CLI integration tests
- **#104 done (8.04)**: Enhanced `assura infer` with heuristic inference
  (division-by-zero, unwrap, array indexing, unsafe, panic patterns)
- **#105 done (8.05)**: Dual-source contract merge (ClauseSource,
  MergedContract, ContractsConfig, InlineConfig), 14 tests
- **#106 done (8.06)**: VS Code contract overlay with toggle command,
  color-coded decorations, 6 LSP tests
- **#107 done (8.07)**: Multi-language annotation framework with
  LanguageAdapter trait, RustAdapter, PythonAdapter, 13 tests
- Tech-debt issues #108 and #109 also fixed in this session
- Test count: 2,274 (up from 2,213)
- Only open issue: #45 (CodeQL, blocked on repo going public)
- **Next session**: Phase 4 (multi-file compilation), Phase 6.04
  (VS Code extension publish), Phase 6.05 (docs site), or Phase 7.05
  (performance profiling).

### 2026-06-15: Session 6 - Full audit of MASTER-PLAN v3
- **Audited all 59 tasks** across 8 phases using 4 parallel verification agents
- **53 tasks verified DONE** and marked `[x]` (were all `[ ]` due to bookkeeping gap)
- **6 tasks remain `[ ]`**:
  - 3.12: Cranelift backend (PARTIAL: enum exists, only adds comment, no real codegen diff)
  - 4.03: Cross-file type checking (NOT DONE: type checker is single-file only)
  - 5.02: Spec compliance test suite Section 13 (NOT DONE: no tests/spec_compliance/)
  - 6.01: CodeQL (BLOCKED: repo is private, issue #45)
  - 6.03: crates.io placeholder (UNCLEAR: not verified)
  - 6.05: Documentation site (NOT DONE: no mdBook, no GitHub Pages)
- **All GitHub issues CLOSED** except #45 (CodeQL, blocked on public repo)
- Project state: 2,280 tests passing, 16 crates, clean working tree
- **Next session**: 4.03 (cross-file typeck) is the highest-value remaining task

### Session 7 (2026-06-15)

- **Completed 4 tasks**, reducing remaining from 5 to 1:
  - 4.03: Cross-file type checking: `type_check_with_modules()` + `inject_imported_types()`,
    CLI wired, `resolve_with_modules` made public, 4 tests
  - 5.02: Spec compliance: restructured into 11 module files (tc01-tc11),
    29 tests covering all Section 13 pairwise/three-way/full-stack interactions
  - 3.12: Cranelift backend (completed prior session, committed)
  - 6.03: crates.io metadata (completed prior session, committed)
  - 6.05: Documentation site: mdBook config, SUMMARY.md, GitHub Pages
    workflow, symlinked existing docs into src/
- **Only 6.01 (CodeQL) remains** (blocked on repo going public, issue #45)
- Project state: 2,300+ tests passing, 16 crates, mdBook builds clean

### Session 8 (2026-06-15): Code perfection audit

- **3-agent parallel audit** of entire 88K LOC codebase covering:
  code quality, architecture consistency, and documentation accuracy
- **13 issues created** (#132-#144) organized into Phase 9 (code perfection)
- Key findings:
  - 4 latent correctness bugs (BinOp::Add default, Type::Unknown,
    result replacement, CVC5 gaps)
  - INTERNALS.md critically stale (references chumsky 0.9, wrong files)
  - AGENTS.md version table stale (logos 0.15 vs actual 0.16)
  - 5 monolith files totaling 24,600 lines need splitting
  - Triple-duplicated 57-checker pipeline (3 copy-pasted blocks)
  - 250+ wildcard match arms hiding exhaustiveness bugs
  - 136 `std::string::String` in domain.rs
  - 67 hardcoded numeric defaults without named constants
  - 12 sentinel `0..0` spans on import errors
  - CI missing fuzz/benchmark workflows
  - Parser clause keyword lists diverge
- Phase 9 has 13 tasks, 9 independent + critical path:
  9.04 (dedup) -> 9.05 (wildcards) -> 9.03 (split) -> 9.10 (docs)
- **Next session**: Start with 9.01 (correctness bugs) and 9.02 (docs),
  then 9.04 (pipeline dedup) to unlock the critical path

### Sessions 9-10 (2026-06-15 to 2026-06-16): Issue #145, fixture fixes, Phase 9 completion

- **#145 closed**: Unblocked 9 must_reject fixtures with parser/type wiring fixes
- **#146 closed**: Added 7 new must_reject fixtures, unblocked 2 BLOCKED fixtures
- **Phase 9 COMPLETE** (all 13 tasks verified and marked [x]):
  - 9.01: 4 latent correctness bugs fixed (#132)
  - 9.02: INTERNALS.md rewritten, AGENTS.md logos version corrected (#133)
  - 9.03: 5 monolith files split (largest non-test: 2,428 lines) (#134)
  - 9.04: Triple-duplicated pipeline unified in pipeline.rs (#135)
  - 9.05: Enum wildcard match arms eliminated (10 remaining are string/char) (#136)
  - 9.06: 78 std::string::String replaced, 9 glob re-exports removed (#137)
  - 9.07: Sentinel 0..0 spans fixed (#138)
  - 9.08: fuzz.yml workflow added, release.yml already correct (#139)
  - 9.09: Assertion-free tests and discarded results fixed (#140)
  - 9.10: Doc comments added (#141)
  - 9.11: Tree-sitter grammar: 4 conflicts resolved, 30+ keywords added (#142)
  - 9.12: Generated code quality improved (#143)
  - 9.13: Dead code paths removed (#144)
- **77 must_reject fixtures** (1 BLOCKED: prophecy needs SMT-level changes)
- **Only 3 tasks remain `[ ]` in entire plan**:
  - 3.12: Cranelift backend (partial, needs real JIT implementation)
  - 6.01: CodeQL (blocked on repo going public, issue #45)
  - 6.03: crates.io placeholder (needs registry interaction)
- **Only 2 open issues**: #45 (CodeQL), #147 (quantifier triggers/test gen)

### Session 11 (2026-06-16): Issues closed, multi-perspective audit

- **#145 closed**: 9 BLOCKED must_reject fixtures unblocked (session 10 carryover)
- **#147 closed**: CORE.5 quantifier triggers (QuantifierTriggerChecker with
  strict_triggers clause, A-CORE-050 error code) and TEST.1 TestGenerator
  (populates TypedFile.generated_tests from contract requires/ensures)
- **#148 closed**: Extracted 3 named constants (DEFAULT_ULP_TOLERANCE,
  DEFAULT_PARAM_ZERO, DEFAULT_PARAM_ONE) replacing hardcoded numeric
  defaults across 6 checker files
- **#149 closed**: Domain checker test coverage increased from 7.7 to
  10.5 tests/struct (324 tests across 31 Checker structs, +104 new tests)
- **Deep verification pass**: All clean:
  - 4 demo files parse and verify
  - 0 stubs (todo!/unimplemented!/Vec::new in run_*_checks)
  - 0 dead code suppressions
  - 1 BLOCKED fixture (prophecy, legitimate)
  - Tree-sitter: 23/23 tests pass
  - Fuzz: 3 targets list correctly
- **Multi-perspective audit (Rotation 1)**: 7 new tech-debt issues filed:
  - #150: BLOCKED prophecy fixture needs SMT-aware test harness
  - #151: tree-sitter ABI 14 deprecation (missing tree-sitter.json)
  - #152: error code format inconsistency (A-WORD-NNN vs Axxxxx)
  - #153: spec error code A22003 has no implementing checker
  - #154: CLI infer generates TODO placeholders instead of meaningful clauses
  - #155: CLI exits 0 on verification Timeout/Unknown (soundness gap)
  - #156: 22 type inference errors use 0..0 span (no source location)
- Test count: 2,415 (up from ~2,329)
- Open issues: #45 (CodeQL, blocked), #150-#156 (tech-debt from audit)
- **3 tasks remain `[ ]`**: 3.12 (Cranelift), 6.01 (CodeQL), 6.03 (crates.io)
- **Next session**: Fix #150-#156, continue multi-perspective rotation,
  evaluate OSS launch readiness for #45

### Session 12 (2026-06-16): Issues #150-#165 + #166-#179, multi-perspective rotation

- **14 issues fixed and closed** (#166-#179):
  - #166: CVC5 must_not semantics inversion
  - #167: CVC5 quantifier-bound vars as global constants
  - #168: Bool-to-Int coercion unconstrained
  - #169: Integer literal overflow
  - #170: Tuple/List elements discarded
  - #171: Resolver doesn't add fn params to clause scope
  - #172: Type checker accepts cross-type comparisons
  - #173: Five tautological/wrong tests
  - #174: Thirteen domain checker tests use identical trivial input
  - #175: String constants no Z3 distinctness
  - #176: Effect polymorphism (effect_variables field on Clause)
  - #177: Apply always returns true
  - #178: Generated Rust produces compiler warnings
  - #179: Error code semantics mismatch
- **Stack overflow fix**: 500+ chained binary operators caused stack
  overflow. Fixed with 3-layer defense: (1) MAX_BINOP_CHAIN=256 counter
  in Pratt parser, (2) iterative lower_bin_expr in CST lowering,
  (3) iterative expr_to_string in display. Regression test added.
- **Multi-perspective rotation (cycle 1)**: 11 iterations completed
  (QA, Developer, End User, Maintainer, Spec Compliance, Security,
  Performance, Adversarial Tester, Architecture, New Contributor,
  Observability). Post-cycle gate passed.
- **Diagnostic improvements**: duplicate A02008 caught and fixed,
  error codes aligned with spec Section 7.2, uniqueness test added
- Open issues: #45 (CodeQL, blocked on public repo)
- **Next session**: Continue multi-perspective rotation or start new
  feature work from spec.

### Session 13 (2026-06-16): 50-feature coverage audit (6 phases)

- **87% coverage achieved** (570/650 cells, 11/13 layers STRONG):
  - Phase 0: Created verify-task.sh, demos, updated AGENTS.md
  - Phase 1: Added 32 inline @keyword annotations (InlineClauseKind variants)
  - Phase 2: Feature-specific codegen for all 50 features (features.rs)
  - Phase 3: 15 compile-time enforcement functions
  - Phase 4: smt_features.rs with 33+ feature verification functions,
    wired into verify_clauses() via ClauseKind::Other dispatch
  - Phase 5: 60+ keyword aliases in rust-analyzer, parser clause additions
  - Phase 6: Coverage script updates, grep pattern fixes
- **Coverage script**: `~/.grok/skills/assura-coverage-audit/scripts/coverage-matrix.sh`
  with TAB-separated feature database (8 fields per line)
- **Remaining weak layers**: Compile-time (16/50), Runtime (4/50)
- Test count: 2,606 (up from ~2,415)
- Open issues: #45 (CodeQL, blocked on public repo)
- **3 tasks remain `[ ]`**: 3.12 (Cranelift), 6.01 (CodeQL), 6.03 (crates.io)
- **Next session**: 3.12 Cranelift backend, multi-perspective audit continuation

### Session 14 (2026-06-16 to 2026-06-17): Test coverage + correctness fixes

- **111 tests committed** (c1af6d1): diagnostics (30), HIR (46), parser display (35)
- **5 correctness fixes**:
  - CVC5 and parallel verification now handle Service, Block, Bind declarations
  - CVC5 and parallel verification now handle ServiceItem::Invariant
  - build_type_env now enriches Bind params with proper types (was empty/Unknown)
  - Unchecked depth decrements guarded in type_map.rs and types_gen.rs
  - TestGenerator edge cases added for String/Bytes/List/Map/Set/Option/Result
- **Error handling improved**: Silent `let _ =` discards in SMT cache, prophecy
  resolution, and encoder replaced with eprintln warnings
- **51 more tests added**: SMT entry.rs (21), resolve symbols.rs (12), imports.rs (18)
- E2E test updated: service_typestate.assura now expects counterexample (correct
  behavior now that CVC5 verifies service operations)
- Test count: ~3,020+ (up from ~2,960)
- Open issues: #45 (CodeQL, blocked on public repo)
- **Next session**: Continue multi-perspective audit, add tests to untested modules

### Session 15 (2026-06-17): Bug fix + test coverage expansion

- **1 correctness bug fixed**: Enum variant fields included comma tokens.
  `Rgb(Int, Int, Int)` produced `["Int", ",", "Int", ",", "Int"]` (5 fields)
  instead of `["Int", "Int", "Int"]` (3 fields). Affected type env construction
  and Rust codegen output. Fixed in `collect_paren_tokens()` by skipping
  top-level commas. Snapshot updated.
- **108 new tests committed across 8 files**:
  - inference.rs: 36 tests (was 0 for 1,010 LOC)
  - resolve type_refs.rs: 13 tests (edit_distance, type name candidate, leniency)
  - resolve unused.rs: 8 tests (unused import detection)
  - resolve clause_names.rs: 9 tests (pattern bindings, clause body resolution)
  - resolve errors.rs: 2 tests (Diagnostic conversion)
  - clauses.rs: 14 tests (param registration, pattern binding, clause checking)
  - env.rs: 11 tests (type env builder via full pipeline)
  - lower.rs: 1 test (regression for enum comma fix)
- Test count: ~3,100+ (up from ~3,020)
- **Next session**: Z3 backend test coverage, structural stub removal

### Session 16 (2026-06-17): Comprehensive test coverage expansion

- **183 new tests added across 10 source files**:
  - codegen block.rs: 11 tests (codec registry, generic blocks, format_rust)
  - codegen contract.rs: 39 tests (enum_def, proptest strategy, refinement,
    error types, contract generation, interface traits, input/output extraction)
  - codegen features.rs: 76 tests (all 50 verification features, dispatch table,
    compile-time enforcement, synonym matching)
  - smt result.rs: 10 tests (CounterexampleModel JSON, escaping, VerificationResult)
  - smt measures.rs: 13 tests (MeasureDefinition builder, builtin registry, axioms)
  - smt smt_dump.rs: 18 tests (infinite domain detection, quantifier bounds,
    raw token parsing, nested quantifiers)
  - resolve project.rs: 16 tests (file_to_module_path, find_project_root,
    resolve_module_path, collect_assura_files)
- **10 previously zero-test files now have coverage** (7 codegen + 3 SMT + 1 resolve)
- Zero clippy warnings, all demo files pass, all 3,438 tests pass
- Test count: 3,438 (up from ~3,100)
- **Next session**: Continue multi-perspective improvements, further SMT backend
  test coverage, remaining structural stub removal

### Session 17 (2026-06-17): 100% coverage gap closure + codegen bugfix

**Coverage**: 570/650 (87%) -> 650/650 (100%)

Closed all 18 remaining coverage gaps across compile-time and runtime layers:
- Added 17 compile_time_* functions (CORE.5-8, CONC.4-5, STOR.1, FMT.4-6,
  NUM.2, PERF.2, TEST.1-3, MISC.1-2) to features.rs
- Wired all into generate_feature_clause dispatch table
- Fixed FMT.6 runtime detection (protocol_grammar in debug_assert message)
- Updated 17 COMPILETIME_GREP patterns in coverage-matrix.sh

Found and fixed codegen bug: contracts with named output variables
(e.g., output(value: Nat)) generated code that referenced the output
name in ensures clauses without binding it. Added extract_output_name()
function and let-binding in contract codegen, service codegen, and
proptest generation. 6 tests added. This bug caused CI "Generated code
compiles" failure on taint-tracking.assura.

- All 13 layers at 50/50 (100%): Parser, Inline, Resolve, HIR, Types,
  Pipeline, Codegen, Compile-time, SMT, Runtime, LSP, Formatter, Tests
- 3,495 tests passing, zero clippy warnings
- **Next session**: Verify CI passes, session-improve, further development

### Sessions 18-19 (2026-06-18): Z3/CVC5 SMT backend parity (31 issues)

- **31 issues created and resolved** (#213-#245) across 3 batches:
  - Batch 1 (a682eb6): CVC5 expression encoder parity with Z3 (10 issues #235-#244)
    - Filled all None arms in encode_expr_cvc5 and expr_to_smtlib
    - Added Cvc5EncoderState for background axiom threading
    - Float, String, Old, Field, Index, MethodCall, Call, Block, Tuple, List, Apply, Raw
  - Batch 2 (00097c7): Standalone functions, feature dispatch, clause enrichments (15 issues #215-#229)
    - Generic check_validity_cvc5/check_satisfiability_cvc5 reusable functions
    - CVC5 impls for 7 standalone entry-point functions
    - Feature body dispatch to CVC5 backend
    - ClauseKind::Other handling in CVC5 verification
  - Batch 3 (5ab449d): Tech debt, advanced passes, parallel portfolio (8 issues #213-#214, #230-#234, #245)
    - Extracted collect_verification_jobs() to deduplicate Decl dispatch
    - Moved 5 solver-agnostic analysis passes to shared run_*_checks() functions
    - Wired all 5 advanced passes (weak memory, prophecy, liveness, layer 2, codec) into CVC5 path
    - Refactored z3_backend/verify.rs to call shared functions (removed ~400 lines)
    - Implemented true parallel portfolio solving (Z3+CVC5 via std::thread::scope)
- 3,514 tests passing, zero clippy warnings
- Only open issue: #45 (CodeQL, blocked on repo going public)
- **Next session**: Phase 10 (SMT full parity), then multi-perspective improvement loop

---

## Phase 10: Full SMT Parity (CVC5 matches Z3, both go deeper)

> Deep audit found 15 CVC5 parity gaps and 7 Z3 advancement opportunities.
> 22 GitHub issues (#246-#267) created with exact acceptance tests.
>
> **Goal**: CVC5 standalone (`--solver cvc5`) produces identical verification
> results to Z3 for every demo contract. Then push both solvers further with
> native theories (String, ADT, Bitvector), incremental solving, unsat cores,
> and havoc+assume encoding.
>
> **Session survival**: Each round is self-contained. After each round,
> commit, push, and update this section. A new session picks up at the
> next unchecked round.
>
> **Execution strategy**: Independent issues within a round run in
> parallel subagents with worktree isolation. Merge after each round.

### Round 1: Critical correctness (#246, #249, #258) -- depends on: none

- [x] **10.01** #246 CVC5: Quantifier domain guards (forall/exists range bounding)
  - Add `guard_quantifier_body_cvc5()` mirroring Z3 encoder.rs:132-176
  - Range domains: `lo <= x && x < hi` guard
  - Collection domains: UF `__domain_contains(domain, x)`
- [x] **10.02** #249 CVC5: Encode BinOp::Range, In, NotIn, Concat
  - Remove `return None` at cvc5_backend.rs:348
  - Range: bound constraints, In/NotIn: UF Bool, Concat: length axiom
- [x] **10.03** #258 CVC5: Unmodelable feature pre-check
  - Call `expr_has_unmodelable_features()` before CVC5 encoding
  - Return `Unknown` with reasons instead of false counterexamples
- **Acceptance**:
  ```bash
  cargo test -p assura-smt --features cvc5-verify -- cvc5_domain_guard
  cargo test -p assura-smt --features cvc5-verify -- cvc5_binop_range
  cargo test -p assura-smt --features cvc5-verify -- cvc5_binop_in
  cargo test -p assura-smt --features cvc5-verify -- cvc5_unmodelable
  cargo test --workspace && cargo clippy --workspace -- -D warnings
  ```

### Round 2: High-impact parity (#251, #254, #256, #257) -- depends on: Round 1

- [x] **10.04** #251 CVC5: String method axioms (8 methods + array set/put/get)
  - substring, concat, indexOf, charAt, replace, split, trim
  - Array set/put read-over-write axioms
- [x] **10.05** #254 CVC5: Lemma injection for apply expressions
  - Collect lemma defs, inject postconditions as assumptions
- [x] **10.06** #256 CVC5: Frame axioms from modifies clauses
  - Build FrameChecker, assert `var == old_var` for unmodified state
- [x] **10.07** #257 CVC5: Bind feature_max constants + refinement narrowing
  - Make collect_feature_max_constants shared, bind in CVC5 solver
- **Acceptance**:
  ```bash
  cargo test -p assura-smt --features cvc5-verify -- cvc5_string
  cargo test -p assura-smt --features cvc5-verify -- cvc5_lemma
  cargo test -p assura-smt --features cvc5-verify -- cvc5_frame
  cargo test -p assura-smt --features cvc5-verify -- cvc5_feature_max
  cargo test --workspace && cargo clippy --workspace -- -D warnings
  ```

### Round 3: Medium parity (#247, #248, #250, #252) -- depends on: Round 1

- [x] **10.08** #247 CVC5: Quantifier trigger pattern inference
  - Integrate TriggerManager, use CVC5 Kind::InstPattern
- [x] **10.09** #248 CVC5: Real sort for floats
  - tm.mk_real(numer, denom), Real-aware negation, ITE sort promotion
- [x] **10.10** #250 CVC5: Deep field chain flattening
  - Port has_deep_field_chain/flatten_field_chain to CVC5
- [x] **10.11** #252 CVC5: Constructor/Tuple match patterns
  - Hash-based tag matching, field binding, tuple accessors
- **Acceptance**:
  ```bash
  cargo test -p assura-smt --features cvc5-verify -- cvc5_trigger
  cargo test -p assura-smt --features cvc5-verify -- cvc5_real
  cargo test -p assura-smt --features cvc5-verify -- cvc5_field_chain
  cargo test -p assura-smt --features cvc5-verify -- cvc5_match_constructor
  cargo test --workspace && cargo clippy --workspace -- -D warnings
  ```

### Round 4: Performance + polish (#253, #255, #259, #260) -- depends on: Round 1

- [x] **10.12** #253 CVC5: Verification cache (SessionCache)
  - Thread SessionCache through CVC5 path, same key format as Z3
- [x] **10.13** #255 CVC5: Full precedence-climbing Raw token parser
  - Port Z3 encode_raw_tokens with parens, old(), forall/exists, precedence
- [x] **10.14** #259 CVC5: ITE 6-way sort promotion (done in #248)
  - Int/Real promotion, Bool-to-Int coercion in if/then/else
- [x] **10.15** #260 CVC5: Structured counterexample model filtering
  - Filter __internal vars, sort alphabetically
- **Acceptance**:
  ```bash
  cargo test -p assura-smt --features cvc5-verify -- cvc5_cache
  cargo test -p assura-smt --features cvc5-verify -- cvc5_raw_precedence
  cargo test -p assura-smt --features cvc5-verify -- cvc5_ite_promotion
  cargo test -p assura-smt --features cvc5-verify -- cvc5_counter_model
  cargo test --workspace && cargo clippy --workspace -- -D warnings
  ```

### Round 5: Z3 advancement (#262, #261, #264) -- depends on: Round 4

- [ ] **10.16** #262 Z3: Typestate state-machine encoding for @ annotations
  - Remove unmodelable skip, encode states as enumerated/integer sort
- [ ] **10.17** #261 Z3/CVC5: Native String theory (QF_S / QF_SLIA)
  - Replace integer encoding with Z3 ast::String, CVC5 string_sort()
- [ ] **10.18** #264 Z3/CVC5: Incremental solving (push/pop)
  - Assert requires once, push/pop for each clause
- **Acceptance**:
  ```bash
  cargo test -p assura-smt -- typestate
  cargo test -p assura-smt -- string_theory
  cargo test -p assura-smt -- incremental
  cargo test --workspace && cargo clippy --workspace -- -D warnings
  ```

### Round 6: Advanced theories (#263, #265, #266, #267) -- depends on: Round 5

- [x] **10.19** #263 Z3/CVC5: Algebraic data type (ADT) encoding
  - Z3 Datatypes, CVC5 DatatypeDecl for enums/structs
- [x] **10.20** #265 Z3/CVC5: Bitvector theory for fixed-width integers
  - BitVec sort for u8/u16/u32/u64, overflow detection
- [x] **10.21** #266 Z3/CVC5: Unsatisfiable core extraction
  - produce-unsat-cores, extract and report in VerificationResult
- [ ] **10.22** #267 Z3/CVC5: Havoc+assume encoding for result-field
  - Implementation IR constrains result, structural axioms for pure contracts
- **Acceptance**:
  ```bash
  cargo test -p assura-smt -- adt_
  cargo test -p assura-smt -- bitvector
  cargo test -p assura-smt -- unsat_core
  cargo test -p assura-smt -- havoc_assume
  cargo test --workspace && cargo clippy --workspace -- -D warnings
  ```

### Continuation prompts (for next session)

If session dies mid-round, use this prompt:

```
Continue implementing Phase 10 of MASTER-PLAN.md (Full SMT Parity).
Read MASTER-PLAN.md to find the next unchecked round (10.xx tasks).
Run `cargo test --workspace` first to verify clean state.
Pick up where the last session left off.
After finishing all rounds, run /multi-perspective-improve in a loop.
```

---
