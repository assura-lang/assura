[![CI](https://github.com/assura-lang/assura/actions/workflows/ci.yml/badge.svg)](https://github.com/assura-lang/assura/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE-MIT)

# Assura

**Write what it should do. AI proves it does.**

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

## The Problem

AI writes most new code. Nobody trusts it. 85% of developers use AI coding
tools, but only 29% trust the output. 45% of AI-generated code contains
security vulnerabilities. AI-generated tests mirror implementation bugs: if
`divide(10, 0)` returns `0` due to a bug, the generated test asserts `== 0`.

Assura replaces trust with proof. Contracts define *what* the code must do.
The compiler uses SMT solvers (Z3/CVC5) to *prove* the implementation
satisfies every contract, or returns a counterexample showing exactly how
it fails.

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

Three verification tiers, fastest first:

| Tier | Time | What it checks |
|------|------|----------------|
| Structural | < 10ms | Types, syntax, names |
| Decidable SMT | < 200ms | Refinement types, flow analysis, effects |
| Heavy SMT | < 10s | Full invariants, temporal properties |

## Quick Start

### Prerequisites

The compiler needs Z3 for SMT verification. Check your setup with:

```bash
cargo run -- doctor
```

### Build from source

```bash
git clone https://github.com/assura-lang/assura.git
cd assura
cargo build
```

### Usage

```bash
# Initialize a new project
cargo run -- init my-project

# Check a contract file
cargo run -- check demos/libwebp-huffman.assura

# Check with JSON output
cargo run -- check demos/libwebp-huffman.assura --json

# Dump the AST
cargo run -- --ast demos/libwebp-huffman.assura

# Dump tokens
cargo run -- --tokens demos/libwebp-huffman.assura

# Explain an error code
cargo run -- explain A03001

# Build and generate Rust code
cargo run -- build demos/libwebp-huffman.assura

# Format a contract file
cargo run -- fmt demos/libwebp-huffman.assura

# Infer contracts from Rust source
cargo run -- infer src/main.rs

# Print AI agent instructions (for setting up AI coding assistants)
cargo run -- agent-instructions
```

## Example: CVE Prevention

CVE-2023-4863 was a CVSS 9.8 heap buffer overflow in libwebp that affected
Chrome, Firefox, Safari, Android, iOS, and every Electron app on the planet.

In Assura, it is mathematically impossible. Four features block it: memory
regions (MEM.1), taint tracking (SEC.1), precomputed table verification
(NUM.2), and axiomatic definitions (CORE.4). See
[`demos/libwebp-huffman.assura`](demos/libwebp-huffman.assura) for the full
contract.

## 50 Features, 12 Categories

| Category | Features |
|----------|----------|
| **CORE** Verification Infrastructure | Ghost code, lemmas, frame conditions, axiomatic definitions, quantifier triggers, opaque functions, prophecy variables, liveness contracts |
| **MEM** Memory Safety | Memory regions, fixed-width integers, allocator contracts, circular buffer contracts |
| **TYPE** Types and Contracts | Interface contracts, recursive structural invariants, error propagation |
| **SEC** Trust and Security | Taint tracking, FFI boundaries, constant-time execution, secure erasure, cryptographic spec conformance |
| **CONC** Concurrency | Shared memory protocols, callback re-entrancy, determinism, lock ordering, temporal deadlines, weak memory ordering |
| **NUM** Numerical and Precision | Numerical precision contracts, precomputed table verification |
| **PERF** Performance | Unsafe escape with proof obligation, complexity bounds |
| **FMT** Binary Formats | Binary/bit-level format contracts, string encoding, codec dispatch, checksum, protocol grammar |
| **STOR** Storage | Crash recovery, page cache, MVCC, rollback, monotonic state, failure models |
| **PLAT** Platform | Platform abstraction, feature flags, resource limits |
| **TEST** Testing | Test generation from contracts, behavioral equivalence, multi-pass refinement |
| **MISC** Miscellaneous | Incremental contracts, scoped invariant suspension |

A project activates only the categories it needs. CORE is always on.

## Documentation

- [Tutorial](docs/TUTORIAL.md) (getting started, first contract, verification layers)
- [Quick Reference](docs/CHEATSHEET.md) (types, clauses, effects, CLI commands on one page)
- [Scenario Guides](docs/SCENARIOS.md) (greenfield dev, retrofit existing code, security audit, CI, team onboarding)
- [Contract Cookbook](docs/COOKBOOK.md) (25 ready-to-copy contract patterns by category)
- [Troubleshooting / FAQ](docs/FAQ.md) (Z3 timeouts, counterexamples, common errors)
- [Internals](docs/INTERNALS.md) (architecture, crate map, SMT encoding)
- [Language Specification](docs/SPECIFICATION.md) (195 EBNF productions, 50 verification features, ~278 error codes)
- [Implementation Roadmap](docs/ROADMAP.md)
- [Competitive Analysis](docs/INVESTIGATION.md)
- [Contributing](CONTRIBUTING.md)
- [Demo Contracts](demos/)

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
