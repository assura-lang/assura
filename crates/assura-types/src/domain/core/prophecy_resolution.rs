//! Prophecy resolution checker.

use std::collections::HashSet;
use std::ops::Range;

use assura_parser::ast::{BlockKind, Decl, Expr, SpExpr};

use crate::TypeError;

/// Validates that prophecy declarations have matching resolve() calls.
pub(crate) struct ProphecyResolutionChecker;

impl ProphecyResolutionChecker {
    /// AST-walking entry point: check that each referenced prophecy is resolved.
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut errors = Vec::new();
        let prophecies: Vec<(&str, &Range<usize>)> = source
            .decls
            .iter()
            .filter_map(|d| match &d.node {
                Decl::Prophecy(p) => Some((p.name.as_str(), &d.span)),
                Decl::Block {
                    kind: BlockKind::Other(k),
                    name,
                    ..
                } if k == "prophecy" => Some((name.as_str(), &d.span)),
                _ => None,
            })
            .collect();
        if prophecies.is_empty() {
            return errors;
        }
        let prophecy_names: HashSet<&str> = prophecies.iter().map(|(n, _)| *n).collect();
        let mut referenced_names = HashSet::new();
        let mut resolved_names = HashSet::new();
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                collect_resolve_calls(&clause.body, &mut resolved_names);
                collect_ident_refs(&clause.body, &prophecy_names, &mut referenced_names);
            }
        }
        for (name, span) in prophecies {
            if referenced_names.contains(name) && !resolved_names.contains(name) {
                errors.push(TypeError {
                    code: "A05025".into(),
                    message: format!("prophecy variable `{name}` is never resolved"),
                    span: span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }
}

fn collect_ident_refs(expr: &SpExpr, prophecy_names: &HashSet<&str>, found: &mut HashSet<String>) {
    match &expr.node {
        Expr::Ident(name) => {
            if prophecy_names.contains(name.as_str()) {
                found.insert(name.clone());
            }
        }
        Expr::Raw(tokens) => {
            for tok in tokens {
                if prophecy_names.contains(tok.as_str()) {
                    found.insert(tok.clone());
                }
            }
        }
        Expr::Call { func, args } => {
            collect_ident_refs(func, prophecy_names, found);
            for arg in args {
                collect_ident_refs(arg, prophecy_names, found);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_ident_refs(lhs, prophecy_names, found);
            collect_ident_refs(rhs, prophecy_names, found);
        }
        Expr::UnaryOp { expr, .. } | Expr::Old(expr) | Expr::Ghost(expr) => {
            collect_ident_refs(expr, prophecy_names, found);
        }
        Expr::Block(es) | Expr::List(es) => {
            for e in es {
                collect_ident_refs(e, prophecy_names, found);
            }
        }
        _ => {}
    }
}

fn collect_resolve_calls(expr: &SpExpr, names: &mut HashSet<String>) {
    match &expr.node {
        Expr::Call { func, args } => {
            if let Expr::Ident(fname) = &func.as_ref().node
                && (fname == "resolve" || fname == "resolve_prophecy")
                && let Some(_sp_arg) = args.first()
                && let Expr::Ident(var) = &_sp_arg.node
            {
                names.insert(var.clone());
            }
            for arg in args {
                collect_resolve_calls(arg, names);
            }
        }
        Expr::Raw(tokens) => {
            // Parentheses may be kept in clause raw tokens after nested-delimiter
            // preservation (`resolve(x)` → ["resolve", "(", "x", ")"]). Accept both
            // adjacent and parenthesized forms (#899).
            let mut i = 0;
            while i < tokens.len() {
                let t = tokens[i].as_str();
                if t == "resolve" || t == "resolve_prophecy" {
                    let arg = match tokens.get(i + 1).map(|s| s.as_str()) {
                        Some("(") => tokens.get(i + 2).map(|s| s.as_str()),
                        Some(name) if name != ")" => Some(name),
                        _ => None,
                    };
                    if let Some(name) = arg.filter(|n| !n.is_empty() && *n != ")") {
                        names.insert(name.to_string());
                    }
                }
                i += 1;
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_resolve_calls(lhs, names);
            collect_resolve_calls(rhs, names);
        }
        Expr::UnaryOp { expr, .. } | Expr::Old(expr) | Expr::Ghost(expr) => {
            collect_resolve_calls(expr, names);
        }
        Expr::Block(es) | Expr::List(es) => {
            for e in es {
                collect_resolve_calls(e, names);
            }
        }
        _ => {}
    }
}
