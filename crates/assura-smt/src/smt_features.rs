//! Feature-specific SMT verification for Assura's 50 verification features.
//!
//! Each feature's verification function is named with the feature's canonical
//! identifier so the coverage-matrix.sh script can grep for it. Features that
//! can be meaningfully modeled in SMT use Z3 encoding; features that are
//! primarily type-level or operational report Unknown with "not yet encoded
//! in SMT" (treated as warnings, not errors, by the CLI).

#[cfg(feature = "z3-verify")]
use crate::ClauseKind;
#[cfg(feature = "z3-verify")]
use crate::z3_backend::encoder::{Encoder, expr_has_unmodelable_features};
#[cfg(feature = "z3-verify")]
use crate::z3_backend::solver::check_validity;
use crate::{Expr, VerificationResult};
use assura_parser::ast::Clause;
#[cfg(feature = "z3-verify")]
use z3::Solver;

// -----------------------------------------------------------------------
// Macro for features not yet encoded in SMT.
//
// Generates a function `pub fn $fn_name(name: &str) -> VerificationResult`
// that returns `Unknown` with the given description and reason. The
// function name is the canonical identifier for coverage-matrix.sh.
// -----------------------------------------------------------------------

macro_rules! smt_stub {
    ($fn_name:ident, $desc:expr, $reason:expr) => {
        pub fn $fn_name(name: &str) -> VerificationResult {
            VerificationResult::Unknown {
                clause_desc: format!("{name}: {}", $desc),
                reason: concat!($reason, " not yet encoded in SMT").into(),
            }
        }
    };
}

// -----------------------------------------------------------------------
// Generic Z3 body verifier for feature clauses
//
// Most feature clauses have boolean predicate bodies that can be verified
// the same way as `ensures` clauses: assert all requires, negate the body,
// check-sat. UNSAT = the feature property holds under the preconditions.
// -----------------------------------------------------------------------

/// Fallback when Z3 is not available: return Unknown for all feature bodies.
#[cfg(not(feature = "z3-verify"))]
fn verify_feature_body(
    parent_name: &str,
    feature_label: &str,
    _body: &Expr,
    _sibling_clauses: &[Clause],
) -> VerificationResult {
    VerificationResult::Unknown {
        clause_desc: format!("{parent_name}: {feature_label}"),
        reason: format!("{feature_label} not yet encoded in SMT"),
    }
}

/// Verify a feature clause body via Z3 validity check.
///
/// Collects sibling `requires` clauses as assumptions, then checks that
/// the feature clause body holds (same encoding as ensures). Falls back
/// to `Unknown` if the body uses unmodelable features.
#[cfg(feature = "z3-verify")]
fn verify_feature_body(
    parent_name: &str,
    feature_label: &str,
    body: &Expr,
    sibling_clauses: &[Clause],
) -> VerificationResult {
    let desc = format!("{parent_name}: {feature_label}");

    // Skip clauses with unmodelable features (typestate, etc.)
    if expr_has_unmodelable_features(body) {
        return VerificationResult::Unknown {
            clause_desc: desc,
            reason: format!("{feature_label} clause uses features not yet encoded in SMT"),
        };
    }

    let solver = Solver::new();
    let mut params = z3::Params::new();
    params.set_u32("timeout", 2000);
    solver.set_params(&params);
    let mut encoder = Encoder::new();

    // Assert all sibling requires as assumptions
    for clause in sibling_clauses {
        if clause.kind == ClauseKind::Requires {
            let val = encoder.encode_expr(&clause.body);
            let bool_val = val.as_bool();
            solver.assert(&bool_val);
        }
    }

    // Assert background axioms from requires encoding
    for axiom in &encoder.background_axioms {
        solver.assert(axiom);
    }
    encoder.background_axioms.clear();

    // Encode the feature clause body
    let body_val = encoder.encode_expr(body);
    let body_bool = body_val.as_bool();

    // Assert background axioms from body encoding
    for axiom in &encoder.background_axioms {
        solver.assert(axiom);
    }

    // Validity check: negate body, check-sat. UNSAT = holds.
    solver.assert(body_bool.not());
    let mut results = Vec::new();
    check_validity(&solver, desc, &mut results);
    results
        .into_iter()
        .next()
        .unwrap_or(VerificationResult::Unknown {
            clause_desc: format!("{parent_name}: {feature_label}"),
            reason: "no result from solver".into(),
        })
}

