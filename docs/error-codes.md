# Error Code Index (quick lookup)

**Purpose:** Use this table to find the **compiler phase** and **primary crate/files** for any error code. Full catalog is in `docs/SPECIFICATION.md` §7.2 / Appendix D.

**How to use**
1. Note the code prefix (`A01` = parser, `A02` = resolve, `A03` = types, ...).
2. Open the primary crate/files below (or `rg 'A0xxxx' crates --glob '*.rs'`).
3. Do **not** fix a types error by changing the SMT backend unless the code is `A04`/`A11`/`A05100` and the failure is genuinely solver-side.
4. For unknown codes not listed here: `rg 'A0xxxx' docs/SPECIFICATION.md` then `rg 'A0xxxx' crates`.

## By series (agent phase map)

| Prefix | Phase | Primary crate | Start here |
|--------|-------|---------------|------------|
| A01xxx | parser | assura-parser | grammar/, lexer.rs, lower/ |
| A02xxx | resolve | assura-resolve | lib.rs, type_refs.rs, imports.rs |
| A03xxx | types | assura-types | inference.rs, clauses.rs, checks/ |
| A04xxx | smt+types | assura-smt / assura-types | entry/, z3_backend/, refinement paths |
| A05xxx | types | assura-types | checks/linear_typestate.rs, checkers/linear.rs |
| A06xxx | types | assura-types | checks/linear_typestate.rs, checkers/typestate.rs |
| A07xxx | types | assura-types | checks/effects.rs, checkers/effects.rs |
| A08xxx | types | assura-types | checks/info_flow.rs, checkers/taint.rs, checkers/info_flow.rs |
| A09xxx | types | assura-types | checks/meta.rs (match), checkers/totality.rs |
| A10xxx | types | assura-types | checks/meta.rs, match exhaustiveness |
| A11xxx | smt+types | assura-smt / assura-types | entry/, invariant checks |
| A12xxx | types | assura-types | checks/concurrency.rs, checkers/security/ |
| A13xxx | types | assura-types | checks/numeric.rs, domain/numeric.rs |
| A31xxx | types | assura-types | checks/core.rs (liveness prove/fairness) |
| A05 (impl) | smt+cli | assura-smt / assura-cli | `A05100` SMT inconclusive / limitation |

## Codes from SPEC §7.2 (plus a few high-traffic impl codes)

