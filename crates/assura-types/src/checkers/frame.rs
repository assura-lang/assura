use super::*;

// Frame condition checking (T045 - CORE.3)
// ---------------------------------------------------------------------------

/// Extract the set of variable/field names from a `modifies` clause body.
///
/// The modifies clause body is typically:
/// - `Expr::Ident("x")` for a single variable
/// - `Expr::Block([Expr::Ident("x"), Expr::Ident("y")])` for multiple
/// - `Expr::Field(Expr::Ident("obj"), "field")` for `obj.field`
/// - `Expr::List([...])` for comma-separated list
///
/// Returns a set of string representations (e.g., `"x"`, `"node.keys"`).
pub(crate) fn extract_modifies_targets(expr: &Expr) -> Vec<std::string::String> {
    let mut targets = Vec::new();
    collect_modifies_targets(expr, &mut targets);
    targets
}

/// Recursively collect modifies targets from an expression.
fn collect_modifies_targets(expr: &Expr, targets: &mut Vec<std::string::String>) {
    match expr {
        Expr::Ident(name) => {
            targets.push(name.clone());
        }
        Expr::Field(receiver, field) => {
            // Build dotted path: "obj.field"
            let mut path = std::string::String::new();
            build_field_path(receiver, &mut path);
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(field);
            targets.push(path);
        }
        Expr::Block(exprs) => {
            for e in exprs {
                collect_modifies_targets(e, targets);
            }
        }
        Expr::List(items) => {
            for item in items {
                collect_modifies_targets(item, targets);
            }
        }
        Expr::Paren(inner) => {
            collect_modifies_targets(inner, targets);
        }
        Expr::Raw(tokens) => {
            // Parse comma-separated identifiers from raw tokens
            for tok in tokens {
                let trimmed = tok.trim();
                if !trimmed.is_empty() && trimmed != "," {
                    targets.push(trimmed.to_string());
                }
            }
        }
        // Other expression types are not valid modifies targets
        _ => {}
    }
}

/// Build a dotted field path from nested Field expressions.
fn build_field_path(expr: &Expr, path: &mut std::string::String) {
    match expr {
        Expr::Ident(name) => {
            path.push_str(name);
        }
        Expr::Field(receiver, field) => {
            build_field_path(receiver, path);
            path.push('.');
            path.push_str(field);
        }
        _ => {}
    }
}

/// Collect all variable names referenced via `old(expr)` in an expression.
///
/// Walks the expression tree and whenever it finds `Expr::Old(inner)`,
/// extracts the variable/field name from `inner`. This is used to find
/// which pre-state variables an `ensures` clause references.
pub(crate) fn collect_old_references(expr: &Expr) -> Vec<std::string::String> {
    let mut refs = Vec::new();
    collect_old_refs_inner(expr, &mut refs);
    refs
}

fn collect_old_refs_inner(expr: &Expr, refs: &mut Vec<std::string::String>) {
    match expr {
        Expr::Old(inner) => {
            // Extract the name from the inner expression
            match inner.as_ref() {
                Expr::Ident(name) => {
                    refs.push(name.clone());
                }
                Expr::Field(receiver, field) => {
                    let mut path = std::string::String::new();
                    build_field_path(receiver, &mut path);
                    if !path.is_empty() {
                        path.push('.');
                    }
                    path.push_str(field);
                    refs.push(path);
                }
                _ => {}
            }
            // Also recurse into the inner expression
            collect_old_refs_inner(inner, refs);
        }
        Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => {}
        Expr::Field(receiver, _) => collect_old_refs_inner(receiver, refs),
        Expr::MethodCall { receiver, args, .. } => {
            collect_old_refs_inner(receiver, refs);
            for arg in args {
                collect_old_refs_inner(arg, refs);
            }
        }
        Expr::Call { func, args } => {
            collect_old_refs_inner(func, refs);
            for arg in args {
                collect_old_refs_inner(arg, refs);
            }
        }
        Expr::Index { expr: base, index } => {
            collect_old_refs_inner(base, refs);
            collect_old_refs_inner(index, refs);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_old_refs_inner(lhs, refs);
            collect_old_refs_inner(rhs, refs);
        }
        Expr::UnaryOp { expr: inner, .. } => collect_old_refs_inner(inner, refs),
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_old_refs_inner(domain, refs);
            collect_old_refs_inner(body, refs);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_old_refs_inner(cond, refs);
            collect_old_refs_inner(then_branch, refs);
            if let Some(else_br) = else_branch {
                collect_old_refs_inner(else_br, refs);
            }
        }
        Expr::Paren(inner) => collect_old_refs_inner(inner, refs),
        Expr::List(items) => {
            for item in items {
                collect_old_refs_inner(item, refs);
            }
        }
        Expr::Cast { expr: inner, .. } => collect_old_refs_inner(inner, refs),
        Expr::Ghost(inner) => collect_old_refs_inner(inner, refs),
        Expr::Apply { args, .. } => {
            for arg in args {
                collect_old_refs_inner(arg, refs);
            }
        }
        Expr::Match { scrutinee, arms } => {
            collect_old_refs_inner(scrutinee, refs);
            for arm in arms {
                collect_old_refs_inner(&arm.body, refs);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_old_refs_inner(value, refs);
            collect_old_refs_inner(body, refs);
        }
        Expr::Block(exprs) => {
            for e in exprs {
                collect_old_refs_inner(e, refs);
            }
        }
        Expr::Tuple(elems) => {
            for e in elems {
                collect_old_refs_inner(e, refs);
            }
        }
    }
}

