//! Shared list encoding for CVC5 backends.

pub(crate) const LIST_FRESH_PLACEHOLDER: &str = "__list_fresh";

#[cfg(feature = "cvc5-verify")]
pub(crate) use crate::encode_atom_policy::LIST_GET_UF_NAME;

/// Shell-out list encoding (placeholder until full list axioms land in SMT-LIB path).
pub(crate) fn encode_list_smtlib() -> String {
    LIST_FRESH_PLACEHOLDER.to_string()
}

/// Native CVC5: fresh Int constant with `__list_get` element axioms and length UF.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_list_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    elem_vals: &[cvc5::Term<'a>],
    axioms: &mut Vec<cvc5::Term<'a>>,
    fresh_counter: &mut usize,
    len_func: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    let list_name = crate::encode_atom_policy::list_fresh_name(*fresh_counter);
    *fresh_counter += 1;
    let list_val = tm.mk_const(tm.integer_sort(), &list_name);

    let get_sort = tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
    let get_func = tm.mk_const(get_sort, crate::encode_atom_policy::LIST_GET_UF_NAME);

    for (i, elem_val) in elem_vals.iter().enumerate() {
        let idx = tm.mk_integer(i as i64);
        let accessed = tm.mk_term(
            cvc5::Kind::ApplyUf,
            &[get_func.clone(), list_val.clone(), idx],
        );
        axioms.push(tm.mk_term(cvc5::Kind::Equal, &[accessed, elem_val.clone()]));
    }

    let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), list_val.clone()]);
    let expected_len = tm.mk_integer(elem_vals.len() as i64);
    axioms.push(tm.mk_term(cvc5::Kind::Equal, &[len_result, expected_len]));

    list_val
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_placeholder_constant() {
        assert_eq!(encode_list_smtlib(), "__list_fresh");
    }
}
