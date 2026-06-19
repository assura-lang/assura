//! Shared Call and MethodCall encoding for CVC5 shell-out and native backends.

use assura_parser::ast::Expr;

use crate::cvc5_builtins::known_builtin_to_smtlib;
use crate::cvc5_common::sanitize_smtlib_name;

#[cfg(feature = "cvc5-verify")]
use std::collections::HashMap;

#[cfg(feature = "cvc5-verify")]
use crate::cvc5_encoder_state::{Cvc5EncoderState, canonical_length_cvc5};
#[cfg(feature = "cvc5-verify")]
use crate::cvc5_native_builtins::{
    encode_known_builtin_cvc5, encode_uf_call_cvc5, field_len_of_receiver_cvc5,
};

/// Encode `f(args)` as SMT-LIB2 (builtin table or generic UF).
pub(crate) fn encode_call_smtlib<F>(func: &Expr, args: &[Expr], encode: F) -> Option<String>
where
    F: FnMut(&Expr) -> Option<String>,
{
    let f = match func {
        Expr::Ident(name) => sanitize_smtlib_name(name),
        _ => return None,
    };
    if args.is_empty() {
        return Some(f);
    }
    let arg_strs: Option<Vec<String>> = args.iter().map(encode).collect();
    let arg_strs = arg_strs?;
    if let Some(s) = known_builtin_to_smtlib(f.as_str(), &arg_strs) {
        return Some(s);
    }
    Some(format!("({f} {})", arg_strs.join(" ")))
}

/// Encode `receiver.method(args)` as SMT-LIB2 (receiver prepended to arg list).
pub(crate) fn encode_method_call_smtlib<F>(
    receiver: &Expr,
    method: &str,
    args: &[Expr],
    mut encode: F,
) -> Option<String>
where
    F: FnMut(&Expr) -> Option<String>,
{
    let r = encode(receiver)?;
    let arg_strs: Option<Vec<String>> = args.iter().map(encode).collect();
    let arg_strs = arg_strs.unwrap_or_default();
    let mut all_args = vec![r];
    all_args.extend(arg_strs);
    if let Some(s) = known_builtin_to_smtlib(method, &all_args) {
        return Some(s);
    }
    if all_args.len() == 1 {
        Some(format!("({method} {})", all_args[0]))
    } else {
        Some(format!("({method} {})", all_args.join(" ")))
    }
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_length_receiver_cvc5<'a, F>(
    tm: &'a cvc5::TermManager,
    receiver: &Expr,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
    mut encode: F,
) -> Option<cvc5::Term<'a>>
where
    F: FnMut(
        &Expr,
        &mut HashMap<String, cvc5::Term<'a>>,
        &mut Cvc5EncoderState<'a>,
    ) -> Option<cvc5::Term<'a>>,
{
    if let Expr::Ident(name) = receiver {
        return Some(canonical_length_cvc5(tm, name, vars, state));
    }
    let recv_val = encode(receiver, vars, state)?;
    Some(field_len_of_receiver_cvc5(tm, &recv_val, state))
}

/// Encode `f(args)` as a native CVC5 term (builtin table or generic UF).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_call_cvc5<'a, F>(
    tm: &'a cvc5::TermManager,
    func: &Expr,
    args: &[Expr],
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
    mut encode: F,
) -> Option<cvc5::Term<'a>>
where
    F: FnMut(
        &Expr,
        &mut HashMap<String, cvc5::Term<'a>>,
        &mut Cvc5EncoderState<'a>,
    ) -> Option<cvc5::Term<'a>>,
{
    if let Expr::Ident(name) = func {
        let f_name = sanitize_smtlib_name(name);
        if args.is_empty() {
            return vars
                .get(&f_name)
                .cloned()
                .or_else(|| Some(tm.mk_const(tm.integer_sort(), &f_name)));
        }
        let encoded_args: Option<Vec<cvc5::Term>> =
            args.iter().map(|a| encode(a, vars, state)).collect();
        let encoded_args = encoded_args?;
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
    receiver: &Expr,
    method: &str,
    args: &[Expr],
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
    mut encode: F,
) -> Option<cvc5::Term<'a>>
where
    F: FnMut(
        &Expr,
        &mut HashMap<String, cvc5::Term<'a>>,
        &mut Cvc5EncoderState<'a>,
    ) -> Option<cvc5::Term<'a>>,
{
    if matches!(method, "length" | "len") && args.is_empty() {
        return encode_length_receiver_cvc5(tm, receiver, vars, state, encode);
    }

    let recv_val = encode(receiver, vars, state)?;
    let mut all_encoded = vec![recv_val];
    for arg in args {
        all_encoded.push(encode(arg, vars, state)?);
    }
    let f_name = sanitize_smtlib_name(method);
    if let Some(term) = encode_known_builtin_cvc5(tm, f_name.as_str(), &all_encoded, state) {
        return Some(term);
    }
    encode_uf_call_cvc5(tm, &f_name, &all_encoded, state)
}

#[cfg(test)]
mod tests {
    use assura_parser::ast::{Expr, Literal};

    use super::*;

    fn ident(name: &str) -> Expr {
        Expr::Ident(name.to_string())
    }

    fn encode_lit(expr: &Expr) -> Option<String> {
        match expr {
            Expr::Literal(Literal::Int(n)) => Some(n.clone()),
            Expr::Ident(name) => Some(sanitize_smtlib_name(name)),
            _ => None,
        }
    }

    #[test]
    fn call_no_args_returns_name() {
        let func = ident("foo");
        let args: Vec<Expr> = vec![];
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
