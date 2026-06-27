# Assura Master Plan v4

> Updated 2026-06-27 after code-level verification of all 50 feature
> checkers, AI integration, and codegen. Replaces v3 (2026-06-14) which
> contained a stale audit snapshot that misclassified completed work.
>
> **State:** 152K LOC, 4,540 tests, 19 crates, 50 feature checkers wired
> with real logic, Z3 + CVC5 verification, WASM codegen, MCP/gRPC servers.
>
> **What this file is for:** Tracking the remaining gaps between "working
> compiler" and "product that delivers the market-research scenarios
> end-to-end." Phases 1-11 from v3 are complete; git history has the
> details.

## Rules (non-negotiable)

- **Never mark `[x]` without running every acceptance test command.**
  "I wrote the code" is not done. "The test passes in my terminal" is.
- **Never skip an acceptance test** because "it's obvious" or "similar
  to another test." Run it. See it pass. Then check it off.
- **Every task has a verification command block.** If you can't run
  one of them, the task is blocked. Say so. Don't mark it done.
- **Scope limit**: Focus on the next 1-3 tasks only. Don't load the
  whole plan into working memory.
- **Session protocol**: At the end of each session, update the
  Progress Notes section with what was completed and what to do next.

## Agent entrypoint (LLM / agent sessions)

Before implementing a task, open the **agent entrypoint** line if present
(one primary file + where to wire). If missing, use AGENTS.md
"Agent entrypoint" table and `docs/error-codes-agent.md` for error codes.

| Work area | Agent entrypoint | Wire / register |
|-----------|------------------|------------------|
| New Layer 0 checker | `crates/assura-types/src/checks/<name>.rs` (scaffold: `bash scripts/agent-new-checker.sh <name>`) | `CHECKER_PIPELINE` in `crates/assura-types/src/pipeline.rs` |
| Domain / CVE feature logic | `crates/assura-types/src/domain/` or `checkers/` (see `CHECKER-LAYERS.md`) | Thin `run_*_checks` in `checks/` + pipeline row |
| SMT encoding / manager method | `crates/assura-smt/src/advanced.rs` or `z3_backend/encoder/` | `entry/mod.rs` `verify()` or encoder; agent-guards section 7 |
| CVC5 parity follow-on | `crates/assura-smt/src/cvc5_backend.rs` (+ `cvc5_*`) | Mirror Z3 path; `cargo test -p assura-smt --features cvc5-verify` |
| Codegen / IR | `crates/assura-codegen/src/lib.rs`, `contract.rs`, `types_gen.rs` | `crates/assura-cli/src/ir_cmd.rs` |
| CLI UX | `crates/assura-cli/src/check/` | Prefer `assura_pipeline::{compile, compile_full, verify_typed}` |
| MCP / gRPC server | `crates/assura-mcp/src/lib.rs`, `crates/assura-server/src/main.rs` | Pipeline integration |

---

## Current State (2026-06-27, verified)

| Crate | LOC | Tests | Status |
|-------|-----|-------|--------|
| assura-parser | 8,561 | 187 | Solid. 24 demos parse. Pratt expressions, 195+ productions. |
| assura-ast | 2,706 | 23 | DeclVisitor, ExprVisitor, ExprFolder, BinOp helpers. |
| assura-resolve | 5,704 | 182 | Scope analysis, imports, stdlib prelude type injection. |
| assura-types | 42,906 | 1,705 | 60+ `run_*_checks` in CHECKER_PIPELINE. All 50 features have real logic. |
| assura-smt | 52,577 | 1,228 | Z3 + CVC5 (native + shell). Layer 2 verifier. Advanced passes (prophecy, liveness, weak memory). IR exec. |
| assura-codegen | 15,217 | 630 | Multi-file Rust projects, proptest gen, WASM target, Cranelift config, IR body substitution. |
| assura-pipeline | 1,375 | 50 | Canonical compile/compile_full/verify_typed/run_at. |
| assura-config | 1,128 | 44 | assura.toml, VerifyOptions, CompilerConfig. |
| assura-diagnostics | 4,027 | 66 | Error code registry, ariadne + JSON rendering. |
| assura-cli | 8,586 | 253 | 22 commands: check, build, init, fmt, infer, test-gen, audit, repl, ir, ir-prompt, doc, mcp, lsp, ... |
| assura-lsp | 1,975 | 55 | Hover, completion, go-to-def, document symbols. |
| assura-server | 798 | 16 | gRPC (check, build, explain, health, check_stream) + HTTP/JSON fallback. |
| assura-mcp | 740 | 20 | 5 MCP tools: assura_check, assura_infer, assura_explain, assura_type_map, assura_ir_prompt. |
| assura-fmt | 1,609 | 76 | Formatter using ExprFolder. |
| assura-macros | 782 | 20 | Proc macros. |
| assura-stdlib | 409 | 0 | 12 .assura modules (math, string, collections, option, result, io, fs, net, crypto, iter, bytes, time). |
| assura-rust-analyzer | 2,315 | 84 | Syn-based Rust source parser for contract inference. |
| assura-test-support | 376 | 0 | Shared test helpers (typecheck_ok, verify_ok, compile_result, etc.). |
| assura-bench | 2 | 0 | Criterion benchmarks (pipeline.rs). |
| **Total** | **151,793** | **4,540** | |

