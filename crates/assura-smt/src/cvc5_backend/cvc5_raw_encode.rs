//! Unified `Expr::Raw` dispatch for CVC5 shell-out and native backends.

use crate::cvc5_atom_encode::{encode_raw_empty_smtlib, encode_raw_single_token_smtlib};
use crate::cvc5_raw_smtlib::encode_raw_tokens_smtlib;

/// Encode `Expr::Raw` tokens as SMT-LIB2 (empty, single-token fast path, or Pratt parser).
pub(crate) fn encode_raw_expr_smtlib(tokens: &[String]) -> Option<String> {
    if tokens.is_empty() {
        return Some(encode_raw_empty_smtlib());
    }
    if tokens.len() == 1 {
        return encode_raw_single_token_smtlib(&tokens[0]);
    }
    encode_raw_tokens_smtlib(tokens)
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_raw_expr_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    tokens: &[String],
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    use crate::cvc5_atom_encode::{encode_raw_empty_cvc5, encode_raw_single_token_cvc5};
    use crate::cvc5_raw_native::encode_raw_tokens_cvc5;

    if tokens.is_empty() {
        return Some(encode_raw_empty_cvc5(tm));
    }
    if tokens.len() == 1 {
        return encode_raw_single_token_cvc5(tm, &tokens[0], vars);
    }
    encode_raw_tokens_cvc5(tm, tokens, vars, state)
}
