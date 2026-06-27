//! CORE.5 Quantifier Trigger validation.

use std::ops::Range;

use assura_parser::ast::{ClauseKind, Expr, SpExpr};

use crate::TypeError;

/// Validates quantifier trigger annotations for verification performance.
///
/// Error codes:
/// - A53006: quantifier has no trigger annotation
/// - A53007: trigger references variable not bound by the quantifier
/// - A53008: trigger term is a sub-expression of the quantifier body (matching loop risk)
#[derive(Debug, Clone)]
pub struct QuantifierTriggerChecker {
    quantifiers: Vec<QuantifierInfo>,
}

#[derive(Debug, Clone)]
struct QuantifierInfo {
    var: String,
    has_trigger: bool,
    span: Range<usize>,
}

impl QuantifierTriggerChecker {
    pub fn new() -> Self {
        Self {
            quantifiers: Vec::new(),
        }
    }

    /// Register a quantifier expression found in a clause body.
    /// `has_trigger` indicates whether a trigger annotation (e.g., `triggers { ... }`)
    /// was found on this quantifier. Currently we detect trigger annotations by
    /// checking if the quantifier domain or body contains a `triggers` identifier.
    pub fn add_quantifier(&mut self, var: String, has_trigger: bool, span: Range<usize>) {
        self.quantifiers.push(QuantifierInfo {
            var,
            has_trigger,
            span,
        });
    }

    /// Check that all quantifiers have trigger annotations.
    /// Returns errors for quantifiers missing triggers.
    pub fn check_triggers(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for q in &self.quantifiers {
            if !q.has_trigger {
                errors.push(TypeError {
                    code: "A53006".into(),
                    message: format!(
                        "quantifier over `{}` has no trigger annotation; \
                         add a `triggers` clause for verification performance",
                        q.var
                    ),
                    span: q.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    /// AST-walking entry point: scan clause bodies for quantifiers missing triggers.
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = QuantifierTriggerChecker::new();
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn_extern(&decl.node) else {
                continue;
            };
            let has_strict = clauses
                .iter()
                .any(|c| matches!(&c.kind, ClauseKind::Other(k) if k == "strict_triggers"));
            if !has_strict {
                continue;
            }
            for clause in clauses {
                collect_quantifiers(&clause.body, &mut checker, &decl.span);
            }
        }
        checker.check_triggers()
    }
}

impl Default for QuantifierTriggerChecker {
    fn default() -> Self {
        Self::new()
    }
}

fn collect_quantifiers(
    expr: &SpExpr,
    checker: &mut QuantifierTriggerChecker,
    fallback_span: &std::ops::Range<usize>,
) {
    match &expr.node {
        Expr::Forall { var, domain, body } | Expr::Exists { var, domain, body } => {
            let has_trigger =
                expr_contains_text(domain, "triggers") || expr_contains_text(body, "triggers");
            checker.add_quantifier(var.clone(), has_trigger, fallback_span.clone());
            collect_quantifiers(domain, checker, fallback_span);
            collect_quantifiers(body, checker, fallback_span);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_quantifiers(lhs, checker, fallback_span);
            collect_quantifiers(rhs, checker, fallback_span);
        }
        Expr::UnaryOp { expr: e, .. } | Expr::Old(e) => {
            collect_quantifiers(e, checker, fallback_span);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_quantifiers(cond, checker, fallback_span);
            collect_quantifiers(then_branch, checker, fallback_span);
            if let Some(eb) = else_branch {
                collect_quantifiers(eb, checker, fallback_span);
            }
        }
        Expr::Call { func, args } => {
            collect_quantifiers(func, checker, fallback_span);
            for a in args {
                collect_quantifiers(a, checker, fallback_span);
            }
        }
        Expr::Block(exprs) | Expr::List(exprs) => {
            for e in exprs {
                collect_quantifiers(e, checker, fallback_span);
            }
        }
        Expr::Field(e, _) | Expr::Index { expr: e, .. } => {
            collect_quantifiers(e, checker, fallback_span);
        }
        Expr::Match { scrutinee, arms } => {
            collect_quantifiers(scrutinee, checker, fallback_span);
            for arm in arms {
                collect_quantifiers(&arm.body, checker, fallback_span);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_quantifiers(value, checker, fallback_span);
            collect_quantifiers(body, checker, fallback_span);
        }
        _ => {}
    }
}

fn expr_contains_text(expr: &SpExpr, text: &str) -> bool {
    match &expr.node {
        Expr::Ident(s) => s == text,
        Expr::Raw(tokens) => tokens.iter().any(|t| t == text),
        Expr::Block(exprs) | Expr::List(exprs) => exprs.iter().any(|e| expr_contains_text(e, text)),
        Expr::Call { func, args } => {
            expr_contains_text(func, text) || args.iter().any(|a| expr_contains_text(a, text))
        }
        _ => false,
    }
}
