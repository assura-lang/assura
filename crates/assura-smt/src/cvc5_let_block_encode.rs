//! Shared Let and Block encoding for CVC5 shell-out and native backends.

use assura_parser::ast::Expr;

use crate::cvc5_common::sanitize_smtlib_name;

/// Encode `let name = value in body` as SMT-LIB2 `(let ((v val)) body)`.
pub(crate) fn encode_let_smtlib<F>(
    name: &str,
    value: &Expr,
    body: &Expr,
    mut encode: F,
) -> Option<String>
where
    F: FnMut(&Expr) -> Option<String>,
{
    let v = sanitize_smtlib_name(name);
    let val = encode(value)?;
    let b = encode(body)?;
    Some(format!("(let (({v} {val})) {b})"))
}

/// Encode a block as its last expression (SMT-LIB has no block form).
pub(crate) fn encode_block_smtlib<F>(body: &[Expr], mut encode: F) -> Option<String>
where
    F: FnMut(&Expr) -> Option<String>,
{
    if body.is_empty() {
        return Some("true".to_string());
    }
    encode(body.last()?)
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_let_cvc5<'a, F>(
    tm: &'a cvc5::TermManager,
    name: &str,
    value: &Expr,
    body: &Expr,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
    mut encode: F,
) -> Option<cvc5::Term<'a>>
where
    F: FnMut(
        &Expr,
        &mut std::collections::HashMap<String, cvc5::Term<'a>>,
        &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
    ) -> Option<cvc5::Term<'a>>,
{
    let v = encode(value, vars, state)?;
    let mut local_vars = vars.clone();
    local_vars.insert(sanitize_smtlib_name(name), v);
    encode(body, &mut local_vars, state)
}

/// Encode all block expressions for side effects; return the last.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_block_cvc5<'a, F>(
    tm: &'a cvc5::TermManager,
    body: &[Expr],
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
    mut encode: F,
) -> Option<cvc5::Term<'a>>
where
    F: FnMut(
        &Expr,
        &mut std::collections::HashMap<String, cvc5::Term<'a>>,
        &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
    ) -> Option<cvc5::Term<'a>>,
{
    if body.is_empty() {
        return Some(tm.mk_boolean(true));
    }
    let mut result = None;
    for e in body {
        result = encode(e, vars, state);
    }
    result
}

#[cfg(test)]
mod tests {
    use assura_parser::ast::{Expr, Literal};

    use super::*;

    fn ident(name: &str) -> Expr {
        Expr::Ident(name.to_string())
    }

    fn encode_simple(expr: &Expr) -> Option<String> {
        match expr {
            Expr::Ident(name) => Some(sanitize_smtlib_name(name)),
            Expr::Literal(Literal::Int(n)) => Some(n.clone()),
            _ => None,
        }
    }

    #[test]
    fn let_smtlib_binds_name() {
        let body = ident("x");
        let value = Expr::Literal(Literal::Int("1".into()));
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
            encode_block_smtlib(&[] as &[Expr], encode_simple),
            Some("true".into())
        );
    }
}