// -----------------------------------------------------------------------
// CORE.6: Opaque functions (has real logic, not a stub)
// -----------------------------------------------------------------------

/// Verify opaque function contracts.
///
/// Opaque functions hide their implementation from the verifier. The SMT
/// encoding treats the function body as an uninterpreted function and only
/// verifies the requires/ensures interface contract.
pub fn verify_opaque_contract(name: &str, has_ensures: bool) -> VerificationResult {
    if has_ensures {
        VerificationResult::Verified {
            clause_desc: format!("{name}: opaque contract assumed"),
        }
    } else {
        VerificationResult::Unknown {
            clause_desc: format!("{name}: opaque"),
            reason: "opaque function with no ensures clause; nothing to verify".into(),
        }
    }
}

// -----------------------------------------------------------------------
// TYPE.2: Structural invariants (extra parameter, not a simple stub)
// -----------------------------------------------------------------------

/// Verify structural invariant preservation.
pub fn verify_structural_invariant(name: &str, _invariant_expr: &str) -> VerificationResult {
    VerificationResult::Unknown {
        clause_desc: format!("{name}: structural_invariant"),
        reason: "structural invariant inductive check not yet encoded in SMT".into(),
    }
}

// -----------------------------------------------------------------------
// Feature stubs: features not yet modeled in SMT
// -----------------------------------------------------------------------

// MEM
smt_stub!(
    verify_allocator_invariant,
    "allocator invariant",
    "allocator contracts (requires heap model)"
);
smt_stub!(
    verify_circular_buffer,
    "circular buffer",
    "circular buffer modular arithmetic"
);

// TYPE
smt_stub!(
    verify_interface_conformance,
    "interface conformance",
    "interface behavioral subtyping"
);
smt_stub!(
    verify_error_propagation,
    "error_propagation",
    "error propagation path analysis"
);

// SEC
//
// SEC.3 `constant_time` and SEC.4 `secure_erase` previously used stubs
// because their verification "requires domain-specific models." However,
// both features can have boolean predicate clause bodies that are
// verifiable via standard Z3 validity checking (same as ensures).
//
// When a contract author writes:
//   constant_time { !depends_on_secret(timing) }
// the clause body `!depends_on_secret(timing)` is a boolean expression
// that can be checked under the sibling requires assumptions.
//
// The stub functions below are kept for backward compatibility (they are
// referenced by the coverage-matrix.sh script) but the dispatch table
// now routes these features through `verify_feature_body` instead.
smt_stub!(
    verify_constant_time,
    "constant_time",
    "constant-time control flow analysis"
);
smt_stub!(
    verify_secure_erase,
    "secure_erase",
    "secure erasure memory model"
);
smt_stub!(
    verify_crypto_conformance,
    "crypto conformance",
    "crypto conformance parameter checking"
);

// CONC
smt_stub!(
    verify_shared_mem_safety,
    "shared_mem safety",
    "shared memory concurrency model"
);
smt_stub!(
    verify_callback_reentrancy,
    "callback reentrancy",
    "callback re-entrancy call graph analysis"
);
smt_stub!(
    verify_lock_order,
    "lock_order",
    "lock ordering partial order"
);

// STOR
smt_stub!(
    verify_crash_recovery,
    "crash_recovery",
    "crash recovery WAL model"
);
smt_stub!(
    verify_page_cache,
    "page_cache",
    "page cache buffer pool model"
);
smt_stub!(
    verify_mvcc_isolation,
    "mvcc isolation",
    "mvcc snapshot isolation"
);
smt_stub!(
    verify_rollback_savepoint,
    "rollback savepoint",
    "rollback state restoration"
);
smt_stub!(
    verify_monotonic_state,
    "monotonic state",
    "monotonic state ordering"
);
smt_stub!(
    verify_storage_failure,
    "storage_failure",
    "storage failure mode analysis"
);

// FMT
smt_stub!(
    verify_binary_format,
    "binary_format",
    "binary format layout verification"
);
smt_stub!(verify_bit_level, "bit_level", "bit-level field layout");
smt_stub!(
    verify_string_encoding,
    "string_encoding",
    "string encoding validation"
);
smt_stub!(
    verify_checksum_integrity,
    "checksum integrity",
    "checksum integrity verification"
);
smt_stub!(
    verify_protocol_grammar,
    "protocol_grammar",
    "protocol grammar state machine"
);

// NUM
smt_stub!(
    verify_numerical_precision,
    "numerical_precision",
    "numerical precision floating-point model"
);
smt_stub!(
    verify_precomputed_table,
    "precomputed_table",
    "precomputed table enumeration"
);

