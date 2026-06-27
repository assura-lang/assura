//! Shared clause **solver-outcome** interpretation (one compiler brain).
//!
//! Owns SAT/UNSAT/timeout/unknown → [`VerificationResult`] mapping for clause
//! checks (validity vs satisfiability). Z3 and CVC5 (native + shell) supply
//! only the coarse outcome + optional model/core; they must not diverge on
//! whether invariant UNSAT is a counterexample vs ensures UNSAT is verified.
//!
//! Complements [`crate::clause_policy::ClauseCheckPolarity`] (what to assert)
//! and [`crate::clause_gate_policy`] (pre-solver gates). Does not unify
//! expression encoding or model extraction APIs.

use assura_ast::ClauseKind;

use crate::CounterexampleModel;
use crate::VerificationResult;

/// Coarse `check-sat` outcome, solver-neutral (Z3 `SatResult`, CVC5 `is_sat`/`is_unsat`, shell stdout).
#[derive(Debug, Clone)]
pub(crate) enum ClauseSatOutcome {
    Unsat {
        /// Optional unsat-core labels (Z3 when cores enabled; CVC5 often empty).
        unsat_core: Option<Vec<String>>,
    },
    Sat {
        model_str: String,
        counter_model: Option<CounterexampleModel>,
    },
    Timeout,
    /// Solver returned unknown for a non-timeout reason.
    Unknown {
        reason: String,
    },
}

impl ClauseSatOutcome {
    #[inline]
    pub(crate) fn unsat() -> Self {
        Self::Unsat { unsat_core: None }
    }

    #[inline]
    pub(crate) fn unsat_with_core(core: Vec<String>) -> Self {
        Self::Unsat {
            unsat_core: if core.is_empty() { None } else { Some(core) },
        }
    }

    #[inline]
    pub(crate) fn sat(model_str: String, counter_model: Option<CounterexampleModel>) -> Self {
        Self::Sat {
            model_str,
            counter_model,
        }
    }

    #[inline]
    pub(crate) fn timeout() -> Self {
        Self::Timeout
    }

    #[inline]
    pub(crate) fn unknown(reason: impl Into<String>) -> Self {
        Self::Unknown {
            reason: reason.into(),
        }
    }
}

/// Whether this clause kind uses satisfiability semantics (SAT = ok, UNSAT = fail).
///
/// Aligns with [`crate::clause_policy::ClauseCheckPolarity::SatisfiabilityAssertBody`]
/// (currently only `invariant`).
#[inline]
pub(crate) fn clause_uses_satisfiability_semantics(kind: &ClauseKind) -> bool {
    matches!(kind, ClauseKind::Invariant)
}

/// Map SAT/UNSAT/timeout/unknown to [`VerificationResult`] using clause-kind semantics.
///
/// - **Satisfiability** (`invariant`): SAT → verified; UNSAT → counterexample (unsat body).
/// - **Validity** (ensures, rule, must_not, decreases, …): UNSAT → verified; SAT → counterexample.
pub(crate) fn interpret_clause_check_result(
    desc: &str,
    kind: &ClauseKind,
    outcome: ClauseSatOutcome,
) -> VerificationResult {
    let sat_semantics = clause_uses_satisfiability_semantics(kind);
    match outcome {
        ClauseSatOutcome::Unsat { unsat_core } => {
            if sat_semantics {
                VerificationResult::Counterexample {
                    clause_desc: desc.to_string(),
                    model: "invariant is unsatisfiable".to_string(),
                    counter_model: None,
                }
            } else if let Some(core) = unsat_core.filter(|c| !c.is_empty()) {
                VerificationResult::verified_with_core(desc.to_string(), core)
            } else {
                VerificationResult::verified(desc.to_string())
            }
        }
        ClauseSatOutcome::Sat {
            model_str,
            counter_model,
        } => {
            if sat_semantics {
                VerificationResult::verified(desc.to_string())
            } else {
                VerificationResult::Counterexample {
                    clause_desc: desc.to_string(),
                    model: model_str,
                    counter_model,
                }
            }
        }
        ClauseSatOutcome::Timeout => VerificationResult::Timeout {
            clause_desc: desc.to_string(),
        },
        ClauseSatOutcome::Unknown { reason } => VerificationResult::Unknown {
            clause_desc: desc.to_string(),
            reason,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensures_unsat_is_verified() {
        let r = interpret_clause_check_result(
            "C::ensures",
            &ClauseKind::Ensures,
            ClauseSatOutcome::unsat(),
        );
        assert!(matches!(r, VerificationResult::Verified { .. }));
    }

    #[test]
    fn ensures_unsat_with_core_preserves_core() {
        let r = interpret_clause_check_result(
            "C::ensures",
            &ClauseKind::Ensures,
            ClauseSatOutcome::unsat_with_core(vec!["r0".into()]),
        );
        match r {
            VerificationResult::Verified { unsat_core, .. } => {
                assert_eq!(unsat_core, Some(vec!["r0".to_string()]));
            }
            other => panic!("expected Verified with core, got {other:?}"),
        }
    }

    #[test]
    fn ensures_sat_is_counterexample() {
        let r = interpret_clause_check_result(
            "C::ensures",
            &ClauseKind::Ensures,
            ClauseSatOutcome::sat("x = 1".into(), None),
        );
        assert!(matches!(r, VerificationResult::Counterexample { .. }));
    }

    #[test]
    fn invariant_unsat_is_counterexample() {
        let r = interpret_clause_check_result(
            "C::invariant",
            &ClauseKind::Invariant,
            ClauseSatOutcome::unsat(),
        );
        assert!(matches!(r, VerificationResult::Counterexample { .. }));
    }

    #[test]
    fn invariant_sat_is_verified() {
        let r = interpret_clause_check_result(
            "C::invariant",
            &ClauseKind::Invariant,
            ClauseSatOutcome::sat(String::new(), None),
        );
        assert!(matches!(r, VerificationResult::Verified { .. }));
    }

    #[test]
    fn must_not_uses_validity_semantics() {
        let r = interpret_clause_check_result(
            "C::must_not",
            &ClauseKind::MustNot,
            ClauseSatOutcome::unsat(),
        );
        assert!(matches!(r, VerificationResult::Verified { .. }));
    }

    #[test]
    fn timeout_and_unknown_pass_through() {
        assert!(matches!(
            interpret_clause_check_result(
                "C::ensures",
                &ClauseKind::Ensures,
                ClauseSatOutcome::timeout()
            ),
            VerificationResult::Timeout { .. }
        ));
        assert!(matches!(
            interpret_clause_check_result(
                "C::ensures",
                &ClauseKind::Ensures,
                ClauseSatOutcome::unknown("incomplete")
            ),
            VerificationResult::Unknown { .. }
        ));
    }
}
