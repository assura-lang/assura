# Assura Compiler Internals

This document covers the architecture and internal design of the Assura
compiler. It is intended for contributors who want to understand the
codebase, add new features, or fix bugs.

## Pipeline Overview

The compiler processes `.assura` source files through a linear pipeline:

```
Source (.assura)
  |
  v
Lexer (logos 0.16)           crates/assura-parser/src/lexer.rs
  | produces tokens via logos derive
  v
Parser (rowan 0.16 CST)      crates/assura-parser/src/cst.rs
  | hand-written recursive descent + Pratt expression parsing
  | produces GreenNode (lossless concrete syntax tree)
  v
CST -> AST Lowering          crates/assura-parser/src/lower.rs
  | produces SourceFile (AST)
  | uses helpers (spanned, missing_expr, lower_expr_children, etc.)
  | to avoid boilerplate (see AGENTS.md "Lowering Helpers")
  v
Name Resolution              crates/assura-resolve/src/lib.rs
  | produces ResolvedFile + SymbolTable
  v
Type Checking                crates/assura-types/src/lib.rs
  | produces TypedFile + Vec<TypeError>
  v
SMT Verification (Z3/CVC5)  crates/assura-smt/src/lib.rs
  | produces Vec<VerificationResult>
  v
Code Generation              crates/assura-codegen/src/lib.rs
  | produces GeneratedProject (Rust source files)
  v
rustc (external)
```

The CLI (`assura check`) runs the pipeline through SMT verification.
The CLI (`assura build`) runs the full pipeline including codegen and
optionally invokes `cargo check` on the generated Rust project.

## Crate Map

| Crate | LOC | Tests | Purpose |
|-------|-----|-------|---------|
| `assura-parser` | 8,100 | 149 | Lexer (logos 0.16), CST (rowan 0.16), recursive descent parser, Pratt expressions, CST-to-AST lowering |
| `assura-resolve` | 4,300 | 91 | Name resolution, scope analysis, symbol table |
| `assura-types` | 33,800 | 1,081 | Type checking, 50+ domain-specific checkers |
| `assura-smt` | 13,800 | 397 | Z3/CVC5 SMT solver integration, verification |
| `assura-codegen` | 7,200 | 159 | Rust code generation via prettyplease |
| `assura-diagnostics` | 2,000 | 23 | Unified Diagnostic type, error catalog with O(1) lookup |
| `assura-cli` | 6,200 | 74 | CLI binary (check, build, init, fmt, explain, infer, audit, diff, REPL) |
| `assura-fmt` | 1,300 | 56 | Source code formatter |
| `assura-config` | 900 | 34 | `assura.toml` configuration parsing |
| `assura-pipeline` | 400 | 7 | Orchestrates multi-file compilation pipeline |
| `assura-macros` | 300 | 20 | `#[contract]` and `#[trust]` proc macros for Rust interop |
| `assura-stdlib` | 300 | 13 | Standard library type and contract definitions |
| `assura-lsp` | 2,000 | 55 | Language Server Protocol (tower-lsp 0.20) |
| `assura-mcp` | 600 | 24 | Model Context Protocol server (rmcp 1.7) |
| `assura-server` | 800 | 27 | gRPC (tonic 0.14) + HTTP (axum 0.8) API server |
| `assura-rust-analyzer` | 1,600 | 40 | Rust source analysis for `assura infer` and `assura audit` |
| `assura-bench` | 2 | - | Criterion benchmarks for all pipeline stages |
| **Total** | **~85,000** | **2,334** | |

## Crate Details

### assura-parser

**Entry point:** `assura_parser::parse(source: &str) -> SourceFile`

**Note on layering (Phase 11):** The canonical AST types live in `assura-ast` (the compiler IR crate). `assura-parser` re-exports them for convenience (`assura_parser::ast`). Downstream crates (`assura-codegen`, `assura-smt`) depend only on `assura-ast` (plus `assura-types`/`assura-resolve`) to avoid parser layering violations. `expr_to_string` and the `Feature` registry were moved to `assura-ast` as part of this.

Also: `parse_unwrap(source)` (panics on error, for tests) and
`parse_cst(source) -> (GreenNode, Vec<ParseError>)` (raw CST access).

