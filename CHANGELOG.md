# Changelog

All notable changes to Assura are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added

- `assura completions <shell>` command for bash/zsh/fish/powershell tab completion
- `assura doctor` command to check installation (Z3, CVC5, Rust toolchain, WASM target)
- `assura coverage` command to report contract coverage of a Rust project
- `assura agent-instructions` command for AI coding agent quick reference
- `CONTRIBUTING.md` contributor guide
- `docs/CHEATSHEET.md` one-page quick reference
- `docs/FAQ.md` troubleshooting guide
- `docs/COOKBOOK.md` with 25 contract patterns
- `docs/SCENARIOS.md` with 5 practical walkthroughs
- AI agent templates: service-typestate, concurrency (CONC.1-6), error propagation

## [0.1.0] - 2025-06-14

Initial release of the Assura compiler.

### Compiler Pipeline

- **Parser**: lexer (logos) + recursive-descent parser (rowan CST) with
  full Pratt expression parsing (8 precedence levels)
- **Name Resolution**: symbol table, scope analysis, cross-reference tracking
- **HIR Lowering**: AST to high-level IR with desugaring
- **Type Checker**: 50+ domain-specific checkers covering all 14 feature
  categories (MEM, SEC, TYPE, CONC, NUM, PERF, FMT, STOR, PLAT, TEST, CORE, MISC)
- **SMT Verification**: Z3 backend with Layer 0 (structural) and Layer 1
  (SMT-based) verification; CVC5 fallback; portfolio solver mode
- **Code Generation**: Rust source output via prettyplease; generates
  Cargo workspace with debug_assert! from contracts; proptest generation
  for timeout/unknown results

### CLI Commands

- `assura check` -- full pipeline (parse, resolve, type-check, verify)
  with `--watch`, `--stats`, `--dump-smt`, `--layer`, `--solver` options
- `assura build` -- verify + generate Rust project + cargo check + WASM support
- `assura init` -- scaffold new project with assura.toml and starter contract
- `assura explain` -- look up error codes from 43-entry catalog
- `assura fmt` -- format .assura source files with `--check` mode for CI
- `assura infer` -- generate skeleton bind contracts from Rust source
- `assura test-gen` -- generate proptest code from contracts
- `assura audit` -- scan Rust projects for contract violations

### Language Features

- 195 EBNF grammar productions from the specification
- 10 declaration types: contract, bind, fn, service, type, enum, extern,
  block, prophecy, codec_registry
- Refinement types, linear types, typestate, effect system, taint tracking
- ~278 error codes across 8 categories (A01xxx-A08xxx)
- Watch mode with filesystem notifications and content-hash deduplication

### Editor Support

- VS Code extension with TextMate syntax highlighting and LSP client
- Tree-sitter grammar for editor integration
- LSP server with diagnostics, go-to-definition, hover, completion,
  document symbols, formatting, find references, and rename

### Infrastructure

- CI pipeline with clippy, tests, no-z3 build, generated code check
- 3 demo contracts (libwebp CVE-2023-4863, zlib CVE-2022-37434,
  mbedtls 4x CVSS 9.8 CVEs)
- 50+ must_compile, 30+ must_reject fixture tests, 19 e2e tests