| Code | Phase | Primary crate | Message | Cause (spec) | SPEC subsection | Start in tree |
|------|-------|---------------|---------|--------------|-----------------|---------------|
| A01001 | parser | assura-parser | Unexpected token | Parser error | Syntax (A01xxx) | grammar/, lexer.rs, lower/ |
| A01002 | parser | assura-parser | Unterminated string literal | Missing closing quote | Syntax (A01xxx) | grammar/, lexer.rs, lower/ |
| A01003 | parser | assura-parser | Invalid numeric literal | Malformed number | Syntax (A01xxx) | grammar/, lexer.rs, lower/ |
| A01004 | parser | assura-parser | Reserved keyword used as identifier | Naming conflict | Syntax (A01xxx) | grammar/, lexer.rs, lower/ |
| A01005 | parser | assura-parser | Mismatched braces | Unbalanced `{}` | Syntax (A01xxx) | grammar/, lexer.rs, lower/ |
| A02001 | resolve | assura-resolve | Undefined identifier `X` | Name not in scope | Name Resolution (A02xxx) | lib.rs, type_refs.rs, imports.rs |
| A02002 | resolve | assura-resolve | Undefined type `X` | Type not declared | Name Resolution (A02xxx) | lib.rs, type_refs.rs, imports.rs |
| A02003 | resolve | assura-resolve | Duplicate definition of `X` | Name collision | Name Resolution (A02xxx) | lib.rs, type_refs.rs, imports.rs |
| A02004 | resolve | assura-resolve | Ambiguous import `X` | Multiple modules export same name | Name Resolution (A02xxx) | lib.rs, type_refs.rs, imports.rs |
| A02005 | resolve | assura-resolve | Circular import | Module A imports B imports A | Name Resolution (A02xxx) | lib.rs, type_refs.rs, imports.rs |
| A03001 | types | assura-types | Expected `T1`, found `T2` / empty tuple type | Incompatible types; invalid `(,)` | Type Mismatch (A03xxx) | inference.rs, clauses.rs, checks/ |
| A03002 | types | assura-types | Type parameter count mismatch | Wrong number of generics | Type Mismatch (A03xxx) | inference.rs, clauses.rs, checks/ |
| A03003 | types | assura-types | Cannot unify `T1` with `T2` | Failed unification | Type Mismatch (A03xxx) | inference.rs, clauses.rs, checks/ |
| A03004 | types | assura-types | Missing field `F` in struct | Incomplete construction | Type Mismatch (A03xxx) | inference.rs, clauses.rs, checks/ |
| A03005 | types | assura-types | Unknown field `F` in type `T` | Field does not exist | Type Mismatch (A03xxx) | inference.rs, clauses.rs, checks/ |
| A03006 | types | assura-types | Clause not Bool / dependent index mismatch | Non-Bool clause body; or `Vec<T,3>` vs `Vec<T,5>` | Type Mismatch (A03xxx) | clauses.rs, checkers/info_flow.rs |
| A04001 | smt+types | assura-smt / assura-types | Precondition may not hold | `requires` clause violated | Refinement Violation (A04xxx) | entry/, z3_backend/, refinement paths |
| A04002 | smt+types | assura-smt / assura-types | Postcondition may not hold | `ensures` clause violated | Refinement Violation (A04xxx) | entry/, z3_backend/, refinement paths |
| A04003 | smt+types | assura-smt / assura-types | Refinement subtype check failed | `{v: T \ | Refinement Violation (A04xxx) | entry/, z3_backend/, refinement paths |
| A04004 | smt+types | assura-smt / assura-types | Division by zero possible | Divisor may be 0 | Refinement Violation (A04xxx) | entry/, z3_backend/, refinement paths |
| A04005 | smt+types | assura-smt / assura-types | Index out of bounds possible | Index may exceed length | Refinement Violation (A04xxx) | entry/, z3_backend/, refinement paths |
| A04006 | smt+types | assura-smt / assura-types | Arithmetic overflow possible | Result may exceed bounds | Refinement Violation (A04xxx) | entry/, z3_backend/, refinement paths |
| A04007 | smt+types | assura-smt / assura-types | Refinement timeout | SMT solver timed out | Refinement Violation (A04xxx) | entry/, z3_backend/, refinement paths |
| A05001 | types | assura-types | Linear variable `X` used twice | Grade 1, used 2+ times | Linearity (A05xxx) | checks/linear_typestate.rs, checkers/linear.rs |
| A05002 | types | assura-types | Linear variable `X` not used | Grade 1, never consumed | Linearity (A05xxx) | checks/linear_typestate.rs, checkers/linear.rs |
| A05003 | types | assura-types | Grade mismatch: expected `N`, used `M` | Exact count violated | Linearity (A05xxx) | checks/linear_typestate.rs, checkers/linear.rs |
| A05004 | types | assura-types | Cannot copy linear value | Tried to duplicate | Linearity (A05xxx) | checks/linear_typestate.rs, checkers/linear.rs |
| A05005 | types | assura-types | Linear value dropped without consuming | Resource leak | Linearity (A05xxx) | checks/linear_typestate.rs, checkers/linear.rs |
| A06001 | types | assura-types | Invalid transition: `S1` -> `S2` | Not in state machine | Typestate (A06xxx) | checks/linear_typestate.rs, checkers/typestate.rs |
| A06002 | types | assura-types | Operation requires state `S`, found `S'` | Wrong current state | Typestate (A06xxx) | checks/linear_typestate.rs, checkers/typestate.rs |
| A06003 | types | assura-types | Object not in final state at end of scope | Protocol incomplete | Typestate (A06xxx) | checks/linear_typestate.rs, checkers/typestate.rs |
| A06004 | types | assura-types | Ambiguous state after branch | Different states in if/else | Typestate (A06xxx) | checks/linear_typestate.rs, checkers/typestate.rs |
| A06005 | types | assura-types | Missing transition guard | Required predicate missing | Typestate (A06xxx) | checks/linear_typestate.rs, checkers/typestate.rs |
| A07001 | types | assura-types | Undeclared effect `E` | Effect not in function signature | Effect Violation (A07xxx) | checks/effects.rs, checkers/effects.rs |
| A07002 | types | assura-types | Pure function performs effect `E` | Side effect in pure context | Effect Violation (A07xxx) | checks/effects.rs, checkers/effects.rs |
| A07003 | types | assura-types | Effect `E` in must-not list | Explicitly forbidden effect | Effect Violation (A07xxx) | checks/effects.rs, checkers/effects.rs |
| A07004 | types | assura-types | Effect handler missing for `E` | Unhandled effect | Effect Violation (A07xxx) | checks/effects.rs, checkers/effects.rs |
| A07005 | types | assura-types | Effect hierarchy violation | Sub-effect used but parent not declared | Effect Violation (A07xxx) | checks/effects.rs, checkers/effects.rs |
| A08001 | types | assura-types | Data flow violation: `L1` to `L2` | High to low flow | Information Flow (A08xxx) | checks/info_flow.rs, checkers/taint.rs, checkers/info_flow.rs |
| A08002 | types | assura-types | PII leaked to logs | Restricted data in Public sink | Information Flow (A08xxx) | checks/info_flow.rs, checkers/taint.rs, checkers/info_flow.rs |
| A08003 | types | assura-types | Implicit flow via branch | Secret in branch condition | Information Flow (A08xxx) | checks/info_flow.rs, checkers/taint.rs, checkers/info_flow.rs |
| A08004 | types | assura-types | Purpose violation | Data used for undeclared purpose | Information Flow (A08xxx) | checks/info_flow.rs, checkers/taint.rs, checkers/info_flow.rs |
| A08005 | types | assura-types | Missing declassification | Label downgrade without `declassify` | Information Flow (A08xxx) | checks/info_flow.rs, checkers/taint.rs, checkers/info_flow.rs |
| A09001 | types | assura-types | Non-exhaustive pattern match | Missing cases | Totality (A09xxx) | checks/meta.rs (match), checkers/totality.rs |
| A09002 | types | assura-types | Recursion may not terminate | No decreasing measure | Totality (A09xxx) | checks/meta.rs (match), checkers/totality.rs |
| A09003 | types | assura-types | Decreasing measure not well-founded | Measure does not decrease | Totality (A09xxx) | checks/meta.rs (match), checkers/totality.rs |
| A09004 | types | assura-types | Partial function called from total context | Missing `trust` | Totality (A09xxx) | checks/meta.rs (match), checkers/totality.rs |
| A11001 | smt+types | assura-smt / assura-types | Invariant violated | SMT found counterexample | Business Invariant (A11xxx) | entry/, invariant checks |
| A11002 | smt+types | assura-smt / assura-types | Invariant not preserved by operation | Mutation breaks invariant | Business Invariant (A11xxx) | entry/, invariant checks |
| A11003 | smt+types | assura-smt / assura-types | Invariant verification timeout | SMT solver timed out | Business Invariant (A11xxx) | entry/, invariant checks |
| A11004 | smt+types | assura-smt / assura-types | Rule clause violated | Business rule not satisfied | Business Invariant (A11xxx) | entry/, invariant checks |
| A12001 | types | assura-types | Exclusive resource accessed concurrently | Data race possible | Concurrency (A12xxx) | checks/concurrency.rs, checkers/security/ |
| A12002 | types | assura-types | Actor isolation violated | Cross-actor mutable access | Concurrency (A12xxx) | checks/concurrency.rs, checkers/security/ |
| A12003 | types | assura-types | Shared-read resource modified | Write in shared-read context | Concurrency (A12xxx) | checks/concurrency.rs, checkers/security/ |
| A13001 | types | assura-types | Unit mismatch: `U1` vs `U2` | e.g., USD + EUR | Numerical Precision (A13xxx) | checks/numeric.rs, domain/numeric.rs |
| A13002 | types | assura-types | Dimensionally invalid operation | e.g., Money * Money | Numerical Precision (A13xxx) | checks/numeric.rs, domain/numeric.rs |
| A13003 | types | assura-types | Float used where fixed-point required | Precision loss | Numerical Precision (A13xxx) | checks/numeric.rs, domain/numeric.rs |
| A13004 | types | assura-types | Integer overflow possible | Arithmetic exceeds bounds | Numerical Precision (A13xxx) | checks/numeric.rs, domain/numeric.rs |
| A16001 | ? | ? | Purpose violation | Data used outside declared purposes | Privacy (A16xxx) | rg code in crates |
| A16002 | ? | ? | Retention policy missing | No retention declared for PII | Privacy (A16xxx) | rg code in crates |
| A16003 | ? | ? | Anonymization required | Retention period expired | Privacy (A16xxx) | rg code in crates |
| A17001 | ? | ? | Breaking field removal | Required field removed | Schema Evolution (A17xxx) | rg code in crates |
| A17002 | ? | ? | Missing default for new field | Non-optional field added | Schema Evolution (A17xxx) | rg code in crates |
| A17003 | ? | ? | Type change without migration | Incompatible field type change | Schema Evolution (A17xxx) | rg code in crates |
| A21001 | ? | ? | Breaking response field removal | Client may depend on field | API Evolution (A21xxx) | rg code in crates |
| A21002 | ? | ? | New required request field | Existing clients will fail | API Evolution (A21xxx) | rg code in crates |
| A21003 | ? | ? | Error variant removed | Client handlers break | API Evolution (A21xxx) | rg code in crates |
| A22001 | ? | ? | Exceeds declared complexity | O(n^2) found, O(n) declared | Complexity Bounds (A22xxx) | rg code in crates |
| A22002 | ? | ? | Complexity analysis timeout | AARA solver timed out | Complexity Bounds (A22xxx) | rg code in crates |
| A22003 | ? | ? | Unbounded allocation detected | No allocation bound proved | Complexity Bounds (A22xxx) | rg code in crates |
| A05100 | smt+cli | assura-smt / assura-cli | SMT counterexample found (verification failed) | Fix the contract (real violation) | (impl) | check/report.rs |
| A05101 | cli | assura-cli | SMT solver timed out | Increase `--timeout` | (impl) | check/report.rs |
| A05102 | cli | assura-cli | Known compiler limitation (warning, exit 0) | No action needed | (impl) | check/report.rs |
| A05103 | cli | assura-cli | Solver inconclusive (error, exit 1) | Simplify the contract | (impl) | check/report.rs |
| A10002 | types | assura-types | Match on unknown scrutinee without wildcard | (implementation; see CLI/SMT Unknown policy) | (impl) | checks/meta.rs (match exhaustiveness) |