The parser uses a three-stage architecture:

1. **Lexing** (`lexer.rs`): `logos` 0.16 derive macro tokenizes ~200
   token types (keywords, operators, literals).
2. **CST construction** (`cst.rs`): Hand-written recursive descent
   parser builds a lossless rowan `GreenNode` tree using an
   events/markers pattern (Open/Close/Advance). Expressions use Pratt
   parsing with 8 binding power levels.
3. **Lowering** (`lower.rs`): Converts the CST `SyntaxNode` tree into
   a typed AST (`SourceFile`).

**Key types:**
- `lexer::Token`: All ~200 token types (keywords, operators, literals)
- `syntax_kind::SyntaxKind`: Rowan node/token kinds, with `From<&Token>`
- `ast::SourceFile`: Top-level AST node containing `Vec<Spanned<Decl>>`
- `ast::Decl`: Declaration variants (Contract, Service, TypeDef, EnumDef,
  Extern, FnDef, Block, Import, Module, Bind, Trait)
- `ast::Clause`: Contract clause with kind and body
- `ast::Expr`: Expression AST (22 variants: literals, binary ops, calls,
  quantifiers, match, let, field access, index, etc.)
- `ast::Literal`: Literal values (Int, Float, Str, Bool)

**Source files:**
- `lexer.rs`: Token enum with `#[derive(Logos)]`, ~200 keyword mappings
- `syntax_kind.rs`: `SyntaxKind` enum for rowan, `AssuraLanguage` trait impl
- `cst.rs`: Parser engine with events/markers, `GreenNodeBuilder`
- `grammar/mod.rs`: Top-level grammar (source_file, project, module, import)
- `grammar/items.rs`: Declaration grammar (contract, type, enum, fn, service, extern, bind, trait)
- `grammar/clauses.rs`: Clause grammar (requires, ensures, invariant, effects, etc.)
- `grammar/expressions.rs`: Pratt expression parser (8 precedence levels)
- `grammar/params.rs`: Parameter lists, return types, type parameters
- `ast.rs`: All AST node types, `Spanned<T>` wrapper
- `lower.rs`: CST-to-AST lowering
- `display.rs`: Human-readable Display impls for AST nodes
- `lib.rs`: `parse()` entry point wiring lex + CST + lower

**Important patterns:**
- All AST nodes carry `Span = Range<usize>` (byte offsets)
- The CST is lossless (whitespace, comments preserved for formatting)
- `Expr::Raw(Vec<String>)` only appears in non-expression clause bodies
  (input, output, effects); expression clauses (requires, ensures,
  invariant, decreases) always produce structured `Expr`

### assura-resolve

**Entry point:** `assura_resolve::resolve(source: &SourceFile) -> Result<ResolvedFile, Vec<ResolutionError>>`

Builds a symbol table and resolves all name references.

**Key types:**
- `SymbolTable`: Collection of `Symbol` entries with scope hierarchy
- `Symbol`: Name, kind, span, scope index, and optional type info
- `Scope`: Contains symbols and a parent scope index
- `ResolvedFile`: Original `SourceFile` + `SymbolTable` + resolved imports
- `ResolutionError`: Error with code (A02xxx), message, and span

**Multi-file support:** `resolve_with_modules()` accepts a `ModuleMap`
for cross-file resolution.

### assura-types

**Entry point:** `assura_types::type_check(resolved: &ResolvedFile) -> Result<TypedFile, Vec<TypeError>>`

The largest crate (33,800 lines). Runs 50+ checkers organized into phases:

**Source files:**
- `lib.rs`: Entry point, `Type` enum, `TypeEnv`, core checker wiring
- `checkers.rs`: 20+ analysis pass checkers (linearity, typestate, effects,
  taint, totality, etc.)
- `checkers/interface.rs`: Interface conformance checking
- `domain.rs`: 34 domain-specific checkers (allocators, crypto,
  concurrency, formats, storage, etc.)
- `inference.rs`: Expression type inference
- `clauses.rs`: Clause body type checking
- `tests.rs`: Unit tests

