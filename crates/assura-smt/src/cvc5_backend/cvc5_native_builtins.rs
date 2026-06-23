//! Native CVC5 encoding for known builtins and generic UF calls.
//!
//! Mirrors Z3 `encoder/core_impl.rs` `encode_call` axioms (#364) for CVC5 parity.

use crate::cvc5_builtins::{KnownBuiltin, classify_known_builtin, is_bool_returning_uf};
use crate::cvc5_encoder_state::{Cvc5EncoderState, field_len_fn_cvc5, intern_uf_cvc5};
use crate::cvc5_native_binops::alloc_fresh_int_cvc5;

#[cfg(feature = "cvc5-verify")]
fn fresh_int_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    alloc_fresh_int_cvc5(tm, &mut state.fresh_counter)
}

/// Unary integer UF application (e.g. `len(x)`, `__field_len(x)`), interned per session.
#[cfg(feature = "cvc5-verify")]
fn apply_unary_int_uf_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    state: &mut Cvc5EncoderState<'a>,
    name: &str,
    arg: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    let func = intern_uf_cvc5(tm, state, name, 1, false);
    tm.mk_term(cvc5::Kind::ApplyUf, &[func, arg.clone()])
}

/// `len_uf(coll)` via the requested length UF (`len` or `__field_len`).
#[cfg(feature = "cvc5-verify")]
fn collection_len_of_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    state: &mut Cvc5EncoderState<'a>,
    coll: &cvc5::Term<'a>,
    len_uf: &str,
) -> cvc5::Term<'a> {
    apply_unary_int_uf_cvc5(tm, state, len_uf, coll)
}

/// Assert `len_uf(obj) == val` and `val >= 0`.
/// When `len_uf` is `len` or `__field_len`, also links the other alias (Z3 parity).
#[cfg(feature = "cvc5-verify")]
fn assert_collection_len_eq_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    state: &mut Cvc5EncoderState<'a>,
    obj: &cvc5::Term<'a>,
    val: &cvc5::Term<'a>,
    len_uf: &str,
) {
    let got = collection_len_of_cvc5(tm, state, obj, len_uf);
    let zero = tm.mk_integer(0);
    state
        .axioms
        .push(tm.mk_term(cvc5::Kind::Equal, &[got, val.clone()]));
    state
        .axioms
        .push(tm.mk_term(cvc5::Kind::Geq, &[val.clone(), zero]));
    if len_uf == "len" || len_uf == "__field_len" {
        for other in ["len", "__field_len"] {
            if other != len_uf {
                let o = collection_len_of_cvc5(tm, state, obj, other);
                state
                    .axioms
                    .push(tm.mk_term(cvc5::Kind::Equal, &[o, val.clone()]));
            }
        }
    }
}

/// Link `len`/`__field_len` UFs on a collection term to a canonical length value.
/// Used when the collection is a named binding (`xs.length()` vs `len(xs)` / `push(xs, x)`).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn link_ident_length_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    state: &mut Cvc5EncoderState<'a>,
    coll_term: &cvc5::Term<'a>,
    canon_len: &cvc5::Term<'a>,
) {
    for uf in ["len", "__field_len"] {
        let got = collection_len_of_cvc5(tm, state, coll_term, uf);
        state
            .axioms
            .push(tm.mk_term(cvc5::Kind::Equal, &[got, canon_len.clone()]));
    }
}

#[cfg(feature = "cvc5-verify")]
fn field_len_of_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    state: &mut Cvc5EncoderState<'a>,
    value: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    let _ = field_len_fn_cvc5(tm, state); // seed field_len_fn + uf_cache
    apply_unary_int_uf_cvc5(tm, state, "__field_len", value)
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn apply_int_uf_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    state: &mut Cvc5EncoderState<'a>,
    name: &str,
    args: &[cvc5::Term<'a>],
    returns_bool: bool,
) -> cvc5::Term<'a> {
    let func_const = intern_uf_cvc5(tm, state, name, args.len(), returns_bool);
    let mut apply_args = vec![func_const];
    apply_args.extend_from_slice(args);
    tm.mk_term(cvc5::Kind::ApplyUf, &apply_args)
}

