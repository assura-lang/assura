//! Native CVC5 encoding for known builtins and generic UF calls.

use crate::cvc5_builtins::{KnownBuiltin, classify_known_builtin, is_bool_returning_uf};
use crate::cvc5_encoder_state::{Cvc5EncoderState, field_len_fn_cvc5};
use crate::cvc5_native_binops::{alloc_fresh_int_cvc5, encode_concat_binop_cvc5};

#[cfg(feature = "cvc5-verify")]
fn fresh_int_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    alloc_fresh_int_cvc5(tm, &mut state.fresh_counter)
}

#[cfg(feature = "cvc5-verify")]
fn field_len_of_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    state: &mut Cvc5EncoderState<'a>,
    value: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    let len_func = field_len_fn_cvc5(tm, state);
    tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, value.clone()])
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn apply_int_uf_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    name: &str,
    args: &[cvc5::Term<'a>],
    returns_bool: bool,
) -> cvc5::Term<'a> {
    let domain: Vec<cvc5::Sort> = (0..args.len()).map(|_| tm.integer_sort()).collect();
    let codomain = if returns_bool {
        tm.boolean_sort()
    } else {
        tm.integer_sort()
    };
    let func_sort = tm.mk_fun_sort(&domain, codomain);
    let func_const = tm.mk_const(func_sort, name);
    let mut apply_args = vec![func_const];
    apply_args.extend_from_slice(args);
    tm.mk_term(cvc5::Kind::ApplyUf, &apply_args)
}

