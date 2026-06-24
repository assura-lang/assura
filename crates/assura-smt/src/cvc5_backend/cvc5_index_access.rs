//! Shared index-access encoding for CVC5 shell-out and native backends.
//!
//! Index **plans** live in [`crate::encode_index_policy`]; UF names / SMT-LIB2
//! in [`crate::encode_atom_policy`]. This module re-exports stable import paths
//! and keeps native term + bounds axiom construction.

// Re-exported for stable `cvc5_index_access` import paths (atom owns impls).
#[allow(unused_imports, reason = "stable cvc5_index_access re-export surface")]
pub(crate) use crate::encode_atom_policy::{INDEX_UF_NAME, index_access_smtlib};

#[cfg(feature = "cvc5-verify")]
#[cfg_attr(not(test), allow(unused_imports))]
pub(crate) use crate::encode_atom_policy::INDEX_BOUNDS_LEN_UF_NAME as LEN_UF_NAME;

/// Native CVC5: UF `__index` with bounds axioms `0 <= idx`, `len(coll) >= 0`, `idx < len(coll)`.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_index_access_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    coll_val: cvc5::Term<'a>,
    idx_val: cvc5::Term<'a>,
    axioms: &mut Vec<cvc5::Term<'a>>,
) -> cvc5::Term<'a> {
    use crate::encode_index_policy::{
        IndexAccessPlan, index_bounds_len_uf_name, index_uf_name, plan_index_access,
    };

    let plan = plan_index_access(true);
    debug_assert!(matches!(
        plan,
        IndexAccessPlan::UfWithOptionalBounds {
            emit_bounds_axioms: true
        }
    ));

    let zero = tm.mk_integer(0);
    if matches!(
        plan,
        IndexAccessPlan::UfWithOptionalBounds {
            emit_bounds_axioms: true
        }
    ) {
        axioms.push(tm.mk_term(cvc5::Kind::Geq, &[idx_val.clone(), zero.clone()]));

        let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
        let len_func = tm.mk_const(len_sort, index_bounds_len_uf_name());
        let len_val = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, coll_val.clone()]);
        axioms.push(tm.mk_term(cvc5::Kind::Geq, &[len_val.clone(), zero]));
        axioms.push(tm.mk_term(cvc5::Kind::Lt, &[idx_val.clone(), len_val]));
    }

    let idx_sort = tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
    let idx_func = tm.mk_const(idx_sort, index_uf_name());
    tm.mk_term(cvc5::Kind::ApplyUf, &[idx_func, coll_val, idx_val])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode_index_policy::{IndexAccessPlan, plan_index_access};

    #[test]
    fn index_access_smtlib_renders_uf() {
        assert_eq!(index_access_smtlib("buf", "i"), "(__index buf i)");
        assert_eq!(
            plan_index_access(false),
            IndexAccessPlan::UfWithOptionalBounds {
                emit_bounds_axioms: false
            }
        );
    }
}
