//! Core structural checks.
//!
//! Liveness, axiomatic, CRUD auth.

use assura_parser::ast::{BlockKind, ClauseKind, Decl, Expr, ServiceItem, SpExpr};

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
                    code: "A31006".into(),
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
                    code: "A31007".into(),
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

/// CORE.5: Validate quantifier trigger annotations.
///
/// Scans clause bodies for `forall`/`exists` quantifiers and checks each
/// has a trigger annotation (a `triggers` sub-expression). Without triggers
/// the SMT solver may explore the quantifier body exhaustively, causing
/// verification timeouts.
///
/// Only fires on contracts/functions that opt in via a `strict_triggers true`
/// clause. Without this clause, quantifiers are allowed without trigger
/// annotations for ergonomic reasons (most simple quantifiers over finite
/// ranges do not need triggers).
pub(crate) fn run_quantifier_trigger_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = QuantifierTriggerChecker::new();

    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Extern(e) => &e.clauses,
            _ => continue,
        };

        // Only check quantifiers if the decl has `strict_triggers true`
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

/// Recursively walk an expression tree collecting quantifier occurrences.
fn collect_quantifiers(
    expr: &SpExpr,
    checker: &mut QuantifierTriggerChecker,
    fallback_span: &std::ops::Range<usize>,
) {
    match &expr.node {
        Expr::Forall { var, domain, body } | Expr::Exists { var, domain, body } => {
            // Check if the quantifier has a trigger annotation.
            // A trigger annotation is detected when the domain or body contains
            // a `triggers` identifier reference.
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

/// Structural check: top-level prophecy declarations must have a
/// matching resolve() call somewhere in the file. Without one, the
/// prophecy variable is never resolved, which is always an error.
///
/// Error code: A05025 (unresolved prophecy variable).
pub(crate) fn run_prophecy_resolution_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut errors = Vec::new();

    use assura_parser::ast::BlockKind;

    // Collect top-level prophecy declarations (both forms)
    let prophecies: Vec<(&str, &std::ops::Range<usize>)> = source
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

    let prophecy_names: std::collections::HashSet<&str> =
        prophecies.iter().map(|(n, _)| *n).collect();

    // Scan all clause bodies for references and resolve() calls
    let mut referenced_names = std::collections::HashSet::new();
    let mut resolved_names = std::collections::HashSet::new();
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            collect_resolve_calls(&clause.body, &mut resolved_names);
            collect_ident_refs(&clause.body, &prophecy_names, &mut referenced_names);
        }
    }

    // Only flag prophecies that are referenced but never resolved
    for (name, span) in prophecies {
        if referenced_names.contains(name) && !resolved_names.contains(name) {
            errors.push(TypeError {
                code: "A05025".into(),
                message: format!("prophecy variable `{name}` is never resolved"),
                span: span.clone(),
                secondary: None,
            });
        }
    }

    errors
}

/// Recursively collect identifier references that match known prophecy names.
fn collect_ident_refs(
    expr: &SpExpr,
    prophecy_names: &std::collections::HashSet<&str>,
    found: &mut std::collections::HashSet<String>,
) {
    match &expr.node {
        Expr::Ident(name) => {
            if prophecy_names.contains(name.as_str()) {
                found.insert(name.clone());
            }
        }
        Expr::Raw(tokens) => {
            // Raw clause bodies have parentheses stripped; scan tokens
            // for prophecy name references.
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

/// Recursively collect names passed to resolve() or resolve_prophecy() calls.
fn collect_resolve_calls(expr: &SpExpr, names: &mut std::collections::HashSet<String>) {
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
            // Raw clause bodies have parentheses stripped, producing
            // ["resolve", "var_name"] instead of a Call node. Scan for
            // the resolve/resolve_prophecy keyword followed by a name.
            for window in tokens.windows(2) {
                if (window[0] == "resolve" || window[0] == "resolve_prophecy") && window[1] != "(" {
                    names.insert(window[1].clone());
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
        let (sf, errs) = assura_parser::parse(src);
        assert!(errs.is_empty(), "parse errors: {errs:?}");
        sf.unwrap()
    }

    // --- prophecy resolution checks ---

    #[test]
    fn prophecy_referenced_but_unresolved() {
        let src = r#"
module test;
prophecy future_val: Int
contract Use {
    input(x: Int)
    requires { x > 0 }
    ensures { result > future_val }
}
"#;
        let sf = parse_source(src);
        let errors = run_prophecy_resolution_checks(&sf);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05025");
        assert!(errors[0].message.contains("future_val"));
    }

    #[test]
    fn prophecy_referenced_and_resolved() {
        let src = r#"
module test;
prophecy future_val: Int
contract Use {
    input(x: Int)
    requires { x > 0 }
    ensures { result > future_val }
    ensures { resolve(future_val) }
}
"#;
        let sf = parse_source(src);
        let errors = run_prophecy_resolution_checks(&sf);
        assert!(errors.is_empty(), "expected no errors: {errors:?}");
    }

    #[test]
    fn prophecy_declared_but_unused() {
        // Declared but never referenced in any clause: not an error.
        let src = r#"
module test;
prophecy unused_val: Int
contract Unrelated {
    input(x: Int)
    requires { x > 0 }
    ensures { result >= 0 }
}
"#;
        let sf = parse_source(src);
        let errors = run_prophecy_resolution_checks(&sf);
        assert!(
            errors.is_empty(),
            "unused prophecy should not error: {errors:?}"
        );
    }

    #[test]
    fn multiple_prophecies_mixed() {
        // Two prophecies in separate contracts to avoid parser merging
        // of consecutive prophecy declarations (known parser limitation).
        let src = r#"
module test;
prophecy alpha: Int

contract UseAlpha {
    input(x: Int)
    ensures { result > alpha }
    ensures { resolve(alpha) }
}

prophecy beta: Int

contract UseBeta {
    input(x: Int)
    ensures { result > beta }
}
"#;
        let sf = parse_source(src);
        let errors = run_prophecy_resolution_checks(&sf);
        assert_eq!(errors.len(), 1, "only beta should error: {errors:?}");
        assert!(errors[0].message.contains("beta"));
    }

    #[test]
    fn prophecy_resolved_via_resolve_prophecy() {
        let src = r#"
module test;
prophecy pv: Int
contract Use {
    input(x: Int)
    ensures { result > pv }
    ensures { resolve_prophecy(pv) }
}
"#;
        let sf = parse_source(src);
        let errors = run_prophecy_resolution_checks(&sf);
        assert!(
            errors.is_empty(),
            "resolve_prophecy should count: {errors:?}"
        );
    }

    #[test]
    fn no_prophecies_no_errors() {
        let src = r#"
module test;
contract Simple {
    input(x: Int)
    requires { x > 0 }
    ensures { result >= 0 }
}
"#;
        let sf = parse_source(src);
        let errors = run_prophecy_resolution_checks(&sf);
        assert!(errors.is_empty());
    }

    // --- liveness checks ---

    #[test]
    fn liveness_block_missing_prove() {
        let src = r#"
module test;
liveness EventualResponse {
    assume { fair }
}
"#;
        let sf = parse_source(src);
        let errors = run_liveness_checks(&sf);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A31006");
    }

    #[test]
    fn liveness_block_with_prove_ok() {
        let src = r#"
module test;
liveness EventualResponse {
    prove { leads_to(request, response) }
    assume { fair }
}
"#;
        let sf = parse_source(src);
        let errors = run_liveness_checks(&sf);
        assert!(
            errors.is_empty(),
            "valid liveness block should pass: {errors:?}"
        );
    }
}
