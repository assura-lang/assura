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
pub(crate) type TypestateError = CheckerError;

/// A transition in the typestate DFA.
///
/// Each transition is `(operation_name, required_state, next_state)`.
/// The operation can only be called when the object is in `required_state`,
/// and after the call the object moves to `next_state`.
#[derive(Debug, Clone)]
struct Transition {
    operation: String,
    from_state: String,
    to_state: String,
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
    pub(crate) states: Vec<String>,
    /// All declared transitions.
    transitions: Vec<Transition>,
    /// Current state of the tracked variable.
    current: String,
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
        states: Vec<String>,
        transitions: Vec<(String, String, String)>,
        initial_state: String,
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
pub(crate) fn expr_usages(expr: &SpExpr, tracker: &mut UsageTracker) {
    struct UsageVisitor<'a>(&'a mut UsageTracker);
    impl ExprVisitor for UsageVisitor<'_> {
        fn visit_ident(&mut self, name: &str) {
            self.0.use_var(name);
        }
        // Ghost blocks and apply expressions are erased at runtime;
        // do not count usages inside them.
        fn visit_ghost(&mut self, _inner: &SpExpr) {}
        fn visit_apply(&mut self, _name: &str, _args: &[SpExpr]) {}
    }
    let mut v = UsageVisitor(tracker);
    v.visit_expr(expr);
}

// ---------------------------------------------------------------------------
// Source-level typestate check (moved from checks/linear_typestate.rs)
// ---------------------------------------------------------------------------

