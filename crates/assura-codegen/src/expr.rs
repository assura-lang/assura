//! Expression-to-Rust code generation.
//!
//! Translates Assura AST expressions into Rust source code strings.

use super::*;

/// Heuristic: returns true if the expression is likely a numeric value
/// (variable, constant, literal, or arithmetic). Used to decide whether to
/// emit `i128::from(...)` casts for cross-width comparisons.
pub(crate) fn is_numeric_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(_) | Expr::Literal(Literal::Int(_)) | Expr::Literal(Literal::Float(_)) => true,
        Expr::Field(_, _) => true,
        Expr::BinOp { op, .. } => matches!(
            op,
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod
        ),
        Expr::UnaryOp {
            op: UnaryOp::Neg, ..
        } => true,
        Expr::Old(e) | Expr::Cast { expr: e, .. } => is_numeric_expr(e),
        Expr::Call { .. } | Expr::MethodCall { .. } | Expr::Index { .. } => true,
        Expr::Let { body, .. } => is_numeric_expr(body),
        Expr::If { then_branch, .. } => is_numeric_expr(then_branch),
        Expr::Match { arms, .. } => arms.first().is_some_and(|a| is_numeric_expr(&a.body)),
        // These are definitively not numeric expressions
        Expr::Literal(Literal::Str(_) | Literal::Bool(_))
        | Expr::UnaryOp {
            op: UnaryOp::Not, ..
        }
        | Expr::Forall { .. }
        | Expr::Exists { .. }
        | Expr::List(_)
        | Expr::Tuple(_)
        | Expr::Ghost(_)
        | Expr::Apply { .. }
        | Expr::Block(_)
        | Expr::Raw(_) => false,
    }
}

/// Resolve an ordering clause body to a Rust `std::sync::atomic::Ordering` variant name.
pub(crate) fn resolve_ordering_variant(body: &Expr) -> Option<&'static str> {
    use assura_parser::ast::MemoryOrdering;
    let s = match body {
        Expr::Ident(s) => s.as_str(),
        Expr::Raw(tokens) => {
            return tokens
                .iter()
                .find_map(|t| MemoryOrdering::parse(t))
                .map(|o| o.to_rust_ordering());
        }
        _ => return None,
    };
    MemoryOrdering::parse(s).map(|o| o.to_rust_ordering())
}

