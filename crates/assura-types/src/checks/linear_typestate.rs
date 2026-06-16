//! Linearity and typestate checks.

use assura_parser::ast::{ClauseKind, Decl, Expr, ServiceItem};

use crate::TypeError;
use crate::checkers::*;

pub(crate) fn run_linearity_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut errors = Vec::new();
    for decl in &source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                let mut tracker = UsageTracker::new();
                // Declare inputs as linear if they have linear annotation
                for clause in &c.clauses {
                    if clause.kind == ClauseKind::Input {
                        declare_linear_params_from_expr(&clause.body, &mut tracker, &decl.span);
                    }
                }
                // Walk ensures/requires/invariant bodies
                let mut ctx = LinearContext::new(tracker);
                for clause in &c.clauses {
                    if matches!(
                        clause.kind,
                        ClauseKind::Requires | ClauseKind::Ensures | ClauseKind::Invariant
                    ) {
                        errors.extend(check_expr_linearity(&clause.body, &mut ctx));
                    }
                }
                errors.extend(ctx.check());
            }
            Decl::FnDef(f) => {
                let tracker = UsageTracker::new();
                let mut ctx = LinearContext::new(tracker);
                for param in &f.params {
                    let grade = infer_usage_grade(&param.ty);
                    if grade != UsageGrade::Unlimited {
                        ctx.declare(param.name.clone(), grade, decl.span.clone());
                    }
                }
                for clause in &f.clauses {
                    errors.extend(check_expr_linearity(&clause.body, &mut ctx));
                }
                errors.extend(ctx.check());
            }
            Decl::Extern(e) => {
                let tracker = UsageTracker::new();
                let mut ctx = LinearContext::new(tracker);
                for param in &e.params {
                    let grade = infer_usage_grade(&param.ty);
                    if grade != UsageGrade::Unlimited {
                        ctx.declare(param.name.clone(), grade, decl.span.clone());
                    }
                }
                for clause in &e.clauses {
                    errors.extend(check_expr_linearity(&clause.body, &mut ctx));
                }
                errors.extend(ctx.check());
            }
            Decl::Service(s) => {
                for item in &s.items {
                    if let ServiceItem::Operation { clauses, .. }
                    | ServiceItem::Query { clauses, .. } = item
                    {
                        let tracker = UsageTracker::new();
                        let mut ctx = LinearContext::new(tracker);
                        for clause in clauses {
                            errors.extend(check_expr_linearity(&clause.body, &mut ctx));
                        }
                        errors.extend(ctx.check());
                    }
                }
            }
            _ => {}
        }
    }
    errors
}

/// Infer a usage grade from type annotation tokens.
///
/// - `linear` -> Linear (grade 1)
/// - `ghost` or `erased` -> Erased (grade 0)
/// - `exact(N)` -> Exact(N)
/// - otherwise -> Unlimited (grade omega)
fn infer_usage_grade(ty_tokens: &[String]) -> UsageGrade {
    for (i, t) in ty_tokens.iter().enumerate() {
        match t.as_str() {
            "linear" => return UsageGrade::Linear,
            "ghost" | "erased" => return UsageGrade::Erased,
            "exact" => {
                // Look for a number after "exact"
                if let Some(n_str) = ty_tokens.get(i + 1)
                    && let Ok(n) = n_str.parse::<u32>()
                {
                    return UsageGrade::Exact(n);
                }
                return UsageGrade::Linear;
            }
            _ => {}
        }
    }
    UsageGrade::Unlimited
}

/// Helper: declare linear parameters from an input clause expression.
///
/// Handles multiple Expr patterns where `linear` can appear:
/// - `Expr::Raw`: token sequences like `x : linear Int, y : Int`
/// - `Expr::Call`: `input(x as linear Int)` produces Call with Cast args
/// - `Expr::Cast`: single param `x as linear Int`
/// - `Expr::Block`/`Expr::Tuple`: sequences containing linear-annotated items
/// - `Expr::Paren`: unwrap and recurse
pub(crate) fn declare_linear_params_from_expr(
    expr: &Expr,
    tracker: &mut UsageTracker,
    span: &std::ops::Range<usize>,
) {
    match expr {
        Expr::Raw(tokens) => {
            declare_linear_params_from_raw(tokens, tracker, span);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                declare_linear_single_param(arg, tracker, span);
            }
        }
        Expr::Cast { expr: inner, ty } => {
            if ty.contains("linear")
                && let Expr::Ident(name) = inner.as_ref()
            {
                tracker.declare(name.clone(), UsageGrade::Linear, span.clone());
            }
        }
        Expr::Ident(_) => {
            // Single untyped param, no linear annotation possible
        }
        Expr::Paren(inner) => declare_linear_params_from_expr(inner, tracker, span),
        Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                declare_linear_single_param(item, tracker, span);
            }
        }
        _ => {}
    }
}

