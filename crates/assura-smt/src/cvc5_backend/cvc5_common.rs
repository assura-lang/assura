//! Shared CVC5 utilities used by shell-out and native backends.
//!
//! Atom/naming helpers delegate to [`crate::encode_atom_policy`] (encode convergence groundwork).

use assura_ast::SpExpr;

/// Sanitize an Assura identifier for SMT-LIB2/CVC5 names.
pub(crate) fn sanitize_smtlib_name(name: &str) -> String {
    crate::encode_atom_policy::sanitize_smt_name(name)
}

/// Append a dotted raw-token segment (`tok . segment`) to a sanitized base name.
pub(crate) fn append_raw_dotted_segment(base: &mut String, segment: &str) {
    crate::encode_atom_policy::append_raw_dotted_segment(base, segment);
}

/// Map `result` to the encoder's return-value name.
pub(crate) fn smtlib_result_name() -> &'static str {
    crate::encode_atom_policy::RESULT_VAR_NAME
}

/// Canonical length variable name for shell-out SMT-LIB (`__canonical_len_{name}`).
pub(crate) fn canonical_length_smtlib_name(name: &str) -> String {
    crate::encode_atom_policy::canonical_length_name(name)
}

/// SMT-LIB name for an `old()` snapshot of an identifier.
pub(crate) fn old_ident_smtlib_name(name: &str) -> String {
    crate::encode_atom_policy::old_ident_name(name)
}

/// Render a float literal as SMT-LIB rational `(/ numer denom)`.
pub(crate) fn float_literal_to_smtlib(f: &str) -> String {
    crate::encode_atom_policy::float_literal_to_smtlib(f)
}

// -------------------------------------------------------------------------
// Deep field-chain flattening (#250)
// -------------------------------------------------------------------------

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
// Counterexample model filtering
// -------------------------------------------------------------------------

pub(crate) fn is_internal_cvc5_var(name: &str) -> bool {
    crate::encode_atom_policy::is_internal_encoder_var(name)
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
    use assura_ast::{Expr, Spanned};

    fn spb(e: Expr) -> Box<SpExpr> {
        Box::new(Spanned::no_span(e))
    }

    #[test]
    fn sanitize_dots() {
        assert_eq!(sanitize_smtlib_name("a.b"), "a_b");

        let mut name = sanitize_smtlib_name("state");
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
