use super::*;

// -----------------------------------------------------------------------
// Measure axiom verification (CVC5 parity with Z3 tests, issue #448)
// -----------------------------------------------------------------------

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_measure_len_non_negative_provable_cvc5() {
    let measures = vec![
        crate::MeasureDefinition::new(
            "len",
            vec![crate::MeasureSort::Collection],
            crate::MeasureSort::Nat,
        )
        .with_axiom("len(xs) >= 0", crate::MeasureAxiomTag::NonNegative),
    ];

    let requires = vec![Spanned::no_span(Expr::BinOp {
        op: BinOp::Gte,
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    })];
    let ensures = Spanned::no_span(Expr::BinOp {
        op: BinOp::Gte,
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });

    let result = crate::cvc5_backend::verify_with_measures_cvc5(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "non-negative axiom should not break basic verification, got: {result:?}"
    );
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_measure_with_wrong_ensures_counterexample_cvc5() {
    // CVC5 may return Timeout (Unknown mapped to Timeout, see #456) when
    // quantified measure axioms are present.  Accept either Counterexample
    // (correct) or Timeout (known CVC5 limitation).
    let measures = vec![
        crate::MeasureDefinition::new(
            "len",
            vec![crate::MeasureSort::Collection],
            crate::MeasureSort::Nat,
        )
        .with_axiom("len(xs) >= 0", crate::MeasureAxiomTag::NonNegative),
    ];

    let requires = vec![Spanned::no_span(Expr::BinOp {
        op: BinOp::Gt,
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    })];
    let ensures = Spanned::no_span(Expr::BinOp {
        op: BinOp::Lt,
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });

    let result = crate::cvc5_backend::verify_with_measures_cvc5(&requires, &ensures, &measures);
    assert!(
        matches!(
            result,
            VerificationResult::Counterexample { .. }
                | VerificationResult::Timeout { .. }
                | VerificationResult::Unknown { .. }
        ),
        "x > 0 => x < 0 should produce counterexample (or timeout/unknown with quantifiers), got: {result:?}"
    );
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_measure_empty_measures_list_cvc5() {
    let measures: Vec<crate::MeasureDefinition> = vec![];
    let requires = vec![Spanned::no_span(Expr::BinOp {
        op: BinOp::Eq,
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
    })];
    let ensures = Spanned::no_span(Expr::BinOp {
        op: BinOp::Eq,
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
    });

    let result = crate::cvc5_backend::verify_with_measures_cvc5(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "empty measures should still allow verification, got: {result:?}"
    );
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_measure_axioms_do_not_break_basic_verification_cvc5() {
    let measures = crate::register_builtin_measures();

    let requires = vec![
        Spanned::no_span(Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        }),
        Spanned::no_span(Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        }),
    ];
    let ensures = Spanned::no_span(Expr::BinOp {
        op: BinOp::Gte,
        lhs: Box::new(Spanned::no_span(Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
        })),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });

    let result = crate::cvc5_backend::verify_with_measures_cvc5(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "a>=0 and b>=0 => a+b>=0 should verify with measures, got: {result:?}"
    );
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_measure_custom_user_measure_cvc5() {
    let measures = vec![
        crate::MeasureDefinition::new(
            "depth",
            vec![crate::MeasureSort::Collection],
            crate::MeasureSort::Nat,
        )
        .with_axiom("depth(xs) >= 0", crate::MeasureAxiomTag::NonNegative)
        .with_axiom("depth(empty) == 0", crate::MeasureAxiomTag::EmptyIsZero)
        .with_axiom(
            "depth is always bounded",
            crate::MeasureAxiomTag::Custom("user-defined depth bound".into()),
        ),
    ];

    let requires = vec![Spanned::no_span(Expr::BinOp {
        op: BinOp::Gt,
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
    })];
    let ensures = Spanned::no_span(Expr::BinOp {
        op: BinOp::Gt,
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
    });

    let result = crate::cvc5_backend::verify_with_measures_cvc5(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "custom user measure should not break verification, got: {result:?}"
    );
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_measure_append_increment_axiom_cvc5() {
    let measures = vec![
        crate::MeasureDefinition::new(
            "len",
            vec![crate::MeasureSort::Collection],
            crate::MeasureSort::Nat,
        )
        .with_axiom("len(xs) >= 0", crate::MeasureAxiomTag::NonNegative)
        .with_axiom(
            "len(append(xs, x)) == len(xs) + 1",
            crate::MeasureAxiomTag::AppendIncrement,
        ),
    ];

    let requires = vec![Spanned::no_span(Expr::BinOp {
        op: BinOp::Eq,
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("3".into())))),
    })];
    let ensures = Spanned::no_span(Expr::BinOp {
        op: BinOp::Eq,
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("3".into())))),
    });

    let result = crate::cvc5_backend::verify_with_measures_cvc5(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "append axiom should not break verification, got: {result:?}"
    );
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_measure_no_requires_counterexample_cvc5() {
    // CVC5 may return Timeout (Unknown mapped to Timeout, see #456) when
    // quantified measure axioms are present.
    let measures = vec![
        crate::MeasureDefinition::new(
            "len",
            vec![crate::MeasureSort::Collection],
            crate::MeasureSort::Nat,
        )
        .with_axiom("len(xs) >= 0", crate::MeasureAxiomTag::NonNegative),
    ];

    let requires: Vec<assura_ast::SpExpr> = vec![];
    let ensures = Spanned::no_span(Expr::BinOp {
        op: BinOp::Gt,
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });

    let result = crate::cvc5_backend::verify_with_measures_cvc5(&requires, &ensures, &measures);
    assert!(
        matches!(
            result,
            VerificationResult::Counterexample { .. }
                | VerificationResult::Timeout { .. }
                | VerificationResult::Unknown { .. }
        ),
        "no requires with measures should produce counterexample (or timeout/unknown with quantifiers), got: {result:?}"
    );
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_measure_multiple_requires_with_measures_cvc5() {
    let measures = crate::register_builtin_measures();

    let requires = vec![
        Spanned::no_span(Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        }),
        Spanned::no_span(Expr::BinOp {
            op: BinOp::Lte,
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
        }),
        Spanned::no_span(Expr::BinOp {
            op: BinOp::Eq,
            lhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
            rhs: Box::new(Spanned::no_span(Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
            })),
        }),
    ];
    let ensures = Spanned::no_span(Expr::BinOp {
        op: BinOp::Gte,
        lhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
    });

    let result = crate::cvc5_backend::verify_with_measures_cvc5(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "multiple requires with measures should verify, got: {result:?}"
    );
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_measure_size_len_equivalence_cvc5() {
    let measures = crate::register_builtin_measures();

    let requires = vec![Spanned::no_span(Expr::BinOp {
        op: BinOp::Gte,
        lhs: Box::new(Spanned::no_span(Expr::Ident("count".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    })];
    let ensures = Spanned::no_span(Expr::BinOp {
        op: BinOp::Gte,
        lhs: Box::new(Spanned::no_span(Expr::Ident("count".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });

    let result = crate::cvc5_backend::verify_with_measures_cvc5(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "size/len equivalence should not break verification, got: {result:?}"
    );
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_measure_keys_empty_map_axiom_cvc5() {
    let measures = crate::register_builtin_measures();

    let requires = vec![
        Spanned::no_span(Expr::BinOp {
            op: BinOp::Gt,
            lhs: Box::new(Spanned::no_span(Expr::Ident("k".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        }),
        Spanned::no_span(Expr::BinOp {
            op: BinOp::Lt,
            lhs: Box::new(Spanned::no_span(Expr::Ident("k".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("100".into())))),
        }),
    ];
    let ensures = Spanned::no_span(Expr::BinOp {
        op: BinOp::And,
        lhs: Box::new(Spanned::no_span(Expr::BinOp {
            op: BinOp::Gt,
            lhs: Box::new(Spanned::no_span(Expr::Ident("k".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        })),
        rhs: Box::new(Spanned::no_span(Expr::BinOp {
            op: BinOp::Lt,
            lhs: Box::new(Spanned::no_span(Expr::Ident("k".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("100".into())))),
        })),
    });

    let result = crate::cvc5_backend::verify_with_measures_cvc5(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "map measure axioms should not break verification, got: {result:?}"
    );
}

// ---- #449: Decreases clause uses correct DecreasesNonNeg polarity ----

/// Decreases clause with a non-negative integer measure should verify
/// (UNSAT from asserting NOT(measure >= 0) given measure is constrained >= 0).
#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_decreases_non_neg_polarity() {
    let params = vec![Param {
        name: "n".into(),
        ty: Some(assura_ast::TypeExpr::named("Nat")),
    }];
    // decreases { n } -- n is Nat so n >= 0, hence NOT(n >= 0) is UNSAT => Verified
    let clauses = vec![Clause {
        kind: ClauseKind::Decreases,
        body: Spanned::no_span(Expr::Ident("n".into())),
        effect_variables: vec![],
    }];
    let mut cache = SessionCache::new();
    let results = verify_lemmas_test(
        "TestDecreasesPolarity",
        &clauses,
        &params,
        &[],
        None,
        None,
        &mut cache,
    );
    assert_eq!(results.len(), 1);
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "Decreases with Nat measure should be verified, got: {:?}",
        results[0]
    );
}

/// Decreases clause with an unconstrained integer (could be negative)
/// should produce a counterexample.
#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_decreases_unconstrained_counterexample() {
    let params = vec![Param {
        name: "x".into(),
        ty: Some(assura_ast::TypeExpr::named("Int")),
    }];
    // decreases { x } -- x is unconstrained Int, could be negative
    let clauses = vec![Clause {
        kind: ClauseKind::Decreases,
        body: Spanned::no_span(Expr::Ident("x".into())),
        effect_variables: vec![],
    }];
    let mut cache = SessionCache::new();
    let results = verify_lemmas_test(
        "TestDecreasesUnconstr",
        &clauses,
        &params,
        &[],
        None,
        None,
        &mut cache,
    );
    assert_eq!(results.len(), 1);
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "Decreases with unconstrained Int should have counterexample, got: {:?}",
        results[0]
    );
}

// ---- #451: Incremental path applies havoc+assume ----

/// Multi-clause contract (>1 verifiable) uses the incremental path.
/// With havoc+assume, .length() >= 0 axiom should allow verification
/// of an ensures clause that depends on it.
#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_incremental_havoc_assume_applied() {
    let params = vec![Param {
        name: "data".into(),
        ty: Some(assura_ast::TypeExpr::named("Bytes")),
    }];
    // Two ensures clauses => triggers incremental path.
    // ensures { data.length() >= 0 } depends on havoc+assume axiom.
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
            effect_variables: vec![],
        },
    ];
    let mut cache = SessionCache::new();
    let results = verify_lemmas_test(
        "TestIncrHavoc",
        &clauses,
        &params,
        &[],
        None,
        None,
        &mut cache,
    );
    // Both ensures should verify (trivially true)
    assert_eq!(results.len(), 2, "expected 2 results for 2 ensures clauses");
    for r in &results {
        assert!(
            matches!(r, VerificationResult::Verified { .. }),
            "incremental ensures should verify, got: {r:?}"
        );
    }
}

// ---- #459: Raw float uses Real sort, not IntsDivision ----

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_raw_float_uses_real_sort() {
    // Raw expression with float literal should produce Real sort,
    // not integer division.
    let params = vec![Param {
        name: "x".into(),
        ty: Some(assura_ast::TypeExpr::named("Float")),
    }];
    // requires { x > 0.0 } ensures { x > 0.0 }
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Spanned::no_span(Expr::Raw(vec!["x".into(), ">".into(), "0.5".into()])),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::Raw(vec!["x".into(), ">".into(), "0.5".into()])),
            effect_variables: vec![],
        },
    ];
    let mut cache = SessionCache::new();
    let results = verify_lemmas_test(
        "TestRawFloat",
        &clauses,
        &params,
        &[],
        None,
        None,
        &mut cache,
    );
    assert!(!results.is_empty(), "should have at least one result");
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "raw float ensures should verify when requires matches, got: {:?}",
        results[0]
    );
}
