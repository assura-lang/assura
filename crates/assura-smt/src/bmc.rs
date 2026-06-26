//! Bounded Model Checking (BMC) engine for Layer 3 verification.
//!
//! Encodes contract state transitions as Z3 constraints with K unrolling
//! steps (`state_0, state_1, ..., state_K`). Includes a lasso detector
//! that checks if `state_K` matches any earlier `state_i` while the bad
//! property holds in the loop.
//!
//! # Architecture
//!
//! The BMC engine operates in three phases:
//!
//! 1. **Unrolling**: Create K copies of each state variable (`x_0`, `x_1`, ..., `x_K`)
//!    and assert the transition relation between consecutive steps.
//! 2. **Safety checking**: Assert the negation of the safety property at each step
//!    and check for counterexamples (a reachable bad state).
//! 3. **Lasso detection**: Check if `state_K == state_i` for any `0 <= i < K` and
//!    whether the bad property (negated liveness) holds in the detected loop.

/// Configuration for the BMC engine.
#[derive(Debug, Clone)]
pub struct BmcConfig {
    /// Maximum unrolling bound (default: 10)
    pub bound: usize,
    /// Timeout in milliseconds per query (default: 30_000)
    pub timeout_ms: u64,
    /// Whether to enable lasso detection for liveness properties
    pub enable_lasso: bool,
}

impl Default for BmcConfig {
    fn default() -> Self {
        Self {
            bound: 10,
            timeout_ms: 30_000,
            enable_lasso: true,
        }
    }
}

impl BmcConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_bound(mut self, k: usize) -> Self {
        self.bound = k;
        self
    }

    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }
}

/// A state variable in the BMC model.
#[derive(Debug, Clone)]
pub struct StateVariable {
    /// Base name of the variable (e.g., "x")
    pub name: String,
    /// Sort: "Int", "Bool", "Real"
    pub sort: BmcSort,
}

/// Supported sorts for BMC state variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BmcSort {
    Int,
    Bool,
    Real,
}

/// A transition relation constraint.
///
/// Represents a relationship between state variables at step `i` and step `i+1`.
/// For example, `x_{i+1} = x_i + 1` would be:
/// ```text
/// TransitionConstraint {
///     predicate: "x' == x + 1",
///     ..
/// }
/// ```
#[derive(Debug, Clone)]
pub struct TransitionConstraint {
    /// Human-readable predicate text (for diagnostics)
    pub predicate: String,
    /// Variables referenced (used for renaming per step)
    pub variables: Vec<String>,
}

/// A property to check (safety or liveness).
#[derive(Debug, Clone)]
pub enum BmcProperty {
    /// Safety: "bad state is never reached" (checked at each step)
    Safety {
        name: String,
        /// The bad-state predicate (negation of the invariant)
        bad_predicate: String,
    },
    /// Liveness: "eventually this property holds" (checked via lasso detection)
    Liveness {
        name: String,
        /// The goal predicate that must eventually be true
        goal_predicate: String,
    },
}

/// Result of a BMC analysis.
#[derive(Debug, Clone)]
pub enum BmcResult {
    /// No counterexample found up to the bound (NOT a proof of correctness).
    Safe { property: String, bound: usize },
    /// A counterexample (bad state) was found.
    Counterexample {
        property: String,
        /// The step at which the bad state was found
        step: usize,
        /// Variable assignments at each step of the trace
        trace: Vec<BmcTraceStep>,
    },
    /// A lasso was found: the system can loop while violating liveness.
    Lasso {
        property: String,
        /// The stem length (steps before the loop starts)
        stem_length: usize,
        /// The loop length (steps in the repeating cycle)
        loop_length: usize,
        /// Variable assignments in the lasso trace
        trace: Vec<BmcTraceStep>,
    },
    /// The solver timed out or returned unknown.
    Unknown { property: String, reason: String },
}

/// One step in a BMC trace (counterexample or lasso).
#[derive(Debug, Clone)]
pub struct BmcTraceStep {
    /// Step index (0, 1, ..., K)
    pub step: usize,
    /// Variable assignments at this step: (name, value)
    pub assignments: Vec<(String, String)>,
}

/// The BMC engine.
#[derive(Debug, Clone)]
pub struct BmcEngine {
    pub config: BmcConfig,
    pub state_variables: Vec<StateVariable>,
    pub transitions: Vec<TransitionConstraint>,
    pub initial_constraints: Vec<String>,
    pub properties: Vec<BmcProperty>,
}

