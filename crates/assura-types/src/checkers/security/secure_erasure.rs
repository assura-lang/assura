use super::*;

// ---------------------------------------------------------------------------
// T060: SEC.4 Secure erasure
// ---------------------------------------------------------------------------

/// Error from the secure erasure checker.
pub(crate) type SecureErasureError = CheckerError;

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

impl SecureErasureChecker {
    /// AST-walking entry point: scan for `#[sensitive]` params and check erasure.
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        use assura_parser::ast::{BinOp, ClauseKind, Expr, Span};

        let mut checker = SecureErasureChecker::new();
        let mut has_sensitive = false;

        for decl in &source.decls {
            let params = decl.node.params();
            if params.is_empty() {
                continue;
            }
            for param in params {
                let p_tokens = param.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
                let is_sensitive = p_tokens
                    .iter()
                    .any(|t| t == "sensitive" || t == "#[sensitive]");
                if is_sensitive {
                    checker.mark_sensitive(param.name.clone());
                    has_sensitive = true;
                }
            }
        }

        if !has_sensitive {
            return Vec::new();
        }

        let mut errors = Vec::new();
        let sensitive_names = checker.sensitive_names();
        let mut sensitive_decl_span: std::collections::HashMap<String, Span> =
            std::collections::HashMap::new();
        for decl in &source.decls {
            let params = decl.node.params();
            if params.is_empty() {
                continue;
            }
            for param in params {
                let p_tokens = param.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
                if p_tokens
                    .iter()
                    .any(|t| t == "sensitive" || t == "#[sensitive]")
                {
                    sensitive_decl_span
                        .entry(param.name.clone())
                        .or_insert_with(|| decl.span.clone());
                }
            }
        }
        for name in &sensitive_names {
            for decl in &source.decls {
                let clauses = decl.node.clauses();
                if clauses.is_empty() {
                    continue;
                }
                let return_ty_tokens = decl
                    .node
                    .return_ty()
                    .map(|t| t.to_tokens())
                    .unwrap_or_default();

                let has_erasure = clauses.iter().any(|c| {
                    c.kind == ClauseKind::Ensures
                        && super::super::expr_references_var(&c.body, name)
                });
                if has_erasure {
                    checker.mark_zeroized(name.clone());
                }

                for clause in clauses {
                    if clause.kind == ClauseKind::Ensures
                        && let Expr::BinOp {
                            lhs,
                            op: BinOp::Eq,
                            rhs,
                        } = &clause.body.node
                        && let Expr::Ident(src) = &rhs.as_ref().node
                        && src == name
                        && let Expr::Ident(tgt) = &lhs.as_ref().node
                    {
                        let tgt_is_sensitive = checker.sensitive_names().contains(tgt);
                        for err in checker.check_copy(name, tgt, tgt_is_sensitive, &decl.span) {
                            errors.push(err.into());
                        }
                    }
                }

                let fn_return_is_sensitive = return_ty_tokens
                    .iter()
                    .any(|t| t == "sensitive" || t == "#[sensitive]");
                for err in checker.check_return(name, fn_return_is_sensitive, &decl.span) {
                    errors.push(err.into());
                }
            }

            let fallback_span = 0..0usize;
            let scope_span = sensitive_decl_span.get(name).unwrap_or(&fallback_span);
            for err in checker.check_scope_exit(name, scope_span) {
                errors.push(err.into());
            }
        }

        let first_sensitive_span = sensitive_decl_span
            .values()
            .next()
            .cloned()
            .unwrap_or(0..0usize);
        for err in checker.check_all_erased(&first_sensitive_span) {
            errors.push(err.into());
        }

        errors
    }
}
