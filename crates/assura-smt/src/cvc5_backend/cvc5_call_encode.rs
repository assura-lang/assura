//! Shared Call and MethodCall encoding for CVC5 shell-out and native backends.

use assura_ast::{Expr, SpExpr};

use crate::encode_atom_policy::{canonical_length_name, sanitize_smt_name};
use crate::encode_call_policy::{
    EncodeCallKind, classify_encode_call, debug_assert_known_builtin_encode_kind,
};
use crate::encode_method_policy::{classify_known_builtin, known_builtin_to_smtlib};

#[cfg(feature = "cvc5-verify")]
use std::collections::HashMap;

#[cfg(feature = "cvc5-verify")]
use crate::cvc5_encoder_state::{Cvc5EncoderState, canonical_length_cvc5};
#[cfg(feature = "cvc5-verify")]
use crate::cvc5_native_builtins::{
    encode_known_builtin_cvc5, encode_uf_call_cvc5, field_len_of_receiver_cvc5,
    link_ident_length_cvc5,
};

/// Encode `f(args)` as SMT-LIB2 (builtin table or generic UF).
pub(crate) fn encode_call_smtlib<F>(func: &SpExpr, args: &[SpExpr], encode: F) -> Option<String>
where
    F: FnMut(&SpExpr) -> Option<String>,
{
    let f = match &func.node {
        Expr::Ident(name) => sanitize_smt_name(name),
        _ => return None,
    };
    if args.is_empty() {
        return Some(f);
    }
    let arg_strs: Option<Vec<String>> = args.iter().map(encode).collect();
    let arg_strs = arg_strs?;
    if let Some(s) = known_builtin_to_smtlib(f.as_str(), &arg_strs) {
        if let Some(kb) = classify_known_builtin(f.as_str(), arg_strs.len()) {
            debug_assert_known_builtin_encode_kind(f.as_str(), arg_strs.len(), kb);
        }
        return Some(s);
    }
    // Shell/UF fallthrough: align with Z3/CVC5 order table (size field or uninterpreted).
    let kind = classify_encode_call(f.as_str(), arg_strs.len());
    debug_assert!(
        matches!(
            kind,
            EncodeCallKind::SizeFieldUf
                | EncodeCallKind::UninterpretedUf
                | EncodeCallKind::BoolReturningUf
        ),
        "encode_call_smtlib fallthrough unexpected kind {kind:?} for {f}"
    );
    Some(format!("({f} {})", arg_strs.join(" ")))
}

/// Encode `receiver.method(args)` as SMT-LIB2 (receiver prepended to arg list).
pub(crate) fn encode_method_call_smtlib<F>(
    receiver: &SpExpr,
    method: &str,
    args: &[SpExpr],
    mut encode: F,
) -> Option<String>
where
    F: FnMut(&SpExpr) -> Option<String>,
{
    if matches!(method, "length" | "len")
        && args.is_empty()
        && let Expr::Ident(name) = &receiver.node
    {
        return Some(canonical_length_name(name));
    }
    let r = encode(receiver)?;
    let arg_strs: Option<Vec<String>> = args.iter().map(encode).collect();
    let arg_strs = arg_strs.unwrap_or_default();
    let mut all_args = vec![r];
    all_args.extend(arg_strs);
    if let Some(s) = known_builtin_to_smtlib(method, &all_args) {
        if let Some(kb) = classify_known_builtin(method, all_args.len()) {
            debug_assert_known_builtin_encode_kind(method, all_args.len(), kb);
        }
        return Some(s);
    }
    let kind = classify_encode_call(method, all_args.len());
    debug_assert!(
        matches!(
            kind,
            EncodeCallKind::SizeFieldUf
                | EncodeCallKind::UninterpretedUf
                | EncodeCallKind::BoolReturningUf
        ),
        "encode_method_call_smtlib fallthrough unexpected kind {kind:?} for {method}"
    );
    if all_args.len() == 1 {
        Some(format!("({method} {})", all_args[0]))
    } else {
        Some(format!("({method} {})", all_args.join(" ")))
    }
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_length_receiver_cvc5<'a, F>(
    tm: &'a cvc5::TermManager,
    receiver: &SpExpr,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
    mut encode: F,
) -> Option<cvc5::Term<'a>>
where
    F: FnMut(
        &SpExpr,
        &mut HashMap<String, cvc5::Term<'a>>,
        &mut Cvc5EncoderState<'a>,
    ) -> Option<cvc5::Term<'a>>,
{
    if let Expr::Ident(name) = &receiver.node {
        return Some(canonical_length_cvc5(tm, name, vars, state));
    }
    let recv_val = encode(receiver, vars, state)?;
    Some(field_len_of_receiver_cvc5(tm, &recv_val, state))
}

