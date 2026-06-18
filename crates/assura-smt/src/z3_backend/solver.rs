//! Z3 solver interaction: validity/satisfiability checks, model extraction,
//! and clause description formatting.

use crate::*;
use z3::ast::Ast;
use z3::{Model, Params, SatResult, Solver, ast};

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
pub(super) fn extract_counter_model(model: &Model) -> CounterexampleModel {
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
// Unsat core helpers (#266)
// -----------------------------------------------------------------------

/// Enable unsat-core production and optional core minimization on a solver.
pub(crate) fn enable_unsat_cores(solver: &Solver) {
    let mut params = Params::new();
    params.set_bool("unsat_core", true);
    params.set_bool("smt.core.minimize", true);
    solver.set_params(&params);
}

/// Assert `expr` and track it under `label` for unsat-core extraction.
pub(crate) fn assert_tracked(solver: &Solver, expr: &ast::Bool, label: &str) {
    let track = ast::Bool::new_const(label);
    solver.assert_and_track(expr, &track);
}

/// Extract tracking-label names from the solver's unsat core after UNSAT.
pub(crate) fn extract_unsat_core_labels(solver: &Solver) -> Option<Vec<String>> {
    let core = solver.get_unsat_core();
    if core.is_empty() {
        return None;
    }
    let mut labels: Vec<String> = core.iter().map(|b| b.decl().name().to_string()).collect();
    labels.sort();
    labels.dedup();
    Some(labels)
}

// -----------------------------------------------------------------------
// Solver result interpretation
// -----------------------------------------------------------------------

/// Interpret solver result for a validity check (ensures/rule).
/// We negate the goal and check-sat: UNSAT = valid.
pub(crate) fn check_validity(solver: &Solver, desc: String, results: &mut Vec<VerificationResult>) {
    match solver.check() {
        SatResult::Unsat => {
            let core = extract_unsat_core_labels(solver);
            results.push(VerificationResult::Verified {
                clause_desc: desc,
                unsat_core: core,
            });
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
    solver: &Solver,
    desc: String,
    results: &mut Vec<VerificationResult>,
) {
    match solver.check() {
        SatResult::Sat => {
            results.push(VerificationResult::verified(desc));
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
