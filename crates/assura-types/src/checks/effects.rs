//! Effect checking.

use assura_parser::ast::{ClauseKind, Decl, Expr, ServiceItem};
use std::collections::HashMap;

use crate::TypeError;
use crate::checkers::*;

pub(crate) fn run_effect_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let checker = EffectChecker::new();
    let mut errors = Vec::new();

    // Pass 1: Build effect map (name -> declared EffectSet) for call-graph inference.
    let effect_map = build_effect_map(source, &checker);

    for decl in &source.decls {
        match &decl.node {
            Decl::FnDef(f) => {
                let (declared, actual) = extract_effects_from_clauses(&f.clauses);
                if let Some(ref declared_set) = declared {
                    // Validate all effect names are known
                    for ee in checker.check_known(declared_set, &decl.span) {
                        errors.push(TypeError {
                            code: ee.code,
                            message: ee.message,
                            span: ee.span,
                            secondary: None,
                        });
                    }
                    // Check containment: actual subset of declared
                    if let Some(actual_set) = actual {
                        for ee in checker.check_containment(declared_set, &actual_set, &decl.span) {
                            errors.push(TypeError {
                                code: ee.code,
                                message: ee.message,
                                span: ee.span,
                                secondary: None,
                            });
                        }
                    }
                }

                // Pass 2: Call-graph effect inference. For each function call in
                // clause bodies, look up the callee's declared effects and check
                // they are a subset of the caller's declared effects.
                if let Some(ref declared_set) = declared {
                    let callee_effects = infer_callee_effects(&f.clauses, &effect_map);
                    for ee in checker.check_containment(declared_set, &callee_effects, &decl.span) {
                        // Rewrite the error message to include call-graph context
                        errors.push(TypeError {
                            code: ee.code,
                            message: ee.message,
                            span: ee.span,
                            secondary: None,
                        });
                    }
                }
            }
            Decl::Extern(e) => {
                let (declared, _) = extract_effects_from_clauses(&e.clauses);
                if let Some(declared_set) = declared {
                    for ee in checker.check_known(&declared_set, &decl.span) {
                        errors.push(TypeError {
                            code: ee.code,
                            message: ee.message,
                            span: ee.span,
                            secondary: None,
                        });
                    }
                }
            }
            Decl::Contract(c) => {
                let (declared, _) = extract_effects_from_clauses(&c.clauses);
                if let Some(declared_set) = declared {
                    for ee in checker.check_known(&declared_set, &decl.span) {
                        errors.push(TypeError {
                            code: ee.code,
                            message: ee.message,
                            span: ee.span,
                            secondary: None,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    errors
}

/// Build a map from function/contract/extern names to their declared (expanded)
/// effect sets. Used for call-graph-based effect inference in S002.
pub(crate) fn build_effect_map(
    source: &assura_parser::ast::SourceFile,
    checker: &EffectChecker,
) -> HashMap<String, EffectSet> {
    let mut map = HashMap::new();
    for decl in &source.decls {
        match &decl.node {
            Decl::FnDef(f) => {
                let (declared, _) = extract_effects_from_clauses(&f.clauses);
                if let Some(declared_set) = declared {
                    map.insert(f.name.clone(), checker.expand(&declared_set));
                }
            }
            Decl::Contract(c) => {
                let (declared, _) = extract_effects_from_clauses(&c.clauses);
                if let Some(declared_set) = declared {
                    map.insert(c.name.clone(), checker.expand(&declared_set));
                }
            }
            Decl::Extern(e) => {
                let (declared, _) = extract_effects_from_clauses(&e.clauses);
                if let Some(declared_set) = declared {
                    map.insert(e.name.clone(), checker.expand(&declared_set));
                }
            }
            Decl::Service(s) => {
                // Service operations may have effects
                for item in &s.items {
                    if let ServiceItem::Operation { name, clauses, .. } = item {
                        let (declared, _) = extract_effects_from_clauses(clauses);
                        if let Some(declared_set) = declared {
                            map.insert(name.clone(), checker.expand(&declared_set));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    map
}

/// Infer the union of all callee effects from function calls in clause bodies.
///
/// Scans `requires`, `ensures`, and `modifies` clause bodies for `Call` and
/// `MethodCall` expressions. For each call target that appears in the effect
/// map, unions that target's effects into the result.
fn infer_callee_effects(
    clauses: &[assura_parser::ast::Clause],
    effect_map: &HashMap<String, EffectSet>,
) -> EffectSet {
    let mut result = EffectSet::pure();
    for clause in clauses {
        if matches!(
            clause.kind,
            ClauseKind::Requires
                | ClauseKind::Ensures
                | ClauseKind::Modifies
                | ClauseKind::Invariant
                | ClauseKind::Rule
        ) {
            collect_call_effects(&clause.body, effect_map, &mut result);
        }
    }
    result
}

/// Recursively collect effects from function calls in an expression.
fn collect_call_effects(
    expr: &Expr,
    effect_map: &HashMap<String, EffectSet>,
    effects: &mut EffectSet,
) {
    match expr {
        Expr::Call { func, args } => {
            // Extract the function name from the call target
            if let Some(name) = extract_call_name(func)
                && let Some(callee_effects) = effect_map.get(&name)
            {
                for eff in callee_effects.iter() {
                    effects.insert(eff.to_string());
                }
            }
            // Also recurse into arguments
            for arg in args {
                collect_call_effects(arg, effect_map, effects);
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            // Check if the method name is in the effect map
            if let Some(callee_effects) = effect_map.get(method.as_str()) {
                for eff in callee_effects.iter() {
                    effects.insert(eff.to_string());
                }
            }
            collect_call_effects(receiver, effect_map, effects);
            for arg in args {
                collect_call_effects(arg, effect_map, effects);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_call_effects(lhs, effect_map, effects);
            collect_call_effects(rhs, effect_map, effects);
        }
        Expr::UnaryOp { expr: inner, .. } => {
            collect_call_effects(inner, effect_map, effects);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_call_effects(cond, effect_map, effects);
            collect_call_effects(then_branch, effect_map, effects);
            if let Some(el) = else_branch {
                collect_call_effects(el, effect_map, effects);
            }
        }
        Expr::Block(items) | Expr::List(items) | Expr::Tuple(items) => {
            for item in items {
                collect_call_effects(item, effect_map, effects);
            }
        }
        Expr::Forall { body, domain, .. } | Expr::Exists { body, domain, .. } => {
            collect_call_effects(body, effect_map, effects);
            collect_call_effects(domain, effect_map, effects);
        }
        Expr::Old(inner)
        | Expr::Paren(inner)
        | Expr::Ghost(inner)
        | Expr::Field(inner, _)
        | Expr::Cast { expr: inner, .. } => {
            collect_call_effects(inner, effect_map, effects);
        }
        Expr::Index { expr: base, index } => {
            collect_call_effects(base, effect_map, effects);
            collect_call_effects(index, effect_map, effects);
        }
        Expr::Apply { args, .. } => {
            for arg in args {
                collect_call_effects(arg, effect_map, effects);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_call_effects(value, effect_map, effects);
            collect_call_effects(body, effect_map, effects);
        }
        Expr::Match { scrutinee, arms } => {
            collect_call_effects(scrutinee, effect_map, effects);
            for arm in arms {
                collect_call_effects(&arm.body, effect_map, effects);
            }
        }
        // Leaf expressions have no sub-calls
        Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => {}
    }
}

/// Extract a function name from a Call target expression.
fn extract_call_name(func: &Expr) -> Option<String> {
    match func {
        Expr::Ident(name) => Some(name.clone()),
        Expr::Field(_, name) => Some(name.clone()),
        _ => None,
    }
}

/// Extract declared and actual effect sets from a list of clauses.
fn extract_effects_from_clauses(
    clauses: &[assura_parser::ast::Clause],
) -> (Option<EffectSet>, Option<EffectSet>) {
    let mut declared: Option<EffectSet> = None;
    let mut actual: Option<EffectSet> = None;

    for clause in clauses {
        if clause.kind == ClauseKind::Effects {
            // Extract effect names from the clause body
            let effects = extract_effect_names_from_expr(&clause.body);
            declared = Some(EffectSet::from_effect_names(effects));
        }
    }

    // Infer actual effects from other clauses (requires/ensures with IO references)
    let mut inferred = EffectSet::pure();
    for clause in clauses {
        if matches!(
            clause.kind,
            ClauseKind::Requires | ClauseKind::Ensures | ClauseKind::Modifies
        ) {
            infer_effects_from_expr(&clause.body, &mut inferred);
        }
    }
    if !inferred.is_pure() {
        actual = Some(inferred);
    }

    (declared, actual)
}

/// Extract effect names from an effects clause expression.
///
/// Effect names may be dot-separated (e.g., `console.read`) which the lexer
/// tokenizes as `["console", ".", "read"]`. This function joins them back
/// into single names before returning.
fn extract_effect_names_from_expr(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::Ident(name) => vec![name.clone()],
        Expr::Raw(tokens) => {
            // Join dot-separated tokens: ["console", ".", "read"] -> "console.read"
            let filtered: Vec<&str> = tokens
                .iter()
                .map(|s| s.as_str())
                .filter(|t| *t != "," && *t != "{" && *t != "}")
                .collect();
            let mut names = Vec::new();
            let mut current = String::new();
            for tok in filtered {
                if tok == "." {
                    current.push('.');
                } else if current.ends_with('.') {
                    current.push_str(tok);
                } else {
                    if !current.is_empty() {
                        names.push(current);
                    }
                    current = tok.to_string();
                }
            }
            if !current.is_empty() {
                names.push(current);
            }
            names
        }
        Expr::Block(items) => items
            .iter()
            .flat_map(extract_effect_names_from_expr)
            .collect(),
        Expr::Field(base, field) => {
            // Field access expression: `console.read` parsed as Field(Ident("console"), "read")
            let mut base_names = extract_effect_names_from_expr(base);
            if let Some(last) = base_names.last_mut() {
                last.push('.');
                last.push_str(field);
            } else {
                base_names.push(field.clone());
            }
            base_names
        }
        _ => Vec::new(),
    }
}

/// Infer effects from expression content (look for IO-related identifiers).
///
/// Recognizes the full effect hierarchy from Section 3.6 of the spec:
/// - `io` sub-effects: console, file, network, process, env, time, random
/// - `mem` effects: alloc, dealloc, resize
/// - `panic` effects: panic, abort, unreachable
fn infer_effects_from_expr(expr: &Expr, effects: &mut EffectSet) {
    match expr {
        Expr::Ident(name) => {
            // IO sub-effects: console, file, network, socket, http, process, env, time, random
            let io_prefixes = [
                "console",
                "file",
                "stdin",
                "stdout",
                "stderr",
                "network",
                "socket",
                "http",
                "tcp",
                "udp",
                "process",
                "env",
                "time",
                "random",
                "rand",
                "print",
                "read_line",
                "write_file",
                "read_file",
                "open",
                "close",
                "flush",
                "seek",
            ];
            for prefix in &io_prefixes {
                if name.starts_with(prefix) || name == *prefix {
                    effects.insert("io".into());
                    return;
                }
            }
            // Memory effects
            if name.starts_with("alloc")
                || name.starts_with("dealloc")
                || name.starts_with("malloc")
                || name.starts_with("free")
                || name.starts_with("realloc")
                || name.starts_with("resize")
            {
                effects.insert("mem".into());
            }
            // Panic/divergence effects
            if name == "panic"
                || name == "abort"
                || name == "unreachable"
                || name == "exit"
                || name == "todo"
            {
                effects.insert("panic".into());
            }
        }
        Expr::Field(base, field) => {
            // Detect `obj.read()`, `obj.write()`, etc.
            let io_methods = [
                "read",
                "write",
                "flush",
                "close",
                "open",
                "seek",
                "send",
                "recv",
                "connect",
                "listen",
                "accept",
                "print",
                "println",
                "read_line",
            ];
            if io_methods.contains(&field.as_str()) {
                effects.insert("io".into());
            }
            infer_effects_from_expr(base, effects);
        }
        Expr::Call { func, args } => {
            infer_effects_from_expr(func, effects);
            for a in args {
                infer_effects_from_expr(a, effects);
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let io_methods = [
                "read",
                "write",
                "flush",
                "close",
                "open",
                "seek",
                "send",
                "recv",
                "connect",
                "listen",
                "accept",
                "print",
                "println",
                "read_line",
            ];
            if io_methods.contains(&method.as_str()) {
                effects.insert("io".into());
            }
            infer_effects_from_expr(receiver, effects);
            for a in args {
                infer_effects_from_expr(a, effects);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            infer_effects_from_expr(lhs, effects);
            infer_effects_from_expr(rhs, effects);
        }
        Expr::UnaryOp { expr, .. } | Expr::Paren(expr) | Expr::Old(expr) => {
            infer_effects_from_expr(expr, effects);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            infer_effects_from_expr(cond, effects);
            infer_effects_from_expr(then_branch, effects);
            if let Some(e) = else_branch {
                infer_effects_from_expr(e, effects);
            }
        }
        Expr::Block(items) | Expr::List(items) => {
            for item in items {
                infer_effects_from_expr(item, effects);
            }
        }
        _ => {}
    }
}