/// Convert an Assura `Expr` to a Rust expression string.
pub(crate) fn expr_to_rust(expr: &Expr) -> String {
    match expr {
        Expr::Literal(lit) => match lit {
            Literal::Int(s) | Literal::Float(s) => s.clone(),
            Literal::Str(s) => format!("\"{s}\""),
            Literal::Bool(b) => b.to_string(),
        },
        Expr::Ident(s) => {
            if s == "result" {
                "__result".to_string()
            } else {
                s.clone()
            }
        }
        Expr::Field(recv, field) => format!("{}.{field}", expr_to_rust(recv)),
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let args_s: Vec<String> = args.iter().map(expr_to_rust).collect();
            format!("{}.{method}({})", expr_to_rust(receiver), args_s.join(", "))
        }
        Expr::Call { func, args } => {
            let args_s: Vec<String> = args.iter().map(expr_to_rust).collect();
            format!("{}({})", expr_to_rust(func), args_s.join(", "))
        }
        Expr::Index { expr: e, index } => {
            format!("{}[{}]", expr_to_rust(e), expr_to_rust(index))
        }
        Expr::BinOp { lhs, op, rhs } => {
            // For ordering comparisons, cast both sides to i128 to avoid
            // type mismatch between different integer widths (e.g., u16 vs u64).
            // This mirrors Assura's abstract numeric semantics.
            // We only do this for ordering (< <= > >=), not equality (== !=),
            // because equality works via PartialEq and wrapping in i128::from
            // would fail on non-numeric types.
            let is_numeric_cmp = matches!(op, BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte)
                && is_numeric_expr(lhs)
                && is_numeric_expr(rhs);

            let op_s = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Mod => "%",
                BinOp::Eq => "==",
                BinOp::Neq => "!=",
                BinOp::Lt => "<",
                BinOp::Lte => "<=",
                BinOp::Gt => ">",
                BinOp::Gte => ">=",
                BinOp::And => "&&",
                BinOp::Or => "||",
                BinOp::Implies => {
                    return format!("(!{} || {})", expr_to_rust(lhs), expr_to_rust(rhs));
                }
                BinOp::In => {
                    // `x in S` means S.contains(&x)
                    return format!("{}.contains(&{})", expr_to_rust(rhs), expr_to_rust(lhs));
                }
                BinOp::NotIn => {
                    return format!("!{}.contains(&{})", expr_to_rust(rhs), expr_to_rust(lhs));
                }
                BinOp::Concat => {
                    return format!("[{}, {}].concat()", expr_to_rust(lhs), expr_to_rust(rhs));
                }
                BinOp::Range => "..",
            };
            if is_numeric_cmp {
                format!(
                    "(i128::from({}) {op_s} i128::from({}))",
                    expr_to_rust(lhs),
                    expr_to_rust(rhs)
                )
            } else {
                format!("({} {op_s} {})", expr_to_rust(lhs), expr_to_rust(rhs))
            }
        }
        Expr::UnaryOp { op, expr: e } => {
            let op_s = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
            };
            format!("({op_s}{})", expr_to_rust(e))
        }
        Expr::Old(e) => {
            // old(expr) references a pre-state snapshot saved at function entry.
            // The variable name is derived from the inner expression.
            format!("__old_{}", old_var_name(e))
        }
        Expr::Forall { var, domain, body } => {
            format!(
                "{}.iter().all(|{var}| {})",
                expr_to_rust(domain),
                expr_to_rust(body)
            )
        }
        Expr::Exists { var, domain, body } => {
            format!(
                "{}.iter().any(|{var}| {})",
                expr_to_rust(domain),
                expr_to_rust(body)
            )
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => match else_branch {
            Some(eb) => format!(
                "if {} {{ {} }} else {{ {} }}",
                expr_to_rust(cond),
                expr_to_rust(then_branch),
                expr_to_rust(eb)
            ),
            None => format!(
                "if {} {{ {} }}",
                expr_to_rust(cond),
                expr_to_rust(then_branch)
            ),
        },
        Expr::List(items) => {
            let elems: Vec<String> = items.iter().map(expr_to_rust).collect();
            format!("vec![{}]", elems.join(", "))
        }
        Expr::Cast { expr: e, ty } => {
            format!("({} as {})", expr_to_rust(e), map_type_token(ty))
        }
        Expr::Block(exprs) => {
            let strs: Vec<String> = exprs.iter().map(expr_to_rust).collect();
            strs.join(" ")
        }
        Expr::Ghost(_inner) => {
            // Ghost blocks are erased at runtime; emit nothing.
            "/* ghost erased */()".to_string()
        }
        Expr::Apply { lemma_name, .. } => {
            // Lemma applications are erased at runtime; emit comment.
            format!("/* lemma {lemma_name} applied */")
        }
        Expr::Match { scrutinee, arms } => {
            let scrut = expr_to_rust(scrutinee);
            let arms_code: Vec<String> = arms
                .iter()
                .map(|arm| {
                    let pat = match &arm.pattern {
                        assura_parser::ast::Pattern::Ident(name) => name.clone(),
                        assura_parser::ast::Pattern::Wildcard => "_".into(),
                        assura_parser::ast::Pattern::Literal(lit) => match lit {
                            Literal::Int(s) | Literal::Float(s) => s.clone(),
                            Literal::Str(s) => format!("\"{s}\""),
                            Literal::Bool(b) => b.to_string(),
                        },
                        assura_parser::ast::Pattern::Constructor { name, fields } => {
                            if fields.is_empty() {
                                name.clone()
                            } else {
                                let fs: Vec<String> = fields.iter().map(pattern_to_rust).collect();
                                format!("{name}({})", fs.join(", "))
                            }
                        }
                        assura_parser::ast::Pattern::Tuple(pats) => {
                            let ps: Vec<String> = pats.iter().map(pattern_to_rust).collect();
                            format!("({})", ps.join(", "))
                        }
                    };
                    let body = expr_to_rust(&arm.body);
                    format!("    {pat} => {body},")
                })
                .collect();
            // Add wildcard fallback if no arm is a catch-all
            let has_wildcard = arms.iter().any(|arm| {
                matches!(
                    &arm.pattern,
                    assura_parser::ast::Pattern::Wildcard | assura_parser::ast::Pattern::Ident(_)
                )
            });
            if !has_wildcard {
                let mut all_arms = arms_code;
                all_arms.push("    _ => unreachable!(\"non-exhaustive match\"),".to_string());
                format!("match {} {{\n{}\n}}", scrut, all_arms.join("\n"))
            } else {
                format!("match {} {{\n{}\n}}", scrut, arms_code.join("\n"))
            }
        }
        Expr::Let { name, value, body } => {
            format!(
                "{{ let {} = {}; {} }}",
                name,
                expr_to_rust(value),
                expr_to_rust(body)
            )
        }
        Expr::Tuple(elems) => {
            let items: Vec<String> = elems.iter().map(expr_to_rust).collect();
            format!("({})", items.join(", "))
        }
        Expr::Raw(tokens) => raw_tokens_to_rust(tokens),
    }
}

