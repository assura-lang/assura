# Changelog

All notable changes to Assura are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/).

## [0.3.0](https://github.com/assura-lang/assura/compare/v0.2.0...v0.3.0) (2026-07-07)


### Features

* co-publish assura CLI and frontends to crates.io ([#845](https://github.com/assura-lang/assura/issues/845)) ([b651fc2](https://github.com/assura-lang/assura/commit/b651fc2ab8c7f7deffb9d4dbf1412114fc6c9885)), closes [#838](https://github.com/assura-lang/assura/issues/838)
* P1/P2 proof equating, offline IR, bin, strict, multi-contract ([#870](https://github.com/assura-lang/assura/issues/870)) ([9a724a5](https://github.com/assura-lang/assura/commit/9a724a53f9fdedad476a4660b1002ec98e9ebb41))


### Bug Fixes

* address GitHub AI code quality findings ([#850](https://github.com/assura-lang/assura/issues/850)) ([ebf9b8d](https://github.com/assura-lang/assura/commit/ebf9b8d5782fc62d6bcf8330cea223960294792d))
* CVC5 signed BV order for fixed-width I* types ([#860](https://github.com/assura-lang/assura/issues/860)) ([d06ab67](https://github.com/assura-lang/assura/commit/d06ab67cecd332b660f0e0448b3894a0666a7a39))
* signed BV comparisons for fixed-width I* types ([#859](https://github.com/assura-lang/assura/issues/859)) ([927063c](https://github.com/assura-lang/assura/commit/927063c4ce4b723e98df608a715c2201160c5013))
* SMT/types/IR batch (fixed-width, match, verify_ir, evolution) ([#856](https://github.com/assura-lang/assura/issues/856)) ([0b0d6eb](https://github.com/assura-lang/assura/commit/0b0d6ebdc1736936041e3a69b5eda6eba3177435))

## [Unreleased]

### Changed

* deps: upgrade rmcp 2.1 → 2.2 in assura-mcp (#907)

## [0.2.0](https://github.com/assura-lang/assura/compare/v0.1.0...v0.2.0) (2026-07-04)


### Features

* register feature_max in resolve; demos use named SMT bounds ([#832](https://github.com/assura-lang/assura/issues/832)) ([fd24fa8](https://github.com/assura-lang/assura/commit/fd24fa8863121905c1fb6b2835ff2cf0aafbcb21))


### Bug Fixes

* distinguish requires-only from empty contracts in check UX ([#822](https://github.com/assura-lang/assura/issues/822)) ([308eac3](https://github.com/assura-lang/assura/commit/308eac3cd4249498fee7777f1bd63f3636e44a36))
* do not fail publish on already-published crates ([#804](https://github.com/assura-lang/assura/issues/804)) ([ee96c5e](https://github.com/assura-lang/assura/commit/ee96c5e5dd9c6c34f5b484c2ad79c04e8dfe215b))
* drop ir_generate expect; document JSON vacuous and driver exclude ([#830](https://github.com/assura-lang/assura/issues/830)) ([a8a30ce](https://github.com/assura-lang/assura/commit/a8a30ced4805221b1de162074ae85765e751c3ce))
* JSON vacuous flags; truncate verification display names ([#828](https://github.com/assura-lang/assura/issues/828)) ([caa1455](https://github.com/assura-lang/assura/commit/caa145577a44b49525b16a16f5cfed4c74082a1c))
* order crates.io publish by all path deps including dev ([#801](https://github.com/assura-lang/assura/issues/801)) ([62662f8](https://github.com/assura-lang/assura/commit/62662f87a6d82bcaa995710a2abcc8141a1ba20d))
* publish-plan order, CI wire-up, vacuous check message ([#818](https://github.com/assura-lang/assura/issues/818)) ([4686a89](https://github.com/assura-lang/assura/commit/4686a89ec0fc7ba0633d5add8f7df3adeb8a18a5))
* retry crates.io 429 and space new crate publishes ([#803](https://github.com/assura-lang/assura/issues/803)) ([e7cc20c](https://github.com/assura-lang/assura/commit/e7cc20c02d5defac2e6f6c7a9cfdd69450334171))
* ship IR prompt templates inside assura-smt for crates.io ([#805](https://github.com/assura-lang/assura/issues/805)) ([3bd06f0](https://github.com/assura-lang/assura/commit/3bd06f08d786e170c55b9e97dc31b36bbd12bbe5))

## 0.1.0 (2026-07-04)


### Bug Fixes

* address issues [#328](https://github.com/assura-lang/assura/issues/328), [#329](https://github.com/assura-lang/assura/issues/329), [#330](https://github.com/assura-lang/assura/issues/330) ([cb9e150](https://github.com/assura-lang/assura/commit/cb9e150158e52a261435ec09b44d98d5293891df))
* address reviewer findings [#707](https://github.com/assura-lang/assura/issues/707)-[#712](https://github.com/assura-lang/assura/issues/712) ([5b165c0](https://github.com/assura-lang/assura/commit/5b165c03d962c82e87126d69c96f8dcc4f1d2029))
* apply cargo fmt to all workspace files ([362bc3b](https://github.com/assura-lang/assura/commit/362bc3b534134252b4f2c94af7bf3b5a9d87c933))
* clean up 5 unused import warnings in assura-smt test files ([f79189e](https://github.com/assura-lang/assura/commit/f79189edc605eec43a05748a623c717837d3127e))
* fmt binop to use binop_str for &&/|| in Assura syntax ([e3fd903](https://github.com/assura-lang/assura/commit/e3fd9038f62db68ce45ecdc1a17fea308fac5933))
* fmt dead code and imports for gate ([94a85a9](https://github.com/assura-lang/assura/commit/94a85a92b62d25e1426fd7737f8d411302f10063))
* force first release-please cut to 0.1.0 ([#782](https://github.com/assura-lang/assura/issues/782)) ([e37ab31](https://github.com/assura-lang/assura/commit/e37ab31c2fc66082cc304d6cc37175a7c1390d7f))
* formatter idempotency, test coverage, and code cleanup ([#682](https://github.com/assura-lang/assura/issues/682)) ([953c6f2](https://github.com/assura-lang/assura/commit/953c6f2f618bb4c33d5768bbd75f8234b96de049))
* MPI cycle 1 — A31007 fairness, vacuous check UX, dead checker cleanup ([#768](https://github.com/assura-lang/assura/issues/768)) ([d300daa](https://github.com/assura-lang/assura/commit/d300daa5853f191ea1b2afda01f3bfe5b9983f48))
* multi-perspective improvement cycle (tests, CI, docs) ([#698](https://github.com/assura-lang/assura/issues/698)) ([d5a3e51](https://github.com/assura-lang/assura/commit/d5a3e51af6668ec1b8a3ff895d5dc8241cd18138))
* multi-perspective improvement cycle 1 ([#646](https://github.com/assura-lang/assura/issues/646)) ([7b9107e](https://github.com/assura-lang/assura/commit/7b9107e1a71850a64c635b95138a4c2e7101e94b))
* ProjectConfig import + SMT-LIB shell-out bugs ([cbb66fb](https://github.com/assura-lang/assura/commit/cbb66fb99352ebcd31324b2514e9c2beca7c7200))
* remove unused SpExpr import after cvc5 migration ([#320](https://github.com/assura-lang/assura/issues/320)) ([49e4264](https://github.com/assura-lang/assura/commit/49e4264adb9bec5d453e13becbf156014a2af73a))
* resolve clippy warnings, fix cargo fmt with cvc5 test module path ([640355d](https://github.com/assura-lang/assura/commit/640355ddb3ed9235c5b1cfdbf35e93e30927206c))
* resolve issues [#316](https://github.com/assura-lang/assura/issues/316), [#320](https://github.com/assura-lang/assura/issues/320), [#321](https://github.com/assura-lang/assura/issues/321), [#322](https://github.com/assura-lang/assura/issues/322) ([6fb29c7](https://github.com/assura-lang/assura/commit/6fb29c77470064c1fd22d0f0bd53ac0590ebe354))
* use canonical type map in check-rust, implement --public-only, document check-rust ([cf2049e](https://github.com/assura-lang/assura/commit/cf2049e225e2309753dcde57384ccccbc70f49b3))
* use simple release-please type for cargo workspace ([#780](https://github.com/assura-lang/assura/issues/780)) ([f703991](https://github.com/assura-lang/assura/commit/f7039910b18e20c5589ee9fa0b465171bd52c56e))
* wire liveness monitor state enums (closes [#770](https://github.com/assura-lang/assura/issues/770)) ([#771](https://github.com/assura-lang/assura/issues/771)) ([8f043cd](https://github.com/assura-lang/assura/commit/8f043cd89d2293c3857c731d635378cff4df6cf0))
* Z3Value soundness fixes, dead code removal, dep bumps ([#515](https://github.com/assura-lang/assura/issues/515)) ([ec58897](https://github.com/assura-lang/assura/commit/ec588970bc18df66f049e7151b48dea122f94064))

### Initial public description (historical notes, 2025-06-14)

Initial release of the Assura compiler.

### Compiler Pipeline

- **Parser**: lexer (logos) + recursive-descent parser (rowan CST) with
  full Pratt expression parsing (8 precedence levels)
- **Name Resolution**: symbol table, scope analysis, cross-reference tracking
- **Type Checker**: 50+ domain-specific checkers covering all 12 feature
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

[Unreleased]: https://github.com/assura-lang/assura/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/assura-lang/assura/releases/tag/v0.1.0
