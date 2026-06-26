use super::*;
use assura_ast::{BinOp, Expr, Literal, SpExpr};

/// Helper: verify_decrease with trivial preconditions.
fn check_decrease(measure: &SpExpr, call_arg: &SpExpr, desc: &str) -> VerificationResult {
    verify_decrease(&[], measure, call_arg, desc.to_string())
}

/// Helper: verify_decrease with preconditions.
fn check_decrease_with_pre(
    preconditions: &[SpExpr],
    measure: &SpExpr,
    call_arg: &SpExpr,
    desc: &str,
) -> VerificationResult {
    verify_decrease(preconditions, measure, call_arg, desc.to_string())
}

// -- Factorial: decreases n, calls with n-1, with requires n > 0 --

#[test]
fn factorial_terminates() {
    // decreases n, call arg = n - 1, precondition: n > 0
    let measure = Spanned::no_span(Expr::Ident("n".into()));
    let call_arg = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        op: BinOp::Sub,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
    });
    let pre = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        op: BinOp::Gt,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });
    let result = check_decrease_with_pre(&[pre], &measure, &call_arg, "factorial::decreases");
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "factorial should verify: {result:?}"
    );
}

// -- Fibonacci: decreases n, calls with n-1 and n-2 --

#[test]
fn fibonacci_n_minus_1_terminates() {
    let measure = Spanned::no_span(Expr::Ident("n".into()));
    let call_arg = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        op: BinOp::Sub,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
    });
    let pre = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        op: BinOp::Gt,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
    });
    let result = check_decrease_with_pre(&[pre], &measure, &call_arg, "fib::decreases(n-1)");
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "fib(n-1) should verify: {result:?}"
    );
}

#[test]
fn fibonacci_n_minus_2_terminates() {
    let measure = Spanned::no_span(Expr::Ident("n".into()));
    let call_arg = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        op: BinOp::Sub,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("2".into())))),
    });
    let pre = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        op: BinOp::Gt,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
    });
    let result = check_decrease_with_pre(&[pre], &measure, &call_arg, "fib::decreases(n-2)");
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "fib(n-2) should verify: {result:?}"
    );
}

// -- Non-decreasing: spin(n) calling spin(n) should NOT verify --

#[test]
fn spin_same_arg_does_not_terminate() {
    // decreases n, call arg = n (same, not decreasing)
    let measure = Spanned::no_span(Expr::Ident("n".into()));
    let call_arg = Spanned::no_span(Expr::Ident("n".into()));
    let result = check_decrease(&measure, &call_arg, "spin::decreases");
    assert!(
        !matches!(result, VerificationResult::Verified { .. }),
        "spin(n) calling spin(n) should NOT verify: {result:?}"
    );
}

// -- Increasing: bad(n) calling bad(n+1) should NOT verify --

#[test]
fn increasing_arg_does_not_terminate() {
    let measure = Spanned::no_span(Expr::Ident("n".into()));
    let call_arg = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        op: BinOp::Add,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
    });
    let result = check_decrease(&measure, &call_arg, "bad::decreases");
    assert!(
        !matches!(result, VerificationResult::Verified { .. }),
        "bad(n+1) should NOT verify: {result:?}"
    );
}

// -- With precondition ensuring non-negativity --

#[test]
fn decrease_with_nat_precondition() {
    // decreases n, call arg = n - 1, precondition: n >= 1
    let measure = Spanned::no_span(Expr::Ident("n".into()));
    let call_arg = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        op: BinOp::Sub,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
    });
    let pre = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        op: BinOp::Gte,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
    });
    let result = check_decrease_with_pre(&[pre], &measure, &call_arg, "countdown::decreases");
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "countdown with n >= 1 should verify: {result:?}"
    );
}
