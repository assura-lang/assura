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

    /// Reduce all liveness obligations to safety properties via monitor automata.
    ///
    /// For `eventually P`: introduces a 2-state monitor (waiting/satisfied).
    /// The safety property is that the monitor never stays in `waiting` forever
    /// (detected via lasso/loop detection in BMC).
    ///
    /// For `leads_to(A, B)`: introduces a 3-state monitor (idle/triggered/fulfilled).
    /// The safety property is that the monitor never stays in `triggered` forever.
    ///
    /// Returns `MonitorReduction` structs ready to be dispatched to BMC/k-induction.
    pub fn reduce_to_safety(&self) -> Vec<MonitorReduction> {
        self.obligations
            .iter()
            .map(|o| match &o.kind {
                LivenessKind::Eventually => {
                    MonitorReduction::eventually(o.name.clone(), o.conclusion.clone())
                }
                LivenessKind::LeadsTo => MonitorReduction::leads_to(
                    o.name.clone(),
                    o.premise.clone(),
                    o.conclusion.clone(),
                ),
                LivenessKind::EventuallyWithin(bound) => MonitorReduction::eventually_within(
                    o.name.clone(),
                    o.conclusion.clone(),
                    *bound,
                ),
            })
            .collect()
    }
}

impl Default for LivenessChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Monitor automaton reduction (liveness -> safety)
// ---------------------------------------------------------------------------

/// Monitor state for `eventually P` (2-state automaton).
///
/// Documents the integer encoding used in `MonitorReduction::eventually()`:
/// `Waiting = 0`, `Satisfied = 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(dead_code)]
pub enum EventuallyState {
    /// Waiting for P to become true
    Waiting,
    /// P has been observed
    Satisfied,
}

/// Monitor state for `leads_to(A, B)` (3-state automaton).
///
/// Documents the integer encoding used in `MonitorReduction::leads_to()`:
/// `Idle = 0`, `Triggered = 1`, `Fulfilled = 2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(dead_code)]
pub enum LeadsToState {
    /// Neither A nor B has been seen
    Idle,
    /// A has been seen, waiting for B
    Triggered,
    /// B has been seen after A
    Fulfilled,
}

/// A liveness property reduced to a safety property via a monitor automaton.
///
/// The `safety_bad_predicate` is the negation of the safety property:
/// if this predicate holds in a loop (lasso), the liveness property is violated.
#[derive(Debug, Clone)]
pub struct MonitorReduction {
    /// Name of the original liveness obligation
    pub name: String,
    /// The monitor state variable name (e.g., "__monitor_eventually_progress")
    pub monitor_var: String,
    /// Initial value of the monitor variable (integer encoding of the state)
    pub initial_state: i64,
    /// Transition predicate for the monitor (references the goal/premise predicates)
    pub transition_predicate: String,
    /// The bad-state predicate for BMC safety checking
    pub safety_bad_predicate: String,
    /// The original goal predicate (for diagnostics)
    pub goal_predicate: String,
    /// Additional state variables introduced by the monitor
    pub extra_state_vars: Vec<(String, String)>,
    /// Additional initial constraints for the monitor
    pub initial_constraints: Vec<String>,
    /// Additional transition constraints for the monitor
    pub transition_constraints: Vec<String>,
}

impl MonitorReduction {
    /// Create a monitor reduction for `eventually P`.
    ///
    /// Monitor automaton:
    /// - State 0 (Waiting): P has not yet been observed
    /// - State 1 (Satisfied): P has been observed
    /// - Transition: if state == 0 && P holds, next_state = 1; else next_state = state
    /// - Bad state for lasso: state == 0 (stuck waiting forever)
    pub fn eventually(name: String, goal: String) -> Self {
        let monitor_var = format!("__monitor_eventually_{}", sanitize_name(&name));
        let transition_predicate = format!("if ({goal}) then 1 else {monitor_var}");
        let transition_constraints = vec![format!(
            "{monitor_var}' == (if ({goal}') then 1 else {monitor_var})"
        )];
        Self {
            transition_predicate,
            safety_bad_predicate: format!("{monitor_var} == 0"),
            goal_predicate: goal,
            extra_state_vars: vec![(monitor_var.clone(), "Int".into())],
            initial_constraints: vec![format!("{monitor_var} == 0")],
            transition_constraints,
            initial_state: 0,
            monitor_var,
            name,
        }
    }

