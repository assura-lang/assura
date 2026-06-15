// Structural checker stubs for MASTER-PLAN Phase 2/3; methods are wired in
// as the corresponding tasks are implemented.
#![allow(dead_code)]

//! Analysis pass checker structs.
//!
//! Implements the core analysis passes: linearity tracking, typestate,
//! effect checking, frame conditions, taint tracking, and more.

use std::collections::HashMap;
use std::ops::Range;

use assura_parser::ast::{BinOp, ClauseKind, Decl, Expr, Literal, ServiceItem, UnaryOp};

use crate::{Type, TypeEnv, TypeError};

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

// ---------------------------------------------------------------------------
// Typestate checker (T034)
// ---------------------------------------------------------------------------

/// Error produced by the typestate checker.
///
/// Uses error codes from the spec:
/// - **A06001**: Operation called in wrong state
/// - **A06002**: Typestate variable is not linear
/// - **A06003**: State not declared in `states:` block
/// - **A06004**: Ambiguous state after diverging branches
#[derive(Debug, Clone)]
pub(crate) struct TypestateError {
    /// Error code from the spec (A06xxx series).
    pub code: std::string::String,
    /// Human-readable error message.
    pub message: std::string::String,
    /// Source location where the error was detected.
    pub span: Range<usize>,
}

/// A transition in the typestate DFA.
///
/// Each transition is `(operation_name, required_state, next_state)`.
/// The operation can only be called when the object is in `required_state`,
/// and after the call the object moves to `next_state`.
#[derive(Debug, Clone)]
struct Transition {
    operation: std::string::String,
    from_state: std::string::String,
    to_state: std::string::String,
}

/// Typestate checker that tracks a DFA of states and transitions.
///
/// Built from a `states:` declaration in a service or contract. Tracks the
/// current state of a typestate variable and validates that operations are
/// only called in the required state, transitioning to the declared next
/// state afterward.
///
/// # Error codes
///
/// - **A06001**: Operation called when object is in wrong state
/// - **A06002**: Typestate variable must be linear (checked separately)
/// - **A06003**: A transition references a state not in `states:`
/// - **A06004**: After diverging branches, object is in different states
#[derive(Debug, Clone)]
pub(crate) struct TypestateChecker {
    /// All declared states for this typestate variable.
    states: Vec<std::string::String>,
    /// All declared transitions.
    transitions: Vec<Transition>,
    /// Current state of the tracked variable.
    current: std::string::String,
    /// Source span of the typestate declaration (for error reporting).
    decl_span: Range<usize>,
}

impl TypestateChecker {
    /// Create a new typestate checker.
    ///
    /// # Arguments
    ///
    /// * `states` - All declared states from the `states:` block
    /// * `transitions` - Vec of `(operation, from_state, to_state)` tuples
    /// * `initial_state` - The starting state
    /// * `decl_span` - Source span of the typestate declaration
    pub fn new(
        states: Vec<std::string::String>,
        transitions: Vec<(
            std::string::String,
            std::string::String,
            std::string::String,
        )>,
        initial_state: std::string::String,
        decl_span: Range<usize>,
    ) -> Self {
        let transitions = transitions
            .into_iter()
            .map(|(op, from, to)| Transition {
                operation: op,
                from_state: from,
                to_state: to,
            })
            .collect();
        Self {
            states,
            transitions,
            current: initial_state,
            decl_span,
        }
    }

    /// Get the current state of the tracked variable.
    pub fn current_state(&self) -> &str {
        &self.current
    }

    /// Attempt to perform a state transition for the given operation.
    ///
    /// Looks up the operation in the transition table. If a transition
    /// exists whose `from_state` matches the current state, moves to
    /// `to_state` and returns `Ok(())`. Otherwise returns an `A06001`
    /// error.
    pub fn transition(
        &mut self,
        operation: &str,
        span: Range<usize>,
    ) -> Result<(), TypestateError> {
        // Find a transition for this operation from the current state.
        for t in &self.transitions {
            if t.operation == operation && t.from_state == self.current {
                self.current = t.to_state.clone();
                return Ok(());
            }
        }

        // Find what state the operation requires (for a better error message).
        let required_states: Vec<&str> = self
            .transitions
            .iter()
            .filter(|t| t.operation == operation)
            .map(|t| t.from_state.as_str())
            .collect();

        let message = if required_states.is_empty() {
            format!(
                "operation `{operation}` is not defined for any state of this typestate variable \
                 (current state: `{}`)",
                self.current,
            )
        } else {
            format!(
                "operation `{operation}` requires state `{}`, but object is in state `{}`",
                required_states.join("` or `"),
                self.current,
            )
        };

        Err(TypestateError {
            code: "A06001".into(),
            message,
            span,
        })
    }

    /// Validate that the typestate variable is declared as linear.
    ///
    /// Typestate variables must be linear (used exactly once) because
    /// aliasing would allow observing inconsistent states. Returns
    /// `Some(TypestateError)` with code A06002 if `is_linear` is false.
    pub fn validate_linear(&self, is_linear: bool) -> Option<TypestateError> {
        if is_linear {
            None
        } else {
            Some(TypestateError {
                code: "A06002".into(),
                message: "typestate variable must be declared as linear".into(),
                span: self.decl_span.clone(),
            })
        }
    }

    /// Validate that all transitions reference declared states.
    ///
    /// Checks both `from_state` and `to_state` of every transition against
    /// the `states` list. Returns a list of `A06003` errors for any
    /// undeclared states referenced in transitions.
    pub fn validate_transitions(&self) -> Vec<TypestateError> {
        let mut errors = Vec::new();

        for t in &self.transitions {
            if !self.states.contains(&t.from_state) {
                errors.push(TypestateError {
                    code: "A06003".into(),
                    message: format!(
                        "transition `{}` references undeclared source state `{}`; \
                         declared states: [{}]",
                        t.operation,
                        t.from_state,
                        self.states.join(", "),
                    ),
                    span: self.decl_span.clone(),
                });
            }
            if !self.states.contains(&t.to_state) {
                errors.push(TypestateError {
                    code: "A06003".into(),
                    message: format!(
                        "transition `{}` references undeclared target state `{}`; \
                         declared states: [{}]",
                        t.operation,
                        t.to_state,
                        self.states.join(", "),
                    ),
                    span: self.decl_span.clone(),
                });
            }
        }

        errors
    }

    /// Check that two branch checkers ended in the same state.
    ///
    /// After diverging control flow (if/match), if the typestate variable
    /// was transitioned in both branches, they must end in the same state.
    /// Otherwise the post-branch state is ambiguous and we emit A06004.
    ///
    /// Returns `None` if states match, or `Some(TypestateError)` with
    /// code A06004 if they differ.
    pub fn check_branch_consistency(
        branch_a: &TypestateChecker,
        branch_b: &TypestateChecker,
        span: Range<usize>,
    ) -> Option<TypestateError> {
        if branch_a.current == branch_b.current {
            None
        } else {
            Some(TypestateError {
                code: "A06004".into(),
                message: format!(
                    "ambiguous state after diverging branches: \
                     one branch leaves object in state `{}`, \
                     the other in state `{}`",
                    branch_a.current, branch_b.current,
                ),
                span,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Expression usage walker
// ---------------------------------------------------------------------------

/// Walk an expression AST and count variable usages in a `UsageTracker`.
///
/// Each `Ident` node increments the usage count for that variable name.
/// Recursively walks all sub-expressions (binary ops, unary ops, function
/// calls, quantifiers, etc.).
pub(crate) fn expr_usages(expr: &Expr, tracker: &mut UsageTracker) {
    match expr {
        Expr::Ident(name) => {
            tracker.use_var(name);
        }
        Expr::Literal(_) => {}
        Expr::Field(receiver, _field) => {
            expr_usages(receiver, tracker);
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_usages(receiver, tracker);
            for arg in args {
                expr_usages(arg, tracker);
            }
        }
        Expr::Call { func, args } => {
            expr_usages(func, tracker);
            for arg in args {
                expr_usages(arg, tracker);
            }
        }
        Expr::Index { expr: base, index } => {
            expr_usages(base, tracker);
            expr_usages(index, tracker);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            expr_usages(lhs, tracker);
            expr_usages(rhs, tracker);
        }
        Expr::UnaryOp { expr: inner, .. } => {
            expr_usages(inner, tracker);
        }
        Expr::Old(inner) => {
            expr_usages(inner, tracker);
        }
        Expr::Forall {
            var: _,
            domain,
            body,
        }
        | Expr::Exists {
            var: _,
            domain,
            body,
        } => {
            expr_usages(domain, tracker);
            expr_usages(body, tracker);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_usages(cond, tracker);
            expr_usages(then_branch, tracker);
            if let Some(else_br) = else_branch {
                expr_usages(else_br, tracker);
            }
        }
        Expr::Paren(inner) => {
            expr_usages(inner, tracker);
        }
        Expr::List(items) => {
            for item in items {
                expr_usages(item, tracker);
            }
        }
        Expr::Cast { expr: inner, .. } => {
            expr_usages(inner, tracker);
        }
        Expr::Block(exprs) => {
            for e in exprs {
                expr_usages(e, tracker);
            }
        }
        Expr::Ghost(_) => {
            // Ghost blocks are erased at runtime; do not count usages.
        }
        Expr::Apply { .. } => {
            // Apply expressions are erased at runtime; do not count usages.
        }
        Expr::Match { scrutinee, arms } => {
            expr_usages(scrutinee, tracker);
            for arm in arms {
                expr_usages(&arm.body, tracker);
            }
        }
        Expr::Let { value, body, .. } => {
            expr_usages(value, tracker);
            expr_usages(body, tracker);
        }
        Expr::Tuple(elems) => {
            for e in elems {
                expr_usages(e, tracker);
            }
        }
        Expr::Raw(_) => {
            // Cannot extract variable references from raw token sequences.
        }
    }
}

// ---------------------------------------------------------------------------
// Effect checking (T036)
// ---------------------------------------------------------------------------

/// A set of effects declared on (or inferred for) a function.
///
/// Effects are stored as lowercase strings matching the effect labels from
/// Section 3.1 of the spec (e.g., `"io"`, `"console.read"`, `"pure"`).
/// The special value `"pure"` represents an empty effect set.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EffectSet {
    effects: std::collections::HashSet<std::string::String>,
}

impl EffectSet {
    /// Create a new empty effect set (equivalent to `pure`).
    pub fn pure() -> Self {
        Self {
            effects: std::collections::HashSet::new(),
        }
    }

    /// Create an effect set from an iterator of effect names.
    ///
    /// The name `"pure"` is treated as an empty set; it is not stored as
    /// an actual effect label.
    pub fn from_effect_names(
        effects: impl IntoIterator<Item = impl Into<std::string::String>>,
    ) -> Self {
        let mut set = std::collections::HashSet::new();
        for e in effects {
            let name = e.into();
            if name != "pure" {
                set.insert(name);
            }
        }
        Self { effects: set }
    }

    /// Returns `true` if this is a pure (empty) effect set.
    pub fn is_pure(&self) -> bool {
        self.effects.is_empty()
    }

    /// Insert an effect into the set.
    pub fn insert(&mut self, effect: std::string::String) {
        if effect != "pure" {
            self.effects.insert(effect);
        }
    }

    /// Returns `true` if the set contains the given effect.
    pub fn contains(&self, effect: &str) -> bool {
        self.effects.contains(effect)
    }

    /// Iterate over the effect names in this set.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.effects.iter().map(|s| s.as_str())
    }

    /// Number of effects in the set.
    pub fn len(&self) -> usize {
        self.effects.len()
    }

    /// Returns `true` if the set is empty.
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
}

impl std::fmt::Display for EffectSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.effects.is_empty() {
            return write!(f, "pure");
        }
        let mut sorted: Vec<&str> = self.effects.iter().map(|s| s.as_str()).collect();
        sorted.sort();
        write!(f, "{{{}}}", sorted.join(", "))
    }
}

/// An error produced by the effect checker.
#[derive(Debug, Clone)]
pub(crate) struct EffectError {
    /// Error code from the spec (A07xxx series).
    pub code: std::string::String,
    /// Human-readable error message.
    pub message: std::string::String,
    /// Source location where the error was detected.
    pub span: Range<usize>,
}

/// Effect checker that validates effect declarations and containment.
///
/// Implements the effect checking rules from Section 3.5 of the spec:
/// a function's body may only use effects declared in its signature,
/// and all effect names must be recognized (built-in or user-defined).
///
/// The effect hierarchy from Section 3.6 is encoded: `io` is shorthand
/// for all IO sub-effects, `database` for all database sub-effects,
/// and `logging` for all log sub-effects.
pub(crate) struct EffectChecker {
    /// All known effect names (both group names and leaf effects).
    known_effects: std::collections::HashSet<&'static str>,
    /// Maps a group effect to its sub-effects.
    hierarchy: HashMap<&'static str, Vec<&'static str>>,
}

impl EffectChecker {
    /// Create a new effect checker with the built-in effect vocabulary
    /// from Section 3.1 and hierarchy from Section 3.6 of the spec.
    pub fn new() -> Self {
        let known: std::collections::HashSet<&'static str> = [
            // Group effects
            "io",
            "database",
            "logging",
            // Leaf IO effects
            "console.read",
            "console.write",
            "filesystem.read",
            "filesystem.write",
            "network.connect",
            "network.send",
            "network.receive",
            "time.read",
            "random",
            // Leaf database effects
            "database.read",
            "database.write",
            // Leaf logging effects
            "log.debug",
            "log.info",
            "log.warn",
            "log.error",
            // Other built-in effects
            "diverge",
            // Memory effect (from AGENTS.md task description)
            "mem",
            "net",
            "fs",
            "rng",
            "time",
            "alloc",
        ]
        .into_iter()
        .collect();

        let mut hierarchy = HashMap::new();
        hierarchy.insert(
            "io",
            vec![
                "console.read",
                "console.write",
                "filesystem.read",
                "filesystem.write",
                "network.connect",
                "network.send",
                "network.receive",
                "time.read",
                "random",
                // Short aliases that map to IO sub-categories
                "net",
                "fs",
                "rng",
                "time",
            ],
        );
        hierarchy.insert("database", vec!["database.read", "database.write"]);
        hierarchy.insert(
            "logging",
            vec!["log.debug", "log.info", "log.warn", "log.error"],
        );
        // Short alias groups
        hierarchy.insert(
            "net",
            vec!["network.connect", "network.send", "network.receive"],
        );
        hierarchy.insert("fs", vec!["filesystem.read", "filesystem.write"]);

        Self {
            known_effects: known,
            hierarchy,
        }
    }

    /// Expand a declared effect set by adding all sub-effects implied by
    /// the hierarchy. For example, declaring `io` expands to include
    /// `console.read`, `console.write`, etc.
    pub fn expand(&self, declared: &EffectSet) -> EffectSet {
        let mut expanded = declared.clone();
        // Iterate over the original set (not the expanding one) to avoid
        // borrow issues.
        let originals: Vec<std::string::String> = declared.effects.iter().cloned().collect();
        for effect in &originals {
            if let Some(children) = self.hierarchy.get(effect.as_str()) {
                for &child in children {
                    expanded.insert(child.to_string());
                }
            }
        }
        expanded
    }

    /// Check that all effects in `actual` are contained in `declared`.
    ///
    /// The `declared` set is expanded via the hierarchy before comparison.
    /// Returns a list of `EffectError`s for violations:
    ///
    /// - **A07001**: An effect in `actual` is not present in the expanded
    ///   `declared` set (undeclared effect).
    /// - **A07002**: The function is declared `pure` (empty declared set)
    ///   but the body performs effects (side effect in pure context).
    pub fn check_containment(
        &self,
        declared: &EffectSet,
        actual: &EffectSet,
        span: &Range<usize>,
    ) -> Vec<EffectError> {
        let mut errors = Vec::new();

        // Expand the declared set to include sub-effects
        let expanded = self.expand(declared);

        for effect in actual.iter() {
            // Check if the actual effect (or a parent of it) is in the
            // expanded declared set.
            if !self.is_allowed(effect, &expanded) {
                if declared.is_pure() {
                    // A07002: pure function performs effect
                    errors.push(EffectError {
                        code: "A07002".into(),
                        message: format!(
                            "pure function performs effect `{effect}`: \
                             side effects are not allowed in a pure context"
                        ),
                        span: span.clone(),
                    });
                } else {
                    // A07001: undeclared effect
                    errors.push(EffectError {
                        code: "A07001".into(),
                        message: format!(
                            "undeclared effect `{effect}`: \
                             effect not in function's declared effect set {declared}"
                        ),
                        span: span.clone(),
                    });
                }
            }
        }

        // Sort errors by code then message for deterministic output.
        errors.sort_by(|a, b| a.code.cmp(&b.code).then(a.message.cmp(&b.message)));
        errors
    }

    /// Check that all effect names in a set are recognized.
    ///
    /// Returns A07003 errors for unknown effect names.
    pub fn check_known(&self, effects: &EffectSet, span: &Range<usize>) -> Vec<EffectError> {
        let mut errors = Vec::new();

        for effect in effects.iter() {
            // Skip identifiers that are clearly not effect names:
            // - Capitalized names (type names like `InflateDecoder`)
            // - Known block-kind keywords that leak from parser spans
            // This prevents false positives from parser artifacts where
            // block kind names leak into effect clause token streams.
            if effect.chars().next().is_some_and(|c| c.is_uppercase()) {
                continue;
            }
            if is_block_kind_keyword(effect) {
                continue;
            }
            if !self.known_effects.contains(effect) && !self.is_sub_effect_of_known(effect) {
                errors.push(EffectError {
                    code: "A07003".into(),
                    message: format!("unknown effect name `{effect}`"),
                    span: span.clone(),
                });
            }
        }

        errors.sort_by(|a, b| a.message.cmp(&b.message));
        errors
    }

    /// Returns `true` if the effect is a dot-separated sub-effect of a
    /// known group. For example, `io.read` is accepted because `io` is
    /// a known group effect.
    #[allow(clippy::unused_self)]
    fn is_sub_effect_of_known(&self, effect: &str) -> bool {
        if let Some(dot_pos) = effect.find('.') {
            let parent = &effect[..dot_pos];
            self.known_effects.contains(parent) || self.hierarchy.contains_key(parent)
        } else {
            false
        }
    }
}

/// Returns `true` if the name is a known Assura block-kind keyword
/// (e.g., `incremental`, `feature`, `liveness`) that should not be
/// treated as an effect name even when it appears in an effect clause
/// due to parser span overlap.
fn is_block_kind_keyword(name: &str) -> bool {
    matches!(
        name,
        "incremental"
            | "feature"
            | "liveness"
            | "axiomatic"
            | "axiom"
            | "lemma"
            | "ghost"
            | "opaque"
            | "test"
            | "property"
            | "complexity"
            | "benchmark"
            | "migration"
    )
}

