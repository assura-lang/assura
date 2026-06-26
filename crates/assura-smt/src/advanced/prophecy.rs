// ===========================================================================
// T093: CORE.7 Prophecy variables
// ===========================================================================

/// Ghost state with deferred resolution for future-dependent reasoning.
#[derive(Debug, Clone)]
pub struct ProphecyVariable {
    pub name: String,
    pub resolved: bool,
    pub resolution_value: Option<String>,
    pub constraints: Vec<String>,
}

/// Structured prophecy error with error code and context.
#[derive(Debug, Clone, PartialEq)]
pub struct ProphecyError {
    /// Error code: "A05025" (unresolved) or "A05026" (double-resolved/unconstrained).
    pub code: &'static str,
    /// Human-readable error message.
    pub message: String,
    /// The prophecy variable name involved.
    pub variable: String,
}

/// Manages prophecy variables for verification.
#[derive(Debug, Clone)]
pub struct ProphecyManager {
    variables: std::collections::HashMap<String, ProphecyVariable>,
}

impl ProphecyManager {
    pub fn new() -> Self {
        Self {
            variables: std::collections::HashMap::new(),
        }
    }

    pub fn declare(&mut self, name: String) {
        self.variables.insert(
            name.clone(),
            ProphecyVariable {
                name,
                resolved: false,
                resolution_value: None,
                constraints: Vec::new(),
            },
        );
    }

    pub fn add_constraint(&mut self, name: &str, constraint: String) {
        if let Some(v) = self.variables.get_mut(name) {
            v.constraints.push(constraint);
        }
    }

    pub fn resolve(&mut self, name: &str, value: String) -> Result<(), String> {
        if let Some(v) = self.variables.get_mut(name) {
            if v.resolved {
                return Err(format!("prophecy variable `{name}` already resolved"));
            }
            v.resolved = true;
            v.resolution_value = Some(value);
            Ok(())
        } else {
            Err(format!("unknown prophecy variable `{name}`"))
        }
    }

    /// Check that all prophecy variables are eventually resolved.
    pub fn check_all_resolved(&self) -> Vec<ProphecyError> {
        self.variables
            .iter()
            .filter(|(_, v)| !v.resolved)
            .map(|(n, _)| ProphecyError {
                code: "A05025",
                message: format!("prophecy variable `{n}` was never resolved"),
                variable: n.clone(),
            })
            .collect()
    }

    /// Check for prophecy variables with no constraints (useless).
    pub fn check_unconstrained(&self) -> Vec<ProphecyError> {
        self.variables
            .iter()
            .filter(|(_, v)| v.constraints.is_empty())
            .map(|(n, _)| ProphecyError {
                code: "A05026",
                message: format!("prophecy variable `{n}` has no constraints"),
                variable: n.clone(),
            })
            .collect()
    }

    pub fn variable_count(&self) -> usize {
        self.variables.len()
    }
}

impl Default for ProphecyManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prophecy_new_is_empty() {
        let pm = ProphecyManager::new();
        assert_eq!(pm.variable_count(), 0);
    }

    #[test]
    fn prophecy_default_is_empty() {
        let pm = ProphecyManager::default();
        assert_eq!(pm.variable_count(), 0);
    }

    #[test]
    fn prophecy_declare_increases_count() {
        let mut pm = ProphecyManager::new();
        pm.declare("future_val".into());
        assert_eq!(pm.variable_count(), 1);
    }

    #[test]
    fn prophecy_resolve_succeeds() {
        let mut pm = ProphecyManager::new();
        pm.declare("p".into());
        pm.resolve("p", "42".into()).unwrap();
    }

    #[test]
    fn prophecy_double_resolve_fails() {
        let mut pm = ProphecyManager::new();
        pm.declare("p".into());
        pm.resolve("p", "1".into()).unwrap();
        let err = pm.resolve("p", "2".into()).unwrap_err();
        assert!(err.contains("already resolved"));
    }

    #[test]
    fn prophecy_resolve_unknown_fails() {
        let mut pm = ProphecyManager::new();
        let err = pm.resolve("ghost", "val".into()).unwrap_err();
        assert!(err.contains("unknown prophecy variable"));
    }

    #[test]
    fn prophecy_check_all_resolved_reports_unresolved() {
        let mut pm = ProphecyManager::new();
        pm.declare("a".into());
        pm.declare("b".into());
        pm.resolve("a", "done".into()).unwrap();
        let errors = pm.check_all_resolved();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05025");
        assert_eq!(errors[0].variable, "b");
    }

    #[test]
    fn prophecy_check_all_resolved_empty_when_all_done() {
        let mut pm = ProphecyManager::new();
        pm.declare("p".into());
        pm.resolve("p", "done".into()).unwrap();
        assert!(pm.check_all_resolved().is_empty());
    }

    #[test]
    fn prophecy_check_unconstrained_reports_no_constraints() {
        let mut pm = ProphecyManager::new();
        pm.declare("p".into());
        let errors = pm.check_unconstrained();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05026");
        assert!(errors[0].message.contains("no constraints"));
    }

    #[test]
    fn prophecy_check_unconstrained_ok_with_constraint() {
        let mut pm = ProphecyManager::new();
        pm.declare("p".into());
        pm.add_constraint("p", "p > 0".into());
        assert!(pm.check_unconstrained().is_empty());
    }

    #[test]
    fn prophecy_add_constraint_to_unknown_is_noop() {
        let mut pm = ProphecyManager::new();
        pm.add_constraint("nonexistent", "x > 0".into());
        assert_eq!(pm.variable_count(), 0);
    }
}
