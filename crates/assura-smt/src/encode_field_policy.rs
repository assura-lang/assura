//! Shared **field access** encode policy (encode convergence step 6).
//!
//! Owns flatten-vs-shallow-UF planning, ident `.len`/`.length` canonical-length
//! fast path, and SMT-LIB field/old naming so Z3 `encode_field_access` and CVC5
//! shell/native field paths agree on *which* strategy applies before backend
//! term construction.
//!
//! String-theory `str.len` on non-ident receivers stays backend-local (needs
//! solver sort / `use_string_theory`); only the ident canonical-length path is
//! fully shared here (parity with [`crate::encode_call_policy`] length preambles).
//!
//! Complements [`crate::encode_atom_policy`] (UF/snapshot names),
//! [`crate::encode_method_policy`] (bool field tables), and
//! [`crate::unmodelable`] (field-chain depth / flatten).

use assura_ast::{Expr, SpExpr, Spanned};

use crate::encode_atom_policy::{
    canonical_length_name, field_uif_name, is_length_method_name, old_snapshot_name,
};
use crate::unmodelable::{flatten_field_chain_sp, has_deep_field_chain_sp, is_self_rooted_sp};

/// How a field access `obj.field` should be encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FieldAccessPlan {
    /// `ident.len` / `ident.length` → shared canonical length variable (`#267`).
    CanonicalLength { obj_name: String },
    /// Deep/self-rooted chain flattened to a single name (`a__b__c`).
    Flatten(String),
    /// Shallow access via UF `__field_{field}(obj)`.
    ShallowUf { field: String },
}

/// If `obj.field` is an ident + `len`/`length` length field, return the ident name.
///
/// Shared guard for Z3/CVC5/shell before flatten/shallow or string-theory fallthrough.
pub(crate) fn ident_length_field_obj_name<'a>(obj: &'a SpExpr, field: &str) -> Option<&'a str> {
    if !is_length_method_name(field) {
        return None;
    }
    match &obj.node {
        Expr::Ident(name) => Some(name.as_str()),
        _ => None,
    }
}

/// Whether backends may try solver string-theory length on this field access.
///
/// True when the field is `len`/`length` and the receiver is **not** a simple
/// ident (idents use [`FieldAccessPlan::CanonicalLength`] first). Term build
/// (Z3 `Str::length`, CVC5 `StringLength`) remains backend-local.
pub(crate) fn field_access_may_use_string_theory_length(obj: &SpExpr, field: &str) -> bool {
    is_length_method_name(field) && ident_length_field_obj_name(obj, field).is_none()
}

/// Decide encoding strategy for `obj.field` (canonical length, flatten, or shallow UF).
pub(crate) fn plan_field_access(obj: &SpExpr, field: &str) -> FieldAccessPlan {
    if let Some(name) = ident_length_field_obj_name(obj, field) {
        return FieldAccessPlan::CanonicalLength {
            obj_name: name.to_string(),
        };
    }
    let full_expr = Spanned::no_span(Expr::Field(Box::new(obj.clone()), field.to_string()));
    if has_deep_field_chain_sp(&full_expr) || is_self_rooted_sp(&full_expr) {
        FieldAccessPlan::Flatten(flatten_field_chain_sp(&full_expr))
    } else {
        FieldAccessPlan::ShallowUf {
            field: field.to_string(),
        }
    }
}

/// SMT-LIB name for [`FieldAccessPlan::CanonicalLength`] (`__canonical_len_{obj}`).
pub(crate) fn canonical_length_field_smtlib(obj_name: &str) -> String {
    canonical_length_name(obj_name)
}

/// SMT-LIB / solver UF name for a shallow field (`__field_{field}`).
pub(crate) fn field_uf_smtlib_name(field: &str) -> String {
    field_uif_name(field)
}

/// Render a shallow field UF in SMT-LIB2: `(__field_f obj)`.
pub(crate) fn shallow_field_smtlib(field: &str, obj_smt: &str) -> String {
    format!("({} {obj_smt})", field_uf_smtlib_name(field))
}

/// Render `old(flattened)` as `{flat}__old` (source/flat snapshot naming).
pub(crate) fn old_flat_field_smtlib(flat_name: &str) -> String {
    old_snapshot_name(flat_name)
}

