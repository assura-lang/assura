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
Lexer (logos 0.15)           crates/assura-parser/src/lexer.rs
  | produces Vec<(Token, Span)>
  v
Parser (chumsky 0.9)         crates/assura-parser/src/parser.rs
  | produces SourceFile (AST)
  v
Name Resolution              crates/assura-resolve/src/lib.rs
  | produces ResolvedFile + SymbolTable
  v
HIR Lowering                 crates/assura-hir/src/lower.rs
  | produces HirFile (desugared, name-resolved)
  v
Type Checking                crates/assura-types/src/lib.rs
  | produces TypedFile + Vec<TypeError>
  v
SMT Verification (Z3)       crates/assura-smt/src/lib.rs
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
| `assura-parser` | ~2,800 | ~50 | Lexing (logos) + parsing (chumsky 0.9) |
| `assura-resolve` | ~3,400 | ~80 | Name resolution, scope analysis, symbol table |
| `assura-hir` | ~800 | ~23 | High-level IR, AST-to-HIR lowering |
| `assura-types` | ~25,000 | ~860 | Type checking, 50+ domain-specific checkers |
| `assura-smt` | ~7,000 | ~170 | Z3 SMT solver integration, verification |
| `assura-codegen` | ~3,300 | ~100 | Rust code generation via prettyplease |
| `assura-diagnostics` | ~200 | ~6 | Unified Diagnostic type for all passes |
| `assura-cli` | ~2,400 | ~45 | CLI binary (check, build, init, fmt, explain) |
| `assura-lsp` | ~850 | ~33 | Language Server Protocol (tower-lsp) |
| `assura-server` | ~500 | ~26 | gRPC (tonic) + HTTP (axum) API server |
| `assura-bench` | ~170 | - | Criterion benchmarks for all pipeline stages |

## Crate Details

### assura-parser

**Entry point:** `assura_parser::parse(source: &str) -> (Option<SourceFile>, Vec<Simple<Token>>)`

The parser performs lexing and parsing in one call. It uses `logos` for
tokenization and `chumsky` 0.9 parser combinators for grammar parsing.

**Key types:**
- `lexer::Token`: All ~200 token types (keywords, operators, literals)
- `ast::SourceFile`: Top-level AST node containing `Vec<Spanned<Decl>>`
- `ast::Decl`: Declaration variants (Contract, Service, TypeDef, EnumDef, Extern, FnDef, Block, Import)
- `ast::Clause`: Contract clause with kind and body
- `ast::Expr`: Expression AST (literals, binary ops, calls, quantifiers, etc.)
- `ast::Literal`: Literal values (Int, Float, String, Bool, Char)

**Source files:**
- `lexer.rs`: Token enum with `#[derive(Logos)]`, keyword mappings
- `ast.rs`: All AST node types, `Spanned<T>` wrapper, shared utilities
- `parser.rs`: chumsky combinators for all grammar productions
- `lib.rs`: `parse()` function that wires lex + parse

**Important patterns:**
- All AST nodes carry `Span = Range<usize>` (byte offsets)
- The parser uses `parse_recovery()` for error recovery
- `Expr::Raw(Vec<String>)` only appears in non-expression clause bodies (input, output, effects); expression clauses (requires, ensures, invariant, decreases) always produce structured `Expr`

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

### assura-hir

**Entry point:** `assura_hir::lower(resolved: &ResolvedFile) -> HirFile`

Lowers the AST into a High-level IR with:
1. Names resolved to `DefId` (index into the symbol table or unresolved)
2. Raw token type sequences converted to structured `HirType`
3. Expressions preserved as `HirExpr` (mirrors `ast::Expr` with `DefId`)
4. Normalized clause representations

**Key types:**
- `HirFile`: File with `Vec<HirDecl>` and reference to `ResolvedFile`
- `DefId`: `Resolved(usize)` (symbol table index) or `Unresolved(String)`
- `HirType`: Structured type (Named, Generic, Tuple, Fn, Refined, Unit)
- `HirExpr`: Expression with `DefId`-based name resolution

**Backward compatibility:** `HirExpr::to_ast_expr()` and
`HirClause::to_ast_clause()` convert back to AST types, allowing the
type checker to continue operating on the original representations
during migration.

### assura-types

**Entry point:** `assura_types::type_check(resolved: &ResolvedFile) -> Result<TypedFile, Vec<TypeError>>`

The largest crate. Runs 50+ checkers organized into phases:

**Source files:**
- `lib.rs` (~3,000 lines): Entry point, `Type` enum, `TypeEnv`, core wiring
- `checkers.rs` (~5,700 lines): 20+ analysis pass checkers (linearity, typestate, effects, taint, totality, etc.)
- `domain.rs` (~3,800 lines): 34 domain-specific checkers (allocators, crypto, concurrency, formats, storage, etc.)
- `inference.rs` (~860 lines): Expression type inference
- `clauses.rs` (~540 lines): Clause body type checking
- `tests.rs` (~11,000 lines): All unit tests