/// Collect all identifier names referenced in an expression (non-recursive
/// into old()).
///
/// Used to find which variables an ensures clause mentions so we can
/// determine which frame axioms to inject.
pub(crate) fn collect_ident_references(expr: &Expr) -> Vec<std::string::String> {
    let mut refs = Vec::new();
    collect_idents_inner(expr, &mut refs);
    refs
}

fn collect_idents_inner(expr: &Expr, refs: &mut Vec<std::string::String>) {
    match expr {
        Expr::Ident(name) => {
            if name != "true" && name != "false" && name != "result" && name != "self" {
                refs.push(name.clone());
            }
        }
        Expr::Literal(_) | Expr::Raw(_) => {}
        Expr::Old(inner) => collect_idents_inner(inner, refs),
        Expr::Field(receiver, field) => {
            let mut path = std::string::String::new();
            build_field_path(receiver, &mut path);
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(field);
            refs.push(path);
            collect_idents_inner(receiver, refs);
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_idents_inner(receiver, refs);
            for arg in args {
                collect_idents_inner(arg, refs);
            }
        }
        Expr::Call { func, args } => {
            collect_idents_inner(func, refs);
            for arg in args {
                collect_idents_inner(arg, refs);
            }
        }
        Expr::Index { expr: base, index } => {
            collect_idents_inner(base, refs);
            collect_idents_inner(index, refs);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_idents_inner(lhs, refs);
            collect_idents_inner(rhs, refs);
        }
        Expr::UnaryOp { expr: inner, .. } => collect_idents_inner(inner, refs),
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_idents_inner(domain, refs);
            collect_idents_inner(body, refs);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_idents_inner(cond, refs);
            collect_idents_inner(then_branch, refs);
            if let Some(else_br) = else_branch {
                collect_idents_inner(else_br, refs);
            }
        }
        Expr::Paren(inner) => collect_idents_inner(inner, refs),
        Expr::List(items) => {
            for item in items {
                collect_idents_inner(item, refs);
            }
        }
        Expr::Cast { expr: inner, .. } => collect_idents_inner(inner, refs),
        Expr::Ghost(inner) => collect_idents_inner(inner, refs),
        Expr::Apply { args, .. } => {
            for arg in args {
                collect_idents_inner(arg, refs);
            }
        }
        Expr::Match { scrutinee, arms } => {
            collect_idents_inner(scrutinee, refs);
            for arm in arms {
                collect_idents_inner(&arm.body, refs);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_idents_inner(value, refs);
            collect_idents_inner(body, refs);
        }
        Expr::Block(exprs) => {
            for e in exprs {
                collect_idents_inner(e, refs);
            }
        }
        Expr::Tuple(elems) => {
            for e in elems {
                collect_idents_inner(e, refs);
            }
        }
    }
}

// ---------------------------------------------------------------------------
