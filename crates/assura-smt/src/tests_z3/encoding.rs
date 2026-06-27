use super::*;

// -----------------------------------------------------------------------
// #198: Deep field chain SMT encoding
// -----------------------------------------------------------------------

#[test]
fn deep_field_chain_ensures_verifies() {
    // Deep field chain: state.head.extra.extra_max should be flattened
    // to a single Z3 variable, making the ensures verifiable.
    let src = r#"
        contract DeepChain {
            requires: x.head.extra.extra_max >= 0
            ensures: x.head.extra.extra_max >= 0
        }
    "#;
    let results = verify_source(src);
    assert!(
        !results.is_empty(),
        "should have results for deep field chain"
    );
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "deep field chain ensures should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn deep_field_chain_not_unmodelable() {
    use z3_backend::encoder::expr_has_unmodelable_features;
    // state.head.extra.extra_max should NOT be unmodelable
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Field(
            Box::new(Spanned::no_span(Expr::Field(
                Box::new(Spanned::no_span(Expr::Ident("state".into()))),
                "head".into(),
            ))),
            "extra".into(),
        ))),
        "extra_max".into(),
    ));
    assert!(
        !expr_has_unmodelable_features(&expr),
        "deep field chain should be modelable after #198"
    );
}

// -----------------------------------------------------------------------
// #200: Taint/validate/ghost/region SMT encoding
// -----------------------------------------------------------------------

#[test]
fn taint_keyword_not_unmodelable() {
    use z3_backend::encoder::expr_has_unmodelable_features;
    // Raw tokens with taint/ghost/region but no @ should be modelable
    let expr = Spanned::no_span(Expr::Raw(vec![
        "taint".into(),
        "untrusted".into(),
        "x".into(),
        ">=".into(),
        "0".into(),
    ]));
    assert!(
        !expr_has_unmodelable_features(&expr),
        "taint keywords should be modelable after #200"
    );
}

#[test]
fn typestate_at_now_modelable() {
    use z3_backend::encoder::expr_has_unmodelable_features;
    // #262: Raw tokens with @ are now modelable (encoded as integer equality)
    let expr = Spanned::no_span(Expr::Raw(vec![
        "state".into(),
        ".".into(),
        "status".into(),
        "@".into(),
        "Active".into(),
    ]));
    assert!(
        !expr_has_unmodelable_features(&expr),
        "typestate @ annotation should be modelable after #262"
    );
}

// -----------------------------------------------------------------------
// #262: Typestate @ encoding as integer equality
// -----------------------------------------------------------------------

#[test]
fn z3_typestate_same_state_verifies() {
    use assura_ast::{Clause, ClauseKind, Expr};
    // requires { file @ Open }
    // ensures  { file @ Open }
    // Same typestate in pre and post => should verify
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Spanned::no_span(Expr::Raw(vec!["file".into(), "@".into(), "Open".into()])),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::Raw(vec!["file".into(), "@".into(), "Open".into()])),
            effect_variables: vec![],
        },
    ];
    let results = crate::verify_contract("TypestateIdentity", &clauses);
    assert!(
        !results.is_empty(),
        "should have results for typestate identity"
    );
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "same typestate pre/post should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn z3_typestate_different_state_counterexample() {
    use assura_ast::{Clause, ClauseKind, Expr};
    // requires { file @ Open }
    // ensures  { file @ Closed }
    // Different typestate in pre and post => counterexample
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Spanned::no_span(Expr::Raw(vec!["file".into(), "@".into(), "Open".into()])),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::Raw(vec!["file".into(), "@".into(), "Closed".into()])),
            effect_variables: vec![],
        },
    ];
    let results = crate::verify_contract("TypestateMismatch", &clauses);
    assert!(
        !results.is_empty(),
        "should have results for typestate mismatch"
    );
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "different typestate pre/post should produce counterexample, got: {:?}",
        results[0]
    );
}

