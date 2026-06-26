use super::*;
use assura_ast::{BinOp, Clause, ClauseKind, Expr, Literal};

#[test]
fn verify_contract_single_ensures_verified() {
    // requires x > 0 ensures x > 0 (trivially true)
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
            effect_variables: vec![],
        },
    ];
    let results = verify_contract("TestContract", &clauses);
    assert_eq!(results.len(), 1, "one ensures clause: {results:?}");
    assert!(
        matches!(&results[0], VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("TestContract")),
        "should verify: {results:?}"
    );
}

#[test]
fn verify_contract_counterexample() {
    // No requires, ensures x > 0 (counterexample: x = 0)
    let clauses = vec![Clause {
        kind: ClauseKind::Ensures,
        body: Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        }),
        effect_variables: vec![],
    }];
    let results = verify_contract("NoPrecondition", &clauses);
    assert_eq!(results.len(), 1);
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { clause_desc, .. } if clause_desc.contains("NoPrecondition")),
        "should have counterexample: {results:?}"
    );
}

#[test]
fn verify_contract_multiple_ensures() {
    // requires x > 10
    // ensures x > 5  (verified)
    // ensures x > 20 (counterexample: x = 11)
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
            }),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
            }),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("20".into())))),
            }),
            effect_variables: vec![],
        },
    ];
    let results = verify_contract("MultiClause", &clauses);
    assert_eq!(results.len(), 2, "two ensures clauses: {results:?}");
    // First ensures (x > 5) should verify
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "x > 10 => x > 5 should verify: {:?}",
        results[0]
    );
    // Second ensures (x > 20) should have counterexample
    assert!(
        matches!(&results[1], VerificationResult::Counterexample { .. }),
        "x > 10 => x > 20 should fail: {:?}",
        results[1]
    );
}

#[test]
fn verify_contract_no_verifiable_clauses() {
    // Only requires, no ensures/invariant
    let clauses = vec![Clause {
        kind: ClauseKind::Requires,
        body: Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        }),
        effect_variables: vec![],
    }];
    let results = verify_contract("OnlyRequires", &clauses);
    assert!(results.is_empty(), "no verifiable clauses: {results:?}");
}

// ===================================================================
// #264: Incremental solving (push/pop) tests
// ===================================================================

#[test]
fn incremental_push_pop_three_clauses() {
    // Contract with 3 ensures clauses sharing the same requires.
    // Tests that incremental push/pop produces correct results for all 3.
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gte,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
            }),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("100".into())))),
            }),
            effect_variables: vec![],
        },
    ];
    let results = verify_contract("IncrementalPushPop", &clauses);
    assert_eq!(results.len(), 3, "expected 3 results, got {results:?}");
    // x > 0 => x > 0 (verified)
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "x > 0 => x > 0 should verify: {:?}",
        results[0]
    );
    // x > 0 => x >= 1 (verified for integers)
    assert!(
        matches!(&results[1], VerificationResult::Verified { .. }),
        "x > 0 => x >= 1 should verify: {:?}",
        results[1]
    );
    // x > 0 => x > 100 (counterexample)
    assert!(
        matches!(&results[2], VerificationResult::Counterexample { .. }),
        "x > 0 => x > 100 should have counterexample: {:?}",
        results[2]
    );
}

#[test]
fn incremental_correctness_verified_and_counterexample() {
    // Two clauses: requires { x > 0 }
    //   ensures { x > 0 }     -> verified
    //   ensures { x > 5 }     -> counterexample
    // The push/pop must isolate clause checks so the negation
    // of one does not leak into the next.
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
            }),
            effect_variables: vec![],
        },
    ];
    let results = verify_contract("IncrementalCorrectness", &clauses);
    assert_eq!(results.len(), 2, "expected 2 results, got {results:?}");
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "x > 0 => x > 0 should verify: {:?}",
        results[0]
    );
    assert!(
        matches!(&results[1], VerificationResult::Counterexample { .. }),
        "x > 0 => x > 5 should have counterexample: {:?}",
        results[1]
    );
}

#[test]
fn incremental_no_cross_contamination() {
    // Verify that a counterexample clause does not contaminate
    // the solver state for subsequent clauses.
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
            effect_variables: vec![],
        },
        // This will have a counterexample (not implied by x > 0)
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
            }),
            effect_variables: vec![],
        },
        // This MUST still verify (pop must remove negation of x > 10)
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
            effect_variables: vec![],
        },
    ];
    let results = verify_contract("NoCrossContamination", &clauses);
    assert_eq!(results.len(), 2, "expected 2 results, got {results:?}");
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "x > 0 => x > 10 should have counterexample: {:?}",
        results[0]
    );
    assert!(
        matches!(&results[1], VerificationResult::Verified { .. }),
        "x > 0 => x > 0 should verify after pop: {:?}",
        results[1]
    );
}
