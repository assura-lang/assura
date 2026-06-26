//! Expression-to-Rust code generation.
//!
//! Translates Assura AST expressions into Rust source code strings.

use super::*;
use assura_ast::{ExprFolder, fold_arg_list, fold_joined, literal_to_string};

/// Hygienic variable name for the contract return value in generated Rust.
///
/// Uses a clearly compiler-generated prefix to avoid collision with
/// user-defined variables.
pub(crate) const RESULT_VAR: &str = "__assura_result";

/// Prefix for `old(expr)` pre-state snapshot variables in generated Rust.
pub(crate) const OLD_VAR_PREFIX: &str = "__assura_old_";

/// Heuristic: returns true if the expression is likely a numeric value
/// (variable, constant, literal, or arithmetic). Used to decide whether to
/// emit `i128::from(...)` casts for cross-width comparisons.
pub(crate) fn is_numeric_expr(expr: &SpExpr) -> bool {
    match &expr.node {
        Expr::Ident(_) | Expr::Literal(Literal::Int(_)) | Expr::Literal(Literal::Float(_)) => true,
        Expr::Field(_, _) => true,
        Expr::BinOp { op, .. } => op.is_arithmetic(),
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
pub(crate) fn resolve_ordering_variant(body: &SpExpr) -> Option<&'static str> {
    use assura_ast::MemoryOrdering;
    let s = match &body.node {
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
pub(crate) fn expr_to_rust(expr: &SpExpr) -> String {
    RustCodegenFolder {
        static_context: false,
    }
    .fold_expr(expr)
}

/// Convert an Assura `Expr` to a Rust expression for use in const context.
///
/// Compared to [`expr_to_rust`], this variant:
/// - Does not rename `result` to the compiler-generated result variable
/// - Does not emit `i128::from()` casts for numeric comparisons
/// - Does not translate `Implies`/`In`/`NotIn`/`Concat` to Rust idioms
/// - Emits quantifiers as `/* forall ... */ true` comments
/// - Passes `old(expr)` through as the inner expression
/// - Uses simplified match patterns (`Ctor(..)` instead of field bindings)
pub fn expr_to_rust_static(expr: &SpExpr) -> String {
    RustCodegenFolder {
        static_context: true,
    }
    .fold_expr(expr)
}

/// Unified Assura-to-Rust expression folder.
///
/// When `static_context` is false (runtime), produces full Rust code with
/// `result` renaming, `i128` casts, quantifier-to-iterator translation, etc.
/// When `static_context` is true, produces simplified Rust suitable for
/// const/static contexts.
struct RustCodegenFolder {
    static_context: bool,
}

impl ExprFolder for RustCodegenFolder {
    type Output = String;

    fn fold_literal(&mut self, lit: &Literal) -> String {
        literal_to_string(lit)
    }

    fn fold_ident(&mut self, name: &str) -> String {
        if !self.static_context && name == "result" {
            RESULT_VAR.to_string()
        } else {
            name.to_string()
        }
    }

    fn fold_field(&mut self, base: &SpExpr, field: &str) -> String {
        format!("{}.{field}", self.fold_expr(base))
    }

    fn fold_method_call(&mut self, receiver: &SpExpr, method: &str, args: &[SpExpr]) -> String {
        format!(
            "{}.{method}({})",
            self.fold_expr(receiver),
            fold_arg_list(self, args)
        )
    }

    fn fold_call(&mut self, func: &SpExpr, args: &[SpExpr]) -> String {
        format!("{}({})", self.fold_expr(func), fold_arg_list(self, args))
    }

    fn fold_index(&mut self, base: &SpExpr, index: &SpExpr) -> String {
        format!("{}[{}]", self.fold_expr(base), self.fold_expr(index))
    }

    fn fold_binop(&mut self, lhs: &SpExpr, op: &BinOp, rhs: &SpExpr) -> String {
        if !self.static_context {
            // Runtime: handle special operators and i128 casts
            match op {
                BinOp::Implies => {
                    return format!("(!{} || {})", self.fold_expr(lhs), self.fold_expr(rhs));
                }
                BinOp::In => {
                    return format!("{}.contains(&{})", self.fold_expr(rhs), self.fold_expr(lhs));
                }
                BinOp::NotIn => {
                    return format!(
                        "!{}.contains(&{})",
                        self.fold_expr(rhs),
                        self.fold_expr(lhs)
                    );
                }
                BinOp::Concat => {
                    return format!(
                        "[{}, {}].concat()",
                        self.fold_expr(lhs),
                        self.fold_expr(rhs)
                    );
                }
                _ => {}
            }
            let op_s = op.as_rust_str();
            if op.is_ordering_comparison() && is_numeric_expr(lhs) && is_numeric_expr(rhs) {
                return format!(
                    "(i128::from({}) {op_s} i128::from({}))",
                    self.fold_expr(lhs),
                    self.fold_expr(rhs)
                );
            }
        }
        format!(
            "({} {} {})",
            self.fold_expr(lhs),
            op.as_rust_str(),
            self.fold_expr(rhs)
        )
    }

    fn fold_unary_op(&mut self, op: &UnaryOp, inner: &SpExpr) -> String {
        let inner_s = self.fold_expr(inner);
        if self.static_context {
            format!("{}{inner_s}", op.as_rust_str())
        } else {
            format!("({}{})", op.as_rust_str(), inner_s)
        }
    }

    fn fold_old(&mut self, inner: &SpExpr) -> String {
        if self.static_context {
            // old() is verification-only; emit inner for static
            self.fold_expr(inner)
        } else {
            format!("{OLD_VAR_PREFIX}{}", old_var_name(inner))
        }
    }

    fn fold_forall(&mut self, var: &str, domain: &SpExpr, body: &SpExpr) -> String {
        if self.static_context {
            let d = self.fold_expr(domain);
            let b = self.fold_expr(body);
            format!("/* forall {var} in {d}: {b} */ true")
        } else {
            format!(
                "{}.iter().all(|{var}| {})",
                self.fold_expr(domain),
                self.fold_expr(body)
            )
        }
    }

    fn fold_exists(&mut self, var: &str, domain: &SpExpr, body: &SpExpr) -> String {
        if self.static_context {
            let d = self.fold_expr(domain);
            let b = self.fold_expr(body);
            format!("/* exists {var} in {d}: {b} */ true")
        } else {
            format!(
                "{}.iter().any(|{var}| {})",
                self.fold_expr(domain),
                self.fold_expr(body)
            )
        }
    }

    fn fold_if(&mut self, cond: &SpExpr, then_br: &SpExpr, else_br: Option<&SpExpr>) -> String {
        match else_br {
            Some(eb) => format!(
                "if {} {{ {} }} else {{ {} }}",
                self.fold_expr(cond),
                self.fold_expr(then_br),
                self.fold_expr(eb)
            ),
            None => format!(
                "if {} {{ {} }}",
                self.fold_expr(cond),
                self.fold_expr(then_br)
            ),
        }
    }

    fn fold_list(&mut self, items: &[SpExpr]) -> String {
        format!("vec![{}]", fold_joined(self, items, ", "))
    }

    fn fold_cast(&mut self, inner: &SpExpr, ty: &str) -> String {
        if self.static_context {
            format!("({} as {ty})", self.fold_expr(inner))
        } else {
            format!("({} as {})", self.fold_expr(inner), map_type_token(ty))
        }
    }

    fn fold_block(&mut self, exprs: &[SpExpr]) -> String {
        fold_joined(self, exprs, " ")
    }

    fn fold_ghost(&mut self, inner: &SpExpr) -> String {
        if self.static_context {
            let s = self.fold_expr(inner);
            format!("/* ghost: {s} */ ()")
        } else {
            "/* ghost erased */()".to_string()
        }
    }

    fn fold_apply(&mut self, lemma_name: &str, args: &[SpExpr]) -> String {
        if self.static_context {
            format!("/* apply {lemma_name}({}) */ ()", fold_arg_list(self, args))
        } else {
            format!("/* lemma {lemma_name} applied */")
        }
    }

    fn fold_let(&mut self, name: &str, value: &SpExpr, body: &SpExpr) -> String {
        format!(
            "{{ let {} = {}; {} }}",
            name,
            self.fold_expr(value),
            self.fold_expr(body)
        )
    }

    fn fold_match(&mut self, scrutinee: &SpExpr, arms: &[assura_ast::MatchArm]) -> String {
        let scrut = self.fold_expr(scrutinee);
        if self.static_context {
            let arm_strs: Vec<String> = arms
                .iter()
                .map(|arm| {
                    let pat = match &arm.pattern {
                        assura_ast::Pattern::Ident(s) => s.clone(),
                        assura_ast::Pattern::Wildcard => "_".to_string(),
                        assura_ast::Pattern::Literal(lit) => match lit {
                            Literal::Int(s) | Literal::Float(s) => s.clone(),
                            Literal::Str(s) => format!("\"{s}\""),
                            Literal::Bool(b) => b.to_string(),
                        },
                        assura_ast::Pattern::Constructor { name, fields } => {
                            if fields.is_empty() {
                                name.clone()
                            } else {
                                format!("{name}(..)")
                            }
                        }
                        assura_ast::Pattern::Tuple(pats) => {
                            let ps: Vec<&str> = pats.iter().map(|_| "_").collect();
                            format!("({})", ps.join(", "))
                        }
                    };
                    let body = self.fold_expr(&arm.body);
                    format!("{pat} => {body}")
                })
                .collect();
            format!("match {scrut} {{ {} }}", arm_strs.join(", "))
        } else {
            let arms_code: Vec<String> = arms
                .iter()
                .map(|arm| {
                    let pat = match &arm.pattern {
                        assura_ast::Pattern::Ident(name) => name.clone(),
                        assura_ast::Pattern::Wildcard => "_".into(),
                        assura_ast::Pattern::Literal(lit) => match lit {
                            Literal::Int(s) | Literal::Float(s) => s.clone(),
                            Literal::Str(s) => format!("\"{s}\""),
                            Literal::Bool(b) => b.to_string(),
                        },
                        assura_ast::Pattern::Constructor { name, fields } => {
                            if fields.is_empty() {
                                name.clone()
                            } else {
                                let fs: Vec<String> = fields.iter().map(pattern_to_rust).collect();
                                format!("{name}({})", fs.join(", "))
                            }
                        }
                        assura_ast::Pattern::Tuple(pats) => {
                            let ps: Vec<String> = pats.iter().map(pattern_to_rust).collect();
                            format!("({})", ps.join(", "))
                        }
                    };
                    let body = self.fold_expr(&arm.body);
                    format!("    {pat} => {body},")
                })
                .collect();
            let has_wildcard = arms.iter().any(|arm| {
                matches!(
                    &arm.pattern,
                    assura_ast::Pattern::Wildcard | assura_ast::Pattern::Ident(_)
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
    }

    fn fold_tuple(&mut self, items: &[SpExpr]) -> String {
        format!("({})", fold_joined(self, items, ", "))
    }

    fn fold_raw(&mut self, tokens: &[String]) -> String {
        if self.static_context {
            let clean: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();
            if clean.len() == 1 {
                return clean[0].to_string();
            }
            if clean.len() >= 2 && clean[0] == "=" {
                return clean[1..].join(" ");
            }
            clean.join(" ")
        } else {
            raw_tokens_to_rust(tokens)
        }
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

    // Check for `result` keyword — replace with result var
    let mapped: Vec<String> = tokens
        .iter()
        .map(|t| {
            if t == "result" {
                RESULT_VAR.to_string()
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
/// E.g., `old(x)` -> `__assura_old_x`, `old(buf.len)` -> `__assura_old_buf_len`.
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
/// - References to `{RESULT_VAR}.field`
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
    // Result var field references (but not .iter(), etc.)
    let result_dot = format!("{RESULT_VAR}.");
    if expr.contains(&result_dot) {
        // Check if all occurrences are followed by method calls
        for chunk in expr.split(&result_dot) {
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
#[cfg(test)]
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
pub(crate) fn pattern_to_rust(pat: &assura_ast::Pattern) -> String {
    match pat {
        assura_ast::Pattern::Ident(name) => name.clone(),
        assura_ast::Pattern::Wildcard => "_".into(),
        assura_ast::Pattern::Literal(lit) => match lit {
            Literal::Int(s) | Literal::Float(s) => s.clone(),
            Literal::Str(s) => format!("\"{s}\""),
            Literal::Bool(b) => b.to_string(),
        },
        assura_ast::Pattern::Constructor { name, fields } => {
            if fields.is_empty() {
                name.clone()
            } else {
                let fs: Vec<String> = fields.iter().map(pattern_to_rust).collect();
                format!("{name}({})", fs.join(", "))
            }
        }
        assura_ast::Pattern::Tuple(pats) => {
            let ps: Vec<String> = pats.iter().map(pattern_to_rust).collect();
            format!("({})", ps.join(", "))
        }
    }
}

pub(crate) fn old_var_name(expr: &SpExpr) -> String {
    match &expr.node {
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
            format!(
                "{}_{}_{}",
                old_var_name(lhs),
                op.as_ident(),
                old_var_name(rhs)
            )
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
pub(crate) fn collect_old_exprs(expr: &SpExpr) -> Vec<(String, String)> {
    let mut result = Vec::new();
    collect_old_exprs_inner(expr, &mut result);
    result
}

pub(crate) fn collect_old_exprs_inner(expr: &SpExpr, out: &mut Vec<(String, String)>) {
    match &expr.node {
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
        Expr::UnaryOp { expr: e, .. } | Expr::Field(e, _) | Expr::Cast { expr: e, .. } => {
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
#[path = "expr_tests.rs"]
mod tests;
