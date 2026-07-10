[![CI](https://github.com/assura-lang/assura/actions/workflows/ci.yml/badge.svg)](https://github.com/assura-lang/assura/actions/workflows/ci.yml)
[![Security](https://github.com/assura-lang/assura/actions/workflows/security.yml/badge.svg)](https://github.com/assura-lang/assura/actions/workflows/security.yml)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/assura-lang/assura/badge)](https://scorecard.dev/viewer/?uri=github.com/assura-lang/assura)
[![OpenSSF Best Practices](https://www.bestpractices.dev/projects/13476/badge)](https://www.bestpractices.dev/projects/13476)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-4500%2B%20passing-brightgreen)](#)

# Assura

**Write what it should do. AI proves it does.**

A contract-first language for the AI era. Humans write behavioral contracts.
AI writes verified implementations. The compiler proves correctness
mathematically. Ships as Rust.

```assura
contract HeartbeatResponse {
  input(record_length: Nat, payload_length: Nat, padding_length: Nat)

  requires { record_length >= 3 }              // TLS header: type + 2-byte length
  requires { payload_length >= 1 }
  requires { padding_length >= 16 }            // RFC 6520 minimum
  requires { 3 + payload_length + padding_length <= record_length }

  ensures  { payload_length + 16 <= record_length }   // response fits in buffer
  effects  { pure }
}
```

You write *what*. AI figures out *how*. Z3 proves it. `rustc` compiles the result.

## The Problem

AI writes most new code. Nobody trusts it. AI-generated tests mirror
implementation bugs: if `divide(10, 0)` returns `0` due to a bug, the
generated test asserts `== 0`. The test passes. The bug ships.

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

### Install the CLI

**Preferred (crates.io):**

```bash
cargo install assura --locked
```

Requires a [Rust toolchain](https://rustup.rs/) (edition 2024 / rustc 1.85+).
The first build downloads a Z3 prebuilt via the `z3` crate (`gh-release`); no
manual Z3 install is needed for normal use. See
[docs/CRATES-IO.md](docs/CRATES-IO.md).

**Prebuilt binaries:** [GitHub Releases](https://github.com/assura-lang/assura/releases)
(cargo-dist installers; multi-platform).

**From a monorepo clone:**

```bash
git clone https://github.com/assura-lang/assura.git
cd assura
cargo install --path crates/assura-cli --locked
# Optional standalone LSP binary:
cargo install --path crates/assura-lsp --locked
```

**Embedding as a library:** the public compile/verify facade is
[`assura-pipeline`](https://crates.io/crates/assura-pipeline) on crates.io
(v0.3.0+):

```toml
[dependencies]
assura-pipeline = "0.3"
```

Prefer crates.io for apps; use a git path dependency only when tracking
unreleased `main`. Release process: [docs/CRATES-IO.md](docs/CRATES-IO.md).

### Usage

**Primary path (install → check → build → test):** see
[docs/GETTING-STARTED.md](docs/GETTING-STARTED.md). That guide works on a
clean machine with copy-paste files (no monorepo required).

```bash
# Initialize a new project
assura init my-project

# Happy-path demos (must-pass). Prefer these over *-audit.assura files.
# See demos/README.md for the showcase vs EXPECT FAIL taxonomy.
assura check demos/heartbleed.assura
# Result-bearing postconditions need IR (`ShowcaseEcho.ir` next to the source):
assura check demos/showcase-echo.assura
# Verify, inject IR into Rust, and cargo test:
#   assura build demos/showcase-echo.assura --output /tmp/assura-out
#   (cd /tmp/assura-out && cargo test)

# Check with JSON output
assura check demos/libwebp-huffman.assura --json
# Agents: on success, inspect file_info.vacuous / vacuous_reason so empty
# sources or contracts with no SMT proof obligations are not treated as
# verified coverage (see also human-mode check-passed summaries).

# Check with verbose timing info
assura check demos/libwebp-huffman.assura --verbose

# Check with verification statistics
assura check demos/libwebp-huffman.assura --stats

# Explain an error code
assura explain A03001

# Build and generate Rust code
assura build demos/libwebp-huffman.assura

# Format a contract file
assura fmt demos/libwebp-huffman.assura

# Infer contracts from Rust source
assura infer src/main.rs

# Verify inline contract annotations in Rust source files
assura check-rust src/
# Body proof paths (in order):
#   1) co-located {Name}.ir
#   2) simple encoded Rust body (identity, + / - on params/literals)
# Otherwise ensures are body_not_modeled (not silent verified).
assura check-rust src/ --json

# Suggest contracts for unannotated functions
assura check-rust src/ --suggest

# Shell completions (raw script, or JSON with --json for agents)
assura completions zsh
assura completions bash --json   # {"command","shell","script"}
```

> **Tip:** If running from source without installing, prefix commands with `cargo run --`, e.g. `cargo run -- check demos/libwebp-huffman.assura`.

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
| **SEC** Trust and Security | Taint tracking, dependent types, constant-time execution, secure erasure, cryptographic spec conformance |
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
- [Demo Contracts](demos/) (real CVE prevention examples)
- [50 Example Contracts](examples/) (one per verification feature, organized by category)

## License

Dual-licensed under [MIT](LICENSE) or [Apache-2.0](LICENSE-APACHE), at your option.
