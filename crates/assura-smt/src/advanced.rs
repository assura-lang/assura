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
}

impl TriggerManager {
    pub fn new() -> Self {
        Self {
            known_functions: Vec::new(),
            triggers: std::collections::HashMap::new(),
        }
    }

    pub fn register_function(&mut self, name: String) {
        if !self.known_functions.contains(&name) {
            self.known_functions.push(name);
        }
    }

    pub fn add_trigger(&mut self, formula_name: String, pattern: TriggerPattern) {
        self.triggers.entry(formula_name).or_default().push(pattern);
    }

    /// Infer a trigger pattern from the quantifier body.
    /// Returns None if no suitable trigger can be inferred.
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

    /// Validate that a trigger pattern mentions only known functions.
    pub fn validate_trigger(&self, pattern: &TriggerPattern) -> Vec<String> {
        let mut warnings = Vec::new();
        for term in &pattern.terms {
            let has_known = self
                .known_functions
                .iter()
                .any(|f| term.contains(f.as_str()));
            if !has_known {
                warnings.push(format!(
                    "trigger term `{term}` does not reference any known function"
                ));
            }
        }
        warnings
    }

    pub fn get_triggers(&self, formula_name: &str) -> Option<&Vec<TriggerPattern>> {
        self.triggers.get(formula_name)
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
