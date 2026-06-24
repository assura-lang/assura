//! Shared CVC5 builtin classification and SMT-LIB rendering.
//!
//! Native encoding (`encode_known_builtin_cvc5`) and shell-out (`expr_to_smtlib`)
//! share the same builtin name/arity table defined here.

/// FNV-1a hash for constructor tag matching (same algorithm as Z3 backend).
pub(crate) fn pattern_hash_name(name: &str) -> i64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash as i64
}

/// UFs that return Bool in the native encoder (integer-encoding mode).
#[cfg(feature = "cvc5-verify")]
pub(crate) const BOOL_RETURNING_UFS: &[&str] = &[
    "contains",
    "is_empty",
    "is_some",
    "is_none",
    "is_ok",
    "is_err",
    "any",
    "all",
    "contains_key",
    "starts_with",
    "ends_with",
    "is_subset",
    "is_superset",
];

#[cfg(feature = "cvc5-verify")]
pub(crate) fn is_bool_returning_uf(name: &str) -> bool {
    BOOL_RETURNING_UFS.contains(&name)
}

/// Field/method names treated as Bool-valued in native encoding.
#[cfg(feature = "cvc5-verify")]
pub(crate) const BOOL_FIELD_NAMES: &[&str] = &["is_empty", "is_some", "is_none", "is_ok", "is_err"];

/// Field/method names with non-negativity size axioms in native encoding.
///
/// Re-exported from [`crate::encode_atom_policy`] for stable `cvc5_builtins` paths.
#[cfg(feature = "cvc5-verify")]
pub(crate) use crate::encode_atom_policy::SIZE_FIELD_NAMES;

#[cfg(feature = "cvc5-verify")]
pub(crate) fn is_bool_field(name: &str) -> bool {
    BOOL_FIELD_NAMES.contains(&name)
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn is_size_field(name: &str) -> bool {
    crate::encode_atom_policy::is_size_field_name(name)
}

/// Builtin operations shared between native and shell-out backends.
/// Mirrors Z3 `encode_call` semantics (#364) for CVC5 parity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KnownBuiltin {
    Abs,
    Min,
    Max,
    Substring,
    Concat,
    Append,
    IndexOf,
    CharAt,
    Replace,
    Split,
    Trim,
    Clone,
    Reverse,
    Clear,
    Push,
    Pop,
    Insert,
    Remove,
    Slice,
    Take,
    Drop,
    Tail,
    First,
    /// Array/map element access (`get(coll, key)`); not a bool predicate.
    Get,
    Set,
    Put,
}

/// Classify a call by sanitized name and total arity (receiver + args for methods).
pub(crate) fn classify_known_builtin(op: &str, arity: usize) -> Option<KnownBuiltin> {
    match (op, arity) {
        ("abs", 1) => Some(KnownBuiltin::Abs),
        ("min", 2) => Some(KnownBuiltin::Min),
        ("max", 2) => Some(KnownBuiltin::Max),
        ("substring" | "substr", 3) => Some(KnownBuiltin::Substring),
        ("concat", 2) => Some(KnownBuiltin::Concat),
        ("append", 2) => Some(KnownBuiltin::Append),
        ("index_of" | "find" | "indexOf", 2) => Some(KnownBuiltin::IndexOf),
        ("char_at" | "charAt" | "code_unit_at", 2) => Some(KnownBuiltin::CharAt),
        ("replace", 3) => Some(KnownBuiltin::Replace),
        ("split", 2) => Some(KnownBuiltin::Split),
        ("trim" | "to_lowercase" | "to_uppercase" | "to_lower" | "to_upper", 1) => {
            Some(KnownBuiltin::Trim)
        }
        ("clone" | "to_string" | "to_owned" | "as_str", 1) => Some(KnownBuiltin::Clone),
        ("reverse", 1) => Some(KnownBuiltin::Reverse),
        ("clear", 1) => Some(KnownBuiltin::Clear),
        ("push" | "push_back" | "push_front", 2) => Some(KnownBuiltin::Push),
        ("pop" | "pop_back" | "pop_front", 1) => Some(KnownBuiltin::Pop),
        ("insert", 3) => Some(KnownBuiltin::Insert),
        ("remove" | "remove_at", 2) => Some(KnownBuiltin::Remove),
        ("slice", 3) => Some(KnownBuiltin::Slice),
        ("take", 2) => Some(KnownBuiltin::Take),
        ("drop", 2) => Some(KnownBuiltin::Drop),
        ("tail" | "rest", 1) => Some(KnownBuiltin::Tail),
        ("first" | "last" | "head" | "front" | "back", 1) => Some(KnownBuiltin::First),
        ("get", 2) => Some(KnownBuiltin::Get),
        ("set", 3) => Some(KnownBuiltin::Set),
        ("put", 3) => Some(KnownBuiltin::Put),
        _ => None,
    }
}

/// Render a known builtin as SMT-LIB2 prefix notation.
pub(crate) fn known_builtin_to_smtlib(op: &str, args: &[String]) -> Option<String> {
    let kind = classify_known_builtin(op, args.len())?;
    match kind {
        KnownBuiltin::Abs => {
            let x = &args[0];
            Some(format!("(ite (>= {x} 0) {x} (- {x}))"))
        }
        KnownBuiltin::Min => {
            let (a, b) = (&args[0], &args[1]);
            Some(format!("(ite (<= {a} {b}) {a} {b})"))
        }
        KnownBuiltin::Max => {
            let (a, b) = (&args[0], &args[1]);
            Some(format!("(ite (>= {a} {b}) {a} {b})"))
        }
        KnownBuiltin::Substring => Some(format!("(substring {} {} {})", args[0], args[1], args[2])),
        KnownBuiltin::Concat | KnownBuiltin::Append => {
            Some(format!("(__concat {} {})", args[0], args[1]))
        }
        KnownBuiltin::IndexOf => Some(format!("(index_of {} {})", args[0], args[1])),
        KnownBuiltin::CharAt => Some(format!("(char_at {} {})", args[0], args[1])),
        KnownBuiltin::Replace => Some(format!("(replace {} {} {})", args[0], args[1], args[2])),
        KnownBuiltin::Split => Some(format!("(split {} {})", args[0], args[1])),
        KnownBuiltin::Trim
        | KnownBuiltin::Clone
        | KnownBuiltin::Reverse
        | KnownBuiltin::Clear
        | KnownBuiltin::Pop
        | KnownBuiltin::Tail
        | KnownBuiltin::First => Some(format!("({op} {})", args[0])),
        KnownBuiltin::Push | KnownBuiltin::Remove | KnownBuiltin::Take | KnownBuiltin::Drop => {
            Some(format!("({op} {} {})", args[0], args[1]))
        }
        KnownBuiltin::Get => Some(format!("(get {} {})", args[0], args[1])),
        KnownBuiltin::Insert | KnownBuiltin::Slice => {
            Some(format!("({op} {} {} {})", args[0], args[1], args[2]))
        }
        KnownBuiltin::Set => Some(format!("(set {} {} {})", args[0], args[1], args[2])),
        KnownBuiltin::Put => Some(format!("(put {} {} {})", args[0], args[1], args[2])),
    }
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

    #[cfg(feature = "cvc5-verify")]
    #[test]
    fn bool_uf_membership() {
        assert!(is_bool_returning_uf("contains"));
        assert!(!is_bool_returning_uf("concat"));
    }
}
