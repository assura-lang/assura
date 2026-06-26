use super::*;

// ---------------------------------------------------------------------------
// Usage tracking for linear types (T031)
// ---------------------------------------------------------------------------

/// Usage grade for a variable, following Section 2.5 of the spec.
///
/// Determines how many times a variable may be used at runtime.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum UsageGrade {
    /// Grade 0: ghost/erased, no runtime usage allowed.
    Erased,
    /// Grade 1: linear, must be used exactly once.
    Linear,
    /// Grade n: must be used exactly `n` times.
    Exact(u32),
    /// Grade omega: unlimited, can be used any number of times.
    Unlimited,
}

impl std::fmt::Display for UsageGrade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UsageGrade::Erased => write!(f, "erased (grade 0)"),
            UsageGrade::Linear => write!(f, "linear (grade 1)"),
            UsageGrade::Exact(n) => write!(f, "exact (grade {n})"),
            UsageGrade::Unlimited => write!(f, "unlimited (grade ω)"),
        }
    }
}

/// Tracks variable usage counts and compares against expected grades.
///
/// Used for linearity checking: each variable is declared with an expected
/// `UsageGrade`, and each use of the variable increments its actual count.
/// After analysis, `check()` compares actual counts against expected grades
/// and produces errors for violations.
#[derive(Debug, Clone, Default)]
pub(crate) struct UsageTracker {
    /// Maps variable name -> (expected grade, actual usage count, declaration span).
    usages: HashMap<String, (UsageGrade, u32, Range<usize>)>,
}

impl UsageTracker {
    /// Create an empty usage tracker.
    pub fn new() -> Self {
        Self {
            usages: HashMap::new(),
        }
    }

    /// Declare a variable with its expected usage grade and declaration span.
    ///
    /// If the variable was already declared, updates its grade and resets
    /// the count.
    pub fn declare(&mut self, name: String, grade: UsageGrade, span: Range<usize>) {
        self.usages.insert(name, (grade, 0, span));
    }

    /// Record a use of a variable. Increments its usage count.
    ///
    /// If the variable was not declared via `declare()`, this is a no-op
    /// (the variable may be unlimited/external and not tracked).
    pub fn use_var(&mut self, name: &str) {
        if let Some((_grade, count, _span)) = self.usages.get_mut(name) {
            *count += 1;
        }
    }

    /// Check all tracked variables against their expected usage grades.
    ///
    /// Returns a list of `TypeError`s for any violations:
    /// - **A05001**: Linear variable used more than once
    /// - **A05002**: Linear variable never used (or erased variable used)
    /// - **A05003**: Exact-count variable used wrong number of times
    pub fn check(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();

        for (name, (grade, count, span)) in &self.usages {
            match grade {
                UsageGrade::Erased => {
                    if *count > 0 {
                        errors.push(TypeError {
                            code: "A05002".into(),
                            message: format!(
                                "erased variable `{name}` must not be used at runtime, \
                                 but was used {count} time(s)"
                            ),
                            span: span.clone(),
                            secondary: None,
                        });
                    }
                }
                UsageGrade::Linear => {
                    if *count == 0 {
                        errors.push(TypeError {
                            code: "A05002".into(),
                            message: format!("linear variable `{name}` was never used"),
                            span: span.clone(),
                            secondary: None,
                        });
                    } else if *count > 1 {
                        errors.push(TypeError {
                            code: "A05001".into(),
                            message: format!(
                                "linear variable `{name}` used {count} times, \
                                 but must be used exactly once"
                            ),
                            span: span.clone(),
                            secondary: None,
                        });
                    }
                }
                UsageGrade::Exact(expected) => {
                    if count != expected {
                        errors.push(TypeError {
                            code: "A05003".into(),
                            message: format!(
                                "variable `{name}` used {count} time(s), \
                                 but must be used exactly {expected} time(s)"
                            ),
                            span: span.clone(),
                            secondary: None,
                        });
                    }
                }
                UsageGrade::Unlimited => {
                    // No restrictions on usage count.
                }
            }
        }

        // Sort errors by span start for deterministic output.
        errors.sort_by_key(|e| e.span.start);
        errors
    }