/// Encode builtins with known semantics (shared by `Call` and `MethodCall`).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_known_builtin_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    op: &str,
    args: &[cvc5::Term<'a>],
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    let kind = classify_known_builtin(op, args.len())?;
    match kind {
        KnownBuiltin::Abs => {
            let x = &args[0];
            let zero = tm.mk_integer(0);
            let neg = tm.mk_term(cvc5::Kind::Neg, std::slice::from_ref(x));
            let cond = tm.mk_term(cvc5::Kind::Geq, &[x.clone(), zero]);
            Some(tm.mk_term(cvc5::Kind::Ite, &[cond, x.clone(), neg]))
        }
        KnownBuiltin::Min => {
            let (a, b) = (&args[0], &args[1]);
            let cond = tm.mk_term(cvc5::Kind::Leq, &[a.clone(), b.clone()]);
            Some(tm.mk_term(cvc5::Kind::Ite, &[cond, a.clone(), b.clone()]))
        }
        KnownBuiltin::Max => {
            let (a, b) = (&args[0], &args[1]);
            let cond = tm.mk_term(cvc5::Kind::Geq, &[a.clone(), b.clone()]);
            Some(tm.mk_term(cvc5::Kind::Ite, &[cond, a.clone(), b.clone()]))
        }
        KnownBuiltin::Substring => {
            let str_val = &args[0];
            let start = &args[1];
            let end = &args[2];
            let result = fresh_int_cvc5(tm, state);
            let zero = tm.mk_integer(0);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[start.clone(), zero.clone()]));
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Leq, &[start.clone(), end.clone()]));
            let len_func = field_len_fn_cvc5(tm, state);
            let str_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), str_val.clone()]);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Leq, &[end.clone(), str_len]));
            let res_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
            let diff = tm.mk_term(cvc5::Kind::Sub, &[end.clone(), start.clone()]);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[res_len.clone(), diff]));
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[res_len, zero]));
            Some(result)
        }
        KnownBuiltin::Concat => {
            let len_func = field_len_fn_cvc5(tm, state);
            Some(encode_concat_binop_cvc5(
                tm,
                &mut state.axioms,
                &mut state.fresh_counter,
                &len_func,
                args[0].clone(),
                args[1].clone(),
            ))
        }
        KnownBuiltin::IndexOf => {
            let str_val = &args[0];
            let result = fresh_int_cvc5(tm, state);
            let neg_one = tm.mk_integer(-1);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), neg_one]));
            let str_len = field_len_of_cvc5(tm, state, str_val);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Lt, &[result.clone(), str_len]));
            Some(result)
        }
        KnownBuiltin::CharAt => {
            let str_val = &args[0];
            let idx = &args[1];
            let zero = tm.mk_integer(0);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[idx.clone(), zero]));
            let str_len = field_len_of_cvc5(tm, state, str_val);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Lt, &[idx.clone(), str_len]));
            Some(fresh_int_cvc5(tm, state))
        }
        KnownBuiltin::Replace => {
            let result = fresh_int_cvc5(tm, state);
            let res_len = field_len_of_cvc5(tm, state, &result);
            let zero = tm.mk_integer(0);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[res_len, zero]));
            Some(result)
        }
        KnownBuiltin::Split => {
            let result = fresh_int_cvc5(tm, state);
            let res_len = field_len_of_cvc5(tm, state, &result);
            let one = tm.mk_integer(1);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[res_len, one]));
            Some(result)
        }
        KnownBuiltin::Trim => {
            let str_val = &args[0];
            let result = fresh_int_cvc5(tm, state);
            let len_func = field_len_fn_cvc5(tm, state);
            let str_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), str_val.clone()]);
            let res_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
            let zero = tm.mk_integer(0);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[res_len.clone(), zero]));
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Leq, &[res_len, str_len]));
            Some(result)
        }
        KnownBuiltin::Set => {
            let arr = &args[0];
            let i = &args[1];
            let v = &args[2];
            let result = fresh_int_cvc5(tm, state);
            let get_sort =
                tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
            let get_func = tm.mk_const(get_sort, "get");
            let get_result_i =
                tm.mk_term(cvc5::Kind::ApplyUf, &[get_func, result.clone(), i.clone()]);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[get_result_i, v.clone()]));
            let len_func = field_len_fn_cvc5(tm, state);
            let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), result.clone()]);
            let len_arr = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, arr.clone()]);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[len_result.clone(), len_arr]));
            let zero = tm.mk_integer(0);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[len_result, zero]));
            Some(result)
        }
        KnownBuiltin::Put => {
            let map = &args[0];
            let k = &args[1];
            let v = &args[2];
            let result = fresh_int_cvc5(tm, state);
            let get_sort =
                tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
            let get_func = tm.mk_const(get_sort, "get");
            let get_result_k =
                tm.mk_term(cvc5::Kind::ApplyUf, &[get_func, result.clone(), k.clone()]);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[get_result_k, v.clone()]));
            let size_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let size_func = tm.mk_const(size_sort, "size");
            let size_result = tm.mk_term(cvc5::Kind::ApplyUf, &[size_func.clone(), result.clone()]);
            let size_map = tm.mk_term(cvc5::Kind::ApplyUf, &[size_func, map.clone()]);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[size_result.clone(), size_map]));
            let zero = tm.mk_integer(0);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[size_result, zero]));
            Some(result)
        }
    }
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_uf_call_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    f_name: &str,
    encoded_args: &[cvc5::Term<'a>],
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    if is_bool_returning_uf(f_name) {
        return Some(apply_int_uf_cvc5(tm, f_name, encoded_args, true));
    }
    if state.use_string_theory
        && matches!(f_name, "len" | "length")
        && encoded_args.len() == 1
        && encoded_args[0].sort().is_string()
    {
        let len = tm.mk_term(cvc5::Kind::StringLength, &[encoded_args[0].clone()]);
        let zero = tm.mk_integer(0);
        state
            .axioms
            .push(tm.mk_term(cvc5::Kind::Geq, &[len.clone(), zero]));
        return Some(len);
    }
    if matches!(f_name, "len" | "length" | "size" | "count" | "capacity") {
        let result = apply_int_uf_cvc5(tm, f_name, encoded_args, false);
        let zero = tm.mk_integer(0);
        state
            .axioms
            .push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), zero]));
        return Some(result);
    }
    Some(apply_int_uf_cvc5(tm, f_name, encoded_args, false))
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn field_len_of_receiver_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    recv_val: &cvc5::Term<'a>,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    let len = field_len_of_cvc5(tm, state, recv_val);
    let zero = tm.mk_integer(0);
    state
        .axioms
        .push(tm.mk_term(cvc5::Kind::Geq, &[len.clone(), zero]));
    len
}