    /// Create a monitor reduction for `leads_to(A, B)`.
    ///
    /// Monitor automaton:
    /// - State 0 (Idle): neither A nor B relevant yet
    /// - State 1 (Triggered): A has been seen, waiting for B
    /// - State 2 (Fulfilled): B has been seen after A
    /// - Transitions:
    ///   - Idle + A holds -> Triggered
    ///   - Triggered + B holds -> Fulfilled
    ///   - Fulfilled stays Fulfilled
    /// - Bad state for lasso: state == 1 (stuck triggered forever, B never comes)
    pub fn leads_to(name: String, premise: String, conclusion: String) -> Self {
        let monitor_var = format!("__monitor_leads_to_{}", sanitize_name(&name));
        let transition_predicate = format!(
            "if ({monitor_var} == 1 && ({conclusion})) then 2 \
             else if ({monitor_var} == 0 && ({premise})) then 1 \
             else {monitor_var}"
        );
        let transition_constraints = vec![format!(
            "{monitor_var}' == (if ({monitor_var} == 1 && ({conclusion}')) then 2 \
             else if ({monitor_var} == 0 && ({premise}')) then 1 \
             else {monitor_var})"
        )];
        Self {
            transition_predicate,
            safety_bad_predicate: format!("{monitor_var} == 1"),
            goal_predicate: conclusion,
            extra_state_vars: vec![(monitor_var.clone(), "Int".into())],
            initial_constraints: vec![format!("{monitor_var} == 0")],
            transition_constraints,
            initial_state: 0,
            monitor_var,
            name,
        }
    }

    /// Create a monitor reduction for `eventually_within(P, bound)`.
    ///
    /// Like `eventually P` but with a step counter. The bad state is
    /// `counter >= bound && state == 0` (P not observed within bound steps).
    /// This is actually a bounded safety property (no lasso needed).
    pub fn eventually_within(name: String, goal: String, bound: u64) -> Self {
        let monitor_var = format!("__monitor_ev_within_{}", sanitize_name(&name));
        let counter_var = format!("__counter_ev_within_{}", sanitize_name(&name));
        let transition_predicate = format!("if ({goal}) then 1 else {monitor_var}");
        let safety_bad = format!("{counter_var} >= {bound} && {monitor_var} == 0");
        let transition_constraints = vec![
            format!("{monitor_var}' == (if ({goal}') then 1 else {monitor_var})"),
            format!("{counter_var}' == {counter_var} + 1"),
        ];
        Self {
            transition_predicate,
            safety_bad_predicate: safety_bad,
            goal_predicate: goal,
            extra_state_vars: vec![
                (monitor_var.clone(), "Int".into()),
                (counter_var.clone(), "Int".into()),
            ],
            initial_constraints: vec![format!("{monitor_var} == 0"), format!("{counter_var} == 0")],
            transition_constraints,
            initial_state: 0,
            monitor_var,
            name,
        }
    }

    /// Convert this monitor reduction into BMC engine components.
    ///
    /// Returns (state_variables, initial_constraints, transitions, property).
    pub fn to_bmc_components(&self) -> BmcComponents {
        use crate::bmc::{BmcProperty, BmcSort, StateVariable, TransitionConstraint};

        let state_vars: Vec<StateVariable> = self
            .extra_state_vars
            .iter()
            .map(|(name, sort)| StateVariable {
                name: name.clone(),
                sort: match sort.as_str() {
                    "Bool" => BmcSort::Bool,
                    "Real" => BmcSort::Real,
                    _ => BmcSort::Int,
                },
            })
            .collect();

        let transitions: Vec<TransitionConstraint> = self
            .transition_constraints
            .iter()
            .map(|pred| TransitionConstraint {
                predicate: pred.clone(),
                variables: self
                    .extra_state_vars
                    .iter()
                    .map(|(n, _)| n.clone())
                    .collect(),
            })
            .collect();

        // Use Liveness property for eventually/leads_to (needs lasso detection)
        // Use Safety property for eventually_within (bounded, no lasso needed)
        let property = if self.safety_bad_predicate.contains("__counter_") {
            BmcProperty::Safety {
                name: self.name.clone(),
                bad_predicate: self.safety_bad_predicate.clone(),
            }
        } else {
            BmcProperty::Liveness {
                name: self.name.clone(),
                goal_predicate: self.goal_predicate.clone(),
            }
        };

        BmcComponents {
            state_vars,
            initial_constraints: self.initial_constraints.clone(),
            transitions,
            property,
        }
    }
}

/// BMC engine components extracted from a monitor reduction.
pub struct BmcComponents {
    pub state_vars: Vec<crate::bmc::StateVariable>,
    pub initial_constraints: Vec<String>,
    pub transitions: Vec<crate::bmc::TransitionConstraint>,
    pub property: crate::bmc::BmcProperty,
}

