//! Transparent wrapper expressions (paren, ghost, cast) for both backends.

use assura_parser::ast::Expr;

/// Encode through a wrapper by recursing on the inner expression (SMT-LIB2).
pub(crate) fn encode_wrapper_smtlib<F>(inner: &Expr, mut encode: F) -> Option<String>
where
    F: FnMut(&Expr) -> Option<String>,
{
    encode(inner)
}

/// Encode through a wrapper by recursing on the inner expression (native CVC5).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_wrapper_cvc5<'a, F>(
    inner: &Expr,
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
    encode(inner, vars, state)
}
