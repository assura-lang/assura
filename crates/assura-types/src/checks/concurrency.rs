//! Concurrency-related checks.
//!
//! Determinism, callback re-entrancy, temporal deadlines.

use assura_parser::ast::{ClauseKind, Decl, Expr};

use crate::TypeError;
use crate::checkers::*;
use crate::domain::*;
use crate::types::*;

// ---------------------------------------------------------------------------
// Determinism wiring (T067)
// ---------------------------------------------------------------------------

/// Scan for functions with `pure` effect annotation and check that their
/// clause bodies do not reference non-deterministic sources.
pub(crate) fn run_determinism_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut all_errors = Vec::new();
    let mut checker = DeterminismChecker::new();

    for decl in &source.decls {
        let (fn_name, clauses) = match &decl.node {
            Decl::FnDef(f) => (f.name.as_str(), f.clauses.as_slice()),
            Decl::Contract(c) => (c.name.as_str(), c.clauses.as_slice()),
            _ => continue,
        };

        // Check if the function has a pure effects clause
        let is_pure = clauses.iter().any(|c| {
            c.kind == ClauseKind::Effects && matches!(&c.body, Expr::Ident(name) if name == "pure")
        });
        if !is_pure {
            continue;
        }

        checker.mark_deterministic(fn_name.to_string());

        // Register custom non-deterministic sources from annotations
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && k == "non_deterministic"
            {
                for name in collect_ident_references(&clause.body) {
                    checker.add_non_det_source(name);
                }
            }
        }

        // Collect all identifiers referenced in clause bodies
        let mut used_names = Vec::new();
        for clause in clauses {
            let refs = collect_ident_references(&clause.body);
            used_names.extend(refs);
        }

        for err in checker.check_fn_body(fn_name, &used_names, &decl.span) {
            all_errors.push(TypeError {
                code: err.code,
                message: err.message,
                span: err.span,
                secondary: None,
            });
        }

        // Check iteration over non-deterministic collections
        for name in &used_names {
            for err in checker.check_iteration(fn_name, name, &decl.span) {
                all_errors.push(TypeError {
                    code: err.code,
                    message: err.message,
                    span: err.span,
                    secondary: None,
                });
            }
        }
    }

    all_errors
}

pub(crate) fn run_callback_reentrancy_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = CallbackReentrancyChecker::new();
    let mut found = false;
    let mut max_depth_override: Option<usize> = None;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "non_reentrant" || k == "callback")
            {
                found = true;
                if let Expr::Ident(name) = &clause.body {
                    checker.mark_non_reentrant(name.clone(), decl.span.clone());
                }
            }
            // Extract max_depth configuration
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "max_depth" || k == "callback_depth")
                && let Some(depth) = extract_int_literal(&clause.body)
            {
                max_depth_override = Some(depth as usize);
            }
        }
    }
    // Apply max_depth configuration if specified
    if let Some(depth) = max_depth_override {
        checker = checker.with_max_depth(depth);
    }
    if !found {
        return Vec::new();
    }
    // Walk call references in clause bodies and simulate call/return for re-entrancy.
    //
    // A declaration that marks a function as non_reentrant AND references that
    // same function in its requires/ensures clause bodies represents a potential
    // re-entrant call pattern. This catches both:
    // - A function calling itself (self-re-entry)
    // - A contract with non_reentrant annotation that references the guarded fn
    let mut errors = Vec::new();
    for decl in &source.decls {
        let (fn_name, clauses) = match &decl.node {
            Decl::FnDef(f) => (f.name.as_str(), &f.clauses),
            Decl::Contract(c) => (c.name.as_str(), &c.clauses),
            _ => continue,
        };
        // Enter the function scope
        let enter_errors = checker.enter_call(fn_name, &decl.span);
        errors.extend(enter_errors);
        // Check for callback registration and re-entrant calls in clause bodies
        for clause in clauses {
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(err) = checker.check_register_callback(name, &decl.span) {
                        errors.push(err);
                    }
                    // Simulate re-entrant call: if a non-reentrant function
                    // references itself (or another non-reentrant function
                    // already on the stack) in its clause bodies, that is
                    // a re-entrant call pattern.
                    let re_enter_errors = checker.enter_call(name, &decl.span);
                    errors.extend(re_enter_errors);
                    checker.exit_call();
                }
            }
        }
        checker.exit_call();
    }
    // Static re-entrancy detection: if a declaration marks a function as
    // non_reentrant and also references that function in clause bodies,
    // flag the potential re-entrant invocation.
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => c.clauses.as_slice(),
            Decl::FnDef(f) => f.clauses.as_slice(),
            _ => continue,
        };
        // Collect non-reentrant targets declared in this decl
        let mut nr_targets: Vec<String> = Vec::new();
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && k == "non_reentrant"
                && let Expr::Ident(name) = &clause.body
            {
                nr_targets.push(name.clone());
            }
        }
        if nr_targets.is_empty() {
            continue;
        }
        // Check if clause bodies reference the non-reentrant targets
        for clause in clauses {
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if nr_targets.contains(name) {
                        errors.push(TypeError {
                            code: "A24001".into(),
                            message: format!("re-entrant call to non-reentrant function `{name}`"),
                            span: decl.span.clone(),
                            secondary: None,
                        });
                    }
                }
            }
        }
    }
    // Include depth information in diagnostics if there are errors
    if !errors.is_empty() {
        let depth = checker.current_depth();
        if depth > 0 {
            errors.push(TypeError {
                code: "A24003".into(),
                message: format!("callback stack depth is {depth} at end of analysis"),
                span: 0..1,
                secondary: None,
            });
        }
    }
    errors
}

