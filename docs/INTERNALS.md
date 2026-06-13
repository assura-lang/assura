# Assura Internals

## Architecture

```
Source (.assura) -> Lexer -> Parser -> AST
                                        |
                                    Resolver -> SymbolTable
                                        |
                                  Type Checker -> TypedFile
                                   /         \
                              SMT Verifier   Code Generator
                              (Z3/CVC5)      (Rust output)
```

## Crate Map

| Crate | Purpose | Key types |
|-------|---------|-----------|
| `assura-parser` | Lexing (logos) + parsing (chumsky 0.9) | `Token`, `SourceFile`, `Expr` |
| `assura-resolve` | Name resolution, scope analysis | `SymbolTable`, `ResolvedFile` |
| `assura-types` | Type checking, all domain checkers | `Type`, `TypeEnv`, `TypeError` |
| `assura-smt` | Z3 SMT solver integration | `SmtContext`, `VerificationResult` |
| `assura-codegen` | Rust code generation | `GeneratedProject` |
| `assura-cli` | CLI binary (`assura check/build/init/explain`) | - |
| `assura-lsp` | Language Server Protocol | - |
| `assura-server` | gRPC + HTTP API server | - |

## Type System

The type checker implements 30+ domain-specific checkers:

- **Core**: Ghost code, lemmas, frame conditions, axiomatic defs, triggers, opaque functions, prophecy vars, liveness
- **Memory**: Regions, fixed-width ints, allocators, circular buffers
- **Security**: Taint tracking, FFI boundaries, constant-time, secure erasure, crypto conformance
- **Concurrency**: Shared memory, re-entrancy, determinism, lock ordering, deadlines, weak memory
- **Format**: Binary/bit-level formats, string encoding, codec dispatch, checksums, protocol grammars
- **Storage**: Crash recovery, page cache, MVCC, rollback, monotonic state, failure models
- **Verification**: Complexity bounds, behavioral equivalence, multi-pass refinement

## SMT Encoding

Layer 1 uses quantifier-free theories:
- `QF_UFLIA`: integer arithmetic + uninterpreted functions
- `QF_UFLRA`: real arithmetic
- `QF_DT`: datatypes

Layer 2 adds quantifiers (`AUFLIA`) for:
- Universal invariants
- Termination proofs
- Roundtrip verification

## Error Code Scheme

| Range | Category |
|-------|----------|
| A01xxx | Syntax |
| A02xxx | Resolution |
| A03xxx | Type checking |
| A05xxx | Linearity |
| A06xxx | Typestate |
| A07xxx | Effects |
| A08xxx | Information flow |
| A09xxx | Totality |
| A10xxx | Exhaustiveness |
| A22xxx-A55xxx | Domain checkers |