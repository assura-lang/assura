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
#[cfg(feature = "cvc5-verify")]
pub(crate) const SIZE_FIELD_NAMES: &[&str] = &["len", "length", "size", "capacity", "count"];

#[cfg(feature = "cvc5-verify")]
pub(crate) fn is_bool_field(name: &str) -> bool {
    BOOL_FIELD_NAMES.contains(&name)
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn is_size_field(name: &str) -> bool {
    SIZE_FIELD_NAMES.contains(&name)
}

/// Builtin operations shared between native and shell-out backends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KnownBuiltin {
    Abs,
    Min,
    Max,
    Substring,
    Concat,
    IndexOf,
    CharAt,
    Replace,
    Split,
    Trim,
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
        ("index_of" | "find" | "indexOf", 2) => Some(KnownBuiltin::IndexOf),
        ("char_at" | "charAt", 2) => Some(KnownBuiltin::CharAt),
        ("replace", 3) => Some(KnownBuiltin::Replace),
        ("split", 2) => Some(KnownBuiltin::Split),
        ("trim" | "to_lowercase" | "to_uppercase" | "to_lower" | "to_upper", 1) => {
            Some(KnownBuiltin::Trim)
        }
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
        KnownBuiltin::Concat => Some(format!("(__concat {} {})", args[0], args[1])),
        KnownBuiltin::IndexOf => Some(format!("(index_of {} {})", args[0], args[1])),
        KnownBuiltin::CharAt => Some(format!("(char_at {} {})", args[0], args[1])),
        KnownBuiltin::Replace => Some(format!("(replace {} {} {})", args[0], args[1], args[2])),
        KnownBuiltin::Split => Some(format!("(split {} {})", args[0], args[1])),
        KnownBuiltin::Trim => Some(format!("({op} {})", args[0])),
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
    fn classify_unknown_arity_returns_none() {
        assert_eq!(classify_known_builtin("abs", 2), None);
        assert_eq!(classify_known_builtin("unknown", 1), None);
    }

    #[cfg(feature = "cvc5-verify")]
    #[test]
    fn bool_uf_membership() {
        assert!(is_bool_returning_uf("contains"));
        assert!(!is_bool_returning_uf("concat"));
    }
}
