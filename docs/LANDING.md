# Assura

## Write what it should do. AI proves it does.

A contract-first language for the AI era. Humans write behavioral contracts.
AI writes verified implementations. The compiler proves correctness
mathematically. Ships as Rust.

```assura
contract SafeDivision {
  input(a: Int, b: Int)
  output(result: Int)

  requires { b != 0 }
  ensures  { result * b + (a mod b) == a }
  ensures  { abs(result) <= abs(a) }
  effects  { pure }
}
```

You write *what*. AI figures out *how*. `rustc` compiles the result.

---

## The Problem

AI writes most new code. Nobody trusts it.

- **85% of developers use AI coding tools**, but only **29% trust the output**
  (Stack Overflow 2025)
- **66% cite "almost right but not quite"** as their top frustration
- **45% of AI-generated code contains security vulnerabilities** (Veracode)
- AI code produces **1.7x more issues per PR** than human code (CodeRabbit 2025)
- Senior engineers spend **4-6 extra hours per week** reviewing AI output

Unit tests do not solve this. An OOPSLA 2025 study proved property-based tests
catch **~50x more bugs per test** than unit tests. AI-generated tests are
worse: they mirror implementation bugs. If `divide(10, 0)` returns `0` due to
a bug, the generated test asserts `== 0`.

Human code review has collapsed under the volume. The current pipeline is
broken: AI generates code nobody can fully review, verified by tests that
do not actually verify behavior.

---

## How It Works

```
Human writes contracts (.assura)
    |
    v
AI generates implementation
    |
    v
Assura compiler verifies (Z3/CVC5 SMT solver)
    |
    +--[proof fails]--> counterexample returned to AI --> AI fixes --> re-verify
    |
    v
Generates Rust source (.rs)
    |
    v
rustc compiles --> native binary / WASM
```

**Two layers, two audiences:**

| Layer | Who writes it | What it contains |
|-------|---------------|------------------|
| Contract language | Humans | *What* the software must do: preconditions, postconditions, invariants, effects |
| Implementation IR | AI | *How* it works: flat structure, typed slots, explicit annotations, optimized for Transformers |

**Three verification tiers, fastest first:**

| Tier | Time | What it checks |
|------|------|----------------|
| Structural | < 10ms | Types, syntax, names |
| Decidable SMT | < 200ms | Refinement types, flow analysis, effects |
| Heavy SMT | < 10s | Full invariants, temporal properties |

AI iterates on tiers 1-2 at sub-second speed. Most fix cycles take under
200ms. The AI can do **100 write-verify-fix cycles** in the time a human
completes one code review.

---

## CVE Prevention

**CVE-2023-4863** was a heap buffer overflow in libwebp's Huffman table
construction. It affected Chrome, Firefox, Safari, Android, iOS, and every
Electron app on the planet. CVSS 9.8. Exploited in the wild.

In Assura, it is mathematically impossible. Four features block it:
MEM.1 (buffer bounds on `huffman_tables`), SEC.1 (taint tracking from
`VP8LReadBits()` through `code_lengths`), NUM.2 (table size verification),
CORE.4 (axiomatic bound on secondary table entries).

The contracts that prevent it are not written from hindsight. Assura's
language *requires* these decisions: input must be marked tainted, integer
overflow behavior must be explicit, buffer bounds must be specified, errors
must propagate. C lets you skip all of them.

**Verified against 6 CVSS 9.8 CVEs across 3 critical C libraries:**

| CVE | Library | Root Cause | Assura Prevention |
|-----|---------|------------|-------------------|
| CVE-2023-4863 | libwebp | Tainted input overflows Huffman table | MEM.1 + SEC.1 + NUM.2 |
| CVE-2022-37434 | zlib | Tainted XLEN overflows gzip extra buffer | MEM.1 + SEC.1 + MISC.1 |
| CVE-2023-45199 | mbedTLS | Tainted key length overflows ECDH buffer | MEM.1 + SEC.1 + CORE.3 |
| CVE-2022-46393 | mbedTLS | Config flag mismatch in DTLS CID buffer | MEM.1 + PLAT.2 |
| CVE-2024-23775 | mbedTLS | Integer overflow in X.509 extension | MEM.2 + SEC.1 |
| CVE-2024-45158 | mbedTLS | Stack buffer sized by config constant | MEM.1 + PLAT.2 |

SEC.1 (taint tracking) + MEM.1 (memory regions) is the common denominator in
5 of 6. These two features alone prevent the majority of CVSS 9.8 memory
safety vulnerabilities in C libraries.

---

## 50 Features, 12 Categories

Every feature was discovered by stress-testing real C/C++ codebases, not by
imagination. 20 projects across 14 domains. The last 5 projects found zero
new features from 28 candidates, confirming convergence.

| Category | ID | Features |
|----------|----|----------|
| **CORE** Verification Infrastructure | CORE.1-8 | Ghost code, lemmas, frame conditions, axiomatic definitions, quantifier triggers, opaque functions, prophecy variables, liveness contracts |
| **MEM** Memory Safety | MEM.1-4 | Memory regions, fixed-width integers, allocator contracts, circular buffer contracts |
| **TYPE** Types and Contracts | TYPE.1-3 | Interface contracts, recursive structural invariants, error propagation |
| **SEC** Trust and Security | SEC.1-5 | Taint tracking, FFI boundaries, constant-time execution, secure erasure, cryptographic spec conformance |
| **CONC** Concurrency | CONC.1-6 | Shared memory protocols, callback re-entrancy, determinism, lock ordering, temporal deadlines, weak memory ordering |
| **STOR** Storage and Durability | STOR.1-6 | Crash recovery, page cache, MVCC/snapshot isolation, transactional rollback, monotonic state, storage failure model |
| **FMT** Data Formats and Parsing | FMT.1-6 | Binary format, bit-level format, string encoding, codec dispatch, checksum integrity, protocol grammar conformance |
| **NUM** Numerical and Precision | NUM.1-2 | Numerical precision contracts, precomputed table verification |
| **PLAT** Platform and Configuration | PLAT.1-3 | Platform abstraction, compile-time feature flags, resource limits |
| **PERF** Performance | PERF.1-2 | Unsafe escape with proof obligation, complexity bounds |
| **TEST** Testing and Verification | TEST.1-3 | Test generation from contracts, behavioral equivalence, multi-pass refinement |
| **MISC** Specialized | MISC.1-2 | Incremental/coroutine contracts, scoped invariant suspension |