/// Encode `is_empty(x) <=> len(x) == 0` bidirectionally (Z3 parity).
#[cfg(feature = "cvc5-verify")]
fn encode_is_empty_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    coll: &cvc5::Term<'a>,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    let b = apply_int_uf_cvc5(tm, state, "is_empty", std::slice::from_ref(coll), true);
    let len_val = collection_len_of_cvc5(tm, state, coll, "len");
    let zero = tm.mk_integer(0);
    let len_is_zero = tm.mk_term(cvc5::Kind::Equal, &[len_val, zero]);
    // Both directions: empty iff length zero.
    state
        .axioms
        .push(tm.mk_term(cvc5::Kind::Implies, &[b.clone(), len_is_zero.clone()]));
    state
        .axioms
        .push(tm.mk_term(cvc5::Kind::Implies, &[len_is_zero, b.clone()]));
    b
}

/// Length-preserving view/copy (`clone`, `reverse`, etc.).
#[cfg(feature = "cvc5-verify")]
fn encode_len_preserving_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    src: &cvc5::Term<'a>,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    let result = fresh_int_cvc5(tm, state);
    let old_len = collection_len_of_cvc5(tm, state, src, "len");
    assert_collection_len_eq_cvc5(tm, state, &result, &old_len, "len");
    result
}

