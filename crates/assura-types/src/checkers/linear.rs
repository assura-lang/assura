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
    usages: HashMap<std::string::String, (UsageGrade, u32, Range<usize>)>,
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
    pub fn declare(&mut self, name: std::string::String, grade: UsageGrade, span: Range<usize>) {
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

    /// Get the declaration span for a variable.
    pub fn get_span(&self, name: &str) -> Option<Range<usize>> {
        self.usages.get(name).map(|(_, _, span)| span.clone())
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

    /// Get the current usage count for a variable in this context.
    pub fn get_count(&self, name: &str) -> Option<u32> {
        self.tracker.get_count(name)
    }

    /// Get the declaration span for a variable in this context.
    pub fn get_span(&self, name: &str) -> Option<Range<usize>> {
        self.tracker.get_span(name)
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
pub(crate) fn check_expr_linearity(expr: &Expr, ctx: &mut LinearContext) -> Vec<TypeError> {
    let mut errors = Vec::new();
    check_expr_linearity_inner(expr, ctx, &mut errors);
    errors
}

/// Inner recursive walker for `check_expr_linearity`.
fn check_expr_linearity_inner(expr: &Expr, ctx: &mut LinearContext, errors: &mut Vec<TypeError>) {
    match expr {
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
        Expr::Paren(inner) => {
            check_expr_linearity_inner(inner, ctx, errors);
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
        Expr::Apply { args, .. } => {
            // Apply expressions are erased at runtime (like ghost).
            // Arguments are verified but do not count as linear uses.
            let _ = args;
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