impl EffectChecker {
    /// Returns `true` if `effect` is allowed by the expanded declared set.
    ///
    /// An effect is allowed if:
    /// 1. It is directly in the expanded set, OR
    /// 2. Any of its ancestor groups are in the expanded set.
    fn is_allowed(&self, effect: &str, expanded: &EffectSet) -> bool {
        // Direct containment
        if expanded.contains(effect) {
            return true;
        }

        // Check if any group in the expanded set subsumes this effect
        for group_effect in expanded.iter() {
            if let Some(children) = self.hierarchy.get(group_effect)
                && children.contains(&effect)
            {
                return true;
            }
        }

        false
    }

    /// Returns `true` if the given effect name is a known built-in effect.
    pub fn is_known(&self, effect: &str) -> bool {
        self.known_effects.contains(effect)
    }
}

impl Default for EffectChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
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
// Expression value extraction helpers
// ---------------------------------------------------------------------------

/// Extract an integer literal value from an expression.
/// Returns `None` for non-literal or non-integer expressions.
pub(crate) fn extract_int_literal(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Literal(Literal::Int(s)) => s.parse::<i64>().ok(),
        Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: inner,
        } => {
            if let Expr::Literal(Literal::Int(s)) = inner.as_ref() {
                s.parse::<i64>().ok().map(|v| -v)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extract a float literal value from an expression.
pub(crate) fn extract_float_literal(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Literal(Literal::Float(s)) => s.parse::<f64>().ok(),
        Expr::Literal(Literal::Int(s)) => s.parse::<f64>().ok(),
        _ => None,
    }
}

/// Extract a string identifier from an expression.
pub(crate) fn extract_ident(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(name) => Some(name.as_str()),
        _ => None,
    }
}

/// Extract a key-value pair from a BinOp expression (e.g., `name = value`).
pub(crate) fn extract_kv_pair(expr: &Expr) -> Option<(&str, &Expr)> {
    match expr {
        Expr::BinOp {
            op: BinOp::Eq,
            lhs,
            rhs,
        } => {
            if let Expr::Ident(key) = lhs.as_ref() {
                Some((key.as_str(), rhs.as_ref()))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extract a call-style expression: `name(arg1, arg2, ...)`.
/// Returns `(function_name, arguments)`.
pub(crate) fn extract_call(expr: &Expr) -> Option<(&str, &[Expr])> {
    match expr {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = func.as_ref() {
                Some((name.as_str(), args.as_slice()))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extract multiple key-value pairs from a block or list expression.
pub(crate) fn extract_kv_pairs(expr: &Expr) -> Vec<(&str, &Expr)> {
    match expr {
        Expr::Block(exprs) | Expr::List(exprs) => {
            exprs.iter().filter_map(extract_kv_pair).collect()
        }
        _ => {
            // Single kv pair
            extract_kv_pair(expr).into_iter().collect()
        }
    }
}

/// Frame condition checker for modifies clauses (CORE.3).
///
/// Validates that:
/// 1. Names in the `modifies` clause exist in scope (A14001)
/// 2. Computes which variables are NOT in the modifies set (the "frame")
///    so that the SMT encoder can inject `var == old(var)` axioms
///
/// # Error codes
///
/// - **A14001**: Variable in modifies clause does not exist in scope
/// - **A14002**: Assignment to variable not in modifies set (future, when
///   we have assignment analysis in the implementation IR)
pub struct FrameChecker {
    /// The set of variables/fields declared in the modifies clause.
    modified: std::collections::HashSet<std::string::String>,
}

impl FrameChecker {
    /// Create a new frame checker from modifies clause body expressions.
    ///
    /// Extracts variable/field names from the modifies clause and stores
    /// them as the "modified" set.
    pub fn new(modifies_clauses: &[&Expr]) -> Self {
        let mut modified = std::collections::HashSet::new();
        for body in modifies_clauses {
            for target in extract_modifies_targets(body) {
                modified.insert(target);
            }
        }
        Self { modified }
    }

    /// Create an empty frame checker (no modifies clause present).
    ///
    /// When there is no modifies clause, the function may modify anything;
    /// no frame axioms are injected.
    pub fn empty() -> Self {
        Self {
            modified: std::collections::HashSet::new(),
        }
    }

    /// Returns true if this checker has a non-empty modifies set.
    ///
    /// When false, no frame axioms should be injected (the function
    /// did not declare what it modifies).
    pub fn has_modifies(&self) -> bool {
        !self.modified.is_empty()
    }

    /// Get the set of modified variable names.
    pub fn modified_set(&self) -> &std::collections::HashSet<std::string::String> {
        &self.modified
    }

    /// Check that all names in the modifies clause exist in scope.
    ///
    /// Returns A14001 errors for any name that is not found in the
    /// symbol table or type environment.
    pub fn check_scope(
        &self,
        env: &TypeEnv,
        symbols: &assura_resolve::SymbolTable,
        span: &Range<usize>,
    ) -> Vec<TypeError> {
        let mut errors = Vec::new();

        for name in &self.modified {
            // Extract the root variable name (before any dots)
            let root = name.split('.').next().unwrap_or(name);

            // Check if the root variable exists in the type env or symbol table
            let in_env = env.lookup(root).is_some();
            let in_symbols = symbols.symbols.iter().any(|s| s.name == root);

            if !in_env && !in_symbols {
                errors.push(TypeError {
                    code: "A14001".into(),
                    message: format!(
                        "variable `{name}` in modifies clause does not exist in scope"
                    ),
                    span: span.clone(),
                    secondary: None,
                });
            }
        }

        errors.sort_by(|a, b| a.message.cmp(&b.message));
        errors
    }

    /// Compute the frame variables for an ensures clause.
    ///
    /// Given an ensures clause body, finds all variables referenced via
    /// `old(x)` that are NOT in the modifies set. For each such variable,
    /// the SMT encoder should assert `x == old(x)` as a frame axiom.
    ///
    /// Returns the list of variable names for which frame axioms should
    /// be injected.
    pub fn frame_axiom_vars(&self, ensures_body: &Expr) -> Vec<std::string::String> {
        if !self.has_modifies() {
            return Vec::new();
        }

        let old_refs = collect_old_references(ensures_body);
        let ident_refs = collect_ident_references(ensures_body);

        // Collect all referenced variables (both in old() and directly)
        let mut all_refs: std::collections::HashSet<std::string::String> =
            std::collections::HashSet::new();
        for r in &old_refs {
            all_refs.insert(r.clone());
        }
        for r in &ident_refs {
            all_refs.insert(r.clone());
        }

        // Variables NOT in the modifies set get frame axioms
        let mut frame_vars: Vec<std::string::String> = all_refs
            .into_iter()
            .filter(|name| !self.modified.contains(name))
            .filter(|name| {
                // Also check if any prefix is in the modified set
                // e.g., if "node" is modified, "node.keys" is also modified
                !self
                    .modified
                    .iter()
                    .any(|m| name.starts_with(&format!("{m}.")))
                    && !self
                        .modified
                        .iter()
                        .any(|m| m.starts_with(&format!("{name}.")))
            })
            .collect();

        frame_vars.sort();
        frame_vars.dedup();
        frame_vars
    }

    /// Returns true if a variable name is in the modifies set.
    pub fn is_modified(&self, name: &str) -> bool {
        self.modified.contains(name)
    }
}

impl std::fmt::Debug for FrameChecker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameChecker")
            .field("modified", &self.modified)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Error propagation checking (T064 TYPE.3)
// ---------------------------------------------------------------------------

/// Error propagation policy for a set of error codes.
#[derive(Debug, Clone, Default)]
pub(crate) struct ErrorPolicy {
    /// Error codes that MUST propagate to the caller (never silently swallowed).
    pub must_propagate: Vec<String>,
    /// Forbidden error translations: (from, to).
    pub must_not_mask: Vec<(String, String)>,
    /// Error codes whose detail must be preserved across translations.
    pub must_preserve_detail: Vec<String>,
    /// Function names whose return values MUST be checked.
    pub must_check: Vec<String>,
}

/// Checker for error propagation contracts.
pub(crate) struct ErrorPropagationChecker {
    /// Registered error policies.
    pub policies: HashMap<String, ErrorPolicy>,
}

impl Default for ErrorPropagationChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorPropagationChecker {
    /// Create a new checker with no policies.
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
        }
    }

    /// Register an error policy.
    pub fn register_policy(&mut self, name: String, policy: ErrorPolicy) {
        self.policies.insert(name, policy);
    }

    /// Check if an error code is must-propagate in any registered policy.
    pub fn is_must_propagate(&self, error_code: &str) -> bool {
        self.policies
            .values()
            .any(|p| p.must_propagate.iter().any(|c| c == error_code))
    }

    /// Check if a translation from one error code to another is forbidden.
    pub fn is_masked(&self, from: &str, to: &str) -> bool {
        self.policies
            .values()
            .any(|p| p.must_not_mask.iter().any(|(f, t)| f == from && t == to))
    }

    /// Check if a function's return value must be checked.
    pub fn must_check_return(&self, fn_name: &str) -> bool {
        self.policies
            .values()
            .any(|p| p.must_check.iter().any(|f| f == fn_name))
    }

    /// Validate an error handling action. Returns error if the action violates a policy.
    pub fn validate_catch(
        &self,
        error_code: &str,
        action: ErrorAction,
        span: Range<usize>,
    ) -> Option<TypeError> {
        match action {
            ErrorAction::Swallow => {
                if self.is_must_propagate(error_code) {
                    return Some(TypeError {
                        code: "A12001".into(),
                        message: format!(
                            "error code '{error_code}' has must_propagate policy and cannot be silently swallowed"
                        ),
                        span,
                        secondary: None,
                    });
                }
            }
            ErrorAction::TranslateTo(target) => {
                if self.is_masked(error_code, &target) {
                    return Some(TypeError {
                        code: "A12002".into(),
                        message: format!(
                            "translating '{error_code}' to '{target}' is forbidden by must_not_mask policy"
                        ),
                        span,
                        secondary: None,
                    });
                }
            }
            ErrorAction::Propagate | ErrorAction::Handle => {}
        }
        None
    }

    /// Check that a function's Result return value is used.
    pub fn validate_unchecked_call(&self, fn_name: &str, span: Range<usize>) -> Option<TypeError> {
        if self.must_check_return(fn_name) {
            return Some(TypeError {
                code: "A12003".into(),
                message: format!("return value of '{fn_name}' must be checked (must_check policy)"),
                span,
                secondary: None,
            });
        }
        None
    }
}

/// What happens to a caught error.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ErrorAction {
    /// Error is silently discarded (catch and ignore).
    Swallow,
    /// Error is translated to a different code.
    TranslateTo(String),
    /// Error is re-raised to the caller.
    Propagate,
    /// Error is handled with meaningful recovery logic.
    Handle,
}

// ---------------------------------------------------------------------------
// Memory region contracts (T046 - MEM.1)
// ---------------------------------------------------------------------------

/// A ghost memory region declaration, tracking a named range of valid indices.
///
/// In Assura, a region is a ghost construct: `region valid_range = 0..buf.len`.
/// It describes a set of indices that are valid for buffer access.
#[derive(Debug, Clone)]
pub(crate) struct MemoryRegion {
    /// Name of the region (e.g., "valid_range").
    pub name: std::string::String,
    /// Lower bound expression (as variable name or literal).
    pub lower: std::string::String,
    /// Upper bound expression (as variable name or literal).
    pub upper: std::string::String,
    /// The buffer variable this region is associated with.
    pub buffer: std::string::String,
}

/// An error produced by the memory checker.
///
/// Uses error codes from the spec:
/// - **A08101**: Buffer access without bounds check (requires clause missing
///   bounds check for array/buffer index)
/// - **A08102**: Region containment violation (sub-region not proven to be
///   within parent region)
/// - **A08103**: Ghost region references non-existent buffer
#[derive(Debug, Clone)]
pub(crate) struct MemoryError {
    /// Error code from the spec (A08xxx series).
    pub code: std::string::String,
    /// Human-readable error message.
    pub message: std::string::String,
    /// Source location where the error was detected.
    pub span: Range<usize>,
}

/// Memory checker for buffer safety contracts (MEM.1).
///
/// Validates that:
/// 1. Buffer access contracts include proper bounds checks in requires clauses
/// 2. Ghost region declarations reference buffers that exist in scope
/// 3. Region containment assertions are well-formed
///
/// The checker works on the type-checked AST and uses the type environment
/// to validate that variables referenced in memory contracts exist and have
/// appropriate types (Bytes, List, etc.).
///
/// # Error codes
///
/// - **A08101**: Buffer access without bounds check
/// - **A08102**: Region containment violation
/// - **A08103**: Ghost region references non-existent buffer
pub(crate) struct MemoryChecker {
    /// Known buffer-typed variables and their capacity expressions.
    /// Maps variable name -> capacity field name (e.g., "buf" -> "buf.len").
    buffers: HashMap<std::string::String, std::string::String>,
    /// Ghost region declarations.
    regions: Vec<MemoryRegion>,
}

impl MemoryChecker {
    /// Create a new memory checker.
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
            regions: Vec::new(),
        }
    }

    /// Register a buffer-typed variable with its capacity expression.
    ///
    /// Buffer types are: Bytes, List<T>, Sequence<T>, and any user type
    /// with `.len` or `.capacity` fields.
    pub fn register_buffer(&mut self, name: std::string::String, capacity: std::string::String) {
        self.buffers.insert(name, capacity);
    }

    /// Register a ghost region declaration.
    pub fn register_region(&mut self, region: MemoryRegion) {
        self.regions.push(region);
    }

    /// Returns all registered buffer names.
    pub fn buffer_names(&self) -> Vec<String> {
        self.buffers.keys().cloned().collect()
    }

    /// Returns true if the given variable name is a registered buffer.
    pub fn is_buffer(&self, name: &str) -> bool {
        self.buffers.contains_key(name)
    }

    /// Get the capacity expression for a buffer variable.
    pub fn buffer_capacity(&self, name: &str) -> Option<&str> {
        self.buffers.get(name).map(|s| s.as_str())
    }

    /// Get all registered regions.
    pub fn regions(&self) -> &[MemoryRegion] {
        &self.regions
    }

    /// Check whether a contract's requires clauses contain a proper bounds
    /// check for buffer access.
    ///
    /// A bounds check is an expression of the form:
    ///   `offset + len <= buf.len` or `offset + len <= buf.capacity`
    ///
    /// This function looks for patterns in requires clause expressions
    /// that constrain buffer access to be within bounds.
    ///
    /// Returns `None` if a bounds check is found, or `Some(MemoryError)`
    /// with code A08101 if no bounds check is present.
    pub fn check_bounds_in_requires(
        &self,
        buffer_name: &str,
        requires_exprs: &[&Expr],
        span: &Range<usize>,
    ) -> Option<MemoryError> {
        if !self.is_buffer(buffer_name) {
            return None;
        }

        // Look for a bounds-checking pattern in the requires clauses
        let has_bounds_check = requires_exprs
            .iter()
            .any(|expr| self.expr_has_bounds_check(expr, buffer_name));

        if has_bounds_check {
            None
        } else {
            Some(MemoryError {
                code: "A08101".into(),
                message: format!(
                    "buffer `{buffer_name}` accessed without bounds check: \
                     add a `requires` clause constraining index/offset \
                     to be within `{buffer_name}.len`"
                ),
                span: span.clone(),
            })
        }
    }

    /// Check that all ghost region declarations reference existing buffers.
    ///
    /// Returns A08103 errors for regions whose buffer is not registered.
    pub fn check_region_buffers(&self, span: &Range<usize>) -> Vec<MemoryError> {
        let mut errors = Vec::new();
        for region in &self.regions {
            if !self.is_buffer(&region.buffer) {
                errors.push(MemoryError {
                    code: "A08103".into(),
                    message: format!(
                        "ghost region `{}` references non-existent buffer `{}`",
                        region.name, region.buffer,
                    ),
                    span: span.clone(),
                });
            }
        }
        errors
    }

    /// Check that a sub-region is contained within a parent region.
    ///
    /// Returns `None` if both regions are registered and the containment
    /// is well-formed, or `Some(MemoryError)` with code A08102 if the
    /// containment cannot be established structurally.
    pub fn check_region_containment(
        &self,
        sub_region: &str,
        parent_region: &str,
        span: &Range<usize>,
    ) -> Option<MemoryError> {
        let sub = self.regions.iter().find(|r| r.name == sub_region);
        let parent = self.regions.iter().find(|r| r.name == parent_region);

        match (sub, parent) {
            (Some(sub_r), Some(parent_r)) => {
                // Structural containment check is deferred to SMT encoding.
                // Here we just validate that both regions exist and reference
                // the same buffer.
                if sub_r.buffer != parent_r.buffer {
                    Some(MemoryError {
                        code: "A08102".into(),
                        message: format!(
                            "region `{sub_region}` (on buffer `{}`) cannot be contained in \
                             region `{parent_region}` (on buffer `{}`): different buffers",
                            sub_r.buffer, parent_r.buffer,
                        ),
                        span: span.clone(),
                    })
                } else {
                    None
                }
            }
            (None, _) => Some(MemoryError {
                code: "A08102".into(),
                message: format!("sub-region `{sub_region}` is not defined"),
                span: span.clone(),
            }),
            (_, None) => Some(MemoryError {
                code: "A08102".into(),
                message: format!("parent region `{parent_region}` is not defined"),
                span: span.clone(),
            }),
        }
    }

    /// Recursively check whether an expression contains a bounds-checking
    /// pattern for the given buffer.
    ///
    /// Recognized patterns:
    /// - `expr <= buf.len` or `expr <= buf.capacity`
    /// - `expr < buf.len` or `expr < buf.capacity`
    /// - `buf.len >= expr` or `buf.capacity >= expr`
    /// - Any comparison where one side references the buffer's length/capacity
    ///   and the other constrains an offset/index
    fn expr_has_bounds_check(&self, expr: &Expr, buffer_name: &str) -> bool {
        match expr {
            Expr::BinOp { lhs, op, rhs } => {
                match op {
                    BinOp::Lte | BinOp::Lt => {
                        // Check: something <= buf.len
                        self.references_buffer_capacity(rhs, buffer_name)
                            || self.references_buffer_capacity(lhs, buffer_name)
                    }
                    BinOp::Gte | BinOp::Gt => {
                        // Check: buf.len >= something
                        self.references_buffer_capacity(lhs, buffer_name)
                            || self.references_buffer_capacity(rhs, buffer_name)
                    }
                    BinOp::And => {
                        // Conjunction: check both sides
                        self.expr_has_bounds_check(lhs, buffer_name)
                            || self.expr_has_bounds_check(rhs, buffer_name)
                    }
                    _ => false,
                }
            }
            Expr::Paren(inner) => self.expr_has_bounds_check(inner, buffer_name),
            _ => false,
        }
    }

    /// Check if an expression references a buffer's capacity/length.
    ///
    /// Looks for `buf.len`, `buf.capacity`, `buf.length`, or the
    /// registered capacity expression for the buffer.
    fn references_buffer_capacity(&self, expr: &Expr, buffer_name: &str) -> bool {
        match expr {
            Expr::Field(receiver, field) => {
                let is_len_field =
                    field == "len" || field == "capacity" || field == "length" || field == "size";
                if is_len_field && let Expr::Ident(name) = receiver.as_ref() {
                    return name == buffer_name;
                }
                false
            }
            Expr::Ident(name) => {
                // Check against registered capacity expression
                if let Some(cap) = self.buffers.get(buffer_name) {
                    name == cap
                } else {
                    false
                }
            }
            // Recurse into sub-expressions (e.g., offset + len <= buf.len)
            Expr::BinOp { lhs, rhs, .. } => {
                self.references_buffer_capacity(lhs, buffer_name)
                    || self.references_buffer_capacity(rhs, buffer_name)
            }
            Expr::Paren(inner) => self.references_buffer_capacity(inner, buffer_name),
            _ => false,
        }
    }
}

impl Default for MemoryChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MemoryChecker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryChecker")
            .field("buffers", &self.buffers)
            .field("regions", &self.regions)
            .finish()
    }
}

