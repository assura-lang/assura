use super::*;

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
