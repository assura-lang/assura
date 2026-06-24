//! Shared CVC5 utilities used by shell-out and native backends.
//!
//! Atom/naming lives in [`crate::encode_atom_policy`] (import directly at encode sites).
//! This module keeps CVC5-facing aliases for unmodelable/field-chain walks and lemma
//! apply-ref collection (`tests_cvc5_smtlib` / verify paths).

use assura_ast::SpExpr;

// -------------------------------------------------------------------------
// Field chain + unmodelable walk (shared `crate::unmodelable`; CVC5 name aliases)
// -------------------------------------------------------------------------
// Aliases keep `tests_cvc5_smtlib` / call sites stable; several are test-only in lib builds.

/// CVC5-facing alias: unmodelable walk is solver-neutral in [`crate::unmodelable`].
pub(crate) fn expr_has_unmodelable_features_cvc5(expr: &SpExpr) -> bool {
    crate::unmodelable::expr_has_unmodelable_features(expr)
}

pub(crate) fn collect_unmodelable_reasons_cvc5(expr: &SpExpr) -> Vec<String> {
    crate::unmodelable::collect_unmodelable_reasons(expr)
}

pub(crate) fn is_self_rooted_cvc5(expr: &SpExpr) -> bool {
    crate::unmodelable::is_self_rooted_sp(expr)
}

#[cfg_attr(
    not(test),
    allow(dead_code, reason = "used in tests_cvc5_smtlib field_chain tests")
)]
pub(crate) fn field_chain_depth_cvc5(expr: &SpExpr) -> usize {
    crate::unmodelable::field_chain_depth_sp(expr)
}

pub(crate) fn has_deep_field_chain_cvc5(expr: &SpExpr) -> bool {
    crate::unmodelable::has_deep_field_chain_sp(expr)
}

/// Flatten a field chain like `a.b.c` into `"a__b__c"`.
pub(crate) fn flatten_field_chain_cvc5(expr: &SpExpr) -> String {
    crate::unmodelable::flatten_field_chain_sp(expr)
}

// -------------------------------------------------------------------------
// Lemma apply-ref collection (delegates to lemma_inject_policy)
// -------------------------------------------------------------------------

pub(crate) fn collect_apply_refs_from_expr(expr: &SpExpr) -> Vec<String> {
    crate::lemma_inject_policy::collect_apply_refs_from_expr(expr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode_atom_policy::{
        append_raw_dotted_segment, float_literal_to_smtlib, sanitize_smt_name,
    };
    use assura_ast::{Expr, Spanned};

    fn spb(e: Expr) -> Box<SpExpr> {
        Box::new(Spanned::no_span(e))
    }

    #[test]
    fn sanitize_dots_via_atom_policy() {
        assert_eq!(sanitize_smt_name("a.b"), "a_b");

        let mut name = sanitize_smt_name("state");
        append_raw_dotted_segment(&mut name, "field");
        assert_eq!(name, "state_field");
    }

    #[test]
    fn flatten_deep_chain() {
        let expr = Spanned::no_span(Expr::Field(
            spb(Expr::Field(spb(Expr::Ident("state".into())), "head".into())),
            "extra".into(),
        ));
        assert_eq!(flatten_field_chain_cvc5(&expr), "state__head__extra");
    }

    #[test]
    fn float_rational_encoding() {
        assert_eq!(float_literal_to_smtlib("1.5"), "(/ 1500000 1000000)");
    }
}
