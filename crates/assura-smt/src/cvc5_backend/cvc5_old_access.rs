//! Shared old() encoding for CVC5 shell-out and native backends.

use assura_parser::ast::{Expr, SpExpr, Spanned};

use crate::cvc5_common::old_ident_smtlib_name;
use crate::cvc5_field_access::{
    FieldAccessPlan, old_flat_field_smtlib, plan_field_access, shallow_field_smtlib,
};

/// How `old(inner)` should be encoded.
#[derive(Debug, Clone)]
pub(crate) enum OldAccessPlan {
    Ident(String),
    FlatField(String),
    ShallowField {
        obj: Box<SpExpr>,
        field: String,
    },
    MethodCall {
        receiver: Box<SpExpr>,
        method: String,
    },
    Other,
}

pub(crate) fn plan_old_access(inner: &Expr) -> OldAccessPlan {
    match inner {
        Expr::Ident(name) => OldAccessPlan::Ident(name.clone()),
        Expr::Field(obj, field) => match plan_field_access(&obj.as_ref().node, field) {
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

/// Encode `old(inner)` as SMT-LIB2 via recursive `encode` callback.
pub(crate) fn encode_old_smtlib<F>(inner: &SpExpr, mut encode: F) -> Option<String>
where
    F: FnMut(&SpExpr) -> Option<String>,
{
    match plan_old_access(&inner.node) {
        OldAccessPlan::Ident(name) => Some(old_ident_smtlib_name(&name)),
        OldAccessPlan::FlatField(flat) => Some(old_flat_field_smtlib(&flat)),
        OldAccessPlan::ShallowField { obj, field } => {
            let old_expr = Spanned::no_span(Expr::Old(obj));
            let old_obj = encode(&old_expr)?;
            Some(shallow_field_smtlib(&field, &old_obj))
        }
        OldAccessPlan::MethodCall { receiver, method } => {
            let old_expr = Spanned::no_span(Expr::Old(receiver));
            let old_recv = encode(&old_expr)?;
            Some(format!("({method} {old_recv})"))
        }
        OldAccessPlan::Other => encode(inner),
    }
}

/// Encode `old(inner)` as a native CVC5 term via recursive `encode` callback.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_old_cvc5<'a, E>(
    tm: &'a cvc5::TermManager,
    inner: &SpExpr,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
    mut encode: E,
) -> Option<cvc5::Term<'a>>
where
    E: FnMut(
        &SpExpr,
        &mut std::collections::HashMap<String, cvc5::Term<'a>>,
        &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
    ) -> Option<cvc5::Term<'a>>,
{
    use crate::cvc5_common::sanitize_smtlib_name;
    use crate::cvc5_field_access::encode_shallow_field_cvc5;
    use assura_parser::ast::Spanned;

    match plan_old_access(&inner.node) {
        OldAccessPlan::Ident(name) => {
            let key = sanitize_smtlib_name(&old_ident_smtlib_name(&name));
            Some(
                vars.get(&key)
                    .cloned()
                    .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &key)),
            )
        }
        OldAccessPlan::FlatField(flat) => {
            Some(tm.mk_const(tm.integer_sort(), &format!("{flat}__old")))
        }
        OldAccessPlan::ShallowField { obj, field } => {
            let old_expr = Spanned::no_span(Expr::Old(obj));
            let old_obj = encode(&old_expr, vars, state)?;
            Some(encode_shallow_field_cvc5(
                tm,
                &field,
                old_obj,
                &mut state.axioms,
                state.use_string_theory,
            ))
        }
        OldAccessPlan::MethodCall { receiver, method } => {
            let old_expr = Spanned::no_span(Expr::Old(receiver));
            let old_recv = encode(&old_expr, vars, state)?;
            let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let func_const = tm.mk_const(func_sort, &method);
            Some(tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, old_recv]))
        }
        OldAccessPlan::Other => encode(inner, vars, state),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_ident_plan() {
        assert!(matches!(
            plan_old_access(&Expr::Ident("x".into())),
            OldAccessPlan::Ident(name) if name == "x"
        ));
    }
}