/// Convert raw token sequences to Rust, handling quantifier patterns.
///
/// Detects `forall var in domain: body` and `exists var in domain: body`
/// in raw tokens and translates them to `.iter().all(|var| body)` /
/// `.iter().any(|var| body)` respectively. Falls back to joined tokens
/// for non-quantifier sequences.
pub(crate) fn raw_tokens_to_rust(tokens: &[String]) -> String {
    if tokens.is_empty() {
        return String::new();
    }
    // Detect: forall/exists VAR in DOMAIN : BODY
    let first = tokens[0].as_str();
    if matches!(first, "forall" | "exists")
        && tokens.len() >= 5
        && let Some(in_pos) = tokens[1..].iter().position(|t| t == "in")
    {
        let in_pos = in_pos + 1; // offset from tokens[0]
        let var = &tokens[1..in_pos].join("_");
        // Find the colon that separates domain from body
        if let Some(colon_offset) = tokens[in_pos + 1..].iter().position(|t| t == ":") {
            let colon_pos = in_pos + 1 + colon_offset;
            let domain_tokens = &tokens[in_pos + 1..colon_pos];
            let body_tokens = &tokens[colon_pos + 1..];

            let domain = {
                let mapped: Vec<&str> = domain_tokens.iter().map(|t| map_type_token(t)).collect();
                smart_join_type_tokens(&mapped)
            };
            let body = raw_tokens_to_rust(body_tokens);

            let method = if first == "forall" { "all" } else { "any" };
            return format!("{domain}.iter().{method}(|{var}| {body})");
        }
    }

    // Strip typestate annotations: `expr @ State` -> `true /* typestate: expr @ State */`
    if let Some(at_pos) = tokens.iter().position(|t| t == "@") {
        let before = &tokens[..at_pos];
        let after = &tokens[at_pos + 1..];
        let expr_s = raw_tokens_to_rust(before);
        let state_s = after.join(" ");
        return format!("true /* typestate: {expr_s} @ {state_s} */");
    }

    // Check for `result` keyword — replace with `__result`
    let mapped: Vec<String> = tokens
        .iter()
        .map(|t| {
            if t == "result" {
                "__result".to_string()
            } else {
                map_type_token(t).to_string()
            }
        })
        .collect();
    let refs: Vec<&str> = mapped.iter().map(|s| s.as_str()).collect();
    smart_join_type_tokens(&refs)
}

// ---------------------------------------------------------------------------
// old(expr) support
// ---------------------------------------------------------------------------

/// Derive a variable name for an `old(expr)` snapshot from the expression.
/// E.g., `old(x)` -> `__old_x`, `old(buf.len)` -> `__old_buf_len`.
/// Generate a debug_assert! that handles multi-line expressions.
///
/// If the expression contains newlines (e.g. a match block), wraps it in a
/// block `{ ... }` so the assert is valid Rust syntax.
///
/// If the expression contains patterns that would fail on stub types
/// (nested field accesses like `a.b.c`), emit it as a comment instead
/// to keep the generated code compilable while preserving the contract intent.
pub(crate) fn generate_debug_assert(code: &mut String, expr: &str, label: &str) {
    // If expression references deep field chains (e.g., state.head.extra.extra_max),
    // emit as a comment since stub types don't have these fields.
    if has_deep_field_access(expr) {
        code.push_str(&format!("    // {label}: {}\n", expr.replace('"', "\\\"")));
        return;
    }
    if expr.contains('\n') {
        // Multi-line expressions (match, etc.) need a block wrapper
        let msg = expr.replace('\n', " ").replace('"', "\\\"");
        code.push_str(&format!(
            "    debug_assert!({{ {expr} }}, \"{label}: {msg}\");\n"
        ));
    } else {
        code.push_str(&format!(
            "    debug_assert!({expr}, \"{label}: {}\");\n",
            expr.replace('"', "\\\"")
        ));
    }
}

/// Check if an expression string contains patterns that would fail to compile
/// against placeholder stub types:
/// - Any field access (a.b) since stub types have no fields
/// - Method calls on unknown objects
/// - References to `__result.field`
pub(crate) fn has_deep_field_access(expr: &str) -> bool {
    // Detect struct field access like `state.head.extra` that would fail on stub types.
    // Exclude method-call chains like `.iter().all()`, `.len()`, `.clone()` which are
    // standard library methods and work fine.
    let method_names = [
        "iter",
        "all",
        "any",
        "map",
        "filter",
        "len",
        "is_empty",
        "clone",
        "count",
        "sum",
        "collect",
        "flat_map",
        "zip",
        "enumerate",
        "take",
        "skip",
        "find",
        "fold",
        "for_each",
        "min",
        "max",
        "contains",
        "position",
        "into_iter",
        "as_ref",
        "as_mut",
        "unwrap",
        "unwrap_or",
        "expect",
        "ok",
        "err",
        "is_some",
        "is_none",
        "is_ok",
        "is_err",
    ];
    for word in expr.split(|c: char| !c.is_alphanumeric() && c != '.' && c != '_') {
        if word.contains('.') && !word.is_empty() {
            let parts: Vec<&str> = word.split('.').collect();
            if parts.len() >= 2
                && parts[0]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
            {
                // Check if ALL dotted segments after the first are known method names
                let all_methods = parts[1..].iter().all(|p| method_names.contains(p));
                if !all_methods {
                    return true;
                }
            }
        }
    }
    // __result.field references (but not __result.iter(), etc.)
    if expr.contains("__result.") {
        // Check if all occurrences of __result. are followed by method calls
        for chunk in expr.split("__result.") {
            if chunk.is_empty() {
                continue;
            }
            let after: String = chunk
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !after.is_empty() && !method_names.contains(&after.as_str()) {
                return true;
            }
        }
    }
    false
}

/// Like `generate_debug_assert` but with configurable indent level.
pub(crate) fn generate_debug_assert_indented(
    code: &mut String,
    expr: &str,
    label: &str,
    indent: usize,
) {
    let pad = "    ".repeat(indent);
    if has_deep_field_access(expr) {
        code.push_str(&format!("{pad}// {label}: {}\n", expr.replace('"', "\\\"")));
        return;
    }
    if expr.contains('\n') {
        let msg = expr.replace('\n', " ").replace('"', "\\\"");
        code.push_str(&format!(
            "{pad}debug_assert!({{ {expr} }}, \"{label}: {msg}\");\n"
        ));
    } else {
        code.push_str(&format!(
            "{pad}debug_assert!({expr}, \"{label}: {}\");\n",
            expr.replace('"', "\\\"")
        ));
    }
}