#[test]
fn z3_typestate_mismatch_completes_without_hang() {
    use assura_ast::{Clause, ClauseKind, Expr};
    use std::time::{Duration, Instant};

    // Regression (#264): eager Option ADT forall axioms made this SAT query hang.
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Spanned::no_span(Expr::Raw(vec!["file".into(), "@".into(), "Open".into()])),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::Raw(vec!["file".into(), "@".into(), "Closed".into()])),
            effect_variables: vec![],
        },
    ];
    let start = Instant::now();
    let results = crate::verify_contract("TypestateMismatchPerf", &clauses);
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "typestate mismatch check should finish quickly, took {:?}",
        start.elapsed()
    );
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "expected counterexample, got: {:?}",
        results[0]
    );
}

#[test]
fn z3_typestate_with_dot_field() {
    use assura_ast::{Clause, ClauseKind, Expr};
    // requires { conn.state @ Connected }
    // ensures  { conn.state @ Connected }
    // Dot-separated field + typestate should verify
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Spanned::no_span(Expr::Raw(vec![
                "conn".into(),
                ".".into(),
                "state".into(),
                "@".into(),
                "Connected".into(),
            ])),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::Raw(vec![
                "conn".into(),
                ".".into(),
                "state".into(),
                "@".into(),
                "Connected".into(),
            ])),
            effect_variables: vec![],
        },
    ];
    let results = crate::verify_contract("TypestateField", &clauses);
    assert!(
        !results.is_empty(),
        "should have results for typestate with field access"
    );
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "typestate with dot field should verify, got: {:?}",
        results[0]
    );
}

// -----------------------------------------------------------------------
// #201: Unknown method call encoding as uninterpreted functions
// -----------------------------------------------------------------------

#[test]
fn unknown_method_call_not_unmodelable() {
    use z3_backend::encoder::expr_has_unmodelable_features;
    // Method calls should NOT be unmodelable (encoded as uninterpreted functions)
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(Spanned::no_span(Expr::Ident("data".into()))),
        method: "custom_check".into(),
        args: vec![Spanned::no_span(Expr::Ident("x".into()))],
    });
    assert!(
        !expr_has_unmodelable_features(&expr),
        "unknown method calls should be modelable after #201"
    );
}

#[test]
fn field_access_not_unmodelable() {
    use z3_backend::encoder::expr_has_unmodelable_features;
    // Field access (even unknown fields) should be modelable
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Ident("obj".into()))),
        "custom_field".into(),
    ));
    assert!(
        !expr_has_unmodelable_features(&expr),
        "field access should be modelable after #198"
    );
}

// -----------------------------------------------------------------------
// #261 — Native string theory support behind config flag
// -----------------------------------------------------------------------

#[test]
fn test_string_theory_literal_z3() {
    use crate::z3_backend::encoder::{Encoder, Z3Value};
    use assura_ast::{Expr, Literal};
    z3::with_z3_config(&z3::Config::new(), || {
        // With string_theory=true, string literals produce Z3Value::Str
        let mut encoder = Encoder::with_string_theory(true);
        let val = encoder.encode_expr(&Spanned::no_span(Expr::Literal(Literal::Str(
            "hello".into(),
        ))));
        assert!(
            matches!(val, Z3Value::Str(_)),
            "With string_theory=true, string literals must produce Z3Value::Str"
        );
        // Background axiom for length == 5 should be present
        assert!(
            !encoder.background_axioms.is_empty(),
            "String theory literal must produce a length axiom"
        );
    });
}

#[test]
fn test_string_theory_default_uses_int() {
    use crate::z3_backend::encoder::{Encoder, Z3Value};
    use assura_ast::{Expr, Literal};
    z3::with_z3_config(&z3::Config::new(), || {
        // Default (string_theory=false): string literals produce Z3Value::Int
        let mut encoder = Encoder::new();
        assert!(!encoder.use_string_theory);
        let val = encoder.encode_expr(&Spanned::no_span(Expr::Literal(Literal::Str(
            "hello".into(),
        ))));
        assert!(
            matches!(val, Z3Value::Int(_)),
            "Default encoding must produce Z3Value::Int for strings"
        );
    });
}

#[test]
fn test_string_theory_length_z3() {
    use crate::z3_backend::encoder::{Encoder, Z3Value};
    use assura_ast::{Expr, Literal};
    z3::with_z3_config(&z3::Config::new(), || {
        let mut encoder = Encoder::with_string_theory(true);
        // Encode "abc".length -> should use native str.len, producing an Int
        let str_expr = Spanned::no_span(Expr::Literal(Literal::Str("abc".into())));
        let field_expr = Spanned::no_span(Expr::Field(Box::new(str_expr), "length".into()));
        let val = encoder.encode_expr(&field_expr);
        assert!(
            matches!(val, Z3Value::Int(_)),
            "String .length() must return Z3Value::Int"
        );
    });
}

