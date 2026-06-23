use super::*;

// T059: SEC.3 Constant-time execution
// ---------------------------------------------------------------------------

/// Error from the constant-time checker.
pub(crate) type ConstantTimeError = CheckerError;

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
    pub fn references_secret(&self, expr: &SpExpr) -> bool {
        struct SecretChecker<'a> {
            secrets: &'a HashMap<String, bool>,
            found: bool,
        }
        impl ExprVisitor for SecretChecker<'_> {
            fn visit_ident(&mut self, name: &str) {
                if self.secrets.contains_key(name) {
                    self.found = true;
                }
            }
        }
        let mut c = SecretChecker {
            secrets: &self.secrets,
            found: false,
        };
        c.visit_expr(expr);
        c.found
    }

    /// Check that branches do not depend on secret data.
    /// - A14001: branch condition depends on secret data (timing leak)
    pub fn check_branch(&self, condition: &SpExpr, span: &Range<usize>) -> Vec<ConstantTimeError> {
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
    pub fn check_index(&self, index_expr: &SpExpr, span: &Range<usize>) -> Vec<ConstantTimeError> {
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
    pub fn check_expr(&self, expr: &SpExpr, span: &Range<usize>) -> Vec<ConstantTimeError> {
        let mut errors = Vec::new();
        match &expr.node {
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
