//! Shared index-access encoding for CVC5 shell-out and native backends.
//!
//! UF names and SMT-LIB text live in [`crate::encode_atom_policy`]; this module
//! re-exports them for stable `cvc5_index_access` import paths.

// Re-exported for stable `cvc5_index_access` import paths (policy owns impls).
// INDEX_UF_NAME is consumed via direct encode_atom_policy paths in production; keep re-export.
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
    let zero = tm.mk_integer(0);
    axioms.push(tm.mk_term(cvc5::Kind::Geq, &[idx_val.clone(), zero.clone()]));

    let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
    let len_func = tm.mk_const(
        len_sort,
        crate::encode_atom_policy::INDEX_BOUNDS_LEN_UF_NAME,
    );
    let len_val = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, coll_val.clone()]);
    axioms.push(tm.mk_term(cvc5::Kind::Geq, &[len_val.clone(), zero]));
    axioms.push(tm.mk_term(cvc5::Kind::Lt, &[idx_val.clone(), len_val]));

    let idx_sort = tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
    let idx_func = tm.mk_const(idx_sort, crate::encode_atom_policy::INDEX_UF_NAME);
    tm.mk_term(cvc5::Kind::ApplyUf, &[idx_func, coll_val, idx_val])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_access_smtlib_renders_uf() {
        assert_eq!(index_access_smtlib("buf", "i"), "(__index buf i)");
    }
}