impl BmcEngine {
    pub fn new(config: BmcConfig) -> Self {
        Self {
            config,
            state_variables: Vec::new(),
            transitions: Vec::new(),
            initial_constraints: Vec::new(),
            properties: Vec::new(),
        }
    }

    /// Add a state variable to the model.
    pub fn add_state_variable(&mut self, name: impl Into<String>, sort: BmcSort) {
        self.state_variables.push(StateVariable {
            name: name.into(),
            sort,
        });
    }

    /// Add a transition constraint (relationship between step i and step i+1).
    pub fn add_transition(&mut self, predicate: impl Into<String>, variables: Vec<String>) {
        self.transitions.push(TransitionConstraint {
            predicate: predicate.into(),
            variables,
        });
    }

    /// Add an initial state constraint (holds at step 0).
    pub fn add_initial_constraint(&mut self, constraint: impl Into<String>) {
        self.initial_constraints.push(constraint.into());
    }

    /// Add a property to check.
    pub fn add_property(&mut self, property: BmcProperty) {
        self.properties.push(property);
    }

    /// Generate the renamed variable name for a given step.
    ///
    /// `x` at step 3 becomes `x_3`.
    pub fn rename_var(name: &str, step: usize) -> String {
        format!("{name}_{step}")
    }

    /// Run the BMC analysis for all properties.
    pub fn check(&self) -> Vec<BmcResult> {
        #[cfg(feature = "z3-verify")]
        {
            self.check_with_z3()
        }
        #[cfg(not(feature = "z3-verify"))]
        {
            self.properties
                .iter()
                .map(|p| {
                    let name = match p {
                        BmcProperty::Safety { name, .. } => name.clone(),
                        BmcProperty::Liveness { name, .. } => name.clone(),
                    };
                    BmcResult::Unknown {
                        property: name,
                        reason: "Z3 not available (compiled without z3-verify feature)".into(),
                    }
                })
                .collect()
        }
    }

    #[cfg(feature = "z3-verify")]
    fn check_with_z3(&self) -> Vec<BmcResult> {
        use z3::Config;

        let mut cfg = Config::new();
        cfg.set_timeout_msec(self.config.timeout_ms);

        z3::with_z3_config(&cfg, || {
            let mut results = Vec::new();

            for property in &self.properties {
                match property {
                    BmcProperty::Safety {
                        name,
                        bad_predicate,
                    } => {
                        let result = self.check_safety_z3(name, bad_predicate);
                        results.push(result);
                    }
                    BmcProperty::Liveness {
                        name,
                        goal_predicate,
                    } => {
                        if self.config.enable_lasso {
                            let result = self.check_liveness_z3(name, goal_predicate);
                            results.push(result);
                        } else {
                            results.push(BmcResult::Unknown {
                                property: name.clone(),
                                reason: "lasso detection disabled".into(),
                            });
                        }
                    }
                }
            }

            results
        })
    }

    /// Check a safety property: is a bad state reachable within K steps?
    #[cfg(feature = "z3-verify")]
    fn check_safety_z3(&self, name: &str, bad_predicate: &str) -> BmcResult {
        use z3::{SatResult, Solver};

        let solver = Solver::new();

        // Create all stepped variables: x_0, x_1, ..., x_K
        let stepped_vars = self.create_stepped_vars();

        // Assert initial constraints at step 0
        for ic in &self.initial_constraints {
            if let Some(z3_ic) = self.parse_predicate_at_step(ic, 0, &stepped_vars) {
                solver.assert(&z3_ic);
            }
        }

        // Assert transition relation between consecutive steps
        for step in 0..self.config.bound {
            for trans in &self.transitions {
                if let Some(z3_trans) =
                    self.parse_transition_at_step(&trans.predicate, step, &stepped_vars)
                {
                    solver.assert(&z3_trans);
                }
            }
        }

        // Check: is the bad predicate reachable at any step?
        // We use incremental solving: push, assert bad at step k, check, pop
        for step in 0..=self.config.bound {
            solver.push();

            if let Some(z3_bad) = self.parse_predicate_at_step(bad_predicate, step, &stepped_vars) {
                solver.assert(&z3_bad);

                match solver.check() {
                    SatResult::Sat => {
                        // Found a counterexample at this step
                        let trace = self.extract_trace(&solver, step, &stepped_vars);
                        return BmcResult::Counterexample {
                            property: name.to_string(),
                            step,
                            trace,
                        };
                    }
                    SatResult::Unknown => {
                        let reason = solver
                            .get_reason_unknown()
                            .unwrap_or_else(|| "unknown".into());
                        if reason.contains("timeout") {
                            return BmcResult::Unknown {
                                property: name.to_string(),
                                reason: format!("timeout at step {step}"),
                            };
                        }
                    }
                    SatResult::Unsat => {
                        // Not reachable at this step, continue
                    }
                }
            }

            solver.pop(1);
        }

        BmcResult::Safe {
            property: name.to_string(),
            bound: self.config.bound,
        }
    }