**Key types:**
- `Type`: All type variants (Int, Nat, Float, Bool, String, Generic,
  Refined, Linear, Typestate, Effect, etc.) with `is_indeterminate()`
  for `Unknown`/`Error` handling
- `TypeEnv`: Type environment mapping names to `Type`
- `TypedFile`: Type-checked output with `resolved`, `typed_bindings`,
  `pending_decrease_checks`
- `TypeError`: Error with code, severity, message, primary span,
  secondary spans

**Checker phases (executed in order):**
1. Expression type inference
2. Contract clause checking
3. Generic instantiation
4. Pattern exhaustiveness
5. Linearity (linear type usage tracking, context splitting)
6. Typestate (DFA state transition checking)
7. Effects (effect set containment, call-graph inference)
8. Information flow (security label propagation)
9. Totality (termination checking with SMT-backed decrease verification)
10. Domain-specific checkers (30+ specialized analyzers)

### assura-smt

**Entry point:** `assura_smt::verify(typed: &TypedFile) -> Vec<VerificationResult>`

Callers outside this crate should prefer `assura_pipeline::verify_typed` /
`compile_full` (CLI, LSP, MCP, tests via `assura_test_support::verify_ok`).

Z3 integration behind the `z3-verify` feature flag (enabled by default).
CVC5 fallback via external binary in portfolio mode.

**Key types:**
- `VerificationResult`: Enum with `Verified`, `Counterexample`,
  `Timeout`, `Unknown`, `Skipped`, `Error` variants
- `CounterexampleModel`: Structured counterexample with variable assignments
- `MeasureDefinition`: Termination measure for recursion checking
- `Encoder`: Translates `Expr` into Z3 AST (arithmetic, comparisons,
  quantifiers, field access, function calls)

**Module map (agent edit surface — encode here / verify here / result here):**

| Area | Path | Edit here for… |
|------|------|----------------|
| Public verify API | `entry/` (`verify.rs`, `jobs.rs`, `advanced_passes.rs`, `helpers.rs`, `evolution.rs`) | `verify()`, job collection, advanced passes, decrease dispatch |
| CLI check command | `assura-cli/src/check/` (`run.rs`, `report.rs`, `watch.rs`, `project.rs`, `check_rust.rs`) | `assura check` / verify reporting / watch / project |
| Results / limitation marker | `result.rs` | `VerificationResult`, `KNOWN_SMT_LIMITATION_MARKER` |
| Managers (prophecy, trigger, weak memory) | `advanced.rs` | New manager methods (must call from entry/encoder, not tests only) |
| Z3 solve loop | `z3_backend/verify.rs` | Per-clause solve, timeouts, portfolio |
| Z3 encoding | `z3_backend/encoder/` (`value`, `core_impl`, `methods`, `unmodelable`, `bitvector`) | Expr → Z3 AST; edit `methods` for `encode_expr`/raw/binop, `core_impl` for ADT/call/field |
| CVC5 | `cvc5_backend.rs` (+ `cvc5_*`) | CVC5 parity / shell-out |
| IR / layer 2 | `ir_*.rs`, `layer2.rs` | Intermediate IR, quantifier layer |
| SMT-LIB dump | `smt_dump.rs` | `--dump-smt` offline scripts |
| Display / stats | `display.rs` | Contract name collection (`DeclVisitor`), verify stats |
| Measures / termination | `measures.rs` | Decrease / measure definitions |

**Agent rule:** add SMT behavior in the row above, then wire from `entry/mod.rs` or
`z3_backend/encoder`. `scripts/guards.sh` section 7 fails if high-signal
methods exist only in `advanced.rs` / tests.

**Source files (legacy list, still accurate):**
- `lib.rs`: Crate root, re-exports, feature-gated backends
- `z3_backend/`: Z3-specific encoding and solving (split modules)
- `cvc5_backend.rs`: CVC5 SMT-LIB output and external binary invocation
- `layer2.rs`: Layer 2 quantifier verification (10s timeout)
- `advanced.rs`: Advanced verification (prophecy variables, triggers)
- `display.rs`: Contract name collection and stats

**Verification layers:**
- Layer 1 (1s timeout): Quantifier-free (QF_UFLIA, QF_UFLRA)
- Layer 2 (10s timeout): With quantifiers (AUFLIA)

