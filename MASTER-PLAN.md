# Assura Development Status

> 152K lines of Rust, 4,540 tests, 19 crates.

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

## Crate Breakdown

| Crate | LOC | Tests | Role |
|-------|-----|-------|------|
| assura-parser | 8,561 | 187 | Lexer (logos) + recursive-descent parser (rowan CST), Pratt expressions, 195+ productions |
| assura-ast | 2,706 | 23 | Canonical AST, DeclVisitor, ExprVisitor, ExprFolder |
| assura-resolve | 5,704 | 182 | Scope analysis, imports, stdlib prelude injection |
| assura-types | 42,906 | 1,705 | 60+ checkers in CHECKER_PIPELINE, all 50 spec features |
| assura-smt | 52,577 | 1,228 | Z3 + CVC5, Layer 2 verifier, prophecy/liveness/weak-memory passes, IR exec |
| assura-codegen | 15,217 | 630 | Multi-file Rust projects, proptest gen, WASM, Cranelift config, IR body substitution |
| assura-pipeline | 1,375 | 50 | Canonical compile/compile_full/verify_typed/run_at |
| assura-config | 1,128 | 44 | assura.toml, VerifyOptions, CompilerConfig |
| assura-diagnostics | 4,027 | 66 | ~278 error codes, ariadne + JSON rendering |
| assura-cli | 8,586 | 253 | 22 commands (check, build, init, fmt, infer, test-gen, audit, repl, ir, doc, ...) |
| assura-lsp | 1,975 | 55 | Language server (tower-lsp) |
| assura-server | 798 | 16 | gRPC + HTTP/JSON API |
| assura-mcp | 740 | 20 | MCP server (5 tools for AI agent integration) |
| assura-fmt | 1,609 | 76 | Formatter |
| assura-macros | 782 | 20 | Proc macros (`#[contract]`, `#[trust]`) |
| assura-stdlib | 409 | 0 | 12 stdlib modules (math, string, collections, option, result, io, fs, net, crypto, iter, bytes, time) |
| assura-rust-analyzer | 2,315 | 84 | Syn-based Rust source parser for contract inference |
| assura-test-support | 376 | 0 | Shared test helpers |
| assura-bench | 2 | 0 | Criterion benchmarks |
| **Total** | **151,793** | **4,540** | |

## Remaining Work

### Public launch

- [x] Close the AI verification loop (IR semantic verification via SMT)
- [x] `assura build` produces native binaries (not just `cargo check`)
- [x] Runtime contract monitoring (`--runtime-checks` persists in release builds)
- [x] Stdlib contracts auto-import (abs, min, max, clamp available without explicit import)
- [x] Large-scale verification benchmarks (500+, 1000+, 5000+ clauses)
- [x] LLM verification success rate benchmark (20 graded contracts, 4 reference IRs)
- [ ] Make repo public, publish to crates.io, enable CodeQL

### Future directions

These are not blocking the initial release but are on the radar:

- Online playground (try Assura without installing Z3)
- Homebrew formula / pre-built binaries via `cargo install assura`
- Editor support beyond VS Code (Neovim, Emacs, JetBrains)
- Constant folding for `feature_max` named constants in SMT encoding
- Richer stdlib postconditions (e.g., `abs` ensures `result >= 0`)
- CI integration action (`assura-lang/assura-action`)
- Package registry for shareable contract libraries

## Development History

Phases 1 through 11 are complete (106/108 tasks). The 2 remaining items
from that era are blocked on the repo being private (CodeQL scanning,
crates.io publish).

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