/// Convert a pattern to Rust pattern syntax.
pub(crate) fn pattern_to_rust(pat: &assura_parser::ast::Pattern) -> String {
    match pat {
        assura_parser::ast::Pattern::Ident(name) => name.clone(),
        assura_parser::ast::Pattern::Wildcard => "_".into(),
        assura_parser::ast::Pattern::Literal(lit) => match lit {
            Literal::Int(s) | Literal::Float(s) => s.clone(),
            Literal::Str(s) => format!("\"{s}\""),
            Literal::Bool(b) => b.to_string(),
        },
        assura_parser::ast::Pattern::Constructor { name, fields } => {
            if fields.is_empty() {
                name.clone()
            } else {
                let fs: Vec<String> = fields.iter().map(pattern_to_rust).collect();
                format!("{name}({})", fs.join(", "))
            }
        }
        assura_parser::ast::Pattern::Tuple(pats) => {
            let ps: Vec<String> = pats.iter().map(pattern_to_rust).collect();
            format!("({})", ps.join(", "))
        }
    }
}

pub(crate) fn old_var_name(expr: &Expr) -> String {
    match expr {
        Expr::Ident(s) => s.clone(),
        Expr::Field(recv, field) => format!("{}_{field}", old_var_name(recv)),
        Expr::Call { func, .. } => old_var_name(func),
        Expr::MethodCall {
            receiver, method, ..
        } => format!("{}_{method}", old_var_name(receiver)),
        Expr::Index { expr: e, .. } => format!("{}_idx", old_var_name(e)),
        Expr::Literal(lit) => match lit {
            Literal::Int(s) | Literal::Float(s) => format!("lit_{s}"),
            Literal::Str(s) => format!("lit_{}", s.trim_matches('"')),
            Literal::Bool(b) => format!("lit_{b}"),
        },
        Expr::BinOp { lhs, op, rhs } => {
            let op_name = match op {
                BinOp::Add => "add",
                BinOp::Sub => "sub",
                BinOp::Mul => "mul",
                BinOp::Div => "div",
                BinOp::Mod => "mod",
                BinOp::And => "and",
                BinOp::Or => "or",
                BinOp::Eq => "eq",
                BinOp::Neq => "neq",
                BinOp::Lt => "lt",
                BinOp::Gt => "gt",
                BinOp::Lte => "lte",
                BinOp::Gte => "gte",
                BinOp::Implies => "implies",
                BinOp::In => "in",
                BinOp::NotIn => "notin",
                BinOp::Concat => "concat",
                BinOp::Range => "range",
            };
            format!("{}_{op_name}_{}", old_var_name(lhs), old_var_name(rhs))
        }
        Expr::UnaryOp { op, expr: e } => {
            let prefix = match op {
                UnaryOp::Neg => "neg",
                UnaryOp::Not => "not",
            };
            format!("{prefix}_{}", old_var_name(e))
        }
        Expr::Old(inner) => old_var_name(inner),
        Expr::Cast { expr: e, .. } => old_var_name(e),
        Expr::Ghost(inner) => format!("ghost_{}", old_var_name(inner)),
        Expr::Forall { var, .. } => format!("forall_{var}"),
        Expr::Exists { var, .. } => format!("exists_{var}"),
        Expr::If { cond, .. } => format!("if_{}", old_var_name(cond)),
        Expr::Let { name, .. } => format!("let_{name}"),
        Expr::Match { scrutinee, .. } => format!("match_{}", old_var_name(scrutinee)),
        Expr::Apply { lemma_name, .. } => format!("apply_{lemma_name}"),
        Expr::List(_) => "list".to_string(),
        Expr::Tuple(_) => "tuple".to_string(),
        Expr::Block(exprs) => {
            if let Some(first) = exprs.first() {
                old_var_name(first)
            } else {
                "block".to_string()
            }
        }
        Expr::Raw(tokens) => {
            if let Some(first) = tokens.first() {
                first.clone()
            } else {
                "raw".to_string()
            }
        }
    }
}

/// Walk an expression tree and collect all `old(inner)` sub-expressions.
/// Returns `(var_name, rust_expr)` pairs for generating pre-state snapshots.
pub(crate) fn collect_old_exprs(expr: &Expr) -> Vec<(String, String)> {
    let mut result = Vec::new();
    collect_old_exprs_inner(expr, &mut result);
    result
}