    /// Check a liveness property via lasso detection.
    ///
    /// A lasso is a path: `s_0 -> s_1 -> ... -> s_i -> ... -> s_K` where
    /// `s_K == s_i` (forming a cycle from step i to K), and the goal
    /// predicate is false at every step in the cycle.
    #[cfg(feature = "z3-verify")]
    fn check_liveness_z3(&self, name: &str, goal_predicate: &str) -> BmcResult {
        use z3::{SatResult, Solver};

        let solver = Solver::new();

        let stepped_vars = self.create_stepped_vars();

        // Assert initial constraints at step 0
        for ic in &self.initial_constraints {
            if let Some(z3_ic) = self.parse_predicate_at_step(ic, 0, &stepped_vars) {
                solver.assert(&z3_ic);
            }
        }

        // Assert transition relation for all steps
        for step in 0..self.config.bound {
            for trans in &self.transitions {
                if let Some(z3_trans) =
                    self.parse_transition_at_step(&trans.predicate, step, &stepped_vars)
                {
                    solver.assert(&z3_trans);
                }
            }
        }

        // Assert that the goal is NOT satisfied at any step (negated liveness)
        for step in 0..=self.config.bound {
            if let Some(z3_goal) = self.parse_predicate_at_step(goal_predicate, step, &stepped_vars)
            {
                solver.assert(z3_goal.not());
            }
        }

        // Try lasso detection: state_K == state_i for each i in 0..K
        let k = self.config.bound;
        for i in 0..k {
            solver.push();

            // Assert state_K == state_i (loop back)
            let mut loop_eqs = Vec::new();
            for var in &self.state_variables {
                let var_k = Self::rename_var(&var.name, k);
                let var_i = Self::rename_var(&var.name, i);
                if let (Some(zk), Some(zi)) = (
                    self.lookup_int_var(&var_k, &stepped_vars),
                    self.lookup_int_var(&var_i, &stepped_vars),
                ) {
                    loop_eqs.push(zk.eq(zi));
                }
            }

            if !loop_eqs.is_empty() {
                let refs: Vec<&z3::ast::Bool> = loop_eqs.iter().collect();
                solver.assert(z3::ast::Bool::and(&refs));

                match solver.check() {
                    SatResult::Sat => {
                        let trace = self.extract_trace(&solver, k, &stepped_vars);
                        return BmcResult::Lasso {
                            property: name.to_string(),
                            stem_length: i,
                            loop_length: k - i,
                            trace,
                        };
                    }
                    SatResult::Unknown => {
                        let reason = solver
                            .get_reason_unknown()
                            .unwrap_or_else(|| "unknown".into());
                        if reason.contains("timeout") {
                            return BmcResult::Unknown {
                                property: name.to_string(),
                                reason: format!("timeout during lasso check at loop point {i}"),
                            };
                        }
                    }
                    SatResult::Unsat => {
                        // No lasso with loop-back to step i
                    }
                }
            }

            solver.pop(1);
        }

        BmcResult::Safe {
            property: name.to_string(),
            bound: self.config.bound,
        }
    }

    /// Create Z3 Int variables for each state variable at each step.
    #[cfg(feature = "z3-verify")]
    fn create_stepped_vars(&self) -> Vec<(String, z3::ast::Int)> {
        let mut vars = Vec::new();
        for step in 0..=self.config.bound {
            for sv in &self.state_variables {
                let stepped_name = Self::rename_var(&sv.name, step);
                let z3_var = z3::ast::Int::new_const(stepped_name.as_str());
                vars.push((stepped_name, z3_var));
            }
        }
        vars
    }

