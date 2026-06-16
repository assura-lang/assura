//! Z3 solver interaction: validity/satisfiability checks, model extraction,
//! and clause description formatting.

use crate::*;
use z3::{Model, SatResult, Solver};

// -----------------------------------------------------------------------
// Clause description helper
// -----------------------------------------------------------------------

pub(super) fn clause_desc(parent_name: &str, kind: &ClauseKind) -> String {
    let kind_str = match kind {
        ClauseKind::Requires => "requires",
        ClauseKind::Ensures => "ensures",
        ClauseKind::Invariant => "invariant",
        ClauseKind::Effects => "effects",
        ClauseKind::Modifies => "modifies",
        ClauseKind::Input => "input",
        ClauseKind::Output => "output",
        ClauseKind::Errors => "errors",
        ClauseKind::Rule => "rule",
        ClauseKind::DataFlow => "data_flow",
        ClauseKind::MustNot => "must_not",
        ClauseKind::Decreases => "decreases",
        ClauseKind::Ordering => "ordering",
        ClauseKind::Other(s) => s.as_str(),
    };
    format!("{parent_name}::{kind_str}")
}

// -----------------------------------------------------------------------
// Solver result interpretation
// -----------------------------------------------------------------------

// -----------------------------------------------------------------------
// Model extraction (T040)
// -----------------------------------------------------------------------

/// Parse a Z3 model into a structured `CounterexampleModel`.
///
/// Iterates over the constant declarations in the model, evaluates
/// each one with model completion, and collects `(name, value)` pairs.
/// Internal variables (prefixed with `__`) are excluded.
pub(super) fn extract_counter_model(model: &Model<'_>) -> CounterexampleModel {
    let mut variables: Vec<(String, String)> = Vec::new();
    for decl in model.iter() {
        // Skip non-constant declarations (uninterpreted functions with
        // arity > 0 produce multi-line `{ value }` blocks in the model)
        if decl.arity() > 0 {
            continue;
        }
        let name = decl.name();
        // Skip internal/fresh/coercion variables, but keep __result
        if name.starts_with("__") && name != "__result" {
            continue;
        }
        // Try to get the interpretation as a string
        let value = model
            .get_const_interp(&decl.apply(&[]))
            .map(|v| format!("{v}"))
            .unwrap_or_else(|| "?".into());
        // Strip __field_ prefix from variable names leaked by the encoder
        let clean_name = name.strip_prefix("__field_").unwrap_or(&name).to_string();
        variables.push((clean_name, value));
    }
    // Sort for deterministic output
    variables.sort_by(|a, b| a.0.cmp(&b.0));
    CounterexampleModel { variables }
}

// -----------------------------------------------------------------------
// Solver result interpretation
// -----------------------------------------------------------------------

/// Interpret solver result for a validity check (ensures/rule).
/// We negate the goal and check-sat: UNSAT = valid.
pub(super) fn check_validity(
    solver: &Solver<'_>,
    desc: String,
    results: &mut Vec<VerificationResult>,
) {
    match solver.check() {
        SatResult::Unsat => {
            results.push(VerificationResult::Verified { clause_desc: desc });
        }
        SatResult::Sat => {
            let (model_str, counter_model) = if let Some(m) = solver.get_model() {
                let cm = extract_counter_model(&m);
                (format!("{m}"), Some(cm))
            } else {
                ("(no model)".into(), None)
            };
            results.push(VerificationResult::Counterexample {
                clause_desc: desc,
                model: model_str,
                counter_model,
            });
        }
        SatResult::Unknown => {
            let reason = solver
                .get_reason_unknown()
                .unwrap_or_else(|| "unknown".into());
            if reason.contains("timeout") {
                results.push(VerificationResult::Timeout { clause_desc: desc });
            } else {
                results.push(VerificationResult::Unknown {
                    clause_desc: desc,
                    reason,
                });
            }
        }
    }
}

/// Interpret solver result for a satisfiability check (invariant).
/// We assert the formula directly: SAT = satisfiable = good.
pub(super) fn check_satisfiability(
    solver: &Solver<'_>,
    desc: String,
    results: &mut Vec<VerificationResult>,
) {
    match solver.check() {
        SatResult::Sat => {
            results.push(VerificationResult::Verified { clause_desc: desc });
        }
        SatResult::Unsat => {
            results.push(VerificationResult::Counterexample {
                clause_desc: desc,
                model: "invariant is unsatisfiable (always false)".into(),
                counter_model: None,
            });
        }
        SatResult::Unknown => {
            let reason = solver
                .get_reason_unknown()
                .unwrap_or_else(|| "unknown".into());
            if reason.contains("timeout") {
                results.push(VerificationResult::Timeout { clause_desc: desc });
            } else {
                results.push(VerificationResult::Unknown {
                    clause_desc: desc,
                    reason,
                });
            }
        }
    }
}