/// Shallow / flattened field **sort + axiom** class (solver-neutral).
///
/// Used after [`plan_field_access`] resolves flatten vs shallow so Z3
/// `encode_field_access` and CVC5 `encode_shallow_field_cvc5` / flatten arms
/// agree on bool vs size-nonneg vs default int before term construction.
/// String-theory length on shallow receivers remains backend-local (sort /
/// `use_string_theory` checks in Z3/CVC5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldValueKind {
    /// Boolean field table ([`crate::encode_method_policy::is_bool_field_name`]).
    Bool,
    /// Size/length-like field with non-negativity axiom ([`is_size_field_name`]).
    SizeNonNeg,
    /// Default uninterpreted int (or bool/int UF result sort at backend).
    Int,
}

/// Classify a source field name for flatten/shallow encode (not the plan shape).
pub(crate) fn classify_field_value_kind(field: &str) -> FieldValueKind {
    use crate::encode_atom_policy::is_size_field_name;
    use crate::encode_method_policy::is_bool_field_name;

    if is_bool_field_name(field) {
        FieldValueKind::Bool
    } else if is_size_field_name(field) {
        FieldValueKind::SizeNonNeg
    } else {
        FieldValueKind::Int
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shallow_field_for_simple_access() {
        let obj = Spanned::no_span(Expr::Ident("x".into()));
        assert_eq!(
            plan_field_access(&obj, "y"),
            FieldAccessPlan::ShallowUf { field: "y".into() }
        );
    }

    #[test]
    fn ident_len_field_is_canonical_length() {
        let obj = Spanned::no_span(Expr::Ident("raw".into()));
        assert_eq!(
            plan_field_access(&obj, "length"),
            FieldAccessPlan::CanonicalLength {
                obj_name: "raw".into()
            }
        );
        assert_eq!(
            plan_field_access(&obj, "len"),
            FieldAccessPlan::CanonicalLength {
                obj_name: "raw".into()
            }
        );
        assert_eq!(ident_length_field_obj_name(&obj, "length"), Some("raw"));
        assert_eq!(
            canonical_length_field_smtlib("raw"),
            canonical_length_name("raw")
        );
        assert!(!field_access_may_use_string_theory_length(&obj, "length"));
    }

    #[test]
    fn non_ident_length_may_use_string_theory() {
        // Receiver is a method call (not Ident), so not CanonicalLength; shallow UF plan.
        let recv = Spanned::no_span(Expr::Ident("s".into()));
        let obj = Spanned::no_span(Expr::MethodCall {
            receiver: Box::new(recv),
            method: "to_string".into(),
            args: vec![],
        });
        assert!(field_access_may_use_string_theory_length(&obj, "length"));
        assert!(matches!(
            plan_field_access(&obj, "length"),
            FieldAccessPlan::ShallowUf { field } if field == "length"
        ));
    }

    #[test]
    fn flatten_self_rooted_chain() {
        let parent = Spanned::no_span(Expr::Ident("self".into()));
        assert!(matches!(
            plan_field_access(&parent, "head"),
            FieldAccessPlan::Flatten(_)
        ));
        if let FieldAccessPlan::Flatten(name) = plan_field_access(&parent, "head") {
            assert_eq!(name, "self__head");
        }
    }

    #[test]
    fn shallow_field_smtlib_shape() {
        assert_eq!(shallow_field_smtlib("len", "buf"), "(__field_len buf)");
        assert_eq!(old_flat_field_smtlib("state__head"), "state__head__old");
    }

    #[test]
    fn field_value_kind_tables() {
        assert_eq!(classify_field_value_kind("is_empty"), FieldValueKind::Bool);
        assert_eq!(classify_field_value_kind("len"), FieldValueKind::SizeNonNeg);
        assert_eq!(
            classify_field_value_kind("length"),
            FieldValueKind::SizeNonNeg
        );
        assert_eq!(classify_field_value_kind("head"), FieldValueKind::Int);
        assert_eq!(
            classify_field_value_kind("capacity"),
            FieldValueKind::SizeNonNeg
        );
    }
}
