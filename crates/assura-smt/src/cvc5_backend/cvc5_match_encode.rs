//! Shared match-expression encoding for CVC5 shell-out and native backends.
//!
//! Arm/tag **policy** lives in [`crate::encode_match_policy`]; this module builds
//! SMT-LIB2 / native `ite` chains via encode callbacks.

#[cfg(feature = "cvc5-verify")]
use assura_ast::Literal;
use assura_ast::{MatchArm, Pattern, SpExpr};

use crate::encode_match_policy::{
    MatchArmKind, classify_match_arm, ctor_tag_eq_smtlib, literal_eq_smtlib,
};
#[cfg(feature = "cvc5-verify")]
use crate::encode_method_policy::pattern_hash_name;

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
        let kind = classify_match_arm(arm);
        match kind {
            MatchArmKind::Wildcard => {
                result = Some(encode(&arm.body)?);
            }
            MatchArmKind::BindIdent => {
                result = Some(encode(&arm.body)?);
            }
            MatchArmKind::CtorTagIdent => {
                let Pattern::Ident(name) = &arm.pattern else {
                    unreachable!("CtorTagIdent requires Ident pattern");
                };
                let body = encode(&arm.body)?;
                let cond = ctor_tag_eq_smtlib(&s, name);
                let default = result.as_ref()?;
                result = Some(format!("(ite {cond} {body} {default})"));
            }
            MatchArmKind::Literal => {
                let Pattern::Literal(lit) = &arm.pattern else {
                    unreachable!("Literal kind requires Literal pattern");
                };
                let body = encode(&arm.body)?;
                let cond = literal_eq_smtlib(&s, lit)?;
                let default = result.as_ref()?;
                result = Some(format!("(ite {cond} {body} {default})"));
            }
            MatchArmKind::Constructor => {
                let Pattern::Constructor { name, fields: _ } = &arm.pattern else {
                    unreachable!("Constructor kind requires Constructor pattern");
                };
                let body = encode(&arm.body)?;
                let default = result.as_ref()?;
                let cond = constructor_test(name, &s);
                result = Some(format!("(ite {cond} {body} {default})"));
            }
            MatchArmKind::Tuple => {
                result = Some(encode(&arm.body)?);
            }
        }
    }
    result
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn bind_pattern_vars_cvc5(
    tm: &mut cvc5::TermManager,
    pattern: &Pattern,
    vars: &mut std::collections::HashMap<String, cvc5::Term>,
) {
    match pattern {
        Pattern::Ident(name) => {
            if !vars.contains_key(name) {
                let v = {
                    let __cvc0 = tm.integer_sort();
                    tm.mk_const(__cvc0, name)
                }
                .expect("cvc5");
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
///
/// Arm kinds come from [`crate::encode_match_policy::classify_match_arm`] (parity
/// with `encode_match_smtlib`).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_match_cvc5<E>(
    tm: &mut cvc5::TermManager,
    scrutinee: &SpExpr,
    arms: &[MatchArm],
    vars: &mut std::collections::HashMap<String, cvc5::Term>,
    state: &mut crate::cvc5_encoder_state::Cvc5EncoderState,
    mut encode: E,
) -> Option<cvc5::Term>
where
    E: FnMut(
        &mut cvc5::TermManager,
        &SpExpr,
        &mut std::collections::HashMap<String, cvc5::Term>,
        &mut crate::cvc5_encoder_state::Cvc5EncoderState,
    ) -> Option<cvc5::Term>,
{
    use crate::encode_match_policy::{MatchArmKind, classify_match_arm};

    if arms.is_empty() {
        return None;
    }
    let s = encode(tm, scrutinee, vars, state)?;
    let mut result: Option<cvc5::Term> = None;
    for arm in arms.iter().rev() {
        let kind = classify_match_arm(arm);
        match kind {
            MatchArmKind::Wildcard => {
                result = Some(encode(tm, &arm.body, vars, state)?);
            }
            MatchArmKind::BindIdent => {
                let mut local_vars = vars.clone();
                bind_pattern_vars_cvc5(tm, &arm.pattern, &mut local_vars);
                result = Some(encode(tm, &arm.body, &mut local_vars, state)?);
            }
            MatchArmKind::CtorTagIdent => {
                let Pattern::Ident(name) = &arm.pattern else {
                    unreachable!("CtorTagIdent requires Ident pattern");
                };
                let mut local_vars = vars.clone();
                bind_pattern_vars_cvc5(tm, &arm.pattern, &mut local_vars);
                let body = encode(tm, &arm.body, &mut local_vars, state)?;
                let tag_val = tm.mk_integer(pattern_hash_name(name));
                let cond = tm
                    .mk_term(cvc5::Kind::Equal, &[s.clone(), tag_val])
                    .expect("cvc5");
                if let Some(default) = result.as_ref() {
                    result = Some(
                        tm.mk_term(cvc5::Kind::Ite, &[cond, body, default.clone()])
                            .expect("cvc5"),
                    );
                } else {
                    result = Some(body);
                }
            }
            MatchArmKind::Literal => {
                let Pattern::Literal(lit) = &arm.pattern else {
                    unreachable!("Literal kind requires Literal pattern");
                };
                let body = encode(tm, &arm.body, vars, state)?;
                let lit_term = match lit {
                    Literal::Int(n) => {
                        let val: i64 = n.parse().ok()?;
                        tm.mk_integer(val)
                    }
                    // Bool params are Int 0/1; do not mix Bool and Int in Equal.
                    Literal::Bool(b) => tm.mk_integer(if *b { 1 } else { 0 }),
                    _ => return None,
                };
                let default = result.as_ref()?.clone();
                let cond = tm
                    .mk_term(cvc5::Kind::Equal, &[s.clone(), lit_term])
                    .expect("cvc5");
                result = Some(
                    tm.mk_term(cvc5::Kind::Ite, &[cond, body, default])
                        .expect("cvc5"),
                );
            }
            MatchArmKind::Constructor => {
                let Pattern::Constructor { name, fields } = &arm.pattern else {
                    unreachable!("Constructor kind requires Constructor pattern");
                };
                let tag_val = tm.mk_integer(pattern_hash_name(name));
                let cond = tm
                    .mk_term(cvc5::Kind::Equal, &[s.clone(), tag_val])
                    .expect("cvc5");
                let mut local_vars = vars.clone();
                for field in fields {
                    bind_pattern_vars_cvc5(tm, field, &mut local_vars);
                }
                let body = encode(tm, &arm.body, &mut local_vars, state)?;
                let default = result.as_ref()?.clone();
                result = Some(
                    tm.mk_term(cvc5::Kind::Ite, &[cond, body, default])
                        .expect("cvc5"),
                );
            }
            MatchArmKind::Tuple => {
                let Pattern::Tuple(pats) = &arm.pattern else {
                    unreachable!("Tuple kind requires Tuple pattern");
                };
                let mut local_vars = vars.clone();
                for pat in pats {
                    bind_pattern_vars_cvc5(tm, pat, &mut local_vars);
                }
                let body = encode(tm, &arm.body, &mut local_vars, state)?;
                result = Some(body);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use crate::encode_match_policy::is_constructor_tag_pattern;

    #[test]
    fn constructor_tag_detects_uppercase() {
        assert!(is_constructor_tag_pattern("Some"));
        assert!(!is_constructor_tag_pattern("x"));
    }
}
