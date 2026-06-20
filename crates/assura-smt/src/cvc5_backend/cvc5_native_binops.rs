//! Native CVC5 encoding for special AST `BinOp` variants (Range/In/Concat).

#[cfg(feature = "cvc5-verify")]
pub(crate) fn alloc_fresh_int_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    fresh_counter: &mut usize,
) -> cvc5::Term<'a> {
    let fresh_name = format!("__fresh_{fresh_counter}");
    *fresh_counter += 1;
    tm.mk_const(tm.integer_sort(), &fresh_name)
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn push_concat_length_axioms_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    axioms: &mut Vec<cvc5::Term<'a>>,
    len_fn: &cvc5::Term<'a>,
    left: &cvc5::Term<'a>,
    right: &cvc5::Term<'a>,
    result: &cvc5::Term<'a>,
) {
    let len_l = tm.mk_term(cvc5::Kind::ApplyUf, &[len_fn.clone(), left.clone()]);
    let len_r = tm.mk_term(cvc5::Kind::ApplyUf, &[len_fn.clone(), right.clone()]);
    let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_fn.clone(), result.clone()]);
    let zero = tm.mk_integer(0);
    axioms.push(tm.mk_term(cvc5::Kind::Geq, &[len_l.clone(), zero.clone()]));
    axioms.push(tm.mk_term(cvc5::Kind::Geq, &[len_r.clone(), zero.clone()]));
    let sum = tm.mk_term(cvc5::Kind::Add, &[len_l, len_r]);
    axioms.push(tm.mk_term(cvc5::Kind::Equal, &[len_result.clone(), sum]));
    axioms.push(tm.mk_term(cvc5::Kind::Geq, &[len_result, zero]));
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_range_binop_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    axioms: &mut Vec<cvc5::Term<'a>>,
    fresh_counter: &mut usize,
    lo: cvc5::Term<'a>,
    hi: cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    let fresh = alloc_fresh_int_cvc5(tm, fresh_counter);
    let ge_lo = tm.mk_term(cvc5::Kind::Geq, &[fresh.clone(), lo]);
    let lt_hi = tm.mk_term(cvc5::Kind::Lt, &[fresh.clone(), hi]);
    axioms.push(tm.mk_term(cvc5::Kind::And, &[ge_lo, lt_hi]));
    fresh
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_contains_binop_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    collection: cvc5::Term<'a>,
    elem: cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    let func_sort = tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.boolean_sort());
    let contains = tm.mk_const(func_sort, "__contains");
    tm.mk_term(cvc5::Kind::ApplyUf, &[contains, collection, elem])
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_concat_binop_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    axioms: &mut Vec<cvc5::Term<'a>>,
    fresh_counter: &mut usize,
    len_fn: &cvc5::Term<'a>,
    left: cvc5::Term<'a>,
    right: cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    let result = alloc_fresh_int_cvc5(tm, fresh_counter);
    push_concat_length_axioms_cvc5(tm, axioms, len_fn, &left, &right, &result);
    result
}
