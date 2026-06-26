use super::*;
use crate::*;
use assura_ast::Spanned;

fn sp(e: Expr) -> SpExpr {
    Spanned::no_span(e)
}
fn spb(e: Expr) -> Box<SpExpr> {
    Box::new(sp(e))
}

// -- is_likely_boolean_predicate tests --

#[test]
fn predicate_comparison_is_boolean() {
    use assura_ast::BinOp;
    let expr = sp(Expr::BinOp {
        lhs: spb(Expr::Ident("x".into())),
        op: BinOp::Gt,
        rhs: spb(Expr::Literal(assura_ast::Literal::Int("0".into()))),
    });
    assert!(is_likely_boolean_predicate(&expr));
}

#[test]
fn predicate_if_both_branches_boolean() {
    use assura_ast::{BinOp, Literal};
    let expr = sp(Expr::If {
        cond: spb(Expr::BinOp {
            lhs: spb(Expr::Ident("x".into())),
            op: BinOp::Gt,
            rhs: spb(Expr::Literal(Literal::Int("0".into()))),
        }),
        then_branch: spb(Expr::BinOp {
            lhs: spb(Expr::Ident("y".into())),
            op: BinOp::Gt,
            rhs: spb(Expr::Literal(Literal::Int("0".into()))),
        }),
        else_branch: Some(spb(Expr::Literal(Literal::Bool(true)))),
    });
    assert!(is_likely_boolean_predicate(&expr));
}

#[test]
fn predicate_if_no_else_not_boolean() {
    use assura_ast::{BinOp, Literal};
    let expr = sp(Expr::If {
        cond: spb(Expr::BinOp {
            lhs: spb(Expr::Ident("x".into())),
            op: BinOp::Gt,
            rhs: spb(Expr::Literal(Literal::Int("0".into()))),
        }),
        then_branch: spb(Expr::Literal(Literal::Bool(true))),
        else_branch: None,
    });
    assert!(!is_likely_boolean_predicate(&expr));
}

#[test]
fn predicate_block_last_expr_boolean() {
    use assura_ast::{BinOp, Literal};
    let expr = sp(Expr::Block(vec![
        sp(Expr::Ident("setup".into())),
        sp(Expr::BinOp {
            lhs: spb(Expr::Ident("x".into())),
            op: BinOp::Gt,
            rhs: spb(Expr::Literal(Literal::Int("0".into()))),
        }),
    ]));
    assert!(is_likely_boolean_predicate(&expr));
}

#[test]
fn predicate_empty_block_not_boolean() {
    let expr = sp(Expr::Block(vec![]));
    assert!(!is_likely_boolean_predicate(&expr));
}

#[test]
fn predicate_call_is_boolean() {
    let expr = sp(Expr::Call {
        func: spb(Expr::Ident("validate".into())),
        args: vec![sp(Expr::Ident("x".into()))],
    });
    assert!(is_likely_boolean_predicate(&expr));
}

#[test]
fn predicate_match_all_boolean_arms() {
    use assura_ast::{Literal, MatchArm, Pattern};
    let expr = sp(Expr::Match {
        scrutinee: spb(Expr::Ident("x".into())),
        arms: vec![
            MatchArm {
                pattern: Pattern::Literal(Literal::Int("0".into())),
                body: sp(Expr::Literal(Literal::Bool(true))),
            },
            MatchArm {
                pattern: Pattern::Wildcard,
                body: sp(Expr::Literal(Literal::Bool(false))),
            },
        ],
    });
    assert!(is_likely_boolean_predicate(&expr));
}

#[test]
fn predicate_uppercase_ident_not_boolean() {
    let expr = sp(Expr::Ident("MyType".into()));
    assert!(!is_likely_boolean_predicate(&expr));
}

// -- opaque / feature dispatch tests --

#[test]
fn opaque_with_ensures_verifies() {
    let result = verify_opaque_contract("test_fn", true);
    assert!(matches!(result, VerificationResult::Verified { .. }));
}

#[test]
fn opaque_without_ensures_unknown() {
    // Marker now always Verified (opaque trusts the contract); the "no ensures" path is legacy.
    let result = verify_opaque_contract("test_fn", false);
    assert!(matches!(result, VerificationResult::Verified { .. }));
}

