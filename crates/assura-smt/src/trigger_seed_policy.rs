//! Shared **trigger/e-matching seed** from clause/expression trees (one compiler brain).
//!
//! Owns which function/method names are registered on a [`TriggerManager`] by walking
//! `Call` / `MethodCall` (and recursing through the full expr tree). Z3 and CVC5
//! previously duplicated this walk with slightly different coverage (Z3 omitted
//! `Field`/`Apply`/`Let`/`Match`/`Cast`/collections); both use this module now.
//!
//! Complements [`crate::lemma_inject_policy`] (`apply` lemma names, not Call/MethodCall).
//! Does not own quantifier pattern validation or SMT term construction.

use assura_ast::{Clause, Expr, SpExpr};

use crate::advanced::TriggerManager;

/// Register `Call`/`MethodCall` names from an expression for quantifier e-matching.
pub(crate) fn register_trigger_functions_from_expr(expr: &SpExpr, tm: &mut TriggerManager) {
    match &expr.node {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = &func.as_ref().node {
                tm.register_function(name.clone());
            }
            for a in args {
                register_trigger_functions_from_expr(a, tm);
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            tm.register_function(method.clone());
            register_trigger_functions_from_expr(receiver, tm);
            for a in args {
                register_trigger_functions_from_expr(a, tm);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            register_trigger_functions_from_expr(lhs, tm);
            register_trigger_functions_from_expr(rhs, tm);
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Old(inner)
        | Expr::Ghost(inner)
        | Expr::Field(inner, _)
        | Expr::Cast { expr: inner, .. } => {
            register_trigger_functions_from_expr(inner, tm);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            register_trigger_functions_from_expr(cond, tm);
            register_trigger_functions_from_expr(then_branch, tm);
            if let Some(eb) = else_branch {
                register_trigger_functions_from_expr(eb, tm);
            }
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            register_trigger_functions_from_expr(domain, tm);
            register_trigger_functions_from_expr(body, tm);
        }
        Expr::Index { expr: e, index } => {
            register_trigger_functions_from_expr(e, tm);
            register_trigger_functions_from_expr(index, tm);
        }
        Expr::Block(items) | Expr::Tuple(items) | Expr::List(items) => {
            for e in items {
                register_trigger_functions_from_expr(e, tm);
            }
        }
        Expr::Apply { args, .. } => {
            for a in args {
                register_trigger_functions_from_expr(a, tm);
            }
        }
        Expr::Let { value, body, .. } => {
            register_trigger_functions_from_expr(value, tm);
            register_trigger_functions_from_expr(body, tm);
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            register_trigger_functions_from_expr(scrutinee, tm);
            for arm in arms {
                register_trigger_functions_from_expr(&arm.body, tm);
            }
        }
        _ => {}
    }
}

/// Seed a trigger manager from all clause bodies (contract-level prelude step).
pub(crate) fn seed_trigger_manager_from_clauses(clauses: &[Clause], tm: &mut TriggerManager) {
    for clause in clauses {
        register_trigger_functions_from_expr(&clause.body, tm);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::{ClauseKind, Spanned};

    fn sp(e: Expr) -> SpExpr {
        Spanned::no_span(e)
    }
    fn spb(e: Expr) -> Box<SpExpr> {
        Box::new(sp(e))
    }

    #[test]
    fn registers_call_and_method_names() {
        let mut tm = TriggerManager::new();
        let expr = sp(Expr::BinOp {
            op: assura_ast::BinOp::And,
            lhs: spb(Expr::Call {
                func: spb(Expr::Ident("f".into())),
                args: vec![],
            }),
            rhs: spb(Expr::MethodCall {
                receiver: spb(Expr::Ident("x".into())),
                method: "len".into(),
                args: vec![],
            }),
        });
        register_trigger_functions_from_expr(&expr, &mut tm);
        let known = tm.known_functions();
        assert!(known.iter().any(|n| n == "f"));
        assert!(known.iter().any(|n| n == "len"));
    }

    #[test]
    fn walks_field_and_let_not_only_shallow_calls() {
        let mut tm = TriggerManager::new();
        // Call nested under Field (old Z3 walk skipped Field entirely).
        let field_only = sp(Expr::Field(
            spb(Expr::Call {
                func: spb(Expr::Ident("inner_fn".into())),
                args: vec![],
            }),
            "x".into(),
        ));
        register_trigger_functions_from_expr(&field_only, &mut tm);
        assert!(tm.known_functions().iter().any(|n| n == "inner_fn"));

        let mut tm2 = TriggerManager::new();
        let let_e = sp(Expr::Let {
            name: "y".into(),
            value: spb(Expr::Ident("x".into())),
            body: spb(Expr::MethodCall {
                receiver: spb(Expr::Ident("y".into())),
                method: "via_let".into(),
                args: vec![],
            }),
        });
        register_trigger_functions_from_expr(&let_e, &mut tm2);
        assert!(tm2.known_functions().iter().any(|n| n == "via_let"));
    }

    #[test]
    fn seed_from_clauses_covers_all_bodies() {
        let mut tm = TriggerManager::new();
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: sp(Expr::Call {
                    func: spb(Expr::Ident("req_fn".into())),
                    args: vec![],
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: sp(Expr::MethodCall {
                    receiver: spb(Expr::Ident("r".into())),
                    method: "ens_m".into(),
                    args: vec![],
                }),
                effect_variables: vec![],
            },
        ];
        seed_trigger_manager_from_clauses(&clauses, &mut tm);
        let known = tm.known_functions();
        assert!(known.iter().any(|n| n == "req_fn"));
        assert!(known.iter().any(|n| n == "ens_m"));
    }
}