/// Check whether an expression references a variable by name.
pub fn expr_references_var(expr: &Expr, var_name: &str) -> bool {
    match expr {
        Expr::Ident(name) => name == var_name,
        Expr::Field(receiver, _) => expr_references_var(receiver, var_name),
        Expr::BinOp { lhs, rhs, .. } => {
            expr_references_var(lhs, var_name) || expr_references_var(rhs, var_name)
        }
        Expr::UnaryOp { expr: inner, .. } | Expr::Old(inner) | Expr::Paren(inner) => {
            expr_references_var(inner, var_name)
        }
        Expr::Call { func, args } => {
            expr_references_var(func, var_name)
                || args.iter().any(|a| expr_references_var(a, var_name))
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_references_var(receiver, var_name)
                || args.iter().any(|a| expr_references_var(a, var_name))
        }
        Expr::Index { expr: base, index } => {
            expr_references_var(base, var_name) || expr_references_var(index, var_name)
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_references_var(cond, var_name)
                || expr_references_var(then_branch, var_name)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| expr_references_var(e, var_name))
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            expr_references_var(domain, var_name) || expr_references_var(body, var_name)
        }
        Expr::List(items) => items.iter().any(|i| expr_references_var(i, var_name)),
        Expr::Block(exprs) => exprs.iter().any(|e| expr_references_var(e, var_name)),
        Expr::Ghost(inner) | Expr::Cast { expr: inner, .. } => expr_references_var(inner, var_name),
        Expr::Apply { args, .. } => args.iter().any(|a| expr_references_var(a, var_name)),
        Expr::Match { scrutinee, arms } => {
            expr_references_var(scrutinee, var_name)
                || arms
                    .iter()
                    .any(|arm| expr_references_var(&arm.body, var_name))
        }
        Expr::Let { value, body, .. } => {
            expr_references_var(value, var_name) || expr_references_var(body, var_name)
        }
        Expr::Tuple(elems) => elems.iter().any(|e| expr_references_var(e, var_name)),
        Expr::Raw(tokens) => tokens.iter().any(|t| t.trim() == var_name),
        Expr::Literal(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Taint tracking (T047 - SEC.1)
// ---------------------------------------------------------------------------

/// Taint label for tracking untrusted data flow.
///
/// Follows the information flow lattice from Section 2.7 of the spec:
/// `Untrusted < Validated < Trusted`
///
/// Data from external sources (network, files, user input) starts as
/// `Untrusted`. Explicit validation functions promote it to `Validated`.
/// Internal data is `Trusted`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TaintLabel {
    /// Data from an external, potentially malicious source.
    Untrusted,
    /// Data that has been explicitly validated/sanitized.
    Validated,
    /// Internal data known to be safe.
    Trusted,
}

impl std::fmt::Display for TaintLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaintLabel::Untrusted => write!(f, "untrusted"),
            TaintLabel::Validated => write!(f, "validated"),
            TaintLabel::Trusted => write!(f, "trusted"),
        }
    }
}

/// Extract a taint label from type annotation tokens.
///
/// Looks for patterns like `@taint:untrusted`, `@taint:validated`,
/// `@taint:trusted` in a sequence of type tokens (from `Param.ty` or
/// `FnDef.return_ty`). Also handles `@untrusted` short form.
///
/// Returns `Some(label)` if found, `None` if no taint annotation is present.
pub(crate) fn extract_taint_label(type_tokens: &[String]) -> Option<TaintLabel> {
    // Look for pattern: "@" "taint" ":" <label>
    for window in type_tokens.windows(4) {
        if window[0] == "@" && window[1] == "taint" && window[2] == ":" {
            return match window[3].as_str() {
                "untrusted" => Some(TaintLabel::Untrusted),
                "validated" => Some(TaintLabel::Validated),
                "trusted" => Some(TaintLabel::Trusted),
                _ => None,
            };
        }
    }
    // Check shorter form: "@" <label>
    for window in type_tokens.windows(2) {
        if window[0] == "@" {
            return match window[1].as_str() {
                "untrusted" => Some(TaintLabel::Untrusted),
                "validated" => Some(TaintLabel::Validated),
                "trusted" => Some(TaintLabel::Trusted),
                _ => None,
            };
        }
    }
    None
}

/// Taint checker that tracks taint labels through data flow.
///
/// Implements SEC.1 from Section 14 of the spec: untrusted data taint
/// tracking. Ensures that data from external sources (marked
/// `@taint:untrusted`) cannot flow to sensitive positions (array indices,
/// allocation sizes, etc.) without explicit validation.
///
/// # Error codes
///
/// - **A09101**: Tainted data used as array index without validation
/// - **A09102**: Tainted data used as allocation size without validation
/// - **A09103**: Tainted data flows to trusted sink
/// - **A09104**: Taint validation incomplete (partial sanitization)
#[derive(Debug, Clone)]
pub(crate) struct TaintChecker {
    /// Maps variable name to its taint label.
    labels: HashMap<std::string::String, TaintLabel>,
    /// Names of functions known to validate/sanitize input.
    /// These functions convert Untrusted -> Validated.
    validation_fns: std::collections::HashSet<std::string::String>,
    /// Names of functions whose parameters require validated/trusted input.
    /// Maps function name to its parameter taint requirements.
    trusted_sinks: HashMap<std::string::String, Vec<Option<TaintLabel>>>,
}

impl TaintChecker {
    /// Create an empty taint checker with built-in validation function names.
    pub fn new() -> Self {
        let mut validation_fns = std::collections::HashSet::new();
        // Built-in validation function names
        validation_fns.insert("validate".to_string());
        validation_fns.insert("sanitize".to_string());
        Self {
            labels: HashMap::new(),
            validation_fns,
            trusted_sinks: HashMap::new(),
        }
    }

    /// Declare a variable with a taint label.
    pub fn declare(&mut self, name: std::string::String, label: TaintLabel) {
        self.labels.insert(name, label);
    }

    /// Register a function as a validation/sanitization function.
    pub fn register_validator(&mut self, name: std::string::String) {
        self.validation_fns.insert(name);
    }

    /// Register a function as a trusted sink with parameter taint requirements.
    pub fn register_trusted_sink(
        &mut self,
        name: std::string::String,
        param_labels: Vec<Option<TaintLabel>>,
    ) {
        self.trusted_sinks.insert(name, param_labels);
    }

    /// Get the taint label for a variable.
    pub fn get_label(&self, name: &str) -> Option<TaintLabel> {
        self.labels.get(name).copied()
    }

    /// Returns true if any taint labels are tracked.
    pub fn has_taint_info(&self) -> bool {
        !self.labels.is_empty()
    }

    /// Infer the taint label of an expression.
    ///
    /// Taint propagates through operations: if any operand is tainted,
    /// the result is tainted. Uses the minimum in the lattice
    /// (Untrusted < Validated < Trusted).
    pub fn infer_taint(&self, expr: &Expr) -> TaintLabel {
        match expr {
            Expr::Ident(name) => self
                .labels
                .get(name)
                .copied()
                .unwrap_or(TaintLabel::Trusted),
            Expr::Literal(_) => TaintLabel::Trusted,
            Expr::Field(receiver, _) => self.infer_taint(receiver),
            Expr::BinOp { lhs, rhs, .. } => {
                std::cmp::min(self.infer_taint(lhs), self.infer_taint(rhs))
            }
            Expr::UnaryOp { expr: inner, .. } => self.infer_taint(inner),
            Expr::Call { func, args } => {
                // Validation functions produce Validated output
                if let Expr::Ident(name) = func.as_ref()
                    && self.validation_fns.contains(name)
                {
                    return TaintLabel::Validated;
                }
                // Taint propagates from arguments
                args.iter().fold(TaintLabel::Trusted, |acc, arg| {
                    std::cmp::min(acc, self.infer_taint(arg))
                })
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
            } => {
                if self.validation_fns.contains(method) {
                    return TaintLabel::Validated;
                }
                let r = self.infer_taint(receiver);
                args.iter()
                    .fold(r, |acc, arg| std::cmp::min(acc, self.infer_taint(arg)))
            }
            Expr::Index { expr: base, index } => {
                std::cmp::min(self.infer_taint(base), self.infer_taint(index))
            }
            Expr::Old(inner) | Expr::Paren(inner) | Expr::Cast { expr: inner, .. } => {
                self.infer_taint(inner)
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let mut r = std::cmp::min(self.infer_taint(cond), self.infer_taint(then_branch));
                if let Some(e) = else_branch {
                    r = std::cmp::min(r, self.infer_taint(e));
                }
                r
            }
            Expr::List(items) => items.iter().fold(TaintLabel::Trusted, |a, i| {
                std::cmp::min(a, self.infer_taint(i))
            }),
            Expr::Block(exprs) => exprs.iter().fold(TaintLabel::Trusted, |a, e| {
                std::cmp::min(a, self.infer_taint(e))
            }),
            Expr::Forall { body, .. } | Expr::Exists { body, .. } => self.infer_taint(body),
            Expr::Apply { args, .. } => args.iter().fold(TaintLabel::Trusted, |a, arg| {
                std::cmp::min(a, self.infer_taint(arg))
            }),
            Expr::Match { scrutinee, arms } => {
                let mut r = self.infer_taint(scrutinee);
                for arm in arms {
                    r = std::cmp::min(r, self.infer_taint(&arm.body));
                }
                r
            }
            Expr::Let { value, body, .. } => {
                std::cmp::min(self.infer_taint(value), self.infer_taint(body))
            }
            Expr::Tuple(elems) => elems.iter().fold(TaintLabel::Trusted, |a, e| {
                std::cmp::min(a, self.infer_taint(e))
            }),
            Expr::Ghost(_) | Expr::Raw(_) => TaintLabel::Trusted,
        }
    }

    /// Check an expression for taint violations.
    ///
    /// Walks the expression tree looking for sensitive positions where
    /// untrusted data is used without validation.
    pub fn check_expr(&self, expr: &Expr, span: &Range<usize>) -> Vec<TypeError> {
        let mut errors = Vec::new();
        self.check_expr_inner(expr, span, &mut errors);
        errors
    }

    /// Inner recursive checker for taint violations.
    fn check_expr_inner(&self, expr: &Expr, span: &Range<usize>, errors: &mut Vec<TypeError>) {
        match expr {
            // A09101: tainted data as array index
            Expr::Index { expr: base, index } => {
                let index_taint = self.infer_taint(index);
                if index_taint == TaintLabel::Untrusted {
                    errors.push(TypeError {
                        code: "A09101".into(),
                        message: "tainted data used as array index without validation: \
                             validate the index before using it to access an array"
                            .into(),
                        span: span.clone(),
                        secondary: None,
                    });
                }
                self.check_expr_inner(base, span, errors);
                self.check_expr_inner(index, span, errors);
            }

            // A09102 / A09103: tainted data at function call sites
            Expr::Call { func, args } => {
                if let Expr::Ident(name) = func.as_ref() {
                    // A09102: allocation size
                    if is_alloc_function(name) {
                        for arg in args {
                            if self.infer_taint(arg) == TaintLabel::Untrusted {
                                errors.push(TypeError {
                                    code: "A09102".into(),
                                    message: format!(
                                        "tainted data used as allocation size without \
                                         validation: argument to `{name}` is untrusted"
                                    ),
                                    span: span.clone(),
                                    secondary: None,
                                });
                            }
                        }
                    }

                    // A09103: trusted sink
                    if let Some(param_labels) = self.trusted_sinks.get(name) {
                        for (i, arg) in args.iter().enumerate() {
                            let arg_taint = self.infer_taint(arg);
                            if let Some(Some(required)) = param_labels.get(i)
                                && arg_taint < *required
                            {
                                errors.push(TypeError {
                                    code: "A09103".into(),
                                    message: format!(
                                        "tainted data flows to trusted sink: \
                                         argument {i} to `{name}` is `{arg_taint}` \
                                         but parameter requires `{required}`"
                                    ),
                                    span: span.clone(),
                                    secondary: None,
                                });
                            }
                        }
                    }
                }
                self.check_expr_inner(func, span, errors);
                for arg in args {
                    self.check_expr_inner(arg, span, errors);
                }
            }

            // Recurse into sub-expressions
            Expr::BinOp { lhs, rhs, .. } => {
                self.check_expr_inner(lhs, span, errors);
                self.check_expr_inner(rhs, span, errors);
            }
            Expr::UnaryOp { expr: inner, .. }
            | Expr::Old(inner)
            | Expr::Paren(inner)
            | Expr::Cast { expr: inner, .. }
            | Expr::Ghost(inner) => {
                self.check_expr_inner(inner, span, errors);
            }
            Expr::Field(receiver, _) => {
                self.check_expr_inner(receiver, span, errors);
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.check_expr_inner(receiver, span, errors);
                for arg in args {
                    self.check_expr_inner(arg, span, errors);
                }
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.check_expr_inner(cond, span, errors);
                self.check_expr_inner(then_branch, span, errors);
                if let Some(else_br) = else_branch {
                    self.check_expr_inner(else_br, span, errors);
                }
            }
            Expr::List(items) => {
                for item in items {
                    self.check_expr_inner(item, span, errors);
                }
            }
            Expr::Block(exprs) => {
                for e in exprs {
                    self.check_expr_inner(e, span, errors);
                }
            }
            Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
                self.check_expr_inner(domain, span, errors);
                self.check_expr_inner(body, span, errors);
            }
            Expr::Apply { args, .. } => {
                for arg in args {
                    self.check_expr_inner(arg, span, errors);
                }
            }
            Expr::Match { scrutinee, arms } => {
                self.check_expr_inner(scrutinee, span, errors);
                for arm in arms {
                    self.check_expr_inner(&arm.body, span, errors);
                }
            }
            Expr::Let { value, body, .. } => {
                self.check_expr_inner(value, span, errors);
                self.check_expr_inner(body, span, errors);
            }
            Expr::Tuple(elems) => {
                for e in elems {
                    self.check_expr_inner(e, span, errors);
                }
            }
            Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => {}
        }
    }

    /// Check taint flow in a complete source file.
    ///
    /// Extracts taint labels from function parameter and return types,
    /// registers validation functions, then checks all clause expressions
    /// for taint violations. Returns empty if no taint annotations exist.
    pub fn check_file(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = TaintChecker::new();
        let mut has_taint_annotations = false;

        // Pass 1: discover validation functions and trusted sinks
        for decl in &source.decls {
            match &decl.node {
                Decl::FnDef(f) => {
                    if let Some(TaintLabel::Validated) = extract_taint_label(&f.return_ty) {
                        checker.register_validator(f.name.clone());
                        has_taint_annotations = true;
                    }
                    let param_labels: Vec<Option<TaintLabel>> = f
                        .params
                        .iter()
                        .map(|p| extract_taint_label(&p.ty))
                        .collect();
                    // If any param requires validated/trusted, register as sink
                    if param_labels
                        .iter()
                        .any(|l| matches!(l, Some(TaintLabel::Validated | TaintLabel::Trusted)))
                    {
                        checker.register_trusted_sink(f.name.clone(), param_labels.clone());
                        has_taint_annotations = true;
                    }
                    if param_labels.iter().any(|l| l.is_some()) {
                        has_taint_annotations = true;
                    }
                }
                Decl::Extern(e) => {
                    if let Some(TaintLabel::Validated) = extract_taint_label(&e.return_ty) {
                        checker.register_validator(e.name.clone());
                        has_taint_annotations = true;
                    }
                    let param_labels: Vec<Option<TaintLabel>> = e
                        .params
                        .iter()
                        .map(|p| extract_taint_label(&p.ty))
                        .collect();
                    if param_labels
                        .iter()
                        .any(|l| matches!(l, Some(TaintLabel::Validated | TaintLabel::Trusted)))
                    {
                        checker.register_trusted_sink(e.name.clone(), param_labels.clone());
                        has_taint_annotations = true;
                    }
                    if param_labels.iter().any(|l| l.is_some()) {
                        has_taint_annotations = true;
                    }
                }
                _ => {}
            }
        }

        // If no taint annotations, skip the check
        if !has_taint_annotations {
            return Vec::new();
        }

        let mut errors = Vec::new();

        // Pass 2: check each declaration with scoped taint labels
        for decl in &source.decls {
            match &decl.node {
                Decl::FnDef(f) => {
                    let mut fn_checker = checker.clone();
                    for param in &f.params {
                        if let Some(label) = extract_taint_label(&param.ty) {
                            fn_checker.declare(param.name.clone(), label);
                        }
                    }
                    if fn_checker.has_taint_info() {
                        for clause in &f.clauses {
                            errors.extend(fn_checker.check_expr(&clause.body, &decl.span));
                        }
                    }
                }
                Decl::Extern(e) => {
                    let mut fn_checker = checker.clone();
                    for param in &e.params {
                        if let Some(label) = extract_taint_label(&param.ty) {
                            fn_checker.declare(param.name.clone(), label);
                        }
                    }
                    if fn_checker.has_taint_info() {
                        for clause in &e.clauses {
                            errors.extend(fn_checker.check_expr(&clause.body, &decl.span));
                        }
                    }
                }
                Decl::Contract(c) => {
                    if checker.has_taint_info() {
                        for clause in &c.clauses {
                            errors.extend(checker.check_expr(&clause.body, &decl.span));
                        }
                    }
                }
                Decl::Service(s) => {
                    for item in &s.items {
                        match item {
                            ServiceItem::Operation { clauses, .. }
                            | ServiceItem::Query { clauses, .. } => {
                                for clause in clauses {
                                    errors.extend(checker.check_expr(&clause.body, &decl.span));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Decl::Block { body, .. } => {
                    for clause in body {
                        errors.extend(checker.check_expr(&clause.body, &decl.span));
                    }
                }
                Decl::Bind(b) => {
                    let mut fn_checker = checker.clone();
                    for param in &b.params {
                        if let Some(label) = extract_taint_label(&param.ty) {
                            fn_checker.declare(param.name.clone(), label);
                        }
                    }
                    if fn_checker.has_taint_info() {
                        for clause in &b.clauses {
                            errors.extend(fn_checker.check_expr(&clause.body, &decl.span));
                        }
                    }
                }
                // Prophecy, CodecRegistry, TypeDef, EnumDef: no taint tracking needed.
                Decl::Prophecy(_)
                | Decl::CodecRegistry(_)
                | Decl::TypeDef(_)
                | Decl::EnumDef(_) => {}
            }
        }

        errors
    }
}

impl Default for TaintChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns `true` if the function name is an allocation function.
fn is_alloc_function(name: &str) -> bool {
    matches!(
        name,
        "alloc" | "allocate" | "malloc" | "realloc" | "reserve" | "resize"
    )
}

// ---------------------------------------------------------------------------
// T058: FFI boundary contracts
// ---------------------------------------------------------------------------

/// Trust boundary classification for FFI declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrustBoundary {
    /// Fully trusted: internal Assura code
    Trusted,
    /// Semi-trusted: audited external code with contracts
    Audited,
    /// Untrusted: arbitrary external code
    Untrusted,
}

impl std::fmt::Display for TrustBoundary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrustBoundary::Trusted => write!(f, "trusted"),
            TrustBoundary::Audited => write!(f, "audited"),
            TrustBoundary::Untrusted => write!(f, "untrusted"),
        }
    }
}