/// Scan for temporal deadline annotations and validate deadlines.
pub(crate) fn run_temporal_deadline_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = TemporalDeadlineChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "deadline" || k == "timeout" || k == "bounded_time")
            {
                found = true;
                // Extract deadline name and value from expression
                match &clause.body {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = func.as_ref() {
                            let ms = args
                                .first()
                                .and_then(extract_int_literal)
                                .unwrap_or(DEFAULT_DEADLINE_MS)
                                as u64;
                            if let Some(err) = checker.enter_deadline(name.clone(), ms, &decl.span)
                            {
                                return vec![err];
                            }
                        }
                    }
                    Expr::Ident(name) => {
                        // bare identifier, use default 1000ms
                        if let Some(err) = checker.enter_deadline(name.clone(), 1000, &decl.span) {
                            return vec![err];
                        }
                    }
                    _ => {
                        // Try to extract kv pairs for named params
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed");
                        let ms = kvs
                            .iter()
                            .find(|(k, _)| *k == "ms" || *k == "timeout")
                            .and_then(|(_, v)| extract_int_literal(v))
                            .unwrap_or(DEFAULT_DEADLINE_MS) as u64;
                        if let Some(err) = checker.enter_deadline(name.to_string(), ms, &decl.span)
                        {
                            return vec![err];
                        }
                    }
                }
            }
            // Register operation bounds
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "worst_case" || k == "bound")
                && let Some((op, args)) = extract_call(&clause.body)
            {
                let ms = args.first().and_then(extract_int_literal).unwrap_or(0) as u64;
                checker.register_bound(op.to_string(), ms);
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Check operations within deadline contexts
    let mut errors = Vec::new();
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
                    if let Some(err) = checker.check_operation(name, &decl.span) {
                        // Include current deadline context in error
                        if let Some((dl_name, dl_ms)) = checker.current_deadline() {
                            errors.push(TypeError {
                                code: err.code.clone(),
                                message: format!(
                                    "{} (active deadline: `{dl_name}` {dl_ms}ms)",
                                    err.message
                                ),
                                span: err.span.clone(),
                                secondary: err.secondary.clone(),
                            });
                        } else {
                            errors.push(err);
                        }
                    }
                }
            }
            // Exit deadline scope for scope-exit annotations
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "exit_deadline" || k == "end_deadline")
            {
                checker.exit_deadline();
            }
        }
    }
    errors
}
