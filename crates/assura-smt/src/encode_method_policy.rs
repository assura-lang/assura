//! Shared **method / builtin dispatch** policy (encode convergence step 4).
//!
//! Owns constructor-tag hashing, bool-returning UF/field tables, known
//! builtin classification + SMT-LIB rendering, and `is_*_builtin` guards used by
//! Z3 `encode_call` / havoc IR, CVC5 native/shell, and model/field paths.
//! Complements [`crate::encode_atom_policy`] (identifier/UF **names**) and
//! [`crate::encode_raw_ops_policy`] (raw operators).
//!
//! Prefer importing this module directly (not via `cvc5_builtins` re-exports).
//! Still **not** full `Expr` → solver term encode: Z3 `Encoder` and CVC5 term
//! builders remain backend-local; only dispatch tables and SMT-LIB method text
//! live here.

use crate::encode_atom_policy::CONCAT_UF_NAME;

/// FNV-1a hash for constructor/typestate tag matching (Z3 and CVC5 agree).
pub(crate) fn pattern_hash_name(name: &str) -> i64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash as i64
}

/// UFs that return Bool in integer-encoding mode (native CVC5 / Z3 method dispatch).
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

pub(crate) fn is_bool_returning_uf(name: &str) -> bool {
    BOOL_RETURNING_UFS.contains(&name)
}

/// Field/method names treated as Bool-valued in native encoding / Z3 field access.
pub(crate) const BOOL_FIELD_NAMES: &[&str] = &["is_empty", "is_some", "is_none", "is_ok", "is_err"];

pub(crate) fn is_bool_field_name(name: &str) -> bool {
    BOOL_FIELD_NAMES.contains(&name)
}

/// Termination measure "empty collection" distinguished constant.
pub(crate) const MEASURE_EMPTY_CONST_NAME: &str = "__empty";

/// Whether `op` at `arity` is a registered [`KnownBuiltin`] (Z3/CVC5 call dispatch guard).
///
/// Referenced from tests and available for backend entry-point guards.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn is_known_builtin(op: &str, arity: usize) -> bool {
    classify_known_builtin(op, arity).is_some()
}

/// Whether `op` is a min/max binary builtin (Z3 encodes with `ite`, not a free UF).
pub(crate) fn is_min_max_builtin(op: &str, arity: usize) -> bool {
    matches!(
        classify_known_builtin(op, arity),
        Some(KnownBuiltin::Min | KnownBuiltin::Max)
    )
}

/// Whether `op` is the unary `abs` builtin.
pub(crate) fn is_abs_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::Abs))
}

/// Whether `op` is a get/set/put collection accessor at the given arity.
///
/// Prefer [`is_get_builtin`] / [`is_set_builtin`] / [`is_put_builtin`] at encode sites.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn is_collection_access_builtin(op: &str, arity: usize) -> bool {
    matches!(
        classify_known_builtin(op, arity),
        Some(KnownBuiltin::Get | KnownBuiltin::Set | KnownBuiltin::Put)
    )
}

/// Whether `op` is get at arity 2.
pub(crate) fn is_get_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::Get))
}

/// Whether `op` is set at arity 3.
pub(crate) fn is_set_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::Set))
}

/// Whether `op` is put at arity 3.
pub(crate) fn is_put_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::Put))
}

/// Whether `op` is min at arity 2 (not max).
pub(crate) fn is_min_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::Min))
}

/// Whether `op` is max at arity 2 (not min).
pub(crate) fn is_max_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::Max))
}

/// Whether `op` is substring/substr at arity 3 (Z3 length-axiom path).
pub(crate) fn is_substring_builtin(op: &str, arity: usize) -> bool {
    matches!(
        classify_known_builtin(op, arity),
        Some(KnownBuiltin::Substring)
    )
}

/// Whether `op` is concat/append at arity 2 (Z3 length-sum axiom path).
pub(crate) fn is_concat_append_builtin(op: &str, arity: usize) -> bool {
    matches!(
        classify_known_builtin(op, arity),
        Some(KnownBuiltin::Concat | KnownBuiltin::Append)
    )
}

/// Whether `op` is index_of/find/indexOf at arity 2.
pub(crate) fn is_index_of_builtin(op: &str, arity: usize) -> bool {
    matches!(
        classify_known_builtin(op, arity),
        Some(KnownBuiltin::IndexOf)
    )
}

/// Whether `op` is char_at/charAt/code_unit_at at arity 2.
pub(crate) fn is_char_at_builtin(op: &str, arity: usize) -> bool {
    matches!(
        classify_known_builtin(op, arity),
        Some(KnownBuiltin::CharAt)
    )
}

/// Whether `op` is replace at arity 3.
pub(crate) fn is_replace_builtin(op: &str, arity: usize) -> bool {
    matches!(
        classify_known_builtin(op, arity),
        Some(KnownBuiltin::Replace)
    )
}

/// Whether `op` is split at arity 2.
pub(crate) fn is_split_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::Split))
}

/// Whether `op` is trim at arity 1.
pub(crate) fn is_trim_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::Trim))
}

