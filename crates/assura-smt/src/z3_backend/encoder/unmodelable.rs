//! Unmodelable-feature detection for SMT clauses.

use assura_ast::{Expr, SpExpr};

/// Returns `true` if the expression tree contains features that the SMT
/// encoder cannot faithfully represent (field-access chains on `self`,
/// typestate annotations, taint annotations, validate blocks, region
/// types, etc.).
pub(crate) fn expr_has_unmodelable_features(expr: &SpExpr) -> bool {
    match &expr.node {
        // #198: Field access is now always modelable. Deep field chains
        // are flattened into single Z3 variables, and self-rooted access
        // is treated the same as any other variable.
        Expr::Field(obj, _field) => expr_has_unmodelable_features(obj),
        // #201: Method calls are now always modelable. Unknown methods
        // are encoded as uninterpreted functions (sound overapproximation).
        Expr::MethodCall {
            receiver,
            method: _,
            args,
        } => {
            expr_has_unmodelable_features(receiver)
                || args.iter().any(expr_has_unmodelable_features)
        }
        // #200, #262: Raw tokens for taint, ghost, region, validate, and
        // typestate are now modelable. Ghost vars are regular Z3 vars,
        // taint levels are encoded as integers, regions as bounded
        // constraints, dotted field access is flattened, and typestate
        // `@` annotations are encoded as integer equality checks.
        Expr::Raw(_tokens) => false,
        Expr::BinOp { lhs, rhs, .. } => {
            expr_has_unmodelable_features(lhs) || expr_has_unmodelable_features(rhs)
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Old(inner)
        | Expr::Ghost(inner)
        | Expr::Cast { expr: inner, .. } => expr_has_unmodelable_features(inner),
        Expr::Call { func, args } => {
            expr_has_unmodelable_features(func) || args.iter().any(expr_has_unmodelable_features)
        }
        Expr::Index { expr: e, index } => {
            expr_has_unmodelable_features(e) || expr_has_unmodelable_features(index)
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            expr_has_unmodelable_features(domain) || expr_has_unmodelable_features(body)
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_has_unmodelable_features(cond)
                || expr_has_unmodelable_features(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| expr_has_unmodelable_features(e))
        }
        Expr::Let { value, body, .. } => {
            expr_has_unmodelable_features(value) || expr_has_unmodelable_features(body)
        }
        Expr::Match { scrutinee, arms } => {
            expr_has_unmodelable_features(scrutinee)
                || arms.iter().any(|a| expr_has_unmodelable_features(&a.body))
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            items.iter().any(expr_has_unmodelable_features)
        }
        Expr::Apply { args, .. } => args.iter().any(expr_has_unmodelable_features),
        Expr::Literal(_) | Expr::Ident(_) => false,
    }
}

pub(crate) fn is_self_rooted(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(name) => name == "self",
        Expr::Field(obj, _) => is_self_rooted(&obj.node),
        _ => false,
    }
}

/// Returns `true` if `expr` is a field access chain of depth >= 2
/// (e.g., `state.head.extra`). Single-level field access (`buf.len`)
/// is handled by the encoder, but deeper chains produce unconstrained
/// nested uninterpreted functions that Z3 finds trivial counterexamples for.
pub(crate) fn has_deep_field_chain(expr: &Expr) -> bool {
    field_chain_depth(expr) >= 2
}

pub(crate) fn field_chain_depth(expr: &Expr) -> usize {
    match expr {
        Expr::Field(obj, _) => 1 + field_chain_depth(&obj.node),
        _ => 0,
    }
}

/// Flatten a field chain like `state.head.extra.extra_max` into a single
/// Z3 variable name `state__head__extra__extra_max`. This avoids nested
/// uninterpreted functions that produce unconstrained counterexamples.
pub(crate) fn flatten_field_chain(expr: &Expr) -> String {
    match expr {
        Expr::Field(obj, field) => {
            let prefix = flatten_field_chain(&obj.node);
            format!("{prefix}__{field}")
        }
        Expr::Ident(name) => name.clone(),
        _ => format!("__obj_{:p}", expr as *const _),
    }
}

pub(crate) fn collect_unmodelable_reasons(_expr: &SpExpr) -> Vec<String> {
    // #198, #200, #201, #262: All expression types are now modelable.
    // Field access, method calls, raw tokens (including typestate @),
    // taint, ghost, region, and validate are all encoded in SMT.
    // This function returns an empty list but is kept for API stability.
    Vec::new()
}