pub(crate) fn run_typestate_checks_source(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    use assura_parser::ast::{ClauseKind, Decl, Expr, ServiceItem};

    let mut errors = Vec::new();
    for decl in &source.decls {
        if let Decl::Service(s) = &decl.node {
            let states: Vec<String> = s
                .items
                .iter()
                .filter_map(|item| {
                    if let ServiceItem::States(s) = item {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .flatten()
                .collect();

            if states.is_empty() {
                continue;
            }

            let mut transitions = Vec::new();
            for item in &s.items {
                if let ServiceItem::Operation { name, clauses } = item {
                    for clause in clauses {
                        if let ClauseKind::Other(ref k) = clause.kind
                            && (k == "transition" || k == "from_state" || k == "to_state")
                            && let Expr::Raw(tokens) = &clause.body.node
                            && tokens.len() >= 3
                        {
                            transitions.push((name.clone(), tokens[0].clone(), tokens[2].clone()));
                        }
                    }
                }
            }

            if !transitions.is_empty() {
                let initial = states.first().cloned().unwrap_or_default();
                let mut checker =
                    TypestateChecker::new(states, transitions, initial, decl.span.clone());
                for tse in checker.validate_transitions() {
                    errors.push(TypeError {
                        code: tse.code,
                        message: tse.message,
                        span: tse.span,
                        secondary: None,
                    });
                }

                let has_linear_annotation = s.items.iter().any(|item| {
                    if let ServiceItem::Operation { clauses, .. } = item {
                        clauses
                            .iter()
                            .any(|c| matches!(&c.kind, ClauseKind::Other(k) if k == "linear"))
                    } else {
                        false
                    }
                });
                if let Some(tse) = checker.validate_linear(has_linear_annotation) {
                    errors.push(TypeError {
                        code: tse.code,
                        message: tse.message,
                        span: tse.span,
                        secondary: None,
                    });
                }

                let mut branch_checkers: Vec<TypestateChecker> = Vec::new();
                for item in &s.items {
                    if let ServiceItem::Operation { name, clauses } = item {
                        let pre_state = checker.current_state().to_string();
                        if let Err(tse) = checker.transition(name, decl.span.clone()) {
                            errors.push(TypeError {
                                code: tse.code,
                                message: tse.message,
                                span: tse.span,
                                secondary: None,
                            });
                        }

                        let mut usage_tracker = UsageTracker::new();
                        for clause in clauses {
                            expr_usages(&clause.body, &mut usage_tracker);
                        }

                        if !pre_state.is_empty() {
                            branch_checkers.push(TypestateChecker::new(
                                checker.states.clone(),
                                Vec::new(),
                                checker.current_state().to_string(),
                                decl.span.clone(),
                            ));
                        }
                    }
                }

                for pair in branch_checkers.windows(2) {
                    if let Some(tse) = TypestateChecker::check_branch_consistency(
                        &pair[0],
                        &pair[1],
                        decl.span.clone(),
                    ) {
                        errors.push(TypeError {
                            code: tse.code,
                            message: tse.message,
                            span: tse.span,
                            secondary: None,
                        });
                    }
                }
            }
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Range<usize> {
        0..10
    }

    fn sample_checker() -> TypestateChecker {
        TypestateChecker::new(
            vec!["Open".into(), "Closed".into(), "Reading".into()],
            vec![
                ("open".into(), "Closed".into(), "Open".into()),
                ("read".into(), "Open".into(), "Reading".into()),
                ("close".into(), "Open".into(), "Closed".into()),
                ("close".into(), "Reading".into(), "Closed".into()),
            ],
            "Closed".into(),
            span(),
        )
    }

    #[test]
    fn valid_transition_sequence() {
        let mut tc = sample_checker();
        assert_eq!(tc.current_state(), "Closed");
        tc.transition("open", span()).unwrap();
        assert_eq!(tc.current_state(), "Open");
        tc.transition("read", span()).unwrap();
        assert_eq!(tc.current_state(), "Reading");
        tc.transition("close", span()).unwrap();
        assert_eq!(tc.current_state(), "Closed");
    }

    #[test]
    fn invalid_transition_a06001() {
        let mut tc = sample_checker();
        // Try to read while Closed (requires Open)
        let err = tc.transition("read", span());
        assert!(err.is_err());
        let e = err.unwrap_err();
        assert_eq!(e.code.as_ref(), "A06001");
        assert!(e.message.contains("Open"));
    }

    #[test]
    fn undefined_operation_a06001() {
        let mut tc = sample_checker();
        let err = tc.transition("delete", span());
        assert!(err.is_err());
        let e = err.unwrap_err();
        assert_eq!(e.code.as_ref(), "A06001");
        assert!(e.message.contains("not defined"));
    }

    #[test]
    fn validate_linear_false_a06002() {
        let tc = sample_checker();
        let err = tc.validate_linear(false);
        assert_eq!(err.unwrap().code.as_ref(), "A06002");
    }

    #[test]
    fn validate_linear_true_ok() {
        let tc = sample_checker();
        assert!(tc.validate_linear(true).is_none());
    }

    #[test]
    fn validate_transitions_undeclared_state_a06003() {
        let tc = TypestateChecker::new(
            vec!["Open".into(), "Closed".into()],
            vec![("go".into(), "Open".into(), "Flying".into())], // Flying not declared
            "Open".into(),
            span(),
        );
        let errs = tc.validate_transitions();
        assert!(!errs.is_empty());
        assert!(errs.iter().all(|e| e.code.as_ref() == "A06003"));
        assert!(errs.iter().any(|e| e.message.contains("Flying")));
    }

    #[test]
    fn validate_transitions_all_declared_ok() {
        let tc = sample_checker();
        let errs = tc.validate_transitions();
        assert!(errs.is_empty());
    }

    #[test]
    fn branch_consistency_same_state_ok() {
        let mut a = sample_checker();
        let mut b = sample_checker();
        a.transition("open", span()).unwrap();
        b.transition("open", span()).unwrap();
        let err = TypestateChecker::check_branch_consistency(&a, &b, span());
        assert!(err.is_none());
    }

    #[test]
    fn branch_consistency_different_state_a06004() {
        let mut a = sample_checker();
        let b = sample_checker(); // stays Closed
        a.transition("open", span()).unwrap(); // moves to Open
        let err = TypestateChecker::check_branch_consistency(&a, &b, span());
        assert_eq!(err.unwrap().code.as_ref(), "A06004");
    }
}

// ---------------------------------------------------------------------------