A project activates only the categories it needs. CORE is always on. A CLI
tool might use `[core, mem, type]`. A TLS library uses all 12.

---

## Beyond the State of the Art

Every comparable tool proves safety. None proves liveness, models weak
memory, or provides prophecy variables for linearizability proofs.

| Capability | Assura | Dafny | Verus | SPARK Ada | F* |
|------------|--------|-------|-------|-----------|-----|
| Standalone contract language | Yes | Yes | No (Rust annotations) | No (Ada annotations) | Yes |
| Generates code from spec | Yes (Rust) | Yes (C#, Java, Go, JS, Python) | No (verifies existing Rust) | No (verifies existing Ada) | Partial (extracts to OCaml/F#/C) |
| AI-native design | Yes | No | No | No | No |
| Systems-level verification | 50 features | General-purpose | Functional correctness | Safety-critical domains | General-purpose |
| Verification tiering | 3-tier (<10ms / <200ms / <10s) | Single-pass SMT | Single-pass SMT | 5 progressive levels | Single-pass SMT |
| Weak memory ordering | CONC.6: per-thread ghost views, all 5 C++ orderings | No | No | No (sequentially consistent) | No |
| Prophecy variables | CORE.7: deferred-resolution ghost state | No | No | No | No |
| Liveness proofs | CORE.8: liveness-to-safety reduction + BMC | No (IronFleet excluded liveness) | No | No (safety only) | No |
| Target language | Rust (zero-cost, ownership as safety net) | GC languages | Rust | Ada | OCaml/F#/C |

Assura extends safety verification to include liveness ("good things
eventually happen"), weak memory models (real hardware behavior under
`Ordering::Relaxed`), and prophecy variables (proving linearizability of
lock-free data structures). These are research-frontier capabilities that
no comparable tool offers as integrated language features.

---

## Proven Across 8 Demo Projects

Each project exercises a different domain and demonstrates different
verification capabilities.

| Demo Project | Domain | Features Used | Proof Point |
|---|---|---|---|
| libwebp | Image codec (CVE-2023-4863) | 27/50 | The worst image CVE of the decade, made impossible |
| zlib | Compression (CVE-2022-37434) | 22/50 | The most deployed C library, verified |
| mbedTLS | TLS/crypto (4 CVSS 9.8 CVEs) | 32/50 | Four critical TLS vulnerabilities, all prevented |
| FreeRTOS | Safety-critical RTOS | 26/50 | Zero priority inversion, zero tick-overflow corruption |
| llama.cpp | AI inference engine | 27/50 | Quantization error bounded, SIMD implementations proven equivalent |
| sudo/sudo-rs | Privilege management | 27/50 | Rust rewrite proven equivalent to C original |
| PX4 Autopilot | Real-time flight control | 36/50 | Schedule proven feasible, all failure modes handled |
| Unbound DNS | Recursive DNS resolver | 29/50 | DNSSEC chains proven valid, compression loops impossible |

PX4 uses 36 of 50 features (72%), the highest coverage of any project
tested. The CORE infrastructure features (ghost code, lemmas, frame
conditions) appear in 57% of all verification proofs, serving as the
connective tissue that lets domain-specific properties compose without
dedicated features.

---

## Get Started

```bash
cargo install assura --locked
```

Requires a [Rust toolchain](https://rustup.rs/) (1.85+). Prebuilt installers
are also on [GitHub Releases](https://github.com/assura-lang/assura/releases).
Details: [CRATES-IO.md](CRATES-IO.md).

```bash
# Check contracts
assura check contracts/

# Generate and verify implementation
assura build contracts/ --target rust

# Full pipeline: verify + compile
assura build contracts/ --target rust --compile
```

**Repository:** [github.com/assura-lang/assura](https://github.com/assura-lang/assura)

**License:** MIT OR Apache-2.0 (dual license)

---

## Backed by Research

The thesis that AI-generated code needs formal verification is supported
by converging research from independent sources.

| Source | Finding |
|--------|---------|
| Martin Kleppmann (Dec 2025) | "AI will make formal verification go mainstream" |
| Leonardo de Moura, Z3 creator (Feb 2026) | "When AI Writes the World's Software, Who Verifies It?" |
| Ben Congdon (Dec 2025) | "The Coming Need for Formal Specification" |
| Stack Overflow 2025 | 85% use AI tools; 29% trust them; 66% cite "almost right" |
| CodeRabbit 2025 | AI code: 10.83 issues/PR vs human code: 6.45 issues/PR |
| OOPSLA 2025 | Property-based tests catch ~50x more bugs per test than unit tests |
| PLDI 2025 (Mundler et al.) | Type checking in the LLM loop cuts compilation errors by 74.8% |
| Veracode | ~45% of AI-generated code contains security vulnerabilities |
| METR 2025 RCT | AI tools made experienced developers 19% slower on complex tasks |
| Vericoding benchmark | Dafny LLM verification success: 82-96% (and improving) |