/// Declare a single input parameter as linear if it has a linear annotation.
fn declare_linear_single_param(
    expr: &Expr,
    tracker: &mut UsageTracker,
    span: &std::ops::Range<usize>,
) {
    match expr {
        Expr::Cast { expr: inner, ty } => {
            if ty.contains("linear")
                && let Expr::Ident(name) = inner.as_ref()
            {
                tracker.declare(name.clone(), UsageGrade::Linear, span.clone());
            }
        }
        Expr::Paren(inner) => declare_linear_single_param(inner, tracker, span),
        Expr::Raw(tokens) => {
            declare_linear_params_from_raw(tokens, tracker, span);
        }
        _ => {}
    }
}

/// Parse raw tokens for linear parameter declarations.
fn declare_linear_params_from_raw(
    tokens: &[String],
    tracker: &mut UsageTracker,
    span: &std::ops::Range<usize>,
) {
    let mut i = 0;
    while i < tokens.len() {
        // Look for pattern: name : linear Type  OR  name as linear Type
        let sep = tokens.get(i + 1).map(|s| s.as_str());
        if i + 2 < tokens.len()
            && matches!(sep, Some(":" | "as"))
            && tokens[i + 2..]
                .iter()
                .take_while(|t| *t != ",")
                .any(|t| t == "linear")
        {
            let name = &tokens[i];
            tracker.declare(name.clone(), UsageGrade::Linear, span.clone());
            // Skip to the next parameter (past comma)
            while i < tokens.len() && tokens[i] != "," {
                i += 1;
            }
        }
        i += 1;
    }
}

/// T034: Run typestate checks on services with `states:` declarations.
///
/// For each service with a States item, builds a TypestateChecker with
/// the declared states and validates transitions and operation ordering.
pub(crate) fn run_typestate_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut errors = Vec::new();
    for decl in &source.decls {
        if let Decl::Service(s) = &decl.node {
            // Find states declaration
            let states: Vec<String> = s
                .items
                .iter()
                .filter_map(|item| {
                    if let ServiceItem::States(s) = item {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .flatten()
                .collect();

            if states.is_empty() {
                continue;
            }

            // Build transitions from operation clauses
            let mut transitions = Vec::new();
            for item in &s.items {
                if let ServiceItem::Operation { name, clauses } = item {
                    for clause in clauses {
                        if let ClauseKind::Other(ref k) = clause.kind
                            && (k == "transition" || k == "from_state" || k == "to_state")
                            && let Expr::Raw(tokens) = &clause.body
                            && tokens.len() >= 3
                        {
                            transitions.push((name.clone(), tokens[0].clone(), tokens[2].clone()));
                        }
                    }
                }
            }

            if !transitions.is_empty() {
                let initial = states.first().cloned().unwrap_or_default();
                let mut checker =
                    TypestateChecker::new(states, transitions, initial, decl.span.clone());
                // Validate transitions reference valid states
                for tse in checker.validate_transitions() {
                    errors.push(TypeError {
                        code: tse.code,
                        message: tse.message,
                        span: tse.span,
                        secondary: None,
                    });
                }

                // Validate linearity: typestate variables must be linear
                let has_linear_annotation = s.items.iter().any(|item| {
                    if let ServiceItem::Operation { clauses, .. } = item {
                        clauses
                            .iter()
                            .any(|c| matches!(&c.kind, ClauseKind::Other(k) if k == "linear"))
                    } else {
                        false
                    }
                });
                if let Some(tse) = checker.validate_linear(has_linear_annotation) {
                    errors.push(TypeError {
                        code: tse.code,
                        message: tse.message,
                        span: tse.span,
                        secondary: None,
                    });
                }

                // Simulate transitions in operation order and check consistency
                let mut branch_checkers: Vec<TypestateChecker> = Vec::new();
                for item in &s.items {
                    if let ServiceItem::Operation { name, clauses } = item {
                        let pre_state = checker.current_state().to_string();
                        if let Err(tse) = checker.transition(name, decl.span.clone()) {
                            errors.push(TypeError {
                                code: tse.code,
                                message: tse.message,
                                span: tse.span,
                                secondary: None,
                            });
                        }

                        // Track variable usages in clause bodies
                        let mut usage_tracker = UsageTracker::new();
                        for clause in clauses {
                            expr_usages(&clause.body, &mut usage_tracker);
                        }

                        // Record checker state after each branch for consistency check
                        if !pre_state.is_empty() {
                            branch_checkers.push(TypestateChecker::new(
                                checker.states.clone(),
                                Vec::new(),
                                checker.current_state().to_string(),
                                decl.span.clone(),
                            ));
                        }
                    }
                }

                // Check branch consistency between sequential operations
                for pair in branch_checkers.windows(2) {
                    if let Some(tse) = TypestateChecker::check_branch_consistency(
                        &pair[0],
                        &pair[1],
                        decl.span.clone(),
                    ) {
                        errors.push(TypeError {
                            code: tse.code,
                            message: tse.message,
                            span: tse.span,
                            secondary: None,
                        });
                    }
                }
            }
        }
    }
    errors
}
