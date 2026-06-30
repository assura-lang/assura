# Assura Implementation Roadmap

> For implementers deciding whether to build Assura, and in what order.
> Based on the SPECIFICATION.md (195 EBNF productions, 50 verification
> features, ~278 error codes) and INVESTIGATION.md (tech stack,
> competitive analysis, stress-tested demo projects).

## Scope Summary

Assura is a contract-first language that transpiles to Rust. The compiler
is written in Rust, performs 3-layer verification (structural, decidable
SMT, heavy SMT) using Z3/CVC5, and generates Rust source code that
`rustc` compiles to native or WASM binaries.

The full specification defines:
- 50 verification features across 12 categories + 8 CORE
- 195 EBNF grammar productions, ~199 keywords
- 6-feature type system (refinement, dependent, linear, typestate,
  effect rows, information flow)
- 3-layer verification: Layer 0 (<10ms), Layer 1 (<200ms), Layer 2
  (<10s), Layer 3 (BMC/k-induction)
- ~278 error codes with structured JSON output
- CLI, AI Agent API (gRPC), LSP server

This roadmap sequences the work from "nothing exists" to "all 50
features verified and tested."

## Current Status

The compiler is functional with all core pipeline stages implemented:
parser (195 EBNF productions), name resolution, type checker (50+
domain-specific checkers across 12 categories), SMT verification
(Z3 primary, CVC5 fallback, portfolio mode), and Rust code generation.
The CLI, LSP server, formatter, and MCP server are operational.
Over 4,500 tests pass across 19 crates.

See [MASTER-PLAN.md](../MASTER-PLAN.md) for the detailed task-level
status of each phase.

---

## Phase 0: Foundation (Months 1-3)

**Goal**: A working compiler that parses contracts, builds an AST,
performs structural checks, and emits valid Rust source code for
trivial contracts. Prove the transpile-to-Rust architecture works.

### Month 1: Lexer, Parser, AST

**Deliverable**: `assura check file.assura` parses a contract file
and reports syntax errors with error codes.

| Task | Effort | Details |
|------|--------|---------|
| Lexer | 1 week | ~199 keywords (Section 1.1). Tokenize identifiers, literals, operators, comments. Decision: hand-rolled lexer in Rust vs `logos` crate. Recommend `logos` for speed and simplicity; it handles keyword disambiguation well. |
| Parser | 2-3 weeks | Recursive descent for the core EBNF (Sections 1.2-1.11). Start with a subset: `SourceFile`, `ServiceDecl`, `ContractDecl`, `TypeDecl`, `EnumDecl`, `OperationDecl`, `QueryDecl`, `RequiresClause`, `EnsuresClause`, `EffectsClause`, `Predicate`, `Expr`. Skip Layers 8-21 (extended contract layers) initially. |
| AST types | 1 week | Define Rust structs for every AST node. Use `Span` annotations for source locations on every node. Derive `Debug`, `Clone`, `PartialEq`. |
| Error reporting | 1 week | Implement error codes A01001-A01005 (syntax errors). JSON output format from Section 7.3. Human-readable mode with `ariadne` or `miette` crate. |

**Key decisions**:

- **Parser generator vs hand-rolled**: Hand-rolled recursive descent.
  The grammar has enough context sensitivity (refinement types,
  effect rows, where clauses) that parser generators add friction.
  Gleam, Rust, and Swift all use hand-rolled parsers. A PEG/parser
  combinator (`chumsky`, `winnow`) is a reasonable middle ground.
- **tree-sitter**: Build a tree-sitter grammar in parallel for editor
  support, but do NOT use it as the compiler's parser. tree-sitter
  is error-tolerant (good for editors, bad for a verification compiler
  that needs exact parses).

**Risk**: The grammar is large (195 productions). Prioritize the
contract language (what humans write) before the IR grammar (what AI
generates). The IR grammar (Section 4) can be deferred to Phase 1.

### Month 2: Name Resolution, Type Checking (Layer 0)

**Deliverable**: The compiler rejects contracts with undefined names,
type mismatches, and missing fields. Error codes A02001-A03006.

