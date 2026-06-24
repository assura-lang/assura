//! Shared Let and Block encoding for CVC5 shell-out and native backends.
//!
//! SMT-LIB **policy** lives in [`crate::encode_let_policy`]; this module re-exports
//! it and keeps CVC5-native term encode (var map + last-expr block).

#[cfg(any(test, feature = "cvc5-verify"))]
use assura_ast::SpExpr;

#[cfg(any(test, feature = "cvc5-verify"))]
use crate::encode_atom_policy::sanitize_smt_name;

// Stable import paths; shell may use encode_let_policy directly.
#[allow(unused_imports, reason = "re-export surface; cvc5_expr_smtlib prefers encode_let_policy")]
pub(crate) use crate::encode_let_policy::{encode_block_smtlib, encode_let_smtlib};

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_let_cvc5<'a, F>(
    _tm: &'a cvc5::TermManager,
    name: &str,
    value: &SpExpr,
    body: &SpExpr,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
    mut encode: F,
) -> Option<cvc5::Term<'a>>
where
    F: FnMut(
        &SpExpr,
        &mut std::collections::HashMap<String, cvc5::Term<'a>>,
        &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
    ) -> Option<cvc5::Term<'a>>,
{
    let v = encode(value, vars, state)?;
    let mut local_vars = vars.clone();
    local_vars.insert(sanitize_smt_name(name), v);
    encode(body, &mut local_vars, state)
}

/// Encode all block expressions for side effects; return the last.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_block_cvc5<'a, F>(
    tm: &'a cvc5::TermManager,
    body: &[SpExpr],
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
    mut encode: F,
) -> Option<cvc5::Term<'a>>
where
    F: FnMut(
        &SpExpr,
        &mut std::collections::HashMap<String, cvc5::Term<'a>>,
        &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
    ) -> Option<cvc5::Term<'a>>,
{
    use crate::encode_let_policy::{BlockReducePlan, classify_block};
    match classify_block(body) {
        BlockReducePlan::Empty => Some(tm.mk_boolean(true)),
        BlockReducePlan::LastExpr => {
            let mut result = None;
            for e in body {
                result = encode(e, vars, state);
            }
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use assura_ast::{Expr, Literal, Spanned};

    use super::*;

    fn ident(name: &str) -> SpExpr {
        Spanned::no_span(Expr::Ident(name.to_string()))
    }

    fn encode_simple(expr: &SpExpr) -> Option<String> {
        match &expr.node {
            Expr::Ident(name) => Some(sanitize_smt_name(name)),
            Expr::Literal(Literal::Int(n)) => Some(n.clone()),
            _ => None,
        }
    }

    #[test]
    fn let_smtlib_binds_name() {
        let body = ident("x");
        let value = Spanned::no_span(Expr::Literal(Literal::Int("1".into())));
        assert_eq!(
            encode_let_smtlib("x", &value, &body, encode_simple),
            Some("(let ((x 1)) x)".into())
        );
    }

    #[test]
    fn block_smtlib_returns_last() {
        let body = vec![ident("a"), ident("b")];
        assert_eq!(encode_block_smtlib(&body, encode_simple), Some("b".into()));
    }

    #[test]
    fn block_smtlib_empty_is_true() {
        assert_eq!(
            encode_block_smtlib(&[] as &[SpExpr], encode_simple),
            Some("true".into())
        );
    }
}
