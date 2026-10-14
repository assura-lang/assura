//! Shared tuple encoding for CVC5 backends.
//!
//! Placeholder / plan **policy** lives in [`crate::encode_tuple_policy`]; this module
//! re-exports shell shapes and keeps native CVC5 term + accessor axioms.

use crate::encode_tuple_policy::{TupleEncodePlan, plan_tuple_encode};

/// Shell-out tuple encoding (placeholder until full tuple axioms land in SMT-LIB path).
pub(crate) fn encode_tuple_smtlib() -> String {
    match plan_tuple_encode(0, true) {
        TupleEncodePlan::ShellPlaceholder => {
            crate::encode_tuple_policy::encode_tuple_smtlib_placeholder()
        }
        TupleEncodePlan::FreshWithAccessors { .. } => {
            // Shell path always requests placeholder; defensive fallback.
            crate::encode_tuple_policy::encode_tuple_smtlib_placeholder()
        }
    }
}

/// Native CVC5: fresh Int constant with per-element accessor UF axioms.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_tuple_cvc5(
    tm: &mut cvc5::TermManager,
    elem_vals: &[cvc5::Term],
    axioms: &mut Vec<cvc5::Term>,
    fresh_counter: &mut usize,
) -> cvc5::Term {
    use crate::encode_tuple_policy::{tuple_accessor_uf_name, tuple_value_fresh_name};
    let arity = elem_vals.len();
    let plan = plan_tuple_encode(arity, false);
    debug_assert!(matches!(
        plan,
        TupleEncodePlan::FreshWithAccessors { arity: a } if a == arity
    ));
    let tuple_name = tuple_value_fresh_name(*fresh_counter);
    *fresh_counter += 1;
    let tuple_val = {
        let __cvc2 = tm.integer_sort();
        tm.mk_const(__cvc2, &tuple_name)
    }
    .expect("cvc5");
    for (i, elem_val) in elem_vals.iter().enumerate() {
        let accessor_name = tuple_accessor_uf_name(arity, i);
        let acc_sort = {
            let __cvc1 = tm.integer_sort();
            let __cvc0 = tm.integer_sort();
            tm.mk_fun_sort(&[__cvc1], __cvc0)
        }
        .expect("cvc5");
        let acc_func = tm.mk_const(acc_sort, &accessor_name).expect("cvc5");
        let accessed = tm
            .mk_term(cvc5::Kind::ApplyUf, &[acc_func, tuple_val.clone()])
            .expect("cvc5");
        axioms.push(
            tm.mk_term(cvc5::Kind::Equal, &[accessed, elem_val.clone()])
                .expect("cvc5"),
        );
    }
    tuple_val
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode_tuple_policy::TUPLE_FRESH_PLACEHOLDER;

    #[test]
    fn tuple_placeholder_constant() {
        assert_eq!(encode_tuple_smtlib(), "__tuple_fresh");
        assert_eq!(TUPLE_FRESH_PLACEHOLDER, "__tuple_fresh");
        assert_eq!(
            plan_tuple_encode(2, false),
            TupleEncodePlan::FreshWithAccessors { arity: 2 }
        );
    }
}