| Task | Effort | Details |
|------|--------|---------|
| Scope analysis | 1 week | Build a symbol table. Resolve `QualifiedName` references. Detect duplicate definitions (A02003), circular imports (A02005). Module paths match file paths (Section 8.1). |
| Core type checker | 2 weeks | Check base types (`Int`, `Nat`, `Float`, `Bool`, `String`, `Bytes`, `Unit`, `Never`). Check generic types (`List<T>`, `Map<K,V>`, `Set<T>`, `Option<T>`). Check field access, function calls, pattern matches. Emit A03001-A03005. |
| Pattern exhaustiveness | 1 week | Coverage checker for `match` expressions over enum variants. Emit A10001 (non-exhaustive). This is a well-known algorithm (Maranget's approach). |

**What NOT to build yet**: Refinement types, dependent types, linear
types, typestate, effect system, information flow. All of those are
Layer 0 features but involve significant complexity. Get basic types
working first.

### Month 3: Rust Codegen + End-to-End

**Deliverable**: `assura build` produces a Cargo project with
generated Rust code that compiles and runs. Contract pre/post
conditions become `debug_assert!` calls.

| Task | Effort | Details |
|------|--------|---------|
| Codegen framework | 1 week | Generate Rust source code as strings (or use `quote`/`prettyplease` for formatting). Create `Cargo.toml` with workspace setup per Section 10.3. Output to `generated/` directory. |
| Type mapping | 1 week | Implement Section 6.1: `Int` -> `i64`, `Nat` -> `u64`, `List<T>` -> `Vec<T>`, `Map<K,V>` -> `BTreeMap<K,V>`, etc. Generate newtype wrappers for refinement types (Section 6.2). |
| Contract codegen | 1 week | `requires` -> `debug_assert!` at function entry. `ensures` -> `debug_assert!` before return. `old()` expressions -> save values before body executes (Section 6.7). |
| CLI | 1 week | Implement `assura check`, `assura build`, `assura init`, `assura explain` (Section 10). Wire up `--json` / `--human` output modes. `assura build` invokes `cargo build` on the generated project. |

**End-of-Phase 0 milestone**: This contract compiles and runs:

```assura
contract SafeDivision {
  input(a: Int, b: Int)
  output(result: Int)
  requires { b != 0 }
  ensures  { result * b + (a mod b) == a }
  effects  { pure }
}
```

It generates Rust with `debug_assert!(b != 0)` and
`debug_assert!(result * b + (a % b) == a)`. The contract is
not SMT-verified yet; that comes in Phase 1.

**Phase 0 team**: 1-2 engineers. One strong Rust developer who has
built a parser before. A second person can work on the CLI and
codegen in parallel.

**Phase 0 effort**: ~3 person-months.

---

## Phase 1: v0.1 Alpha (Months 4-8)

**Goal**: Minimum viable verification pipeline. Z3 integration,
CORE infrastructure features, MEM.1 + SEC.1 (the two features that
catch the most CVEs), and the libwebp demo working end-to-end.

### Month 4: Layer 0 Completion (Linearity, Typestate, Effects)

**Deliverable**: The compiler checks linearity, typestate, and
effect containment without invoking Z3.

| Task | Effort | Details |
|------|--------|---------|
| Linear type checker | 2 weeks | Implement context splitting (Section 2.5). Track usage grades: 0 (erased), 1 (linear), n (exact), omega (unlimited). Emit A05001-A05005. The key insight: refinement predicates are ghost (grade 0), not computational. See Test Case 1 in Section 13. |
| Typestate checker | 1 week | Finite state machine DFA per typestate variable (Section 2.6). Track state through branches; reject ambiguous states after diverging branches (A06004). Typestate variables must be linear. |
| Effect checker | 1 week | Set inclusion check. Each function's body effects must be a subset of its declared effect row (Section 3.5). Implement effect hierarchy (`io` = union of all IO sub-effects, Section 3.6). Emit A07001-A07005. |

**Interaction priority**: Implement Linear + Typestate first (typestate
requires linearity), then Linear + Effect (resource-scoped effects).
See Section 13 implementation priority.

### Month 5: Z3 Integration + Layer 1

**Deliverable**: Refinement type checking via Z3. The compiler proves
or disproves `requires`/`ensures` clauses using SMT.

| Task | Effort | Details |
|------|--------|---------|
| Z3 bindings | 1 week | Use the `z3` crate (Rust bindings to libz3). Set up the solver context, declare sorts, define functions. Implement the timeout mechanism (1s default for Layer 1, configurable via `assura.toml`). |
| Refinement type encoding | 2 weeks | Translate `{v: T \| P} <: {v: T \| Q}` into SMT queries per Section 5.2. Encode `P => Q` as `(assert P) (assert (not Q)) (check-sat)`. UNSAT means the subtyping holds. SAT means counterexample exists. |
| Counterexample extraction | 1 week | When Z3 returns SAT, extract the model (concrete variable values) and format as structured JSON (Section 5.3). This is critical for AI iteration: the counterexample tells the AI exactly what input breaks the contract. |

**SMT theories used in Layer 1** (all decidable):
- QF_UFLIA: quantifier-free uninterpreted functions + linear integer arithmetic (refinement types)
- QF_UFLRA: same with real arithmetic (float contracts)
- QF_DT: datatypes (information flow labels, typestate guards)
- QF_LIA: linear integer arithmetic (grade arithmetic)

**Key risk**: Z3 binding complexity. The `z3` crate is well-maintained
but the API is low-level. Expect 1-2 weeks of wrestling with lifetimes
and sort declarations before the first query works.

### Month 6: CORE Features (Ghost Code, Lemmas, Frame Conditions)

**Deliverable**: Ghost variables, lemma functions, and frame
conditions work. These are the connective tissue that makes
domain-specific features composable.

| Task | Effort | Details |
|------|--------|---------|
| CORE.1 Ghost code | 1.5 weeks | Ghost variables, functions, and blocks (Section 14.CORE.1). Enforce erasure: ghost code cannot affect runtime. Ghost functions must be pure. Ghost assertions become SMT obligations. Codegen: completely erased (or `debug_assert` in debug mode). Error codes A54001-A54005. |
| CORE.2 Lemmas | 1.5 weeks | Proof functions that generate no runtime code (Section 14.CORE.2). `apply lemma_name(args)` adds the lemma's ensures as an assumption. `induction var` generates base/inductive cases. Error codes A55001-A55005. |
| CORE.3 Frame conditions | 1 week | `modifies` clauses declaring what a function changes (Section 14.CORE.3). Everything else is implicitly unchanged. This is critical for modular verification: without frame conditions, the verifier must re-prove all invariants after every call. |

**Why these matter**: In the stress-testing rounds (INVESTIGATION.md),
CORE features appeared in 57% of gap collapses (16 of 28 in Round 6).
Ghost code alone simplifies MEM.1, FMT.2, STOR.5, and TEST.3.

### Month 7: MEM.1 + SEC.1 (The CVE Killers)

**Deliverable**: Memory region contracts and taint tracking. These
two features together catch 5 of 6 CVSS 9.8 CVEs analyzed in the
investigation.

| Task | Effort | Details |
|------|--------|---------|
| MEM.1 Memory regions | 2 weeks | Buffer bounds contracts. `requires offset + len <= buf.capacity`. Ghost regions tracking valid index ranges. SMT encoding of region containment (`region_a ⊆ region_b`). Error codes: buffer overread, overwrite, out-of-bounds access. This is the single most impactful safety feature. |
| SEC.1 Untrusted data taint | 2 weeks | Taint labels on data from external sources (network, file, user input). Taint propagation through operations. Taint must be explicitly validated before use in sensitive positions (array indices, allocation sizes, SQL queries). Information flow lattice (Section 2.7) handles the label hierarchy. |

**Why these two first**: The CVE prevention matrix (INVESTIGATION.md)
shows SEC.1 + MEM.1 as the common denominator in 5 of 6 CVSS 9.8
vulnerabilities across libwebp, zlib, and mbedTLS. Getting these two
right is the minimum viable safety story.

### Month 8: Error Reporting + Integration Testing

**Deliverable**: Complete error reporting with all Phase 1 error codes.
The libwebp demo contract compiles, verifies, and generates correct
Rust.

| Task | Effort | Details |
|------|--------|---------|
| Error catalog | 1 week | Implement all error codes for Phase 1 features: A01xxx-A08xxx, A10xxx, A11xxx (from Section 7.2). Each error includes: location, secondary locations, contract reference, counterexample (when SMT), suggested fixes with confidence scores. |
| `assura explain` | 0.5 weeks | Implement the explain command for all Phase 1 error codes (Section 10.2). Each explanation includes: description, example code, fix guidance. |
| libwebp demo | 2 weeks | Write ~200-300 lines of Assura contracts for the libwebp Huffman table parsing path (the CVE-2023-4863 attack surface). Verify that MEM.1 + SEC.1 catch the buffer overflow. Generate Rust code. Compile and run. |
| Integration tests | 0.5 weeks | Test suite exercising all 11 type interaction test cases from Section 13 (those applicable to Phase 1 features). Each test case is both a specification and a regression test. |

**End-of-Phase 1 milestone**: A developer can write this contract and
get a verified Rust implementation:

```assura
service HuffmanDecoder {
  type BitReader {
    data: Bytes,
    pos: Nat,
    ghost remaining: Nat
  }

  operation decode_table {
    input(reader: BitReader, code_lengths: List<Nat>)
    output(table: HuffmanTable)

    requires { reader.pos < reader.data.len() }
    requires { forall cl in code_lengths: cl <= 15 }
    ensures  { table.entries.len() <= MAX_TABLE_SIZE }
    effects  { pure }
  }
}
```

**Phase 1 team**: 2-3 engineers. The Z3 integration requires someone
with SMT solver experience (or willingness to learn; the `z3` crate
docs and Dafny's Z3 encoding are good references).

**Phase 1 effort**: ~8-10 person-months.

---

## Phase 2: v0.2 Beta (Months 9-14)

**Goal**: Feature completeness for primary use cases. Remaining MEM,
SEC, TYPE, CONC, and FMT features. Layer 2 verification. LSP server.
Test generation.

### Months 9-10: Remaining Type System Features

| Task | Effort | Details |
|------|--------|---------|
| Information flow checker | 2 weeks | Full security lattice (Public < Internal < Confidential < Restricted). Declassification points tracked and auditable (Section 2.7). Purpose labels for GDPR (Section 2.7). Error codes A08001-A08005. |
| Dependent types (restricted) | 2 weeks | Types depending on `Nat`, `Bool`, and finite enums (Section 2.4). `Vec<T, n>` with index arithmetic. Index erasure at runtime. Error code A03006 (dependent index mismatch). Full value-level dependency deferred to v2. |
| Totality checker | 1 week | Exhaustive pattern matches (already done in Phase 0). Termination checking via `decreases` measures (Section 2.8). `partial` escape hatch. Error codes A09001-A09004. |
| Measures | 1 week | Structurally recursive functions lifted into the logic (Section 2.3). `len`, `elems`, `keys`, `values`, `size`. Encode as uninterpreted functions in SMT with definitional axioms. |

### Months 10-11: Feature Categories (MEM, SEC, TYPE, CONC)

| Feature | Effort | Priority | Rationale |
|---------|--------|----------|-----------|
| MEM.2 Fixed-width integers | 1 week | High | Overflow detection. Already partially covered by refinement types; this adds first-class `checked_add`/`checked_mul` and width-aware arithmetic. |
| MEM.3 Allocator contracts | 1.5 weeks | Medium | Allocation/deallocation pairing, size tracking, arena lifetime. Needed for jemalloc demo. |
| MEM.4 Circular buffer contracts | 1 week | Medium | Wrap-around indexing, logical-to-physical mapping. Found from zlib stress test. |
| SEC.2 FFI boundary contracts | 1.5 weeks | High | extern/bind declarations (Section 1.10). Trust boundaries at Rust interop points. Runtime assertion wrappers in debug mode. |
| SEC.3 Constant-time execution | 1 week | Medium | Reject branches and memory accesses dependent on secret data. Found from WireGuard stress test. |
| SEC.4 Secure erasure | 1 week | Medium | Guarantee sensitive data is zeroed after use. Linear type consumed via `zeroize`. |
| SEC.5 Cryptographic conformance | 1.5 weeks | Low | Top-level theorem connecting code to math spec. Found from mbedTLS stress test. Deferred if team is small. |
| TYPE.1 Interface contracts | 1 week | High | Trait-like contracts for abstract interfaces. Callback re-entrancy restrictions. |
| TYPE.2 Recursive structural invariants | 1 week | High | Tree balance, list sortedness, graph acyclicity as type-level properties. |
| TYPE.3 Error propagation | 1 week | High | `must_propagate` on error types. Detect silently swallowed errors. |
| CONC.1 Shared memory protocols | 2 weeks | High | Per-object access modes (exclusive, shared-read, actor-isolated). Detect data races at compile time. |
| CONC.2 Callback re-entrancy | 1 week | Medium | Prevent re-entrant calls through callback chains. |
| CONC.3 Determinism contracts | 1 week | Medium | Guarantee reproducible output. Ban HashMap (use BTreeMap), ban random, enforce ordering. |
| CONC.4 Lock ordering | 1 week | Medium | Static lock hierarchy. Prevent deadlocks by enforcing acquisition order. Found from jemalloc stress test. |
| CONC.5 Temporal deadlines | 1 week | Medium | Bounded response time. `must_complete_within(ticks: N)`. Found from WireGuard stress test. |

### Months 12-13: FMT Features + Layer 2

| Feature | Effort | Priority | Rationale |
|---------|--------|----------|-----------|
| FMT.1 Binary format | 1.5 weeks | High | Byte-aligned format contracts. Offset, length, magic bytes. Critical for parser demos (libwebp, zlib). |
| FMT.2 Bit-level format | 1.5 weeks | High | Sub-byte parsing (Huffman codes, JPEG, H.264). Ghost bit cursor. Found from stb_image stress test. |
| FMT.3 String encoding | 1 week | Medium | UTF-8/UTF-16 safety. Encoding-aware string operations. |
| FMT.4 Codec dispatch | 1 week | Medium | Magic-byte routing to per-format contract sets. Found from stb_image. |
| FMT.5 Checksum integrity | 0.5 weeks | Medium | CRC32, Adler-32, SHA verification. |
| FMT.6 Protocol grammar | 1.5 weeks | High | RFC conformance. State machine for protocol parsing (HTTP, TLS, DNS). |
| Layer 2 verification | 2 weeks | High | Quantified invariants (AUFLIA), functional correctness (AUFLIA + UF), termination, serialization roundtrip. Timeout strategy (Section 12.3): emit warning, generate property-based test, flag for `--deep`. |
| CORE.4 Axiomatic definitions | 1 week | Medium | Abstract mathematical concepts (hash functions, cryptographic primitives). |
| CORE.5 Quantifier triggers | 1 week | Medium | E-matching hints for the SMT solver. Critical for preventing solver instability on quantified formulas. |
| CORE.6 Opaque functions | 1 week | Medium | Hide implementation from verifier. Reason only about the contract. Needed for scalability. |

### Month 14: LSP + Test Generation

| Task | Effort | Details |
|------|--------|---------|
| LSP server | 3 weeks | Implement Language Server Protocol for the contract language. Completions for keywords, types, effects. Go-to-definition. Hover for type info. Inline diagnostics. Error squiggles. VS Code extension (syntax highlighting via TextMate grammar, LSP client). |
| TEST.1 Test generation | 2 weeks | Generate property-based tests from contracts (Section 14.TEST.1). Each `requires`/`ensures` pair produces a test case. Use `proptest` or `quickcheck` in generated Rust. Also generate boundary value tests from refinement predicates. |

**End-of-Phase 2 milestone**: All MEM, SEC, TYPE, CONC, and FMT
features work. Layer 2 verification handles quantified invariants.
Developers have editor support. The zlib and mbedTLS demos work
end-to-end.

**Phase 2 team**: 3-4 engineers. One focused on SMT encoding (Layer 2
is significantly harder than Layer 1). One on the feature categories.
One on LSP and tooling. One floating for integration testing and demos.

**Phase 2 effort**: ~16-20 person-months.

---

## Phase 3: v0.3 (Months 15-20)

**Goal**: Advanced features, production readiness. Storage features,
liveness proofs, weak memory ordering, prophecy variables, AI Agent
API, and performance optimization.

### Months 15-16: STOR Features

| Feature | Effort | Priority | Rationale |
|---------|--------|----------|-----------|
| STOR.1 Crash recovery | 2 weeks | High | Write-ahead log contracts, crash point annotations, recovery proof obligations. Critical for database and filesystem demos (SQLite, littlefs). |
| STOR.2 Page cache contracts | 1.5 weeks | Medium | Pin/unpin protocols, eviction invariants, dirty page tracking. SQLite B-tree demo. |
| STOR.3 MVCC / snapshot isolation | 1.5 weeks | Medium | Read snapshots, write set isolation, serializability. |
| STOR.4 Transactional rollback | 1 week | Medium | Compensation actions on failure. Already partially covered by typestate. |
| STOR.5 Monotonic state | 1 week | High | Values that only increase. Epoch counters, sequence numbers, wear counters. |
| STOR.6 Storage failure model | 1 week | Medium | Flash wear, partial writes, bad sector modeling. Found from littlefs stress test. |

### Months 17-18: Advanced Verification (CORE.7, CORE.8, CONC.6)

These are the hardest features in the entire specification. They
extend Assura beyond what Dafny, F\*, or SPARK offer.

| Feature | Effort | Difficulty | Details |
|---------|--------|------------|---------|
| CONC.6 Weak memory ordering | 3 weeks | Very hard | Per-thread ghost views (GPS/RSL approach). Model all 5 C++ memory orderings (SeqCst, AcqRel, Acquire, Release, Relaxed). The SMT encoding is complex: each thread has its own view of shared state, and synchronization operations merge views. |
| CORE.7 Prophecy variables | 2 weeks | Hard | Ghost state with deferred resolution. Needed for linearizability proofs of lock-free data structures (Michael-Scott queue, Treiber stack). The variable's value is determined by a future event but constrained now. SMT encoding uses Skolemization. |
| CORE.8 Liveness contracts | 3 weeks | Hard | `eventually`, `leads_to`, `eventually_within`. Verification via liveness-to-safety reduction (Biere et al.). BMC with lasso detection at Layer 2. K-induction for unbounded proofs at Layer 3. Fairness encoding (compassion, justice). |

**Honest assessment**: CONC.6 and CORE.7 are research-adjacent. The
techniques exist (GPS, RSL, Iris, prophecy variables in Verus) but
have not been integrated into a single tool targeting Rust codegen.
Expect 1.5x-2x the estimated effort on these features. Budget for
dead ends and redesigns.

### Months 19-20: NUM, PLAT, PERF, MISC + AI Agent API

| Feature | Effort | Details |
|---------|--------|---------|
| NUM.1 Numerical precision | 1 week | Per-operation precision contracts. Fixed-point types. Unit-aware arithmetic. |
| NUM.2 Precomputed table verification | 1 week | `table[i] == f(i)` for all valid `i`. CRC tables, Huffman tables, zigzag tables. |
| PLAT.1 Platform abstraction | 1 week | OS-specific behavior behind trait boundaries. |
| PLAT.2 Feature flags | 1 week | Compile-time configuration. Combinatorial flag interactions. The mbedTLS CVEs showed why this matters. |
| PLAT.3 Resource limits | 1 week | Memory, stack, allocation bounds. |
| PERF.1 Unsafe escape | 1 week | `unsafe` blocks with proof obligations that the safety invariant is maintained. |
| PERF.2 Complexity bounds | 1.5 weeks | AARA (automatic amortized resource analysis). O(n), O(log n), O(n log n) bounds verified via LP solver. |
| TEST.2 Behavioral equivalence | 1.5 weeks | N-way equivalence testing (C vs Rust, SIMD vs scalar). Differential testing harness generation. |
| TEST.3 Multi-pass refinement | 1 week | Progressive JPEG, iterative solvers. Each pass improves on the previous. |
| MISC.1 Incremental contracts | 1 week | Stateful parsers, streaming decoders. Resume-from-any-point guarantees. |
| MISC.2 Scoped invariant suspension | 1 week | Temporarily break an invariant within a transaction, restore at boundary. |
| AI Agent API | 2 weeks | gRPC service (Section 11.2). `Check`, `Build`, `Explain`, `Health` RPCs. `CheckStream` for incremental AI iteration. JSON-over-HTTP fallback. |

**End-of-Phase 3 milestone**: All 50 features implemented. Layer 3
(BMC/k-induction) operational for liveness proofs. AI agents can
submit code via gRPC and get streaming verification results. The
full demo portfolio (libwebp, zlib, mbedTLS, FreeRTOS, sudo, PX4)
has working contract files.

**Phase 3 team**: 4-5 engineers. CONC.6 and CORE.7/CORE.8 each need
a dedicated engineer with formal methods background.

**Phase 3 effort**: ~22-28 person-months.

---

## Phase 4: v1.0 (Months 21+)

**Goal**: Production release. All 50 features verified and tested.
Comprehensive standard library. CI/CD integrations. Documentation.

### Months 21-23: Hardening and Standard Library

| Task | Effort | Details |
|------|--------|---------|
| Standard library | 3 weeks | Implement Section 9: core types (`Pos`, `NonNeg`, `Email`, `Uuid`), collection contracts (`ListOps`, sort, filter), numerical types (`Money<C>`, `FixedDecimal`), CRUD patterns, auth contracts. |
| Module system completion | 2 weeks | Contract composition with `extends` (Section 8.3). Service dependencies (Section 8.4). Contract libraries as publishable packages (Section 8.5). |
| Configuration | 1 week | Full `assura.toml` support (Section 10.3). Project profiles (Section 1.2): minimal, parser, database, embedded, crypto, tls, systems. |
| IR format | 2 weeks | Implement the Implementation IR (Section 4). Text format parser, binary (MessagePack) serializer. Canonical serialization. IR metadata. This is what AI agents generate; the compiler verifies it against contracts. |
| Performance tuning | 3 weeks | Verification caching (hash contract + implementation, skip re-verification if unchanged). Parallel SMT queries across independent contracts. Incremental compilation (only re-check changed modules). Target: Layer 0+1 in <1s for 10K-line projects. |

### Months 24-26: Ecosystem and Documentation

| Task | Effort | Details |
|------|--------|---------|
| CI/CD integration | 2 weeks | GitHub Action (`assura-lang/verify-action`). Docker image for CI pipelines. GitLab CI template. |
| Documentation | 3 weeks | Language tutorial (contract writing guide). Compiler internals guide. API reference. Error code reference (auto-generated from error catalog). Migration guides for Dafny/SPARK users. |
| Showcase builds | 4 weeks | Complete the demo portfolio: libwebp (CVE-2023-4863), zlib (CVE-2022-37434), mbedTLS (4 CVSS 9.8 CVEs). Full differential testing. CVE replay demonstrations. |
| Cranelift backend | 3 weeks | Dev-mode fast compilation. Cranelift for `assura build` (10x faster than rustc). Keep rustc for `assura build --release`. This is the v2 backend from the INVESTIGATION.md roadmap. |

**Phase 4 effort**: ~20-25 person-months.

---

## Critical Path

What blocks what. This is the dependency chain that determines the
minimum calendar time.

```
Parser ─────────────────────────────────────────────────────────────►
  │
  ├─► AST ──► Name Resolution ──► Core Type Checker
  │                                      │
  │              ┌───────────────────────┘
  │              │
  │              ├─► Linearity ──► Typestate ──► Effect Checker
  │              │        │              │              │
  │              │        │    ┌─────────┘              │
  │              │        │    │                        │
  │              ├─► Z3 Integration ──► Layer 1 ──► Layer 2 ──► Layer 3
  │              │        │                │
  │              │        ├─► MEM.1 ◄──────┘
  │              │        │
  │              │        ├─► SEC.1
  │              │        │
  │              │        └─► CORE.1-3 (ghost, lemmas, frames)
  │              │
  │              └─► Rust Codegen ──► Cargo Integration
  │                       │
  │                       └─► End-to-End Pipeline
  │
  └─► tree-sitter Grammar ──► LSP Server ──► VS Code Extension
```

**The bottleneck is Z3 integration.** Everything before it is
standard compiler engineering. Everything after it depends on having
a working SMT solver connection. The Z3 encoding strategy (Section 5.2)
is the make-or-break technical challenge of the project.

**Parallelizable work**:
- tree-sitter grammar and LSP (independent of verification engine)
- CLI and build system (independent of type checker)
- Standard library contracts (independent of compiler internals)
- Documentation (ongoing)
- Demo project contracts (can be written before the compiler verifies them)

---

## Risk Areas

### 1. Z3 Encoding Complexity (High Risk)

**What**: Translating Assura's 6-feature type system into SMT queries
that Z3 can solve reliably.

**Why it's hard**: Each feature alone has a well-known encoding. The
composition of all six features generates SMT queries that combine
quantifiers, uninterpreted functions, datatypes, and linear arithmetic
in ways that can cause solver instability. The "dangerous combinations"
in Section 12.2 are real:
- Quantified refinements + recursive measures trigger unbounded MBQI
- Nonlinear integer arithmetic (NIA) is undecidable
- Array theory + quantifiers cause exponential blowup

**Mitigation**: Start with decidable fragments only (Layer 1: QF_UFLIA,
QF_DT). Add quantifiers cautiously in Layer 2 with strict timeouts.
Study Dafny's and Verus's Z3 encoding strategies; both have solved
similar problems. Use CVC5 as a fallback solver.

### 2. Layer 3 BMC Scalability (High Risk)

**What**: Bounded model checking for liveness contracts (CORE.8)
scales exponentially with state space.

**Why it's hard**: BMC unrolls the transition relation K times. For
a system with N state variables, each step doubles the formula size.
K=1000 (the default) on a system with even modest state space can
produce SMT queries that take hours.

**Mitigation**: Modular verification (verify each component in
isolation). Compositional reasoning (verify component A's liveness
assuming component B's safety). Reduce K for development; increase
for pre-release. K-induction can prove properties unboundedly if the
invariant is inductive, but finding the right inductive invariant is
often the hard part.

### 3. Type Interaction Soundness (Medium Risk)

**What**: The 6 type features must compose correctly. Section 13
identifies 15 pairwise interactions and 4 three-way interactions.

**Why it's hard**: Each pairwise interaction has subtle corner cases.
Test Case 1 (refinement + linear): should a refinement predicate
count as a linear use? Test Case 5 (typestate + info flow): how does
declassification interact with state transitions? Getting even one
interaction wrong produces unsoundness (the compiler accepts code
that violates a contract).

**Mitigation**: Implement the 11 test cases from Section 13 as
regression tests. Add each pairwise interaction incrementally, with
a test before the code. Fuzz the type checker with randomly generated
contracts.

### 4. Rust Codegen Semantic Gap (Medium Risk)

**What**: Assura's memory model may diverge from Rust's ownership
model, forcing generated code to use `Rc`/`Arc` heavily.

**Why it's hard**: Assura has linear types (use exactly once) which
map cleanly to Rust ownership. But graded types (use exactly N times)
and shared-read access patterns may require reference counting. If
too much generated code uses `Arc<Mutex<T>>`, performance suffers.

**Mitigation**: Design the codegen to prefer ownership transfer
(move semantics) wherever possible. Use `Rc` only for omega-graded
values that are genuinely shared. Profile early; if `Arc` overhead
is significant, investigate Cranelift direct codegen (Phase 4) as
an alternative to transpiling through Rust.

### 5. Solver Performance at Scale (Medium Risk)

**What**: Verification time for a real project (10K+ lines of
contracts) may be unacceptable.

**Why it's hard**: Each contract clause generates one or more SMT
queries. A 10K-line project might have 500+ contracts, each with
3-5 clauses, generating 2,000+ SMT queries. If each query takes
100ms, total verification takes 3+ minutes.

**Mitigation**: Verification caching (skip unchanged contracts).
Parallel SMT queries. Modular verification (verify each module
independently; only re-verify modules that changed or whose
dependencies changed). The 3-layer architecture helps: most
iteration happens at Layer 0+1 (<200ms), with Layer 2 reserved
for pre-commit.

---

## MVP Definition

**The absolute minimum that's useful**:

Parser + Layer 0 type checking + MEM.1 + SEC.1 + Rust codegen

This is approximately Phase 0 + the first half of Phase 1 (~5 months
for 1-2 people).

**What it can do**: Parse contracts, check basic types and linearity,
check buffer bounds (MEM.1) and taint propagation (SEC.1) via Z3,
generate Rust code with debug assertions.

**What it proves**: The libwebp CVE-2023-4863 (CVSS 9.8, the single
worst image codec CVE of the decade) is mathematically impossible.
This is the minimum viable demo.

**What it can't do yet**: Ghost code, lemmas, frame conditions,
typestate, effects, information flow, dependent types, Layer 2/3,
LSP, test generation, AI Agent API.

**Why this matters**: A tool that catches the most common class of
CVEs (tainted input driving buffer overflows) is useful even without
the other 48 features. Ship early, iterate.

---

---

## Technology Decisions

### Compiler Implementation Language: Rust

Decided. The compiler is written in Rust. It generates Rust. The
INVESTIGATION.md evaluated 8 approaches (Section "Architecture
Decision") and concluded transpile-to-Rust is the best path:

- All effort on novel parts (type system, effect system, verification)
- `rustc` as a second safety net (borrow checker catches codegen bugs)
- Cargo ecosystem access for generated code
- Proven model (Gleam transpiles to Erlang, TypeScript to JavaScript)
- Fastest prototype timeline (3-6 months)

### Parser Strategy: Hand-Rolled Recursive Descent

The grammar has 195 productions with context-sensitive elements
(refinement type syntax, effect rows, where clauses, extended
contract layers). Parser generators (LALR, PEG) struggle with:
- `'requires' ['{'] Predicate ['}']` (optional braces)
- Refinement types `{v: T | P}` (ambiguous with set literals)
- Effect rows `<e1, e2 | tail>` (pipe as separator vs row variable)

A recursive descent parser handles these naturally with lookahead.
Use `logos` for lexing (fast, minimal).

**tree-sitter**: Build separately for editor support. tree-sitter
grammars are error-tolerant by design, which is the opposite of what
a verification compiler needs. They share the grammar specification
but not implementation.

### SMT Strategy: Z3 Primary, CVC5 Fallback

- **Z3** (primary): 15+ years mature. Best Rust bindings (`z3` crate).
  Handles QF_UFLIA, QF_DT, AUFLIA well. Default solver.
- **CVC5** (fallback): Competitive on quantified formulas. Better at
  some datatype theories. Use when Z3 times out. The `cvc5` crate
  exists but is less mature.
- **Portfolio mode** (future): Run both solvers in parallel, take the
  first result. Verus does this and reports significant reliability
  improvements.

**Encoding strategy**: Study Dafny's Boogie-to-Z3 encoding (open
source, well-documented) and Verus's direct Z3 encoding (also open
source). Both have solved the same class of problems.

### Codegen Strategy: String Templates with `prettyplease`

Generate Rust source code as formatted strings. Use `prettyplease`
for consistent formatting. Do NOT use `syn`/`quote` for generation
(they are designed for proc macros, not full program generation).

The generated code should be human-readable (with comments linking
back to contract clauses) for debugging, but NOT intended for human
editing. The `generated/` directory is a build artifact.

### Build System: Standard Cargo Workspace

Generated projects are standard Cargo workspaces:

```
project/
  Cargo.toml           # workspace
  contracts/           # human-written .assura files
  generated/           # compiler output (Rust source)
    Cargo.toml
    src/lib.rs
  app/                 # hand-written Rust (optional)
    Cargo.toml
    src/main.rs
```

This is the interop model from INVESTIGATION.md. The `generated/`
crate is a dependency of `app/`. Normal Cargo semantics.

### Error Reporting: `ariadne` for Human, JSON for AI

- **Human mode** (`--human`): Use `ariadne` crate for rich terminal
  diagnostics with source snippets, underlines, and suggested fixes.
- **AI mode** (`--json`, default): Structured JSON per Section 7.3.
  Error code, location, counterexample, suggested fixes with
  confidence scores.

### Testing Strategy for the Compiler Itself

- **Unit tests**: Each parser production, each type checker rule, each
  SMT encoding. Use Rust's `#[test]` framework.
- **Snapshot tests**: Parse a `.assura` file, serialize the AST, compare
  to a golden file. Use `insta` crate for snapshot testing.
- **Integration tests**: Each of the 11 type interaction test cases from
  Section 13. Each test is a `.assura` file with `// MUST COMPILE` or
  `// MUST REJECT <error code>` annotations.
- **Fuzzing**: Fuzz the parser with `cargo-fuzz`. Fuzz the type checker
  with randomly generated ASTs. Fuzz the Z3 encoding by generating
  random contracts and checking that verification terminates.

---

## What to Read Before Starting

1. **Gleam compiler source** (github.com/gleam-lang/gleam): The closest
   architectural precedent. Rust compiler that transpiles to another
   language. Study its parser, type checker, and codegen structure.

2. **Verus source** (github.com/verus-lang/verus): SMT-based Rust
   verification. Study its Z3 encoding strategy, especially how it
   handles quantifiers and triggers.

3. **Dafny source** (github.com/dafny-lang/dafny): The most mature
   verification language. Study its Boogie IR and Z3 encoding. The
   OOPSLA papers on Dafny's verification methodology are essential.

4. **Liquid Haskell papers**: The foundational work on refinement type
   checking via SMT. The encoding strategy (subtyping -> SMT validity)
   is exactly what Assura needs.

5. **Koka effect system**: Row-polymorphic effects. Study how Koka
   encodes effect rows and how it handles effect handlers.

6. **Section 13 of SPECIFICATION.md**: The 11 type interaction test
   cases. These are the specification for the hardest part of the
   type checker. Implement them as tests before writing the code.

---

## Known Challenges

**Type system composition** (Section 13): The 6-feature type system
composition is novel. No existing tool combines all six features.
Each feature alone is well-understood; the interactions are where
complexity lives.

**Research-adjacent features**: CONC.6 (weak memory ordering), CORE.7
(prophecy variables), and CORE.8 (liveness via BMC) extend Assura
beyond what comparable tools offer. They are also what differentiates
Assura from Dafny/Verus/SPARK.

**SMT scalability**: Z3 encoding must work beyond small examples.
Mitigations already in place: modular verification, caching, parallel
queries, portfolio solver mode (Z3 + CVC5).
