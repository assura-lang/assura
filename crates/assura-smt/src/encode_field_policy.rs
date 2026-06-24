//! Shared **field access** encode policy (encode convergence step 6).
//!
//! Owns flatten-vs-shallow-UF planning and SMT-LIB field/old naming so Z3
//! `encode_field_access` and CVC5 shell/native field paths agree on *which*
//! strategy applies before backend term construction.
//!
//! Complements [`crate::encode_atom_policy`] (UF/snapshot names),
//! [`crate::encode_method_policy`] (bool field tables), and
//! [`crate::unmodelable`] (field-chain depth / flatten).

use assura_ast::{Expr, SpExpr, Spanned};

use crate::encode_atom_policy::{field_uif_name, old_snapshot_name};
use crate::unmodelable::{flatten_field_chain_sp, has_deep_field_chain_sp, is_self_rooted_sp};

/// How a field access `obj.field` should be encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FieldAccessPlan {
    /// Deep/self-rooted chain flattened to a single name (`a__b__c`).
    Flatten(String),
    /// Shallow access via UF `__field_{field}(obj)`.
    ShallowUf { field: String },
}

/// Decide flatten-vs-UF encoding for `obj.field`.
pub(crate) fn plan_field_access(obj: &SpExpr, field: &str) -> FieldAccessPlan {
    let full_expr = Spanned::no_span(Expr::Field(Box::new(obj.clone()), field.to_string()));
    if has_deep_field_chain_sp(&full_expr) || is_self_rooted_sp(&full_expr) {
        FieldAccessPlan::Flatten(flatten_field_chain_sp(&full_expr))
    } else {
        FieldAccessPlan::ShallowUf {
            field: field.to_string(),
        }
    }
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
}
