//! Variable name collection from AST expressions (CVC5 backends).

use std::collections::HashSet;

use assura_ast::{Expr, SpExpr};

use crate::cvc5_common::{sanitize_smtlib_name, smtlib_result_name};

/// Collect all variable names referenced in an expression.
pub fn collect_vars(expr: &SpExpr, vars: &mut HashSet<String>) {
    match &expr.node {
        Expr::Ident(name) => {
            if name == "result" {
                vars.insert(smtlib_result_name().to_string());
            } else {
                vars.insert(sanitize_smtlib_name(name));
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_vars(lhs, vars);
            collect_vars(rhs, vars);
        }
        Expr::UnaryOp { expr: inner, .. } => collect_vars(inner, vars),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_vars(cond, vars);
            collect_vars(then_branch, vars);
            if let Some(e) = else_branch {
                collect_vars(e, vars);
            }
        }
        Expr::Forall {
            var, body, domain, ..
        }
        | Expr::Exists {
            var, body, domain, ..
        } => {
            collect_vars(body, vars);
            collect_vars(domain, vars);
            vars.remove(&sanitize_smtlib_name(var));
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_vars(arg, vars);
            }
        }
        Expr::Old(inner) | Expr::Ghost(inner) => {
            collect_vars(inner, vars);
        }
        Expr::Cast { expr: inner, .. } => collect_vars(inner, vars),
        Expr::Field(receiver, _) => collect_vars(receiver, vars),
        Expr::MethodCall { receiver, args, .. } => {
            collect_vars(receiver, vars);
            for arg in args {
                collect_vars(arg, vars);
            }
        }
        Expr::Index { expr, index } => {
            collect_vars(expr, vars);
            collect_vars(index, vars);
        }
        Expr::Let { value, body, .. } => {
            collect_vars(value, vars);
            collect_vars(body, vars);
        }
        Expr::Match { scrutinee, arms } => {
            collect_vars(scrutinee, vars);
            for arm in arms {
                collect_vars(&arm.body, vars);
            }
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                collect_vars(item, vars);
            }
        }
        Expr::Apply { args, .. } => {
            for arg in args {
                collect_vars(arg, vars);
            }
        }
        Expr::Literal(_) => {}
        Expr::Raw(tokens) => {
            for tok in tokens {
                if tok
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                    && tok != "true"
                    && tok != "false"
                {
                    vars.insert(sanitize_smtlib_name(tok));
                }
            }
        }
    }
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn collect_cvc5_var_names(requires: &[&SpExpr], body: &SpExpr) -> HashSet<String> {
    let mut names = HashSet::new();
    for req in requires {
        collect_vars(req, &mut names);
    }
    collect_vars(body, &mut names);
    names
}

pub(crate) fn collect_cvc5_var_names_from_clauses(
    requires: &[&SpExpr],
    clauses: &[&assura_ast::Clause],
) -> HashSet<String> {
    let mut names = HashSet::new();
    for req in requires {
        collect_vars(req, &mut names);
    }
    for clause in clauses {
        collect_vars(&clause.body, &mut names);
    }
    names
}

#[cfg(feature = "cvc5-verify")]
#[cfg_attr(feature = "z3-verify", allow(dead_code))]
pub(crate) fn collect_cvc5_var_names_from_assumptions(
    assumptions: &[&SpExpr],
    body: &SpExpr,
) -> HashSet<String> {
    let mut names = HashSet::new();
    for a in assumptions {
        collect_vars(a, &mut names);
    }
    collect_vars(body, &mut names);
    names
}

#[cfg(test)]
mod tests {
    use assura_ast::{Expr, Spanned};

    use super::*;

    fn sp(e: Expr) -> SpExpr {
        Spanned::no_span(e)
    }
    fn spb(e: Expr) -> Box<SpExpr> {
        Box::new(sp(e))
    }

    #[test]
    fn collect_vars_ident() {
        let mut vars = HashSet::new();
        collect_vars(&sp(Expr::Ident("x".into())), &mut vars);
        assert!(vars.contains("x"));
    }

    #[test]
    fn collect_vars_result() {
        let mut vars = HashSet::new();
        collect_vars(&sp(Expr::Ident("result".into())), &mut vars);
        assert!(vars.contains("__result"));
        assert!(!vars.contains("result"));
    }

    #[test]
    fn collect_vars_quantifier_removes_bound_var() {
        let mut vars = HashSet::new();
        let forall_expr = sp(Expr::Forall {
            var: "i".into(),
            domain: spb(Expr::Ident("domain".into())),
            body: spb(Expr::Ident("i".into())),
        });
        collect_vars(&forall_expr, &mut vars);
        assert!(!vars.contains("i"));
        assert!(vars.contains("domain"));
    }
}
