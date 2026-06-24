//! Shared list encoding for CVC5 backends.
//!
//! Placeholder / plan **policy** lives in [`crate::encode_list_policy`]; this module
//! keeps native CVC5 term + get/len axioms.

use crate::encode_list_policy::{ListEncodePlan, plan_list_encode};

/// Shell-out list encoding (placeholder until full list axioms land in SMT-LIB path).
pub(crate) fn encode_list_smtlib() -> String {
    match plan_list_encode(0, true) {
        ListEncodePlan::ShellPlaceholder => {
            crate::encode_list_policy::encode_list_smtlib_placeholder()
        }
        ListEncodePlan::FreshWithElements { .. } => {
            crate::encode_list_policy::encode_list_smtlib_placeholder()
        }
    }
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
    use crate::encode_list_policy::{list_get_uf_name, list_value_fresh_name};

    let len = elem_vals.len();
    let plan = plan_list_encode(len, false);
    debug_assert!(matches!(
        plan,
        ListEncodePlan::FreshWithElements { len: l } if l == len
    ));

    let list_name = list_value_fresh_name(*fresh_counter);
    *fresh_counter += 1;
    let list_val = tm.mk_const(tm.integer_sort(), &list_name);

    let get_sort = tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
    let get_func = tm.mk_const(get_sort, list_get_uf_name());

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
    use crate::encode_list_policy::list_fresh_placeholder;

    #[test]
    fn list_placeholder_constant() {
        assert_eq!(encode_list_smtlib(), "__list_fresh");
        assert_eq!(list_fresh_placeholder(), "__list_fresh");
        assert_eq!(
            plan_list_encode(1, false),
            ListEncodePlan::FreshWithElements { len: 1 }
        );
    }
}
