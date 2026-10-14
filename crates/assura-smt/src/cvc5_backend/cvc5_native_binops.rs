//! Native CVC5 encoding for special AST `BinOp` variants (Range/In/Concat).

#[cfg(feature = "cvc5-verify")]
pub(crate) fn alloc_fresh_int_cvc5(
    tm: &mut cvc5::TermManager,
    fresh_counter: &mut usize,
) -> cvc5::Term {
    let fresh_name = crate::encode_atom_policy::fresh_temp_name(*fresh_counter);
    *fresh_counter += 1;
    {
        let __cvc3 = tm.integer_sort();
        tm.mk_const(__cvc3, &fresh_name)
    }
    .expect("cvc5")
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn push_concat_length_axioms_cvc5(
    tm: &mut cvc5::TermManager,
    axioms: &mut Vec<cvc5::Term>,
    len_fn: &cvc5::Term,
    left: &cvc5::Term,
    right: &cvc5::Term,
    result: &cvc5::Term,
) {
    let len_l = tm
        .mk_term(cvc5::Kind::ApplyUf, &[len_fn.clone(), left.clone()])
        .expect("cvc5");
    let len_r = tm
        .mk_term(cvc5::Kind::ApplyUf, &[len_fn.clone(), right.clone()])
        .expect("cvc5");
    let len_result = tm
        .mk_term(cvc5::Kind::ApplyUf, &[len_fn.clone(), result.clone()])
        .expect("cvc5");
    let zero = tm.mk_integer(0);
    axioms.push(
        tm.mk_term(cvc5::Kind::Geq, &[len_l.clone(), zero.clone()])
            .expect("cvc5"),
    );
    axioms.push(
        tm.mk_term(cvc5::Kind::Geq, &[len_r.clone(), zero.clone()])
            .expect("cvc5"),
    );
    let sum = tm.mk_term(cvc5::Kind::Add, &[len_l, len_r]).expect("cvc5");
    axioms.push(
        tm.mk_term(cvc5::Kind::Equal, &[len_result.clone(), sum])
            .expect("cvc5"),
    );
    axioms.push(
        tm.mk_term(cvc5::Kind::Geq, &[len_result, zero])
            .expect("cvc5"),
    );
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_range_binop_cvc5(
    tm: &mut cvc5::TermManager,
    axioms: &mut Vec<cvc5::Term>,
    fresh_counter: &mut usize,
    lo: cvc5::Term,
    hi: cvc5::Term,
) -> cvc5::Term {
    let fresh = alloc_fresh_int_cvc5(tm, fresh_counter);
    let ge_lo = tm
        .mk_term(cvc5::Kind::Geq, &[fresh.clone(), lo])
        .expect("cvc5");
    let lt_hi = tm
        .mk_term(cvc5::Kind::Lt, &[fresh.clone(), hi])
        .expect("cvc5");
    axioms.push(tm.mk_term(cvc5::Kind::And, &[ge_lo, lt_hi]).expect("cvc5"));
    fresh
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_contains_binop_cvc5(
    tm: &mut cvc5::TermManager,
    collection: cvc5::Term,
    elem: cvc5::Term,
) -> cvc5::Term {
    let func_sort = {
        let __cvc2 = tm.integer_sort();
        let __cvc1 = tm.integer_sort();
        let __cvc0 = tm.boolean_sort();
        tm.mk_fun_sort(&[__cvc2, __cvc1], __cvc0)
    }
    .expect("cvc5");
    let contains = tm
        .mk_const(func_sort, crate::encode_atom_policy::CONTAINS_UF_NAME)
        .expect("cvc5");
    tm.mk_term(cvc5::Kind::ApplyUf, &[contains, collection, elem])
        .expect("cvc5")
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_concat_binop_cvc5(
    tm: &mut cvc5::TermManager,
    axioms: &mut Vec<cvc5::Term>,
    fresh_counter: &mut usize,
    len_fn: &cvc5::Term,
    left: cvc5::Term,
    right: cvc5::Term,
) -> cvc5::Term {
    let result = alloc_fresh_int_cvc5(tm, fresh_counter);
    push_concat_length_axioms_cvc5(tm, axioms, len_fn, &left, &right, &result);
    result
}
