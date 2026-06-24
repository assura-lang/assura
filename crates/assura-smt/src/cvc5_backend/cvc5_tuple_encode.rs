//! Shared tuple encoding for CVC5 backends.

pub(crate) const TUPLE_FRESH_PLACEHOLDER: &str = "__tuple_fresh";

/// Shell-out tuple encoding (placeholder until full tuple axioms land in SMT-LIB path).
pub(crate) fn encode_tuple_smtlib() -> String {
    TUPLE_FRESH_PLACEHOLDER.to_string()
}

/// Native CVC5: fresh Int constant with per-element accessor UF axioms.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_tuple_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    elem_vals: &[cvc5::Term<'a>],
    axioms: &mut Vec<cvc5::Term<'a>>,
    fresh_counter: &mut usize,
) -> cvc5::Term<'a> {
    let tuple_name = crate::encode_atom_policy::tuple_fresh_name(*fresh_counter);
    *fresh_counter += 1;
    let tuple_val = tm.mk_const(tm.integer_sort(), &tuple_name);
    let arity = elem_vals.len();
    for (i, elem_val) in elem_vals.iter().enumerate() {
        let accessor_name = crate::encode_atom_policy::tuple_accessor_name(arity, i);
        let acc_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
        let acc_func = tm.mk_const(acc_sort, &accessor_name);
        let accessed = tm.mk_term(cvc5::Kind::ApplyUf, &[acc_func, tuple_val.clone()]);
        axioms.push(tm.mk_term(cvc5::Kind::Equal, &[accessed, elem_val.clone()]));
    }
    tuple_val
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tuple_placeholder_constant() {
        assert_eq!(encode_tuple_smtlib(), "__tuple_fresh");
    }
}
