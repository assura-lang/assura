use super::*;
use assura_ast::{BinOp, Expr, Literal};

#[test]
fn forall_trivially_true() {
    // forall x in 0..10: x == x (always true)
    let body = Spanned::no_span(Expr::Forall {
        var: "x".into(),
        domain: Box::new(Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            op: BinOp::Range,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
        })),
        body: Box::new(Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Eq,
            rhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        })),
    });
    let result = verify_quantified_expr("trivial_forall", &[], &body);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "forall x in 0..10: x == x should verify: {result:?}"
    );
}

#[test]
fn forall_with_counterexample() {
    // forall x in 0..10: x > 0 (false: x = 0 is a counterexample)
    let body = Spanned::no_span(Expr::Forall {
        var: "x".into(),
        domain: Box::new(Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            op: BinOp::Range,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
        })),
        body: Box::new(Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        })),
    });
    let result = verify_quantified_expr("nonpositive_forall", &[], &body);
    assert!(
        matches!(result, VerificationResult::Counterexample { .. }),
        "forall x in 0..10: x > 0 should have counterexample: {result:?}"
    );
}

#[test]
fn exists_trivially_satisfiable() {
    // exists x in 0..100: x > 5 (true: e.g. x = 6)
    let body = Spanned::no_span(Expr::Exists {
        var: "x".into(),
        domain: Box::new(Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            op: BinOp::Range,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("100".into())))),
        })),
        body: Box::new(Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
        })),
    });
    let result = verify_quantified_expr("trivial_exists", &[], &body);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "exists x in 0..100: x > 5 should verify: {result:?}"
    );
}

#[test]
fn forall_with_assumption() {
    // Assumption: n > 0
    // Check: forall x in 0..10: n + x >= x (always true when n > 0)
    let assumption = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        op: BinOp::Gt,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });
    let body = Spanned::no_span(Expr::Forall {
        var: "x".into(),
        domain: Box::new(Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            op: BinOp::Range,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
        })),
        body: Box::new(Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
                op: BinOp::Add,
                rhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            })),
            op: BinOp::Gte,
            rhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        })),
    });
    let result = verify_quantified_expr("forall_with_pre", &[assumption], &body);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "forall x in 0..10: n + x >= x with n > 0 should verify: {result:?}"
    );
}

#[test]
fn layer2_verifier_verify_method() {
    // Test the Layer2Verifier.verify() method
    let config = Layer2Config::default();
    let verifier = Layer2Verifier::new(config);
    let results = verifier.verify();
    assert!(results.is_empty(), "empty verifier returns no results");
}

#[test]
fn layer2_verifier_with_invariant() {
    let config = Layer2Config::new().with_timeout(5000);
    let mut verifier = Layer2Verifier::new(config);
    verifier.add_invariant(QuantifiedInvariant {
        name: "sorted_invariant".into(),
        bound_vars: vec![("i".into(), "Int".into())],
        body: "i >= 0".into(),
        triggers: Vec::new(),
    });
    let results = verifier.verify();
    assert_eq!(results.len(), 1);
    // "i >= 0" is NOT universally true (i = -1 is a counterexample)
    assert!(matches!(results[0], Layer2Result::Counterexample { .. }));
}