/// Whether `op` is clone/to_string/to_owned/as_str at arity 1.
pub(crate) fn is_clone_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::Clone))
}

/// Whether `op` is reverse at arity 1.
pub(crate) fn is_reverse_builtin(op: &str, arity: usize) -> bool {
    matches!(
        classify_known_builtin(op, arity),
        Some(KnownBuiltin::Reverse)
    )
}

/// Whether `op` is clear at arity 1.
pub(crate) fn is_clear_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::Clear))
}

/// Whether `op` is push/push_back/push_front at arity 2.
pub(crate) fn is_push_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::Push))
}

/// Whether `op` is pop/pop_back/pop_front at arity 1.
pub(crate) fn is_pop_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::Pop))
}

/// Whether `op` is insert at arity 3.
pub(crate) fn is_insert_builtin(op: &str, arity: usize) -> bool {
    matches!(
        classify_known_builtin(op, arity),
        Some(KnownBuiltin::Insert)
    )
}

/// Whether `op` is remove/remove_at at arity 2.
pub(crate) fn is_remove_builtin(op: &str, arity: usize) -> bool {
    matches!(
        classify_known_builtin(op, arity),
        Some(KnownBuiltin::Remove)
    )
}

/// Whether `op` is slice at arity 3.
pub(crate) fn is_slice_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::Slice))
}

/// Whether `op` is take at arity 2.
pub(crate) fn is_take_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::Take))
}

/// Whether `op` is drop at arity 2.
pub(crate) fn is_drop_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::Drop))
}

/// Whether `op` is tail/rest at arity 1.
pub(crate) fn is_tail_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::Tail))
}

/// Whether `op` is first/last/head/front/back at arity 1.
pub(crate) fn is_first_builtin(op: &str, arity: usize) -> bool {
    matches!(classify_known_builtin(op, arity), Some(KnownBuiltin::First))
}

/// Case-fold methods not in [`KnownBuiltin`] but with Z3 length axioms (trim-like).
pub(crate) fn is_case_fold_method(op: &str, arity: usize) -> bool {
    arity == 1
        && matches!(
            op,
            "to_lowercase" | "to_uppercase" | "to_lower" | "to_upper"
        )
}

/// Builtin operations shared between CVC5 native/shell and (eventually) Z3 call encode.
/// Mirrors historical CVC5 `encode_call` / Z3 `encode_call` semantics for parity.
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

/// Render a known builtin as SMT-LIB2 prefix notation (solver-neutral text).
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
            Some(format!("({CONCAT_UF_NAME} {} {})", args[0], args[1]))
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
    fn pattern_hash_stable() {
        assert_eq!(pattern_hash_name("Some"), pattern_hash_name("Some"));
        assert_ne!(pattern_hash_name("Some"), pattern_hash_name("None"));
    }

    #[test]
    fn bool_tables() {
        assert!(is_bool_returning_uf("contains"));
        assert!(is_bool_field_name("is_empty"));
        assert!(!is_bool_field_name("len"));
        assert_eq!(MEASURE_EMPTY_CONST_NAME, "__empty");
    }

    #[test]
    fn classify_and_smtlib_abs_parity() {
        assert_eq!(classify_known_builtin("abs", 1), Some(KnownBuiltin::Abs));
        assert_eq!(
            known_builtin_to_smtlib("abs", &[String::from("x")]),
            Some("(ite (>= x 0) x (- x))".into())
        );
        assert_eq!(
            known_builtin_to_smtlib("append", &["a".into(), "b".into()]).as_deref(),
            Some("(__concat a b)")
        );
    }

    #[test]
    fn classify_collection_methods_parity() {
        assert_eq!(classify_known_builtin("push", 2), Some(KnownBuiltin::Push));
        assert_eq!(
            classify_known_builtin("push_back", 2),
            Some(KnownBuiltin::Push)
        );
        assert_eq!(classify_known_builtin("get", 2), Some(KnownBuiltin::Get));
        assert_eq!(classify_known_builtin("abs", 2), None);
        assert_eq!(classify_known_builtin("unknown", 1), None);
        assert!(is_known_builtin("push", 2));
        assert!(is_min_max_builtin("min", 2));
        assert!(is_min_max_builtin("max", 2));
        assert!(!is_min_max_builtin("min", 1));
        assert!(is_abs_builtin("abs", 1));
        assert!(is_collection_access_builtin("get", 2));
        assert!(is_collection_access_builtin("set", 3));
        assert!(is_collection_access_builtin("put", 3));
        assert!(is_get_builtin("get", 2));
        assert!(is_set_builtin("set", 3));
        assert!(is_put_builtin("put", 3));
        assert!(is_min_builtin("min", 2));
        assert!(is_max_builtin("max", 2));
        assert!(!is_min_builtin("max", 2));
        assert!(is_substring_builtin("substring", 3));
        assert!(is_substring_builtin("substr", 3));
        assert!(is_concat_append_builtin("concat", 2));
        assert!(is_concat_append_builtin("append", 2));
        assert!(is_index_of_builtin("index_of", 2));
        assert!(is_char_at_builtin("char_at", 2));
        assert!(is_replace_builtin("replace", 3));
    }
}