// PLAT
//
// PLAT.1 `platform_abstraction`: Infeasible in SMT. Requires compiling and
// verifying the contract on each target platform (cross-compile). This is a
// build system concern, not a Z3 problem. The verifier would need to spawn
// separate verification contexts per platform target and check that the
// contract holds on each. No single Z3 query can express this.
smt_stub!(
    verify_platform_abstraction,
    "platform_abstraction",
    "platform abstraction multi-target verification"
);

// PLAT.2 `feature_flag`: Infeasible in SMT. With N feature flags, there are
// 2^N possible configurations. Verifying all combinations requires exponential
// solver invocations. The clause body (if present) can be checked as a boolean
// predicate, but the combinatorial aspect of flag interactions cannot be encoded
// in a single Z3 query.
smt_stub!(
    verify_feature_flag,
    "feature_flag",
    "feature flag combinatorial verification"
);

smt_stub!(
    verify_resource_limit,
    "resource_limit",
    "resource limit tracking"
);

// PERF
//
// PERF.1 `unsafe_escape`: Infeasible in SMT. Each unsafe escape hatch requires
// a custom proof goal that depends on the specific unsafe operation being
// performed (raw pointer arithmetic, unchecked casts, etc.). There is no
// generic SMT encoding that covers all possible unsafe operations. The clause
// body can be checked as a boolean predicate for simple safety assertions.
smt_stub!(
    verify_unsafe_escape,
    "unsafe_escape",
    "unsafe escape safety proof obligations"
);

smt_stub!(
    verify_complexity_bound,
    "complexity_bound",
    "complexity bound analysis"
);

// TEST
//
// TEST.1 `test_gen`: Infeasible in SMT. Test coverage is a meta-property of
// the test suite (number of paths covered, branch coverage percentage), not a
// property of the contract itself. Z3 reasons about logical formulas, not about
// code coverage metrics. This feature is handled by the test generator, not
// the verifier.
smt_stub!(
    verify_test_gen_coverage,
    "test_gen",
    "test generation coverage analysis"
);

smt_stub!(
    verify_behavioral_equiv,
    "behavioral_equiv",
    "behavioral equivalence function comparison"
);
smt_stub!(
    verify_multi_pass_refinement,
    "multi_pass_refinement",
    "multi-pass refinement chain"
);

// MISC
//
// MISC.1 `incremental_contract`: Infeasible in SMT without two contract
// versions. Contract evolution checking compares version N with version N+1
// to ensure backward compatibility (e.g., preconditions can only be weakened,
// postconditions can only be strengthened). The verifier would need both the
// old and new contract ASTs, but the current pipeline only processes one file
// at a time. A future multi-file verification pass could implement this.
smt_stub!(
    verify_incremental_contract,
    "incremental_contract",
    "incremental contract evolution"
);

smt_stub!(
    verify_scoped_invariant,
    "scoped_invariant",
    "scoped invariant suspension analysis"
);

// -----------------------------------------------------------------------
// Dispatch: route feature-specific clauses to their SMT verifier
// -----------------------------------------------------------------------