    /// Get the current usage count for a variable.
    pub fn get_count(&self, name: &str) -> Option<u32> {
        self.usages.get(name).map(|(_, count, _)| *count)
    }

    /// Set the usage count for a variable (used during context merge).
    pub fn set_count(&mut self, name: &str, count: u32) {
        if let Some((_grade, c, _span)) = self.usages.get_mut(name) {
            *c = count;
        }
    }
}

// ---------------------------------------------------------------------------
// Linear context with branch support (T032)
// ---------------------------------------------------------------------------

/// Linear type context with branching support for context splitting.
///
/// Wraps a `UsageTracker` and adds fork/merge operations for handling
/// if/match branches correctly. At each branch point, the context is
/// forked, each branch is checked independently, and the results are
/// merged back with consistency checks.
#[derive(Debug, Clone)]
pub(crate) struct LinearContext {
    tracker: UsageTracker,
}

impl LinearContext {
    /// Create a new linear context from a usage tracker.
    pub fn new(tracker: UsageTracker) -> Self {
        Self { tracker }
    }

    /// Record a variable use in this context.
    pub fn use_var(&mut self, name: &str) {
        self.tracker.use_var(name);
    }

    /// Declare a variable in this context.
    pub fn declare(&mut self, name: String, grade: UsageGrade, span: Range<usize>) {
        self.tracker.declare(name, grade, span);
    }

    /// Get the current usage count for a variable (test-only).
    #[cfg(test)]
    pub fn get_count(&self, name: &str) -> Option<u32> {
        self.tracker.get_count(name)
    }

    /// Create two independent copies of this context for branching.
    pub fn fork(&self) -> (LinearContext, LinearContext) {
        (self.clone(), self.clone())
    }

    /// Merge two branch contexts back into this context.
    ///
    /// Compares usage counts in `branch_a` and `branch_b` against the
    /// counts in `self` (the pre-branch base state). For linear and
    /// exact-grade variables, if the usage delta differs between branches,
    /// emits A05004 (inconsistent branch usage).
    ///
    /// After merge, updates `self` with the maximum usage count from
    /// either branch (conservative: treat as consumed if used in any path).
    pub fn merge(&mut self, branch_a: &LinearContext, branch_b: &LinearContext) -> Vec<TypeError> {
        let mut errors = Vec::new();

        // Snapshot the base state before mutation.
        let base_state: Vec<(String, UsageGrade, u32, Range<usize>)> = self
            .tracker
            .usages
            .iter()
            .map(|(name, (grade, count, span))| (name.clone(), grade.clone(), *count, span.clone()))
            .collect();

        for (name, grade, base_count, span) in &base_state {
            let a_count = branch_a.tracker.get_count(name).unwrap_or(*base_count);
            let b_count = branch_b.tracker.get_count(name).unwrap_or(*base_count);

            let delta_a = a_count.saturating_sub(*base_count);
            let delta_b = b_count.saturating_sub(*base_count);

            // Check consistency for linear and exact-grade variables.
            if matches!(grade, UsageGrade::Linear | UsageGrade::Exact(_)) && delta_a != delta_b {
                errors.push(TypeError {
                    code: "A05004".into(),
                    message: format!(
                        "linear variable `{name}` used inconsistently across branches: \
                         used {delta_a} time(s) in one branch, {delta_b} time(s) in the other"
                    ),
                    span: span.clone(),
                    secondary: None,
                });
            }

            // Take the maximum: treat as consumed if used in any branch.
            let merged_count = base_count + std::cmp::max(delta_a, delta_b);
            self.tracker.set_count(name, merged_count);
        }

        errors
    }