/// `max(0, old_len - 1)` length update (`pop`, `remove`, `tail`).
#[cfg(feature = "cvc5-verify")]
fn encode_len_dec_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    src: &cvc5::Term<'a>,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    let result = fresh_int_cvc5(tm, state);
    let zero = tm.mk_integer(0);
    let one = tm.mk_integer(1);
    let old_len = collection_len_of_cvc5(tm, state, src, "len");
    let dec = tm.mk_term(cvc5::Kind::Sub, &[old_len.clone(), one.clone()]);
    let cond = tm.mk_term(cvc5::Kind::Geq, &[old_len, one]);
    let new_len = tm.mk_term(cvc5::Kind::Ite, &[cond, dec, zero]);
    assert_collection_len_eq_cvc5(tm, state, &result, &new_len, "len");
    result
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
                .push(tm.mk_term(cvc5::Kind::Geq, &[start.clone(), zero]));
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Leq, &[start.clone(), end.clone()]));
            let str_len = collection_len_of_cvc5(tm, state, str_val, "__field_len");
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Leq, &[end.clone(), str_len]));
            let diff = tm.mk_term(cvc5::Kind::Sub, &[end.clone(), start.clone()]);
            assert_collection_len_eq_cvc5(tm, state, &result, &diff, "__field_len");
            Some(result)
        }
        KnownBuiltin::Concat | KnownBuiltin::Append => {
            // concat/append: len(result) == len(a) + len(b) via __field_len + len aliases.
            let l = &args[0];
            let r = &args[1];
            let result = fresh_int_cvc5(tm, state);
            let len_l = collection_len_of_cvc5(tm, state, l, "__field_len");
            let len_r = collection_len_of_cvc5(tm, state, r, "__field_len");
            let zero = tm.mk_integer(0);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[len_l.clone(), zero.clone()]));
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[len_r.clone(), zero]));
            let sum = tm.mk_term(cvc5::Kind::Add, &[len_l.clone(), len_r.clone()]);
            assert_collection_len_eq_cvc5(tm, state, &result, &sum, "__field_len");
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[sum.clone(), len_l]));
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[sum, len_r]));
            Some(result)
        }
        KnownBuiltin::IndexOf => {
            let str_val = &args[0];
            let result = fresh_int_cvc5(tm, state);
            let neg_one = tm.mk_integer(-1);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), neg_one]));
            let str_len = collection_len_of_cvc5(tm, state, str_val, "__field_len");
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
            let str_len = collection_len_of_cvc5(tm, state, str_val, "__field_len");
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Lt, &[idx.clone(), str_len]));
            Some(fresh_int_cvc5(tm, state))
        }
        KnownBuiltin::Replace => {
            let result = fresh_int_cvc5(tm, state);
            let res_len = fresh_int_cvc5(tm, state);
            let zero = tm.mk_integer(0);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[res_len.clone(), zero]));
            assert_collection_len_eq_cvc5(tm, state, &result, &res_len, "__field_len");
            Some(result)
        }
        KnownBuiltin::Split => {
            let result = fresh_int_cvc5(tm, state);
            let res_len = collection_len_of_cvc5(tm, state, &result, "len");
            let one = tm.mk_integer(1);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[res_len, one]));
            Some(result)
        }
        KnownBuiltin::Trim => {
            let str_val = &args[0];
            let result = fresh_int_cvc5(tm, state);
            let str_len = collection_len_of_cvc5(tm, state, str_val, "__field_len");
            let res_len = collection_len_of_cvc5(tm, state, &result, "__field_len");
            let zero = tm.mk_integer(0);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[res_len.clone(), zero]));
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Leq, &[res_len, str_len]));
            Some(result)
        }
        KnownBuiltin::Clone | KnownBuiltin::Reverse => {
            Some(encode_len_preserving_cvc5(tm, &args[0], state))
        }
        KnownBuiltin::Clear => {
            let result = fresh_int_cvc5(tm, state);
            let zero = tm.mk_integer(0);
            assert_collection_len_eq_cvc5(tm, state, &result, &zero, "len");
            Some(result)
        }
        KnownBuiltin::Push => {
            let src = &args[0];
            let result = fresh_int_cvc5(tm, state);
            let one = tm.mk_integer(1);
            let old_len = collection_len_of_cvc5(tm, state, src, "len");
            let new_len = tm.mk_term(cvc5::Kind::Add, &[old_len, one]);
            assert_collection_len_eq_cvc5(tm, state, &result, &new_len, "len");
            Some(result)
        }
        KnownBuiltin::Pop | KnownBuiltin::Tail => Some(encode_len_dec_cvc5(tm, &args[0], state)),
        KnownBuiltin::Insert => {
            let src = &args[0];
            let idx = &args[1];
            let val = &args[2];
            let result = fresh_int_cvc5(tm, state);
            let one = tm.mk_integer(1);
            let zero = tm.mk_integer(0);
            let old_len = collection_len_of_cvc5(tm, state, src, "len");
            let new_len = tm.mk_term(cvc5::Kind::Add, &[old_len.clone(), one]);
            assert_collection_len_eq_cvc5(tm, state, &result, &new_len, "len");
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[idx.clone(), zero]));
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Leq, &[idx.clone(), old_len]));
            let get_func = intern_uf_cvc5(tm, state, "__index", 2, false);
            let at_idx = tm.mk_term(
                cvc5::Kind::ApplyUf,
                &[get_func, result.clone(), idx.clone()],
            );
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[at_idx, val.clone()]));
            Some(result)
        }
        KnownBuiltin::Remove => Some(encode_len_dec_cvc5(tm, &args[0], state)),
        KnownBuiltin::Slice => {
            let src = &args[0];
            let start = &args[1];
            let end = &args[2];
            let result = fresh_int_cvc5(tm, state);
            let zero = tm.mk_integer(0);
            let old_len = collection_len_of_cvc5(tm, state, src, "len");
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[start.clone(), zero]));
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Leq, &[start.clone(), end.clone()]));
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Leq, &[end.clone(), old_len]));
            let diff = tm.mk_term(cvc5::Kind::Sub, &[end.clone(), start.clone()]);
            assert_collection_len_eq_cvc5(tm, state, &result, &diff, "len");
            Some(result)
        }
        KnownBuiltin::Take => {
            let src = &args[0];
            let n = &args[1];
            let result = fresh_int_cvc5(tm, state);
            let zero = tm.mk_integer(0);
            let old_len = collection_len_of_cvc5(tm, state, src, "len");
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[n.clone(), zero]));
            let cond = tm.mk_term(cvc5::Kind::Leq, &[n.clone(), old_len.clone()]);
            let taken = tm.mk_term(cvc5::Kind::Ite, &[cond, n.clone(), old_len]);
            assert_collection_len_eq_cvc5(tm, state, &result, &taken, "len");
            Some(result)
        }
        KnownBuiltin::Drop => {
            let src = &args[0];
            let n = &args[1];
            let result = fresh_int_cvc5(tm, state);
            let zero = tm.mk_integer(0);
            let old_len = collection_len_of_cvc5(tm, state, src, "len");
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[n.clone(), zero.clone()]));
            let rem = tm.mk_term(cvc5::Kind::Sub, &[old_len.clone(), n.clone()]);
            let cond = tm.mk_term(cvc5::Kind::Leq, &[n.clone(), old_len]);
            let dropped = tm.mk_term(cvc5::Kind::Ite, &[cond, rem, zero]);
            assert_collection_len_eq_cvc5(tm, state, &result, &dropped, "len");
            Some(result)
        }
        KnownBuiltin::First => Some(fresh_int_cvc5(tm, state)),
        KnownBuiltin::Set => {
            let arr = &args[0];
            let i = &args[1];
            let v = &args[2];
            let result = fresh_int_cvc5(tm, state);
            let get_func = intern_uf_cvc5(tm, state, "get", 2, false);
            let get_result_i =
                tm.mk_term(cvc5::Kind::ApplyUf, &[get_func, result.clone(), i.clone()]);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[get_result_i, v.clone()]));
            // Preserve length (link via both len and __field_len).
            let old_len = collection_len_of_cvc5(tm, state, arr, "len");
            assert_collection_len_eq_cvc5(tm, state, &result, &old_len, "len");
            Some(result)
        }
        KnownBuiltin::Put => {
            let map = &args[0];
            let k = &args[1];
            let v = &args[2];
            let result = fresh_int_cvc5(tm, state);
            let get_func = intern_uf_cvc5(tm, state, "get", 2, false);
            let get_result_k =
                tm.mk_term(cvc5::Kind::ApplyUf, &[get_func, result.clone(), k.clone()]);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[get_result_k, v.clone()]));
            let size_func = intern_uf_cvc5(tm, state, "size", 1, false);
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
    // is_empty(x) <=> len(x) == 0 (before generic bool-UF fallthrough).
    if f_name == "is_empty" && encoded_args.len() == 1 {
        return Some(encode_is_empty_cvc5(tm, &encoded_args[0], state));
    }
    if is_bool_returning_uf(f_name) {
        return Some(apply_int_uf_cvc5(tm, state, f_name, encoded_args, true));
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
    // Size-like methods: non-negativity + unify len/length/size/__field_len (Z3 parity).
    if matches!(f_name, "len" | "length" | "size" | "count" | "capacity") && encoded_args.len() == 1
    {
        let coll = &encoded_args[0];
        let len_val = collection_len_of_cvc5(tm, state, coll, "len");
        let zero = tm.mk_integer(0);
        state
            .axioms
            .push(tm.mk_term(cvc5::Kind::Geq, &[len_val.clone(), zero]));
        if f_name != "len" {
            let via_method = apply_unary_int_uf_cvc5(tm, state, f_name, coll);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[via_method, len_val.clone()]));
        }
        let via_fl = field_len_of_cvc5(tm, state, coll);
        state
            .axioms
            .push(tm.mk_term(cvc5::Kind::Equal, &[via_fl, len_val.clone()]));
        return Some(len_val);
    }
    if matches!(f_name, "len" | "length" | "size" | "count" | "capacity") {
        let result = apply_int_uf_cvc5(tm, state, f_name, encoded_args, false);
        let zero = tm.mk_integer(0);
        state
            .axioms
            .push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), zero]));
        return Some(result);
    }
    Some(apply_int_uf_cvc5(tm, state, f_name, encoded_args, false))
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn field_len_of_receiver_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    recv_val: &cvc5::Term<'a>,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    // Use unified length (links len + __field_len) so temporaries from push/concat agree
    // with method-form `.length()` access.
    let len = collection_len_of_cvc5(tm, state, recv_val, "len");
    let zero = tm.mk_integer(0);
    state
        .axioms
        .push(tm.mk_term(cvc5::Kind::Geq, &[len.clone(), zero]));
    let via_fl = field_len_of_cvc5(tm, state, recv_val);
    state
        .axioms
        .push(tm.mk_term(cvc5::Kind::Equal, &[via_fl, len.clone()]));
    len
}
