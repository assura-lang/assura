//! Shared CVC5 builtin classification and SMT-LIB rendering.
//!
//! **Policy** lives in [`crate::encode_method_policy`] / [`crate::encode_atom_policy`].
//! New encode paths should import those modules directly. This module keeps a
//! stable `cvc5_builtins::*` re-export surface (tests/shell aliases) plus
//! CVC5-only thin wrappers (`is_bool_field`, `is_size_field`).

// -------------------------------------------------------------------------
// Re-exports: encode_method_policy (encode convergence step 4)
// -------------------------------------------------------------------------
// Stable `cvc5_builtins::*` paths; some symbols are only consumed via
// `cvc5_verify` / tests, so allow unused in default lib builds.

#[allow(
    unused_imports,
    reason = "stable cvc5_builtins re-export surface for shell/native/tests"
)]
pub(crate) use crate::encode_method_policy::{
    BOOL_FIELD_NAMES, BOOL_RETURNING_UFS, KnownBuiltin, MEASURE_EMPTY_CONST_NAME,
    classify_known_builtin, is_bool_field_name, is_bool_returning_uf, known_builtin_to_smtlib,
    pattern_hash_name,
};

/// Field/method names with non-negativity size axioms in native encoding.
///
/// Re-exported from [`crate::encode_atom_policy`] for stable `cvc5_builtins` paths.
#[cfg(feature = "cvc5-verify")]
pub(crate) use crate::encode_atom_policy::SIZE_FIELD_NAMES;

/// CVC5-facing alias for [`is_bool_field_name`] (stable call sites under `cvc5_builtins`).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn is_bool_field(name: &str) -> bool {
    is_bool_field_name(name)
}

/// CVC5-facing alias for [`crate::encode_atom_policy::is_size_field_name`].
#[cfg(feature = "cvc5-verify")]
pub(crate) fn is_size_field(name: &str) -> bool {
    crate::encode_atom_policy::is_size_field_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_and_smtlib_abs_parity() {
        assert_eq!(classify_known_builtin("abs", 1), Some(KnownBuiltin::Abs));
        assert_eq!(
            known_builtin_to_smtlib("abs", &[String::from("x")]),
            Some("(ite (>= x 0) x (- x))".into())
        );
    }

    #[test]
    fn classify_substr_alias() {
        assert_eq!(
            classify_known_builtin("substr", 3),
            Some(KnownBuiltin::Substring)
        );
    }

    #[test]
    fn classify_collection_methods_parity() {
        assert_eq!(classify_known_builtin("push", 2), Some(KnownBuiltin::Push));
        assert_eq!(
            classify_known_builtin("push_back", 2),
            Some(KnownBuiltin::Push)
        );
        assert_eq!(classify_known_builtin("pop", 1), Some(KnownBuiltin::Pop));
        assert_eq!(
            classify_known_builtin("reverse", 1),
            Some(KnownBuiltin::Reverse)
        );
        assert_eq!(
            classify_known_builtin("clear", 1),
            Some(KnownBuiltin::Clear)
        );
        assert_eq!(classify_known_builtin("take", 2), Some(KnownBuiltin::Take));
        assert_eq!(classify_known_builtin("drop", 2), Some(KnownBuiltin::Drop));
        assert_eq!(
            classify_known_builtin("slice", 3),
            Some(KnownBuiltin::Slice)
        );
        assert_eq!(
            classify_known_builtin("insert", 3),
            Some(KnownBuiltin::Insert)
        );
        assert_eq!(
            classify_known_builtin("remove", 2),
            Some(KnownBuiltin::Remove)
        );
        assert_eq!(
            classify_known_builtin("append", 2),
            Some(KnownBuiltin::Append)
        );
        assert_eq!(
            classify_known_builtin("clone", 1),
            Some(KnownBuiltin::Clone)
        );
        assert_eq!(classify_known_builtin("tail", 1), Some(KnownBuiltin::Tail));
        assert_eq!(
            classify_known_builtin("first", 1),
            Some(KnownBuiltin::First)
        );
        assert_eq!(classify_known_builtin("get", 2), Some(KnownBuiltin::Get));
    }

    #[test]
    fn classify_unknown_arity_returns_none() {
        assert_eq!(classify_known_builtin("abs", 2), None);
        assert_eq!(classify_known_builtin("unknown", 1), None);
        assert_eq!(classify_known_builtin("push", 1), None);
        assert_eq!(classify_known_builtin("get", 1), None);
    }

    #[test]
    fn reexports_measure_empty() {
        assert_eq!(MEASURE_EMPTY_CONST_NAME, "__empty");
    }
}
