//! Shared **ADT / constructor-test** encode policy (encode convergence step).
//!
//! Owns solver-neutral SMT-LIB2 shapes for tag tests and accessors used by Z3
//! ADT emulation and CVC5 shell (`cvc5_adt`). Tag/accessor **UF name strings**
//! delegate to [`crate::encode_atom_policy`]; term/axiom construction stays
//! backend-local.
//!
//! Complements [`crate::encode_match_policy`] (arm kinds / ctor-tag idents via
//! FNV hash when no ADT registry) and [`crate::encode_method_policy::pattern_hash_name`].

/// SMT-LIB2 condition `(= (__adt_tag_<adt> value) tag)` for constructor tests.
pub(crate) fn adt_is_constructor_smtlib(adt_name: &str, value: &str, ctor_tag: i64) -> String {
    let tag_fn = crate::encode_atom_policy::adt_tag_uf_name(adt_name);
    format!("(= ({tag_fn} {value}) {ctor_tag})")
}

/// SMT-LIB2 application `(__adt_<adt>_<accessor> value)`.
pub(crate) fn adt_accessor_smtlib(adt_name: &str, accessor: &str, value: &str) -> String {
    let acc_fn = crate::encode_atom_policy::adt_accessor_uf_name(adt_name, accessor);
    format!("({acc_fn} {value})")
}

/// Look up sequential ctor tag (0-based definition order) by constructor name.
///
/// Returns `None` if `ctor_name` is not in `ctor_names` (caller may default to 0
/// or overapproximate, matching historical CVC5/Z3 behavior).
pub(crate) fn adt_ctor_tag_by_name(ctor_names: &[&str], ctor_name: &str) -> Option<i64> {
    ctor_names
        .iter()
        .position(|n| *n == ctor_name)
        .map(|i| i as i64)
}

/// Resolve ctor tag with historical default: unknown ctor → tag `0`.
pub(crate) fn adt_ctor_tag_or_zero(ctor_names: &[&str], ctor_name: &str) -> i64 {
    adt_ctor_tag_by_name(ctor_names, ctor_name).unwrap_or(0)
}

/// Fresh synthetic ADT name for match-arm registration (`__match_adt_{n}`).
pub(crate) fn match_adt_fresh_name(counter: impl std::fmt::Display) -> String {
    crate::encode_atom_policy::match_adt_fresh_name(counter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor_smtlib_shapes() {
        assert_eq!(
            adt_is_constructor_smtlib("Opt", "x", 1),
            "(= (__adt_tag_Opt x) 1)"
        );
        assert_eq!(adt_accessor_smtlib("Opt", "val", "x"), "(__adt_Opt_val x)");
        let names = ["None", "Some"];
        assert_eq!(adt_ctor_tag_by_name(&names, "Some"), Some(1));
        assert_eq!(adt_ctor_tag_or_zero(&names, "Missing"), 0);
        assert_eq!(
            adt_is_constructor_smtlib("Opt", "x", adt_ctor_tag_or_zero(&names, "Some")),
            "(= (__adt_tag_Opt x) 1)"
        );
        assert_eq!(match_adt_fresh_name(3), "__match_adt_3");
    }
}