### What works end-to-end today

- Parse `.assura` contracts (24 demos, 157 test fixtures)
- Name resolution with stdlib prelude types
- Type checking with 60+ checkers across all 50 spec features
- Z3 verification of requires/ensures/invariant clauses
- CVC5 verification (native FFI + shell fallback)
- Rust codegen: multi-file projects that `cargo check` passes
- WASM codegen: `--target wasm` produces wasm32-wasip1 projects
- IR prompt generation for AI agents
- IR parsing and structural validation against contracts
- MCP server for AI agent integration (5 tools)
- gRPC server with streaming verification (5 RPCs)
- LSP server with hover/completion/go-to-def
- VS Code extension with TextMate grammar + LSP client
- Tree-sitter grammar with highlight/indent/locals queries
- Contract formatter
- Contract inference from Rust source
- Property-based test generation from contracts
- Inline `#[assura::*]` annotation checking in Rust files

---

## Completed Phases (v3 history, 106/108 tasks done)

Phases 1-11 from MASTER-PLAN v3 are complete. Summary:

| Phase | Tasks | What was done |
|-------|-------|---------------|
| 1: Fix Open Bugs | 7/7 | Flaky tests, audit workspace root, infer extractor, cache hasher, parser clause bodies, tech debt (#42-#62) |
| 2: Wire Structural Checkers | 14/14 | All 14 previously-stub checkers (callback reentrancy, temporal deadline, protocol grammar, monotonic state, numerical precision, Layer2Verifier, incremental compiler, weak memory, prophecy, liveness, crash recovery, page cache, MVCC, rollback) now have real logic |
| 3: Fix Partial Checkers | 12/12 | All 12 (binary format, bit level, string encoding, checksum, resource limit, precomputed table, multi-pass refinement, incremental contract, frame checker, stdlib, IR parser, Cranelift) have real logic |
| 4: Multi-File Compilation | 5/5 | Project config, multi-file resolve, cross-file type checking, project-level verify, CLI integration |
| 5: Testing and Quality | 6/6 | MUST COMPILE fixtures, MUST REJECT fixtures, demo regression, snapshot tests, fuzzing harness, CI pipeline |
| 6: Ecosystem | 2/4 | VS Code extension, tree-sitter grammar done. CodeQL blocked (private repo). crates.io blocked (needs verification). |
| 7: Production Hardening | 5/5 | Watch mode, incremental compilation, verification cache, parallel SMT, error recovery |
| 8: Inline Annotations | 7/7 | check-rust command, 7 annotation types (#101-#107) |
| 9: Code Perfection | 16/16 | Deduplication, helper extraction, BinOp methods, ExprFolder, lowering helpers, trivia handling |
| 10: Full SMT Parity | 12/12 | CVC5 native parity, IR body constraints, havoc/assume policy, clause policy, prelude policy, solver outcome, portfolio mode, encode policies |
| 11: Architecture Refactoring | 20/20 | Module splits, encoder extraction, SMT policy unification, span precision, ExprFolder extraction |

**Only 2 tasks remain from v3** (both blocked on external actions):
- 6.01: CodeQL security scanning (blocked: repo is private)
- 6.03: crates.io placeholder publish (blocked: needs verification)

---

## Phase 12: Product Gaps (market-research scenarios)

> These are the verified gaps between the current compiler and the
> end-to-end scenarios described in the market research
> (github.com/SebTardif/market-research/tree/main/ai-language-assura).
> Verified 2026-06-27 by reading actual code, not plan documents.

### 12.01: Close the AI verification loop (IR semantic verification)

- Depends on: none
- **What**: The core value proposition is: AI writes IR, compiler verifies
  IR satisfies contracts, returns counterexample, AI fixes. Currently
  `assura ir` does structural validation (`validate_ir_against_contract`)
  but does NOT run SMT verification on the IR. The loop is parse IR,
  validate structure, codegen Rust. Not: parse IR, verify correctness,
  return counterexample.
- **Fix**: After structural validation, run `verify_ir()` from
  `assura-pipeline` (compiles contract, parses IR, builds in-memory
  extras, runs SMT) and return structured counterexamples.
  Add an MCP tool (`assura_ir_verify`) so AI agents can close the loop
  without CLI access. CLI: `assura ir <file.ir> --contract <spec> --verify`.
- **Agent entrypoint:** `crates/assura-cli/src/ir_cmd.rs` (add verify step
  after codegen), `crates/assura-mcp/src/lib.rs` (add `assura_ir_verify` tool),
  `crates/assura-pipeline/src/lib.rs` (`verify_ir` function)
- [x] **Acceptance Tests**:
  ```bash
  # 1. IR that satisfies a contract returns verified
  cargo run --bin assura -- ir demos/generated/check_alphabet_bounds.ir --contract demos/libwebp-huffman.assura --verify 2>&1 | grep -i 'verified'
  # 2. MCP tool exists and works
  grep 'assura_ir_verify' crates/assura-mcp/src/lib.rs
  # 3. Pipeline + MCP tests pass
  cargo test -p assura-pipeline -- ir
  cargo test -p assura-mcp -- ir_verify
  ```

### 12.02: `assura build` produces native binary

- Depends on: none
- **What**: For native targets, `assura build` runs `cargo check` (not
  `cargo build`), so no binary is produced. WASM correctly runs
  `cargo build` and reports the `.wasm` path. The user must manually
  `cd generated && cargo build` to get a native binary.
- **Fix**: Change the native path in `build.rs` to run `cargo build`
  instead of `cargo check`, and report the binary path.
- **Agent entrypoint:** `crates/assura-cli/src/build.rs` (line with
  `let cargo_verb = if is_wasm { "build" } else { "check" }`)
- [ ] **Acceptance Tests**:
  ```bash
  # 1. Build produces a binary for native target
  cargo run --bin assura -- build tests/fixtures/test_basic.assura
  # Must print a path to a compiled binary, not just "cargo check passed"
  # 2. WASM still works
  cargo run --bin assura -- build tests/fixtures/test_basic.assura --target wasm
  # Must print path to .wasm file
  # 3. Integration test
  cargo test -p assura-cli build_produces_binary
  ```

### 12.03: Runtime contract monitoring

- Depends on: none
- **What**: The market research scenarios (API service, embedded, financial)
  mention "Runtime Contract Monitoring." Currently, contract enforcement
  is only via `debug_assert!` which is stripped in release builds. No
  production monitoring, alerting, or telemetry for contract violations.
- **Fix**: Add a `--runtime-checks` codegen mode that generates
  `if !condition { assura_runtime::violation("contract", "clause", file, line) }`
  calls that persist in release builds. The `assura_runtime` crate
  provides pluggable handlers (log, panic, webhook, OpenTelemetry).
- **Agent entrypoint:** `crates/assura-codegen/src/contract.rs`
  (contract clause codegen), new `crates/assura-runtime/` crate
- [ ] **Acceptance Tests**:
  ```bash
  # 1. Generated code with --runtime-checks has non-debug assertions
  cargo run --bin assura -- build tests/fixtures/test_basic.assura --runtime-checks
  grep -r 'assura_runtime::violation\|contract_violation' generated/src/
  # Must find at least 1 runtime check call
  # 2. Runtime checks survive release compilation
  cd generated && cargo build --release
  # Must compile without error
  # 3. Unit tests
  cargo test -p assura-codegen runtime_checks
  ```

### 12.04: Stdlib contracts auto-import

- Depends on: none
- **What**: Prelude *types* (Int, Nat, List, etc.) are auto-injected into
  every file's symbol table during resolution. But prelude *contracts*
  (`abs`, `min`, `max`, `clamp`) and stdlib module declarations (math,
  string, collections) are NOT auto-imported. The function
  `prelude_contract_names()` in assura-stdlib is dead code (defined but
  never called outside its own crate tests).
- **Fix**: Wire `prelude_contract_names()` into the resolver so standard
  contracts are available without explicit `import std.math`. Optionally
  auto-load stdlib module declarations on `import std.*`.
- **Agent entrypoint:** `crates/assura-resolve/src/lib.rs` (where prelude
  types are injected), `crates/assura-stdlib/src/lib.rs`
  (`prelude_contract_names`)
- [ ] **Acceptance Tests**:
  ```bash
  # 1. prelude_contract_names is called from outside assura-stdlib
  grep -rn 'prelude_contract_names' crates/ --include='*.rs' | grep -v assura-stdlib | grep -v test
  # Must find at least 1 call site in assura-resolve
  # 2. A contract using abs() without import compiles
  echo 'contract Test { input(x: Int) ensures { abs(x) >= 0 } }' > /tmp/stdlib_test.assura
  cargo run --bin assura -- check /tmp/stdlib_test.assura
  # Must not report "unknown function abs"
  # 3. Tests
  cargo test -p assura-resolve stdlib_prelude
  ```

### 12.05: Large-scale verification benchmarks

- Depends on: none
- **What**: Criterion benchmarks exist for up to 100 clauses across 3
  demo files. No stress testing for 1000+ contracts, multi-file projects,
  or published benchmark numbers. Unknown if verification stays
  performant at the scale described in the market research scenarios
  (e.g., SQLite rewrite with thousands of contracts).
- **Fix**: Add benchmark fixtures with 500+, 1000+, and 5000+ clauses.
  Add multi-file project benchmarks. Publish baseline numbers.
- **Agent entrypoint:** `crates/assura-bench/benches/pipeline.rs`
- [ ] **Acceptance Tests**:
  ```bash
  # 1. Large fixture exists
  wc -l tests/fixtures/bench_large.assura
  # Must be >= 500 lines
  # 2. Benchmark runs without timeout
  cargo bench -p assura-bench -- --sample-size 10 2>&1 | tail -20
  # Must complete without panic or timeout
  # 3. Multi-file benchmark exists
  ls tests/fixtures/bench_project/*.assura | wc -l
  # Must be >= 3 files
  ```

### 12.06: Measure LLM verification success rate

- Depends on: 12.01
- **What**: The competitive table shows Dafny at 82-96% LLM verification
  success. Assura's rate is listed as "TBD" with zero measurements. This
  is a critical metric for the "AI-native" positioning. Without it, the
  claim that Assura is designed for AI cannot be validated.
- **Fix**: Create a benchmark suite of 50-100 contracts. For each, use
  `ir-prompt` to generate a prompt, send it to an LLM, collect the IR,
  validate it. Measure pass rate. Publish the numbers.
- **Agent entrypoint:** new `scripts/benchmark-llm-verify.sh` or
  `crates/assura-bench/src/llm_benchmark.rs`
- [ ] **Acceptance Tests**:
  ```bash
  # 1. Benchmark contracts exist
  ls tests/fixtures/llm_bench/*.assura | wc -l
  # Must be >= 20
  # 2. Script/tool exists
  ls scripts/benchmark-llm-verify.sh || ls crates/assura-bench/src/llm*
  # Must find at least one
  # 3. At least one measured result is documented
  grep -i 'verification.*rate\|success.*rate\|pass.*rate' docs/*.md
  ```

### 12.07: Public launch preparation

- Depends on: none
- **What**: The repo is private. No crate published on crates.io. No
  external users. No community. The market research describes a product
  with `cargo install assura` but that does not work today.
- **Fix**: Make repo public, enable CodeQL (task 6.01), publish to
  crates.io (task 6.03), add README badges, set up GitHub Discussions.
- **Agent entrypoint:** `.github/workflows/`, `Cargo.toml` (publish
  metadata), `README.md`
- [ ] **Acceptance Tests**:
  ```bash
  # 1. Repo is public
  gh repo view assura-lang/assura --json visibility --jq '.visibility'
  # Must return "PUBLIC"
  # 2. crates.io publish dry-run passes
  cargo publish -p assura-cli --dry-run
  # 3. CodeQL workflow exists and is not commented out
  grep -L '#.*codeql\|disabled' .github/workflows/security.yml
  ```

---

## Dependency Graph

```
12.01 (AI loop) ──────► 12.06 (LLM benchmark)
12.02 (native binary)   (independent)
12.03 (runtime monitor) (independent)
12.04 (stdlib import)   (independent)
12.05 (scale bench)     (independent)
12.07 (public launch)   (independent, unblocks 6.01 + 6.03)
```

Most tasks are independent. Only 12.06 depends on 12.01 (need the
closed loop before measuring success rate).

---

## Progress Notes

### 2026-06-27: v3 to v4 rewrite

- Verified all 50 feature checkers have real logic (none are stubs).
- Verified WASM target, MCP server, gRPC server, IR pipeline all functional.
- Corrected stale "14 STRUCTURAL / 12 PARTIAL" classifications from the
  June 14 audit. All were fixed during Phases 2-3.
- Collapsed 2,514 lines of completed task history into summary table.
- Identified 7 genuine remaining gaps via code-level investigation.
- Previous v3 detail is preserved in git history.