/// Sanitize a name for use as a Z3 variable name.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // -------------------------------------------------------------------
    // Monitor automaton reduction tests
    // -------------------------------------------------------------------

    #[test]
    fn monitor_eventually_has_correct_states() {
        let m = MonitorReduction::eventually("progress".into(), "done == true".into());
        assert_eq!(m.initial_state, 0);
        assert!(m.safety_bad_predicate.contains("== 0"));
        assert!(m.monitor_var.contains("__monitor_eventually_"));
        assert_eq!(m.extra_state_vars.len(), 1);
        assert_eq!(m.initial_constraints.len(), 1);
        assert_eq!(m.transition_constraints.len(), 1);
    }

    #[test]
    fn monitor_leads_to_has_correct_states() {
        let m = MonitorReduction::leads_to(
            "response".into(),
            "request == true".into(),
            "response == true".into(),
        );
        assert_eq!(m.initial_state, 0);
        assert!(m.safety_bad_predicate.contains("== 1"));
        assert!(m.monitor_var.contains("__monitor_leads_to_"));
        assert_eq!(m.extra_state_vars.len(), 1);
        assert_eq!(m.initial_constraints.len(), 1);
        assert_eq!(m.transition_constraints.len(), 1);
    }

    #[test]
    fn monitor_eventually_within_has_counter() {
        let m = MonitorReduction::eventually_within("timeout".into(), "done == true".into(), 10);
        assert_eq!(m.extra_state_vars.len(), 2);
        assert!(
            m.extra_state_vars
                .iter()
                .any(|(n, _)| n.contains("counter"))
        );
        assert!(m.safety_bad_predicate.contains(">= 10"));
        assert_eq!(m.initial_constraints.len(), 2);
        assert_eq!(m.transition_constraints.len(), 2);
    }

    #[test]
    fn monitor_to_bmc_components_eventually() {
        let m = MonitorReduction::eventually("prog".into(), "x > 0".into());
        let components = m.to_bmc_components();
        assert_eq!(components.state_vars.len(), 1);
        assert_eq!(components.initial_constraints.len(), 1);
        assert_eq!(components.transitions.len(), 1);
        assert!(matches!(
            components.property,
            crate::bmc::BmcProperty::Liveness { .. }
        ));
    }

    #[test]
    fn monitor_to_bmc_components_leads_to() {
        let m = MonitorReduction::leads_to("resp".into(), "req".into(), "res".into());
        let components = m.to_bmc_components();
        assert_eq!(components.state_vars.len(), 1);
        assert!(matches!(
            components.property,
            crate::bmc::BmcProperty::Liveness { .. }
        ));
    }

    #[test]
    fn monitor_to_bmc_components_eventually_within_is_safety() {
        let m = MonitorReduction::eventually_within("bounded".into(), "done".into(), 5);
        let components = m.to_bmc_components();
        assert_eq!(components.state_vars.len(), 2);
        assert!(matches!(
            components.property,
            crate::bmc::BmcProperty::Safety { .. }
        ));
    }

    #[test]
    fn reduce_to_safety_empty() {
        let lc = LivenessChecker::new();
        assert!(lc.reduce_to_safety().is_empty());
    }

    #[test]
    fn reduce_to_safety_eventually() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "progress".into(),
            LivenessKind::Eventually,
            "true".into(),
            "done".into(),
        );
        let reductions = lc.reduce_to_safety();
        assert_eq!(reductions.len(), 1);
        assert!(reductions[0].monitor_var.contains("eventually"));
    }

    #[test]
    fn reduce_to_safety_leads_to() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "response".into(),
            LivenessKind::LeadsTo,
            "request".into(),
            "response".into(),
        );
        let reductions = lc.reduce_to_safety();
        assert_eq!(reductions.len(), 1);
        assert!(reductions[0].monitor_var.contains("leads_to"));
    }

    #[test]
    fn reduce_to_safety_mixed() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "ev".into(),
            LivenessKind::Eventually,
            "true".into(),
            "done".into(),
        );
        lc.add_obligation("lt".into(), LivenessKind::LeadsTo, "a".into(), "b".into());
        lc.add_obligation(
            "ew".into(),
            LivenessKind::EventuallyWithin(50),
            "true".into(),
            "c".into(),
        );
        let reductions = lc.reduce_to_safety();
        assert_eq!(reductions.len(), 3);
    }

    #[test]
    fn sanitize_name_handles_special_chars() {
        assert_eq!(sanitize_name("hello_world"), "hello_world");
        assert_eq!(sanitize_name("a::b"), "a__b");
        assert_eq!(sanitize_name("x.y-z"), "x_y_z");
    }
}
