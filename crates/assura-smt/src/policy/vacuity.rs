//! Clause-level vacuity that does not need a second solver query.
//!
//! An `ensures` or `invariant` that restates a `requires` (including a flipped
//! comparison) proves nothing. On a uniform `Nat` contract, a sum or product
//! compared to `u64::MAX` is true for every input because `+` and `*` wrap.
//! Mixed `Int`/`Nat` arithmetic stays unbounded, so that shape is not stamped.
//! Both checks are applied onto [`crate::VerificationResult::Verified`] after
//! each backend's clause loop so Z3 and CVC5 stay aligned.

use assura_ast::{BinOp, Clause, ClauseKind, Expr, Literal, SpExpr};

use crate::VerificationResult;

pub(crate) const RESTATES_REQUIRES: &str = "restates a requires";
pub(crate) const NAT_WRAP_CEILING: &str = "true for every Nat because + and * wrap at 2^64";
pub(crate) const HOLDS_WITHOUT_REQUIRES: &str = "holds without the requires";

/// Prelude facts that stay when user `requires` are omitted.
#[derive(Clone, Copy)]
pub(crate) struct VacuityPrelude<'a> {
    pub params: &'a [assura_ast::Param],
    pub return_ty: &'a [String],
    pub constants: &'a [(String, i64)],
    pub narrowings: &'a [(String, i64)],
}

pub(crate) fn clause_vacuity_reason(
    clause: &Clause,
    requires: &[&SpExpr],
    nat_machine_wrap: bool,
) -> Option<&'static str> {
    if !matches!(clause.kind, ClauseKind::Ensures | ClauseKind::Invariant) {
        return None;
    }
    if requires
        .iter()
        .any(|req| expr_same_obligation(&clause.body, req))
    {
        return Some(RESTATES_REQUIRES);
    }
    if nat_machine_wrap
        && matches!(clause.kind, ClauseKind::Ensures)
        && expr_is_nat_wrap_ceiling(&clause.body)
    {
        return Some(NAT_WRAP_CEILING);
    }
    None
}

/// Stamp vacuity onto verified results that line up with `verifiable`.
///
/// Lengths must match (one result per verifiable clause). A mismatch leaves
/// the results alone rather than attaching a reason to the wrong clause.
pub(crate) fn stamp_vacuity(
    verifiable: &[&Clause],
    requires: &[&SpExpr],
    prelude: VacuityPrelude<'_>,
    results: &mut [VerificationResult],
) {
    if verifiable.len() != results.len() {
        return;
    }
    let nat_machine_wrap =
        crate::policy::prelude_policy::contract_machine_wrap(prelude.params, prelude.return_ty)
            == Some((64, false));
    for (clause, result) in verifiable.iter().zip(results.iter_mut()) {
        let Some(reason) = clause_vacuity_reason(clause, requires, nat_machine_wrap) else {
            continue;
        };
        if let VerificationResult::Verified { vacuous_reason, .. } = result
            && vacuous_reason.is_none()
        {
            *vacuous_reason = Some(reason.to_string());
        }
    }
    // Second query: still valid when the user's requires are not asserted.
    // Type bounds and wrap axioms stay. Only an UNSAT answer is marked, so a
    // timeout is not reported as vacuous.
    for (clause, result) in verifiable.iter().zip(results.iter_mut()) {
        if !matches!(clause.kind, ClauseKind::Ensures) {
            continue;
        }
        let VerificationResult::Verified { vacuous_reason, .. } = result else {
            continue;
        };
        if vacuous_reason.is_some() {
            continue;
        }
        if clause_holds_without_user_requires(clause, prelude) {
            *vacuous_reason = Some(HOLDS_WITHOUT_REQUIRES.to_string());
        }
    }
}

fn clause_holds_without_user_requires(clause: &Clause, prelude: VacuityPrelude<'_>) -> bool {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::verify::clause_holds_without_user_requires(
            &clause.body,
            prelude.params,
            prelude.return_ty,
            prelude.constants,
            prelude.narrowings,
        )
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        let _ = (clause, prelude);
        false
    }
}

fn expr_same_obligation(a: &SpExpr, b: &SpExpr) -> bool {
    same_expr(&a.node, &b.node)
}

fn expr_is_nat_wrap_ceiling(expr: &SpExpr) -> bool {
    match &expr.node {
        Expr::BinOp {
            op: BinOp::Lte,
            lhs,
            rhs,
        } if is_u64_max_literal(rhs) && expr_contains_add_or_mul(lhs) => true,
        Expr::BinOp {
            op: BinOp::Gte,
            lhs,
            rhs,
        } if is_u64_max_literal(lhs) && expr_contains_add_or_mul(rhs) => true,
        _ => false,
    }
}