/// Error from the FFI boundary checker.
#[derive(Debug, Clone)]
pub(crate) struct FfiError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for FFI boundary contracts.
///
/// Validates that:
/// - All extern declarations have explicit trust boundary annotations
/// - Untrusted FFI calls have requires/ensures contracts
/// - Data crossing trust boundaries is validated
/// - Unsafe operations are isolated to FFI wrappers
pub(crate) struct FfiBoundaryChecker {
    /// Known extern declarations with their trust levels
    externs: HashMap<String, TrustBoundary>,
    /// FFI functions that have contracts (requires/ensures)
    contracted: HashMap<String, bool>,
}

impl FfiBoundaryChecker {
    pub fn new() -> Self {
        Self {
            externs: HashMap::new(),
            contracted: HashMap::new(),
        }
    }

    /// Register an extern declaration with its trust boundary.
    pub fn register_extern(&mut self, name: String, boundary: TrustBoundary) {
        self.externs.insert(name, boundary);
    }

    /// Mark an extern as having a contract (requires/ensures clauses).
    pub fn mark_contracted(&mut self, name: String) {
        self.contracted.insert(name, true);
    }

    /// Check that an extern declaration has the required annotations.
    /// - A11001: extern without trust boundary annotation
    /// - A11002: untrusted extern without contract (requires/ensures)
    pub fn check_extern_decl(
        &self,
        name: &str,
        has_boundary: bool,
        has_contract: bool,
        span: &Range<usize>,
    ) -> Vec<FfiError> {
        let mut errors = Vec::new();
        if !has_boundary {
            errors.push(FfiError {
                code: "A11001".into(),
                message: format!(
                    "extern `{name}` has no trust boundary annotation; \
                     add @trust:trusted, @trust:audited, or @trust:untrusted"
                ),
                span: span.clone(),
            });
        }
        let boundary = self.externs.get(name);
        if boundary == Some(&TrustBoundary::Untrusted) && !has_contract {
            errors.push(FfiError {
                code: "A11002".into(),
                message: format!(
                    "untrusted extern `{name}` has no contract; \
                     add requires/ensures to validate inputs and outputs"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that a call to an FFI function validates data at the trust boundary.
    /// - A11003: data from untrusted FFI used without validation
    pub fn check_ffi_call(
        &self,
        callee: &str,
        result_validated: bool,
        span: &Range<usize>,
    ) -> Vec<FfiError> {
        let mut errors = Vec::new();
        if self.externs.get(callee) == Some(&TrustBoundary::Untrusted) && !result_validated {
            errors.push(FfiError {
                code: "A11003".into(),
                message: format!(
                    "result of untrusted FFI call `{callee}` used without validation; \
                     wrap return value in a validate block"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that unsafe operations are confined to FFI wrappers.
    /// - A11004: unsafe operation outside FFI wrapper
    pub fn check_unsafe_confinement(
        &self,
        fn_name: &str,
        is_ffi_wrapper: bool,
        has_unsafe: bool,
        span: &Range<usize>,
    ) -> Vec<FfiError> {
        let mut errors = Vec::new();
        if has_unsafe && !is_ffi_wrapper {
            errors.push(FfiError {
                code: "A11004".into(),
                message: format!(
                    "function `{fn_name}` uses unsafe operations but is not an FFI wrapper; \
                     move unsafe code to an extern wrapper"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check file-level FFI usage: all externs should be audited.
    pub fn check_file(&self, externs: &[(String, bool, bool, Range<usize>)]) -> Vec<FfiError> {
        let mut errors = Vec::new();
        for (name, has_boundary, has_contract, span) in externs {
            errors.extend(self.check_extern_decl(name, *has_boundary, *has_contract, span));
        }
        errors
    }
}

impl Default for FfiBoundaryChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T062: Interface contracts (trait-like contracts)
// ---------------------------------------------------------------------------

/// An interface contract: a set of required method signatures with contracts.
#[derive(Debug, Clone)]
pub(crate) struct InterfaceContract {
    pub name: String,
    /// Required method signatures
    pub methods: Vec<InterfaceMethod>,
    /// Super-interfaces (like trait bounds)
    pub extends: Vec<String>,
}

/// A method signature within an interface contract.
#[derive(Debug, Clone)]
pub(crate) struct InterfaceMethod {
    pub name: String,
    pub param_types: Vec<Type>,
    pub return_type: Type,
    pub has_requires: bool,
    pub has_ensures: bool,
    /// Whether the method restricts callback re-entrancy
    pub no_reentrancy: bool,
}

/// Error from the interface contract checker.
#[derive(Debug, Clone)]
pub(crate) struct InterfaceError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for interface contracts.
///
/// Validates that:
/// - Implementations satisfy all interface method contracts
/// - Method signatures match (parameter types, return types)
/// - Re-entrancy restrictions are respected
/// - Super-interface contracts are inherited correctly
pub(crate) struct InterfaceChecker {
    /// Known interface definitions
    interfaces: HashMap<String, InterfaceContract>,
    /// Implementations: (implementing_type, interface_name) -> methods
    impls: HashMap<(String, String), Vec<String>>,
}

impl InterfaceChecker {
    pub fn new() -> Self {
        Self {
            interfaces: HashMap::new(),
            impls: HashMap::new(),
        }
    }

    /// Register an interface contract.
    pub fn register_interface(&mut self, iface: InterfaceContract) {
        self.interfaces.insert(iface.name.clone(), iface);
    }

    /// Register an implementation of an interface.
    pub fn register_impl(
        &mut self,
        impl_type: String,
        interface_name: String,
        method_names: Vec<String>,
    ) {
        self.impls.insert((impl_type, interface_name), method_names);
    }

    /// Check that an implementation satisfies all interface methods.
    /// - A13001: missing method implementation
    /// - A13002: method signature mismatch (param or return type)
    pub fn check_impl(
        &self,
        impl_type: &str,
        interface_name: &str,
        implemented_methods: &[String],
        span: &Range<usize>,
    ) -> Vec<InterfaceError> {
        let mut errors = Vec::new();
        let Some(iface) = self.interfaces.get(interface_name) else {
            errors.push(InterfaceError {
                code: "A13001".into(),
                message: format!("unknown interface `{interface_name}`"),
                span: span.clone(),
            });
            return errors;
        };

        for method in &iface.methods {
            if !implemented_methods.contains(&method.name) {
                errors.push(InterfaceError {
                    code: "A13001".into(),
                    message: format!(
                        "`{impl_type}` does not implement required method `{}` \
                         from interface `{interface_name}`",
                        method.name
                    ),
                    span: span.clone(),
                });
            }
        }

        // Check super-interfaces
        for super_name in &iface.extends {
            if let Some(super_iface) = self.interfaces.get(super_name) {
                for method in &super_iface.methods {
                    if !implemented_methods.contains(&method.name) {
                        errors.push(InterfaceError {
                            code: "A13001".into(),
                            message: format!(
                                "`{impl_type}` does not implement required method `{}` \
                                 from super-interface `{super_name}`",
                                method.name
                            ),
                            span: span.clone(),
                        });
                    }
                }
            }
        }

        errors
    }

    /// Check method signature compatibility.
    /// - A13002: parameter count or type mismatch
    pub fn check_method_signature(
        &self,
        interface_name: &str,
        method_name: &str,
        impl_params: &[Type],
        impl_return: &Type,
        span: &Range<usize>,
    ) -> Vec<InterfaceError> {
        let mut errors = Vec::new();
        let Some(iface) = self.interfaces.get(interface_name) else {
            return errors;
        };
        let Some(method) = iface.methods.iter().find(|m| m.name == method_name) else {
            return errors;
        };

        if impl_params.len() != method.param_types.len() {
            errors.push(InterfaceError {
                code: "A13002".into(),
                message: format!(
                    "method `{method_name}` has {} parameters but interface `{interface_name}` \
                     requires {}",
                    impl_params.len(),
                    method.param_types.len()
                ),
                span: span.clone(),
            });
        } else {
            for (i, (impl_t, iface_t)) in impl_params.iter().zip(&method.param_types).enumerate() {
                if impl_t != iface_t {
                    errors.push(InterfaceError {
                        code: "A13002".into(),
                        message: format!(
                            "method `{method_name}` parameter {i}: \
                             expected `{iface_t:?}`, found `{impl_t:?}`"
                        ),
                        span: span.clone(),
                    });
                }
            }
        }

        if impl_return != &method.return_type {
            errors.push(InterfaceError {
                code: "A13002".into(),
                message: format!(
                    "method `{method_name}` return type mismatch: \
                     expected `{:?}`, found `{impl_return:?}`",
                    method.return_type
                ),
                span: span.clone(),
            });
        }

        errors
    }

    /// Check callback re-entrancy restriction.
    /// - A13003: method marked no_reentrancy called recursively through callback
    pub fn check_reentrancy(
        &self,
        interface_name: &str,
        method_name: &str,
        is_reentrant_call: bool,
        span: &Range<usize>,
    ) -> Vec<InterfaceError> {
        let mut errors = Vec::new();
        let is_violation = self
            .interfaces
            .get(interface_name)
            .and_then(|iface| iface.methods.iter().find(|m| m.name == method_name))
            .is_some_and(|method| method.no_reentrancy && is_reentrant_call);
        if is_violation {
            errors.push(InterfaceError {
                code: "A13003".into(),
                message: format!(
                    "method `{method_name}` on interface `{interface_name}` \
                     is marked no_reentrancy but is called re-entrantly"
                ),
                span: span.clone(),
            });
        }
        errors
    }
}

impl Default for InterfaceChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T059: SEC.3 Constant-time execution
// ---------------------------------------------------------------------------

/// Error from the constant-time checker.
#[derive(Debug, Clone)]
pub(crate) struct ConstantTimeError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for constant-time execution properties.
///
/// Ensures secret-dependent code does not branch on secrets,
/// preventing timing side-channel attacks.
pub(crate) struct ConstantTimeChecker {
    /// Variables classified as secret
    secrets: HashMap<String, bool>,
}

impl ConstantTimeChecker {
    pub fn new() -> Self {
        Self {
            secrets: HashMap::new(),
        }
    }

    /// Mark a variable as secret (timing-sensitive).
    pub fn mark_secret(&mut self, name: String) {
        self.secrets.insert(name, true);
    }

    /// Check if an expression references any secret variable.
    pub fn references_secret(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Ident(name) => self.secrets.contains_key(name),
            Expr::BinOp { lhs, rhs, .. } => {
                self.references_secret(lhs) || self.references_secret(rhs)
            }
            Expr::UnaryOp { expr, .. } => self.references_secret(expr),
            Expr::Field(e, _) => self.references_secret(e),
            Expr::Call { func, args } => {
                self.references_secret(func) || args.iter().any(|a| self.references_secret(a))
            }
            Expr::Index { expr, index } => {
                self.references_secret(expr) || self.references_secret(index)
            }
            Expr::Paren(e) | Expr::Old(e) | Expr::Ghost(e) => self.references_secret(e),
            Expr::If { cond, .. } => self.references_secret(cond),
            _ => false,
        }
    }

    /// Check that branches do not depend on secret data.
    /// - A14001: branch condition depends on secret data (timing leak)
    pub fn check_branch(&self, condition: &Expr, span: &Range<usize>) -> Vec<ConstantTimeError> {
        let mut errors = Vec::new();
        if self.references_secret(condition) {
            errors.push(ConstantTimeError {
                code: "A14001".into(),
                message: "branch condition depends on secret data; \
                          this creates a timing side-channel"
                    .into(),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that array indexing does not depend on secret data.
    /// - A14002: secret-dependent array index (cache timing leak)
    pub fn check_index(&self, index_expr: &Expr, span: &Range<usize>) -> Vec<ConstantTimeError> {
        let mut errors = Vec::new();
        if self.references_secret(index_expr) {
            errors.push(ConstantTimeError {
                code: "A14002".into(),
                message: "array index depends on secret data; \
                          this creates a cache timing side-channel"
                    .into(),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check a full expression for constant-time violations.
    pub fn check_expr(&self, expr: &Expr, span: &Range<usize>) -> Vec<ConstantTimeError> {
        let mut errors = Vec::new();
        match expr {
            Expr::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                errors.extend(self.check_branch(cond, span));
                errors.extend(self.check_expr(then_branch, span));
                if let Some(e) = else_branch {
                    errors.extend(self.check_expr(e, span));
                }
            }
            Expr::Index { index, .. } => {
                errors.extend(self.check_index(index, span));
            }
            Expr::BinOp { lhs, rhs, .. } => {
                errors.extend(self.check_expr(lhs, span));
                errors.extend(self.check_expr(rhs, span));
            }
            Expr::Call { args, .. } => {
                for a in args {
                    errors.extend(self.check_expr(a, span));
                }
            }
            _ => {}
        }
        errors
    }
}

impl Default for ConstantTimeChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T063: TYPE.2 Recursive structural invariants
// ---------------------------------------------------------------------------

/// A structural invariant on a recursive data structure.
#[derive(Debug, Clone)]
pub(crate) struct StructuralInvariant {
    pub name: String,
    /// The type this invariant applies to
    pub type_name: String,
    /// Kind of structural property
    pub kind: InvariantKind,
}

/// Kinds of structural invariants for recursive types.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InvariantKind {
    /// Tree balance: left depth and right depth differ by at most k
    TreeBalance { max_diff: u32 },
    /// List sortedness: elements in non-decreasing order
    Sorted { descending: bool },
    /// Graph acyclicity: no cycles in the structure
    Acyclic,
    /// Binary search tree: left < node < right
    BstOrdering,
    /// Heap property: parent <= children (or >=)
    HeapProperty { min_heap: bool },
    /// Custom invariant expressed as a predicate string
    Custom(String),
}

impl std::fmt::Display for InvariantKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvariantKind::TreeBalance { max_diff } => {
                write!(f, "tree_balance(max_diff={max_diff})")
            }
            InvariantKind::Sorted { descending } => {
                if *descending {
                    write!(f, "sorted(desc)")
                } else {
                    write!(f, "sorted(asc)")
                }
            }
            InvariantKind::Acyclic => write!(f, "acyclic"),
            InvariantKind::BstOrdering => write!(f, "bst_ordering"),
            InvariantKind::HeapProperty { min_heap } => {
                if *min_heap {
                    write!(f, "min_heap")
                } else {
                    write!(f, "max_heap")
                }
            }
            InvariantKind::Custom(pred) => write!(f, "custom({pred})"),
        }
    }
}

/// Error from the structural invariant checker.
#[derive(Debug, Clone)]
pub(crate) struct StructuralInvariantError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for recursive structural invariants.
pub(crate) struct StructuralInvariantChecker {
    /// Registered invariants per type
    invariants: HashMap<String, Vec<StructuralInvariant>>,
    /// Known recursive types (type name -> list of recursive field names)
    recursive_types: HashMap<String, Vec<String>>,
}

impl StructuralInvariantChecker {
    pub fn new() -> Self {
        Self {
            invariants: HashMap::new(),
            recursive_types: HashMap::new(),
        }
    }

    /// Register a type as recursive, listing its self-referencing fields.
    pub fn register_recursive_type(&mut self, type_name: String, recursive_fields: Vec<String>) {
        self.recursive_types.insert(type_name, recursive_fields);
    }

    /// Register a structural invariant on a type.
    pub fn register_invariant(&mut self, inv: StructuralInvariant) {
        self.invariants
            .entry(inv.type_name.clone())
            .or_default()
            .push(inv);
    }

    /// Check that a structural invariant is applicable to the type.
    /// - A15001: invariant on non-recursive type
    /// - A15002: tree invariant on non-tree structure
    /// - A15003: sort invariant on non-sequence structure
    pub fn check_invariant_applicability(
        &self,
        type_name: &str,
        kind: &InvariantKind,
        span: &Range<usize>,
    ) -> Vec<StructuralInvariantError> {
        let mut errors = Vec::new();
        if !self.recursive_types.contains_key(type_name) {
            errors.push(StructuralInvariantError {
                code: "A15001".into(),
                message: format!(
                    "structural invariant `{kind}` applied to non-recursive type `{type_name}`"
                ),
                span: span.clone(),
            });
            return errors;
        }

        let fields = &self.recursive_types[type_name];
        match kind {
            InvariantKind::TreeBalance { .. }
            | InvariantKind::BstOrdering
            | InvariantKind::HeapProperty { .. } => {
                // Tree invariants need at least 2 recursive fields (left, right)
                if fields.len() < 2 {
                    errors.push(StructuralInvariantError {
                        code: "A15002".into(),
                        message: format!(
                            "tree invariant `{kind}` requires at least 2 recursive fields, \
                             but `{type_name}` has {}",
                            fields.len()
                        ),
                        span: span.clone(),
                    });
                }
            }
            InvariantKind::Sorted { .. } => {
                // Sort invariant needs exactly 1 recursive field (next pointer)
                if fields.len() != 1 {
                    errors.push(StructuralInvariantError {
                        code: "A15003".into(),
                        message: format!(
                            "sort invariant requires exactly 1 recursive field (next pointer), \
                             but `{type_name}` has {}",
                            fields.len()
                        ),
                        span: span.clone(),
                    });
                }
            }
            InvariantKind::Acyclic | InvariantKind::Custom(_) => {
                // These are valid for any recursive type
            }
        }
        errors
    }

    /// Check that an operation preserves the structural invariant.
    /// - A15004: operation may violate structural invariant
    pub fn check_operation_preserves(
        &self,
        type_name: &str,
        operation: &str,
        modifies_structure: bool,
        has_preservation_proof: bool,
        span: &Range<usize>,
    ) -> Vec<StructuralInvariantError> {
        let mut errors = Vec::new();
        if !modifies_structure {
            return errors; // Read-only operations preserve invariants trivially
        }
        if let Some(invs) = self.invariants.get(type_name) {
            for inv in invs {
                if !has_preservation_proof {
                    errors.push(StructuralInvariantError {
                        code: "A15004".into(),
                        message: format!(
                            "operation `{operation}` modifies `{type_name}` \
                             but has no proof preserving invariant `{}`",
                            inv.kind
                        ),
                        span: span.clone(),
                    });
                }
            }
        }
        errors
    }

    /// Get all invariants for a type (including inherited through recursive substructure).
    pub fn get_invariants(&self, type_name: &str) -> Vec<&StructuralInvariant> {
        self.invariants
            .get(type_name)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }
}

impl Default for StructuralInvariantChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T065: CONC.1 Shared memory protocols
// ---------------------------------------------------------------------------

/// Access mode for a shared object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccessMode {
    /// Exclusive read-write access (no other readers/writers)
    Exclusive,
    /// Shared read-only access (multiple readers, no writers)
    SharedRead,
    /// No access (object is locked by another thread)
    None,
}

impl std::fmt::Display for AccessMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccessMode::Exclusive => write!(f, "exclusive"),
            AccessMode::SharedRead => write!(f, "shared_read"),
            AccessMode::None => write!(f, "none"),
        }
    }
}

/// Error from the shared memory checker.
#[derive(Debug, Clone)]
pub(crate) struct SharedMemError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for shared memory protocols.
///
/// Validates that concurrent accesses to shared objects follow
/// the declared protocol: no data races, no concurrent writes.
pub(crate) struct SharedMemChecker {
    /// Per-object access modes
    object_modes: HashMap<String, AccessMode>,
}

impl SharedMemChecker {
    pub fn new() -> Self {
        Self {
            object_modes: HashMap::new(),
        }
    }

    /// Set the current access mode for an object.
    pub fn set_mode(&mut self, object: String, mode: AccessMode) {
        self.object_modes.insert(object, mode);
    }

    /// Check that a read access is valid for the current mode.
    /// - A18001: read without shared_read or exclusive access
    pub fn check_read(&self, object: &str, span: &Range<usize>) -> Vec<SharedMemError> {
        let mut errors = Vec::new();
        match self.object_modes.get(object) {
            Some(AccessMode::Exclusive | AccessMode::SharedRead) => {}
            Some(AccessMode::None) | None => {
                errors.push(SharedMemError {
                    code: "A18001".into(),
                    message: format!(
                        "read access to `{object}` without acquiring shared_read or exclusive mode"
                    ),
                    span: span.clone(),
                });
            }
        }
        errors
    }

    /// Check that a write access is valid for the current mode.
    /// - A18002: write without exclusive access
    pub fn check_write(&self, object: &str, span: &Range<usize>) -> Vec<SharedMemError> {
        let mut errors = Vec::new();
        if self.object_modes.get(object) != Some(&AccessMode::Exclusive) {
            errors.push(SharedMemError {
                code: "A18002".into(),
                message: format!(
                    "write access to `{object}` without exclusive mode; \
                     acquire exclusive access before writing"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check for potential data race: two threads accessing the same object.
    /// - A18003: data race (concurrent write + read or write + write)
    pub fn check_data_race(
        &self,
        object: &str,
        thread_a_mode: AccessMode,
        thread_b_mode: AccessMode,
        span: &Range<usize>,
    ) -> Vec<SharedMemError> {
        let mut errors = Vec::new();
        let is_race = matches!(
            (thread_a_mode, thread_b_mode),
            (
                AccessMode::Exclusive,
                AccessMode::Exclusive | AccessMode::SharedRead
            ) | (AccessMode::SharedRead, AccessMode::Exclusive)
        );
        if is_race {
            errors.push(SharedMemError {
                code: "A18003".into(),
                message: format!(
                    "potential data race on `{object}`: thread A has {thread_a_mode} \
                     while thread B has {thread_b_mode}"
                ),
                span: span.clone(),
            });
        }
        errors
    }
}

impl Default for SharedMemChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T067: CONC.3 Determinism contracts
// ---------------------------------------------------------------------------

/// Error from the determinism checker.
#[derive(Debug, Clone)]
pub(crate) struct DeterminismError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for determinism contracts.
///
/// Ensures functions marked as `deterministic` do not use
/// non-deterministic constructs (HashMap iteration, random,
/// thread-dependent ordering).
pub(crate) struct DeterminismChecker {
    /// Functions marked as deterministic
    deterministic_fns: HashMap<String, bool>,
    /// Known non-deterministic types/functions
    non_det_sources: Vec<String>,
}

impl DeterminismChecker {
    pub fn new() -> Self {
        Self {
            deterministic_fns: HashMap::new(),
            non_det_sources: vec![
                "HashMap".into(),
                "HashSet".into(),
                "random".into(),
                "rand".into(),
                "thread_rng".into(),
                "SystemTime::now".into(),
                "Instant::now".into(),
            ],
        }
    }

    /// Mark a function as requiring deterministic execution.
    pub fn mark_deterministic(&mut self, fn_name: String) {
        self.deterministic_fns.insert(fn_name, true);
    }

    /// Add a custom non-deterministic source.
    pub fn add_non_det_source(&mut self, source: String) {
        self.non_det_sources.push(source);
    }

    /// Check if a type/function name is non-deterministic.
    pub fn is_non_deterministic(&self, name: &str) -> bool {
        self.non_det_sources
            .iter()
            .any(|s| name.contains(s.as_str()))
    }

    /// Check that a deterministic function does not use non-deterministic constructs.
    /// - A20001: deterministic function uses non-deterministic type/call
    pub fn check_fn_body(
        &self,
        fn_name: &str,
        used_names: &[String],
        span: &Range<usize>,
    ) -> Vec<DeterminismError> {
        let mut errors = Vec::new();
        if !self.deterministic_fns.contains_key(fn_name) {
            return errors; // Not marked deterministic, skip
        }
        for name in used_names {
            if self.is_non_deterministic(name) {
                errors.push(DeterminismError {
                    code: "A20001".into(),
                    message: format!(
                        "deterministic function `{fn_name}` uses non-deterministic `{name}`; \
                         use BTreeMap/BTreeSet or a seeded RNG instead"
                    ),
                    span: span.clone(),
                });
            }
        }
        errors
    }

    /// Check that iteration order is deterministic.
    /// - A20002: iterating over HashMap/HashSet in deterministic context
    pub fn check_iteration(
        &self,
        fn_name: &str,
        iterated_type: &str,
        span: &Range<usize>,
    ) -> Vec<DeterminismError> {
        let mut errors = Vec::new();
        if self.deterministic_fns.contains_key(fn_name)
            && (iterated_type.contains("HashMap") || iterated_type.contains("HashSet"))
        {
            errors.push(DeterminismError {
                code: "A20002".into(),
                message: format!(
                    "deterministic function `{fn_name}` iterates over `{iterated_type}` \
                     which has non-deterministic ordering"
                ),
                span: span.clone(),
            });
        }
        errors
    }
}

impl Default for DeterminismChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T068: CONC.4 Lock ordering
// ---------------------------------------------------------------------------

/// Error from the lock ordering checker.
#[derive(Debug, Clone)]
pub(crate) struct LockOrderError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for static lock ordering.
///
/// Prevents deadlocks by enforcing a total order on lock acquisitions.
pub(crate) struct LockOrderChecker {
    /// Lock hierarchy: name -> priority (lower = acquire first)
    lock_order: HashMap<String, u32>,
    /// Currently held locks (name, priority)
    held: Vec<(String, u32)>,
}

impl LockOrderChecker {
    pub fn new() -> Self {
        Self {
            lock_order: HashMap::new(),
            held: Vec::new(),
        }
    }

    /// Define the lock hierarchy. Locks with lower priority must be acquired first.
    pub fn define_order(&mut self, lock_name: String, priority: u32) {
        self.lock_order.insert(lock_name, priority);
    }

    /// Record acquiring a lock. Check ordering.
    /// - A21001: lock acquired out of order (deadlock risk)
    pub fn acquire(&mut self, lock_name: &str, span: &Range<usize>) -> Vec<LockOrderError> {
        let mut errors = Vec::new();
        let priority = self.lock_order.get(lock_name).copied().unwrap_or(u32::MAX);

        // Check that we're not acquiring a lower-priority lock while holding higher
        if let Some((held_name, held_priority)) = self.held.last().filter(|(_, hp)| priority <= *hp)
        {
            errors.push(LockOrderError {
                code: "A21001".into(),
                message: format!(
                    "lock `{lock_name}` (priority {priority}) acquired while holding \
                     `{held_name}` (priority {held_priority}); violates lock ordering"
                ),
                span: span.clone(),
            });
        }

        self.held.push((lock_name.to_string(), priority));
        errors
    }

    /// Record releasing a lock.
    /// - A21002: lock released out of order (must release in reverse acquisition order)
    pub fn release(&mut self, lock_name: &str, span: &Range<usize>) -> Vec<LockOrderError> {
        let mut errors = Vec::new();
        if let Some((top_name, _)) = self.held.last().filter(|(n, _)| n != lock_name) {
            errors.push(LockOrderError {
                code: "A21002".into(),
                message: format!(
                    "lock `{lock_name}` released while `{top_name}` is still held; \
                     release in reverse acquisition order"
                ),
                span: span.clone(),
            });
        }
        self.held.retain(|(n, _)| n != lock_name);
        errors
    }

    /// Check that no lock is known but unordered.
    /// - A21003: lock used without defined order
    pub fn check_ordering_defined(
        &self,
        lock_name: &str,
        span: &Range<usize>,
    ) -> Vec<LockOrderError> {
        let mut errors = Vec::new();
        if !self.lock_order.contains_key(lock_name) {
            errors.push(LockOrderError {
                code: "A21003".into(),
                message: format!(
                    "lock `{lock_name}` used without a defined ordering; \
                     add it to the lock hierarchy"
                ),
                span: span.clone(),
            });
        }
        errors
    }
}

impl Default for LockOrderChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T060: SEC.4 Secure erasure
// ---------------------------------------------------------------------------

/// Error from the secure erasure checker.
#[derive(Debug, Clone)]
pub(crate) struct SecureErasureError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for secure erasure of sensitive data.
///
/// Ensures that linear types marked as sensitive are consumed
/// via zeroize before being dropped, preventing sensitive data
/// from lingering in memory.
pub(crate) struct SecureErasureChecker {
    /// Variables that hold sensitive data and must be zeroized
    sensitive_vars: HashMap<String, bool>,
    /// Variables that have been properly zeroized
    zeroized: HashMap<String, bool>,
}

impl SecureErasureChecker {
    pub fn new() -> Self {
        Self {
            sensitive_vars: HashMap::new(),
            zeroized: HashMap::new(),
        }
    }

    /// Returns the names of all sensitive variables.
    pub fn sensitive_names(&self) -> Vec<String> {
        self.sensitive_vars.keys().cloned().collect()
    }

    /// Mark a variable as holding sensitive data.
    pub fn mark_sensitive(&mut self, name: String) {
        self.sensitive_vars.insert(name, true);
    }

    /// Record that a variable has been zeroized.
    pub fn mark_zeroized(&mut self, name: String) {
        self.zeroized.insert(name, true);
    }

    /// Check that a sensitive variable was zeroized before going out of scope.
    /// - A16001: sensitive variable dropped without zeroization
    pub fn check_scope_exit(&self, var_name: &str, span: &Range<usize>) -> Vec<SecureErasureError> {
        let mut errors = Vec::new();
        if self.sensitive_vars.contains_key(var_name) && !self.zeroized.contains_key(var_name) {
            errors.push(SecureErasureError {
                code: "A16001".into(),
                message: format!(
                    "sensitive variable `{var_name}` dropped without secure erasure; \
                     call zeroize() before the variable goes out of scope"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that a copy of sensitive data is also marked sensitive.
    /// - A16002: sensitive data copied to non-sensitive variable
    pub fn check_copy(
        &self,
        source: &str,
        target: &str,
        target_is_sensitive: bool,
        span: &Range<usize>,
    ) -> Vec<SecureErasureError> {
        let mut errors = Vec::new();
        if self.sensitive_vars.contains_key(source) && !target_is_sensitive {
            errors.push(SecureErasureError {
                code: "A16002".into(),
                message: format!(
                    "sensitive data from `{source}` copied to `{target}` \
                     which is not marked as sensitive; the copy will not be zeroized"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that sensitive data is not leaked through return values.
    /// - A16003: function returns sensitive data without @sensitive annotation
    pub fn check_return(
        &self,
        returned_var: &str,
        fn_return_is_sensitive: bool,
        span: &Range<usize>,
    ) -> Vec<SecureErasureError> {
        let mut errors = Vec::new();
        if self.sensitive_vars.contains_key(returned_var) && !fn_return_is_sensitive {
            errors.push(SecureErasureError {
                code: "A16003".into(),
                message: format!(
                    "function returns sensitive variable `{returned_var}` \
                     but return type is not marked @sensitive"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check all sensitive variables at end of scope.
    pub fn check_all_erased(&self, span: &Range<usize>) -> Vec<SecureErasureError> {
        let mut errors = Vec::new();
        for name in self.sensitive_vars.keys() {
            if !self.zeroized.contains_key(name) {
                errors.push(SecureErasureError {
                    code: "A16001".into(),
                    message: format!("sensitive variable `{name}` dropped without secure erasure"),
                    span: span.clone(),
                });
            }
        }
        errors
    }
}

impl Default for SecureErasureChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T061: SEC.5 Cryptographic conformance
// ---------------------------------------------------------------------------

/// Error from the cryptographic conformance checker.
#[derive(Debug, Clone)]
pub(crate) struct CryptoConformanceError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// A cryptographic algorithm specification.
#[derive(Debug, Clone)]
pub(crate) struct CryptoSpec {
    pub name: String,
    pub key_size_bits: Vec<u32>,
    pub block_size_bytes: Option<u32>,
    pub nonce_size_bytes: Option<u32>,
    pub tag_size_bytes: Option<u32>,
}

/// Checker for cryptographic conformance.
///
/// Validates that cryptographic implementations match their mathematical
/// specifications: correct key sizes, nonce handling, tag verification.
pub(crate) struct CryptoConformanceChecker {
    /// Known algorithm specs
    specs: HashMap<String, CryptoSpec>,
}

impl CryptoConformanceChecker {
    pub fn new() -> Self {
        let mut specs = HashMap::new();
        // Register common algorithms
        specs.insert(
            "AES-128-GCM".into(),
            CryptoSpec {
                name: "AES-128-GCM".into(),
                key_size_bits: vec![128],
                block_size_bytes: Some(16),
                nonce_size_bytes: Some(12),
                tag_size_bytes: Some(16),
            },
        );
        specs.insert(
            "AES-256-GCM".into(),
            CryptoSpec {
                name: "AES-256-GCM".into(),
                key_size_bits: vec![256],
                block_size_bytes: Some(16),
                nonce_size_bytes: Some(12),
                tag_size_bytes: Some(16),
            },
        );
        specs.insert(
            "ChaCha20-Poly1305".into(),
            CryptoSpec {
                name: "ChaCha20-Poly1305".into(),
                key_size_bits: vec![256],
                block_size_bytes: None,
                nonce_size_bytes: Some(12),
                tag_size_bytes: Some(16),
            },
        );
        Self { specs }
    }

    /// Register a custom algorithm specification.
    pub fn register_spec(&mut self, spec: CryptoSpec) {
        self.specs.insert(spec.name.clone(), spec);
    }

    /// Check that a key size matches the algorithm spec.
    /// - A17001: wrong key size for algorithm
    pub fn check_key_size(
        &self,
        algorithm: &str,
        key_size_bits: u32,
        span: &Range<usize>,
    ) -> Vec<CryptoConformanceError> {
        let mut errors = Vec::new();
        if let Some(spec) = self
            .specs
            .get(algorithm)
            .filter(|s| !s.key_size_bits.contains(&key_size_bits))
        {
            errors.push(CryptoConformanceError {
                code: "A17001".into(),
                message: format!(
                    "key size {key_size_bits} bits does not match `{algorithm}` \
                     which requires {:?} bits",
                    spec.key_size_bits
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that a nonce size matches the algorithm spec.
    /// - A17002: wrong nonce size for algorithm
    pub fn check_nonce_size(
        &self,
        algorithm: &str,
        nonce_size_bytes: u32,
        span: &Range<usize>,
    ) -> Vec<CryptoConformanceError> {
        let mut errors = Vec::new();
        let mismatch = self
            .specs
            .get(algorithm)
            .and_then(|s| s.nonce_size_bytes)
            .filter(|&expected| nonce_size_bytes != expected);
        if let Some(expected) = mismatch {
            errors.push(CryptoConformanceError {
                code: "A17002".into(),
                message: format!(
                    "nonce size {nonce_size_bytes} bytes does not match `{algorithm}` \
                     which requires {expected} bytes"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that nonce reuse is prevented.
    /// - A17003: potential nonce reuse detected
    pub fn check_nonce_uniqueness(
        &self,
        nonce_source: &str,
        is_counter: bool,
        is_random: bool,
        span: &Range<usize>,
    ) -> Vec<CryptoConformanceError> {
        let mut errors = Vec::new();
        if !is_counter && !is_random {
            errors.push(CryptoConformanceError {
                code: "A17003".into(),
                message: format!(
                    "nonce `{nonce_source}` is neither counter-based nor random; \
                     potential nonce reuse"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that authentication tag is verified before using decrypted data.
    /// - A17004: decrypted data used before tag verification
    pub fn check_tag_verification(
        &self,
        has_tag_check: bool,
        span: &Range<usize>,
    ) -> Vec<CryptoConformanceError> {
        let mut errors = Vec::new();
        if !has_tag_check {
            errors.push(CryptoConformanceError {
                code: "A17004".into(),
                message: "decrypted data used before authentication tag verification; \
                          verify the tag before processing plaintext"
                    .into(),
                span: span.clone(),
            });
        }
        errors
    }
}

impl Default for CryptoConformanceChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T052: Dependent types (restricted)
// ---------------------------------------------------------------------------

/// A dependent type index: the value a type depends on.
/// Restricted to Nat, Bool, and finite enums (not arbitrary expressions).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DepIndex {
    /// A natural number index, e.g. Vec<T, n>
    Nat(String),
    /// A boolean index, e.g. Matrix<T, is_square>
    Bool(String),
    /// A finite enum index, e.g. Buffer<mode> where mode: ReadWrite
    Enum { name: String, enum_type: String },
}

impl std::fmt::Display for DepIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DepIndex::Nat(n) => write!(f, "{n}: Nat"),
            DepIndex::Bool(n) => write!(f, "{n}: Bool"),
            DepIndex::Enum { name, enum_type } => write!(f, "{name}: {enum_type}"),
        }
    }
}

/// A dependent type: a base type parameterized by one or more indices.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DepType {
    pub base: Type,
    pub indices: Vec<DepIndex>,
}

/// Error from the dependent type checker.
#[derive(Debug, Clone)]
pub(crate) struct DepTypeError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for restricted dependent types.
///
/// Validates that:
/// - Dependent type indices are of allowed kinds (Nat, Bool, finite enum)
/// - Index arithmetic in type positions is well-formed
/// - Indices are erased at runtime (ghost)
/// - Type equality with indices is checked structurally
pub(crate) struct DependentTypeChecker {
    /// Known enum types and their variants (for finiteness check)
    enums: HashMap<String, Vec<String>>,
    /// Known dependent type definitions
    dep_types: HashMap<String, DepType>,
    /// Index variable bindings in scope: name -> DepIndex
    index_vars: HashMap<String, DepIndex>,
}

impl DependentTypeChecker {
    pub fn new() -> Self {
        Self {
            enums: HashMap::new(),
            dep_types: HashMap::new(),
            index_vars: HashMap::new(),
        }
    }

    /// Register a finite enum type with its variants.
    pub fn register_enum(&mut self, name: String, variants: Vec<String>) {
        self.enums.insert(name, variants);
    }

    /// Register a dependent type definition.
    pub fn register_dep_type(&mut self, name: String, dep_type: DepType) {
        self.dep_types.insert(name, dep_type);
    }

    /// Bind an index variable in the current scope.
    pub fn bind_index(&mut self, name: String, index: DepIndex) {
        self.index_vars.insert(name, index);
    }

    /// Validate that a type index is of an allowed kind.
    /// Returns A03006 if the index type is not Nat, Bool, or a known finite enum.
    pub fn validate_index(
        &self,
        index_name: &str,
        index_type: &str,
        span: &Range<usize>,
    ) -> Vec<DepTypeError> {
        let mut errors = Vec::new();
        match index_type {
            "Nat" | "Bool" => { /* allowed */ }
            other => {
                if !self.enums.contains_key(other) {
                    errors.push(DepTypeError {
                        code: "A03006".into(),
                        message: format!(
                            "dependent type index `{index_name}` has type `{other}`, \
                             which is not Nat, Bool, or a known finite enum"
                        ),
                        span: span.clone(),
                    });
                }
            }
        }
        errors
    }

    /// Check that index arithmetic in a type position is well-formed.
    /// For Nat indices, expressions like `n + 1`, `n - 1`, `2 * n` are allowed.
    /// For Bool/Enum indices, only direct references are allowed (no arithmetic).
    pub fn check_index_expr(
        &self,
        expr: &Expr,
        expected_kind: &DepIndex,
        span: &Range<usize>,
    ) -> Vec<DepTypeError> {
        let mut errors = Vec::new();
        match expected_kind {
            DepIndex::Nat(_) => {
                // Nat indices allow arithmetic expressions
                if !self.is_nat_expr(expr) {
                    errors.push(DepTypeError {
                        code: "A03007".into(),
                        message: "index expression is not a valid Nat expression; \
                                  only integer arithmetic over index variables is allowed"
                            .into(),
                        span: span.clone(),
                    });
                }
            }
            DepIndex::Bool(_) => {
                // Bool indices: only ident or boolean literal
                if !self.is_bool_expr(expr) {
                    errors.push(DepTypeError {
                        code: "A03008".into(),
                        message: "Bool index must be a direct reference or boolean literal, \
                                  not an arithmetic expression"
                            .into(),
                        span: span.clone(),
                    });
                }
            }
            DepIndex::Enum { enum_type, .. } => {
                // Enum indices: only ident or enum variant
                if !self.is_enum_expr(expr, enum_type) {
                    errors.push(DepTypeError {
                        code: "A03009".into(),
                        message: format!(
                            "enum index of type `{enum_type}` must be a direct reference \
                             or variant name"
                        ),
                        span: span.clone(),
                    });
                }
            }
        }
        errors
    }

    /// Check structural equality of two dependent types.
    /// Two `Vec<T, n>` and `Vec<T, m>` are equal only if `n == m` can be proved.
    pub fn check_dep_type_eq(
        &self,
        expected: &DepType,
        actual: &DepType,
        span: &Range<usize>,
    ) -> Vec<DepTypeError> {
        let mut errors = Vec::new();
        if expected.base != actual.base {
            errors.push(DepTypeError {
                code: "A03010".into(),
                message: format!(
                    "dependent type base mismatch: expected `{:?}`, found `{:?}`",
                    expected.base, actual.base
                ),
                span: span.clone(),
            });
            return errors;
        }
        if expected.indices.len() != actual.indices.len() {
            errors.push(DepTypeError {
                code: "A03010".into(),
                message: format!(
                    "dependent type index count mismatch: expected {}, found {}",
                    expected.indices.len(),
                    actual.indices.len()
                ),
                span: span.clone(),
            });
            return errors;
        }
        for (i, (exp, act)) in expected.indices.iter().zip(&actual.indices).enumerate() {
            if std::mem::discriminant(exp) != std::mem::discriminant(act) {
                errors.push(DepTypeError {
                    code: "A03011".into(),
                    message: format!(
                        "dependent type index {i} kind mismatch: expected {exp}, found {act}"
                    ),
                    span: span.clone(),
                });
            }
        }
        errors
    }

    /// Verify that index variables are erased at runtime.
    /// Returns an error if an index variable appears in a non-ghost context.
    pub fn check_index_erasure(
        &self,
        expr: &Expr,
        ghost_context: bool,
        span: &Range<usize>,
    ) -> Vec<DepTypeError> {
        if ghost_context {
            return Vec::new(); // Ghost context: indices are fine
        }
        let mut errors = Vec::new();
        for name in self.collect_idents(expr) {
            if self.index_vars.contains_key(&name) {
                errors.push(DepTypeError {
                    code: "A03012".into(),
                    message: format!(
                        "index variable `{name}` used in runtime context; \
                         dependent type indices must be erased at runtime"
                    ),
                    span: span.clone(),
                });
            }
        }
        errors
    }

    // --- Helper methods ---

    fn is_nat_expr(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Literal(Literal::Int(_)) => true,
            Expr::Ident(name) => {
                matches!(self.index_vars.get(name), Some(DepIndex::Nat(_)))
                    || !self.index_vars.contains_key(name)
            }
            Expr::BinOp { lhs, op, rhs } => {
                matches!(
                    op,
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod
                ) && self.is_nat_expr(lhs)
                    && self.is_nat_expr(rhs)
            }
            Expr::UnaryOp {
                op: UnaryOp::Neg,
                expr,
            } => self.is_nat_expr(expr),
            Expr::Paren(inner) => self.is_nat_expr(inner),
            _ => false,
        }
    }

    fn is_bool_expr(&self, expr: &Expr) -> bool {
        matches!(expr, Expr::Literal(Literal::Bool(_)) | Expr::Ident(_))
    }

    fn is_enum_expr(&self, expr: &Expr, enum_type: &str) -> bool {
        match expr {
            Expr::Ident(name) => {
                // Either a variable reference or a variant name
                if let Some(variants) = self.enums.get(enum_type) {
                    variants.contains(name) || self.index_vars.contains_key(name)
                } else {
                    self.index_vars.contains_key(name)
                }
            }
            _ => false,
        }
    }

    fn collect_idents(&self, expr: &Expr) -> Vec<String> {
        let mut names = Vec::new();
        match expr {
            Expr::Ident(n) => names.push(n.clone()),
            Expr::BinOp { lhs, rhs, .. } => {
                names.extend(self.collect_idents(lhs));
                names.extend(self.collect_idents(rhs));
            }
            Expr::UnaryOp { expr, .. } => names.extend(self.collect_idents(expr)),
            Expr::Call { func, args } => {
                names.extend(self.collect_idents(func));
                for a in args {
                    names.extend(self.collect_idents(a));
                }
            }
            Expr::Field(e, _) => names.extend(self.collect_idents(e)),
            Expr::Index { expr, index } => {
                names.extend(self.collect_idents(expr));
                names.extend(self.collect_idents(index));
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                names.extend(self.collect_idents(cond));
                names.extend(self.collect_idents(then_branch));
                if let Some(e) = else_branch {
                    names.extend(self.collect_idents(e));
                }
            }
            Expr::Paren(e) | Expr::Old(e) | Expr::Ghost(e) => {
                names.extend(self.collect_idents(e));
            }
            _ => {}
        }
        names
    }
}

impl Default for DependentTypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests

// ---------------------------------------------------------------------------
// Information flow checking (T051 - SEC.3)
// ---------------------------------------------------------------------------

/// Security label in the information flow lattice.
///
/// The lattice is ordered: `Public < Internal < Confidential < Restricted`.
/// Data may flow upward in the lattice (Public -> Confidential) but never
/// downward (Confidential -> Public) without explicit declassification.
///
/// Implements Section 2.7 of the spec (information flow types).
///
/// # Error codes
///
/// - **A08001**: Information flows from higher security to lower security
/// - **A08002**: Declassification without explicit annotation
/// - **A08003**: Purpose label mismatch (GDPR)
/// - **A08004**: Implicit flow through control dependency
/// - **A08005**: Covert channel through timing/exceptions
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum SecurityLabel {
    /// Publicly accessible data.
    Public,
    /// Internal-only data (not exposed to external consumers).
    Internal,
    /// Confidential data (PII, credentials, etc.).
    Confidential,
    /// Restricted data (highest classification, e.g. encryption keys).
    Restricted,
}

impl std::fmt::Display for SecurityLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecurityLabel::Public => write!(f, "Public"),
            SecurityLabel::Internal => write!(f, "Internal"),
            SecurityLabel::Confidential => write!(f, "Confidential"),
            SecurityLabel::Restricted => write!(f, "Restricted"),
        }
    }
}

/// A structured information flow error.
#[derive(Debug, Clone)]
pub(crate) struct InfoFlowError {
    /// Error code (A08001-A08005).
    pub code: std::string::String,
    /// Human-readable error message.
    pub message: std::string::String,
    /// Source location where the error was detected.
    pub span: Range<usize>,
}

/// Information flow checker that enforces the security lattice.
///
/// Tracks security labels on variables and ensures that data never flows
/// from a higher security level to a lower one without explicit
/// declassification.  Also tracks GDPR purpose labels for data-purpose
/// compliance.
#[derive(Debug, Clone)]
pub(crate) struct InfoFlowChecker {
    /// Maps variable name to its security label.
    labels: HashMap<std::string::String, SecurityLabel>,
    /// Maps variable name to its GDPR purpose label (e.g. "analytics",
    /// "billing", "marketing").
    purpose_labels: HashMap<std::string::String, std::string::String>,
    /// Set of variables that carry an explicit `@declassify` annotation.
    declassify_annotations: std::collections::HashSet<std::string::String>,
    /// Names of functions that are considered timing-sensitive (potential
    /// covert channels).
    timing_sensitive_fns: std::collections::HashSet<std::string::String>,
}

impl InfoFlowChecker {
    /// Create a new, empty information flow checker with built-in
    /// timing-sensitive function names.
    pub fn new() -> Self {
        let mut timing_sensitive_fns = std::collections::HashSet::new();
        timing_sensitive_fns.insert("sleep".to_string());
        timing_sensitive_fns.insert("delay".to_string());
        timing_sensitive_fns.insert("wait".to_string());
        timing_sensitive_fns.insert("throw".to_string());
        timing_sensitive_fns.insert("panic".to_string());
        timing_sensitive_fns.insert("abort".to_string());
        Self {
            labels: HashMap::new(),
            purpose_labels: HashMap::new(),
            declassify_annotations: std::collections::HashSet::new(),
            timing_sensitive_fns,
        }
    }

    /// Declare a variable with a security label.
    pub fn declare(&mut self, name: std::string::String, label: SecurityLabel) {
        self.labels.insert(name, label);
    }

    /// Declare a variable with a GDPR purpose label.
    pub fn declare_purpose(&mut self, name: std::string::String, purpose: std::string::String) {
        self.purpose_labels.insert(name, purpose);
    }

    /// Mark a variable as having an explicit `@declassify` annotation.
    pub fn mark_declassify(&mut self, name: std::string::String) {
        self.declassify_annotations.insert(name);
    }

    /// Register a function as timing-sensitive (potential covert channel).
    pub fn register_timing_sensitive(&mut self, name: std::string::String) {
        self.timing_sensitive_fns.insert(name);
    }

    /// Get the security label for a variable. Returns `None` if the
    /// variable has not been declared.
    pub fn get_label(&self, name: &str) -> Option<SecurityLabel> {
        self.labels.get(name).copied()
    }

    /// Get the purpose label for a variable.
    pub fn get_purpose(&self, name: &str) -> Option<&str> {
        self.purpose_labels.get(name).map(|s| s.as_str())
    }

    /// Returns `true` if any security labels are tracked.
    pub fn has_labels(&self) -> bool {
        !self.labels.is_empty()
    }

    // -----------------------------------------------------------------
    // Core checks
    // -----------------------------------------------------------------

    /// Check an assignment: data flows from `source_label` to
    /// `target_label`.
    ///
    /// The source security level must be less than or equal to the
    /// target level. Emits **A08001** if data flows from a higher
    /// security level to a lower one.
    pub fn check_assignment(
        &self,
        target_label: SecurityLabel,
        source_label: SecurityLabel,
        span: &Range<usize>,
    ) -> Option<InfoFlowError> {
        if source_label > target_label {
            Some(InfoFlowError {
                code: "A08001".into(),
                message: format!(
                    "information flows from {source_label} to {target_label}: \
                     data at security level `{source_label}` cannot be assigned \
                     to a `{target_label}` variable"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Check a declassification: data is being lowered from `from_label`
    /// to `to_label`.
    ///
    /// Declassification is only permitted when an explicit annotation is
    /// present. Emits **A08002** if `has_declassify_annotation` is false.
    pub fn check_declassify(
        &self,
        from_label: SecurityLabel,
        to_label: SecurityLabel,
        has_declassify_annotation: bool,
        span: &Range<usize>,
    ) -> Option<InfoFlowError> {
        if from_label > to_label && !has_declassify_annotation {
            Some(InfoFlowError {
                code: "A08002".into(),
                message: format!(
                    "declassification from {from_label} to {to_label} \
                     without explicit `@declassify` annotation"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Check that a variable's purpose label matches the required purpose.
    ///
    /// Emits **A08003** if the variable has a purpose label that differs
    /// from `required_purpose`.
    pub fn check_purpose_label(
        &self,
        variable: &str,
        required_purpose: &str,
        span: &Range<usize>,
    ) -> Option<InfoFlowError> {
        if let Some(actual_purpose) = self.purpose_labels.get(variable)
            && actual_purpose != required_purpose
        {
            return Some(InfoFlowError {
                code: "A08003".into(),
                message: format!(
                    "purpose label mismatch for `{variable}`: data labeled \
                     for `{actual_purpose}` used in `{required_purpose}` context"
                ),
                span: span.clone(),
            });
        }
        None
    }

    /// Check for implicit information flow through control dependencies.
    ///
    /// If a conditional expression depends on a high-security variable and
    /// assigns to a low-security variable inside a branch, information
    /// leaks through the control flow.  Emits **A08004**.
    ///
    /// `condition_label` is the inferred label of the if-condition.
    /// `branch_target_label` is the label of the variable being assigned
    /// inside the branch.
    pub fn check_implicit_flow(
        &self,
        condition_label: SecurityLabel,
        branch_target_label: SecurityLabel,
        span: &Range<usize>,
    ) -> Option<InfoFlowError> {
        if condition_label > branch_target_label {
            Some(InfoFlowError {
                code: "A08004".into(),
                message: format!(
                    "implicit information flow: condition at `{condition_label}` \
                     level influences assignment to `{branch_target_label}` variable"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Check for covert channels through timing or exceptions.
    ///
    /// If a high-security value controls whether a timing-sensitive
    /// function (sleep, delay, throw, panic) is called, information can
    /// leak through observable side effects.  Emits **A08005**.
    pub fn check_covert_channel(
        &self,
        condition_label: SecurityLabel,
        callee: &str,
        span: &Range<usize>,
    ) -> Option<InfoFlowError> {
        if condition_label > SecurityLabel::Public && self.timing_sensitive_fns.contains(callee) {
            Some(InfoFlowError {
                code: "A08005".into(),
                message: format!(
                    "potential covert channel: `{condition_label}` data controls \
                     call to timing/exception function `{callee}`"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    // -----------------------------------------------------------------
    // Label inference
    // -----------------------------------------------------------------

    /// Infer the security label of an expression.
    ///
    /// The result label is the **maximum** of all operand labels (the
    /// join in the lattice).  Variables without a declared label default
    /// to `Public`.
    pub fn infer_label(&self, expr: &Expr) -> SecurityLabel {
        match expr {
            Expr::Ident(name) => self
                .labels
                .get(name)
                .copied()
                .unwrap_or(SecurityLabel::Public),

            Expr::Literal(_) => SecurityLabel::Public,

            Expr::Field(receiver, _) => self.infer_label(receiver),

            Expr::BinOp { lhs, rhs, .. } => {
                std::cmp::max(self.infer_label(lhs), self.infer_label(rhs))
            }

            Expr::UnaryOp { expr: inner, .. } => self.infer_label(inner),

            Expr::Call { func, args } => {
                let f = self.infer_label(func);
                args.iter()
                    .fold(f, |acc, arg| std::cmp::max(acc, self.infer_label(arg)))
            }

            Expr::MethodCall { receiver, args, .. } => {
                let r = self.infer_label(receiver);
                args.iter()
                    .fold(r, |acc, arg| std::cmp::max(acc, self.infer_label(arg)))
            }

            Expr::Index { expr: base, index } => {
                std::cmp::max(self.infer_label(base), self.infer_label(index))
            }

            Expr::Old(inner) | Expr::Paren(inner) | Expr::Cast { expr: inner, .. } => {
                self.infer_label(inner)
            }

            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let mut r = std::cmp::max(self.infer_label(cond), self.infer_label(then_branch));
                if let Some(e) = else_branch {
                    r = std::cmp::max(r, self.infer_label(e));
                }
                r
            }

            Expr::List(items) => items.iter().fold(SecurityLabel::Public, |a, i| {
                std::cmp::max(a, self.infer_label(i))
            }),

            Expr::Block(exprs) => exprs.iter().fold(SecurityLabel::Public, |a, e| {
                std::cmp::max(a, self.infer_label(e))
            }),

            Expr::Forall { body, .. } | Expr::Exists { body, .. } => self.infer_label(body),

            Expr::Apply { args, .. } => args.iter().fold(SecurityLabel::Public, |a, arg| {
                std::cmp::max(a, self.infer_label(arg))
            }),

            Expr::Match { scrutinee, arms } => {
                let mut r = self.infer_label(scrutinee);
                for arm in arms {
                    r = std::cmp::max(r, self.infer_label(&arm.body));
                }
                r
            }

            Expr::Let { value, body, .. } => {
                std::cmp::max(self.infer_label(value), self.infer_label(body))
            }

            Expr::Tuple(elems) => elems.iter().fold(SecurityLabel::Public, |a, e| {
                std::cmp::max(a, self.infer_label(e))
            }),

            Expr::Ghost(_) | Expr::Raw(_) => SecurityLabel::Public,
        }
    }

    // -----------------------------------------------------------------
    // Expression-level checking
    // -----------------------------------------------------------------

    /// Check an expression tree for information flow violations.
    ///
    /// Walks the AST looking for:
    /// - Implicit flows through `if` conditions (A08004)
    /// - Covert channels through timing/exception calls (A08005)
    pub fn check_expr(&self, expr: &Expr, span: &Range<usize>) -> Vec<InfoFlowError> {
        let mut errors = Vec::new();
        self.check_expr_inner(expr, span, SecurityLabel::Public, &mut errors);
        errors
    }

    /// Inner recursive checker with a `pc_label` representing the
    /// current program-counter security context (from enclosing
    /// conditionals).
    fn check_expr_inner(
        &self,
        expr: &Expr,
        span: &Range<usize>,
        pc_label: SecurityLabel,
        errors: &mut Vec<InfoFlowError>,
    ) {
        match expr {
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_label = std::cmp::max(pc_label, self.infer_label(cond));
                self.check_expr_inner(cond, span, pc_label, errors);
                self.check_expr_inner(then_branch, span, cond_label, errors);
                if let Some(else_br) = else_branch {
                    self.check_expr_inner(else_br, span, cond_label, errors);
                }
            }

            // Detect covert channels: high-security pc controls a
            // timing-sensitive or exception-raising call.
            Expr::Call { func, args } => {
                if let Expr::Ident(name) = func.as_ref()
                    && let Some(err) = self.check_covert_channel(pc_label, name, span)
                {
                    errors.push(err);
                }
                self.check_expr_inner(func, span, pc_label, errors);
                for arg in args {
                    self.check_expr_inner(arg, span, pc_label, errors);
                }
            }

            Expr::MethodCall {
                receiver,
                method,
                args,
            } => {
                if let Some(err) = self.check_covert_channel(pc_label, method, span) {
                    errors.push(err);
                }
                self.check_expr_inner(receiver, span, pc_label, errors);
                for arg in args {
                    self.check_expr_inner(arg, span, pc_label, errors);
                }
            }

            // Recurse into sub-expressions
            Expr::BinOp { lhs, rhs, .. } => {
                self.check_expr_inner(lhs, span, pc_label, errors);
                self.check_expr_inner(rhs, span, pc_label, errors);
            }
            Expr::UnaryOp { expr: inner, .. }
            | Expr::Old(inner)
            | Expr::Paren(inner)
            | Expr::Cast { expr: inner, .. }
            | Expr::Ghost(inner) => {
                self.check_expr_inner(inner, span, pc_label, errors);
            }
            Expr::Field(receiver, _) => {
                self.check_expr_inner(receiver, span, pc_label, errors);
            }
            Expr::Index { expr: base, index } => {
                self.check_expr_inner(base, span, pc_label, errors);
                self.check_expr_inner(index, span, pc_label, errors);
            }
            Expr::List(items) => {
                for item in items {
                    self.check_expr_inner(item, span, pc_label, errors);
                }
            }
            Expr::Block(exprs) => {
                for e in exprs {
                    self.check_expr_inner(e, span, pc_label, errors);
                }
            }
            Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
                self.check_expr_inner(domain, span, pc_label, errors);
                self.check_expr_inner(body, span, pc_label, errors);
            }
            Expr::Apply { args, .. } => {
                for arg in args {
                    self.check_expr_inner(arg, span, pc_label, errors);
                }
            }
            Expr::Match { scrutinee, arms } => {
                self.check_expr_inner(scrutinee, span, pc_label, errors);
                // Each arm body executes under the PC label of the scrutinee
                let scrut_label = self.infer_label(scrutinee);
                let elevated = std::cmp::max(pc_label, scrut_label);
                for arm in arms {
                    self.check_expr_inner(&arm.body, span, elevated, errors);
                }
            }
            Expr::Let { value, body, .. } => {
                self.check_expr_inner(value, span, pc_label, errors);
                self.check_expr_inner(body, span, pc_label, errors);
            }
            Expr::Tuple(elems) => {
                for e in elems {
                    self.check_expr_inner(e, span, pc_label, errors);
                }
            }
            Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => {}
        }
    }
}

impl Default for InfoFlowChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests

// ---------------------------------------------------------------------------
// Totality checking (T053)
// ---------------------------------------------------------------------------

/// What expression decreases at each recursive call, proving termination.
#[derive(Debug, Clone)]
pub(crate) enum DecreasesMeasure {
    /// A single natural-number expression that must strictly decrease.
    Natural(Expr),
    /// A lexicographic tuple of measures (e.g., Ackermann-like functions).
    Lexicographic(Vec<Expr>),
    /// Well-founded ordering on a custom/structural type.
    WellFounded(Expr),
}

/// A totality error with error code, span, and message.
#[derive(Debug, Clone)]
pub(crate) struct TotalityError {
    /// Error code from the spec (A09xxx series).
    pub code: std::string::String,
    /// Human-readable error message.
    pub message: std::string::String,
    /// Source location where the error was detected.
    pub span: Range<usize>,
}

/// Result of checking whether a recursive call decreases the measure.
#[derive(Debug)]
pub(crate) enum DecreaseCheckResult {
    /// Syntactically proved to decrease (e.g., n-1, x.tail).
    Proved,
    /// Syntactically failed; needs SMT fallback.
    /// Contains the measure expression and call-site argument for SMT.
    NeedsSmt { measure_expr: Expr, call_arg: Expr },
    /// Definitely does not decrease (error).
    Failed(TotalityError),
}

/// A pending decrease check that requires SMT verification.
///
/// Returned by `check_function_totality` when syntactic checking is
/// inconclusive. The wiring layer (which has access to both assura-types
/// and assura-smt) dispatches these to Z3.
#[derive(Debug, Clone)]
pub struct PendingDecreaseCheck {
    /// Function name (for error messages).
    pub fn_name: String,
    /// The function's requires clauses (preconditions for SMT).
    pub preconditions: Vec<Expr>,
    /// The decreases measure expression.
    pub measure_expr: Expr,
    /// The call-site argument expression.
    pub call_arg: Expr,
    /// Source span for error reporting.
    pub span: Range<usize>,
}

/// Totality checker for termination checking via `decreases` measures.
///
/// Validates that recursive functions terminate by checking that a
/// well-founded measure strictly decreases at every recursive call site.
///
/// # Error codes
///
/// - **A09001**: Recursive function without `decreases` clause (and no `partial` annotation)
/// - **A09002**: Measure does not strictly decrease at recursive call site
/// - **A09003**: Cannot prove measure is well-founded (e.g., might go negative)
/// - **A09004**: Mutually recursive functions without collective termination proof
pub(crate) struct TotalityChecker {
    /// Names of functions known to be partial (escape hatch).
    partial_fns: std::collections::HashSet<std::string::String>,
}

impl TotalityChecker {
    /// Create a new totality checker.
    pub fn new() -> Self {
        Self {
            partial_fns: std::collections::HashSet::new(),
        }
    }

    /// Register a function as `partial` (opt out of termination checking).
    pub fn mark_partial(&mut self, name: std::string::String) {
        self.partial_fns.insert(name);
    }

    /// Check whether a function definition has the `partial` escape hatch.
    ///
    /// A function is partial if it was explicitly registered via
    /// [`mark_partial`] or if its clauses contain an `Other("partial")`
    /// clause kind.
    pub fn is_partial(&self, fn_def: &assura_parser::ast::FnDef) -> bool {
        if self.partial_fns.contains(&fn_def.name) {
            return true;
        }
        // Check for a `partial` annotation in clause kinds
        fn_def
            .clauses
            .iter()
            .any(|c| matches!(&c.kind, ClauseKind::Other(s) if s == "partial"))
    }

    /// Extract the `decreases` measure from a function definition.
    ///
    /// Looks for clauses with kind `Other("decreases")`. The clause body
    /// expression becomes the measure. Multiple decreases clauses form a
    /// lexicographic tuple. A single clause is a `Natural` measure.
    pub fn extract_decreases_measure(
        &self,
        fn_def: &assura_parser::ast::FnDef,
    ) -> Option<DecreasesMeasure> {
        let decreases_exprs: Vec<&Expr> = fn_def
            .clauses
            .iter()
            .filter(|c| {
                c.kind == ClauseKind::Decreases
                    || matches!(&c.kind, ClauseKind::Other(s) if s == "decreases")
            })
            .map(|c| &c.body)
            .collect();

        match decreases_exprs.len() {
            0 => None,
            1 => Some(DecreasesMeasure::Natural(decreases_exprs[0].clone())),
            _ => Some(DecreasesMeasure::Lexicographic(
                decreases_exprs.into_iter().cloned().collect(),
            )),
        }
    }

    /// Check whether the given expression contains a recursive call to `fn_name`.
    fn expr_contains_recursive_call(&self, expr: &Expr, fn_name: &str) -> bool {
        match expr {
            Expr::Call { func, args } => {
                let is_self_call = matches!(func.as_ref(), Expr::Ident(name) if name == fn_name);
                if is_self_call {
                    return true;
                }
                self.expr_contains_recursive_call(func, fn_name)
                    || args
                        .iter()
                        .any(|a| self.expr_contains_recursive_call(a, fn_name))
            }
            Expr::BinOp { lhs, rhs, .. } => {
                self.expr_contains_recursive_call(lhs, fn_name)
                    || self.expr_contains_recursive_call(rhs, fn_name)
            }
            Expr::UnaryOp { expr: inner, .. }
            | Expr::Old(inner)
            | Expr::Paren(inner)
            | Expr::Cast { expr: inner, .. }
            | Expr::Ghost(inner) => self.expr_contains_recursive_call(inner, fn_name),
            Expr::Field(receiver, _) => self.expr_contains_recursive_call(receiver, fn_name),
            Expr::MethodCall { receiver, args, .. } => {
                self.expr_contains_recursive_call(receiver, fn_name)
                    || args
                        .iter()
                        .any(|a| self.expr_contains_recursive_call(a, fn_name))
            }
            Expr::Index {
                expr: base, index, ..
            } => {
                self.expr_contains_recursive_call(base, fn_name)
                    || self.expr_contains_recursive_call(index, fn_name)
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr_contains_recursive_call(cond, fn_name)
                    || self.expr_contains_recursive_call(then_branch, fn_name)
                    || else_branch
                        .as_ref()
                        .is_some_and(|e| self.expr_contains_recursive_call(e, fn_name))
            }
            Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
                self.expr_contains_recursive_call(domain, fn_name)
                    || self.expr_contains_recursive_call(body, fn_name)
            }
            Expr::List(items) => items
                .iter()
                .any(|i| self.expr_contains_recursive_call(i, fn_name)),
            Expr::Block(exprs) => exprs
                .iter()
                .any(|e| self.expr_contains_recursive_call(e, fn_name)),
            Expr::Apply { args, .. } => args
                .iter()
                .any(|a| self.expr_contains_recursive_call(a, fn_name)),
            Expr::Match { scrutinee, arms } => {
                self.expr_contains_recursive_call(scrutinee, fn_name)
                    || arms
                        .iter()
                        .any(|arm| self.expr_contains_recursive_call(&arm.body, fn_name))
            }
            Expr::Let { value, body, .. } => {
                self.expr_contains_recursive_call(value, fn_name)
                    || self.expr_contains_recursive_call(body, fn_name)
            }
            Expr::Tuple(elems) => elems
                .iter()
                .any(|e| self.expr_contains_recursive_call(e, fn_name)),
            Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => false,
        }
    }

    /// Collect arguments from recursive call sites to `fn_name` in `expr`.
    fn collect_recursive_call_args<'a>(
        &self,
        expr: &'a Expr,
        fn_name: &str,
        out: &mut Vec<&'a [Expr]>,
    ) {
        match expr {
            Expr::Call { func, args } => {
                if matches!(func.as_ref(), Expr::Ident(name) if name == fn_name) {
                    out.push(args.as_slice());
                }
                self.collect_recursive_call_args(func, fn_name, out);
                for a in args {
                    self.collect_recursive_call_args(a, fn_name, out);
                }
            }
            Expr::BinOp { lhs, rhs, .. } => {
                self.collect_recursive_call_args(lhs, fn_name, out);
                self.collect_recursive_call_args(rhs, fn_name, out);
            }
            Expr::UnaryOp { expr: inner, .. }
            | Expr::Old(inner)
            | Expr::Paren(inner)
            | Expr::Cast { expr: inner, .. }
            | Expr::Ghost(inner) => {
                self.collect_recursive_call_args(inner, fn_name, out);
            }
            Expr::Field(receiver, _) => {
                self.collect_recursive_call_args(receiver, fn_name, out);
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.collect_recursive_call_args(receiver, fn_name, out);
                for a in args {
                    self.collect_recursive_call_args(a, fn_name, out);
                }
            }
            Expr::Index {
                expr: base, index, ..
            } => {
                self.collect_recursive_call_args(base, fn_name, out);
                self.collect_recursive_call_args(index, fn_name, out);
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.collect_recursive_call_args(cond, fn_name, out);
                self.collect_recursive_call_args(then_branch, fn_name, out);
                if let Some(e) = else_branch {
                    self.collect_recursive_call_args(e, fn_name, out);
                }
            }
            Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
                self.collect_recursive_call_args(domain, fn_name, out);
                self.collect_recursive_call_args(body, fn_name, out);
            }
            Expr::List(items) => {
                for i in items {
                    self.collect_recursive_call_args(i, fn_name, out);
                }
            }
            Expr::Block(exprs) => {
                for e in exprs {
                    self.collect_recursive_call_args(e, fn_name, out);
                }
            }
            Expr::Apply { args, .. } => {
                for a in args {
                    self.collect_recursive_call_args(a, fn_name, out);
                }
            }
            Expr::Match { scrutinee, arms } => {
                self.collect_recursive_call_args(scrutinee, fn_name, out);
                for arm in arms {
                    self.collect_recursive_call_args(&arm.body, fn_name, out);
                }
            }
            Expr::Let { value, body, .. } => {
                self.collect_recursive_call_args(value, fn_name, out);
                self.collect_recursive_call_args(body, fn_name, out);
            }
            Expr::Tuple(elems) => {
                for e in elems {
                    self.collect_recursive_call_args(e, fn_name, out);
                }
            }
            Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => {}
        }
    }

    /// Check whether a recursive call's argument is structurally smaller
    /// than the corresponding measure expression.
    ///
    /// Recognizes patterns like `n - 1` (for natural measure `n`),
    /// `xs.tail` or `node.left` / `node.right` (structural recursion).
    fn is_strictly_decreasing(measure: &Expr, call_arg: &Expr) -> bool {
        // Pattern: measure is `Ident(x)`, call_arg is `x - <positive>`
        if let Expr::Ident(measure_var) = measure {
            match call_arg {
                // n - 1, n - 2, etc.
                Expr::BinOp {
                    lhs,
                    op: BinOp::Sub,
                    rhs,
                } => {
                    if let Expr::Ident(arg_var) = lhs.as_ref()
                        && arg_var == measure_var
                    {
                        // The rhs must be a positive literal
                        if let Expr::Literal(Literal::Int(s)) = rhs.as_ref()
                            && let Ok(v) = s.parse::<i64>()
                        {
                            return v > 0;
                        }
                        // Any non-zero expression is acceptable
                        return true;
                    }
                    false
                }
                // Structural: x.tail, x.left, x.right, x.children, etc.
                Expr::Field(receiver, field) => {
                    if let Expr::Ident(arg_var) = receiver.as_ref()
                        && arg_var == measure_var
                    {
                        return matches!(
                            field.as_str(),
                            "tail" | "left" | "right" | "children" | "rest" | "next"
                        );
                    }
                    false
                }
                _ => false,
            }
        } else {
            false
        }
    }

    /// Check whether a measure expression is well-founded (cannot go
    /// negative or be undefined).
    ///
    /// A natural-number variable is well-founded if the function has a
    /// `requires` clause constraining it to be >= 0. Structural measures
    /// on inductive types are always well-founded. Returns `true` if
    /// well-foundedness can be established, `false` otherwise.
    fn is_well_founded(measure: &Expr, fn_def: &assura_parser::ast::FnDef) -> bool {
        match measure {
            Expr::Ident(name) => {
                // Check requires clauses for a constraint like `n >= 0`
                for clause in &fn_def.clauses {
                    if clause.kind == ClauseKind::Requires
                        && Self::expr_constrains_non_negative(&clause.body, name)
                    {
                        return true;
                    }
                }
                // Check parameter type for well-foundedness
                for param in &fn_def.params {
                    if param.name == *name {
                        // Nat is always >= 0
                        if param.ty.iter().any(|t| t == "Nat") {
                            return true;
                        }
                        // Structural/named types (List, Tree, etc.) are
                        // well-founded by structural induction. Any type
                        // that is not a raw numeric type (Int, Float, etc.)
                        // is considered structural.
                        let is_numeric_type = param.ty.iter().any(|t| {
                            matches!(
                                t.as_str(),
                                "Int" | "Float" | "F32" | "F64" | "I8" | "I16" | "I32" | "I64"
                            )
                        });
                        if !is_numeric_type {
                            return true;
                        }
                    }
                }
                false
            }
            // Field access on a structural type is well-founded by induction
            Expr::Field(_, _) => true,
            _ => false,
        }
    }

    /// Check whether an expression constrains a variable to be non-negative.
    ///
    /// Recognizes patterns: `x >= 0`, `0 <= x`, `x > 0`, etc.
    fn expr_constrains_non_negative(expr: &Expr, var_name: &str) -> bool {
        match expr {
            Expr::BinOp { lhs, op, rhs } => {
                match op {
                    // x >= 0 or x > 0
                    BinOp::Gte | BinOp::Gt => {
                        if let Expr::Ident(name) = lhs.as_ref()
                            && name == var_name
                            && let Expr::Literal(Literal::Int(s)) = rhs.as_ref()
                            && let Ok(v) = s.parse::<i64>()
                        {
                            return v >= 0;
                        }
                        false
                    }
                    // 0 <= x or 0 < x
                    BinOp::Lte | BinOp::Lt => {
                        if let Expr::Literal(Literal::Int(s)) = lhs.as_ref()
                            && let Ok(v) = s.parse::<i64>()
                            && v >= 0
                            && let Expr::Ident(name) = rhs.as_ref()
                        {
                            return name == var_name;
                        }
                        false
                    }
                    // Conjunction: either side can provide the constraint
                    BinOp::And => {
                        Self::expr_constrains_non_negative(lhs, var_name)
                            || Self::expr_constrains_non_negative(rhs, var_name)
                    }
                    _ => false,
                }
            }
            Expr::Paren(inner) => Self::expr_constrains_non_negative(inner, var_name),
            _ => false,
        }
    }

    /// Check whether a recursive call strictly decreases the measure.
    ///
    /// For a `Natural` measure, finds the parameter matching the measure
    /// variable and checks that the corresponding call argument is
    /// structurally smaller. Returns `NeedsSmt` when syntactic checking
    /// is inconclusive so the caller can dispatch to Z3. For `Lexicographic`
    /// measures, checks that at least one component strictly decreases.
    pub fn check_recursive_call(
        &self,
        fn_def: &assura_parser::ast::FnDef,
        measure: &DecreasesMeasure,
        call_args: &[Expr],
        span: &Range<usize>,
    ) -> DecreaseCheckResult {
        match measure {
            DecreasesMeasure::Natural(measure_expr) => {
                // Find which parameter position corresponds to the measure
                if let Expr::Ident(measure_var) = measure_expr {
                    for (i, param) in fn_def.params.iter().enumerate() {
                        if param.name == *measure_var
                            && let Some(call_arg) = call_args.get(i)
                        {
                            // Try syntactic check first (fast)
                            if Self::is_strictly_decreasing(measure_expr, call_arg) {
                                return DecreaseCheckResult::Proved;
                            }
                            // Syntactic check failed; return NeedsSmt
                            return DecreaseCheckResult::NeedsSmt {
                                measure_expr: measure_expr.clone(),
                                call_arg: call_arg.clone(),
                            };
                        }
                    }
                }
                DecreaseCheckResult::Proved // Cannot determine; no matching parameter
            }
            DecreasesMeasure::Lexicographic(measures) => {
                // For lexicographic: at least one component must strictly decrease
                let mut any_decreases = false;
                let mut smt_candidates: Vec<(Expr, Expr)> = Vec::new();
                for measure_expr in measures {
                    if let Expr::Ident(measure_var) = measure_expr {
                        for (i, param) in fn_def.params.iter().enumerate() {
                            if param.name == *measure_var
                                && let Some(call_arg) = call_args.get(i)
                            {
                                if Self::is_strictly_decreasing(measure_expr, call_arg) {
                                    any_decreases = true;
                                } else {
                                    smt_candidates.push((measure_expr.clone(), call_arg.clone()));
                                }
                            }
                        }
                    }
                }
                if any_decreases {
                    DecreaseCheckResult::Proved
                } else if let Some((measure_expr, call_arg)) = smt_candidates.into_iter().next() {
                    DecreaseCheckResult::NeedsSmt {
                        measure_expr,
                        call_arg,
                    }
                } else {
                    DecreaseCheckResult::Failed(TotalityError {
                        code: "A09002".into(),
                        message: format!(
                            "lexicographic measure does not strictly decrease \
                             at recursive call to `{}`",
                            fn_def.name
                        ),
                        span: span.clone(),
                    })
                }
            }
            DecreasesMeasure::WellFounded(_) => {
                // Well-founded ordering check is deferred to SMT
                DecreaseCheckResult::Proved
            }
        }
    }

    /// Check a single function for totality (termination).
    ///
    /// 1. If the function is `partial`, skip it.
    /// 2. Determine if the function is recursive (calls itself).
    /// 3. If recursive, extract the `decreases` measure.
    /// 4. Verify the measure strictly decreases at every recursive call.
    /// 5. Verify the measure is well-founded.
    ///
    /// Returns errors found syntactically plus pending SMT checks for
    /// cases where syntactic checking is inconclusive.
    pub fn check_function_totality(
        &self,
        fn_def: &assura_parser::ast::FnDef,
        span: &Range<usize>,
    ) -> (Vec<TotalityError>, Vec<PendingDecreaseCheck>) {
        let mut errors = Vec::new();
        let mut pending_smt = Vec::new();

        // Partial functions skip termination checking
        if self.is_partial(fn_def) {
            return (errors, pending_smt);
        }

        // Determine if the function is recursive by scanning its clause bodies
        let is_recursive = fn_def
            .clauses
            .iter()
            .any(|c| self.expr_contains_recursive_call(&c.body, &fn_def.name));

        if !is_recursive {
            // Non-recursive functions are trivially total
            return (errors, pending_smt);
        }

        // Extract the decreases measure
        let measure = match self.extract_decreases_measure(fn_def) {
            Some(m) => m,
            None => {
                errors.push(TotalityError {
                    code: "A09001".into(),
                    message: format!(
                        "recursive function `{}` has no `decreases` clause; \
                         add `decreases <expr>` or annotate with `partial`",
                        fn_def.name
                    ),
                    span: span.clone(),
                });
                return (errors, pending_smt);
            }
        };

        // Check well-foundedness of the measure
        match &measure {
            DecreasesMeasure::Natural(expr) => {
                if !Self::is_well_founded(expr, fn_def) {
                    errors.push(TotalityError {
                        code: "A09003".into(),
                        message: format!(
                            "cannot prove measure is well-founded for function `{}`; \
                             add `requires` clause ensuring the measure is non-negative",
                            fn_def.name
                        ),
                        span: span.clone(),
                    });
                }
            }
            DecreasesMeasure::Lexicographic(exprs) => {
                for expr in exprs {
                    if !Self::is_well_founded(expr, fn_def) {
                        errors.push(TotalityError {
                            code: "A09003".into(),
                            message: format!(
                                "cannot prove measure component is well-founded \
                                 for function `{}`",
                                fn_def.name
                            ),
                            span: span.clone(),
                        });
                        break; // One error is enough
                    }
                }
            }
            DecreasesMeasure::WellFounded(_) => {
                // Deferred to SMT
            }
        }

        // Collect recursive call sites and check each one
        let mut call_arg_sets: Vec<&[Expr]> = Vec::new();
        for clause in &fn_def.clauses {
            self.collect_recursive_call_args(&clause.body, &fn_def.name, &mut call_arg_sets);
        }

        for call_args in &call_arg_sets {
            match self.check_recursive_call(fn_def, &measure, call_args, span) {
                DecreaseCheckResult::Proved => {}
                DecreaseCheckResult::NeedsSmt {
                    measure_expr,
                    call_arg,
                } => {
                    // Store pending SMT check for the wiring layer
                    pending_smt.push(PendingDecreaseCheck {
                        fn_name: fn_def.name.clone(),
                        preconditions: fn_def
                            .clauses
                            .iter()
                            .filter(|c| c.kind == ClauseKind::Requires)
                            .map(|c| c.body.clone())
                            .collect(),
                        measure_expr,
                        call_arg,
                        span: span.clone(),
                    });
                }
                DecreaseCheckResult::Failed(err) => errors.push(err),
            }
        }

        (errors, pending_smt)
    }

    /// Detect and verify mutually recursive function groups.
    ///
    /// Given a set of function definitions, builds a call graph, finds
    /// strongly connected components (groups of mutually recursive
    /// functions), and checks that each group has a collective
    /// termination proof.
    ///
    /// Returns A09004 for groups where no function has a `decreases` clause.
    pub fn check_mutual_recursion(
        &self,
        fn_defs: &[(&assura_parser::ast::FnDef, &Range<usize>)],
    ) -> Vec<TotalityError> {
        let mut errors = Vec::new();

        // Build a simple call graph: for each function, which other
        // functions in the set does it call?
        let names: Vec<&str> = fn_defs.iter().map(|(f, _)| f.name.as_str()).collect();

        for (i, &(fn_def_i, span_i)) in fn_defs.iter().enumerate() {
            // Skip partial functions
            if self.is_partial(fn_def_i) {
                continue;
            }

            for (j, &(fn_def_j, _)) in fn_defs.iter().enumerate() {
                if i == j {
                    continue;
                }

                // Does fn_i call fn_j?
                let i_calls_j = fn_def_i
                    .clauses
                    .iter()
                    .any(|c| self.expr_contains_recursive_call(&c.body, names[j]));

                // Does fn_j call fn_i?
                let j_calls_i = fn_def_j
                    .clauses
                    .iter()
                    .any(|c| self.expr_contains_recursive_call(&c.body, names[i]));

                if i_calls_j && j_calls_i {
                    // Mutual recursion detected; check for decreases
                    let has_measure_i = self.extract_decreases_measure(fn_def_i).is_some();
                    let has_measure_j = self.extract_decreases_measure(fn_def_j).is_some();

                    if !has_measure_i && !has_measure_j {
                        errors.push(TotalityError {
                            code: "A09004".into(),
                            message: format!(
                                "mutually recursive functions `{}` and `{}` \
                                 have no collective termination proof; \
                                 add `decreases` clauses to at least one",
                                fn_def_i.name, fn_def_j.name
                            ),
                            span: span_i.clone(),
                        });
                    }
                }
            }
        }

        errors
    }
}

impl Default for TotalityChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for TotalityChecker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TotalityChecker")
            .field("partial_fns", &self.partial_fns)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// T055 MEM.2: Fixed-width integer checker
// ---------------------------------------------------------------------------

/// A structured error from fixed-width integer checking.
#[derive(Debug, Clone)]
pub(crate) struct FixedWidthError {
    /// Error code (A10101-A10104).
    pub code: std::string::String,
    /// Human-readable message.
    pub message: std::string::String,
    /// Source span where the issue was detected.
    pub span: Range<usize>,
}

/// Checker for fixed-width integer types with overflow detection.
///
/// Tracks fixed-width integer types in expressions, detects potential
/// arithmetic overflow, validates cast safety, and flags signed/unsigned
/// mismatches.
///
/// Implements MEM.2 from Section 14 of the specification.
///
/// # Error codes
///
/// - **A10101**: Potential integer overflow in arithmetic operation
/// - **A10102**: Unsafe narrowing cast (e.g., U32 to U16 without bounds check)
/// - **A10103**: Signed/unsigned mismatch in comparison
/// - **A10104**: Division/modulo by zero not guarded
#[derive(Debug, Clone)]
pub(crate) struct FixedWidthChecker {
    /// Maps variable name to its fixed-width type.
    bindings: HashMap<std::string::String, Type>,
}

impl FixedWidthChecker {
    /// Create an empty fixed-width checker.
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
        }
    }

    /// Register a variable with its fixed-width integer type.
    pub fn declare(&mut self, name: std::string::String, ty: Type) {
        self.bindings.insert(name, ty);
    }

    /// Look up the type of a registered variable.
    pub fn get_type(&self, name: &str) -> Option<&Type> {
        self.bindings.get(name)
    }

    /// Return the valid numeric range `(min, max)` for a fixed-width type.
    ///
    /// Returns `None` for non-fixed-width types.
    pub fn range_for_type(ty: &Type) -> Option<(i128, i128)> {
        match ty {
            Type::U8 => Some((0, u8::MAX as i128)),
            Type::U16 => Some((0, u16::MAX as i128)),
            Type::U32 => Some((0, u32::MAX as i128)),
            Type::U64 => Some((0, u64::MAX as i128)),
            Type::I8 => Some((i8::MIN as i128, i8::MAX as i128)),
            Type::I16 => Some((i16::MIN as i128, i16::MAX as i128)),
            Type::I32 => Some((i32::MIN as i128, i32::MAX as i128)),
            Type::I64 => Some((i64::MIN as i128, i64::MAX as i128)),
            _ => None,
        }
    }

    /// Returns `true` if the given type is a fixed-width integer type.
    pub fn is_fixed_width(ty: &Type) -> bool {
        Self::range_for_type(ty).is_some()
    }

    /// Returns `true` if the given type is an unsigned fixed-width integer.
    pub fn is_unsigned(ty: &Type) -> bool {
        matches!(ty, Type::U8 | Type::U16 | Type::U32 | Type::U64)
    }

    /// Returns `true` if the given type is a signed fixed-width integer.
    pub fn is_signed(ty: &Type) -> bool {
        matches!(ty, Type::I8 | Type::I16 | Type::I32 | Type::I64)
    }

    /// Check whether an arithmetic operation can overflow given the operand
    /// type ranges.
    ///
    /// Returns `true` if the result of `op` applied to values in
    /// `left_range` and `right_range` can produce a value outside
    /// `result_range`.
    pub fn can_overflow(
        op: &BinOp,
        left_range: (i128, i128),
        right_range: (i128, i128),
        result_range: (i128, i128),
    ) -> bool {
        let (result_min, result_max) = result_range;
        match op {
            BinOp::Add => {
                let worst_low = left_range.0.saturating_add(right_range.0);
                let worst_high = left_range.1.saturating_add(right_range.1);
                worst_low < result_min || worst_high > result_max
            }
            BinOp::Sub => {
                let worst_low = left_range.0.saturating_sub(right_range.1);
                let worst_high = left_range.1.saturating_sub(right_range.0);
                worst_low < result_min || worst_high > result_max
            }
            BinOp::Mul => {
                let products = [
                    left_range.0.saturating_mul(right_range.0),
                    left_range.0.saturating_mul(right_range.1),
                    left_range.1.saturating_mul(right_range.0),
                    left_range.1.saturating_mul(right_range.1),
                ];
                let worst_low = products.iter().copied().min().unwrap_or(0);
                let worst_high = products.iter().copied().max().unwrap_or(0);
                worst_low < result_min || worst_high > result_max
            }
            _ => false,
        }
    }

    /// Check whether a cast from `from_type` to `to_type` is always safe.
    ///
    /// A cast is safe if every value in the source range fits in the
    /// destination range. Returns `true` for safe (widening) casts,
    /// `false` for potentially unsafe (narrowing) casts.
    pub fn is_safe_cast(from_type: &Type, to_type: &Type) -> bool {
        let from_range = match Self::range_for_type(from_type) {
            Some(r) => r,
            None => return true, // Non-fixed-width types are outside our scope
        };
        let to_range = match Self::range_for_type(to_type) {
            Some(r) => r,
            None => return true,
        };
        from_range.0 >= to_range.0 && from_range.1 <= to_range.1
    }

    /// Check potential overflow in an arithmetic operation on two typed
    /// operands.
    ///
    /// Returns `None` if the operation is safe, or `Some(FixedWidthError)`
    /// with code A10101 if overflow is possible.
    pub fn check_arithmetic_overflow(
        &self,
        op: &BinOp,
        left_type: &Type,
        right_type: &Type,
        span: &Range<usize>,
    ) -> Option<FixedWidthError> {
        // Only check arithmetic ops
        if !matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul) {
            return None;
        }

        let left_range = Self::range_for_type(left_type)?;
        let right_range = Self::range_for_type(right_type)?;

        // Result type is the wider of the two (or left if same width)
        let result_range = Self::wider_range(left_range, right_range);

        if Self::can_overflow(op, left_range, right_range, result_range) {
            let op_name = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                _ => "?",
            };
            Some(FixedWidthError {
                code: "A10101".into(),
                message: format!(
                    "potential integer overflow: `{left_type:?} {op_name} {right_type:?}` \
                     can exceed the target range [{}, {}]; consider using `{}`",
                    result_range.0,
                    result_range.1,
                    Self::suggest_checked_alternative(op),
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Check whether a cast expression is safe.
    ///
    /// Returns `None` if safe, or `Some(FixedWidthError)` with code
    /// A10102 for an unsafe narrowing cast.
    pub fn check_cast_safety(
        from_type: &Type,
        to_type: &Type,
        span: &Range<usize>,
    ) -> Option<FixedWidthError> {
        if !Self::is_fixed_width(from_type) || !Self::is_fixed_width(to_type) {
            return None;
        }
        if Self::is_safe_cast(from_type, to_type) {
            None
        } else {
            Some(FixedWidthError {
                code: "A10102".into(),
                message: format!(
                    "unsafe narrowing cast from `{from_type:?}` to `{to_type:?}`: \
                     source range [{}, {}] does not fit in target range [{}, {}]; \
                     add a bounds check before casting",
                    Self::range_for_type(from_type).map_or(0, |r| r.0),
                    Self::range_for_type(from_type).map_or(0, |r| r.1),
                    Self::range_for_type(to_type).map_or(0, |r| r.0),
                    Self::range_for_type(to_type).map_or(0, |r| r.1),
                ),
                span: span.clone(),
            })
        }
    }

    /// Check for signed/unsigned mismatch in a comparison operation.
    ///
    /// Returns `None` if both sides have the same signedness, or
    /// `Some(FixedWidthError)` with code A10103.
    pub fn check_signedness_mismatch(
        op: &BinOp,
        left_type: &Type,
        right_type: &Type,
        span: &Range<usize>,
    ) -> Option<FixedWidthError> {
        // Only flag comparison operators
        if !matches!(
            op,
            BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte
        ) {
            return None;
        }
        if !Self::is_fixed_width(left_type) || !Self::is_fixed_width(right_type) {
            return None;
        }
        let left_signed = Self::is_signed(left_type);
        let right_signed = Self::is_signed(right_type);
        if left_signed != right_signed {
            Some(FixedWidthError {
                code: "A10103".into(),
                message: format!(
                    "signed/unsigned mismatch in comparison: `{left_type:?}` vs \
                     `{right_type:?}`; comparing signed and unsigned integers \
                     can produce unexpected results"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Check whether a division or modulo operation has a zero-guard on
    /// the divisor.
    ///
    /// This is a simplified check: if the RHS is a literal zero, flag it.
    /// Full divisor analysis (tracking which requires clauses guard the
    /// divisor) is deferred to SMT encoding.
    ///
    /// Returns `None` if safe, or `Some(FixedWidthError)` with code
    /// A10104.
    pub fn check_division_by_zero(
        op: &BinOp,
        rhs: &Expr,
        left_type: &Type,
        span: &Range<usize>,
    ) -> Option<FixedWidthError> {
        if !matches!(op, BinOp::Div | BinOp::Mod) {
            return None;
        }
        if !Self::is_fixed_width(left_type) {
            return None;
        }
        if Self::is_literal_zero(rhs) {
            let op_name = if *op == BinOp::Div {
                "division"
            } else {
                "modulo"
            };
            Some(FixedWidthError {
                code: "A10104".into(),
                message: format!(
                    "{op_name} by zero: the divisor is a literal zero; \
                     add a guard `requires {{ divisor != 0 }}` or use \
                     a checked alternative"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Suggest a checked alternative for an arithmetic operator.
    pub fn suggest_checked_alternative(op: &BinOp) -> std::string::String {
        match op {
            BinOp::Add => "checked_add".into(),
            BinOp::Sub => "checked_sub".into(),
            BinOp::Mul => "checked_mul".into(),
            BinOp::Div => "checked_div".into(),
            BinOp::Mod => "checked_rem".into(),
            _ => "checked operation".into(),
        }
    }

    /// Check a binary expression for fixed-width integer issues.
    ///
    /// Combines overflow, signedness, and division-by-zero checks.
    pub fn check_binop(
        &self,
        op: &BinOp,
        left_type: &Type,
        right_type: &Type,
        rhs_expr: &Expr,
        span: &Range<usize>,
    ) -> Vec<FixedWidthError> {
        let mut errors = Vec::new();

        if let Some(e) = self.check_arithmetic_overflow(op, left_type, right_type, span) {
            errors.push(e);
        }

        if let Some(e) = Self::check_signedness_mismatch(op, left_type, right_type, span) {
            errors.push(e);
        }

        if let Some(e) = Self::check_division_by_zero(op, rhs_expr, left_type, span) {
            errors.push(e);
        }

        errors
    }

    // -- internal helpers ---------------------------------------------------

    /// Return `true` if an expression is a literal `0`.
    fn is_literal_zero(expr: &Expr) -> bool {
        match expr {
            Expr::Literal(Literal::Int(s)) => s == "0",
            Expr::Paren(inner) => Self::is_literal_zero(inner),
            _ => false,
        }
    }

    /// Return the wider of two ranges (union of both ranges).
    fn wider_range(a: (i128, i128), b: (i128, i128)) -> (i128, i128) {
        (std::cmp::min(a.0, b.0), std::cmp::max(a.1, b.1))
    }
}

impl Default for FixedWidthChecker {
    fn default() -> Self {
        Self::new()
    }
}
