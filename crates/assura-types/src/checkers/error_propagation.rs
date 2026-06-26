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
        self.frame_axiom_vars_with_candidates(ensures_body, &[])
    }

    /// Like [`Self::frame_axiom_vars`], but also considers explicit candidate
    /// names (typically contract parameters / input fields). This strengthens
    /// frame reasoning when ensures only mentions modified state or `result`
    /// while unmodified inputs must still be framed for soundness.
    pub fn frame_axiom_vars_with_candidates(
        &self,
        ensures_body: &SpExpr,
        candidates: &[String],
    ) -> Vec<String> {
        if !self.has_modifies() {
            return Vec::new();
        }

        let old_refs = collect_old_references(ensures_body);
        let ident_refs = collect_ident_references(ensures_body);

        // Collect all referenced variables (both in old() and directly),
        // plus any explicit candidates (params/inputs).
        let mut all_refs: std::collections::HashSet<String> = std::collections::HashSet::new();
        for r in &old_refs {
            all_refs.insert(r.clone());
        }
        for r in &ident_refs {
            all_refs.insert(r.clone());
        }
        for c in candidates {
            if c != "result" && !c.starts_with("__") {
                all_refs.insert(c.clone());
            }
        }

        // Variables NOT in the modifies set get frame axioms
        let mut frame_vars: Vec<String> = all_refs
            .into_iter()
            .filter(|name| !self.is_modified_or_under_modified(name))
            .collect();

        frame_vars.sort();
        frame_vars.dedup();
        frame_vars
    }

    /// Returns true if `name` is modified or is a sub-field/prefix of a
    /// modified target (e.g. `node.keys` when `node` is modified).
    pub fn is_modified_or_under_modified(&self, name: &str) -> bool {
        if self.modified.contains(name) {
            return true;
        }
        self.modified
            .iter()
            .any(|m| name.starts_with(&format!("{m}.")) || m.starts_with(&format!("{name}.")))
    }

    /// Returns true if a variable name is in the modifies set.
    pub fn is_modified(&self, name: &str) -> bool {
        self.modified.contains(name)
    }

    /// Check ensures clause for references to variables that appear to be
    /// modified (referenced directly, not via `old()`) but are not in the
    /// modifies set. Returns A14002 errors.
    ///
    /// The heuristic: if a variable `x` appears both as `old(x)` and as
    /// bare `x` in an ensures clause, the contract implies `x` changes.
    /// If `x` is not in the modifies set, that violates the frame contract.
    ///
    /// **Frame assertion exclusion**: Patterns like `x == old(x)` or
    /// `old(x) == x` are frame assertions (asserting `x` did NOT change),
    /// not modifications. These are excluded from A14002 detection.
    pub fn check_ensures_modifications(
        &self,
        ensures_body: &SpExpr,
        span: &Range<usize>,
    ) -> Vec<TypeError> {
        if !self.has_modifies() {
            return Vec::new();
        }
        let mut errors = Vec::new();
        let old_refs = collect_old_references(ensures_body);
        let ident_refs = collect_ident_references(ensures_body);
        let frame_eq_vars = collect_frame_equality_vars(ensures_body);

        // Variables that appear both in old() and as bare idents imply
        // modification (e.g., `ensures { x > old(x) }`), UNLESS the
        // co-occurrence is exclusively in a frame equality pattern
        // (`x == old(x)` or `old(x) == x`).
        for name in &old_refs {
            if ident_refs.contains(name)
                && !self.is_modified_or_under_modified(name)
                && !frame_eq_vars.contains(name)
            {
                errors.push(TypeError {
                    code: "A14002".into(),
                    message: format!(
                        "`{name}` appears modified in ensures clause (both `{name}` and \
                         `old({name})` referenced) but is not in the modifies set"
                    ),
                    span: span.clone(),
                    secondary: None,
                });
            }
        }
        errors
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
// Source-level error propagation check
// ---------------------------------------------------------------------------

/// Collect identifier-like tokens from a clause body.
fn clause_body_tokens(body: &assura_parser::ast::Expr) -> Vec<String> {
    use assura_parser::ast::Expr;
    match body {
        Expr::Raw(tokens) => tokens.clone(),
        Expr::Ident(name) => vec![name.clone()],
        _ => Vec::new(),
    }
}

impl ErrorPropagationChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        use assura_parser::ast::{ClauseKind, Decl, Expr};

        let mut checker = Self::new();
        let mut errors = Vec::new();

        // Pass 1: discover error policies from contracts
        for decl in &source.decls {
            if let Decl::Contract(c) = &decl.node {
                let mut policy = ErrorPolicy::default();
                for clause in &c.clauses {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && k == "must_propagate"
                    {
                        let tokens = clause_body_tokens(&clause.body.node);
                        policy.must_propagate.extend(tokens);
                    }
                    if let ClauseKind::Other(ref k) = clause.kind
                        && k == "must_check"
                    {
                        let tokens = clause_body_tokens(&clause.body.node);
                        policy.must_check.extend(tokens);
                    }
                    if let ClauseKind::Other(ref k) = clause.kind
                        && k == "must_not_mask"
                    {
                        let tokens = clause_body_tokens(&clause.body.node);
                        if tokens.len() >= 2 {
                            policy
                                .must_not_mask
                                .push((tokens[0].clone(), tokens[1].clone()));
                        }
                    }
                    if clause.kind == ClauseKind::MustNot {
                        let tokens = clause_body_tokens(&clause.body.node);
                        if tokens.len() >= 2 {
                            policy
                                .must_not_mask
                                .push((tokens[0].clone(), tokens[1].clone()));
                        }
                    }
                    if let ClauseKind::Other(ref k) = clause.kind
                        && k == "must_preserve_detail"
                    {
                        let tokens = clause_body_tokens(&clause.body.node);
                        policy.must_preserve_detail.extend(tokens);
                    }
                }
                if !policy.must_propagate.is_empty()
                    || !policy.must_check.is_empty()
                    || !policy.must_not_mask.is_empty()
                    || !policy.must_preserve_detail.is_empty()
                {
                    checker.register_policy(c.name.clone(), policy);
                }
            }
        }

        // Pass 2: check functions for propagation / masking violations
        for decl in &source.decls {
            if let Decl::FnDef(f) = &decl.node {
                let rt_tokens = f
                    .return_ty
                    .as_ref()
                    .map(|t| t.to_tokens())
                    .unwrap_or_default();
                let returns_error = rt_tokens.iter().any(|t| t == "Result" || t == "Error");

                for clause in &f.clauses {
                    if returns_error
                        && clause.kind == ClauseKind::Errors
                        && let Expr::Raw(tokens) = &clause.body.node
                    {
                        for error_code in tokens {
                            if checker.is_must_propagate(error_code) {
                                errors.push(TypeError {
                                    code: "A64001".into(),
                                    message: format!(
                                        "error code `{error_code}` in function `{}` must be \
                                         propagated, not caught",
                                        f.name
                                    ),
                                    span: decl.span.clone(),
                                    secondary: None,
                                });
                            }
                        }
                    }

                    if let ClauseKind::Other(ref k) = clause.kind
                        && k == "catch"
                    {
                        let tokens = clause_body_tokens(&clause.body.node);
                        let error_code = tokens.first().cloned().unwrap_or_default();
                        let action_kw = tokens.get(1).map(|s| s.as_str()).unwrap_or("");
                        let action = match action_kw {
                            "swallow" | "ignore" => ErrorAction::Swallow,
                            "translate" | "translate_to" => {
                                let target = tokens.get(2).cloned().unwrap_or_default();
                                ErrorAction::TranslateTo(target)
                            }
                            "propagate" | "rethrow" => ErrorAction::Propagate,
                            _ => ErrorAction::Handle,
                        };
                        if let Some(te) =
                            checker.validate_catch(&error_code, action, decl.span.clone())
                        {
                            errors.push(TypeError {
                                code: te.code,
                                message: te.message,
                                span: te.span,
                                secondary: None,
                            });
                        }
                    }

                    if returns_error
                        && matches!(clause.kind, ClauseKind::Ensures | ClauseKind::Requires)
                    {
                        let refs = collect_ident_references(&clause.body);
                        for fn_ref in &refs {
                            if let Some(te) =
                                checker.validate_unchecked_call(fn_ref, decl.span.clone())
                            {
                                errors.push(TypeError {
                                    code: te.code,
                                    message: te.message,
                                    span: te.span,
                                    secondary: None,
                                });
                            }
                        }
                    }
                }
            }
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Range<usize> {
        0..10
    }

    // -- ErrorPropagationChecker --

    #[test]
    fn swallow_must_propagate_a12001() {
        let mut checker = ErrorPropagationChecker::new();
        checker.register_policy(
            "io_policy".into(),
            ErrorPolicy {
                must_propagate: vec!["E_IO".into()],
                ..Default::default()
            },
        );
        let err = checker.validate_catch("E_IO", ErrorAction::Swallow, span());
        assert_eq!(err.unwrap().code.as_ref(), "A12001");
    }

    #[test]
    fn propagate_must_propagate_ok() {
        let mut checker = ErrorPropagationChecker::new();
        checker.register_policy(
            "io_policy".into(),
            ErrorPolicy {
                must_propagate: vec!["E_IO".into()],
                ..Default::default()
            },
        );
        let err = checker.validate_catch("E_IO", ErrorAction::Propagate, span());
        assert!(err.is_none());
    }

    #[test]
    fn masked_translation_a12002() {
        let mut checker = ErrorPropagationChecker::new();
        checker.register_policy(
            "sec_policy".into(),
            ErrorPolicy {
                must_not_mask: vec![("E_AUTH".into(), "E_GENERIC".into())],
                ..Default::default()
            },
        );
        let err = checker.validate_catch(
            "E_AUTH",
            ErrorAction::TranslateTo("E_GENERIC".into()),
            span(),
        );
        assert_eq!(err.unwrap().code.as_ref(), "A12002");
    }

    #[test]
    fn allowed_translation_ok() {
        let mut checker = ErrorPropagationChecker::new();
        checker.register_policy(
            "sec_policy".into(),
            ErrorPolicy {
                must_not_mask: vec![("E_AUTH".into(), "E_GENERIC".into())],
                ..Default::default()
            },
        );
        // Translating to something other than E_GENERIC is fine
        let err = checker.validate_catch(
            "E_AUTH",
            ErrorAction::TranslateTo("E_DETAILED".into()),
            span(),
        );
        assert!(err.is_none());
    }

    #[test]
    fn unchecked_call_a12003() {
        let mut checker = ErrorPropagationChecker::new();
        checker.register_policy(
            "policy".into(),
            ErrorPolicy {
                must_check: vec!["dangerous_fn".into()],
                ..Default::default()
            },
        );
        let err = checker.validate_unchecked_call("dangerous_fn", span());
        assert_eq!(err.unwrap().code.as_ref(), "A12003");
    }

    #[test]
    fn unchecked_call_no_policy_ok() {
        let checker = ErrorPropagationChecker::new();
        let err = checker.validate_unchecked_call("safe_fn", span());
        assert!(err.is_none());
    }

    // -- FrameChecker (A14002) --

    #[test]
    fn a14002_frame_assertion_excluded() {
        use assura_parser::ast::Spanned;
        // modifies { x }, ensures { y == old(y) }
        // y == old(y) is a frame assertion => no A14002
        let modifies_body = Spanned::no_span(Expr::Ident("x".into()));
        let fc = FrameChecker::new(&[&modifies_body]);
        let ensures_body = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
            op: BinOp::Eq,
            rhs: Box::new(Spanned::no_span(Expr::Old(Box::new(Spanned::no_span(
                Expr::Ident("y".into()),
            ))))),
        });
        let errors = fc.check_ensures_modifications(&ensures_body, &span());
        assert!(
            errors.is_empty(),
            "y == old(y) is a frame assertion, not A14002: {errors:?}"
        );
    }

    #[test]
    fn a14002_modification_detected() {
        use assura_parser::ast::Spanned;
        // modifies { x }, ensures { y > old(y) }
        // y > old(y) implies y changed => A14002 (y not in modifies)
        let modifies_body = Spanned::no_span(Expr::Ident("x".into()));
        let fc = FrameChecker::new(&[&modifies_body]);
        let ensures_body = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Old(Box::new(Spanned::no_span(
                Expr::Ident("y".into()),
            ))))),
        });
        let errors = fc.check_ensures_modifications(&ensures_body, &span());
        assert!(
            errors.iter().any(|e| e.code.as_ref() == "A14002"),
            "y > old(y) with modifies {{ x }} should produce A14002: {errors:?}"
        );
    }

    #[test]
    fn a14002_modified_var_no_error() {
        use assura_parser::ast::Spanned;
        // modifies { x }, ensures { x > old(x) }
        // x IS in modifies => no A14002
        let modifies_body = Spanned::no_span(Expr::Ident("x".into()));
        let fc = FrameChecker::new(&[&modifies_body]);
        let ensures_body = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Old(Box::new(Spanned::no_span(
                Expr::Ident("x".into()),
            ))))),
        });
        let errors = fc.check_ensures_modifications(&ensures_body, &span());
        assert!(
            errors.is_empty(),
            "x > old(x) with modifies {{ x }} should not be A14002: {errors:?}"
        );
    }

    #[test]
    fn a14002_old_eq_x_reversed() {
        use assura_parser::ast::Spanned;
        // modifies { x }, ensures { old(y) == y }
        // old(y) == y is a frame assertion => no A14002
        let modifies_body = Spanned::no_span(Expr::Ident("x".into()));
        let fc = FrameChecker::new(&[&modifies_body]);
        let ensures_body = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Old(Box::new(Spanned::no_span(
                Expr::Ident("y".into()),
            ))))),
            op: BinOp::Eq,
            rhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
        });
        let errors = fc.check_ensures_modifications(&ensures_body, &span());
        assert!(
            errors.is_empty(),
            "old(y) == y is a frame assertion: {errors:?}"
        );
    }

    // -- FrameChecker (basic) --

    #[test]
    fn frame_checker_empty_has_no_modifies() {
        let fc = FrameChecker::empty();
        assert!(!fc.has_modifies());
    }

    #[test]
    fn frame_checker_with_modifies() {
        use assura_parser::ast::Spanned;
        let body = Spanned::no_span(Expr::Ident("x".into()));
        let fc = FrameChecker::new(&[&body]);
        assert!(fc.has_modifies());
        assert!(fc.is_modified("x"));
        assert!(!fc.is_modified("y"));
    }

    #[test]
    fn frame_checker_is_modified_or_under() {
        use assura_parser::ast::Spanned;
        let body = Spanned::no_span(Expr::Ident("node".into()));
        let fc = FrameChecker::new(&[&body]);
        // "node" is modified, so "node.keys" is under it
        assert!(fc.is_modified_or_under_modified("node"));
        assert!(fc.is_modified_or_under_modified("node.keys"));
        assert!(!fc.is_modified_or_under_modified("other"));
    }
}

// ---------------------------------------------------------------------------
