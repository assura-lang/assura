//! Shared **`old(expr)` pre-state access** policy (encode convergence step 7).
//!
//! `old(e)` means the value of `e` in the **pre-state** (before the step /
//! mutation), not “deprecated code.” Owns [`OldAccessPlan`] / [`plan_old_access`]
//! so Z3 and CVC5 agree on *which* shape applies (`old(x)` vs `old(obj.f)` vs
//! `old(r.m())`) before backend term construction.
//!
//! Complements [`crate::encode_field_policy`] (field flatten vs shallow UF),
//! [`crate::encode_atom_policy`] (`old_ident_name` / `old_snapshot_name` /
//! `__old` snapshot naming), and [`crate::encode_call_policy`] (method calls).
//!
//! **Naming note:** CVC5 live idents often go through [`encode_ident_name`]
//! (`result` → `__result`), so snapshots use [`old_ident_name`]. Z3 may keep
//! source `result` as `result`, so snapshots use [`old_snapshot_name`]. Planning
//! returns the source ident; backends pick the snapshot function.

use assura_ast::{Expr, SpExpr};

use crate::encode_field_policy::{FieldAccessPlan, plan_field_access};

/// How `old(inner)` should be encoded (pre-state snapshot strategy).
///
/// Not `PartialEq`: variants hold `SpExpr` boxes (spans are not meaningful to compare).
#[derive(Debug, Clone)]
pub(crate) enum OldAccessPlan {
    /// `old(x)` — snapshot of a simple identifier.
    Ident(String),
    /// `old(a.b.c)` when field policy flattens the chain (`a__b__c` + `__old`).
    FlatField(String),
    /// `old(obj.f)` as shallow field UF on `old(obj)`.
    ShallowField { obj: Box<SpExpr>, field: String },
    /// `old(recv.method(...))` as method UF on `old(recv)`.
    MethodCall {
        receiver: Box<SpExpr>,
        method: String,
    },
    /// Unsupported / complex inner: backends encode `inner` directly (weak).
    Other,
}

/// Classify `old(inner)` into an [`OldAccessPlan`] (shared Z3 / CVC5 order).
pub(crate) fn plan_old_access(inner: &SpExpr) -> OldAccessPlan {
    match &inner.node {
        Expr::Ident(name) => OldAccessPlan::Ident(name.clone()),
        Expr::Field(obj, field) => match plan_field_access(obj, field) {
            FieldAccessPlan::Flatten(flat) => OldAccessPlan::FlatField(flat),
            FieldAccessPlan::ShallowUf { field: f } => OldAccessPlan::ShallowField {
                obj: obj.clone(),
                field: f,
            },
        },
        Expr::MethodCall {
            receiver, method, ..
        } => OldAccessPlan::MethodCall {
            receiver: receiver.clone(),
            method: method.clone(),
        },
        _ => OldAccessPlan::Other,
    }
}

/// SMT-LIB2 shape for `old(recv).method` as unary UF apply: `(method old_recv)`.
pub(crate) fn old_method_call_smtlib(method: &str, old_recv_smt: &str) -> String {
    format!("({method} {old_recv_smt})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::Spanned;

    #[test]
    fn old_ident_plan() {
        assert!(matches!(
            plan_old_access(&Spanned::no_span(Expr::Ident("x".into()))),
            OldAccessPlan::Ident(name) if name == "x"
        ));
    }

    #[test]
    fn old_shallow_field_plan() {
        let obj = Box::new(Spanned::no_span(Expr::Ident("buf".into())));
        let inner = Spanned::no_span(Expr::Field(obj, "len".into()));
        assert!(matches!(
            plan_old_access(&inner),
            OldAccessPlan::ShallowField { field, .. } if field == "len"
        ));
    }

    #[test]
    fn old_self_rooted_field_flattens() {
        let obj = Box::new(Spanned::no_span(Expr::Ident("self".into())));
        let inner = Spanned::no_span(Expr::Field(obj, "head".into()));
        assert!(matches!(
            plan_old_access(&inner),
            OldAccessPlan::FlatField(name) if name == "self__head"
        ));
    }

    #[test]
    fn old_method_call_smtlib_shape() {
        assert_eq!(
            old_method_call_smtlib("length", "buf__old"),
            "(length buf__old)"
        );
    }
}