**Graceful fallback:** When compiled without `z3-verify`, all verification
functions return `VerificationResult::Skipped` with a message.

**assura-types layering** (checks vs checkers vs domain): see
`crates/assura-types/src/CHECKER-LAYERS.md` and the AGENTS ergonomics map.

### assura-codegen

**Entry point:** `assura_codegen::codegen(typed: &TypedFile) -> GeneratedProject`

Generates a Cargo project with valid Rust source code.

**Key types:**
- `GeneratedProject`: `Cargo.toml` content + list of `GeneratedFile`
- `BackendConfig`: Target, output directory, feature flags
- `CodegenBackend`: Native or Wasm target

**Source files:**
- `lib.rs`: Main codegen logic, declaration iteration, Cargo.toml generation
- `block.rs`: Block-level code generation, `format_rust()` via prettyplease
- `type_map.rs`: Reverse type mapping (Rust -> Assura) for `assura infer`

**What gets generated:**
- `Cargo.toml` with dependencies (proptest in dev-deps for tests)
- `src/lib.rs` (single-contract files) or multi-file layout
- Struct/enum definitions from AST `TypeDef`, `EnumDef`
- Function stubs with `todo!()` bodies
- `debug_assert!` from `requires` clauses
- Typestate-encoded services (`PhantomData<State>` pattern)
- Proptest property-based tests from `ensures` clauses
- Checked wrappers for `bind` declarations

### assura-diagnostics

Unified `Diagnostic` type used by all compiler passes:

```rust
pub struct Diagnostic {
    pub code: String,           // e.g., "A03001"
    pub severity: Severity,     // Error, Warning, Info
    pub message: String,
    pub primary: Range<usize>,  // primary span
    pub secondary: Vec<(Range<usize>, String)>,
    pub suggestion: Option<Suggestion>,
}
```

`From` conversions exist for `ResolutionError` and `TypeError`.

The error catalog provides `explain(code)` with O(1) HashMap lookup for
all ~278 error codes defined in the spec.

### assura-cli

The CLI binary (`assura`) with subcommands:
- `assura check <file>`: Parse, resolve, type-check, verify (exit 1 on errors)
- `assura build <file>`: Full pipeline + codegen + optional `cargo check`
- `assura init`: Scaffold a new `.assura` project
- `assura fmt <file>`: Format source with consistent style
- `assura explain <code>`: Explain an error code
- `assura infer <file.rs>`: Generate skeleton Assura bind contracts from
  a Rust source file
- `assura audit <path>`: Scan a Cargo crate, discover public functions,
  generate skeleton contracts, and verify them
- `assura diff <a> <b>`: Compare two `.assura` files structurally
- REPL mode: Interactive contract evaluation

**Flags:** `--verbose` (`-v`), `--quiet` (`-q`), `--watch` (`-w`),
`--output <dir>`, `--no-check`, `--ast`, `--tokens`, `--json`

### assura-fmt

Source code formatter for `.assura` files. Produces consistently
styled output with proper indentation, clause alignment, and
whitespace normalization.

### assura-config

Parses `assura.toml` project configuration files. Handles solver
timeouts, codegen backend selection, and project-level settings.

### assura-pipeline

Orchestrates multi-file compilation. Discovers `.assura` files,
resolves cross-module imports, and runs the pipeline on each file
in dependency order.

