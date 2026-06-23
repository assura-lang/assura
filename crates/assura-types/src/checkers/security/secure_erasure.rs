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