    /// Look up a Z3 Int variable by its stepped name.
    #[cfg(feature = "z3-verify")]
    fn lookup_int_var<'a>(
        &self,
        name: &str,
        vars: &'a [(String, z3::ast::Int)],
    ) -> Option<&'a z3::ast::Int> {
        vars.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    /// Parse a predicate string at a specific step (renaming variables).
    ///
    /// Replaces each state variable `x` with `x_{step}` in the predicate,
    /// then parses it into a Z3 Bool.
    #[cfg(feature = "z3-verify")]
    fn parse_predicate_at_step(
        &self,
        predicate: &str,
        step: usize,
        vars: &[(String, z3::ast::Int)],
    ) -> Option<z3::ast::Bool> {
        let renamed = self.rename_predicate(predicate, step);
        parse_bmc_predicate(&renamed, vars)
    }

    /// Parse a transition predicate (uses `x` for step `i` and `x'` for step `i+1`).
    #[cfg(feature = "z3-verify")]
    fn parse_transition_at_step(
        &self,
        predicate: &str,
        step: usize,
        vars: &[(String, z3::ast::Int)],
    ) -> Option<z3::ast::Bool> {
        let renamed = self.rename_transition_predicate(predicate, step);
        parse_bmc_predicate(&renamed, vars)
    }

    /// Rename state variables in a predicate for a specific step.
    pub fn rename_predicate(&self, predicate: &str, step: usize) -> String {
        let mut result = predicate.to_string();
        // Sort by length descending to avoid partial replacements
        let mut sorted_vars: Vec<&str> = self
            .state_variables
            .iter()
            .map(|v| v.name.as_str())
            .collect();
        sorted_vars.sort_by_key(|b| std::cmp::Reverse(b.len()));
        for var_name in sorted_vars {
            let stepped = Self::rename_var(var_name, step);
            result = replace_word(&result, var_name, &stepped);
        }
        result
    }

    /// Rename transition predicate: `x` -> `x_{step}`, `x'` -> `x_{step+1}`.
    fn rename_transition_predicate(&self, predicate: &str, step: usize) -> String {
        let mut result = predicate.to_string();
        // Sort by length descending to avoid partial replacements
        let mut sorted_vars: Vec<&str> = self
            .state_variables
            .iter()
            .map(|v| v.name.as_str())
            .collect();
        sorted_vars.sort_by_key(|b| std::cmp::Reverse(b.len()));
        // Replace primed variables first (x' -> x_{step+1})
        for var_name in &sorted_vars {
            let primed = format!("{var_name}'");
            let stepped_next = Self::rename_var(var_name, step + 1);
            result = replace_word(&result, &primed, &stepped_next);
        }
        // Then unprimed (x -> x_{step})
        for var_name in &sorted_vars {
            let stepped = Self::rename_var(var_name, step);
            result = replace_word(&result, var_name, &stepped);
        }
        result
    }

    /// Extract a trace from a SAT model.
    #[cfg(feature = "z3-verify")]
    fn extract_trace(
        &self,
        solver: &z3::Solver,
        max_step: usize,
        vars: &[(String, z3::ast::Int)],
    ) -> Vec<BmcTraceStep> {
        let model = solver.get_model();
        let mut trace = Vec::new();
        for step in 0..=max_step {
            let mut assignments = Vec::new();
            for sv in &self.state_variables {
                let stepped_name = Self::rename_var(&sv.name, step);
                if let Some(z3_var) = self.lookup_int_var(&stepped_name, vars) {
                    let value = model
                        .as_ref()
                        .and_then(|m| m.eval(z3_var, true))
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "?".into());
                    assignments.push((sv.name.clone(), value));
                }
            }
            trace.push(BmcTraceStep { step, assignments });
        }
        trace
    }
}