**`verify_ir` / multi-contract selection (#853):**
`assura_pipeline::verify_ir(source, ir, config)` validates IR against the
**first** `Decl::Contract` in the source (historical default). For files
with multiple contracts, use
`verify_ir_for_contract(source, ir, config, Some("ContractName"))` so
structural validation and IR extras target that contract; SMT results are
filtered to its clauses. Auto-implement historically worked around this by
building single-contract source via `build_single_contract_source()`.

**Fixed-width signedness (Z3):** Parameters and results registered as
`U8`/`U16`/… use unsigned BV order; `I8`/`I32`/… use signed order for
comparisons. Modular BV add/sub/mul is the same for both. CVC5 still uses
unsigned order for all BV comparisons (tracked in #858).

### assura-macros

Procedural macros for Rust interop:
- `#[contract]`: Generates `debug_assert!` from `@requires` /
  `@ensures` annotations on Rust functions
- `#[trust]`: Marks a function as trusted (skips verification)

### assura-stdlib

Standard library definitions. Provides built-in type and contract
definitions (numeric types, collections, Option, Result) that are
implicitly available in all `.assura` files.

### assura-lsp

Language Server Protocol server built with `tower-lsp` 0.20:
- `textDocument/diagnostic`: Parse/type errors as diagnostics
- `textDocument/hover`: Type info on hover
- `textDocument/definition`: Go to symbol definition
- `textDocument/completion`: Keyword and type completions
- `textDocument/documentSymbol`: Document outline

### assura-mcp

Model Context Protocol server built with `rmcp` 1.7:
- `check`: Run the compiler pipeline on source text
- `explain`: Look up error code descriptions
- `list_declarations`: List contracts and types in a file

### assura-server

gRPC (tonic 0.14) + HTTP (axum 0.8) API server:
- `Check` RPC: Type-check source and return diagnostics
- `Build` RPC: Generate Rust code from source
- `Explain` RPC: Look up error code descriptions
- `Health` RPC: Server health check
- HTTP endpoints mirror gRPC RPCs for REST clients

### assura-rust-analyzer

Rust source file analysis for `assura infer` and `assura audit`.
Parses Rust files with `syn`, extracts function signatures, and
converts Rust types to Assura types via the reverse type mapping
in `assura-codegen/src/type_map.rs`.

## How to Add a New Checker to assura-types

1. **Choose the right file:**
   - Core analysis (linearity, effects, typestate): `checkers.rs`
   - Domain-specific (memory, security, etc.): `domain.rs`

2. **Define the checker struct:**
   ```rust
   pub struct MyNewChecker {
       pub errors: Vec<TypeError>,
   }

   impl MyNewChecker {
       pub fn new() -> Self {
           Self { errors: Vec::new() }
       }

       pub fn check(&mut self, file: &ResolvedFile) {
           for decl in &file.source.decls {
               // Analyze declarations, emit errors via self.errors.push(...)
           }
       }
   }
   ```

3. **Define error codes** following the spec's scheme (Appendix D):
   ```rust
   self.errors.push(TypeError {
       code: "AXXXXX".to_string(),
       severity: "error".to_string(),
       message: "description".to_string(),
       primary_label: "what went wrong here".to_string(),
       span: some_span.clone(),
       secondary: vec![],
   });
   ```

4. **Wire into the pipeline** in `lib.rs`:
   ```rust
   // In type_check() function, after existing checkers:
   let mut my_checker = MyNewChecker::new();
   my_checker.check(&resolved);
   errors.extend(my_checker.errors);
   ```

5. **Add tests** in `tests.rs`:
   ```rust
   #[test]
   fn my_checker_detects_violation() {
       let source = r#"
           contract Bad {
               // ... contract that should trigger the error
           }
       "#;
       let errors = type_check_source(source);
       assert!(errors.iter().any(|e| e.code == "AXXXXX"));
   }
   ```

6. **Add MUST REJECT fixtures** in `tests/fixtures/must_reject/`:
   Create a `.assura` file with `// MUST REJECT AXXXXX` annotation.

## How to Add a New SMT Encoding to assura-smt

1. **Add a public verification function:**
   ```rust
   pub fn verify_my_property(/* inputs */) -> VerificationResult {
       #[cfg(feature = "z3-verify")]
       { verify_my_property_impl(/* inputs */) }
       #[cfg(not(feature = "z3-verify"))]
       { VerificationResult::Skipped("Z3 not available".into()) }
   }
   ```

2. **Implement the Z3 encoding:**
   ```rust
   #[cfg(feature = "z3-verify")]
   fn verify_my_property_impl(/* inputs */) -> VerificationResult {
       let cfg = z3::Config::new();
       let ctx = z3::Context::new(&cfg);
       let solver = z3::Solver::new(&ctx);

       // Set timeout
       let params = z3::Params::new(&ctx);
       params.set_u32("timeout", 1000);
       solver.set_params(&params);

       // Encode the property
       let x = z3::ast::Int::new_const(&ctx, "x");
       // ... build Z3 AST ...

       // Check satisfiability of the negation (to prove validity)
       solver.assert(&negated_property);
       match solver.check() {
           z3::SatResult::Unsat => VerificationResult::Verified,
           z3::SatResult::Sat => {
               let model = solver.get_model().unwrap();
               let ce = extract_counter_model(&model, &ctx);
               VerificationResult::Counterexample(ce)
           }
           z3::SatResult::Unknown => VerificationResult::Unknown,
       }
   }
   ```

3. **Wire into the verification pipeline** (in `verify()` or
   `verify_contract()`), or call directly from the type checker.

4. **Add tests** that cover: verified (property holds), counterexample
   (property fails with concrete values), and timeout/unknown cases.

5. **Use the existing `Encoder`** struct (in `assura-smt/src/lib.rs`)
   for translating `Expr` into Z3 AST. It handles arithmetic, comparisons,
   quantifiers, field access, and function calls.

## How to Add a New Codegen Pass

1. **Identify what to generate** (new Rust construct, new file, new
   dependency in Cargo.toml).

2. **Modify `codegen()` or `codegen_with_config()`** in
   `assura-codegen/src/lib.rs`. The function iterates over declarations
   in the `TypedFile`.

3. **Generate Rust source as a string**, then validate with
   `syn::parse_file()` to ensure syntactic correctness.

4. **For new files**, add entries to the `GeneratedProject.files` vector.

5. **For new dependencies**, modify the `cargo_toml` generation section.

6. **Format the output** using `prettyplease::unparse()` for consistent
   Rust formatting.

7. **Add tests** using the `codegen_ok()` helper that runs the full
   pipeline (parse -> resolve -> typecheck -> codegen) and validates
   the generated Rust is syntactically valid.

## Error Code Scheme

| Range | Category | Crate |
|-------|----------|-------|
| A01xxx | Syntax errors | assura-parser |
| A02xxx | Name resolution | assura-resolve |
| A03xxx | Type checking | assura-types |
| A05xxx | Linearity | assura-types (checkers.rs) |
| A06xxx | Typestate | assura-types (checkers.rs) |
| A07xxx | Effects | assura-types (checkers.rs) |
| A08xxx | Information flow | assura-types (checkers.rs) |
| A09xxx | Totality | assura-types (checkers.rs) |
| A10xxx | Pattern exhaustiveness | assura-types (lib.rs) |
| A13xxx | Interface conformance | assura-types (checkers/interface.rs) |
| A22xxx-A55xxx | Domain-specific | assura-types (domain.rs) |

## Building and Testing

```bash
# Build all crates
cargo build --workspace

# Run all tests (2,334)
cargo test --workspace

# Run clippy (required to pass before commit)
cargo clippy --workspace -- -D warnings

# Format check
cargo fmt --check --all

# Run benchmarks
cargo bench -p assura-bench

# Run the CLI
cargo run --bin assura -- check demos/libwebp-huffman.assura
cargo run --bin assura -- build demos/libwebp-huffman.assura
cargo run --bin assura -- fmt demos/libwebp-huffman.assura --check

# Full pre-commit gate
cargo fmt --all && cargo clippy --workspace -- -D warnings && cargo test --workspace
```

## Key Libraries

| Library | Version | Used For |
|---------|---------|----------|
| logos | 0.16 | Lexer (derive macro) |
| rowan | 0.16 | Lossless concrete syntax tree |
| ariadne | 0.6 | Error display |
| z3 | 0.20 | SMT solver bindings (optional, behind `z3-verify` feature) |
| prettyplease | 0.2 | Rust source formatting in codegen |
| syn | 2 | Rust AST validation and source analysis |
| tower-lsp | 0.20 | LSP server framework |
| rmcp | 1.7 | MCP server framework |
| tonic | 0.14 | gRPC server |
| axum | 0.8 | HTTP server |
| criterion | 0.8 | Benchmarks |
| notify | 8 | Filesystem watching (--watch) |

**Key crate versions:** ariadne 0.6 uses `Report::build(kind, span)` with
a 2-arg API; z3 0.20 uses pre-generated FFI bindings (no bindgen at build
time) and removes lifetime parameters from AST types.