## High-traffic implementation codes (not always in SPEC §7.2 table above)

Agents often hit these in tests/checkers before finding them in Appendix D. Prefer
this table over guessing the phase.

| Code | Phase | Primary crate | Typical meaning | Start in tree |
|------|-------|---------------|-----------------|---------------|
| A02006 | resolve | assura-resolve | Import / path resolution failure | imports.rs |
| A02007 | resolve | assura-resolve | Unused import | unused.rs |
| A02008 | resolve | assura-resolve | Import / module path error | imports.rs |
| A03006 | types | assura-types | Clause body not `Bool` where required | clauses.rs |
| A03007 | types | assura-types | Numeric / refinement constraint failure | checks/numeric.rs, domain/numeric.rs |
| A03010 | types | assura-types | Type / annotation mismatch (impl) | inference.rs, clauses.rs, checks/ |
| A07003 | types | assura-types | Unknown / denied effect | checks/effects.rs (known effect names only) |
| A08102 | types | assura-types | Info-flow / taint violation (impl) | checks/info_flow.rs, checkers/taint.rs |
| A10001 | types | assura-types | Non-exhaustive match | checks/meta.rs |
| A10101 | types | assura-types | Numeric / match interaction (impl) | checks/numeric.rs, checks/meta.rs |
| A11005 | types | assura-types | Invariant / FFI-related type issue | checks/ffi_error.rs, entry/invariant paths |
| A14001 | types | assura-types | Frame / modifies violation | checks/frame_totality.rs |
| A23016 | types | assura-types | Domain / feature checker (impl) | domain/, checks/ |
| A24001 | types | assura-types | Domain / feature checker (impl) | domain/, checks/ |
| A27003 | types | assura-types | Domain / feature checker (impl) | domain/, checks/ |
| A28001 | types | assura-types | Domain / feature checker (impl) | domain/, checks/ |
| A33001 | types | assura-types | Storage / resource checker | checks/storage.rs |
| A37003 | types | assura-types | Storage / resource checker | checks/storage.rs |
| A38001 | types | assura-types | Storage / resource checker | checks/storage.rs |
| A42003 | types | assura-types | Numeric precision / bounds | checks/numeric.rs |
| A43001 | types | assura-types | Numeric precision / bounds | checks/numeric.rs |
| A43002 | types | assura-types | Numeric precision / bounds | checks/numeric.rs |
| A44001 | types | assura-types | Platform / target checker | checks/platform.rs |
| A45001 | types | assura-types | Platform / target checker | checks/platform.rs |
| A47001 | types | assura-types | Safety / CVE pattern checker | checks/safety.rs |
| A48002 | types | assura-types | Meta / match / totality (impl) | checks/meta.rs |
| A49001 | types | assura-types | Meta / match / totality (impl) | checks/meta.rs |
| A49002 | types | assura-types | Meta / match / totality (impl) | checks/meta.rs |
| A50001 | types | assura-types | Meta / feature checker (impl) | checks/meta.rs, domain/ |
| A52001 | types | assura-types | Meta / feature checker (impl) | checks/meta.rs |
| A54001 | types | assura-types | Meta / feature checker (impl) | checks/meta.rs |
| A55001 | types | assura-types | Meta / feature checker (impl) | checks/meta.rs, domain/ |
| A64001 | types | assura-types | FFI / error propagation (impl) | checks/ffi_error.rs |
| A31006 | types | assura-types | Liveness block missing `prove` | checks/core.rs (`run_liveness_checks`) |
| A31007 | types | assura-types | `leads_to` without `assume fair` | checks/core.rs (`run_liveness_checks`); colon form splits `prove`/`leads_to` clauses |

