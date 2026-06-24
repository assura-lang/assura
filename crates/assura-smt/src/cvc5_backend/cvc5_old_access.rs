//! CVC5 `old(expr)` encoding (shell SMT-LIB2 + native terms).
//!
//! Pre-state planning lives in [`crate::encode_old_policy`]; this module owns
//! CVC5/shell term construction and keeps stable `cvc5_old_access::*` imports.

use assura_ast::{Expr, SpExpr, Spanned};

use crate::encode_atom_policy::old_ident_name;
use crate::encode_field_policy::{old_flat_field_smtlib, shallow_field_smtlib};
// Re-export policy surface for tests / any CVC5-local callers.
pub(crate) use crate::encode_old_policy::{OldAccessPlan, old_method_call_smtlib, plan_old_access};

/// Encode `old(inner)` as SMT-LIB2 via recursive `encode` callback.
pub(crate) fn encode_old_smtlib<F>(inner: &SpExpr, mut encode: F) -> Option<String>
where
    F: FnMut(&SpExpr) -> Option<String>,
{
    match plan_old_access(inner) {
        OldAccessPlan::Ident(name) => Some(old_ident_name(&name)),
        OldAccessPlan::FlatField(flat) => Some(old_flat_field_smtlib(&flat)),
        OldAccessPlan::ShallowField { obj, field } => {
            let old_expr = Spanned::no_span(Expr::Old(obj));
            let old_obj = encode(&old_expr)?;
            Some(shallow_field_smtlib(&field, &old_obj))
        }
        OldAccessPlan::MethodCall { receiver, method } => {
            let old_expr = Spanned::no_span(Expr::Old(receiver));
            let old_recv = encode(&old_expr)?;
            Some(old_method_call_smtlib(&method, &old_recv))
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
    use crate::cvc5_field_access::encode_shallow_field_cvc5;
    use assura_ast::Spanned;

    match plan_old_access(inner) {
        OldAccessPlan::Ident(name) => {
            // `old_ident_name` already sanitizes; do not double-sanitize.
            let key = old_ident_name(&name);
            Some(
                vars.get(&key)
                    .cloned()
                    .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &key)),
            )
        }
        OldAccessPlan::FlatField(flat) => {
            let key = crate::encode_atom_policy::old_snapshot_name(&flat);
            Some(tm.mk_const(tm.integer_sort(), &key))
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