pub(crate) fn collect_old_exprs_inner(expr: &Expr, out: &mut Vec<(String, String)>) {
    match expr {
        Expr::Old(inner) => {
            let var = old_var_name(inner);
            let rust = expr_to_rust(inner);
            // Avoid duplicates
            if !out.iter().any(|(v, _)| v == &var) {
                out.push((var, rust));
            }
            // Also recurse into the inner expression (in case of nested old)
            collect_old_exprs_inner(inner, out);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_old_exprs_inner(lhs, out);
            collect_old_exprs_inner(rhs, out);
        }
        Expr::UnaryOp { expr: e, .. }
        | Expr::Field(e, _)
        | Expr::Cast { expr: e, .. } => {
            collect_old_exprs_inner(e, out);
        }
        Expr::Call { func, args } => {
            collect_old_exprs_inner(func, out);
            for a in args {
                collect_old_exprs_inner(a, out);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_old_exprs_inner(receiver, out);
            for a in args {
                collect_old_exprs_inner(a, out);
            }
        }
        Expr::Index { expr: e, index } => {
            collect_old_exprs_inner(e, out);
            collect_old_exprs_inner(index, out);
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_old_exprs_inner(domain, out);
            collect_old_exprs_inner(body, out);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_old_exprs_inner(cond, out);
            collect_old_exprs_inner(then_branch, out);
            if let Some(eb) = else_branch {
                collect_old_exprs_inner(eb, out);
            }
        }
        Expr::List(items) | Expr::Block(items) => {
            for item in items {
                collect_old_exprs_inner(item, out);
            }
        }
        Expr::Ghost(inner) => {
            // Ghost blocks are erased but may reference old() in
            // their verification expressions.
            collect_old_exprs_inner(inner, out);
        }
        Expr::Apply { args, .. } => {
            // Apply is erased but may reference old() in arguments.
            for a in args {
                collect_old_exprs_inner(a, out);
            }
        }
        Expr::Match { scrutinee, arms } => {
            collect_old_exprs_inner(scrutinee, out);
            for arm in arms {
                collect_old_exprs_inner(&arm.body, out);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_old_exprs_inner(value, out);
            collect_old_exprs_inner(body, out);
        }
        Expr::Tuple(elems) => {
            for e in elems {
                collect_old_exprs_inner(e, out);
            }
        }
        // Leaf nodes: no old() inside
        Expr::Literal(_) | Expr::Ident(_) | Expr::Raw(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- is_numeric_expr ----

    #[test]
    fn is_numeric_ident() {
        assert!(is_numeric_expr(&Expr::Ident("x".into())));
    }

    #[test]
    fn is_numeric_int_literal() {
        assert!(is_numeric_expr(&Expr::Literal(Literal::Int("42".into()))));
    }

    #[test]
    fn is_numeric_float_literal() {
        assert!(is_numeric_expr(&Expr::Literal(Literal::Float(
            "3.14".into()
        ))));
    }

    #[test]
    fn is_not_numeric_str_literal() {
        assert!(!is_numeric_expr(&Expr::Literal(Literal::Str(
            "hello".into()
        ))));
    }

    #[test]
    fn is_not_numeric_bool_literal() {
        assert!(!is_numeric_expr(&Expr::Literal(Literal::Bool(true))));
    }

    #[test]
    fn is_numeric_binop_add() {
        let e = Expr::BinOp {
            lhs: Box::new(Expr::Ident("a".into())),
            op: BinOp::Add,
            rhs: Box::new(Expr::Ident("b".into())),
        };
        assert!(is_numeric_expr(&e));
    }

    #[test]
    fn is_not_numeric_binop_and() {
        let e = Expr::BinOp {
            lhs: Box::new(Expr::Literal(Literal::Bool(true))),
            op: BinOp::And,
            rhs: Box::new(Expr::Literal(Literal::Bool(false))),
        };
        assert!(!is_numeric_expr(&e));
    }

    #[test]
    fn is_numeric_neg() {
        let e = Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: Box::new(Expr::Ident("x".into())),
        };
        assert!(is_numeric_expr(&e));
    }

    #[test]
    fn is_not_numeric_not() {
        let e = Expr::UnaryOp {
            op: UnaryOp::Not,
            expr: Box::new(Expr::Literal(Literal::Bool(true))),
        };
        assert!(!is_numeric_expr(&e));
    }

    #[test]
    fn is_numeric_old() {
        let e = Expr::Old(Box::new(Expr::Ident("x".into())));
        assert!(is_numeric_expr(&e));
    }

    #[test]
    fn is_numeric_field() {
        let e = Expr::Field(Box::new(Expr::Ident("s".into())), "len".into());
        assert!(is_numeric_expr(&e));
    }

    #[test]
    fn is_not_numeric_forall() {
        let e = Expr::Forall {
            var: "x".into(),
            domain: Box::new(Expr::Ident("xs".into())),
            body: Box::new(Expr::Literal(Literal::Bool(true))),
        };
        assert!(!is_numeric_expr(&e));
    }

    // ---- expr_to_rust ----

    #[test]
    fn expr_to_rust_int_literal() {
        assert_eq!(
            expr_to_rust(&Expr::Literal(Literal::Int("42".into()))),
            "42"
        );
    }

    #[test]
    fn expr_to_rust_str_literal() {
        assert_eq!(
            expr_to_rust(&Expr::Literal(Literal::Str("hello".into()))),
            "\"hello\""
        );
    }

    #[test]
    fn expr_to_rust_bool_literal() {
        assert_eq!(expr_to_rust(&Expr::Literal(Literal::Bool(true))), "true");
    }

    #[test]
    fn expr_to_rust_result_ident() {
        assert_eq!(expr_to_rust(&Expr::Ident("result".into())), "__result");
    }

    #[test]
    fn expr_to_rust_normal_ident() {
        assert_eq!(expr_to_rust(&Expr::Ident("x".into())), "x");
    }

    #[test]
    fn expr_to_rust_field() {
        let e = Expr::Field(Box::new(Expr::Ident("s".into())), "len".into());
        assert_eq!(expr_to_rust(&e), "s.len");
    }

    #[test]
    fn expr_to_rust_method_call() {
        let e = Expr::MethodCall {
            receiver: Box::new(Expr::Ident("v".into())),
            method: "push".into(),
            args: vec![Expr::Literal(Literal::Int("1".into()))],
        };
        assert_eq!(expr_to_rust(&e), "v.push(1)");
    }

    #[test]
    fn expr_to_rust_call() {
        let e = Expr::Call {
            func: Box::new(Expr::Ident("foo".into())),
            args: vec![Expr::Ident("a".into()), Expr::Ident("b".into())],
        };
        assert_eq!(expr_to_rust(&e), "foo(a, b)");
    }

    #[test]
    fn expr_to_rust_index() {
        let e = Expr::Index {
            expr: Box::new(Expr::Ident("arr".into())),
            index: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        assert_eq!(expr_to_rust(&e), "arr[0]");
    }

    #[test]
    fn expr_to_rust_binop_add() {
        let e = Expr::BinOp {
            lhs: Box::new(Expr::Ident("a".into())),
            op: BinOp::Add,
            rhs: Box::new(Expr::Ident("b".into())),
        };
        assert_eq!(expr_to_rust(&e), "(a + b)");
    }

    #[test]
    fn expr_to_rust_implies() {
        let e = Expr::BinOp {
            lhs: Box::new(Expr::Ident("p".into())),
            op: BinOp::Implies,
            rhs: Box::new(Expr::Ident("q".into())),
        };
        assert_eq!(expr_to_rust(&e), "(!p || q)");
    }

    #[test]
    fn expr_to_rust_in_operator() {
        let e = Expr::BinOp {
            lhs: Box::new(Expr::Ident("x".into())),
            op: BinOp::In,
            rhs: Box::new(Expr::Ident("s".into())),
        };
        assert_eq!(expr_to_rust(&e), "s.contains(&x)");
    }

    #[test]
    fn expr_to_rust_notin_operator() {
        let e = Expr::BinOp {
            lhs: Box::new(Expr::Ident("x".into())),
            op: BinOp::NotIn,
            rhs: Box::new(Expr::Ident("s".into())),
        };
        assert_eq!(expr_to_rust(&e), "!s.contains(&x)");
    }

    #[test]
    fn expr_to_rust_concat() {
        let e = Expr::BinOp {
            lhs: Box::new(Expr::Ident("a".into())),
            op: BinOp::Concat,
            rhs: Box::new(Expr::Ident("b".into())),
        };
        assert_eq!(expr_to_rust(&e), "[a, b].concat()");
    }

    #[test]
    fn expr_to_rust_numeric_cmp_casts_i128() {
        let e = Expr::BinOp {
            lhs: Box::new(Expr::Ident("x".into())),
            op: BinOp::Lt,
            rhs: Box::new(Expr::Ident("y".into())),
        };
        assert_eq!(expr_to_rust(&e), "(i128::from(x) < i128::from(y))");
    }

    #[test]
    fn expr_to_rust_eq_no_cast() {
        // Equality does not cast to i128
        let e = Expr::BinOp {
            lhs: Box::new(Expr::Ident("x".into())),
            op: BinOp::Eq,
            rhs: Box::new(Expr::Ident("y".into())),
        };
        assert_eq!(expr_to_rust(&e), "(x == y)");
    }

    #[test]
    fn expr_to_rust_unary_neg() {
        let e = Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: Box::new(Expr::Ident("x".into())),
        };
        assert_eq!(expr_to_rust(&e), "(-x)");
    }

    #[test]
    fn expr_to_rust_unary_not() {
        let e = Expr::UnaryOp {
            op: UnaryOp::Not,
            expr: Box::new(Expr::Ident("x".into())),
        };
        assert_eq!(expr_to_rust(&e), "(!x)");
    }

    #[test]
    fn expr_to_rust_old() {
        let e = Expr::Old(Box::new(Expr::Ident("x".into())));
        assert_eq!(expr_to_rust(&e), "__old_x");
    }

    #[test]
    fn expr_to_rust_forall() {
        let e = Expr::Forall {
            var: "x".into(),
            domain: Box::new(Expr::Ident("xs".into())),
            body: Box::new(Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            }),
        };
        let result = expr_to_rust(&e);
        assert!(result.contains("iter().all(|x|"));
    }

    #[test]
    fn expr_to_rust_exists() {
        let e = Expr::Exists {
            var: "x".into(),
            domain: Box::new(Expr::Ident("xs".into())),
            body: Box::new(Expr::Ident("x".into())),
        };
        assert!(expr_to_rust(&e).contains("iter().any(|x|"));
    }

    #[test]
    fn expr_to_rust_if_else() {
        let e = Expr::If {
            cond: Box::new(Expr::Ident("c".into())),
            then_branch: Box::new(Expr::Literal(Literal::Int("1".into()))),
            else_branch: Some(Box::new(Expr::Literal(Literal::Int("2".into())))),
        };
        assert_eq!(expr_to_rust(&e), "if c { 1 } else { 2 }");
    }

    #[test]
    fn expr_to_rust_if_no_else() {
        let e = Expr::If {
            cond: Box::new(Expr::Ident("c".into())),
            then_branch: Box::new(Expr::Literal(Literal::Int("1".into()))),
            else_branch: None,
        };
        assert_eq!(expr_to_rust(&e), "if c { 1 }");
    }

    #[test]
    fn expr_to_rust_list() {
        let e = Expr::List(vec![
            Expr::Literal(Literal::Int("1".into())),
            Expr::Literal(Literal::Int("2".into())),
        ]);
        assert_eq!(expr_to_rust(&e), "vec![1, 2]");
    }

    #[test]
    fn expr_to_rust_cast() {
        let e = Expr::Cast {
            expr: Box::new(Expr::Ident("x".into())),
            ty: "u32".into(),
        };
        assert_eq!(expr_to_rust(&e), "(x as u32)");
    }

    #[test]
    fn expr_to_rust_ghost_erased() {
        let e = Expr::Ghost(Box::new(Expr::Ident("x".into())));
        assert_eq!(expr_to_rust(&e), "/* ghost erased */()");
    }

    #[test]
    fn expr_to_rust_apply_erased() {
        let e = Expr::Apply {
            lemma_name: "L1".into(),
            args: vec![],
        };
        assert_eq!(expr_to_rust(&e), "/* lemma L1 applied */");
    }

    #[test]
    fn expr_to_rust_let_binding() {
        let e = Expr::Let {
            name: "v".into(),
            value: Box::new(Expr::Literal(Literal::Int("5".into()))),
            body: Box::new(Expr::Ident("v".into())),
        };
        assert_eq!(expr_to_rust(&e), "{ let v = 5; v }");
    }

    #[test]
    fn expr_to_rust_tuple() {
        let e = Expr::Tuple(vec![
            Expr::Literal(Literal::Int("1".into())),
            Expr::Literal(Literal::Int("2".into())),
        ]);
        assert_eq!(expr_to_rust(&e), "(1, 2)");
    }

    #[test]
    fn expr_to_rust_match_with_wildcard_fallback() {
        use assura_parser::ast::{MatchArm, Pattern};
        let e = Expr::Match {
            scrutinee: Box::new(Expr::Ident("x".into())),
            arms: vec![MatchArm {
                pattern: Pattern::Constructor {
                    name: "Some".into(),
                    fields: vec![Pattern::Ident("v".into())],
                },
                body: Expr::Ident("v".into()),
            }],
        };
        let result = expr_to_rust(&e);
        assert!(result.contains("match x"));
        assert!(result.contains("Some(v) => v,"));
        assert!(result.contains("_ => unreachable!"));
    }

    #[test]
    fn expr_to_rust_match_has_wildcard() {
        use assura_parser::ast::{MatchArm, Pattern};
        let e = Expr::Match {
            scrutinee: Box::new(Expr::Ident("x".into())),
            arms: vec![
                MatchArm {
                    pattern: Pattern::Literal(Literal::Int("1".into())),
                    body: Expr::Ident("a".into()),
                },
                MatchArm {
                    pattern: Pattern::Wildcard,
                    body: Expr::Ident("b".into()),
                },
            ],
        };
        let result = expr_to_rust(&e);
        assert!(result.contains("_ => b,"));
        assert!(!result.contains("unreachable!"));
    }

    // ---- raw_tokens_to_rust ----

    #[test]
    fn raw_tokens_empty() {
        assert_eq!(raw_tokens_to_rust(&[]), "");
    }

    #[test]
    fn raw_tokens_forall_quantifier() {
        let tokens: Vec<String> = vec!["forall", "x", "in", "items", ":", "x"]
            .into_iter()
            .map(String::from)
            .collect();
        let result = raw_tokens_to_rust(&tokens);
        assert!(result.contains(".iter().all(|x|"), "got: {result}");
    }

    #[test]
    fn raw_tokens_exists_quantifier() {
        let tokens: Vec<String> = vec!["exists", "x", "in", "items", ":", "x"]
            .into_iter()
            .map(String::from)
            .collect();
        let result = raw_tokens_to_rust(&tokens);
        assert!(result.contains(".iter().any(|x|"), "got: {result}");
    }

    #[test]
    fn raw_tokens_typestate_annotation() {
        let tokens: Vec<String> = vec!["conn", "@", "Connected"]
            .into_iter()
            .map(String::from)
            .collect();
        let result = raw_tokens_to_rust(&tokens);
        assert!(result.starts_with("true /* typestate:"), "got: {result}");
        assert!(result.contains("Connected"));
    }

    #[test]
    fn raw_tokens_result_replacement() {
        let tokens: Vec<String> = vec!["result"].into_iter().map(String::from).collect();
        assert_eq!(raw_tokens_to_rust(&tokens), "__result");
    }

    // ---- has_deep_field_access ----

    #[test]
    fn no_deep_field_plain() {
        assert!(!has_deep_field_access("x > 0"));
    }

    #[test]
    fn has_deep_field_struct() {
        assert!(has_deep_field_access("state.head.extra"));
    }

    #[test]
    fn no_deep_field_method_chain() {
        assert!(!has_deep_field_access("v.iter().all()"));
    }

    #[test]
    fn has_deep_field_result() {
        assert!(has_deep_field_access("__result.value"));
    }

    #[test]
    fn no_deep_field_result_method() {
        assert!(!has_deep_field_access("__result.is_some()"));
    }

    // ---- generate_debug_assert ----

    #[test]
    fn debug_assert_simple() {
        let mut code = String::new();
        generate_debug_assert(&mut code, "x > 0", "requires");
        assert!(code.contains("debug_assert!(x > 0,"));
        assert!(code.contains("requires"));
    }

    #[test]
    fn debug_assert_deep_field_becomes_comment() {
        let mut code = String::new();
        generate_debug_assert(&mut code, "state.head.extra", "ensures");
        assert!(code.starts_with("    // ensures:"));
        assert!(!code.contains("debug_assert!"));
    }

    #[test]
    fn debug_assert_multiline() {
        let mut code = String::new();
        generate_debug_assert(&mut code, "x > 0\n&& y > 0", "requires");
        assert!(code.contains("debug_assert!({"));
    }

    #[test]
    fn debug_assert_indented() {
        let mut code = String::new();
        generate_debug_assert_indented(&mut code, "x > 0", "test", 2);
        assert!(code.starts_with("        debug_assert!"));
    }

    // ---- pattern_to_rust ----

    #[test]
    fn pattern_ident() {
        use assura_parser::ast::Pattern;
        assert_eq!(pattern_to_rust(&Pattern::Ident("x".into())), "x");
    }

    #[test]
    fn pattern_wildcard() {
        use assura_parser::ast::Pattern;
        assert_eq!(pattern_to_rust(&Pattern::Wildcard), "_");
    }

    #[test]
    fn pattern_literal() {
        use assura_parser::ast::Pattern;
        assert_eq!(
            pattern_to_rust(&Pattern::Literal(Literal::Int("42".into()))),
            "42"
        );
    }

    #[test]
    fn pattern_constructor() {
        use assura_parser::ast::Pattern;
        let p = Pattern::Constructor {
            name: "Some".into(),
            fields: vec![Pattern::Ident("v".into())],
        };
        assert_eq!(pattern_to_rust(&p), "Some(v)");
    }

    #[test]
    fn pattern_constructor_empty() {
        use assura_parser::ast::Pattern;
        let p = Pattern::Constructor {
            name: "None".into(),
            fields: vec![],
        };
        assert_eq!(pattern_to_rust(&p), "None");
    }

    #[test]
    fn pattern_tuple() {
        use assura_parser::ast::Pattern;
        let p = Pattern::Tuple(vec![Pattern::Ident("a".into()), Pattern::Ident("b".into())]);
        assert_eq!(pattern_to_rust(&p), "(a, b)");
    }

    // ---- old_var_name ----

    #[test]
    fn old_var_name_ident() {
        assert_eq!(old_var_name(&Expr::Ident("x".into())), "x");
    }

    #[test]
    fn old_var_name_field() {
        let e = Expr::Field(Box::new(Expr::Ident("buf".into())), "len".into());
        assert_eq!(old_var_name(&e), "buf_len");
    }

    #[test]
    fn old_var_name_index() {
        let e = Expr::Index {
            expr: Box::new(Expr::Ident("arr".into())),
            index: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        assert_eq!(old_var_name(&e), "arr_idx");
    }

    #[test]
    fn old_var_name_binop() {
        let e = Expr::BinOp {
            lhs: Box::new(Expr::Ident("a".into())),
            op: BinOp::Add,
            rhs: Box::new(Expr::Ident("b".into())),
        };
        assert_eq!(old_var_name(&e), "a_add_b");
    }

    // ---- collect_old_exprs ----

    #[test]
    fn collect_old_empty() {
        assert!(collect_old_exprs(&Expr::Ident("x".into())).is_empty());
    }

    #[test]
    fn collect_old_single() {
        let e = Expr::Old(Box::new(Expr::Ident("x".into())));
        let result = collect_old_exprs(&e);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "x");
        assert_eq!(result[0].1, "x");
    }

    #[test]
    fn collect_old_nested_binop() {
        let e = Expr::BinOp {
            lhs: Box::new(Expr::Old(Box::new(Expr::Ident("a".into())))),
            op: BinOp::Add,
            rhs: Box::new(Expr::Old(Box::new(Expr::Ident("b".into())))),
        };
        let result = collect_old_exprs(&e);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn collect_old_deduplicates() {
        let e = Expr::BinOp {
            lhs: Box::new(Expr::Old(Box::new(Expr::Ident("x".into())))),
            op: BinOp::Add,
            rhs: Box::new(Expr::Old(Box::new(Expr::Ident("x".into())))),
        };
        let result = collect_old_exprs(&e);
        assert_eq!(result.len(), 1);
    }

    // ---- resolve_ordering_variant ----

    #[test]
    fn ordering_sequentially_consistent() {
        let e = Expr::Ident("seq_cst".into());
        assert_eq!(resolve_ordering_variant(&e), Some("SeqCst"));
    }

    #[test]
    fn ordering_relaxed() {
        let e = Expr::Ident("relaxed".into());
        assert_eq!(resolve_ordering_variant(&e), Some("Relaxed"));
    }

    #[test]
    fn ordering_unknown() {
        let e = Expr::Ident("garbage".into());
        assert_eq!(resolve_ordering_variant(&e), None);
    }
}