/// If `expr` is a simple ident, link `len`/`__field_len` UFs on its term to the
/// canonical length variable (Z3 `collection_len_of` parity for named bindings).
#[cfg(feature = "cvc5-verify")]
fn maybe_link_ident_length_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    expr: &SpExpr,
    term: &cvc5::Term<'a>,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) {
    if let Expr::Ident(name) = &expr.node {
        let canon = canonical_length_cvc5(tm, name, vars, state);
        link_ident_length_cvc5(tm, state, term, &canon);
    }
}

/// Encode `f(args)` as a native CVC5 term (builtin table or generic UF).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_call_cvc5<'a, F>(
    tm: &'a cvc5::TermManager,
    func: &SpExpr,
    args: &[SpExpr],
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
    mut encode: F,
) -> Option<cvc5::Term<'a>>
where
    F: FnMut(
        &SpExpr,
        &mut HashMap<String, cvc5::Term<'a>>,
        &mut Cvc5EncoderState<'a>,
    ) -> Option<cvc5::Term<'a>>,
{
    if let Expr::Ident(name) = &func.node {
        let f_name = sanitize_smt_name(name);
        if args.is_empty() {
            return vars
                .get(&f_name)
                .cloned()
                .or_else(|| Some(tm.mk_const(tm.integer_sort(), &f_name)));
        }
        let mut encoded_args = Vec::with_capacity(args.len());
        for a in args {
            let t = encode(a, vars, state)?;
            maybe_link_ident_length_cvc5(tm, a, &t, vars, state);
            encoded_args.push(t);
        }
        if let Some(term) = encode_known_builtin_cvc5(tm, f_name.as_str(), &encoded_args, state) {
            return Some(term);
        }
        encode_uf_call_cvc5(tm, &f_name, &encoded_args, state)
    } else {
        None
    }
}

/// Encode `receiver.method(args)` as a native CVC5 term.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_method_call_cvc5<'a, F>(
    tm: &'a cvc5::TermManager,
    receiver: &SpExpr,
    method: &str,
    args: &[SpExpr],
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
    mut encode: F,
) -> Option<cvc5::Term<'a>>
where
    F: FnMut(
        &SpExpr,
        &mut HashMap<String, cvc5::Term<'a>>,
        &mut Cvc5EncoderState<'a>,
    ) -> Option<cvc5::Term<'a>>,
{
    if matches!(method, "length" | "len") && args.is_empty() {
        return encode_length_receiver_cvc5(tm, receiver, vars, state, encode);
    }

    let recv_val = encode(receiver, vars, state)?;
    maybe_link_ident_length_cvc5(tm, receiver, &recv_val, vars, state);
    let mut all_encoded = vec![recv_val];
    for arg in args {
        let t = encode(arg, vars, state)?;
        maybe_link_ident_length_cvc5(tm, arg, &t, vars, state);
        all_encoded.push(t);
    }
    let f_name = sanitize_smt_name(method);
    if let Some(term) = encode_known_builtin_cvc5(tm, f_name.as_str(), &all_encoded, state) {
        return Some(term);
    }
    encode_uf_call_cvc5(tm, &f_name, &all_encoded, state)
}

#[cfg(test)]
mod tests {
    use assura_ast::{Expr, Literal, Spanned};

    use super::*;

    fn sp(e: Expr) -> SpExpr {
        Spanned::no_span(e)
    }

    fn ident(name: &str) -> SpExpr {
        sp(Expr::Ident(name.to_string()))
    }

    fn encode_lit(expr: &SpExpr) -> Option<String> {
        match &expr.node {
            Expr::Literal(Literal::Int(n)) => Some(n.clone()),
            Expr::Ident(name) => Some(sanitize_smt_name(name)),
            _ => None,
        }
    }

    #[test]
    fn call_no_args_returns_name() {
        let func = ident("foo");
        let args: Vec<SpExpr> = vec![];
        assert_eq!(
            encode_call_smtlib(&func, &args, encode_lit),
            Some("foo".into())
        );
    }

    #[test]
    fn call_abs_uses_builtin() {
        let args = vec![ident("x")];
        assert_eq!(
            encode_call_smtlib(&ident("abs"), &args, encode_lit),
            Some("(ite (>= x 0) x (- x))".into())
        );
    }

    #[test]
    fn method_concat_prepends_receiver() {
        let receiver = ident("a");
        let args = vec![ident("b")];
        assert_eq!(
            encode_method_call_smtlib(&receiver, "concat", &args, encode_lit),
            Some("(__concat a b)".into())
        );
    }
}
