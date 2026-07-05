# Assura Development Status

> 164,785 lines of Rust, 4,925 tests, **21 workspace members** (re-counted 2026-07-04 via `scripts/count-crates.sh`).

## What Works Today

| Capability | Status |
|------------|--------|
| Parse `.assura` contracts | 24 demos, 157 test fixtures |
| Name resolution with stdlib prelude types | Done |
| Type checking with 60+ checkers across all 50 spec features | Done |
| Z3 verification of requires/ensures/invariant clauses | Done |
| CVC5 verification (native FFI + shell fallback) | Done |
| Rust codegen: multi-file projects | Done |
| WASM codegen (`--target wasm`) | Done |
| IR prompt generation for AI coding agents | Done |
| IR parsing, structural validation, and SMT verification | Done |
| MCP server (5 tools) | Done |
| gRPC server with streaming verification (5 RPCs) | Done |
| LSP server (hover, completion, go-to-def, symbols) | Done |
| VS Code extension (TextMate grammar + LSP client) | Done |
| Tree-sitter grammar (highlight, indent, locals) | Done |
| Contract formatter (`assura fmt`) | Done |
| Contract inference from Rust source (`assura infer`) | Done |
| Property-based test generation from contracts | Done |
| Inline `#[assura::*]` annotation checking in Rust | Done |
| Runtime contract monitoring (`--runtime-checks`) | Done |
| Watch mode with incremental compilation | Done |
| Parallel SMT verification | Done |
| Library crates on crates.io (v0.1.0+) | Done (CLI via GitHub Releases / cargo-dist) |

## Crate Breakdown

Workspace members only (`Cargo.toml` `members = ["crates/*"]` with
`exclude` for fuzz targets and exploratory `crates/assura-driver`).

| Crate (package) | LOC | Tests | Role |
|-----------------|-----|-------|------|
| assura-parser | 9,597 | 188 | Lexer (logos) + recursive-descent parser (rowan CST), Pratt expressions |
| assura-ast | 2,942 | 39 | Canonical AST, DeclVisitor, ExprVisitor, ExprFolder |
| assura-resolve | 5,767 | 184 | Scope analysis, imports, stdlib prelude injection |
| assura-types | 43,305 | 1,704 | 60+ checkers in CHECKER_PIPELINE, all 50 spec features |
| assura-smt | 53,152 | 1,245 | Z3 + CVC5, Layer 2 verifier, prophecy/liveness/weak-memory, IR exec |
| assura-codegen | 15,867 | 658 | Multi-file Rust projects, proptest gen, WASM, IR body substitution |
| assura-pipeline | 2,103 | 66 | Canonical compile/compile_full/verify_typed/run_at |
| assura-config | 1,299 | 53 | assura.toml, VerifyOptions, CompilerConfig |
| assura-diagnostics | 4,163 | 73 | Error codes, ariadne + JSON rendering |
| assura (dir: assura-cli) | 12,910 | 283 | CLI binary: check, build, init, fmt, infer, … |
| assura-lsp | 1,965 | 55 | Language server (tower-lsp) |
| assura-server | 809 | 27 | gRPC + HTTP/JSON API |
| assura-mcp | 841 | 28 | MCP server for AI agent integration |
| assura-fmt | 648 | 52 | Formatter |
| assura-macros | 1,973 | 58 | Proc macros (`#[contract]`, `#[trust]`) |
| assura-stdlib | 409 | 18 | Stdlib modules (math, string, collections, …) |
| assura-rust-analyzer | 2,514 | 92 | Syn-based Rust source parser for contract inference |
| assura-test-support | 376 | 10 | Shared test helpers |
| assura-bench | 421 | 0 | Criterion benchmarks |
| assura-runtime | 262 | 10 | Runtime support for contracts |
| assura-llm | 3,462 | 82 | LLM provider abstraction for auto-implement / suggest |
| **Total** | **164,785** | **4,925** | |

`crates/assura-driver` is **excluded** from the workspace (exploratory rustc
driver). Refresh counts with `bash scripts/count-crates.sh`.

## Remaining Work

### Public launch

- [x] Close the AI verification loop (IR semantic verification via SMT)
- [x] `assura build` produces native binaries (not just `cargo check`)
- [x] Runtime contract monitoring (`--runtime-checks` persists in release builds)
- [x] Stdlib contracts auto-import (abs, min, max, clamp available without explicit import)
- [x] Large-scale verification benchmarks (500+, 1000+, 5000+ clauses)
- [x] LLM verification success rate benchmark (20 graded contracts, 4 reference IRs)
- [x] Make repo public, enable CodeQL
- [x] Publish library crates to crates.io (v0.1.0; CLI via cargo-dist / GitHub Releases only)

### Future directions

These are not blocking the initial release but are on the radar:

- Online playground (try Assura without installing Z3)
- Homebrew formula / pre-built binaries (GitHub Releases / cargo-dist); crates.io co-publish includes `assura` CLI (#838)
- Editor support beyond VS Code (Neovim, Emacs, JetBrains)
- [x] `feature_max` SMT binding + resolve registration (names in clauses, no false A02001)
- Richer stdlib postconditions (e.g., `abs` ensures `result >= 0`)
- Encode `incremental_contract` in SMT (MISC.1 / #833: parser block form + step/resume subset; zlib InflateDecoder invariant verifies)
- CI integration action (`assura-lang/assura-action`)
- Package registry for shareable contract libraries

## Development History

Phases 1 through 11 are complete (107/109 tasks). The repo is public and
CodeQL is enabled. The 1 remaining item is crates.io publish.

| Phase | Tasks | Summary |
|-------|-------|---------|
| 1: Fix Open Bugs | 7/7 | Flaky tests, parser clause bodies, tech debt |
| 2: Wire Structural Checkers | 14/14 | 14 previously-stub checkers now have real logic |
| 3: Fix Partial Checkers | 12/12 | 12 partial implementations completed |
| 4: Multi-File Compilation | 5/5 | Project config, cross-file resolve/type-check/verify |
| 5: Testing and Quality | 6/6 | MUST COMPILE/REJECT fixtures, fuzzing, CI |
| 6: Ecosystem | 2/4 | VS Code extension, tree-sitter grammar |
| 7: Production Hardening | 5/5 | Watch mode, incremental compilation, parallel SMT |
| 8: Inline Annotations | 7/7 | `check-rust` command, 7 annotation types |
| 9: Code Quality | 16/16 | Deduplication, helper extraction, visitor patterns |
| 10: Full SMT Parity | 12/12 | CVC5 native parity, policy unification |
| 11: Architecture | 20/20 | Module splits, encoder extraction, span precision |
| 12: Product Gaps | 6/7 | AI loop, native build, runtime checks, stdlib, benchmarks |
