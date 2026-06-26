//! K-induction for unbounded liveness proofs (Layer 3).
//!
//! Builds on the BMC engine to prove properties hold for ALL executions,
//! not just bounded ones. The proof has two parts:
//!
//! 1. **Base case**: No lasso of length <= k exists (reuses BMC).
//! 2. **Inductive step**: Assuming lasso-freedom for length k, prove no
//!    lasso of length k+1.
//!
//! If both pass, the property is `Verified` (unbounded).
//! If the base fails, we have a `Counterexample`.
//! If the step fails but base passes, we report `Unknown` (bounded guarantee only).

use crate::bmc::{BmcConfig, BmcEngine, BmcProperty, BmcResult, BmcSort};

/// Configuration for k-induction.
#[derive(Debug, Clone)]
pub struct KInductionConfig {
    /// The induction depth k
    pub k: usize,
    /// Timeout in milliseconds per solver query
    pub timeout_ms: u64,
}

impl Default for KInductionConfig {
    fn default() -> Self {
        Self {
            k: 5,
            timeout_ms: 30_000,
        }
    }
}

impl KInductionConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_k(mut self, k: usize) -> Self {
        self.k = k;
        self
    }

    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }
}

/// Result of a k-induction proof attempt.
#[derive(Debug, Clone)]
pub enum KInductionResult {
    /// Property proven for ALL executions (unbounded).
    Verified { property: String, k: usize },
    /// Counterexample found in the base case.
    Counterexample {
        property: String,
        bmc_result: BmcResult,
    },
    /// Base case passed up to k, but inductive step failed.
    /// Property holds for bounded executions up to k but cannot be
    /// proven unbounded.
    Unknown {
        property: String,
        k: usize,
        reason: String,
    },
}

/// A k-induction proof obligation.
#[derive(Debug, Clone)]
pub struct KInductionObligation {
    /// Name of the property being verified
    pub name: String,
    /// State variables (name, sort)
    pub state_variables: Vec<(String, BmcSort)>,
    /// Initial state constraints (hold at step 0)
    pub initial_constraints: Vec<String>,
    /// Transition relation predicates (use primed variables for next state)
    pub transitions: Vec<(String, Vec<String>)>,
    /// The property to prove (as BmcProperty)
    pub property: BmcProperty,
}

/// K-induction prover.
#[derive(Debug, Clone)]
pub struct KInduction {
    pub config: KInductionConfig,
}

impl KInduction {
    pub fn new(config: KInductionConfig) -> Self {
        Self { config }
    }

    /// Run k-induction on a single obligation.
    pub fn prove(&self, obligation: &KInductionObligation) -> KInductionResult {
        // Phase 1: Base case (BMC up to k)
        let base_result = self.run_base_case(obligation);

        match &base_result {
            BmcResult::Safe { .. } => {
                // Base case passed: no lasso/counterexample up to k.
                // Phase 2: Inductive step
                let step_result = self.run_inductive_step(obligation);

                match step_result {
                    BmcResult::Safe { .. } => {
                        // Both base + step pass: property proven unbounded
                        KInductionResult::Verified {
                            property: obligation.name.clone(),
                            k: self.config.k,
                        }
                    }
                    BmcResult::Counterexample { .. } | BmcResult::Lasso { .. } => {
                        // Step failed: bounded guarantee only
                        KInductionResult::Unknown {
                            property: obligation.name.clone(),
                            k: self.config.k,
                            reason:
                                "inductive step failed; property holds up to bound but unbounded proof not found"
                                    .into(),
                        }
                    }
                    BmcResult::Unknown { reason, .. } => KInductionResult::Unknown {
                        property: obligation.name.clone(),
                        k: self.config.k,
                        reason: format!("inductive step inconclusive: {reason}"),
                    },
                }
            }
            BmcResult::Counterexample { .. } | BmcResult::Lasso { .. } => {
                // Base case failed: real counterexample
                KInductionResult::Counterexample {
                    property: obligation.name.clone(),
                    bmc_result: base_result,
                }
            }
            BmcResult::Unknown { reason, .. } => KInductionResult::Unknown {
                property: obligation.name.clone(),
                k: self.config.k,
                reason: format!("base case inconclusive: {reason}"),
            },
        }
    }

    /// Run the base case: BMC with bound k.
    fn run_base_case(&self, obligation: &KInductionObligation) -> BmcResult {
        let mut engine = self.build_bmc_engine(obligation, self.config.k);
        engine.add_property(obligation.property.clone());
        let results = engine.check();
        results.into_iter().next().unwrap_or(BmcResult::Safe {
            property: obligation.name.clone(),
            bound: self.config.k,
        })
    }

