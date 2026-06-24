//! Shared CVC5 utilities used by shell-out and native backends.
//!
//! Atom/naming: [`crate::encode_atom_policy`].
//! Unmodelable/field-chain: [`crate::unmodelable`].
//! Lemma apply-ref collection: [`crate::lemma_inject_policy`].
//!
//! Historically this module re-exported thin CVC5-named aliases; encode paths
//! now import those modules directly. Kept as a small home for CVC5-common
//! regression tests that exercise atom + field-chain helpers together.

#[cfg(test)]
mod tests {
    use crate::encode_atom_policy::{
        append_raw_dotted_segment, float_literal_to_smtlib, sanitize_smt_name,
    };
    use crate::unmodelable::flatten_field_chain_sp;
    use assura_ast::{Expr, SpExpr, Spanned};

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
        assert_eq!(flatten_field_chain_sp(&expr), "state__head__extra");
    }

    #[test]
    fn float_rational_encoding() {
        assert_eq!(float_literal_to_smtlib("1.5"), "(/ 1500000 1000000)");
    }
}
