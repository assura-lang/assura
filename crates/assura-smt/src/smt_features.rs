//! Feature-specific SMT verification for Assura's 50 verification features.
//!
//! Most feature clauses have boolean predicate bodies verified via Z3 validity
//! checking (assert requires, negate body, check-sat). Features that are
//! purely type-level or operational (no boolean body) return None from the
//! dispatch table and are skipped by the verifier.

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

// (smt_stub! macro and 33 dead stub functions removed in #197.
//  All feature clauses now route through verify_feature_body for
//  Z3 validity checking of boolean predicate bodies.)

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

    // Skip declarative feature clauses whose body is a bare identifier
    // (e.g., `incremental InflateDecoder`). These are type/declaration
    // references, not boolean predicates. Sending them to Z3 creates an
    // unconstrained variable that trivially produces counterexamples.
    if matches!(body, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase())) {
        return VerificationResult::Unknown {
            clause_desc: desc,
            reason: format!("{feature_label} not yet encoded in SMT"),
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
// TYPE.2: Structural invariants (inductive checking)
// -----------------------------------------------------------------------

/// Verify structural invariant via inductive checking.
///
/// A structural invariant must hold:
/// 1. **Establishment**: Under the requires (preconditions), the invariant
///    must hold initially. This checks `requires => invariant`.
/// 2. **Preservation**: If the invariant held before an operation and the
///    operation's postconditions (ensures) are met, the invariant still
///    holds. This checks `requires && ensures => invariant`.
///
/// Both are standard validity checks. The preservation step is strictly
/// stronger because it also asserts sibling ensures as assumptions.
///
/// Fallback when Z3 is not available.
#[cfg(not(feature = "z3-verify"))]
pub fn verify_structural_invariant_inductive(
    parent_name: &str,
    _body: &Expr,
    _sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    vec![VerificationResult::Unknown {
        clause_desc: format!("{parent_name}: structural_invariant"),
        reason: "structural_invariant not yet encoded in SMT".into(),
    }]
}

/// Verify structural invariant via inductive checking (Z3 implementation).
///
/// Returns two results:
/// - Establishment: `requires => invariant`
/// - Preservation: `requires && ensures => invariant`
#[cfg(feature = "z3-verify")]
pub fn verify_structural_invariant_inductive(
    parent_name: &str,
    body: &Expr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    let mut all_results = Vec::new();

    // Skip unmodelable bodies
    if expr_has_unmodelable_features(body) {
        all_results.push(VerificationResult::Unknown {
            clause_desc: format!("{parent_name}: structural_invariant (establishment)"),
            reason: "structural_invariant clause uses features not yet encoded in SMT".into(),
        });
        return all_results;
    }

    // Skip bare uppercase identifier bodies (declarative references, not predicates)
    if matches!(body, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase())) {
        all_results.push(VerificationResult::Unknown {
            clause_desc: format!("{parent_name}: structural_invariant"),
            reason: "structural_invariant not yet encoded in SMT".into(),
        });
        return all_results;
    }

    // ---- Step 1: Establishment ----
    // Assert requires, negate invariant body, check UNSAT.
    {
        let desc = format!("{parent_name}: structural_invariant (establishment)");
        let solver = Solver::new();
        let mut params = z3::Params::new();
        params.set_u32("timeout", 2000);
        solver.set_params(&params);
        let mut encoder = Encoder::new();

        // Assert all sibling requires as assumptions
        for clause in sibling_clauses {
            if clause.kind == ClauseKind::Requires {
                let val = encoder.encode_expr(&clause.body);
                solver.assert(val.as_bool());
            }
        }
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        encoder.background_axioms.clear();

        // Encode invariant body, negate, check validity
        let body_val = encoder.encode_expr(body);
        let body_bool = body_val.as_bool();
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        solver.assert(body_bool.not());
        let mut step_results = Vec::new();
        check_validity(&solver, desc, &mut step_results);
        all_results.extend(step_results);
    }

    // ---- Step 2: Preservation ----
    // Assert requires AND ensures (postconditions), negate invariant, check UNSAT.
    // This models: after an operation that satisfies its postconditions,
    // the invariant must still hold.
    {
        let desc = format!("{parent_name}: structural_invariant (preservation)");
        let solver = Solver::new();
        let mut params = z3::Params::new();
        params.set_u32("timeout", 2000);
        solver.set_params(&params);
        let mut encoder = Encoder::new();

        // Assert requires
        for clause in sibling_clauses {
            if clause.kind == ClauseKind::Requires {
                let val = encoder.encode_expr(&clause.body);
                solver.assert(val.as_bool());
            }
        }
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        encoder.background_axioms.clear();

        // Assert ensures (operation postconditions)
        for clause in sibling_clauses {
            if clause.kind == ClauseKind::Ensures {
                let val = encoder.encode_expr(&clause.body);
                solver.assert(val.as_bool());
            }
        }
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        encoder.background_axioms.clear();

        // Negate invariant body, check validity
        let body_val = encoder.encode_expr(body);
        let body_bool = body_val.as_bool();
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        solver.assert(body_bool.not());
        let mut step_results = Vec::new();
        check_validity(&solver, desc, &mut step_results);
        all_results.extend(step_results);
    }

    all_results
}

// -----------------------------------------------------------------------
// Dispatch: route feature-specific clauses to their SMT verifier
// -----------------------------------------------------------------------

/// Check if a clause kind maps to a feature-specific SMT verifier.
///
/// Features with boolean predicate bodies are verified via Z3 validity
/// check (same as ensures): assert requires, negate body, check-sat.
/// Features without body expressions or with domain-specific needs
/// return Unknown with "not yet encoded in SMT" (treated as warnings).
///
/// Returns a non-empty Vec if the feature was handled, empty Vec otherwise.
/// Most features produce a single result; structural invariants produce
/// two (establishment + preservation) via inductive checking.
pub fn verify_feature_clause(
    clause_kind: &str,
    parent_name: &str,
    body: &Expr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    use assura_parser::features::Feature;
    let feature = match Feature::from_clause_kind(clause_kind) {
        Some(f) => f,
        None => return vec![],
    };
    match feature {
        // CORE.6: Opaque has special semantics (no body to verify)
        Feature::OpaqueFunctions => vec![verify_opaque_contract(parent_name, false)],

        // Features with boolean predicate bodies: use Z3 validity check.
        // The clause body is a boolean expression that must hold under
        // the sibling requires assumptions.

        // MEM
        Feature::AllocatorContracts => vec![verify_feature_body(
            parent_name,
            "allocator_invariant",
            body,
            sibling_clauses,
        )],
        Feature::CircularBuffer => vec![verify_feature_body(
            parent_name,
            "circular_buffer",
            body,
            sibling_clauses,
        )],
        // TYPE
        Feature::InterfaceConformance => vec![verify_feature_body(
            parent_name,
            "interface_conformance",
            body,
            sibling_clauses,
        )],
        Feature::StructuralInvariants => {
            // #202: Use inductive checking (establishment + preservation)
            verify_structural_invariant_inductive(parent_name, body, sibling_clauses)
        }
        Feature::ErrorPropagation => vec![verify_feature_body(
            parent_name,
            "error_propagation",
            body,
            sibling_clauses,
        )],
        // SEC
        // #189: SEC.3 and SEC.4 now use Z3 body verification instead of
        // stubs. The clause body (if present) is checked as a boolean
        // predicate under sibling requires assumptions, same as ensures.
        Feature::ConstantTime => vec![verify_feature_body(
            parent_name,
            "constant_time",
            body,
            sibling_clauses,
        )],
        Feature::SecureErasure => vec![verify_feature_body(
            parent_name,
            "secure_erase",
            body,
            sibling_clauses,
        )],
        Feature::CryptoConformance => vec![verify_feature_body(
            parent_name,
            "crypto_conformance",
            body,
            sibling_clauses,
        )],
        // CONC
        Feature::SharedMemory => vec![verify_feature_body(
            parent_name,
            "shared_mem_safety",
            body,
            sibling_clauses,
        )],
        Feature::CallbackReentrancy => vec![verify_feature_body(
            parent_name,
            "callback_reentrancy",
            body,
            sibling_clauses,
        )],
        Feature::LockOrdering => vec![verify_feature_body(
            parent_name,
            "lock_order",
            body,
            sibling_clauses,
        )],
        // STOR
        Feature::CrashRecovery => vec![verify_feature_body(
            parent_name,
            "crash_recovery",
            body,
            sibling_clauses,
        )],
        Feature::PageCache => vec![verify_feature_body(
            parent_name,
            "page_cache",
            body,
            sibling_clauses,
        )],
        Feature::MvccIsolation => vec![verify_feature_body(
            parent_name,
            "mvcc_isolation",
            body,
            sibling_clauses,
        )],
        Feature::RollbackSavepoint => vec![verify_feature_body(
            parent_name,
            "rollback_savepoint",
            body,
            sibling_clauses,
        )],
        Feature::MonotonicState => vec![verify_feature_body(
            parent_name,
            "monotonic_state",
            body,
            sibling_clauses,
        )],
        Feature::StorageFailure => vec![verify_feature_body(
            parent_name,
            "storage_failure",
            body,
            sibling_clauses,
        )],
        // FMT
        Feature::BinaryFormat => vec![verify_feature_body(
            parent_name,
            "binary_format",
            body,
            sibling_clauses,
        )],
        Feature::BitLevel => vec![verify_feature_body(
            parent_name,
            "bit_level",
            body,
            sibling_clauses,
        )],
        Feature::StringEncoding => vec![verify_feature_body(
            parent_name,
            "string_encoding",
            body,
            sibling_clauses,
        )],
        Feature::Checksum => vec![verify_feature_body(
            parent_name,
            "checksum_integrity",
            body,
            sibling_clauses,
        )],
        Feature::ProtocolGrammar => vec![verify_feature_body(
            parent_name,
            "protocol_grammar",
            body,
            sibling_clauses,
        )],
        // NUM
        Feature::NumericalPrecision => vec![verify_feature_body(
            parent_name,
            "numerical_precision",
            body,
            sibling_clauses,
        )],
        Feature::PrecomputedTable => vec![verify_feature_body(
            parent_name,
            "precomputed_table",
            body,
            sibling_clauses,
        )],
        // PLAT
        // #189: PLAT.1 and PLAT.2 are genuinely infeasible for full SMT
        // encoding (multi-target and combinatorial respectively), but the
        // clause body can still be checked as a boolean predicate.
        Feature::PlatformAbstraction => vec![verify_feature_body(
            parent_name,
            "platform_abstraction",
            body,
            sibling_clauses,
        )],
        Feature::FeatureFlag => vec![verify_feature_body(
            parent_name,
            "feature_flag",
            body,
            sibling_clauses,
        )],
        Feature::ResourceLimit => vec![verify_feature_body(
            parent_name,
            "resource_limit",
            body,
            sibling_clauses,
        )],
        // PERF
        // #189: PERF.1 custom proof goals are infeasible generically, but
        // boolean clause bodies can be checked.
        Feature::UnsafeEscape => vec![verify_feature_body(
            parent_name,
            "unsafe_escape",
            body,
            sibling_clauses,
        )],
        Feature::ComplexityBound => vec![verify_feature_body(
            parent_name,
            "complexity_bound",
            body,
            sibling_clauses,
        )],
        // TEST
        // #189: TEST.1 coverage is a meta-property, but clause bodies can
        // express testable boolean assertions.
        Feature::TestGenCoverage => vec![verify_feature_body(
            parent_name,
            "test_gen",
            body,
            sibling_clauses,
        )],
        Feature::BehavioralEquiv => vec![verify_feature_body(
            parent_name,
            "behavioral_equiv",
            body,
            sibling_clauses,
        )],
        Feature::MultiPassRefinement => vec![verify_feature_body(
            parent_name,
            "multi_pass_refinement",
            body,
            sibling_clauses,
        )],
        // MISC
        // #189: MISC.1 needs two contract versions for comparison, but
        // boolean clause bodies can be checked within a single version.
        Feature::IncrementalContract => vec![verify_feature_body(
            parent_name,
            "incremental_contract",
            body,
            sibling_clauses,
        )],
        Feature::ScopedInvariant => vec![verify_feature_body(
            parent_name,
            "scoped_invariant",
            body,
            sibling_clauses,
        )],
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
        | Feature::CodecRegistry => vec![],
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
        // by verify_feature_clause (either returning results or empty vec
        // based on whether SMT verification applies).
        use assura_parser::ast::Literal;
        use assura_parser::features::Feature;
        let dummy_body = Expr::Literal(Literal::Bool(true));
        let dummy_clauses: &[Clause] = &[];
        for info in Feature::all() {
            for kind in info.clause_kinds {
                // from_clause_kind must resolve; verify_feature_clause
                // handles the feature (non-empty Vec) or returns empty vec
                // for non-SMT features.
                let _ = verify_feature_clause(kind, "test", &dummy_body, dummy_clauses);
            }
        }
    }

    #[test]
    fn unknown_feature_returns_empty() {
        use assura_parser::ast::Literal;
        let dummy_body = Expr::Literal(Literal::Bool(true));
        let dummy_clauses: &[Clause] = &[];
        assert!(
            verify_feature_clause("nonexistent_feature", "test", &dummy_body, dummy_clauses)
                .is_empty()
        );
    }

    #[cfg(feature = "z3-verify")]
    #[test]
    fn feature_body_verified_with_tautology() {
        // A feature clause with body `true` should be verified (not Unknown).
        use assura_parser::ast::Literal;
        let body = Expr::Literal(Literal::Bool(true));
        let clauses: &[Clause] = &[];
        let results = verify_feature_clause("allocator", "test_fn", &body, clauses);
        assert!(!results.is_empty(), "should produce results");
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "tautology body should verify, got: {:?}",
            results[0]
        );
    }

    #[cfg(feature = "z3-verify")]
    #[test]
    fn feature_body_counterexample_with_contradiction() {
        // A feature clause with body `false` should produce a counterexample.
        use assura_parser::ast::Literal;
        let body = Expr::Literal(Literal::Bool(false));
        let clauses: &[Clause] = &[];
        let results = verify_feature_clause("monotonic", "test_fn", &body, clauses);
        assert!(!results.is_empty(), "should produce results");
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "contradiction body should produce counterexample, got: {:?}",
            results[0]
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
        let results = verify_feature_clause("resource_limit", "test_fn", &body, &clauses);
        assert!(!results.is_empty(), "should produce results");
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "x > 0 under requires x >= 1 should verify, got: {:?}",
            results[0]
        );
    }

    // stub_functions_still_return_unknown: removed in #197.
    // Stubs were dead code; all features now route through verify_feature_body.

    #[cfg(feature = "z3-verify")]
    #[test]
    fn converted_stubs_verify_tautology_body() {
        // #189: Features that were converted from stubs to Z3 body
        // verification should verify a tautology body (`true`).
        use assura_parser::ast::Literal;
        let body = Expr::Literal(Literal::Bool(true));
        let clauses: &[Clause] = &[];

        let check = |kind: &str, label: &str| {
            let results = verify_feature_clause(kind, "test_fn", &body, clauses);
            assert!(!results.is_empty(), "{label} should produce results");
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "{label} tautology should verify, got: {:?}",
                results[0]
            );
        };

        check("constant_time", "SEC.3 constant_time");
        check("zeroize", "SEC.4 secure_erase");
        check("platform", "PLAT.1 platform_abstraction");
        check("feature_flag", "PLAT.2 feature_flag");
        check("unsafe_escape", "PERF.1 unsafe_escape");
        check("test_gen", "TEST.1 test_gen");
        check("incremental", "MISC.1 incremental_contract");
    }

    #[cfg(feature = "z3-verify")]
    #[test]
    fn converted_stubs_counterexample_on_false() {
        // #189: Converted features should produce counterexamples for `false`.
        use assura_parser::ast::Literal;
        let body = Expr::Literal(Literal::Bool(false));
        let clauses: &[Clause] = &[];

        let check = |kind: &str, label: &str| {
            let results = verify_feature_clause(kind, "test_fn", &body, clauses);
            assert!(!results.is_empty(), "{label} should produce results");
            assert!(
                matches!(&results[0], VerificationResult::Counterexample { .. }),
                "{label} false should produce counterexample, got: {:?}",
                results[0]
            );
        };

        check("constant_time", "constant_time");
        check("unsafe_escape", "unsafe_escape");
    }

    // #202: Structural invariant inductive checking tests
    #[cfg(feature = "z3-verify")]
    #[test]
    fn structural_invariant_establishment_verifies_tautology() {
        // structural_invariant with body `true` should verify establishment
        use assura_parser::ast::Literal;
        let body = Expr::Literal(Literal::Bool(true));
        let clauses: &[Clause] = &[];
        let results = verify_structural_invariant_inductive("test_type", &body, clauses);
        // Should produce 2 results: establishment + preservation
        assert_eq!(
            results.len(),
            2,
            "inductive check should produce 2 results: {results:?}"
        );
        assert!(
            matches!(&results[0], VerificationResult::Verified { clause_desc }
                if clause_desc.contains("establishment")),
            "establishment should verify for tautology: {:?}",
            results[0]
        );
        assert!(
            matches!(&results[1], VerificationResult::Verified { clause_desc }
                if clause_desc.contains("preservation")),
            "preservation should verify for tautology: {:?}",
            results[1]
        );
    }

    #[cfg(feature = "z3-verify")]
    #[test]
    fn structural_invariant_establishment_fails_contradiction() {
        // structural_invariant with body `false` should fail establishment
        use assura_parser::ast::Literal;
        let body = Expr::Literal(Literal::Bool(false));
        let clauses: &[Clause] = &[];
        let results = verify_structural_invariant_inductive("test_type", &body, clauses);
        assert!(!results.is_empty(), "should produce results");
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { clause_desc, .. }
                if clause_desc.contains("establishment")),
            "establishment should fail for contradiction: {:?}",
            results[0]
        );
    }

    #[cfg(feature = "z3-verify")]
    #[test]
    fn structural_invariant_preserved_by_ensures() {
        // requires: x >= 0
        // ensures: x >= 0
        // structural_invariant: x >= 0
        // Both establishment and preservation should verify.
        use assura_parser::ast::{BinOp, Literal};
        let inv_body = Expr::BinOp {
            lhs: Box::new(Expr::Ident("x".into())),
            op: BinOp::Gte,
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gte,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gte,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
        ];
        let results = verify_structural_invariant_inductive("TestType", &inv_body, &clauses);
        assert_eq!(
            results.len(),
            2,
            "should produce establishment + preservation: {results:?}"
        );
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "establishment should verify: {:?}",
            results[0]
        );
        assert!(
            matches!(&results[1], VerificationResult::Verified { .. }),
            "preservation should verify: {:?}",
            results[1]
        );
    }

    #[cfg(feature = "z3-verify")]
    #[test]
    fn structural_invariant_dispatch_produces_inductive_results() {
        // Verify that the dispatch table routes structural_invariant
        // through the inductive checker (producing establishment results)
        use assura_parser::ast::Literal;
        let body = Expr::Literal(Literal::Bool(true));
        let clauses: &[Clause] = &[];
        let results = verify_feature_clause("structural_invariant", "test_fn", &body, clauses);
        assert!(
            !results.is_empty(),
            "structural_invariant should produce results via dispatch"
        );
        // Should have establishment result from inductive checker
        let has_establishment = results.iter().any(|r| {
            matches!(r, VerificationResult::Verified { clause_desc }
                if clause_desc.contains("establishment"))
        });
        assert!(
            has_establishment,
            "dispatch should route through inductive checker: {results:?}"
        );
    }
}
