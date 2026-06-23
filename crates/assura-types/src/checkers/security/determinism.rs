use super::*;

// ---------------------------------------------------------------------------
// T067: CONC.3 Determinism contracts
// ---------------------------------------------------------------------------

/// Error from the determinism checker.
pub(crate) type DeterminismError = CheckerError;

/// Checker for determinism contracts.
///
/// Ensures functions marked as `deterministic` do not use
/// non-deterministic constructs (HashMap iteration, random,
/// thread-dependent ordering).
pub(crate) struct DeterminismChecker {
    /// Functions marked as deterministic
    deterministic_fns: HashMap<String, bool>,
    /// Known non-deterministic types/functions
    non_det_sources: Vec<String>,
}

impl DeterminismChecker {
    pub fn new() -> Self {
        Self {
            deterministic_fns: HashMap::new(),
            non_det_sources: vec![
                "HashMap".into(),
                "HashSet".into(),
                "random".into(),
                "rand".into(),
                "thread_rng".into(),
                "SystemTime::now".into(),
                "Instant::now".into(),
            ],
        }
    }

    /// Mark a function as requiring deterministic execution.
    pub fn mark_deterministic(&mut self, fn_name: String) {
        self.deterministic_fns.insert(fn_name, true);
    }

    /// Add a custom non-deterministic source.
    pub fn add_non_det_source(&mut self, source: String) {
        self.non_det_sources.push(source);
    }

    /// Check if a type/function name is non-deterministic.
    pub fn is_non_deterministic(&self, name: &str) -> bool {
        self.non_det_sources
            .iter()
            .any(|s| name.contains(s.as_str()))
    }

    /// Check that a deterministic function does not use non-deterministic constructs.
    /// - A20001: deterministic function uses non-deterministic type/call
    pub fn check_fn_body(
        &self,
        fn_name: &str,
        used_names: &[String],
        span: &Range<usize>,
    ) -> Vec<DeterminismError> {
        let mut errors = Vec::new();
        if !self.deterministic_fns.contains_key(fn_name) {
            return errors; // Not marked deterministic, skip
        }
        for name in used_names {
            if self.is_non_deterministic(name) {
                errors.push(DeterminismError {
                    code: "A20001".into(),
                    message: format!(
                        "deterministic function `{fn_name}` uses non-deterministic `{name}`; \
                         use BTreeMap/BTreeSet or a seeded RNG instead"
                    ),
                    span: span.clone(),
                });
            }
        }
        errors
    }

    /// Check that iteration order is deterministic.
    /// - A20002: iterating over HashMap/HashSet in deterministic context
    pub fn check_iteration(
        &self,
        fn_name: &str,
        iterated_type: &str,
        span: &Range<usize>,
    ) -> Vec<DeterminismError> {
        let mut errors = Vec::new();
        if self.deterministic_fns.contains_key(fn_name)
            && (iterated_type.contains("HashMap") || iterated_type.contains("HashSet"))
        {
            errors.push(DeterminismError {
                code: "A20002".into(),
                message: format!(
                    "deterministic function `{fn_name}` iterates over `{iterated_type}` \
                     which has non-deterministic ordering"
                ),
                span: span.clone(),
            });
        }
        errors
    }
}

impl Default for DeterminismChecker {
    fn default() -> Self {
        Self::new()
    }
}
