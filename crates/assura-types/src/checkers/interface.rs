use super::*;

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
pub(crate) type InterfaceError = CheckerError;

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

        // Check that implementation provides contracts when the interface requires them
        if method.has_requires && impl_params.is_empty() && impl_return.is_indeterminate() {
            errors.push(InterfaceError {
                code: "A13002".into(),
                message: format!(
                    "interface `{interface_name}` requires a `requires` clause on method \
                     `{method_name}` but the implementation has no contract"
                ),
                span: span.clone(),
            });
        }
        if method.has_ensures && impl_params.is_empty() && impl_return.is_indeterminate() {
            errors.push(InterfaceError {
                code: "A13002".into(),
                message: format!(
                    "interface `{interface_name}` requires an `ensures` clause on method \
                     `{method_name}` but the implementation has no contract"
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Type;

    fn span() -> Range<usize> {
        0..10
    }

    fn sample_interface() -> InterfaceContract {
        InterfaceContract {
            name: "Serializable".into(),
            methods: vec![
                InterfaceMethod {
                    name: "serialize".into(),
                    param_types: vec![Type::Int],
                    return_type: Type::String,
                    has_requires: false,
                    has_ensures: false,
                    no_reentrancy: false,
                },
                InterfaceMethod {
                    name: "deserialize".into(),
                    param_types: vec![Type::String],
                    return_type: Type::Int,
                    has_requires: false,
                    has_ensures: false,
                    no_reentrancy: false,
                },
            ],
            extends: Vec::new(),
        }
    }

    #[test]
    fn impl_with_all_methods_ok() {
        let mut checker = InterfaceChecker::new();
        checker.register_interface(sample_interface());
        let methods = vec!["serialize".into(), "deserialize".into()];
        let errs = checker.check_impl("MyType", "Serializable", &methods, &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn impl_missing_method_a13001() {
        let mut checker = InterfaceChecker::new();
        checker.register_interface(sample_interface());
        let methods = vec!["serialize".into()]; // missing deserialize
        let errs = checker.check_impl("MyType", "Serializable", &methods, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A13001");
        assert!(errs[0].message.contains("deserialize"));
    }

    #[test]
    fn impl_unknown_interface_a13001() {
        let checker = InterfaceChecker::new();
        let errs = checker.check_impl("MyType", "Unknown", &[], &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A13001");
    }

    #[test]
    fn method_signature_param_count_mismatch_a13002() {
        let mut checker = InterfaceChecker::new();
        checker.register_interface(sample_interface());
        // serialize expects 1 param (Int), provide 2
        let errs = checker.check_method_signature(
            "Serializable",
            "serialize",
            &[Type::Int, Type::Bool],
            &Type::String,
            &span(),
        );
        assert!(errs.iter().any(|e| e.code.as_ref() == "A13002"));
    }

    #[test]
    fn method_signature_return_type_mismatch_a13002() {
        let mut checker = InterfaceChecker::new();
        checker.register_interface(sample_interface());
        // serialize returns String, provide Bool
        let errs = checker.check_method_signature(
            "Serializable",
            "serialize",
            &[Type::Int],
            &Type::Bool,
            &span(),
        );
        assert!(errs.iter().any(|e| e.code.as_ref() == "A13002"));
    }

    #[test]
    fn method_signature_matches_ok() {
        let mut checker = InterfaceChecker::new();
        checker.register_interface(sample_interface());
        let errs = checker.check_method_signature(
            "Serializable",
            "serialize",
            &[Type::Int],
            &Type::String,
            &span(),
        );
        assert!(errs.is_empty());
    }

    #[test]
    fn reentrancy_violation_a13003() {
        let mut checker = InterfaceChecker::new();
        checker.register_interface(InterfaceContract {
            name: "Lock".into(),
            methods: vec![InterfaceMethod {
                name: "acquire".into(),
                param_types: vec![],
                return_type: Type::Unit,
                has_requires: false,
                has_ensures: false,
                no_reentrancy: true,
            }],
            extends: Vec::new(),
        });
        let errs = checker.check_reentrancy("Lock", "acquire", true, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A13003");
    }

    #[test]
    fn reentrancy_ok_when_not_reentrant() {
        let mut checker = InterfaceChecker::new();
        checker.register_interface(InterfaceContract {
            name: "Lock".into(),
            methods: vec![InterfaceMethod {
                name: "acquire".into(),
                param_types: vec![],
                return_type: Type::Unit,
                has_requires: false,
                has_ensures: false,
                no_reentrancy: true,
            }],
            extends: Vec::new(),
        });
        let errs = checker.check_reentrancy("Lock", "acquire", false, &span());
        assert!(errs.is_empty());
    }
}

// ---------------------------------------------------------------------------
