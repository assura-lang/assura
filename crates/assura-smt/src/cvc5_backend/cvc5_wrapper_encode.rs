//! Transparent wrapper expressions (paren, ghost, cast) for both backends.

use assura_ast::SpExpr;

/// Encode through a wrapper by recursing on the inner expression (SMT-LIB2).
pub(crate) fn encode_wrapper_smtlib<F>(inner: &SpExpr, mut encode: F) -> Option<String>
where
    F: FnMut(&SpExpr) -> Option<String>,
{
    encode(inner)
}

/// Encode through a wrapper by recursing on the inner expression (native CVC5).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_wrapper_cvc5<F>(
    inner: &SpExpr,
    vars: &mut std::collections::HashMap<String, cvc5::Term>,
    state: &mut crate::cvc5_encoder_state::Cvc5EncoderState,
    mut encode: F,
) -> Option<cvc5::Term>
where
    F: FnMut(
        &SpExpr,
        &mut std::collections::HashMap<String, cvc5::Term>,
        &mut crate::cvc5_encoder_state::Cvc5EncoderState,
    ) -> Option<cvc5::Term>,
{
    encode(inner, vars, state)
}
