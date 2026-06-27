//! Core structural checks (thin wrappers).
//!
//! Liveness, axiomatic, CRUD auth, quantifier triggers, prophecy resolution.

use assura_parser::ast::{BlockKind, ClauseKind, Decl, Expr, SpExpr};

use crate::TypeError;
use crate::domain::*;

pub(crate) fn run_liveness_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut errors = Vec::new();
    for decl in &source.decls {
        if let Decl::Block {
            kind, name, body, ..
        } = &decl.node
        {
            if *kind != BlockKind::Liveness {
                continue;
            }
            let has_prove = body
                .iter()
                .any(|c| matches!(&c.kind, ClauseKind::Other(k) if k == "prove"));
            if !has_prove {
                errors.push(TypeError {
                    code: "A31006".into(),
                    message: format!(
                        "liveness block `{name}` has no `prove` clause; \
                         at least one liveness property must be stated"
                    ),
                    span: decl.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
            let has_leads_to = body.iter().any(|c| {
                matches!(&c.kind, ClauseKind::Other(k) if k == "prove")
                    && expr_contains_text(&c.body, "leads_to")
            });
            let has_fair = body.iter().any(|c| {
                matches!(&c.kind, ClauseKind::Other(k) if k == "assume")
                    && expr_contains_text(&c.body, "fair")
            });
            if has_leads_to && !has_fair {
                errors.push(TypeError {
                    code: "A31007".into(),
                    message: format!(
                        "liveness block `{name}` uses `leads_to` but has no \
                         `assume fair` clause; fairness is required for \
                         leads-to proofs"
                    ),
                    span: decl.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
    }
    errors
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

pub(crate) fn run_axiomatic_checks(
    source: &assura_parser::ast::SourceFile,
    symbols: &assura_resolve::SymbolTable,
) -> Vec<TypeError> {
    AxiomaticDefChecker::check_source(source, symbols)
}

pub(crate) fn run_crud_auth_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    CrudAuthContracts::check_source(source)
}

pub(crate) fn run_quantifier_trigger_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    QuantifierTriggerChecker::check_source(source)
}

pub(crate) fn run_prophecy_resolution_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    ProphecyResolutionChecker::check_source(source)
}

#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;
