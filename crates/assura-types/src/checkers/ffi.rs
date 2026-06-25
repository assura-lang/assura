use super::*;

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
pub(crate) type FfiError = CheckerError;

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
        // If the callee is contracted (has requires/ensures), skip the
        // validation check since the contract already guards the boundary.
        if self.contracted.get(callee) == Some(&true) {
            return errors;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Range<usize> {
        0..10
    }

    #[test]
    fn extern_with_boundary_and_contract_ok() {
        let mut checker = FfiBoundaryChecker::new();
        checker.register_extern("libc_read".into(), TrustBoundary::Untrusted);
        checker.mark_contracted("libc_read".into());
        let errs = checker.check_extern_decl("libc_read", true, true, &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn extern_missing_boundary_a11001() {
        let checker = FfiBoundaryChecker::new();
        let errs = checker.check_extern_decl("ext_fn", false, true, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A11001");
    }

    #[test]
    fn untrusted_extern_without_contract_a11002() {
        let mut checker = FfiBoundaryChecker::new();
        checker.register_extern("unsafe_fn".into(), TrustBoundary::Untrusted);
        let errs = checker.check_extern_decl("unsafe_fn", true, false, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A11002");
    }

    #[test]
    fn ffi_call_unvalidated_result_a11003() {
        let mut checker = FfiBoundaryChecker::new();
        checker.register_extern("ext_read".into(), TrustBoundary::Untrusted);
        let errs = checker.check_ffi_call("ext_read", false, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A11003");
    }

    #[test]
    fn ffi_call_contracted_skips_validation() {
        let mut checker = FfiBoundaryChecker::new();
        checker.register_extern("ext_read".into(), TrustBoundary::Untrusted);
        checker.mark_contracted("ext_read".into());
        let errs = checker.check_ffi_call("ext_read", false, &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn unsafe_outside_ffi_wrapper_a11004() {
        let checker = FfiBoundaryChecker::new();
        let errs = checker.check_unsafe_confinement("my_fn", false, true, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A11004");
    }

    #[test]
    fn unsafe_inside_ffi_wrapper_ok() {
        let checker = FfiBoundaryChecker::new();
        let errs = checker.check_unsafe_confinement("wrapper", true, true, &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn check_file_aggregates_errors() {
        let mut checker = FfiBoundaryChecker::new();
        checker.register_extern("fn_a".into(), TrustBoundary::Untrusted);
        let externs = vec![
            ("fn_a".to_string(), true, false, span()),  // A11002
            ("fn_b".to_string(), false, true, span()),   // A11001
        ];
        let errs = checker.check_file(&externs);
        assert_eq!(errs.len(), 2);
    }
}

// ---------------------------------------------------------------------------
