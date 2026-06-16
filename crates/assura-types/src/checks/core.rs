//! Core structural checks.
//!
//! Liveness, axiomatic, CRUD auth.

use assura_parser::ast::{BlockKind, ClauseKind, Decl, Expr, ServiceItem};

use crate::TypeError;
use crate::checkers::*;
use crate::domain::*;

/// G006/T094: Validate liveness blocks have required structure.
///
/// Checks that liveness blocks contain at least one `prove` clause
/// and that `leads_to` obligations have accompanying `assume fair`.
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
                    code: "A-CORE-030".into(),
                    message: format!(
                        "liveness block `{name}` has no `prove` clause; \
                         at least one liveness property must be stated"
                    ),
                    span: decl.span.clone(),
                    secondary: None,
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
                    code: "A-CORE-031".into(),
                    message: format!(
                        "liveness block `{name}` uses `leads_to` but has no \
                         `assume fair` clause; fairness is required for \
                         leads-to proofs"
                    ),
                    span: decl.span.clone(),
                    secondary: None,
                });
            }
        }
    }
    errors
}

/// Helper: check if an expression tree contains a text reference.
fn expr_contains_text(expr: &Expr, text: &str) -> bool {
    match expr {
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
    let mut checker = AxiomaticDefChecker::new();
    // First pass: collect all axiom names
    let axiom_names: Vec<String> = source
        .decls
        .iter()
        .filter_map(|d| {
            if let Decl::Block { kind, name, .. } = &d.node
                && *kind == BlockKind::Axiomatic
            {
                Some(name.clone())
            } else {
                None
            }
        })
        .collect();
    // Second pass: declare axioms with references extracted from their bodies
    for decl in &source.decls {
        if let Decl::Block {
            kind, name, body, ..
        } = &decl.node
            && *kind == BlockKind::Axiomatic
        {
            let mut refs = Vec::new();
            for clause in body {
                let idents = collect_ident_references(&clause.body);
                for ident in &idents {
                    if axiom_names.contains(ident) && ident != name {
                        refs.push(ident.clone());
                    }
                }
            }
            refs.sort();
            refs.dedup();
            checker.declare_axiom(AxiomDef {
                name: name.clone(),
                span: decl.span.clone(),
                references: refs,
            });
        }
    }
    // Mark axioms as used if they are referenced in clause bodies
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    checker.mark_used(name);
                }
            }
        }
    }
    let known: Vec<&str> = symbols.symbols.iter().map(|s| s.name.as_str()).collect();
    let mut errors = checker.check_references(&known);
    errors.extend(checker.check_unused());
    errors.extend(checker.check_circular());
    errors
}

/// T109: Scan services for CRUD operations and check auth coverage.
pub(crate) fn run_crud_auth_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut errors = Vec::new();
    for decl in &source.decls {
        if let Decl::Service(s) = &decl.node {
            let mut checker = CrudAuthContracts::new();
            for item in &s.items {
                if let ServiceItem::Operation { name, clauses } = item {
                    let has_auth = clauses.iter().any(|c| {
                        matches!(c.kind, ClauseKind::Other(ref k) if k == "auth" || k == "requires_auth")
                    });
                    let crud_type = if name.starts_with("create") || name.starts_with("add") {
                        CrudType::Create
                    } else if name.starts_with("read")
                        || name.starts_with("get")
                        || name.starts_with("list")
                    {
                        CrudType::Read
                    } else if name.starts_with("update") || name.starts_with("set") {
                        CrudType::Update
                    } else if name.starts_with("delete") || name.starts_with("remove") {
                        CrudType::Delete
                    } else {
                        continue;
                    };
                    checker.add_crud(name.clone(), crud_type, has_auth);
                }
            }
            // Add auth policies from service-level auth clauses
            for item in &s.items {
                if let ServiceItem::Operation { name, clauses } = item {
                    for clause in clauses {
                        if let ClauseKind::Other(ref k) = clause.kind
                            && (k == "auth_policy" || k == "role")
                        {
                            let role = extract_ident(&clause.body).unwrap_or("user").to_string();
                            let allow_self = clauses.iter().any(
                                |c| matches!(&c.kind, ClauseKind::Other(k2) if k2 == "allow_self"),
                            );
                            checker.add_auth_policy(name.clone(), role, allow_self);
                        }
                    }
                }
            }
            errors.extend(checker.check_auth_coverage());
            errors.extend(checker.check_delete_protection());
            errors.extend(checker.check_precondition_coverage());
        }
    }
    errors
}
