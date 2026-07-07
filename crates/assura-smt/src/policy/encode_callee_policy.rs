//! Functional callee specs for ensures-side call equating.
//!
//! When a pure same-file helper has `ensures { result == <expr> }`, calls to
//! that helper in another contract's ensures (or IR call plumbing) can be
//! expanded to `<expr>` with parameters bound to the call arguments, instead
//! of an unconstrained uninterpreted function.
//!
//! This closes the gap where IR inlines `call double` to `x+x` but the ensures
//! `result == double(x)` still treated `double` as a free UF (#P1 call equating).

use assura_ast::{BinOp, Clause, ClauseKind, Expr, Param, SpExpr};
use std::collections::HashMap;

/// Pure functional definition extracted from a helper's ensures clauses.
#[derive(Debug, Clone)]
pub struct CalleeFunctionalSpec {
    pub param_names: Vec<String>,
    /// Body of `result == body` (right-hand side preferred).
    pub result_body: SpExpr,
}

/// Build a map of declaration name → functional ensures body from verification jobs.
pub fn collect_callee_functional_specs(
    jobs: &[(String, Vec<Clause>, Vec<Param>, Vec<String>)],
) -> HashMap<String, CalleeFunctionalSpec> {
    let mut out = HashMap::new();
    for (name, clauses, params, _ret) in jobs {
        if let Some(spec) = functional_spec_from_clauses(clauses, params) {
            out.insert(name.clone(), spec);
        }
    }
    out
}

/// Extract `result == <expr>` from ensures when `<expr>` is free of nested calls.
pub fn functional_spec_from_clauses(
    clauses: &[Clause],
    params: &[Param],
) -> Option<CalleeFunctionalSpec> {
    let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
    for clause in clauses.iter().filter(|c| c.kind == ClauseKind::Ensures) {
        let (lhs, rhs) = equality_operands(&clause.body)?;
        let body = if is_result_ident(lhs) {
            rhs
        } else if is_result_ident(rhs) {
            lhs
        } else {
            continue;
        };
        // Reject nested calls / control flow for safe equating.
        if expr_has_nested_call(body) {
            continue;
        }
        // Body must only reference params (and literals), not free result/other.
        if !expr_only_uses_params(body, &param_names) {
            continue;
        }
        return Some(CalleeFunctionalSpec {
            param_names,
            result_body: body.clone(),
        });
    }
    None
}

fn equality_operands(expr: &SpExpr) -> Option<(&SpExpr, &SpExpr)> {
    match &expr.node {
        Expr::BinOp {
            op: BinOp::Eq,
            lhs,
            rhs,
        } => Some((lhs.as_ref(), rhs.as_ref())),
        _ => None,
    }
}

fn is_result_ident(expr: &SpExpr) -> bool {
    matches!(&expr.node, Expr::Ident(name) if name == "result")
}

fn expr_has_nested_call(expr: &SpExpr) -> bool {
    match &expr.node {
        Expr::Call { .. } | Expr::MethodCall { .. } => true,
        Expr::If { .. } | Expr::Match { .. } | Expr::Forall { .. } | Expr::Exists { .. } => true,
        Expr::BinOp { lhs, rhs, .. } => expr_has_nested_call(lhs) || expr_has_nested_call(rhs),
        Expr::UnaryOp { expr: inner, .. } | Expr::Old(inner) => expr_has_nested_call(inner),
        Expr::Field(inner, _) => expr_has_nested_call(inner),
        Expr::Index { expr, index } => expr_has_nested_call(expr) || expr_has_nested_call(index),
        Expr::List(elems) | Expr::Tuple(elems) => elems.iter().any(expr_has_nested_call),
        _ => false,
    }
}

fn expr_only_uses_params(expr: &SpExpr, params: &[String]) -> bool {
    match &expr.node {
        Expr::Ident(name) => params.iter().any(|p| p == name),
        Expr::Literal(_) => true,
        Expr::BinOp { lhs, rhs, .. } => {
            expr_only_uses_params(lhs, params) && expr_only_uses_params(rhs, params)
        }
        Expr::UnaryOp { expr: inner, .. } | Expr::Old(inner) | Expr::Field(inner, _) => {
            expr_only_uses_params(inner, params)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::{BinOp, Clause, ClauseKind, Expr, Param, Spanned};

    fn sp(e: Expr) -> SpExpr {
        Spanned::no_span(e)
    }
    fn spb(e: Expr) -> Box<SpExpr> {
        Box::new(sp(e))
    }

    #[test]
    fn extracts_x_plus_x_for_double() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::BinOp {
                    op: BinOp::Add,
                    lhs: spb(Expr::Ident("x".into())),
                    rhs: spb(Expr::Ident("x".into())),
                }),
            }),
            effect_variables: vec![],
        }];
        let params = vec![Param {
            name: "x".into(),
            ty: None,
        }];
        let spec = functional_spec_from_clauses(&clauses, &params).expect("spec");
        assert_eq!(spec.param_names, vec!["x".to_string()]);
        assert!(matches!(
            &spec.result_body.node,
            Expr::BinOp { op: BinOp::Add, .. }
        ));
    }

    #[test]
    fn rejects_nested_call_body() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::Call {
                    func: spb(Expr::Ident("other".into())),
                    args: vec![sp(Expr::Ident("x".into()))],
                }),
            }),
            effect_variables: vec![],
        }];
        let params = vec![Param {
            name: "x".into(),
            ty: None,
        }];
        assert!(functional_spec_from_clauses(&clauses, &params).is_none());
    }
}