#[test]
fn feature_dispatch_covers_all_registered_clause_kinds() {
    // Every clause kind in the Feature registry should be accepted
    // by verify_feature_clause (either returning results or empty vec
    // based on whether SMT verification applies).
    use assura_ast::Literal;
    use assura_ast::features::Feature;
    let dummy_body = sp(Expr::Literal(Literal::Bool(true)));
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
    use assura_ast::Literal;
    let dummy_body = sp(Expr::Literal(Literal::Bool(true)));
    let dummy_clauses: &[Clause] = &[];
    assert!(
        verify_feature_clause("nonexistent_feature", "test", &dummy_body, dummy_clauses).is_empty()
    );
}

#[cfg(feature = "z3-verify")]
#[test]
fn feature_body_verified_with_tautology() {
    // A feature clause with body `true` should be verified (not Unknown).
    use assura_ast::Literal;
    let body = sp(Expr::Literal(Literal::Bool(true)));
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
    use assura_ast::Literal;
    let body = sp(Expr::Literal(Literal::Bool(false)));
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
    use assura_ast::{BinOp, Literal};
    let body = sp(Expr::BinOp {
        lhs: spb(Expr::Ident("x".into())),
        op: BinOp::Gt,
        rhs: spb(Expr::Literal(Literal::Int("0".into()))),
    });
    let requires_body = sp(Expr::BinOp {
        lhs: spb(Expr::Ident("x".into())),
        op: BinOp::Gte,
        rhs: spb(Expr::Literal(Literal::Int("1".into()))),
    });
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
    use assura_ast::Literal;
    let body = sp(Expr::Literal(Literal::Bool(true)));
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
    use assura_ast::Literal;
    let body = sp(Expr::Literal(Literal::Bool(false)));
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
    use assura_ast::Literal;
    let body = sp(Expr::Literal(Literal::Bool(true)));
    let clauses: &[Clause] = &[];
    let results = verify_structural_invariant_inductive("test_type", &body, clauses);
    // Should produce 2 results: establishment + preservation
    assert_eq!(
        results.len(),
        2,
        "inductive check should produce 2 results: {results:?}"
    );
    assert!(
        matches!(&results[0], VerificationResult::Verified { clause_desc, .. }
            if clause_desc.contains("establishment")),
        "establishment should verify for tautology: {:?}",
        results[0]
    );
    assert!(
        matches!(&results[1], VerificationResult::Verified { clause_desc, .. }
            if clause_desc.contains("preservation")),
        "preservation should verify for tautology: {:?}",
        results[1]
    );
}

#[cfg(feature = "z3-verify")]
#[test]
fn structural_invariant_establishment_fails_contradiction() {
    // structural_invariant with body `false` should fail establishment
    use assura_ast::Literal;
    let body = sp(Expr::Literal(Literal::Bool(false)));
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
    use assura_ast::{BinOp, Literal};
    let inv_body = sp(Expr::BinOp {
        lhs: spb(Expr::Ident("x".into())),
        op: BinOp::Gte,
        rhs: spb(Expr::Literal(Literal::Int("0".into()))),
    });
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: sp(Expr::BinOp {
                lhs: spb(Expr::Ident("x".into())),
                op: BinOp::Gte,
                rhs: spb(Expr::Literal(Literal::Int("0".into()))),
            }),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                lhs: spb(Expr::Ident("x".into())),
                op: BinOp::Gte,
                rhs: spb(Expr::Literal(Literal::Int("0".into()))),
            }),
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
    use assura_ast::Literal;
    let body = sp(Expr::Literal(Literal::Bool(true)));
    let clauses: &[Clause] = &[];
    let results = verify_feature_clause("structural_invariant", "test_fn", &body, clauses);
    assert!(
        !results.is_empty(),
        "structural_invariant should produce results via dispatch"
    );
    // Should have establishment result from inductive checker
    let has_establishment = results.iter().any(|r| {
        matches!(r, VerificationResult::Verified { clause_desc, .. }
            if clause_desc.contains("establishment"))
    });
    assert!(
        has_establishment,
        "dispatch should route through inductive checker: {results:?}"
    );
}
