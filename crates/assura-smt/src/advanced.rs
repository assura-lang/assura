// ===========================================================================
// T078: CORE.5 Quantifier triggers (e-matching hints)
// ===========================================================================

/// E-matching trigger patterns for SMT quantifier instantiation.
///
/// Triggers guide the SMT solver's quantifier instantiation by specifying
/// which ground terms should cause a quantified formula to be instantiated.
#[derive(Debug, Clone)]
pub struct TriggerPattern {
    /// The pattern terms (multi-trigger if > 1)
    pub terms: Vec<String>,
    /// Whether this is a user-provided trigger
    pub is_user_provided: bool,
}

/// Manages trigger inference and validation for quantified formulas.
#[derive(Debug, Clone)]
pub struct TriggerManager {
    /// Known function symbols for trigger inference
    known_functions: Vec<String>,
    /// User-specified triggers per quantified formula
    triggers: std::collections::HashMap<String, Vec<TriggerPattern>>,
    /// Last validate_trigger warnings (surfaced by encoder paths for diagnostics).
    last_warnings: Vec<String>,
}

impl TriggerManager {
    pub fn new() -> Self {
        Self {
            known_functions: Vec::new(),
            triggers: std::collections::HashMap::new(),
            last_warnings: Vec::new(),
        }
    }

    pub fn register_function(&mut self, name: String) {
        if !self.known_functions.contains(&name) {
            self.known_functions.push(name);
        }
    }

    /// Known function names (for backend pattern construction).
    pub fn known_functions(&self) -> &[String] {
        &self.known_functions
    }

    pub fn add_trigger(&mut self, formula_name: String, pattern: TriggerPattern) {
        self.triggers.entry(formula_name).or_default().push(pattern);
    }

    /// Infer a trigger pattern from the quantifier body (Debug/string form).
    /// Returns None if no suitable trigger can be inferred.
    ///
    /// Prefer [`Self::infer_trigger_from_expr`] when an AST is available; this
    /// string path remains for callers that only have serialized bodies.
    pub fn infer_trigger(&self, body: &str) -> Option<TriggerPattern> {
        for func in &self.known_functions {
            if body.contains(func.as_str()) {
                return Some(TriggerPattern {
                    terms: vec![format!("{func}(x)")],
                    is_user_provided: false,
                });
            }
        }
        None
    }

    /// Infer trigger patterns from quantifier body AST: function/method calls
    /// that mention the bound variable (or any bound-var mention if `bound_var`
    /// is empty and we fall back to known functions in the body).
    pub fn infer_trigger_from_expr(
        &self,
        body: &assura_ast::SpExpr,
        bound_var: &str,
    ) -> Option<TriggerPattern> {
        let mut terms = Vec::new();
        collect_trigger_terms_from_expr(body, bound_var, &self.known_functions, &mut terms);
        if terms.is_empty() {
            // Fallback: string inference on Debug form when AST scan finds nothing
            // but known functions appear in the body serialization.
            let body_str = format!("{body:?}");
            return self.infer_trigger(&body_str);
        }
        terms.sort();
        terms.dedup();
        Some(TriggerPattern {
            terms,
            is_user_provided: false,
        })
    }

    /// Validate that a trigger pattern mentions only known functions.
    /// Stores warnings on `self` for later retrieval via [`Self::take_last_warnings`].
    pub fn validate_trigger(&mut self, pattern: &TriggerPattern) -> Vec<String> {
        let mut warnings = Vec::new();
        for term in &pattern.terms {
            let has_known = self
                .known_functions
                .iter()
                .any(|f| term.contains(f.as_str()));
            if !has_known && !self.known_functions.is_empty() {
                warnings.push(format!(
                    "trigger term `{term}` does not reference any known function"
                ));
            }
        }
        self.last_warnings = warnings.clone();
        warnings
    }

    /// Drain the most recent validate_trigger warnings.
    pub fn take_last_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.last_warnings)
    }

    pub fn get_triggers(&self, formula_name: &str) -> Option<&[TriggerPattern]> {
        self.triggers.get(formula_name).map(|v| v.as_slice())
    }
}

