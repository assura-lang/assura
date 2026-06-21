//! Shared match-expression encoding for CVC5 shell-out and native backends.

use assura_parser::ast::{Literal, MatchArm, Pattern, SpExpr};

use crate::cvc5_builtins::pattern_hash_name;
use crate::cvc5_common::float_literal_to_smtlib;

/// Uppercase-initial identifier patterns are enum constructor tags (hash-matched).
pub(crate) fn is_constructor_tag_pattern(name: &str) -> bool {
    name.starts_with(|c: char| c.is_uppercase())
}

/// Encode match arms as nested `ite` chains in SMT-LIB2 (arms processed right-to-left).
pub(crate) fn encode_match_smtlib<F, G>(
    scrutinee: &SpExpr,
    arms: &[MatchArm],
    mut encode: F,
    constructor_test: G,
) -> Option<String>
where
    F: FnMut(&SpExpr) -> Option<String>,
    G: Fn(&str, &str) -> String,
{
    if arms.is_empty() {
        return None;
    }
    let s = encode(scrutinee)?;
    let mut result = None;
    for arm in arms.iter().rev() {
        match &arm.pattern {
            Pattern::Wildcard => {
                result = Some(encode(&arm.body)?);
            }
            Pattern::Ident(name) => {
                let body = encode(&arm.body)?;
                if is_constructor_tag_pattern(name) {
                    let tag = pattern_hash_name(name);
                    let default = result.as_ref()?;
                    result = Some(format!("(ite (= {s} {tag}) {body} {default})"));
                } else {
                    result = Some(body);
                }
            }
            Pattern::Literal(lit) => {
                let body = encode(&arm.body)?;
                let lit_smt = match lit {
                    Literal::Int(n) => n.clone(),
                    Literal::Float(f) => float_literal_to_smtlib(f),
                    Literal::Bool(b) => b.to_string(),
                    Literal::Str(_) => return None,
                };
                let default = result.as_ref()?;
                result = Some(format!("(ite (= {s} {lit_smt}) {body} {default})"));
            }
            Pattern::Constructor { name, fields: _ } => {
                let body = encode(&arm.body)?;
                let default = result.as_ref()?;
                let cond = constructor_test(name, &s);
                result = Some(format!("(ite {cond} {body} {default})"));
            }
            Pattern::Tuple(_) => {
                result = Some(encode(&arm.body)?);
            }
        }
    }
    result
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn bind_pattern_vars_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    pattern: &Pattern,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
) {
    match pattern {
        Pattern::Ident(name) => {
            if !vars.contains_key(name) {
                let v = tm.mk_const(tm.integer_sort(), name);
                vars.insert(name.clone(), v);
            }
        }
        Pattern::Constructor { fields, .. } => {
            for field in fields {
                bind_pattern_vars_cvc5(tm, field, vars);
            }
        }
        Pattern::Tuple(pats) => {
            for pat in pats {
                bind_pattern_vars_cvc5(tm, pat, vars);
            }
        }
        Pattern::Wildcard | Pattern::Literal(_) => {}
    }
}

/// Encode match arms as nested native `ite` terms (arms processed right-to-left).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_match_cvc5<'a, E>(
    tm: &'a cvc5::TermManager,
    scrutinee: &SpExpr,
    arms: &[MatchArm],
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
    mut encode: E,
) -> Option<cvc5::Term<'a>>
where
    E: FnMut(
        &SpExpr,
        &mut std::collections::HashMap<String, cvc5::Term<'a>>,
        &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
    ) -> Option<cvc5::Term<'a>>,
{
    if arms.is_empty() {
        return None;
    }
    let s = encode(scrutinee, vars, state)?;
    let mut result: Option<cvc5::Term<'_>> = None;
    for arm in arms.iter().rev() {
        match &arm.pattern {
            Pattern::Wildcard => {
                result = Some(encode(&arm.body, vars, state)?);
            }
            Pattern::Ident(name) => {
                let mut local_vars = vars.clone();
                bind_pattern_vars_cvc5(tm, &arm.pattern, &mut local_vars);
                let body = encode(&arm.body, &mut local_vars, state)?;
                if is_constructor_tag_pattern(name) {
                    let tag_val = tm.mk_integer(pattern_hash_name(name));
                    let cond = tm.mk_term(cvc5::Kind::Equal, &[s.clone(), tag_val]);
                    if let Some(default) = result.as_ref() {
                        result = Some(tm.mk_term(cvc5::Kind::Ite, &[cond, body, default.clone()]));
                    } else {
                        result = Some(body);
                    }
                } else {
                    result = Some(body);
                }
            }
            Pattern::Literal(lit) => {
                let body = encode(&arm.body, vars, state)?;
                let lit_term = match lit {
                    Literal::Int(n) => {
                        let val: i64 = n.parse().ok()?;
                        tm.mk_integer(val)
                    }
                    Literal::Bool(b) => tm.mk_boolean(*b),
                    _ => return None,
                };
                let default = result.as_ref()?.clone();
                let cond = tm.mk_term(cvc5::Kind::Equal, &[s.clone(), lit_term]);
                result = Some(tm.mk_term(cvc5::Kind::Ite, &[cond, body, default]));
            }
            Pattern::Constructor { name, fields } => {
                let tag_val = tm.mk_integer(pattern_hash_name(name));
                let cond = tm.mk_term(cvc5::Kind::Equal, &[s.clone(), tag_val]);
                let mut local_vars = vars.clone();
                for field in fields {
                    bind_pattern_vars_cvc5(tm, field, &mut local_vars);
                }
                let body = encode(&arm.body, &mut local_vars, state)?;
                let default = result.as_ref()?.clone();
                result = Some(tm.mk_term(cvc5::Kind::Ite, &[cond, body, default]));
            }
            Pattern::Tuple(pats) => {
                let mut local_vars = vars.clone();
                for pat in pats {
                    bind_pattern_vars_cvc5(tm, pat, &mut local_vars);
                }
                let body = encode(&arm.body, &mut local_vars, state)?;
                result = Some(body);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor_tag_detects_uppercase() {
        assert!(is_constructor_tag_pattern("Some"));
        assert!(!is_constructor_tag_pattern("x"));
    }
}
