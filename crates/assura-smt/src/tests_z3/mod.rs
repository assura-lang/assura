use super::*;
use assura_ast::{BinOp, Expr, Literal, SpExpr, Spanned};

fn verify_source(source: &str) -> Vec<VerificationResult> {
    let typed = assura_test_support::typecheck_ok(source);
    verify(&typed)
}

/// Helper: build `Expr::BinOp { lhs, op, rhs }`.
fn binop(lhs: SpExpr, op: BinOp, rhs: SpExpr) -> SpExpr {
    Spanned::no_span(Expr::BinOp {
        lhs: Box::new(lhs),
        op,
        rhs: Box::new(rhs),
    })
}

/// Helper: build `Expr::Ident(name)`.
fn ident(name: &str) -> SpExpr {
    Spanned::no_span(Expr::Ident(name.to_string()))
}

/// Helper: build `Expr::Literal(Literal::Int(n))`.
fn int_lit(n: i64) -> SpExpr {
    Spanned::no_span(Expr::Literal(Literal::Int(n.to_string())))
}

mod core;
mod counterexample;
mod encoding;
mod frame;
mod old_field;
mod raw_ops;
mod refinement;
mod regressions;
mod taint_measures;
mod theory_verifiers;