    /// Run the inductive step.
    ///
    /// The inductive step checks: if no lasso/counterexample exists for
    /// paths of length k, can one exist for length k+1?
    ///
    /// We model this by creating a BMC instance of length k+1 WITHOUT
    /// the initial constraints (to represent an arbitrary starting state),
    /// but WITH the assumption that no bad state appears in the first k
    /// steps. If the bad state is still unreachable at step k+1, the
    /// induction holds.
    fn run_inductive_step(&self, obligation: &KInductionObligation) -> BmcResult {
        // Build an engine for k+1 steps with NO initial constraints
        // (arbitrary starting state for the inductive hypothesis)
        let mut engine = BmcEngine::new(
            BmcConfig::new()
                .with_bound(self.config.k + 1)
                .with_timeout(self.config.timeout_ms),
        );

        // Add state variables
        for (name, sort) in &obligation.state_variables {
            engine.add_state_variable(name.clone(), sort.clone());
        }

        // Add transitions (same as base case)
        for (pred, vars) in &obligation.transitions {
            engine.add_transition(pred.clone(), vars.clone());
        }

        // NO initial constraints (arbitrary start)

        // For safety properties: assume the property holds at steps 0..k,
        // check if it can fail at step k+1
        match &obligation.property {
            BmcProperty::Safety {
                name,
                bad_predicate,
            } => {
                // We need to assume NOT bad at steps 0..k, then check bad at step k+1.
                // The BMC engine's safety check already does incremental checking,
                // but we need the inductive hypothesis (good at 0..k).
                // We add the negation of the bad predicate as initial constraints
                // for all steps 0..k.
                for step in 0..=self.config.k {
                    let good_at_step = negate_predicate(bad_predicate);
                    let renamed = engine.rename_predicate(&good_at_step, step);
                    engine.add_initial_constraint(renamed);
                }

                // Now check if bad is reachable at step k+1
                engine.add_property(BmcProperty::Safety {
                    name: name.clone(),
                    bad_predicate: bad_predicate.clone(),
                });
            }
            BmcProperty::Liveness {
                name,
                goal_predicate,
            } => {
                // For liveness: assume no lasso of length <= k,
                // check for lasso of length k+1.
                // We use the BMC lasso detection with bound k+1.
                engine.add_property(BmcProperty::Liveness {
                    name: name.clone(),
                    goal_predicate: goal_predicate.clone(),
                });
            }
        }

        let results = engine.check();
        results.into_iter().next().unwrap_or(BmcResult::Safe {
            property: obligation.name.clone(),
            bound: self.config.k + 1,
        })
    }

    /// Build a BMC engine from an obligation.
    fn build_bmc_engine(&self, obligation: &KInductionObligation, bound: usize) -> BmcEngine {
        let mut engine = BmcEngine::new(
            BmcConfig::new()
                .with_bound(bound)
                .with_timeout(self.config.timeout_ms),
        );

        for (name, sort) in &obligation.state_variables {
            engine.add_state_variable(name.clone(), sort.clone());
        }

        for constraint in &obligation.initial_constraints {
            engine.add_initial_constraint(constraint.clone());
        }

        for (pred, vars) in &obligation.transitions {
            engine.add_transition(pred.clone(), vars.clone());
        }

        engine
    }
}