    /// Merge multiple branch contexts (for match arms) back into this context.
    ///
    /// All arms must consume linear variables the same number of times.
    /// If any arm differs, emits A05004. After merge, updates `self` with
    /// the maximum usage count from any arm.
    pub fn merge_arms(&mut self, arm_contexts: &[LinearContext]) -> Vec<TypeError> {
        if arm_contexts.is_empty() {
            return Vec::new();
        }
        let mut errors = Vec::new();

        let base_state: Vec<(String, UsageGrade, u32, Range<usize>)> = self
            .tracker
            .usages
            .iter()
            .map(|(name, (grade, count, span))| (name.clone(), grade.clone(), *count, span.clone()))
            .collect();

        for (name, grade, base_count, span) in &base_state {
            let deltas: Vec<u32> = arm_contexts
                .iter()
                .map(|arm| {
                    arm.tracker
                        .get_count(name)
                        .unwrap_or(*base_count)
                        .saturating_sub(*base_count)
                })
                .collect();

            // Check consistency: all deltas must be equal for linear/exact.
            if matches!(grade, UsageGrade::Linear | UsageGrade::Exact(_)) {
                let first = deltas[0];
                for (i, &delta) in deltas.iter().enumerate().skip(1) {
                    if delta != first {
                        errors.push(TypeError {
                            code: "A05004".into(),
                            message: format!(
                                "linear variable `{name}` used inconsistently across match arms: \
                                 used {first} time(s) in arm 1, {delta} time(s) in arm {}",
                                i + 1
                            ),
                            span: span.clone(),
                            secondary: None,
                        });
                        break; // One error per variable is enough.
                    }
                }
            }

            let max_delta = deltas.iter().copied().max().unwrap_or(0);
            self.tracker.set_count(name, base_count + max_delta);
        }

        errors
    }

    /// Run the final usage check on this context.
    ///
    /// Delegates to `UsageTracker::check()`, producing A05001-A05003 errors
    /// for any remaining linearity violations after all expressions have
    /// been walked.
    pub fn check(&self) -> Vec<TypeError> {
        self.tracker.check()
    }
}

/// Walk an expression AST with linear context splitting for branches.
///
/// For if/match expressions, forks the context, walks each branch
/// independently, and merges the results back. This is the context-
/// splitting implementation for T032.
///
/// Returns errors for:
/// - A05004: linear variable used inconsistently across branches
/// - A05005: linear variable escapes its scope
pub(crate) fn check_expr_linearity(expr: &SpExpr, ctx: &mut LinearContext) -> Vec<TypeError> {
    let mut errors = Vec::new();
    check_expr_linearity_inner(expr, ctx, &mut errors);
    errors
}

/// Inner recursive walker for `check_expr_linearity`.
fn check_expr_linearity_inner(expr: &SpExpr, ctx: &mut LinearContext, errors: &mut Vec<TypeError>) {
    match &expr.node {
        Expr::Ident(name) => {
            ctx.use_var(name);
        }
        Expr::Literal(_) => {}
        Expr::Field(receiver, _field) => {
            check_expr_linearity_inner(receiver, ctx, errors);
        }
        Expr::MethodCall { receiver, args, .. } => {
            check_expr_linearity_inner(receiver, ctx, errors);
            for arg in args {
                check_expr_linearity_inner(arg, ctx, errors);
            }
        }
        Expr::Call { func, args } => {
            check_expr_linearity_inner(func, ctx, errors);
            for arg in args {
                check_expr_linearity_inner(arg, ctx, errors);
            }
        }
        Expr::Index { expr: base, index } => {
            check_expr_linearity_inner(base, ctx, errors);
            check_expr_linearity_inner(index, ctx, errors);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            check_expr_linearity_inner(lhs, ctx, errors);
            check_expr_linearity_inner(rhs, ctx, errors);
        }
        Expr::UnaryOp { expr: inner, .. } => {
            check_expr_linearity_inner(inner, ctx, errors);
        }
        Expr::Old(_inner) => {
            // old(x) references the pre-state (ghost/logical), not a
            // computational use. Does NOT count as a linear use.
        }
        Expr::Forall {
            var: _,
            domain: _,
            body: _,
        }
        | Expr::Exists {
            var: _,
            domain: _,
            body: _,
        } => {
            // Quantifier bodies are ghost/logical (refinement predicates).
            // References inside do NOT count as linear uses per Spec
            // Section 13 Test Case 1 (Ghost Use Problem).
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            // Check condition in current context (condition is always evaluated).
            check_expr_linearity_inner(cond, ctx, errors);

            // Fork context for the two branches.
            let (mut ctx_then, mut ctx_else) = ctx.fork();

            // Walk each branch independently.
            check_expr_linearity_inner(then_branch, &mut ctx_then, errors);

            if let Some(else_br) = else_branch {
                check_expr_linearity_inner(else_br, &mut ctx_else, errors);
            }
            // If there is no else branch, ctx_else stays at the
            // post-condition counts (no additional uses), which makes
            // any variable used only in the then-branch inconsistent.

            // Merge: check consistency and take max usage.
            let merge_errors = ctx.merge(&ctx_then, &ctx_else);
            errors.extend(merge_errors);
        }
        Expr::List(items) => {
            for item in items {
                check_expr_linearity_inner(item, ctx, errors);
            }
        }
        Expr::Cast { expr: inner, .. } => {
            check_expr_linearity_inner(inner, ctx, errors);
        }
        Expr::Block(exprs) => {
            for e in exprs {
                check_expr_linearity_inner(e, ctx, errors);
            }
        }
        Expr::Ghost(_inner) => {
            // Ghost blocks are erased at runtime. Variable references
            // inside ghost blocks do NOT count as linear uses.
        }
        Expr::Apply { .. } => {
            // Apply expressions are erased at runtime (like ghost).
            // Linear uses inside them are not counted.
        }
        Expr::Match { scrutinee, arms } => {
            // Check scrutinee in current context (always evaluated).
            check_expr_linearity_inner(scrutinee, ctx, errors);

            if arms.is_empty() {
                return;
            }

            // Fork context for each arm and check independently.
            let mut arm_contexts: Vec<LinearContext> = Vec::new();
            for arm in arms {
                let mut arm_ctx = ctx.clone();
                check_expr_linearity_inner(&arm.body, &mut arm_ctx, errors);
                arm_contexts.push(arm_ctx);
            }

            // Merge: check consistency across all arms.
            let merge_errs = ctx.merge_arms(&arm_contexts);
            errors.extend(merge_errs);
        }
        Expr::Let { value, body, .. } => {
            check_expr_linearity_inner(value, ctx, errors);
            check_expr_linearity_inner(body, ctx, errors);
        }
        Expr::Tuple(elems) => {
            for e in elems {
                check_expr_linearity_inner(e, ctx, errors);
            }
        }
        Expr::Raw(_) => {
            // Cannot extract variable references from raw token sequences.
        }
    }
}