fn is_u64_max_literal(expr: &SpExpr) -> bool {
    matches!(&expr.node, Expr::Literal(Literal::Int(s)) if s == "18446744073709551615")
}

fn expr_contains_add_or_mul(expr: &SpExpr) -> bool {
    match &expr.node {
        Expr::BinOp {
            op: BinOp::Add | BinOp::Mul,
            ..
        } => true,
        Expr::BinOp { lhs, rhs, .. } => {
            expr_contains_add_or_mul(lhs) || expr_contains_add_or_mul(rhs)
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Old(inner)
        | Expr::Cast { expr: inner, .. }
        | Expr::Ghost(inner) => expr_contains_add_or_mul(inner),
        _ => false,
    }
}

fn same_expr(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (
            Expr::BinOp {
                op: op_a,
                lhs: lhs_a,
                rhs: rhs_a,
            },
            Expr::BinOp {
                op: op_b,
                lhs: lhs_b,
                rhs: rhs_b,
            },
        ) => same_binop(op_a, lhs_a, rhs_a, op_b, lhs_b, rhs_b),
        _ => a == b,
    }
}

fn same_binop(
    op_a: &BinOp,
    lhs_a: &SpExpr,
    rhs_a: &SpExpr,
    op_b: &BinOp,
    lhs_b: &SpExpr,
    rhs_b: &SpExpr,
) -> bool {
    if op_a == op_b {
        let direct = same_expr(&lhs_a.node, &lhs_b.node) && same_expr(&rhs_a.node, &rhs_b.node);
        let swapped = commutative(op_a)
            && same_expr(&lhs_a.node, &rhs_b.node)
            && same_expr(&rhs_a.node, &lhs_b.node);
        return direct || swapped;
    }
    if is_flipped_cmp(op_a, op_b) {
        return same_expr(&lhs_a.node, &rhs_b.node) && same_expr(&rhs_a.node, &lhs_b.node);
    }
    false
}

fn commutative(op: &BinOp) -> bool {
    matches!(
        op,
        BinOp::Eq | BinOp::Neq | BinOp::Add | BinOp::Mul | BinOp::And | BinOp::Or
    )
}

fn is_flipped_cmp(a: &BinOp, b: &BinOp) -> bool {
    matches!(
        (a, b),
        (BinOp::Lte, BinOp::Gte)
            | (BinOp::Gte, BinOp::Lte)
            | (BinOp::Lt, BinOp::Gt)
            | (BinOp::Gt, BinOp::Lt)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::{BinOp, Expr, Literal, Spanned};

    fn ident(name: &str) -> SpExpr {
        Spanned::no_span(Expr::Ident(name.into()))
    }

    fn lit(n: &str) -> SpExpr {
        Spanned::no_span(Expr::Literal(Literal::Int(n.into())))
    }

    fn bin(op: BinOp, lhs: SpExpr, rhs: SpExpr) -> SpExpr {
        Spanned::no_span(Expr::BinOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    }

    fn ensures(body: SpExpr) -> Clause {
        Clause {
            kind: ClauseKind::Ensures,
            body,
            effect_variables: vec![],
        }
    }

    #[test]
    fn flipped_comparison_restates_requires() {
        let req = bin(BinOp::Lte, ident("a"), ident("b"));
        let ens = ensures(bin(BinOp::Gte, ident("b"), ident("a")));
        assert_eq!(
            clause_vacuity_reason(&ens, &[&req], false),
            Some(RESTATES_REQUIRES)
        );
    }

    #[test]
    fn distinct_ensures_is_not_vacuous() {
        let req = bin(BinOp::Lte, ident("a"), ident("max"));
        let ens = ensures(bin(BinOp::Lt, ident("a"), ident("max")));
        assert_eq!(clause_vacuity_reason(&ens, &[&req], false), None);
    }

    #[test]
    fn nat_sum_against_u64_max_is_a_wrap_ceiling() {
        let sum = bin(BinOp::Add, ident("a"), ident("b"));
        let ens = ensures(bin(BinOp::Lte, sum, lit("18446744073709551615")));
        assert_eq!(
            clause_vacuity_reason(&ens, &[], true),
            Some(NAT_WRAP_CEILING)
        );
    }

    #[test]
    fn mixed_arith_sum_against_u64_max_is_not_a_wrap_ceiling() {
        let sum = bin(BinOp::Add, ident("a"), ident("b"));
        let ens = ensures(bin(BinOp::Lte, sum, lit("18446744073709551615")));
        assert_eq!(clause_vacuity_reason(&ens, &[], false), None);
    }
}