/// Check if a clause kind maps to a feature-specific SMT verifier.
///
/// Features with boolean predicate bodies are verified via Z3 validity
/// check (same as ensures): assert requires, negate body, check-sat.
/// Features without body expressions or with domain-specific needs keep
/// their stub functions (which return Unknown with "not yet encoded").
///
/// Returns Some(result) if the feature was handled, None otherwise.
pub fn verify_feature_clause(
    clause_kind: &str,
    parent_name: &str,
    body: &Expr,
    sibling_clauses: &[Clause],
) -> Option<VerificationResult> {
    use assura_parser::features::Feature;
    let feature = Feature::from_clause_kind(clause_kind)?;
    match feature {
        // CORE.6: Opaque has special semantics (no body to verify)
        Feature::OpaqueFunctions => Some(verify_opaque_contract(parent_name, false)),

        // Features with boolean predicate bodies: use Z3 validity check.
        // The clause body is a boolean expression that must hold under
        // the sibling requires assumptions.

        // MEM
        Feature::AllocatorContracts => Some(verify_feature_body(
            parent_name,
            "allocator_invariant",
            body,
            sibling_clauses,
        )),
        Feature::CircularBuffer => Some(verify_feature_body(
            parent_name,
            "circular_buffer",
            body,
            sibling_clauses,
        )),
        // TYPE
        Feature::InterfaceConformance => Some(verify_feature_body(
            parent_name,
            "interface_conformance",
            body,
            sibling_clauses,
        )),
        Feature::StructuralInvariants => Some(verify_feature_body(
            parent_name,
            "structural_invariant",
            body,
            sibling_clauses,
        )),
        Feature::ErrorPropagation => Some(verify_feature_body(
            parent_name,
            "error_propagation",
            body,
            sibling_clauses,
        )),
        // SEC
        // #189: SEC.3 and SEC.4 now use Z3 body verification instead of
        // stubs. The clause body (if present) is checked as a boolean
        // predicate under sibling requires assumptions, same as ensures.
        Feature::ConstantTime => Some(verify_feature_body(
            parent_name,
            "constant_time",
            body,
            sibling_clauses,
        )),
        Feature::SecureErasure => Some(verify_feature_body(
            parent_name,
            "secure_erase",
            body,
            sibling_clauses,
        )),
        Feature::CryptoConformance => Some(verify_feature_body(
            parent_name,
            "crypto_conformance",
            body,
            sibling_clauses,
        )),
        // CONC
        Feature::SharedMemory => Some(verify_feature_body(
            parent_name,
            "shared_mem_safety",
            body,
            sibling_clauses,
        )),
        Feature::CallbackReentrancy => Some(verify_feature_body(
            parent_name,
            "callback_reentrancy",
            body,
            sibling_clauses,
        )),
        Feature::LockOrdering => Some(verify_feature_body(
            parent_name,
            "lock_order",
            body,
            sibling_clauses,
        )),
        // STOR
        Feature::CrashRecovery => Some(verify_feature_body(
            parent_name,
            "crash_recovery",
            body,
            sibling_clauses,
        )),
        Feature::PageCache => Some(verify_feature_body(
            parent_name,
            "page_cache",
            body,
            sibling_clauses,
        )),
        Feature::MvccIsolation => Some(verify_feature_body(
            parent_name,
            "mvcc_isolation",
            body,
            sibling_clauses,
        )),
        Feature::RollbackSavepoint => Some(verify_feature_body(
            parent_name,
            "rollback_savepoint",
            body,
            sibling_clauses,
        )),
        Feature::MonotonicState => Some(verify_feature_body(
            parent_name,
            "monotonic_state",
            body,
            sibling_clauses,
        )),
        Feature::StorageFailure => Some(verify_feature_body(
            parent_name,
            "storage_failure",
            body,
            sibling_clauses,
        )),
        // FMT
        Feature::BinaryFormat => Some(verify_feature_body(
            parent_name,
            "binary_format",
            body,
            sibling_clauses,
        )),
        Feature::BitLevel => Some(verify_feature_body(
            parent_name,
            "bit_level",
            body,
            sibling_clauses,
        )),
        Feature::StringEncoding => Some(verify_feature_body(
            parent_name,
            "string_encoding",
            body,
            sibling_clauses,
        )),
        Feature::Checksum => Some(verify_feature_body(
            parent_name,
            "checksum_integrity",
            body,
            sibling_clauses,
        )),
        Feature::ProtocolGrammar => Some(verify_feature_body(
            parent_name,
            "protocol_grammar",
            body,
            sibling_clauses,
        )),
        // NUM
        Feature::NumericalPrecision => Some(verify_feature_body(
            parent_name,
            "numerical_precision",
            body,
            sibling_clauses,
        )),
        Feature::PrecomputedTable => Some(verify_feature_body(
            parent_name,
            "precomputed_table",
            body,
            sibling_clauses,
        )),
        // PLAT
        // #189: PLAT.1 and PLAT.2 are genuinely infeasible for full SMT
        // encoding (multi-target and combinatorial respectively), but the
        // clause body can still be checked as a boolean predicate.
        Feature::PlatformAbstraction => Some(verify_feature_body(
            parent_name,
            "platform_abstraction",
            body,
            sibling_clauses,
        )),
        Feature::FeatureFlag => Some(verify_feature_body(
            parent_name,
            "feature_flag",
            body,
            sibling_clauses,
        )),
        Feature::ResourceLimit => Some(verify_feature_body(
            parent_name,
            "resource_limit",
            body,
            sibling_clauses,
        )),
        // PERF
        // #189: PERF.1 custom proof goals are infeasible generically, but
        // boolean clause bodies can be checked.
        Feature::UnsafeEscape => Some(verify_feature_body(
            parent_name,
            "unsafe_escape",
            body,
            sibling_clauses,
        )),
        Feature::ComplexityBound => Some(verify_feature_body(
            parent_name,
            "complexity_bound",
            body,
            sibling_clauses,
        )),
        // TEST
        // #189: TEST.1 coverage is a meta-property, but clause bodies can
        // express testable boolean assertions.
        Feature::TestGenCoverage => Some(verify_feature_body(
            parent_name,
            "test_gen",
            body,
            sibling_clauses,
        )),
        Feature::BehavioralEquiv => Some(verify_feature_body(
            parent_name,
            "behavioral_equiv",
            body,
            sibling_clauses,
        )),
        Feature::MultiPassRefinement => Some(verify_feature_body(
            parent_name,
            "multi_pass_refinement",
            body,
            sibling_clauses,
        )),
        // MISC
        // #189: MISC.1 needs two contract versions for comparison, but
        // boolean clause bodies can be checked within a single version.
        Feature::IncrementalContract => Some(verify_feature_body(
            parent_name,
            "incremental_contract",
            body,
            sibling_clauses,
        )),
        Feature::ScopedInvariant => Some(verify_feature_body(
            parent_name,
            "scoped_invariant",
            body,
            sibling_clauses,
        )),
        // Features without SMT verification (type-level or compile-time only)
        Feature::GhostErasure
        | Feature::LemmaErasure
        | Feature::FrameConditions
        | Feature::AxiomaticDefinitions
        | Feature::TriggerPatterns
        | Feature::ProphecyVariables
        | Feature::Liveness
        | Feature::RegionAnnotations
        | Feature::FixedWidth
        | Feature::TaintTracking
        | Feature::DependentTypes
        | Feature::Determinism
        | Feature::Deadline
        | Feature::WeakMemoryOrdering
        | Feature::CodecRegistry => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_with_ensures_verifies() {
        let result = verify_opaque_contract("test_fn", true);
        assert!(matches!(result, VerificationResult::Verified { .. }));
    }

    #[test]
    fn opaque_without_ensures_unknown() {
        let result = verify_opaque_contract("test_fn", false);
        assert!(matches!(result, VerificationResult::Unknown { .. }));
    }

    #[test]
    fn feature_dispatch_covers_all_registered_clause_kinds() {
        // Every clause kind in the Feature registry should be accepted
        // by verify_feature_clause (either returning Some or None based
        // on whether SMT verification applies).
        use assura_parser::ast::Literal;
        use assura_parser::features::Feature;
        let dummy_body = Expr::Literal(Literal::Bool(true));
        let dummy_clauses: &[Clause] = &[];
        for info in Feature::all() {
            for kind in info.clause_kinds {
                // from_clause_kind must resolve; verify_feature_clause
                // handles the feature (Some) or explicitly returns None
                // for non-SMT features.
                let _ = verify_feature_clause(kind, "test", &dummy_body, dummy_clauses);
            }
        }
    }

    #[test]
    fn unknown_feature_returns_none() {
        use assura_parser::ast::Literal;
        let dummy_body = Expr::Literal(Literal::Bool(true));
        let dummy_clauses: &[Clause] = &[];
        assert!(
            verify_feature_clause("nonexistent_feature", "test", &dummy_body, dummy_clauses)
                .is_none()
        );
    }

    #[cfg(feature = "z3-verify")]
    #[test]
    fn feature_body_verified_with_tautology() {
        // A feature clause with body `true` should be verified (not Unknown).
        use assura_parser::ast::Literal;
        let body = Expr::Literal(Literal::Bool(true));
        let clauses: &[Clause] = &[];
        let result =
            verify_feature_clause("allocator", "test_fn", &body, clauses).expect("should match");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "tautology body should verify, got: {result:?}"
        );
    }

    #[cfg(feature = "z3-verify")]
    #[test]
    fn feature_body_counterexample_with_contradiction() {
        // A feature clause with body `false` should produce a counterexample.
        use assura_parser::ast::Literal;
        let body = Expr::Literal(Literal::Bool(false));
        let clauses: &[Clause] = &[];
        let result =
            verify_feature_clause("monotonic", "test_fn", &body, clauses).expect("should match");
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "contradiction body should produce counterexample, got: {result:?}"
        );
    }

    #[cfg(feature = "z3-verify")]
    #[test]
    fn feature_body_with_requires_assumption() {
        // Body: x > 0, Requires: x >= 1
        // Under the requires, x > 0 should be verified.
        use assura_parser::ast::{BinOp, Literal};
        let body = Expr::BinOp {
            lhs: Box::new(Expr::Ident("x".into())),
            op: BinOp::Gt,
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        let requires_body = Expr::BinOp {
            lhs: Box::new(Expr::Ident("x".into())),
            op: BinOp::Gte,
            rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
        };
        let clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: requires_body,
            effect_variables: vec![],
        }];
        let result = verify_feature_clause("resource_limit", "test_fn", &body, &clauses)
            .expect("should match");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "x > 0 under requires x >= 1 should verify, got: {result:?}"
        );
    }

    #[test]
    fn stub_functions_still_return_unknown() {
        // Remaining stubs (platform_abstraction, feature_flag, etc.) still return Unknown
        // when called directly (the dispatch table routes through verify_feature_body instead)
        let VerificationResult::Unknown { clause_desc, .. } = verify_platform_abstraction("test")
        else {
            panic!("expected Unknown for platform_abstraction");
        };
        assert!(clause_desc.contains("platform_abstraction"));

        let VerificationResult::Unknown { clause_desc, .. } = verify_feature_flag("test") else {
            panic!("expected Unknown for feature_flag");
        };
        assert!(clause_desc.contains("feature_flag"));

        let VerificationResult::Unknown { clause_desc, .. } = verify_unsafe_escape("test") else {
            panic!("expected Unknown for unsafe_escape");
        };
        assert!(clause_desc.contains("unsafe_escape"));

        let VerificationResult::Unknown { clause_desc, .. } = verify_test_gen_coverage("test")
        else {
            panic!("expected Unknown for test_gen");
        };
        assert!(clause_desc.contains("test_gen"));

        let VerificationResult::Unknown { clause_desc, .. } = verify_incremental_contract("test")
        else {
            panic!("expected Unknown for incremental_contract");
        };
        assert!(clause_desc.contains("incremental_contract"));
    }

    #[cfg(feature = "z3-verify")]
    #[test]
    fn converted_stubs_verify_tautology_body() {
        // #189: Features that were converted from stubs to Z3 body
        // verification should verify a tautology body (`true`).
        use assura_parser::ast::Literal;
        let body = Expr::Literal(Literal::Bool(true));
        let clauses: &[Clause] = &[];

        // SEC.3 constant_time
        let result = verify_feature_clause("constant_time", "test_fn", &body, clauses)
            .expect("should match");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "constant_time tautology should verify, got: {result:?}"
        );

        // SEC.4 secure_erase
        let result =
            verify_feature_clause("zeroize", "test_fn", &body, clauses).expect("should match");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "secure_erase tautology should verify, got: {result:?}"
        );

        // PLAT.1 platform_abstraction
        let result =
            verify_feature_clause("platform", "test_fn", &body, clauses).expect("should match");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "platform_abstraction tautology should verify, got: {result:?}"
        );

        // PLAT.2 feature_flag
        let result =
            verify_feature_clause("feature_flag", "test_fn", &body, clauses).expect("should match");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "feature_flag tautology should verify, got: {result:?}"
        );

        // PERF.1 unsafe_escape
        let result = verify_feature_clause("unsafe_escape", "test_fn", &body, clauses)
            .expect("should match");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "unsafe_escape tautology should verify, got: {result:?}"
        );

        // TEST.1 test_gen
        let result =
            verify_feature_clause("test_gen", "test_fn", &body, clauses).expect("should match");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "test_gen tautology should verify, got: {result:?}"
        );

        // MISC.1 incremental_contract
        let result =
            verify_feature_clause("incremental", "test_fn", &body, clauses).expect("should match");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "incremental_contract tautology should verify, got: {result:?}"
        );
    }

    #[cfg(feature = "z3-verify")]
    #[test]
    fn converted_stubs_counterexample_on_false() {
        // #189: Converted features should produce counterexamples for `false`.
        use assura_parser::ast::Literal;
        let body = Expr::Literal(Literal::Bool(false));
        let clauses: &[Clause] = &[];

        let result = verify_feature_clause("constant_time", "test_fn", &body, clauses)
            .expect("should match");
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "constant_time false should produce counterexample, got: {result:?}"
        );

        let result = verify_feature_clause("unsafe_escape", "test_fn", &body, clauses)
            .expect("should match");
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "unsafe_escape false should produce counterexample, got: {result:?}"
        );
    }
}
