//! Clause-level vacuity that does not need a second solver query.
//!
//! An `ensures` or `invariant` that restates a `requires` (including a flipped
//! comparison) proves nothing. A `Nat` sum or product compared to `u64::MAX`
//! is true for every input because `+` and `*` wrap. Both are stamped onto
//! [`crate::VerificationResult::Verified`] after each backend's clause loop
//! so Z3 and CVC5 stay aligned.

use assura_ast::{Clause, ClauseKind, SpExpr};

use crate::VerificationResult;

pub(crate) const RESTATES_REQUIRES: &str = "restates a requires";
pub(crate) const NAT_WRAP_CEILING: &str = "true for every Nat because + and * wrap at 2^64";

pub(crate) fn clause_vacuity_reason(clause: &Clause, requires: &[&SpExpr]) -> Option<&'static str> {
    if !matches!(clause.kind, ClauseKind::Ensures | ClauseKind::Invariant) {
        return None;
    }
    if requires
        .iter()
        .any(|req| assura_ast::expr_same_obligation(&clause.body, req))
    {
        return Some(RESTATES_REQUIRES);
    }
    if matches!(clause.kind, ClauseKind::Ensures)
        && assura_ast::expr_is_nat_wrap_ceiling(&clause.body)
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
    results: &mut [VerificationResult],
) {
    if verifiable.len() != results.len() {
        return;
    }
    for (clause, result) in verifiable.iter().zip(results.iter_mut()) {
        let Some(reason) = clause_vacuity_reason(clause, requires) else {
            continue;
        };
        if let VerificationResult::Verified { vacuous_reason, .. } = result
            && vacuous_reason.is_none()
        {
            *vacuous_reason = Some(reason.to_string());
        }
    }
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
            clause_vacuity_reason(&ens, &[&req]),
            Some(RESTATES_REQUIRES)
        );
    }

    #[test]
    fn distinct_ensures_is_not_vacuous() {
        let req = bin(BinOp::Lte, ident("a"), ident("max"));
        let ens = ensures(bin(BinOp::Lt, ident("a"), ident("max")));
        assert_eq!(clause_vacuity_reason(&ens, &[&req]), None);
    }

    #[test]
    fn nat_sum_against_u64_max_is_a_wrap_ceiling() {
        let sum = bin(BinOp::Add, ident("a"), ident("b"));
        let ens = ensures(bin(BinOp::Lte, sum, lit("18446744073709551615")));
        assert_eq!(clause_vacuity_reason(&ens, &[]), Some(NAT_WRAP_CEILING));
    }
}