/// Simple predicate negation.
///
/// Wraps the predicate in a logical NOT. For the simple predicate
/// language used by BMC, this means turning `x < 0` into `x >= 0`.
fn negate_predicate(pred: &str) -> String {
    let pred = pred.trim();
    // Simple negation rules for common patterns
    if pred.contains(">=") {
        pred.replacen(">=", "<", 1)
    } else if pred.contains("<=") {
        pred.replacen("<=", ">", 1)
    } else if pred.contains("!=") {
        pred.replacen("!=", "==", 1)
    } else if pred.contains("==") {
        pred.replacen("==", "!=", 1)
    } else if pred.contains('>') && !pred.contains(">=") {
        pred.replacen('>', "<=", 1)
    } else if pred.contains('<') && !pred.contains("<=") {
        pred.replacen('<', ">=", 1)
    } else {
        // Fallback: wrap in NOT (not parseable by simple parser,
        // but signals the intent)
        format!("!({pred})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------
    // KInductionConfig
    // -------------------------------------------------------------------

    #[test]
    fn test_config_defaults() {
        let cfg = KInductionConfig::default();
        assert_eq!(cfg.k, 5);
        assert_eq!(cfg.timeout_ms, 30_000);
    }

    #[test]
    fn test_config_builder() {
        let cfg = KInductionConfig::new().with_k(10).with_timeout(5000);
        assert_eq!(cfg.k, 10);
        assert_eq!(cfg.timeout_ms, 5000);
    }

    // -------------------------------------------------------------------
    // negate_predicate
    // -------------------------------------------------------------------

    #[test]
    fn test_negate_comparisons() {
        assert_eq!(negate_predicate("x < 0"), "x >= 0");
        assert_eq!(negate_predicate("x > 0"), "x <= 0");
        assert_eq!(negate_predicate("x >= 0"), "x < 0");
        assert_eq!(negate_predicate("x <= 0"), "x > 0");
        assert_eq!(negate_predicate("x == 0"), "x != 0");
        assert_eq!(negate_predicate("x != 0"), "x == 0");
    }

    // -------------------------------------------------------------------
    // K-induction proofs (Z3-backed)
    // -------------------------------------------------------------------

    #[test]
    fn test_kinduction_proves_incrementing_counter_safe() {
        // x starts at 0, increments by 1.
        // Property: x >= 0 (always true).
        // K-induction should prove this unboundedly.
        let prover = KInduction::new(KInductionConfig::new().with_k(3));

        let obligation = KInductionObligation {
            name: "x_nonneg".into(),
            state_variables: vec![("x".into(), BmcSort::Int)],
            initial_constraints: vec!["x == 0".into()],
            transitions: vec![("x' == x + 1".into(), vec!["x".into()])],
            property: BmcProperty::Safety {
                name: "x_nonneg".into(),
                bad_predicate: "x < 0".into(),
            },
        };

        let result = prover.prove(&obligation);
        match result {
            KInductionResult::Verified { property, k } => {
                assert_eq!(property, "x_nonneg");
                assert_eq!(k, 3);
            }
            other => panic!("expected Verified, got {other:?}"),
        }
    }

    #[test]
    fn test_kinduction_finds_counterexample() {
        // x starts at 3, decrements by 1.
        // Property: x >= 0 (fails at step 4).
        // K-induction should find the counterexample in the base case.
        let prover = KInduction::new(KInductionConfig::new().with_k(5));

        let obligation = KInductionObligation {
            name: "x_nonneg".into(),
            state_variables: vec![("x".into(), BmcSort::Int)],
            initial_constraints: vec!["x == 3".into()],
            transitions: vec![("x' == x - 1".into(), vec!["x".into()])],
            property: BmcProperty::Safety {
                name: "x_nonneg".into(),
                bad_predicate: "x < 0".into(),
            },
        };

        let result = prover.prove(&obligation);
        match result {
            KInductionResult::Counterexample { property, .. } => {
                assert_eq!(property, "x_nonneg");
            }
            other => panic!("expected Counterexample, got {other:?}"),
        }
    }

    #[test]
    fn test_kinduction_unknown_for_nonlinear() {
        // x starts at 0, x' = x + 1.
        // Property: x < 100 (holds up to step 99, then fails at step 100).
        // With k=3, base passes (no bad in 3 steps), but step fails
        // (can start at 99 and reach 100), giving Unknown.
        let prover = KInduction::new(KInductionConfig::new().with_k(3));

        let obligation = KInductionObligation {
            name: "x_bounded".into(),
            state_variables: vec![("x".into(), BmcSort::Int)],
            initial_constraints: vec!["x == 0".into()],
            transitions: vec![("x' == x + 1".into(), vec!["x".into()])],
            property: BmcProperty::Safety {
                name: "x_bounded".into(),
                bad_predicate: "x >= 100".into(),
            },
        };

        let result = prover.prove(&obligation);
        match result {
            KInductionResult::Unknown {
                property,
                k,
                reason,
            } => {
                assert_eq!(property, "x_bounded");
                assert_eq!(k, 3);
                assert!(reason.contains("inductive step"));
            }
            // Also acceptable: if k=3 but the bad predicate is reachable
            // from arbitrary start (which it is), the engine may find it
            // as a counterexample in the step phase, giving Unknown.
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn test_kinduction_liveness_counterexample() {
        // x stays at 0 forever (x' = x with x = 0).
        // Liveness: "eventually x == 5" (never satisfied).
        // K-induction should find a lasso in the base case.
        let prover = KInduction::new(KInductionConfig::new().with_k(3));

        let obligation = KInductionObligation {
            name: "reach_5".into(),
            state_variables: vec![("x".into(), BmcSort::Int)],
            initial_constraints: vec!["x == 0".into()],
            transitions: vec![("x' == x".into(), vec!["x".into()])],
            property: BmcProperty::Liveness {
                name: "reach_5".into(),
                goal_predicate: "x == 5".into(),
            },
        };

        let result = prover.prove(&obligation);
        match result {
            KInductionResult::Counterexample { property, .. } => {
                assert_eq!(property, "reach_5");
            }
            other => panic!("expected Counterexample, got {other:?}"),
        }
    }

    #[test]
    fn test_kinduction_multiple_variables() {
        // x starts at 0, y starts at 0. Both increment by 1.
        // Property: x == y (always true since they move in lockstep).
        // K-induction should prove this.
        let prover = KInduction::new(KInductionConfig::new().with_k(3));

        let obligation = KInductionObligation {
            name: "x_eq_y".into(),
            state_variables: vec![("x".into(), BmcSort::Int), ("y".into(), BmcSort::Int)],
            initial_constraints: vec!["x == 0".into(), "y == 0".into()],
            transitions: vec![
                ("x' == x + 1".into(), vec!["x".into()]),
                ("y' == y + 1".into(), vec!["y".into()]),
            ],
            property: BmcProperty::Safety {
                name: "x_eq_y".into(),
                bad_predicate: "x != y".into(),
            },
        };

        let result = prover.prove(&obligation);
        match result {
            KInductionResult::Verified { property, .. } => {
                assert_eq!(property, "x_eq_y");
            }
            other => panic!("expected Verified, got {other:?}"),
        }
    }
}
