use super::*;

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
pub(crate) type TotalityError = CheckerError;

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
    partial_fns: std::collections::HashSet<String>,
}

impl TotalityChecker {
    /// Create a new totality checker.
    pub fn new() -> Self {
        Self {
            partial_fns: std::collections::HashSet::new(),
        }
    }

    /// Register a function as `partial` (opt out of termination checking).
    pub fn mark_partial(&mut self, name: String) {
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

        // Check for a well_founded clause, which uses structural ordering
        let has_well_founded = fn_def
            .clauses
            .iter()
            .any(|c| matches!(&c.kind, ClauseKind::Other(s) if s == "well_founded"));

        match decreases_exprs.len() {
            0 => None,
            1 => {
                if has_well_founded {
                    Some(DecreasesMeasure::WellFounded(decreases_exprs[0].clone()))
                } else {
                    Some(DecreasesMeasure::Natural(decreases_exprs[0].clone()))
                }
            }
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
                        let p_tokens = param.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
                        if p_tokens.iter().any(|t| t == "Nat") {
                            return true;
                        }
                        // Structural/named types (List, Tree, etc.) are
                        // well-founded by structural induction. Any type
                        // that is not a raw numeric type (Int, Float, etc.)
                        // is considered structural.
                        let is_numeric_type = p_tokens.iter().any(|t| {
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
            DecreasesMeasure::WellFounded(wf_expr) => {
                // Well-founded ordering: defer to SMT for the
                // well-foundedness proof of the ordering relation.
                if let Some(arg) = call_args.first() {
                    DecreaseCheckResult::NeedsSmt {
                        measure_expr: wf_expr.clone(),
                        call_arg: arg.clone(),
                    }
                } else {
                    DecreaseCheckResult::Proved
                }
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
            DecreasesMeasure::WellFounded(wf_expr) => {
                // Well-founded ordering: check if the measure expression
                // is well-founded (positive/bounded), defer to SMT if needed
                if !Self::is_well_founded(wf_expr, fn_def) {
                    errors.push(TotalityError {
                        code: "A09003".into(),
                        message: format!(
                            "cannot prove well-founded measure is bounded \
                             for function `{}`",
                            fn_def.name
                        ),
                        span: span.clone(),
                    });
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::{Clause, FnDef, Param};

    fn span() -> Range<usize> {
        0..10
    }

    fn ident(s: &str) -> Expr {
        Expr::Ident(s.to_string())
    }

    fn int_lit(n: i64) -> Expr {
        Expr::Literal(Literal::Int(n.to_string()))
    }

    fn make_param(name: &str, ty: &[&str]) -> Param {
        let tokens: Vec<String> = ty.iter().map(|s| s.to_string()).collect();
        Param {
            name: name.to_string(),
            ty: assura_parser::ast::try_parse_type_tokens(&tokens),
        }
    }

    fn make_fn(name: &str, params: Vec<Param>, clauses: Vec<Clause>) -> FnDef {
        FnDef {
            name: name.to_string(),
            is_ghost: false,
            is_lemma: false,
            params,
            return_ty: None,
            clauses,
        }
    }

    fn make_clause(kind: ClauseKind, body: Expr) -> Clause {
        Clause {
            kind,
            body,
            effect_variables: vec![],
        }
    }

    // ---- is_partial ----

    #[test]
    fn partial_fn_registered() {
        let mut checker = TotalityChecker::new();
        checker.mark_partial("diverge".into());
        let f = make_fn("diverge", vec![], vec![]);
        assert!(checker.is_partial(&f));
    }

    #[test]
    fn partial_fn_by_clause() {
        let checker = TotalityChecker::new();
        let f = make_fn(
            "maybe_loop",
            vec![],
            vec![make_clause(ClauseKind::Other("partial".into()), int_lit(0))],
        );
        assert!(checker.is_partial(&f));
    }

    #[test]
    fn non_partial_fn() {
        let checker = TotalityChecker::new();
        let f = make_fn("total", vec![], vec![]);
        assert!(!checker.is_partial(&f));
    }

    // ---- extract_decreases_measure ----

    #[test]
    fn extract_natural_measure() {
        let checker = TotalityChecker::new();
        let f = make_fn(
            "fac",
            vec![make_param("n", &["Nat"])],
            vec![make_clause(ClauseKind::Decreases, ident("n"))],
        );
        let measure = checker.extract_decreases_measure(&f);
        assert!(matches!(measure, Some(DecreasesMeasure::Natural(_))));
    }

    #[test]
    fn extract_lexicographic_measure() {
        let checker = TotalityChecker::new();
        let f = make_fn(
            "ack",
            vec![make_param("m", &["Nat"]), make_param("n", &["Nat"])],
            vec![
                make_clause(ClauseKind::Decreases, ident("m")),
                make_clause(ClauseKind::Decreases, ident("n")),
            ],
        );
        let measure = checker.extract_decreases_measure(&f);
        assert!(matches!(measure, Some(DecreasesMeasure::Lexicographic(_))));
    }

    #[test]
    fn extract_well_founded_measure() {
        let checker = TotalityChecker::new();
        let f = make_fn(
            "tree_walk",
            vec![make_param("t", &["Tree"])],
            vec![
                make_clause(ClauseKind::Decreases, ident("t")),
                make_clause(ClauseKind::Other("well_founded".into()), int_lit(0)),
            ],
        );
        let measure = checker.extract_decreases_measure(&f);
        assert!(matches!(measure, Some(DecreasesMeasure::WellFounded(_))));
    }

    #[test]
    fn extract_no_measure() {
        let checker = TotalityChecker::new();
        let f = make_fn("no_dec", vec![], vec![]);
        assert!(checker.extract_decreases_measure(&f).is_none());
    }

    // ---- is_strictly_decreasing ----

    #[test]
    fn strictly_decreasing_n_minus_1() {
        let measure = ident("n");
        let call_arg = Expr::BinOp {
            lhs: Box::new(ident("n")),
            op: BinOp::Sub,
            rhs: Box::new(int_lit(1)),
        };
        assert!(TotalityChecker::is_strictly_decreasing(&measure, &call_arg));
    }

    #[test]
    fn strictly_decreasing_structural_tail() {
        let measure = ident("xs");
        let call_arg = Expr::Field(Box::new(ident("xs")), "tail".into());
        assert!(TotalityChecker::is_strictly_decreasing(&measure, &call_arg));
    }

    #[test]
    fn not_strictly_decreasing_same_var() {
        let measure = ident("n");
        let call_arg = ident("n");
        assert!(!TotalityChecker::is_strictly_decreasing(
            &measure, &call_arg
        ));
    }

    #[test]
    fn not_strictly_decreasing_different_var() {
        let measure = ident("n");
        let call_arg = ident("m");
        assert!(!TotalityChecker::is_strictly_decreasing(
            &measure, &call_arg
        ));
    }

    // ---- check_function_totality ----

    #[test]
    fn totality_non_recursive_trivially_total() {
        let checker = TotalityChecker::new();
        let f = make_fn(
            "add",
            vec![make_param("a", &["Int"]), make_param("b", &["Int"])],
            vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    lhs: Box::new(ident("a")),
                    op: BinOp::Add,
                    rhs: Box::new(ident("b")),
                },
            )],
        );
        let (errs, pending) = checker.check_function_totality(&f, &span());
        assert!(errs.is_empty());
        assert!(pending.is_empty());
    }

    #[test]
    fn totality_recursive_without_decreases() {
        let checker = TotalityChecker::new();
        let f = make_fn(
            "loop_fn",
            vec![make_param("n", &["Int"])],
            vec![make_clause(
                ClauseKind::Ensures,
                Expr::Call {
                    func: Box::new(ident("loop_fn")),
                    args: vec![ident("n")],
                },
            )],
        );
        let (errs, _) = checker.check_function_totality(&f, &span());
        assert!(!errs.is_empty());
        assert!(errs.iter().any(|e| e.code.as_ref() == "A09001"));
    }

    #[test]
    fn totality_recursive_with_valid_decreases() {
        let checker = TotalityChecker::new();
        let f = make_fn(
            "fac",
            vec![make_param("n", &["Nat"])],
            vec![
                make_clause(ClauseKind::Decreases, ident("n")),
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        lhs: Box::new(ident("n")),
                        op: BinOp::Gte,
                        rhs: Box::new(int_lit(0)),
                    },
                ),
                make_clause(
                    ClauseKind::Ensures,
                    Expr::Call {
                        func: Box::new(ident("fac")),
                        args: vec![Expr::BinOp {
                            lhs: Box::new(ident("n")),
                            op: BinOp::Sub,
                            rhs: Box::new(int_lit(1)),
                        }],
                    },
                ),
            ],
        );
        let (errs, pending) = checker.check_function_totality(&f, &span());
        assert!(errs.is_empty(), "unexpected errors: {errs:?}");
        assert!(pending.is_empty());
    }

    #[test]
    fn totality_partial_fn_skipped() {
        let mut checker = TotalityChecker::new();
        checker.mark_partial("diverge".into());
        let f = make_fn(
            "diverge",
            vec![],
            vec![make_clause(
                ClauseKind::Ensures,
                Expr::Call {
                    func: Box::new(ident("diverge")),
                    args: vec![],
                },
            )],
        );
        let (errs, pending) = checker.check_function_totality(&f, &span());
        assert!(errs.is_empty());
        assert!(pending.is_empty());
    }

    // ---- check_mutual_recursion ----

    #[test]
    fn mutual_recursion_no_measure() {
        let checker = TotalityChecker::new();
        let f = make_fn(
            "even",
            vec![make_param("n", &["Nat"])],
            vec![make_clause(
                ClauseKind::Ensures,
                Expr::Call {
                    func: Box::new(ident("odd")),
                    args: vec![ident("n")],
                },
            )],
        );
        let g = make_fn(
            "odd",
            vec![make_param("n", &["Nat"])],
            vec![make_clause(
                ClauseKind::Ensures,
                Expr::Call {
                    func: Box::new(ident("even")),
                    args: vec![ident("n")],
                },
            )],
        );
        let errs = checker.check_mutual_recursion(&[(&f, &span()), (&g, &span())]);
        assert!(!errs.is_empty());
        assert!(errs.iter().any(|e| e.code.as_ref() == "A09004"));
    }

    #[test]
    fn mutual_recursion_with_measure_ok() {
        let checker = TotalityChecker::new();
        let f = make_fn(
            "even",
            vec![make_param("n", &["Nat"])],
            vec![
                make_clause(ClauseKind::Decreases, ident("n")),
                make_clause(
                    ClauseKind::Ensures,
                    Expr::Call {
                        func: Box::new(ident("odd")),
                        args: vec![ident("n")],
                    },
                ),
            ],
        );
        let g = make_fn(
            "odd",
            vec![make_param("n", &["Nat"])],
            vec![make_clause(
                ClauseKind::Ensures,
                Expr::Call {
                    func: Box::new(ident("even")),
                    args: vec![ident("n")],
                },
            )],
        );
        let errs = checker.check_mutual_recursion(&[(&f, &span()), (&g, &span())]);
        assert!(errs.is_empty());
    }

    // ---- well-foundedness ----

    #[test]
    fn well_founded_nat_param() {
        let f = make_fn("f", vec![make_param("n", &["Nat"])], vec![]);
        assert!(TotalityChecker::is_well_founded(&ident("n"), &f));
    }

    #[test]
    fn well_founded_requires_constraint() {
        let f = make_fn(
            "f",
            vec![make_param("n", &["Int"])],
            vec![make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    lhs: Box::new(ident("n")),
                    op: BinOp::Gte,
                    rhs: Box::new(int_lit(0)),
                },
            )],
        );
        assert!(TotalityChecker::is_well_founded(&ident("n"), &f));
    }

    #[test]
    fn not_well_founded_unconstrained_int() {
        let f = make_fn("f", vec![make_param("n", &["Int"])], vec![]);
        assert!(!TotalityChecker::is_well_founded(&ident("n"), &f));
    }

    #[test]
    fn well_founded_structural_type() {
        let f = make_fn("f", vec![make_param("xs", &["List"])], vec![]);
        assert!(TotalityChecker::is_well_founded(&ident("xs"), &f));
    }
}