If a code is still missing: `rg 'A0xxxx' crates --glob '*.rs'` then add a row here
in the same PR when agents are likely to hit it again.

## Agent decision shortcuts

| Symptom | First action |
|---------|--------------|
| `A01xxx` | Parser/grammar/lower; minimal reproduction in `tests/fixtures/` |
| `A02xxx` | `assura-resolve`; symbol table / imports / type_refs |
| `A03xxx` | `assura-types` inference/clauses; check `Type::is_indeterminate()` footgun |
| `A04xxx` / counterexample | `assura-smt`; unconstrained `result`/outputs; `verify_typed` |
| `A05xxx` linearity | `checks/linear_typestate.rs` / `checkers/linear.rs` |
| `A06xxx` typestate | `checkers/typestate.rs` |
| `A07xxx` effects | `checks/effects.rs`; known effect names only (see AGENTS pipeline trap) |
| `A08xxx` taint/flow | `checks/info_flow.rs` / `checkers/taint.rs` |
| `A09xxx` / `A10xxx` match/totality | `checks/meta.rs` / `checkers/totality.rs`; parser arm trivia footgun |
| `A14xxx` frame/modifies | `checks/frame_totality.rs` |
| `A31xxx` liveness | `checks/core.rs`; parser may split `prove: leads_to(...)` into two clauses |
| `A05100` counterexample / `A05101` timeout / `A05102` limitation / `A05103` inconclusive | `check/report.rs`; limitation (A05102) = warning, else error |
| `A52xxx` / `A54xxx` / high A-series | domain/meta features: `checks/meta.rs`, `domain/`, then `rg 'Axxxxx' crates` |
| Wrong phase suspicion | `bash scripts/guards.sh` then re-read AGENTS decision tree |

## Maintenance

- Source of truth for meanings: `docs/SPECIFICATION.md` §7.2.
- When adding a new `Axxxxx` in code, add a row here (or in "High-traffic implementation codes") in the same PR if agents are likely to hit it.
- Do **not** try to generate all of Appendix D unless agents repeatedly miss phase; curated + high-traffic is enough.
- Full phase/wiring rules: `AGENTS.md`, `crates/assura-types/src/CHECKER-LAYERS.md`.
