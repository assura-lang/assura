//! Shared index-access encoding for CVC5 shell-out and native backends.

pub(crate) const INDEX_UF_NAME: &str = "__index";

#[cfg(feature = "cvc5-verify")]
pub(crate) const LEN_UF_NAME: &str = "__len";

/// Render `(__index coll idx)` in SMT-LIB2.
pub(crate) fn index_access_smtlib(coll: &str, idx: &str) -> String {
    format!("({INDEX_UF_NAME} {coll} {idx})")
}

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
    let len_func = tm.mk_const(len_sort, LEN_UF_NAME);
    let len_val = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, coll_val.clone()]);
    axioms.push(tm.mk_term(cvc5::Kind::Geq, &[len_val.clone(), zero]));
    axioms.push(tm.mk_term(cvc5::Kind::Lt, &[idx_val.clone(), len_val]));

    let idx_sort = tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
    let idx_func = tm.mk_const(idx_sort, INDEX_UF_NAME);
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
