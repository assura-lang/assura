//! Shared **list literal** encode policy (encode convergence step).
//!
//! Owns solver-neutral placeholders and plans for `Expr::List` / list shell
//! encoding. Element axioms (`__list_get`) and length UFs stay backend-local
//! via [`crate::encode_atom_policy`].
//!
//! Complements [`crate::encode_tuple_policy`] (tuple placeholder/plan pattern).

/// Shell-out / incomplete-path placeholder until full list axioms land in SMT-LIB.
pub(crate) fn list_fresh_placeholder() -> &'static str {
    crate::encode_atom_policy::LIST_FRESH_PLACEHOLDER
}

/// SMT-LIB2 for a list when only a placeholder is emitted (shell path today).
pub(crate) fn encode_list_smtlib_placeholder() -> String {
    list_fresh_placeholder().to_string()
}

/// Plan for encoding a list of given length (solver-neutral).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListEncodePlan {
    /// Shell / incomplete: emit placeholder without element axioms.
    ShellPlaceholder,
    /// Full path: allocate `list_fresh_name(counter)`, assert get/len axioms.
    FreshWithElements { len: usize },
}

/// Classify how backends should encode a list with `len` elements.
pub(crate) fn plan_list_encode(len: usize, shell_placeholder_path: bool) -> ListEncodePlan {
    if shell_placeholder_path {
        ListEncodePlan::ShellPlaceholder
    } else {
        ListEncodePlan::FreshWithElements { len }
    }
}

/// Fresh list constant name for counter `n` (delegates to atom policy).
#[cfg_attr(
    not(any(test, feature = "cvc5-verify")),
    allow(dead_code, reason = "native CVC5; Z3 allocates via fresh_int")
)]
pub(crate) fn list_value_fresh_name(counter: impl std::fmt::Display) -> String {
    crate::encode_atom_policy::list_fresh_name(counter)
}

/// List element accessor UIF name (delegates to atom policy).
pub(crate) fn list_get_uf_name() -> &'static str {
    crate::encode_atom_policy::LIST_GET_UF_NAME
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_and_plans() {
        assert_eq!(encode_list_smtlib_placeholder(), "__list_fresh");
        assert_eq!(list_get_uf_name(), "__list_get");
        assert_eq!(plan_list_encode(3, true), ListEncodePlan::ShellPlaceholder);
        assert_eq!(
            plan_list_encode(2, false),
            ListEncodePlan::FreshWithElements { len: 2 }
        );
        assert_eq!(list_value_fresh_name(4), "__list_4");
    }
}
