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

impl DeterminismChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<crate::TypeError> {
        use assura_parser::ast::{ClauseKind, Expr};

        let mut all_errors = Vec::new();
        let mut checker = Self::new();

        for decl in &source.decls {
            let Some((fn_name, clauses)) = crate::checks::fn_or_contract_name_clauses(&decl.node)
            else {
                continue;
            };

            let is_pure = clauses.iter().any(|c| {
                c.kind == ClauseKind::Effects
                    && matches!(&c.body.node, Expr::Ident(name) if name == "pure")
            });
            if !is_pure {
                continue;
            }

            checker.mark_deterministic(fn_name.to_string());

            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && k == "non_deterministic"
                {
                    for name in collect_ident_references(&clause.body) {
                        checker.add_non_det_source(name);
                    }
                }
            }

            let mut used_names = Vec::new();
            for clause in clauses {
                let refs = collect_ident_references(&clause.body);
                used_names.extend(refs);
            }

            for err in checker.check_fn_body(fn_name, &used_names, &decl.span) {
                all_errors.push(err.into());
            }

            for name in &used_names {
                for err in checker.check_iteration(fn_name, name, &decl.span) {
                    all_errors.push(err.into());
                }
            }
        }

        all_errors
    }
}

impl Default for DeterminismChecker {
    fn default() -> Self {
        Self::new()
    }
}