// ---------------------------------------------------------------------------
// Source-level linearity check (moved from checks/linear_typestate.rs)
// ---------------------------------------------------------------------------

/// Infer a usage grade from type annotation tokens.
fn infer_usage_grade(ty_tokens: &[String]) -> UsageGrade {
    for (i, t) in ty_tokens.iter().enumerate() {
        match t.as_str() {
            "linear" => return UsageGrade::Linear,
            "ghost" | "erased" => return UsageGrade::Erased,
            "exact" => {
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
pub(crate) fn declare_linear_params_from_expr(
    expr: &SpExpr,
    tracker: &mut UsageTracker,
    span: &std::ops::Range<usize>,
) {
    match &expr.node {
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
                && let Expr::Ident(name) = &inner.as_ref().node
            {
                tracker.declare(name.clone(), UsageGrade::Linear, span.clone());
            }
        }
        Expr::Ident(_) => {}
        Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                declare_linear_single_param(item, tracker, span);
            }
        }
        _ => {}
    }
}

fn declare_linear_single_param(
    expr: &SpExpr,
    tracker: &mut UsageTracker,
    span: &std::ops::Range<usize>,
) {
    match &expr.node {
        Expr::Cast { expr: inner, ty } => {
            if ty.contains("linear")
                && let Expr::Ident(name) = &inner.as_ref().node
            {
                tracker.declare(name.clone(), UsageGrade::Linear, span.clone());
            }
        }
        Expr::Raw(tokens) => {
            declare_linear_params_from_raw(tokens, tracker, span);
        }
        _ => {}
    }
}

fn declare_linear_params_from_raw(
    tokens: &[String],
    tracker: &mut UsageTracker,
    span: &std::ops::Range<usize>,
) {
    let mut i = 0;
    while i < tokens.len() {
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
            while i < tokens.len() && tokens[i] != "," {
                i += 1;
            }
        }
        i += 1;
    }
}