/// Replace whole-word occurrences of `from` with `to` in `text`.
///
/// A "word boundary" is a transition between alphanumeric/underscore and
/// non-alphanumeric/non-underscore characters (or start/end of string).
fn replace_word(text: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return text.to_string();
    }
    let mut result = String::with_capacity(text.len());
    let mut i = 0;
    let chars: Vec<char> = text.chars().collect();
    let from_chars: Vec<char> = from.chars().collect();
    let from_len = from_chars.len();

    while i < chars.len() {
        if i + from_len <= chars.len() && chars[i..i + from_len] == from_chars[..] {
            let before_ok = i == 0 || !is_word_char(chars[i - 1]);
            let after_ok = i + from_len >= chars.len() || !is_word_char(chars[i + from_len]);
            if before_ok && after_ok {
                result.push_str(to);
                i += from_len;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Parse a simple BMC predicate into a Z3 Bool.
///
/// Like the Layer 2 `parse_predicate_to_z3` but works with stepped variable
/// names (`x_0`, `x_1`, etc.).
#[cfg(feature = "z3-verify")]
fn parse_bmc_predicate(body: &str, vars: &[(String, z3::ast::Int)]) -> Option<z3::ast::Bool> {
    let body = body.trim();

    if body == "true" {
        return Some(z3::ast::Bool::from_bool(true));
    }
    if body == "false" {
        return Some(z3::ast::Bool::from_bool(false));
    }

    // Handle conjunction
    if body.contains("&&") {
        let parts: Vec<&str> = body.split("&&").collect();
        let mut conjuncts = Vec::new();
        for part in parts {
            conjuncts.push(parse_bmc_predicate(part.trim(), vars)?);
        }
        let refs: Vec<&z3::ast::Bool> = conjuncts.iter().collect();
        return Some(z3::ast::Bool::and(&refs));
    }

    // Handle disjunction
    if body.contains("||") {
        let parts: Vec<&str> = body.split("||").collect();
        let mut disjuncts = Vec::new();
        for part in parts {
            disjuncts.push(parse_bmc_predicate(part.trim(), vars)?);
        }
        let refs: Vec<&z3::ast::Bool> = disjuncts.iter().collect();
        return Some(z3::ast::Bool::or(&refs));
    }

    // Comparisons
    let comparisons = [
        (">=", "ge"),
        ("<=", "le"),
        ("!=", "ne"),
        ("==", "eq"),
        (">", "gt"),
        ("<", "lt"),
    ];
    for (op_str, op_kind) in comparisons {
        if let Some(pos) = body.find(op_str) {
            let lhs_str = body[..pos].trim();
            let rhs_str = body[pos + op_str.len()..].trim();
            let lhs = parse_bmc_int_expr(lhs_str, vars)?;
            let rhs = parse_bmc_int_expr(rhs_str, vars)?;
            return match op_kind {
                "ge" => Some(lhs.ge(&rhs)),
                "le" => Some(lhs.le(&rhs)),
                "ne" => Some(lhs.eq(&rhs).not()),
                "eq" => Some(lhs.eq(&rhs)),
                "gt" => Some(lhs.gt(&rhs)),
                "lt" => Some(lhs.lt(&rhs)),
                _ => None,
            };
        }
    }

    None
}

/// Parse a simple integer expression for BMC predicates.
#[cfg(feature = "z3-verify")]
fn parse_bmc_int_expr(expr: &str, vars: &[(String, z3::ast::Int)]) -> Option<z3::ast::Int> {
    let expr = expr.trim();

    // Addition
    if let Some(pos) = expr.rfind('+')
        && pos > 0
    {
        let lhs = parse_bmc_int_expr(&expr[..pos], vars)?;
        let rhs = parse_bmc_int_expr(&expr[pos + 1..], vars)?;
        return Some(z3::ast::Int::add(&[&lhs, &rhs]));
    }

    // Subtraction (not negative number)
    if let Some(pos) = expr.rfind('-')
        && pos > 0
    {
        let lhs = parse_bmc_int_expr(&expr[..pos], vars)?;
        let rhs = parse_bmc_int_expr(&expr[pos + 1..], vars)?;
        return Some(z3::ast::Int::sub(&[&lhs, &rhs]));
    }

    // Multiplication
    if let Some(pos) = expr.rfind('*') {
        let lhs = parse_bmc_int_expr(&expr[..pos], vars)?;
        let rhs = parse_bmc_int_expr(&expr[pos + 1..], vars)?;
        return Some(z3::ast::Int::mul(&[&lhs, &rhs]));
    }

    // Modulo
    if let Some(pos) = expr.rfind('%') {
        let lhs = parse_bmc_int_expr(&expr[..pos], vars)?;
        let rhs = parse_bmc_int_expr(&expr[pos + 1..], vars)?;
        return Some(lhs.modulo(&rhs));
    }

    // Integer literal
    if let Ok(n) = expr.parse::<i64>() {
        return Some(z3::ast::Int::from_i64(n));
    }

    // Variable lookup
    for (name, z3_var) in vars {
        if name == expr {
            return Some(z3_var.clone());
        }
    }

    None
}
#[cfg(test)]
#[path = "bmc_tests.rs"]
mod tests;