#[test]
fn test_string_theory_equality_z3() {
    use crate::z3_backend::encoder::{Encoder, Z3Value};
    use assura_ast::{BinOp, Expr, Literal};
    z3::with_z3_config(&z3::Config::new(), || {
        let mut encoder = Encoder::with_string_theory(true);
        // "hello" == "hello" should use native string equality
        let eq_expr = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Str(
                "hello".into(),
            )))),
            op: BinOp::Eq,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Str(
                "hello".into(),
            )))),
        });
        let val = encoder.encode_expr(&eq_expr);
        assert!(
            matches!(val, Z3Value::Bool(_)),
            "String equality must return Z3Value::Bool"
        );
    });
}

// =======================================================================
// #264: Z3 incremental solving (push/pop)
// =======================================================================

#[test]
fn z3_incremental_push_pop_multi_clause() {
    // Contract with 3 clauses sharing the same requires.
    // The Z3 backend should use a single solver with push/pop
    // and produce correct results for all 3 clauses.
    let source = r#"
contract MultiClause {
  requires { x > 0 }
  ensures { x > 0 }
  ensures { x >= 1 }
  ensures { x > 100 }
}
"#;
    let results = verify_source(source);
    assert_eq!(results.len(), 3, "expected 3 results, got {results:?}");
    // x > 0 => x > 0 (trivially verified)
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "x > 0 => x > 0 should verify, got: {:?}",
        results[0]
    );
    // x > 0 => x >= 1 (verified: x > 0 means x >= 1 for integers)
    assert!(
        matches!(&results[1], VerificationResult::Verified { .. }),
        "x > 0 => x >= 1 should verify, got: {:?}",
        results[1]
    );
    // x > 0 => x > 100 (counterexample: e.g. x = 1)
    assert!(
        matches!(&results[2], VerificationResult::Counterexample { .. }),
        "x > 0 => x > 100 should have counterexample, got: {:?}",
        results[2]
    );
}

#[test]
fn z3_incremental_correctness_verified_and_counterexample() {
    // Two clauses: one verifiable, one not. The push/pop mechanism
    // must not let clause-specific assertions leak between checks.
    let source = r#"
contract IncrementalCorrectness {
  requires { x > 0 }
  ensures { x > 0 }
  ensures { x > 5 }
}
"#;
    let results = verify_source(source);
    assert_eq!(results.len(), 2, "expected 2 results, got {results:?}");
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "x > 0 => x > 0 should verify, got: {:?}",
        results[0]
    );
    assert!(
        matches!(&results[1], VerificationResult::Counterexample { .. }),
        "x > 0 => x > 5 should have counterexample, got: {:?}",
        results[1]
    );
}

#[test]
fn z3_incremental_single_clause_still_works() {
    // Single clause contract should still work (push/pop with one clause)
    let source = r#"
contract SingleClause {
  requires { y >= 0 }
  ensures { y >= 0 }
}
"#;
    let results = verify_source(source);
    assert_eq!(results.len(), 1, "expected 1 result, got {results:?}");
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "single clause should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn z3_incremental_no_cross_contamination() {
    // Verify that a counterexample clause (x > 10) does not
    // contaminate the solver state for the next clause (x > 0).
    // If push/pop is broken, the negation of x > 10 would persist.
    let source = r#"
contract NoCrossContamination {
  requires { x > 0 }
  ensures { x > 10 }
  ensures { x > 0 }
}
"#;
    let results = verify_source(source);
    assert_eq!(results.len(), 2, "expected 2 results, got {results:?}");
    // First clause: x > 0 does NOT imply x > 10
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "x > 0 => x > 10 should have counterexample, got: {:?}",
        results[0]
    );
    // Second clause: x > 0 => x > 0 must still verify
    // (push/pop must have removed the negated x > 10 assertion)
    assert!(
        matches!(&results[1], VerificationResult::Verified { .. }),
        "x > 0 => x > 0 should verify after pop, got: {:?}",
        results[1]
    );
}