/// Walk expression tree collecting `func(bound)` / `method(bound)` style trigger terms.
fn collect_trigger_terms_from_expr(
    expr: &assura_ast::SpExpr,
    bound_var: &str,
    known: &[String],
    out: &mut Vec<String>,
) {
    use assura_ast::Expr;
    use assura_types::checkers::expr_references_var;

    let mentions_bound = |e: &assura_ast::SpExpr| -> bool {
        if bound_var.is_empty() {
            true
        } else {
            expr_references_var(e, bound_var)
        }
    };

    match &expr.node {
        Expr::Call { func, args } => {
            if let Expr::Ident(fname) = &func.as_ref().node {
                let refs_bound = args.iter().any(mentions_bound);
                let is_known = known.is_empty() || known.iter().any(|k| k == fname);
                if refs_bound && is_known {
                    out.push(format!("{fname}({bound_var})"));
                }
            }
            for a in args {
                collect_trigger_terms_from_expr(a, bound_var, known, out);
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let refs_bound = mentions_bound(receiver) || args.iter().any(mentions_bound);
            let is_known = known.is_empty() || known.iter().any(|k| k == method);
            if refs_bound && is_known {
                out.push(format!("{method}({bound_var})"));
            }
            collect_trigger_terms_from_expr(receiver, bound_var, known, out);
            for a in args {
                collect_trigger_terms_from_expr(a, bound_var, known, out);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_trigger_terms_from_expr(lhs, bound_var, known, out);
            collect_trigger_terms_from_expr(rhs, bound_var, known, out);
        }
        Expr::UnaryOp { expr: inner, .. } | Expr::Old(inner) | Expr::Ghost(inner) => {
            collect_trigger_terms_from_expr(inner, bound_var, known, out);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_trigger_terms_from_expr(cond, bound_var, known, out);
            collect_trigger_terms_from_expr(then_branch, bound_var, known, out);
            if let Some(eb) = else_branch {
                collect_trigger_terms_from_expr(eb, bound_var, known, out);
            }
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_trigger_terms_from_expr(domain, bound_var, known, out);
            collect_trigger_terms_from_expr(body, bound_var, known, out);
        }
        Expr::Index { expr: e, index } => {
            collect_trigger_terms_from_expr(e, bound_var, known, out);
            collect_trigger_terms_from_expr(index, bound_var, known, out);
        }
        Expr::Field(obj, _) => collect_trigger_terms_from_expr(obj, bound_var, known, out),
        Expr::Block(items) | Expr::Tuple(items) | Expr::List(items) => {
            for e in items {
                collect_trigger_terms_from_expr(e, bound_var, known, out);
            }
        }
        Expr::Apply { args, .. } => {
            for a in args {
                collect_trigger_terms_from_expr(a, bound_var, known, out);
            }
        }
        _ => {}
    }
}

impl Default for TriggerManager {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T073: FMT.4 Codec dispatch (magic-byte routing)
// ===========================================================================

/// Routes decoding to the appropriate codec based on magic bytes.
///
/// Error codes (via assura-types):
/// - A33001: unknown magic bytes
/// - A33002: ambiguous magic bytes (multiple codecs match)
/// - A33003: codec not registered
#[derive(Debug, Clone)]
pub struct CodecDispatcher {
    codecs: Vec<CodecEntry>,
}

#[derive(Debug, Clone)]
pub struct CodecEntry {
    pub name: String,
    pub magic_bytes: Vec<u8>,
    pub magic_offset: usize,
}

impl CodecDispatcher {
    pub fn new() -> Self {
        Self { codecs: Vec::new() }
    }

    pub fn register(&mut self, name: String, magic_bytes: Vec<u8>, offset: usize) {
        self.codecs.push(CodecEntry {
            name,
            magic_bytes,
            magic_offset: offset,
        });
    }

    /// Dispatch: find the codec matching the given data prefix.
    pub fn dispatch(&self, data: &[u8]) -> DispatchResult {
        let mut matches: Vec<&CodecEntry> = Vec::new();
        for codec in &self.codecs {
            let end = codec.magic_offset + codec.magic_bytes.len();
            if data.len() >= end && data[codec.magic_offset..end] == codec.magic_bytes {
                matches.push(codec);
            }
        }
        match matches.len() {
            0 => DispatchResult::Unknown,
            1 => DispatchResult::Matched(matches[0].name.clone()),
            _ => DispatchResult::Ambiguous(matches.iter().map(|c| c.name.clone()).collect()),
        }
    }

    /// Check for ambiguous registrations (overlapping magic bytes).
    pub fn check_ambiguity(&self) -> Vec<(String, String)> {
        let mut conflicts = Vec::new();
        for i in 0..self.codecs.len() {
            for j in (i + 1)..self.codecs.len() {
                let a = &self.codecs[i];
                let b = &self.codecs[j];
                if a.magic_offset == b.magic_offset && a.magic_bytes == b.magic_bytes {
                    conflicts.push((a.name.clone(), b.name.clone()));
                }
            }
        }
        conflicts
    }

    pub fn codec_count(&self) -> usize {
        self.codecs.len()
    }
}

impl Default for CodecDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DispatchResult {
    Matched(String),
    Unknown,
    Ambiguous(Vec<String>),
}

// ---------------------------------------------------------------------------
// Non-Z3 unit tests for MeasureDefinition and axiom logic (T054)
// ---------------------------------------------------------------------------

// ===========================================================================
// T092: CONC.6 Weak memory ordering
// ===========================================================================

/// Models C++ memory ordering semantics for verification.
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryOrdering {
    Relaxed,
    Acquire,
    Release,
    AcqRel,
    SeqCst,
}

/// A memory access with its ordering constraint.
#[derive(Debug, Clone)]
pub struct MemoryAccess {
    pub thread_id: u64,
    pub variable: String,
    pub is_write: bool,
    pub ordering: MemoryOrdering,
    pub sequence_num: u64,
}

/// Verifies weak memory ordering contracts.
#[derive(Debug, Clone)]
pub struct WeakMemoryChecker {
    accesses: Vec<MemoryAccess>,
    happens_before: Vec<(u64, u64)>,
    next_seq: u64,
}

impl WeakMemoryChecker {
    pub fn new() -> Self {
        Self {
            accesses: Vec::new(),
            happens_before: Vec::new(),
            next_seq: 0,
        }
    }

    pub fn record_access(
        &mut self,
        thread_id: u64,
        variable: String,
        is_write: bool,
        ordering: MemoryOrdering,
    ) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.accesses.push(MemoryAccess {
            thread_id,
            variable,
            is_write,
            ordering,
            sequence_num: seq,
        });
        seq
    }

    pub fn add_happens_before(&mut self, before: u64, after: u64) {
        self.happens_before.push((before, after));
    }

    fn is_ordered(&self, a: u64, b: u64) -> bool {
        self.happens_before.iter().any(|&(x, y)| x == a && y == b)
    }

    /// Check for data races: concurrent accesses to same variable with at least one write
    /// and no happens-before relationship.
    pub fn check_data_races(&self) -> Vec<String> {
        let mut races = Vec::new();
        for i in 0..self.accesses.len() {
            for j in (i + 1)..self.accesses.len() {
                let a = &self.accesses[i];
                let b = &self.accesses[j];
                if a.variable == b.variable
                    && a.thread_id != b.thread_id
                    && (a.is_write || b.is_write)
                    && !self.is_ordered(a.sequence_num, b.sequence_num)
                    && !self.is_ordered(b.sequence_num, a.sequence_num)
                {
                    races.push(format!(
                        "data race on `{}` between thread {} and thread {}",
                        a.variable, a.thread_id, b.thread_id
                    ));
                }
            }
        }
        races
    }

    /// Check that release-acquire pairs are consistent.
    pub fn check_release_acquire(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        for a in &self.accesses {
            if a.ordering == MemoryOrdering::Release && a.is_write {
                let has_acquire = self.accesses.iter().any(|b| {
                    b.variable == a.variable
                        && !b.is_write
                        && b.thread_id != a.thread_id
                        && b.ordering == MemoryOrdering::Acquire
                });
                if !has_acquire {
                    warnings.push(format!(
                        "release write on `{}` (thread {}) has no matching acquire read",
                        a.variable, a.thread_id
                    ));
                }
            }
        }
        warnings
    }

    /// Check for relaxed accesses that should be stronger.
    pub fn check_ordering_strength(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        for a in &self.accesses {
            if a.ordering == MemoryOrdering::Relaxed && a.is_write {
                let read_by_other = self
                    .accesses
                    .iter()
                    .any(|b| b.variable == a.variable && b.thread_id != a.thread_id && !b.is_write);
                if read_by_other {
                    warnings.push(format!("relaxed write on `{}` (thread {}) is read by another thread; consider Release ordering", a.variable, a.thread_id));
                }
            }
        }
        warnings
    }

    pub fn access_count(&self) -> usize {
        self.accesses.len()
    }
}

impl Default for WeakMemoryChecker {
    fn default() -> Self {
        Self::new()
    }
}

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

// ===========================================================================
// T094: CORE.8 Liveness contracts
// ===========================================================================

/// Liveness property kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum LivenessKind {
    Eventually,
    LeadsTo,
    EventuallyWithin(u64),
}

/// A liveness obligation.
#[derive(Debug, Clone)]
pub struct LivenessObligation {
    pub name: String,
    pub kind: LivenessKind,
    pub premise: String,
    pub conclusion: String,
    pub verified: bool,
}

/// Manages liveness contracts for verification.
#[derive(Debug, Clone)]
pub struct LivenessChecker {
    obligations: Vec<LivenessObligation>,
    fairness_assumptions: Vec<String>,
}

impl LivenessChecker {
    pub fn new() -> Self {
        Self {
            obligations: Vec::new(),
            fairness_assumptions: Vec::new(),
        }
    }

    pub fn add_obligation(
        &mut self,
        name: String,
        kind: LivenessKind,
        premise: String,
        conclusion: String,
    ) {
        self.obligations.push(LivenessObligation {
            name,
            kind,
            premise,
            conclusion,
            verified: false,
        });
    }

    pub fn add_fairness(&mut self, assumption: String) {
        self.fairness_assumptions.push(assumption);
    }

    pub fn mark_verified(&mut self, name: &str) {
        if let Some(o) = self.obligations.iter_mut().find(|o| o.name == name) {
            o.verified = true;
        }
    }

    /// Check for unverified liveness obligations.
    pub fn check_unverified(&self) -> Vec<String> {
        self.obligations
            .iter()
            .filter(|o| !o.verified)
            .map(|o| {
                format!(
                    "liveness obligation `{}` ({:?}) not verified",
                    o.name, o.kind
                )
            })
            .collect()
    }

    /// Check that eventually_within obligations have reasonable bounds.
    pub fn check_bounded(&self) -> Vec<String> {
        self.obligations
            .iter()
            .filter(|o| matches!(o.kind, LivenessKind::EventuallyWithin(t) if t == 0))
            .map(|o| format!("liveness obligation `{}` has zero time bound", o.name))
            .collect()
    }

    /// Check that leads_to obligations have fairness assumptions.
    pub fn check_fairness(&self) -> Vec<String> {
        if self.fairness_assumptions.is_empty() {
            let leads_to: Vec<_> = self
                .obligations
                .iter()
                .filter(|o| o.kind == LivenessKind::LeadsTo)
                .collect();
            if !leads_to.is_empty() {
                return vec![
                    "leads_to obligations present but no fairness assumptions declared".into(),
                ];
            }
        }
        vec![]
    }

    pub fn obligation_count(&self) -> usize {
        self.obligations.len()
    }
}

impl Default for LivenessChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // TriggerManager
    // -----------------------------------------------------------------------

    #[test]
    fn trigger_new_is_empty() {
        let tm = TriggerManager::new();
        assert!(tm.get_triggers("any").is_none());
    }

    #[test]
    fn trigger_default_is_empty() {
        let tm = TriggerManager::default();
        assert!(tm.get_triggers("any").is_none());
    }

    #[test]
    fn trigger_register_function_deduplicates() {
        let mut tm = TriggerManager::new();
        tm.register_function("f".into());
        tm.register_function("f".into());
        // infer should still produce a single-term trigger
        let t = tm.infer_trigger("f(x) > 0").unwrap();
        assert_eq!(t.terms.len(), 1);
    }

    #[test]
    fn trigger_infer_finds_known_function() {
        let mut tm = TriggerManager::new();
        tm.register_function("hash".into());
        let t = tm.infer_trigger("hash(x) == hash(y)").unwrap();
        assert_eq!(t.terms, vec!["hash(x)"]);
        assert!(!t.is_user_provided);
    }

    #[test]
    fn trigger_infer_returns_none_for_unknown() {
        let tm = TriggerManager::new();
        assert!(tm.infer_trigger("x + y > 0").is_none());
    }

    #[test]
    fn trigger_add_and_get() {
        let mut tm = TriggerManager::new();
        tm.add_trigger(
            "q1".into(),
            TriggerPattern {
                terms: vec!["f(x)".into()],
                is_user_provided: true,
            },
        );
        let triggers = tm.get_triggers("q1").unwrap();
        assert_eq!(triggers.len(), 1);
        assert!(triggers[0].is_user_provided);
    }

    #[test]
    fn trigger_validate_warns_on_unknown_function() {
        let mut tm = TriggerManager::new();
        tm.register_function("known_only".into());
        let pat = TriggerPattern {
            terms: vec!["unknown_func(x)".into()],
            is_user_provided: true,
        };
        let warnings = tm.validate_trigger(&pat);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unknown_func(x)"));
    }

    #[test]
    fn trigger_validate_no_warning_for_known() {
        let mut tm = TriggerManager::new();
        tm.register_function("f".into());
        let pat = TriggerPattern {
            terms: vec!["f(x)".into()],
            is_user_provided: true,
        };
        assert!(tm.validate_trigger(&pat).is_empty());
    }

    #[test]
    fn trigger_infer_from_expr_call_with_bound_var() {
        use assura_ast::{Expr, Spanned};
        let mut tm = TriggerManager::new();
        tm.register_function("lookup".into());
        let body = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Call {
                func: Box::new(Spanned::no_span(Expr::Ident("lookup".into()))),
                args: vec![Spanned::no_span(Expr::Ident("i".into()))],
            })),
            op: assura_ast::BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(assura_ast::Literal::Int(
                "0".into(),
            )))),
        });
        let t = tm
            .infer_trigger_from_expr(&body, "i")
            .expect("should infer from Call");
        assert!(
            t.terms.iter().any(|term| term.contains("lookup")),
            "expected lookup trigger, got {:?}",
            t.terms
        );
    }

    // -----------------------------------------------------------------------
    // CodecDispatcher
    // -----------------------------------------------------------------------

    #[test]
    fn codec_new_is_empty() {
        let cd = CodecDispatcher::new();
        assert_eq!(cd.codec_count(), 0);
    }

    #[test]
    fn codec_default_is_empty() {
        let cd = CodecDispatcher::default();
        assert_eq!(cd.codec_count(), 0);
    }

    #[test]
    fn codec_register_increases_count() {
        let mut cd = CodecDispatcher::new();
        cd.register("png".into(), vec![0x89, 0x50, 0x4E, 0x47], 0);
        assert_eq!(cd.codec_count(), 1);
    }

    #[test]
    fn codec_dispatch_matches() {
        let mut cd = CodecDispatcher::new();
        cd.register("png".into(), vec![0x89, 0x50], 0);
        let data = vec![0x89, 0x50, 0x4E, 0x47, 0x00];
        assert_eq!(cd.dispatch(&data), DispatchResult::Matched("png".into()));
    }

    #[test]
    fn codec_dispatch_unknown() {
        let mut cd = CodecDispatcher::new();
        cd.register("png".into(), vec![0x89, 0x50], 0);
        let data = vec![0xFF, 0xD8, 0xFF]; // JPEG magic
        assert_eq!(cd.dispatch(&data), DispatchResult::Unknown);
    }

    #[test]
    fn codec_dispatch_ambiguous() {
        let mut cd = CodecDispatcher::new();
        cd.register("a".into(), vec![0xAA], 0);
        cd.register("b".into(), vec![0xAA], 0);
        let data = vec![0xAA, 0x00];
        assert_eq!(
            cd.dispatch(&data),
            DispatchResult::Ambiguous(vec!["a".into(), "b".into()])
        );
    }

    #[test]
    fn codec_dispatch_with_offset() {
        let mut cd = CodecDispatcher::new();
        cd.register("custom".into(), vec![0xBE, 0xEF], 2);
        let data = vec![0x00, 0x00, 0xBE, 0xEF, 0x00];
        assert_eq!(cd.dispatch(&data), DispatchResult::Matched("custom".into()));
    }

    #[test]
    fn codec_dispatch_data_too_short() {
        let mut cd = CodecDispatcher::new();
        cd.register("wide".into(), vec![0x01, 0x02, 0x03, 0x04], 0);
        let data = vec![0x01, 0x02]; // shorter than magic
        assert_eq!(cd.dispatch(&data), DispatchResult::Unknown);
    }

    #[test]
    fn codec_check_ambiguity_detects_conflict() {
        let mut cd = CodecDispatcher::new();
        cd.register("a".into(), vec![0xFF], 0);
        cd.register("b".into(), vec![0xFF], 0);
        let conflicts = cd.check_ambiguity();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0], ("a".into(), "b".into()));
    }

    #[test]
    fn codec_check_ambiguity_no_conflict() {
        let mut cd = CodecDispatcher::new();
        cd.register("a".into(), vec![0xAA], 0);
        cd.register("b".into(), vec![0xBB], 0);
        assert!(cd.check_ambiguity().is_empty());
    }

    // -----------------------------------------------------------------------
    // WeakMemoryChecker
    // -----------------------------------------------------------------------

    #[test]
    fn wmc_new_is_empty() {
        let wmc = WeakMemoryChecker::new();
        assert_eq!(wmc.access_count(), 0);
    }

    #[test]
    fn wmc_default_is_empty() {
        let wmc = WeakMemoryChecker::default();
        assert_eq!(wmc.access_count(), 0);
    }

    #[test]
    fn wmc_record_access_increments_count() {
        let mut wmc = WeakMemoryChecker::new();
        wmc.record_access(0, "x".into(), true, MemoryOrdering::SeqCst);
        assert_eq!(wmc.access_count(), 1);
    }

    #[test]
    fn wmc_record_access_returns_sequence_numbers() {
        let mut wmc = WeakMemoryChecker::new();
        let s0 = wmc.record_access(0, "x".into(), true, MemoryOrdering::Relaxed);
        let s1 = wmc.record_access(0, "y".into(), false, MemoryOrdering::Relaxed);
        assert_eq!(s0, 0);
        assert_eq!(s1, 1);
    }

    #[test]
    fn wmc_data_race_detected() {
        let mut wmc = WeakMemoryChecker::new();
        wmc.record_access(0, "x".into(), true, MemoryOrdering::Relaxed);
        wmc.record_access(1, "x".into(), false, MemoryOrdering::Relaxed);
        let races = wmc.check_data_races();
        assert_eq!(races.len(), 1);
        assert!(races[0].contains("data race on `x`"));
    }

    #[test]
    fn wmc_no_race_same_thread() {
        let mut wmc = WeakMemoryChecker::new();
        wmc.record_access(0, "x".into(), true, MemoryOrdering::Relaxed);
        wmc.record_access(0, "x".into(), false, MemoryOrdering::Relaxed);
        assert!(wmc.check_data_races().is_empty());
    }

    #[test]
    fn wmc_no_race_both_reads() {
        let mut wmc = WeakMemoryChecker::new();
        wmc.record_access(0, "x".into(), false, MemoryOrdering::Relaxed);
        wmc.record_access(1, "x".into(), false, MemoryOrdering::Relaxed);
        assert!(wmc.check_data_races().is_empty());
    }

    #[test]
    fn wmc_no_race_with_happens_before() {
        let mut wmc = WeakMemoryChecker::new();
        let s0 = wmc.record_access(0, "x".into(), true, MemoryOrdering::Release);
        let s1 = wmc.record_access(1, "x".into(), false, MemoryOrdering::Acquire);
        wmc.add_happens_before(s0, s1);
        assert!(wmc.check_data_races().is_empty());
    }

    #[test]
    fn wmc_release_without_acquire() {
        let mut wmc = WeakMemoryChecker::new();
        wmc.record_access(0, "flag".into(), true, MemoryOrdering::Release);
        let warnings = wmc.check_release_acquire();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("no matching acquire"));
    }

    #[test]
    fn wmc_release_with_acquire_ok() {
        let mut wmc = WeakMemoryChecker::new();
        wmc.record_access(0, "flag".into(), true, MemoryOrdering::Release);
        wmc.record_access(1, "flag".into(), false, MemoryOrdering::Acquire);
        assert!(wmc.check_release_acquire().is_empty());
    }

    #[test]
    fn wmc_relaxed_write_read_by_other_thread() {
        let mut wmc = WeakMemoryChecker::new();
        wmc.record_access(0, "data".into(), true, MemoryOrdering::Relaxed);
        wmc.record_access(1, "data".into(), false, MemoryOrdering::Relaxed);
        let warnings = wmc.check_ordering_strength();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("consider Release ordering"));
    }

    #[test]
    fn wmc_seqcst_no_ordering_warning() {
        let mut wmc = WeakMemoryChecker::new();
        wmc.record_access(0, "data".into(), true, MemoryOrdering::SeqCst);
        wmc.record_access(1, "data".into(), false, MemoryOrdering::SeqCst);
        assert!(wmc.check_ordering_strength().is_empty());
    }

    // -----------------------------------------------------------------------
    // ProphecyManager
    // -----------------------------------------------------------------------

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
        assert!(pm.resolve("p", "42".into()).is_ok());
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

    // -----------------------------------------------------------------------
    // LivenessChecker
    // -----------------------------------------------------------------------

    #[test]
    fn liveness_new_is_empty() {
        let lc = LivenessChecker::new();
        assert_eq!(lc.obligation_count(), 0);
    }

    #[test]
    fn liveness_default_is_empty() {
        let lc = LivenessChecker::default();
        assert_eq!(lc.obligation_count(), 0);
    }

    #[test]
    fn liveness_add_obligation_increases_count() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "progress".into(),
            LivenessKind::Eventually,
            "true".into(),
            "done".into(),
        );
        assert_eq!(lc.obligation_count(), 1);
    }

    #[test]
    fn liveness_unverified_reported() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "termination".into(),
            LivenessKind::Eventually,
            "started".into(),
            "finished".into(),
        );
        let unverified = lc.check_unverified();
        assert_eq!(unverified.len(), 1);
        assert!(unverified[0].contains("termination"));
    }

    #[test]
    fn liveness_mark_verified_clears() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "term".into(),
            LivenessKind::Eventually,
            "a".into(),
            "b".into(),
        );
        lc.mark_verified("term");
        assert!(lc.check_unverified().is_empty());
    }

    #[test]
    fn liveness_mark_verified_unknown_is_noop() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "real".into(),
            LivenessKind::Eventually,
            "a".into(),
            "b".into(),
        );
        lc.mark_verified("fake");
        assert_eq!(lc.check_unverified().len(), 1);
    }

    #[test]
    fn liveness_zero_bound_detected() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "instant".into(),
            LivenessKind::EventuallyWithin(0),
            "a".into(),
            "b".into(),
        );
        let warnings = lc.check_bounded();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("zero time bound"));
    }

    #[test]
    fn liveness_nonzero_bound_ok() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "bounded".into(),
            LivenessKind::EventuallyWithin(100),
            "a".into(),
            "b".into(),
        );
        assert!(lc.check_bounded().is_empty());
    }

    #[test]
    fn liveness_leads_to_without_fairness_warns() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "resp".into(),
            LivenessKind::LeadsTo,
            "request".into(),
            "response".into(),
        );
        let warnings = lc.check_fairness();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("fairness"));
    }

    #[test]
    fn liveness_leads_to_with_fairness_ok() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "resp".into(),
            LivenessKind::LeadsTo,
            "req".into(),
            "res".into(),
        );
        lc.add_fairness("scheduler is fair".into());
        assert!(lc.check_fairness().is_empty());
    }

    #[test]
    fn liveness_eventually_no_fairness_needed() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "term".into(),
            LivenessKind::Eventually,
            "a".into(),
            "b".into(),
        );
        assert!(lc.check_fairness().is_empty());
    }
}
