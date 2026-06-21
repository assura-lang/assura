use super::*;

// Expression value extraction helpers
// ---------------------------------------------------------------------------

/// Extract an integer literal value from an expression.
/// Returns `None` for non-literal or non-integer expressions.
pub(crate) fn extract_int_literal(expr: &SpExpr) -> Option<i64> {
    match &expr.node {
        Expr::Literal(Literal::Int(s)) => s.parse::<i64>().ok(),
        Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: inner,
        } => {
            if let Expr::Literal(Literal::Int(s)) = &inner.node {
                s.parse::<i64>().ok().map(|v| -v)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extract a float literal value from an expression.
pub(crate) fn extract_float_literal(expr: &SpExpr) -> Option<f64> {
    match &expr.node {
        Expr::Literal(Literal::Float(s)) => s.parse::<f64>().ok(),
        Expr::Literal(Literal::Int(s)) => s.parse::<f64>().ok(),
        _ => None,
    }
}

/// Extract a string identifier from an expression.
pub(crate) fn extract_ident(expr: &SpExpr) -> Option<&str> {
    match &expr.node {
        Expr::Ident(name) => Some(name.as_str()),
        _ => None,
    }
}

/// Extract a key-value pair from a BinOp expression (e.g., `name = value`).
pub(crate) fn extract_kv_pair(expr: &SpExpr) -> Option<(&str, &SpExpr)> {
    match &expr.node {
        Expr::BinOp {
            op: BinOp::Eq,
            lhs,
            rhs,
        } => {
            if let Expr::Ident(key) = &lhs.as_ref().node {
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
pub(crate) fn extract_call(expr: &SpExpr) -> Option<(&str, &[SpExpr])> {
    match &expr.node {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = &func.as_ref().node {
                Some((name.as_str(), args.as_slice()))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extract multiple key-value pairs from a block or list expression.
pub(crate) fn extract_kv_pairs(expr: &SpExpr) -> Vec<(&str, &SpExpr)> {
    match &expr.node {
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
    modified: std::collections::HashSet<String>,
}

impl FrameChecker {
    /// Create a new frame checker from modifies clause body expressions.
    ///
    /// Extracts variable/field names from the modifies clause and stores
    /// them as the "modified" set.
    pub fn new(modifies_clauses: &[&SpExpr]) -> Self {
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
    pub fn modified_set(&self) -> &std::collections::HashSet<String> {
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
    pub fn frame_axiom_vars(&self, ensures_body: &SpExpr) -> Vec<String> {
        if !self.has_modifies() {
            return Vec::new();
        }

        let old_refs = collect_old_references(ensures_body);
        let ident_refs = collect_ident_references(ensures_body);

        // Collect all referenced variables (both in old() and directly)
        let mut all_refs: std::collections::HashSet<String> = std::collections::HashSet::new();
        for r in &old_refs {
            all_refs.insert(r.clone());
        }
        for r in &ident_refs {
            all_refs.insert(r.clone());
        }

        // Variables NOT in the modifies set get frame axioms
        let mut frame_vars: Vec<String> = all_refs
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
