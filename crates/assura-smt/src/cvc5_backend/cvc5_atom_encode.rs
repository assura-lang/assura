//! Shared atom-level encoding: literals, idents, raw single tokens, and apply.

use assura_ast::{Literal, SpExpr};

use crate::cvc5_common::{float_literal_to_smtlib, sanitize_smtlib_name, smtlib_result_name};

#[cfg(feature = "cvc5-verify")]
use crate::cvc5_common::float_to_rational_parts;

/// Render an integer literal as SMT-LIB2 (negatives use `(- n)` form).
pub(crate) fn encode_int_literal_smtlib(n: &str) -> String {
    if let Some(stripped) = n.strip_prefix('-') {
        format!("(- {stripped})")
    } else {
        n.to_string()
    }
}

/// Render a literal as SMT-LIB2.
pub(crate) fn encode_literal_smtlib(lit: &Literal) -> Option<String> {
    match lit {
        Literal::Int(n) => Some(encode_int_literal_smtlib(n)),
        Literal::Bool(b) => Some(b.to_string()),
        Literal::Float(f) => Some(float_literal_to_smtlib(f)),
        Literal::Str(s) => Some(format!("__str_{}", sanitize_smtlib_name(s))),
    }
}

/// Render an identifier as SMT-LIB2 (`result` maps to `__result`).
pub(crate) fn encode_ident_smtlib(name: &str) -> String {
    if name == "result" {
        smtlib_result_name().to_string()
    } else {
        sanitize_smtlib_name(name)
    }
}

/// Vacuous raw expression in SMT-LIB2.
pub(crate) fn encode_raw_empty_smtlib() -> String {
    "true".to_string()
}

/// Fast path for a single raw token in SMT-LIB2 (matches native single-token semantics).
pub(crate) fn encode_raw_single_token_smtlib(token: &str) -> Option<String> {
    if token == "true" || token == "false" {
        return Some(token.to_string());
    }
    if token.parse::<i64>().is_ok() {
        return Some(encode_int_literal_smtlib(token));
    }
    Some(encode_ident_smtlib(token))
}

/// Render lemma apply as a named boolean constant.
pub(crate) fn encode_apply_smtlib(lemma_name: &str) -> String {
    format!("__apply_{lemma_name}")
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_literal_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    lit: &Literal,
    state: &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    match lit {
        Literal::Int(n) => {
            let val: i64 = n.parse().ok()?;
            Some(tm.mk_integer(val))
        }
        Literal::Bool(b) => Some(tm.mk_boolean(*b)),
        Literal::Float(f_str) => {
            let (numer, denom) = float_to_rational_parts(f_str);
            Some(tm.mk_real_from_rational(numer, denom))
        }
        Literal::Str(s) => Some(encode_string_literal_cvc5(tm, s, state)),
    }
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_string_literal_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    s: &str,
    state: &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    if state.use_string_theory {
        let str_val = tm.mk_string(s, false);
        let len = tm.mk_term(cvc5::Kind::StringLength, std::slice::from_ref(&str_val));
        let expected_len = tm.mk_integer(s.len() as i64);
        let len_eq = tm.mk_term(cvc5::Kind::Equal, &[len, expected_len]);
        state.axioms.push(len_eq);
        return str_val;
    }

    let const_name = format!("__str_{s}");
    let str_val = tm.mk_const(tm.integer_sort(), &const_name);
    if !state.string_constants.contains(&const_name) {
        for prev in &state.string_constants {
            let prev_val = tm.mk_const(tm.integer_sort(), prev);
            let eq = tm.mk_term(cvc5::Kind::Equal, &[str_val.clone(), prev_val]);
            let neq = tm.mk_term(cvc5::Kind::Not, &[eq]);
            state.axioms.push(neq);
        }
        state.string_constants.push(const_name);
    }
    let len_name = "__field_len";
    let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
    let len_func = tm.mk_const(len_sort, len_name);
    let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, str_val.clone()]);
    let str_len = tm.mk_integer(s.len() as i64);
    let len_eq = tm.mk_term(cvc5::Kind::Equal, &[len_result, str_len]);
    state.axioms.push(len_eq);
    str_val
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_ident_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    name: &str,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
) -> cvc5::Term<'a> {
    let key = encode_ident_smtlib(name);
    vars.get(&key)
        .cloned()
        .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &key))
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_raw_empty_cvc5<'a>(tm: &'a cvc5::TermManager) -> cvc5::Term<'a> {
    tm.mk_boolean(true)
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_raw_single_token_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    token: &str,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
) -> Option<cvc5::Term<'a>> {
    if token == "true" {
        return Some(tm.mk_boolean(true));
    }
    if token == "false" {
        return Some(tm.mk_boolean(false));
    }
    if let Ok(n) = token.parse::<i64>() {
        return Some(tm.mk_integer(n));
    }
    let key = sanitize_smtlib_name(token);
    Some(
        vars.get(&key)
            .cloned()
            .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &key)),
    )
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_apply_cvc5<'a, F>(
    tm: &'a cvc5::TermManager,
    lemma_name: &str,
    args: &[SpExpr],
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
    for arg in args {
        let _ = encode(arg, vars, state);
    }
    Some(tm.mk_const(tm.boolean_sort(), &format!("__apply_{lemma_name}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_negative_uses_prefix_form() {
        assert_eq!(encode_int_literal_smtlib("-3"), "(- 3)");
    }

    #[test]
    fn ident_result_maps_to_smtlib_name() {
        assert_eq!(encode_ident_smtlib("result"), "__result");
        assert_eq!(encode_ident_smtlib("x"), "x");
    }

    #[test]
    fn raw_single_token_bool() {
        assert_eq!(encode_raw_single_token_smtlib("true"), Some("true".into()));
    }

    #[test]
    fn apply_smtlib_name() {
        assert_eq!(encode_apply_smtlib("lemma_foo"), "__apply_lemma_foo");
    }
}
