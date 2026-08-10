# Error Code Index (quick lookup)

**Purpose:** Use this table to find the **compiler phase** and **primary crate/files** for any error code. Full catalog is in `docs/SPECIFICATION.md` §7.2 / Appendix D.

**How to use**
1. Note the code prefix (`A01` = parser, `A02` = resolve, `A03` = types, ...).
2. Open the primary crate/files below (or `rg 'A0xxxx' crates --glob '*.rs'`).
3. Do **not** fix a types error by changing the SMT backend unless the code is `A04`/`A11`/`A05100` and the failure is genuinely solver-side.
4. For unknown codes not listed here: `rg 'A0xxxx' docs/SPECIFICATION.md` then `rg 'A0xxxx' crates`.
5. Rows under **Catalog placeholders** are not emitted by any checker. Do **not**
   open implement issues for them. Ship emit + tests + catalog + index row in
   one PR when product prioritizes the check (same rule as phantom ban #1489).

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
| A05 (impl) | smt+cli | assura-smt / assura-cli | `A05100` CE, `A05101` timeout, `A05102` known limitation, `A05103` inconclusive |

## Codes from SPEC §7.2 (plus a few high-traffic impl codes)

| Code | Phase | Primary crate | Message | Cause (spec) | SPEC subsection | Start in tree |
|------|-------|---------------|---------|--------------|-----------------|---------------|
| A01001 | parser | assura-parser | Unexpected token | Parser error | Syntax (A01xxx) | grammar/, lexer.rs, lower/ |
| A01002 | parser | assura-parser | Unterminated string literal | Missing closing quote | Syntax (A01xxx) | grammar/, lexer.rs, lower/ |
| A02001 | resolve | assura-resolve | Undefined identifier `X` | Name not in scope | Name Resolution (A02xxx) | lib.rs, type_refs.rs, imports.rs |
| A02003 | resolve | assura-resolve | Duplicate definition of `X` | Name collision | Name Resolution (A02xxx) | lib.rs, type_refs.rs, imports.rs |
| A02005 | resolve | assura-resolve | Circular import | Module A imports B imports A | Name Resolution (A02xxx) | lib.rs, type_refs.rs, imports.rs |
| A03001 | types | assura-types | Expected `T1`, found `T2` / empty tuple / pattern arity | Incompatible types; invalid `(,)`; constructor/tuple pattern field count | Type Mismatch (A03xxx) | inference.rs, clauses.rs, checks/ |
| A03002 | types | assura-types | Type parameter count mismatch | Wrong number of generics | Type Mismatch (A03xxx) | inference.rs, clauses.rs, checks/ |
| A03005 | types | assura-types | Unknown field `F` in type `T` | Field does not exist | Type Mismatch (A03xxx) | inference.rs, clauses.rs, checks/ |
| A03006 | types | assura-types | Clause not Bool / dependent index mismatch | Non-Bool clause body; or `Vec<T,3>` vs `Vec<T,5>` | Type Mismatch (A03xxx) | clauses.rs, checkers/info_flow.rs |
| A05001 | types | assura-types | Linear variable `X` used twice | Grade 1, used 2+ times | Linearity (A05xxx) | checks/linear_typestate.rs, checkers/linear.rs |
| A05002 | types | assura-types | Linear variable `X` not used | Grade 1, never consumed | Linearity (A05xxx) | checks/linear_typestate.rs, checkers/linear.rs |
| A05003 | types | assura-types | Grade mismatch: expected `N`, used `M` | Exact count violated | Linearity (A05xxx) | checks/linear_typestate.rs, checkers/linear.rs |
| A05004 | types | assura-types | Cannot copy linear value | Tried to duplicate | Linearity (A05xxx) | checks/linear_typestate.rs, checkers/linear.rs |
| A06001 | types | assura-types | Invalid transition: `S1` -> `S2` | Not in state machine | Typestate (A06xxx) | checks/linear_typestate.rs, checkers/typestate.rs |
| A06002 | types | assura-types | Operation requires state `S`, found `S'` | Wrong current state | Typestate (A06xxx) | checks/linear_typestate.rs, checkers/typestate.rs |
| A06003 | types | assura-types | Object not in final state at end of scope | Protocol incomplete | Typestate (A06xxx) | checks/linear_typestate.rs, checkers/typestate.rs |
| A06004 | types | assura-types | Ambiguous state after branch | Different states in if/else | Typestate (A06xxx) | checks/linear_typestate.rs, checkers/typestate.rs |
| A07001 | types | assura-types | Undeclared effect `E` | Effect not in function signature | Effect Violation (A07xxx) | checks/effects.rs, checkers/effects.rs |
| A07002 | types | assura-types | Pure function performs effect `E` | Side effect in pure context | Effect Violation (A07xxx) | checks/effects.rs, checkers/effects.rs |
| A07003 | types | assura-types | Effect `E` in must-not list | Explicitly forbidden effect | Effect Violation (A07xxx) | checks/effects.rs, checkers/effects.rs |
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
| A05102 | cli | assura-cli | Known compiler limitation (warning, exit 0; error under `--strict`) | Simplify ensures, add IR, or ignore until encoding lands | (impl) | check/report.rs |
| A05103 | cli | assura-cli | Solver inconclusive (error, exit 1) | Simplify the contract or raise `--timeout` | (impl) | check/report.rs |
| A10002 | types | assura-types | Match on unknown scrutinee without wildcard | (implementation; see CLI/SMT Unknown policy) | (impl) | checks/meta.rs (match exhaustiveness) |

## High-traffic implementation codes (not always in SPEC §7.2 table above)

Agents often hit these in tests/checkers before finding them in Appendix D. Prefer
this table over guessing the phase.

| Code | Phase | Primary crate | Typical meaning | Start in tree |
|------|-------|---------------|-----------------|---------------|
| A01000 | cli/pipeline | assura-cli / assura-pipeline | Source file read/IO failure | check/run.rs, pipeline |
| A02006 | resolve | assura-resolve | Duplicate import | imports.rs |
| A02007 | resolve | assura-resolve | Unused import | unused.rs |
| A02008 | resolve | assura-resolve | Invalid import path segment | imports.rs |
| A02010 | resolve | assura-resolve | Cannot resolve import (module not found) | imports.rs, lib.rs |
| A03006 | types | assura-types | Clause body not `Bool` where required | clauses.rs |
| A03007 | types | assura-types | Numeric / refinement constraint failure | checks/numeric.rs, domain/numeric.rs |
| A03010 | types | assura-types | Type / annotation mismatch (impl) | inference.rs, clauses.rs, checks/ |
| A07003 | types | assura-types | Unknown / denied effect | checks/effects.rs (known effect names only) |
| A08102 | types | assura-types | Info-flow / taint violation (impl) | checks/info_flow.rs, checkers/taint.rs |
| A10001 | types | assura-types | Non-exhaustive match | checks/meta.rs |
| A10101 | types | assura-types | Numeric / match interaction (impl) | checks/numeric.rs, checks/meta.rs |
| A11005 | types | assura-types | Invariant / FFI-related type issue | checks/ffi_error.rs, entry/invariant paths |
| A14001 | types | assura-types | Frame / modifies violation | checks/frame_totality.rs |
| A14002 | types | assura-types | Secret-dependent array index (timing) | checkers/error_propagation.rs, checks/frame_totality.rs |
| A04008 | types+cli | assura-types / assura-cli | Ensures references unconstrained output (`result`) | checks/clause_quality.rs; suppressed when IR present (#703) |
| A05025 | smt+types | assura-smt / assura-types | Unresolved prophecy variable | advanced/prophecy.rs; structural checker in types |
| A05026 | smt | assura-smt | Prophecy double-resolved / unconstrained | advanced/prophecy.rs |
| A08101 | types | assura-types | Buffer access without bounds check | checkers/memory.rs |
| A09101 | types | assura-types | Tainted data as array index | checkers/taint.rs |
| A23003 | types | assura-types | Circular buffer empty on read | domain/memory.rs |
| A26001 | types | assura-types | Binary format field offset exceeds buffer | domain/format/binary_format.rs |
| A43005 | types | assura-types | Precomputed table size not a standard domain | domain/numeric.rs |
| A17004 | types | assura-types | Decrypt without `tag_verified` (AEAD) | checks crypto conformance |
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
| A32002 | types | assura-types | Opaque body access without reveal | domain/core/opaque_function.rs |
| A36003 | types | assura-types | Duplicate savepoint name | domain/storage/rollback.rs |
| A52002 | types | assura-types | Suspend undeclared invariant (also empty decoder name in codec registry) | domain/meta/scoped_invariant.rs, domain/format/codec_registry.rs |
| A46002 | types | assura-types | Unbounded resource usage | domain/platform.rs |
| A29001 | types | assura-types | Data used before checksum verification | domain/format/checksum.rs |
| A25003 | types | assura-types | Unbounded operation in deadline | domain/concurrency.rs |
| A09103 | types | assura-types | Tainted data flows to trusted sink | checkers/taint.rs |
| A53006 | types | assura-types | Quantifier missing trigger annotation | domain/core/quantifier_trigger.rs |
| A49003 | types | assura-types | Equivalence missing contract reference | domain/meta/behavioral_equivalence.rs |
| A35003 | types | assura-types | Phantom read | domain/storage/mvcc.rs |
| A34003 | types | assura-types | Page cache capacity exceeded | domain/storage/page_cache.rs |
| A30002 | types | assura-types | Protocol wrong state for message | domain/format/protocol_grammar.rs |
| A23001 | types | assura-types | Circular buffer index exceeds capacity | domain/memory.rs |
| A10104 | types | assura-types | Fixed-width division by zero | checkers/fixed_width.rs |
| A09102 | types | assura-types | Tainted data used as allocation size | checkers/taint.rs |
| A08103 | types | assura-types | Ghost region references missing buffer | checkers/memory.rs |
| A51003 | types | assura-types | Contract version gap | domain/meta/incremental_contract.rs |
| A46003 | types | assura-types | Resource near limit | domain/platform.rs |
| A36001 | types | assura-types | Rollback to unknown savepoint | domain/storage/rollback.rs |
| A35001 | types | assura-types | Write-write conflict | domain/storage/mvcc.rs |
| A10102 | types | assura-types | Unsafe narrowing cast | checkers/fixed_width.rs |
| A10103 | types | assura-types | Signed/unsigned comparison mismatch | checkers/fixed_width.rs |
| A42001 | types | assura-types | Numerical precision loss | domain/numeric.rs |
| A20001 | types | assura-types | Deterministic function uses non-deterministic source | checkers/security/determinism.rs |
| A20002 | types | assura-types | Deterministic function iterates hash collection | checkers/security/determinism.rs |
| A18001 | types | assura-types | Shared memory read without access mode | checkers/security/shared_mem.rs |
| A18003 | types | assura-types | Shared memory data race | checkers/security/shared_mem.rs |
| A24003 | types | assura-types | Callback depth exceeded | domain/concurrency.rs |
| A25001 | types | assura-types | Deadline exceeded | domain/concurrency.rs |
| A22004 | types | assura-types | Arena use after drop | domain/memory.rs |
| A44003 | types | assura-types | Unknown platform in abstraction | domain/platform.rs |
| A46001 | types | assura-types | Resource limit exceeded | domain/platform.rs |
| A55003 | types | assura-types | Duplicate library name | domain/meta/contract_library.rs |
| A32001 | types | assura-types | Opaque function called without contract | domain/core/opaque_function.rs |
| A48001 | types | assura-types | Complexity bound exceeded | domain/meta/complexity_bound.rs |
| A34001 | types | assura-types | Evict pinned page | domain/storage/page_cache.rs |
| A37001 | types | assura-types | Monotonicity violation | domain/storage/monotonic_state.rs |
| A30003 | types | assura-types | Protocol missing required field | domain/format/protocol_grammar.rs |

| A15004 | types | assura-types | Operation may violate invariant | checkers/security/structural_invariant.rs |
| A15001 | types | assura-types | Structural invariant on non-recursive type | checkers/security/structural_invariant.rs |
| A18002 | types | assura-types | Shared memory write without exclusive | checkers/security/shared_mem.rs |
| A33003 | types | assura-types | Fsync before data write | domain/storage/crash_recovery.rs |
| A03012 | types | assura-types | Index variable used at runtime | checkers/info_flow.rs |
| A23002 | types | assura-types | Circular buffer zero capacity | domain/memory.rs |
| A45003 | types | assura-types | Undeclared feature flag | domain/platform.rs |
| A42002 | types | assura-types | ULP bound violation | domain/numeric.rs |
| A31001 | types | assura-types | Undefined axiom reference | domain/core/axiomatic_def.rs |
| A31003 | types | assura-types | Unused axiom | domain/core/axiomatic_def.rs |
| A32003 | types | assura-types | Reveal outside proof context | domain/core/opaque_function.rs |
| A51001 | types | assura-types | Precondition strengthened | domain/meta/incremental_contract.rs |
| A48003 | types | assura-types | Exponential complexity warning | domain/meta/complexity_bound.rs |
| A54003 | types | assura-types | Diamond inheritance in contracts | domain/meta/contract_composition.rs |
| A30001 | types | assura-types | Protocol invalid transition | domain/format/protocol_grammar.rs |
| A29003 | types | assura-types | Checksum range mismatch | domain/format/checksum.rs |
| A28003 | types | assura-types | String truncation splits code unit | domain/format/string_encoding.rs |
| A27001 | types | assura-types | Bit field out of bounds | domain/format/bit_level.rs |
| A26004 | types | assura-types | Binary fields overlap | domain/format/binary_format.rs |
| A26003 | types | assura-types | Binary field missing endianness | domain/format/binary_format.rs |
| A15002 | types | assura-types | Tree invariant insufficient fields | checkers/security/structural_invariant.rs |
| A15003 | types | assura-types | Sort invariant wrong field count | checkers/security/structural_invariant.rs |
| A33002 | types | assura-types | Commit without fsync | domain/storage/crash_recovery.rs |
| A03011 | types | assura-types | Dependent type index kind mismatch | checkers/info_flow.rs |
| A03008 | types | assura-types | Invalid Bool index expression | checkers/info_flow.rs |
| A25002 | types | assura-types | Nested deadline exceeds outer | domain/concurrency.rs |
| A24002 | types | assura-types | Callback registered in non-reentrant context | domain/concurrency.rs |
| A23019 | types | assura-types | Fence ordering mismatch | domain/memory.rs |
| A47002 | types | assura-types | Undischarged safety obligation | domain/safety.rs |
| A47003 | types | assura-types | Empty proof obligations | domain/safety.rs |
| A45002 | types | assura-types | Conflicting feature flags | domain/platform.rs |
| A38002 | types | assura-types | Handler for undeclared failure mode | domain/storage/storage_failure.rs |
| A44002 | types | assura-types | Direct platform reference | domain/platform.rs |
| A55002 | types | assura-types | Library self-dependency | domain/meta/contract_library.rs |
| A54002 | types | assura-types | Circular contract extends chain | domain/meta/contract_composition.rs |
| A43003 | types | assura-types | Zero-size table | domain/numeric.rs |
| A43004 | types | assura-types | Invalid encoding: byte sequence not valid | domain/numeric.rs |
| A31002 | types | assura-types | Circular axiom dependency | domain/core/axiomatic_def.rs |
| A53003 | types | assura-types | After-all predicate not satisfied | domain/core/crud_auth.rs |
| A53001 | types | assura-types | CRUD operation missing auth policy | domain/core/crud_auth.rs |
| A53002 | types | assura-types | Delete without authentication | domain/core/crud_auth.rs |
| A52003 | types | assura-types | Restore non-suspended invariant | domain/meta/scoped_invariant.rs |
| A50002 | types | assura-types | Refinement chain gap | domain/meta/multi_pass_refinement.rs |
| A50003 | types | assura-types | Trivial refinement pass | domain/meta/multi_pass_refinement.rs |
| A36002 | types | assura-types | Resource leak after rollback | domain/storage/rollback.rs |
| A38003 | types | assura-types | Critical failure mode unhandled | domain/storage/storage_failure.rs |
| A35002 | types | assura-types | Snapshot isolation violation | domain/storage/mvcc.rs |
| A34002 | types | assura-types | Evict dirty page without flush | domain/storage/page_cache.rs |
| A29002 | types | assura-types | Checksum algorithm mismatch | domain/format/checksum.rs |
| A28002 | types | assura-types | String encoding mismatch | domain/format/string_encoding.rs |
| A27002 | types | assura-types | Bit field crosses byte boundary | domain/format/bit_level.rs |
| A05200 | types | assura-types | Unbounded quantifier warning | assura-cli/src/check/report.rs |
| A51002 | types | assura-types | Postcondition weakened | domain/meta/incremental_contract.rs |
| A37002 | types | assura-types | Illegal monotonic variable reset | domain/storage/monotonic_state.rs |
| A03009 | types | assura-types | Invalid Enum index expression | checkers/info_flow.rs |

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

## Catalog placeholders (not emitted)

These codes exist in `assura-diagnostics` catalog (so `assura explain` works)
and often in SPEC Appendix D, but **no checker currently emits them**.

**Do not open implement tickets for these.** That is the same class of noise
as phantom codes (#1486/#1487): catalog or docs invent a number before the
check exists. When product prioritizes a check, implement **emit + tests +
catalog row refresh in the same PR**, then move the row into the high-traffic
table with a real start path and **remove the code from**
`scripts/catalog-hollow-allowlist.txt` (guards section 15 freezes that set;
see #1490).

**Authoritative hollow set:** `scripts/catalog-hollow-allowlist.txt` (94 codes
as of #1490). Some SPEC series codes may still appear in the tables above for
phase lookup; if a code is on the allowlist it is not emitted. Section 15
fails CI if the hollow set grows without an allowlist update.

| Code | Phase | Primary crate | Message | Status |
|------|-------|---------------|---------|--------|
| A19001 | types | assura-types | Missing audit trail | not emitted |
| A44005 | types | assura-types | Dirtying unpinned page | not emitted |
| A42005 | types | assura-types | Proof obligation references out-of-scope variable | not emitted |
| A52005 | types | assura-types | No codec matches input | not emitted |
| A02002 | resolve | assura-resolve | Undefined type (catalog-only / test-only construction) | not emitted |
| A02009 | resolve | assura-resolve | Visibility violation | not emitted |
| A55004 | types | assura-types | Lemma has side effects | not emitted |
| A50004 | types | assura-types | Generating function is not total over range | not emitted |
| A36004 | types | assura-types | Nested atomic function swallows error | not emitted |
| A42004 | types | assura-types | Unsafe escape without proof obligation | not emitted |
| A55005 | types | assura-types | Circular lemma dependency | not emitted |
| A31005 | types | assura-types | Reserved space violated | not emitted |
| A35005 | types | assura-types | Callee is not deterministic | not emitted |
| A50005 | types | assura-types | Table size mismatch | not emitted |
| A51005 | types | assura-types | Reference function uses restricted operations | not emitted |
| A54004 | types | assura-types | Ghost variable not updated to match runtime state | not emitted |
| A57001 | types | assura-types | Axiom is inconsistent | not emitted |
| A58005 | types | assura-types | Trigger pattern not found in formula | not emitted |
| A40004 | types | assura-types | Resources not released on terminal state | not emitted |
| A56005 | types | assura-types | Frame condition conflict with effects | not emitted |
| A29004 | types | assura-types | Protocol violation: step out of order | not emitted |
| A37005 | types | assura-types | FFI thread safety violation | not emitted |
| A32004 | types | assura-types | Recovery procedure has side effects beyond repair | not emitted |
| A57005 | types | assura-types | Conflicting axiom definitions | not emitted |
| A59001 | types | assura-types | Cannot prove property: function is opaque | not emitted |
| A35004 | types | assura-types | Pointer-derived value in deterministic context | not emitted |
| A47004 | types | assura-types | Monotonic value overflows without wrap policy | not emitted |
| A34004 | types | assura-types | Callback may fail but is marked infallible | not emitted |
| A58001 | types | assura-types | Trigger does not mention bound variable | not emitted |
| A29005 | types | assura-types | Reader may see partial write | not emitted |
| A37004 | types | assura-types | FFI null pointer not checked | not emitted |
| A49005 | types | assura-types | Bit field constraint not satisfiable | not emitted |
| A39004 | types | assura-types | Limit change may invalidate existing state | not emitted |
| A49004 | types | assura-types | Bit cursor used after byte-level read | not emitted |
| A45005 | types | assura-types | Write to read-only transaction | not emitted |
| A54005 | types | assura-types | Ghost type used in runtime signature | not emitted |
| A41001 | types | assura-types | Output divergence detected | not emitted |
| A56001 | types | assura-types | Function modifies undeclared target | not emitted |
| A51004 | types | assura-types | No reference function for precision contract | not emitted |
| A41005 | types | assura-types | Undocumented exclusion | not emitted |
| A38004 | types | assura-types | Feature max too small for invariant | not emitted |
| A46004 | types | assura-types | IO bound exceeded | not emitted |
| A39001 | types | assura-types | Limit may be exceeded without check | not emitted |
| A45004 | types | assura-types | Stale snapshot: version no longer available | not emitted |
| A48004 | types | assura-types | Return value of reset not checked | not emitted |
| A48005 | types | assura-types | Must-preserve detail violated | not emitted |
| A52004 | types | assura-types | Probe function has side effects | not emitted |
| A40001 | types | assura-types | Step called in invalid state | not emitted |
| A34005 | types | assura-types | Callback invariant not satisfiable | not emitted |
| A59005 | types | assura-types | Opaque type field accessed externally | not emitted |
| A44004 | types | assura-types | Double unpin: pin count already zero | not emitted |
| A53005 | types | assura-types | Refinement state not initialized before first pass | not emitted |
| A53004 | types | assura-types | Pass count exceeds declared maximum | not emitted |
| A58002 | types | assura-types | Potential matching loop in trigger | not emitted |
| A57003 | types | assura-types | Axiom property does not follow from definition | not emitted |
| A40003 | types | assura-types | Incremental progress not guaranteed | not emitted |
| A39002 | types | assura-types | Limit default outside [min, max] | not emitted |
| A41002 | types | assura-types | Error code mismatch | not emitted |
| A39003 | types | assura-types | Limit max exceeds compile-time feature_max | not emitted |
| A41004 | types | assura-types | Type coercion difference | not emitted |
| A57002 | types | assura-types | Recursive axiom not well-founded | not emitted |
| A59004 | types | assura-types | Recursive reveal exceeded fuel | not emitted |
| A57004 | types | assura-types | Axiom used at runtime | not emitted |
| A56004 | types | assura-types | Modifies clause on pure function | not emitted |
| A59002 | types | assura-types | Reveal of non-opaque function | not emitted |
| A58004 | types | assura-types | Conflicting triggers on same quantifier | not emitted |
| A58003 | types | assura-types | Quantifier timeout (no trigger specified) | not emitted |
| A56002 | types | assura-types | Called function modifies outside caller's frame | not emitted |
| A19002 | types | assura-types | Incomplete audit trail | not emitted |
| A56003 | types | assura-types | Function reads undeclared source | not emitted |
| A41003 | types | assura-types | Row ordering difference | not emitted |
| A31004 | types | assura-types | Format exceeds expected size | not emitted |
| A40002 | types | assura-types | Incremental value not finalized | not emitted |
| A04009 | types | assura-types | Feature_max constant in verification clause | not emitted |
| A59003 | types | assura-types | Opaque function contract insufficient | not emitted |
| A01003 | parser | assura-parser | Invalid numeric literal | not emitted |
| A01004 | parser | assura-parser | Reserved keyword used as identifier | not emitted |
| A01005 | parser | assura-parser | Mismatched braces | not emitted |
| A02004 | resolve | assura-resolve | Ambiguous import `X` | not emitted |
| A03003 | types | assura-types | Cannot unify `T1` with `T2` | not emitted |
| A03004 | types | assura-types | Missing field `F` in struct | not emitted |
| A04001 | smt+types | assura-smt / assura-types | Precondition may not hold | not emitted |
| A04002 | smt+types | assura-smt / assura-types | Postcondition may not hold | not emitted |
| A04003 | smt+types | assura-smt / assura-types | Refinement subtype check failed | not emitted |
| A04004 | smt+types | assura-smt / assura-types | Division by zero possible | not emitted |
| A04005 | smt+types | assura-smt / assura-types | Index out of bounds possible | not emitted |
| A04006 | smt+types | assura-smt / assura-types | Arithmetic overflow possible | not emitted |
| A04007 | smt+types | assura-smt / assura-types | Refinement timeout | not emitted |
| A05005 | types | assura-types | Linear value dropped without consuming | not emitted |
| A06005 | types | assura-types | Missing transition guard | not emitted |
| A07004 | types | assura-types | Effect handler missing for `E` | not emitted |
| A07005 | types | assura-types | Effect hierarchy violation | not emitted |
| A13004 | types | assura-types | Integer overflow possible | not emitted |
| A26002 | types | assura-types | Incomplete i18n coverage | not emitted |
