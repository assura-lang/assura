//! Shared **tuple** encode policy (encode convergence step).
//!
//! Owns solver-neutral names and shell placeholder for `Expr::Tuple`. Element
//! encoding and accessor axioms (`__tuple_{arity}_{i}`) stay in backends via
//! [`crate::encode_atom_policy::tuple_fresh_name`] / [`tuple_accessor_name`].
//!
//! Complements [`crate::encode_atom_policy`] (UF/fresh names) and CVC5
//! `cvc5_tuple_encode` (native term + axioms).

/// Shell-out / incomplete-path placeholder until full tuple axioms land in SMT-LIB.
pub(crate) const TUPLE_FRESH_PLACEHOLDER: &str = "__tuple_fresh";

/// SMT-LIB2 for a tuple when only a placeholder is emitted (shell path today).
pub(crate) fn encode_tuple_smtlib_placeholder() -> String {
    TUPLE_FRESH_PLACEHOLDER.to_string()
}

/// Plan for encoding a tuple of given arity (solver-neutral).
/// Used by CVC5 shell/native and Z3 `Expr::Tuple`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TupleEncodePlan {
    /// Shell / incomplete: emit [`TUPLE_FRESH_PLACEHOLDER`] without element axioms.
    ShellPlaceholder,
    /// Full path: allocate `tuple_fresh_name(counter)`, assert accessor equalities.
    FreshWithAccessors { arity: usize },
}

/// Classify how backends should encode a tuple with `arity` elements.
///
/// `FreshWithAccessors` for Z3/CVC5 native; shell uses placeholder independently.
pub(crate) fn plan_tuple_encode(arity: usize, shell_placeholder_path: bool) -> TupleEncodePlan {
    if shell_placeholder_path {
        TupleEncodePlan::ShellPlaceholder
    } else {
        TupleEncodePlan::FreshWithAccessors { arity }
    }
}

/// Accessor UF name for element `index` of an `arity`-tuple (delegates to atom policy).
///
/// Used by CVC5 native / Z3 full tuple paths.
pub(crate) fn tuple_accessor_uf_name(arity: usize, index: usize) -> String {
    crate::encode_atom_policy::tuple_accessor_name(arity, index)
}

/// Fresh tuple constant name for counter `n` (delegates to atom policy).
#[cfg_attr(
    not(any(test, feature = "cvc5-verify")),
    allow(dead_code, reason = "native/Z3 callers; default build is shell-only")
)]
pub(crate) fn tuple_value_fresh_name(counter: impl std::fmt::Display) -> String {
    crate::encode_atom_policy::tuple_fresh_name(counter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_and_plans() {
        assert_eq!(encode_tuple_smtlib_placeholder(), "__tuple_fresh");
        assert_eq!(
            plan_tuple_encode(3, true),
            TupleEncodePlan::ShellPlaceholder
        );
        assert_eq!(
            plan_tuple_encode(2, false),
            TupleEncodePlan::FreshWithAccessors { arity: 2 }
        );
        assert_eq!(tuple_accessor_uf_name(3, 1), "__tuple_3_1");
        assert_eq!(tuple_value_fresh_name(7), "__tuple_7");
    }
}
