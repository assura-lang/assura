//! Expression-to-Rust code generation.
//!
//! Translates Assura AST expressions into Rust source code strings.

use std::collections::HashSet;

use super::*;
use assura_ast::{ExprFolder, fold_arg_list, fold_joined, literal_to_string};

/// Hygienic variable name for the contract return value in generated Rust.
///
/// Uses a clearly compiler-generated prefix to avoid collision with
/// user-defined variables.
pub(crate) const RESULT_VAR: &str = "__assura_result";

/// Prefix for `old(expr)` pre-state snapshot variables in generated Rust.
pub(crate) const OLD_VAR_PREFIX: &str = "__assura_old_";

/// Returns true if the expression contains a literal that exceeds i128 range
/// (e.g. u128::MAX). Such literals cannot be wrapped in `i128::from(...)`.
fn has_u128_literal(expr: &SpExpr) -> bool {
    match &expr.node {
        Expr::Literal(Literal::Int(s)) => s.parse::<i128>().is_err() && s.parse::<u128>().is_ok(),
        Expr::BinOp { lhs, rhs, .. } => has_u128_literal(lhs) || has_u128_literal(rhs),
        Expr::UnaryOp { expr: e, .. }
        | Expr::Old(e)
        | Expr::Cast { expr: e, .. }
        | Expr::Field(e, _) => has_u128_literal(e),
        _ => false,
    }
}

/// Returns true if the expression tree contains a Float literal or references
/// a variable known to be float-typed. Used to skip `i128::from()` wrapping
/// since `f64` does not implement `Into<i128>`.
fn has_float_expr(expr: &SpExpr, float_vars: &HashSet<String>) -> bool {
    match &expr.node {
        Expr::Literal(Literal::Float(_)) => true,
        Expr::Ident(name) => float_vars.contains(name.as_str()),
        Expr::BinOp { lhs, rhs, .. } => {
            has_float_expr(lhs, float_vars) || has_float_expr(rhs, float_vars)
        }
        Expr::UnaryOp { expr: e, .. }
        | Expr::Old(e)
        | Expr::Cast { expr: e, .. }
        | Expr::Field(e, _) => has_float_expr(e, float_vars),
        Expr::MethodCall {
            receiver, args, ..
        } => {
            has_float_expr(receiver, float_vars)
                || args.iter().any(|a| has_float_expr(a, float_vars))
        }
        Expr::Call { args, .. } => args.iter().any(|a| has_float_expr(a, float_vars)),
        Expr::Let { body, .. } => has_float_expr(body, float_vars),
        Expr::If {
            then_branch,
            else_branch,
            ..
        } => {
            has_float_expr(then_branch, float_vars)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| has_float_expr(e, float_vars))
        }
        _ => false,
    }
}

/// Returns true if the folded Rust string already contains i128 widening.
/// Used to detect branch type mismatches in if/match expressions where
/// one branch gets i128::from() from arithmetic widening but the other
/// stays as a plain variable (i64).
fn has_inner_i128(folded: &str) -> bool {
    folded.contains("i128::from(") || folded.contains("_i128")
}

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

/// Returns true if the domain expression is a range (`a..b` via `BinOp::Range`).
///
/// Ranges in Rust implement `IntoIterator` but not `iter()` directly, so
/// quantifiers over ranges must use `.into_iter()` instead of `.iter().copied()`.
fn is_range_domain(expr: &SpExpr) -> bool {
    matches!(
        &expr.node,
        Expr::BinOp {
            op: BinOp::Range,
            ..
        }
    )
}

/// Returns true if the domain expression is an abstract mathematical type
/// identifier (Int, Nat, Float, Bool, String) that cannot be iterated at runtime.
///
/// Quantifiers like `forall x in Int: ...` are verification-only; at runtime
/// they are emitted as `true` with a comment.
fn is_abstract_type_domain(expr: &SpExpr) -> bool {
    match &expr.node {
        Expr::Ident(name) => matches!(
            name.as_str(),
            "Int" | "Nat" | "Float" | "Bool" | "String" | "Bytes" | "Unit"
        ),
        Expr::Raw(tokens) if tokens.len() == 1 => matches!(
            tokens[0].as_str(),
            "Int" | "Nat" | "Float" | "Bool" | "String" | "Bytes" | "Unit"
        ),
        _ => false,
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
        float_vars: HashSet::new(),
    }
    .fold_expr(expr)
}