**Key types:**
- `Type`: All type variants (Int, Nat, Float, Bool, String, Generic, Refined, Linear, Typestate, etc.)
- `TypeEnv`: Type environment mapping names to `Type`
- `TypedFile`: Type-checked output with `resolved`, `typed_bindings`, `pending_decrease_checks`
- `TypeError`: Error with code, severity, message, primary span, secondary spans

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

Z3 integration behind the `z3-verify` feature flag (enabled by default).

**Key types:**
- `VerificationResult`: Verified, Counterexample(model), Timeout, Unknown, Skipped, Error
- `CounterexampleModel`: Structured counterexample with variable assignments
- `MeasureDefinition`: Termination measure for recursion checking

**Public functions:**
- `verify(typed)`: Verify all contracts in a typed file
- `verify_contract(typed, contract_name)`: Verify a single contract
- `check_refinement_subtype(antecedent, consequent)`: Subtype check via Z3
- `verify_buffer_bounds(requires, ensures)`: Buffer safety verification
- `verify_taint_safety(...)`: Taint tracking verification
- `verify_decrease(...)`: Termination measure decrease verification
- `verify_quantified_expr(...)`: Layer 2 quantifier verification (10s timeout)
- `validate_quantifier_bounds(typed)`: Check for unbounded quantifiers

**Verification layers:**
- Layer 1 (1s timeout): Quantifier-free (QF_UFLIA, QF_UFLRA)
- Layer 2 (10s timeout): With quantifiers (AUFLIA)

**Graceful fallback:** When compiled without `z3-verify`, all verification
functions return `VerificationResult::Skipped` with a message.

### assura-codegen

**Entry point:** `assura_codegen::codegen(typed: &TypedFile) -> GeneratedProject`

Generates a Cargo project with valid Rust source code.

**Key types:**
- `GeneratedProject`: `Cargo.toml` content + list of `GeneratedFile`
- `BackendConfig`: Target, output directory, feature flags
- `CodegenBackend`: Native or Wasm target

**What gets generated:**
- `Cargo.toml` with dependencies (proptest in dev-deps if contracts have tests)
- `src/lib.rs` (single-contract files) or multi-file layout
- Struct/enum definitions from AST `TypeDef`, `EnumDef`
- Function stubs with `todo!()` bodies
- `debug_assert!` from `requires` clauses
- Typestate-encoded services (`PhantomData<State>` pattern)
- Proptest property-based tests from `ensures` clauses
- `feature_max` constants

**Multi-file layout (2+ contracts/services):**
```
src/lib.rs                  // shared types, pub mod declarations
src/contract_{name}.rs      // per-contract module
src/{service_name}.rs       // per-service module
```

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

### assura-cli

The CLI binary with subcommands:
- `assura check <file>`: Parse, resolve, type-check, verify (exit 1 on errors)
- `assura build <file>`: Full pipeline + codegen + optional `cargo check`
- `assura init`: Scaffold a new `.assura` project
- `assura fmt <file>`: Format source with consistent style
- `assura explain <code>`: Explain an error code

**Flags:** `--verbose` (`-v`), `--quiet` (`-q`), `--watch` (`-w`),
`--output <dir>`, `--no-check`, `--ast`, `--tokens`, `--json`

### assura-lsp

Language Server Protocol server built with `tower-lsp`:
- `textDocument/diagnostic`: Parse/type errors as diagnostics
- `textDocument/hover`: Type info on hover
- `textDocument/definition`: Go to symbol definition
- `textDocument/completion`: Keyword and type completions
- `textDocument/documentSymbol`: Document outline

### assura-server

gRPC (tonic) + HTTP (axum) API server:
- `Check` RPC: Type-check source and return diagnostics
- `Build` RPC: Generate Rust code from source
- `Explain` RPC: Look up error code descriptions
- `Health` RPC: Server health check
- HTTP endpoints mirror gRPC RPCs for REST clients

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
| A22xxx-A55xxx | Domain-specific | assura-types (domain.rs) |

## Building and Testing

```bash
# Build all crates
cargo build --workspace

# Run all tests (1,395+)
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
| logos | 0.15 | Lexer (derive macro) |
| chumsky | 0.9 | Parser combinators (NOT 0.10+) |
| ariadne | 0.4 | Error display (NOT 0.5+) |
| z3 | 0.12 | SMT solver bindings |
| prettyplease | 0.2 | Rust source formatting |
| syn | 2 | Rust AST validation |
| tower-lsp | latest | LSP server framework |
| tonic | latest | gRPC server |
| axum | latest | HTTP server |
| criterion | 0.5 | Benchmarks |
| notify | 7 | Filesystem watching (--watch) |

**Version constraints:** chumsky must stay at 0.9 (0.10+ has a completely
different API). ariadne must stay at 0.4 (0.5+ changes the Report/Label
API). These are pinned because upgrades would require rewriting
thousands of lines.