pub(crate) fn run_linearity_checks_source(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    use assura_parser::ast::{ClauseKind, Decl, ServiceItem};

    let mut errors = Vec::new();
    for decl in &source.decls {
        if let Decl::Contract(c) = &decl.node {
            let mut tracker = UsageTracker::new();
            for clause in &c.clauses {
                if clause.kind == ClauseKind::Input {
                    declare_linear_params_from_expr(&clause.body, &mut tracker, &decl.span);
                }
            }
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
        } else if matches!(&decl.node, Decl::FnDef(_) | Decl::Extern(_)) {
            let tracker = UsageTracker::new();
            let mut ctx = LinearContext::new(tracker);
            for param in decl.node.params() {
                let p_tokens = param.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
                let grade = infer_usage_grade(&p_tokens);
                if grade != UsageGrade::Unlimited {
                    ctx.declare(param.name.clone(), grade, decl.span.clone());
                }
            }
            for clause in decl.node.clauses() {
                errors.extend(check_expr_linearity(&clause.body, &mut ctx));
            }
            errors.extend(ctx.check());
        } else if let Decl::Service(s) = &decl.node {
            for item in &s.items {
                if let ServiceItem::Operation { clauses, .. } | ServiceItem::Query { clauses, .. } =
                    item
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
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::Spanned;

    fn span() -> Range<usize> {
        0..10
    }

    fn ident(s: &str) -> SpExpr {
        Spanned::no_span(Expr::Ident(s.to_string()))
    }

    fn int_lit(n: i64) -> SpExpr {
        Spanned::no_span(Expr::Literal(Literal::Int(n.to_string())))
    }

    // ---- UsageTracker ----

    #[test]
    fn tracker_linear_used_once_ok() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Linear, span());
        t.use_var("x");
        let errs = t.check();
        assert!(errs.is_empty());
    }

    #[test]
    fn tracker_linear_never_used() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Linear, span());
        let errs = t.check();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A05002");
    }

    #[test]
    fn tracker_linear_used_twice() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Linear, span());
        t.use_var("x");
        t.use_var("x");
        let errs = t.check();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A05001");
    }

    #[test]
    fn tracker_erased_used_at_runtime() {
        let mut t = UsageTracker::new();
        t.declare("g".into(), UsageGrade::Erased, span());
        t.use_var("g");
        let errs = t.check();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A05002");
    }

    #[test]
    fn tracker_erased_not_used_ok() {
        let mut t = UsageTracker::new();
        t.declare("g".into(), UsageGrade::Erased, span());
        let errs = t.check();
        assert!(errs.is_empty());
    }

    #[test]
    fn tracker_exact_correct_count() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Exact(3), span());
        t.use_var("x");
        t.use_var("x");
        t.use_var("x");
        let errs = t.check();
        assert!(errs.is_empty());
    }

    #[test]
    fn tracker_exact_wrong_count() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Exact(2), span());
        t.use_var("x");
        let errs = t.check();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A05003");
    }

    #[test]
    fn tracker_unlimited_any_count_ok() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Unlimited, span());
        t.use_var("x");
        t.use_var("x");
        t.use_var("x");
        t.use_var("x");
        let errs = t.check();
        assert!(errs.is_empty());
    }

    #[test]
    fn tracker_get_count() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Linear, span());
        assert_eq!(t.get_count("x"), Some(0));
        t.use_var("x");
        assert_eq!(t.get_count("x"), Some(1));
        assert_eq!(t.get_count("unknown"), None);
    }

    #[test]
    fn tracker_use_undeclared_is_noop() {
        let mut t = UsageTracker::new();
        t.use_var("unknown"); // should not panic
        let errs = t.check();
        assert!(errs.is_empty());
    }

    // ---- LinearContext ----

    #[test]
    fn ctx_fork_merge_consistent() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Linear, span());
        let mut ctx = LinearContext::new(t);

        let (mut a, mut b) = ctx.fork();
        a.use_var("x");
        b.use_var("x");

        let errs = ctx.merge(&a, &b);
        assert!(errs.is_empty()); // Both branches use x once: consistent
    }

    #[test]
    fn ctx_fork_merge_inconsistent() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Linear, span());
        let mut ctx = LinearContext::new(t);

        let (mut a, b) = ctx.fork();
        a.use_var("x");
        // b does not use x

        let errs = ctx.merge(&a, &b);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A05004");
    }

    #[test]
    fn ctx_merge_arms_consistent() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Linear, span());
        let mut ctx = LinearContext::new(t);

        let mut arm1 = ctx.clone();
        let mut arm2 = ctx.clone();
        let mut arm3 = ctx.clone();
        arm1.use_var("x");
        arm2.use_var("x");
        arm3.use_var("x");

        let errs = ctx.merge_arms(&[arm1, arm2, arm3]);
        assert!(errs.is_empty());
    }

    #[test]
    fn ctx_merge_arms_inconsistent() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Linear, span());
        let mut ctx = LinearContext::new(t);

        let mut arm1 = ctx.clone();
        let arm2 = ctx.clone(); // does not use x
        arm1.use_var("x");

        let errs = ctx.merge_arms(&[arm1, arm2]);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A05004");
    }

    #[test]
    fn ctx_merge_arms_empty() {
        let t = UsageTracker::new();
        let mut ctx = LinearContext::new(t);
        let errs = ctx.merge_arms(&[]);
        assert!(errs.is_empty());
    }

    // ---- check_expr_linearity ----

    #[test]
    fn linearity_ident_records_use() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Linear, span());
        let mut ctx = LinearContext::new(t);
        let errs = check_expr_linearity(&ident("x"), &mut ctx);
        assert!(errs.is_empty());
        assert_eq!(ctx.get_count("x"), Some(1));
    }

    #[test]
    fn linearity_if_forks_context() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Linear, span());
        let mut ctx = LinearContext::new(t);
        let expr = Spanned::no_span(Expr::If {
            cond: Box::new(Spanned::no_span(Expr::Literal(Literal::Bool(true)))),
            then_branch: Box::new(ident("x")),
            else_branch: Some(Box::new(ident("x"))),
        });
        let errs = check_expr_linearity(&expr, &mut ctx);
        assert!(errs.is_empty()); // Used in both branches
    }

    #[test]
    fn linearity_if_one_branch_only() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Linear, span());
        let mut ctx = LinearContext::new(t);
        let expr = Spanned::no_span(Expr::If {
            cond: Box::new(Spanned::no_span(Expr::Literal(Literal::Bool(true)))),
            then_branch: Box::new(ident("x")),
            else_branch: Some(Box::new(int_lit(0))),
        });
        let errs = check_expr_linearity(&expr, &mut ctx);
        assert!(!errs.is_empty());
        assert!(errs.iter().any(|e| e.code.as_ref() == "A05004"));
    }

    #[test]
    fn linearity_old_does_not_count() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Linear, span());
        let mut ctx = LinearContext::new(t);
        let expr = Spanned::no_span(Expr::Old(Box::new(ident("x"))));
        let errs = check_expr_linearity(&expr, &mut ctx);
        assert!(errs.is_empty());
        assert_eq!(ctx.get_count("x"), Some(0)); // old() is ghost, not counted
    }

    #[test]
    fn linearity_ghost_does_not_count() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Linear, span());
        let mut ctx = LinearContext::new(t);
        let expr = Spanned::no_span(Expr::Ghost(Box::new(ident("x"))));
        let errs = check_expr_linearity(&expr, &mut ctx);
        assert!(errs.is_empty());
        assert_eq!(ctx.get_count("x"), Some(0));
    }

    #[test]
    fn linearity_quantifier_does_not_count() {
        let mut t = UsageTracker::new();
        t.declare("x".into(), UsageGrade::Linear, span());
        let mut ctx = LinearContext::new(t);
        let expr = Spanned::no_span(Expr::Forall {
            var: "i".into(),
            domain: Box::new(ident("x")),
            body: Box::new(Spanned::no_span(Expr::Literal(Literal::Bool(true)))),
        });
        let errs = check_expr_linearity(&expr, &mut ctx);
        assert!(errs.is_empty());
        assert_eq!(ctx.get_count("x"), Some(0));
    }

    #[test]
    fn usage_grade_display() {
        assert_eq!(UsageGrade::Erased.to_string(), "erased (grade 0)");
        assert_eq!(UsageGrade::Linear.to_string(), "linear (grade 1)");
        assert_eq!(UsageGrade::Exact(3).to_string(), "exact (grade 3)");
        assert!(UsageGrade::Unlimited.to_string().contains("unlimited"));
    }
}