/// Like [`expr_to_rust`] but with knowledge of which variables are float-typed.
/// Comparisons and arithmetic involving these variables use direct `f64`
/// operations instead of `i128::from()` widening.
pub(crate) fn expr_to_rust_with_floats(
    expr: &SpExpr,
    float_vars: HashSet<String>,
) -> String {
    RustCodegenFolder {
        static_context: false,
        float_vars,
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
        float_vars: HashSet::new(),
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
    /// Names of variables known to be `Float` (`f64` in Rust). When a
    /// comparison or arithmetic involves a float variable or literal,
    /// `i128::from()` wrapping is skipped (f64 does not implement `Into<i128>`).
    float_vars: HashSet<String>,
}

impl ExprFolder for RustCodegenFolder {
    type Output = String;

    fn fold_literal(&mut self, lit: &Literal) -> String {
        match lit {
            // Suffix large integer literals so they are not inferred as i32
            // inside `i128::from(...)` wrappings. Values within i32 range
            // are emitted without a suffix. Values in i64 range get `_i64`.
            // Values exceeding i64 range get `_i128`.
            Literal::Int(s) => {
                if let Ok(v) = s.parse::<i128>() {
                    if v > i128::from(i64::MAX) || v < i128::from(i64::MIN) {
                        return format!("{s}_i128");
                    }
                    if v > i128::from(i32::MAX) || v < i128::from(i32::MIN) {
                        return format!("{s}_i64");
                    }
                } else if s.parse::<u128>().is_ok() {
                    // Value exceeds i128 range (e.g. u128::MAX); emit as u128.
                    return format!("{s}_u128");
                }
                s.clone()
            }
            _ => literal_to_string(lit),
        }
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
        // Assura `length`/`len`/`size` map to Rust `.len()`; Nat is u64.
        if args.is_empty() && matches!(method, "length" | "len" | "size") {
            return format!("{}.len() as u64", self.fold_expr(receiver));
        }
        format!(
            "{}.{method}({})",
            self.fold_expr(receiver),
            fold_arg_list(self, args)
        )
    }

    fn fold_call(&mut self, func: &SpExpr, args: &[SpExpr]) -> String {
        // Map common pure numeric builtins to Rust methods / associated functions.
        if let Expr::Ident(name) = &func.node {
            match (name.as_str(), args.len()) {
                ("abs", 1) => return format!("{}.abs()", self.fold_expr(&args[0])),
                ("min", 2) => {
                    return format!(
                        "{}.min({})",
                        self.fold_expr(&args[0]),
                        self.fold_expr(&args[1])
                    );
                }
                ("max", 2) => {
                    return format!(
                        "{}.max({})",
                        self.fold_expr(&args[0]),
                        self.fold_expr(&args[1])
                    );
                }
                _ => {}
            }
        }
        format!("{}({})", self.fold_expr(func), fold_arg_list(self, args))
    }

    fn fold_index(&mut self, base: &SpExpr, index: &SpExpr) -> String {
        let idx = self.fold_expr(index);
        if !self.static_context && is_numeric_expr(index) {
            // Array/slice indexing requires usize. Assura Int maps to i64/i128
            // in codegen, and numeric expressions may be widened. Cast the index
            // to usize to satisfy Rust's indexing requirements.
            format!("{}[({idx}) as usize]", self.fold_expr(base))
        } else {
            format!("{}[{idx}]", self.fold_expr(base))
        }
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
            // Widen all numeric comparisons and arithmetic to i128 to prevent
            // mixed-type errors (e.g. i64 + u64, u64 == i128) in generated
            // code when contracts mix Int and Nat typed inputs.
            // Skip i128 wrapping when either side has a u128-scale literal
            // (e.g. u128::MAX) since i128::from(u128) does not exist.
            // Also skip when either side involves Float (f64 does not
            // implement Into<i128>).
            if (op.is_comparison() || op.is_arithmetic())
                && is_numeric_expr(lhs)
                && is_numeric_expr(rhs)
                && !has_float_expr(lhs, &self.float_vars)
                && !has_float_expr(rhs, &self.float_vars)
            {
                if has_u128_literal(lhs) || has_u128_literal(rhs) {
                    return format!(
                        "(({} as u128) {op_s} ({} as u128))",
                        self.fold_expr(lhs),
                        self.fold_expr(rhs)
                    );
                }
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
        if self.static_context || is_abstract_type_domain(domain) {
            let d = self.fold_expr(domain);
            let b = self.fold_expr(body);
            format!("/* forall {var} in {d}: {b} */ true")
        } else if is_range_domain(domain) {
            format!(
                "({}).into_iter().all(|{var}| {})",
                self.fold_expr(domain),
                self.fold_expr(body)
            )
        } else {
            format!(
                "{}.iter().copied().all(|{var}| {})",
                self.fold_expr(domain),
                self.fold_expr(body)
            )
        }
    }

    fn fold_exists(&mut self, var: &str, domain: &SpExpr, body: &SpExpr) -> String {
        if self.static_context || is_abstract_type_domain(domain) {
            let d = self.fold_expr(domain);
            let b = self.fold_expr(body);
            format!("/* exists {var} in {d}: {b} */ true")
        } else if is_range_domain(domain) {
            format!(
                "({}).into_iter().any(|{var}| {})",
                self.fold_expr(domain),
                self.fold_expr(body)
            )
        } else {
            format!(
                "{}.iter().copied().any(|{var}| {})",
                self.fold_expr(domain),
                self.fold_expr(body)
            )
        }
    }

    fn fold_if(&mut self, cond: &SpExpr, then_br: &SpExpr, else_br: Option<&SpExpr>) -> String {
        match else_br {
            Some(eb) => {
                let then_s = self.fold_expr(then_br);
                let else_s = self.fold_expr(eb);
                // In runtime context, if either branch contains i128 arithmetic
                // but the other is a plain ident/literal, the branches produce
                // incompatible types. Normalize both to i128::from() when both
                // are numeric to ensure type consistency.
                if !self.static_context
                    && is_numeric_expr(then_br)
                    && is_numeric_expr(eb)
                    && (has_inner_i128(&then_s) || has_inner_i128(&else_s))
                    && !has_float_expr(then_br, &self.float_vars)
                    && !has_float_expr(eb, &self.float_vars)
                {
                    let t = if has_inner_i128(&then_s) {
                        then_s
                    } else {
                        format!("i128::from({then_s})")
                    };
                    let e = if has_inner_i128(&else_s) {
                        else_s
                    } else {
                        format!("i128::from({else_s})")
                    };
                    return format!("if {} {{ {} }} else {{ {} }}", self.fold_expr(cond), t, e);
                }
                format!(
                    "if {} {{ {} }} else {{ {} }}",
                    self.fold_expr(cond),
                    then_s,
                    else_s
                )
            }
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
            return format!("{domain}.iter().copied().{method}(|{var}| {body})");
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
