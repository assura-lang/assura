//! Standalone CVC5 native validity and satisfiability checks.
//!
//! Routes solver outcomes through [`crate::solver_outcome_policy`] so the
//! SAT/UNSAT → VerificationResult mapping is identical to the Z3 path.
#![cfg_attr(feature = "z3-verify", allow(dead_code))]

use std::collections::HashMap;

use assura_ast::SpExpr;

use crate::VerificationResult;
use crate::cvc5_collect::collect_cvc5_var_names_from_assumptions;
use crate::cvc5_native_encoder::{default_cvc5_encoder_state, encode_expr_cvc5};
use crate::cvc5_verify_native_solver::{
    Cvc5SolverOpts, assert_cvc5_axioms, build_cvc5_var_map, extract_cvc5_counterexample_model,
    extract_cvc5_unsat_core_labels, new_cvc5_solver,
};
use crate::cvc5_verify_shared::{cvc5_encode_failure, cvc5_unmodelable_precheck};
use crate::solver_outcome_policy::ClauseSatOutcome;

/// Convert a CVC5 `check_sat` result to a solver-neutral [`ClauseSatOutcome`].
///
/// Mirrors [`crate::z3_backend::solver::z3_clause_sat_outcome`] so that both
/// backends feed the same shared [`crate::solver_outcome_policy::interpret_clause_check_result`].
fn cvc5_clause_sat_outcome(
    sat_result: &cvc5::Result,
    solver: &cvc5::Solver,
    var_map: &HashMap<String, cvc5::Term>,
    tracked_assumptions: &[cvc5::Term],
) -> ClauseSatOutcome {
    if sat_result.is_unsat() {
        let core = extract_cvc5_unsat_core_labels(solver, tracked_assumptions);
        ClauseSatOutcome::unsat_with_core(core)
    } else if sat_result.is_sat() {
        let (model_str, counter_model) = extract_cvc5_counterexample_model(solver, var_map);
        ClauseSatOutcome::sat(model_str, counter_model)
    } else {
        // CVC5 returns Unknown for timeout and incomplete reasoning.
        // Map to Timeout (matches prior behavior). A future improvement
        // could query solver statistics to distinguish timeout from other
        // unknowns, mirroring the Z3 get_reason_unknown() path.
        ClauseSatOutcome::timeout()
    }
}

pub(crate) fn check_validity_cvc5(
    desc: &str,
    assumptions: &[&SpExpr],
    body: &SpExpr,
) -> VerificationResult {
    if let Some(result) = cvc5_unmodelable_precheck(desc, body) {
        return result;
    }

    let tm = cvc5::TermManager::new();
    let mut solver = new_cvc5_solver(
        &tm,
        Cvc5SolverOpts {
            unsat_core: true,
            ..Default::default()
        },
    );

    let var_names = collect_cvc5_var_names_from_assumptions(assumptions, body);
    let mut var_map = build_cvc5_var_map(&tm, &var_names, &[]);

    let mut enc_state = default_cvc5_encoder_state();

    let bool_sort = tm.boolean_sort();
    let mut tracked_assumptions: Vec<cvc5::Term> = Vec::new();

    // Track assumptions with labels for unsat-core extraction (#266).
    for (i, a) in assumptions.iter().enumerate() {
        if let Some(term) = encode_expr_cvc5(&tm, a, &mut var_map, &mut enc_state) {
            let label = format!("req_{i}");
            let track = tm.mk_const(bool_sort.clone(), &label);
            tracked_assumptions.push(track.clone());
            let implication = tm.mk_term(cvc5::Kind::Implies, &[track, term]);
            solver.assert_formula(implication);
        }
    }

    // Encode body
    let body_term = match encode_expr_cvc5(&tm, body, &mut var_map, &mut enc_state) {
        Some(t) => t,
        None => return cvc5_encode_failure(desc),
    };

    assert_cvc5_axioms(&mut solver, &enc_state.axioms);

    let negated = tm.mk_term(cvc5::Kind::Not, &[body_term]);
    solver.assert_formula(negated);

    let sat_result = if tracked_assumptions.is_empty() {
        solver.check_sat()
    } else {
        solver.check_sat_assuming(&tracked_assumptions)
    };
    let outcome = cvc5_clause_sat_outcome(&sat_result, &solver, &var_map, &tracked_assumptions);
    crate::solver_outcome_policy::interpret_clause_check_result(
        desc,
        &assura_ast::ClauseKind::Ensures,
        outcome,
    )
}

/// Check satisfiability of `body` under `assumptions` using CVC5.
///
/// For invariants: assert all assumptions + body, check-sat.
/// SAT = invariant is satisfiable (Verified), UNSAT = unsatisfiable (Counterexample).
pub(crate) fn check_satisfiability_cvc5(
    desc: &str,
    assumptions: &[&SpExpr],
    body: &SpExpr,
) -> VerificationResult {
    if let Some(result) = cvc5_unmodelable_precheck(desc, body) {
        return result;
    }

    let tm = cvc5::TermManager::new();
    let mut solver = new_cvc5_solver(&tm, Cvc5SolverOpts::default());

    let var_names = collect_cvc5_var_names_from_assumptions(assumptions, body);
    let mut var_map = build_cvc5_var_map(&tm, &var_names, &[]);

    let mut enc_state = default_cvc5_encoder_state();

    for a in assumptions {
        if let Some(term) = encode_expr_cvc5(&tm, a, &mut var_map, &mut enc_state) {
            solver.assert_formula(term);
        }
    }

    let body_term = match encode_expr_cvc5(&tm, body, &mut var_map, &mut enc_state) {
        Some(t) => t,
        None => return cvc5_encode_failure(desc),
    };

    assert_cvc5_axioms(&mut solver, &enc_state.axioms);

    solver.assert_formula(body_term);

    let sat_result = solver.check_sat();
    let outcome = cvc5_clause_sat_outcome(&sat_result, &solver, &var_map, &[]);
    crate::solver_outcome_policy::interpret_clause_check_result(
        desc,
        &assura_ast::ClauseKind::Invariant,
        outcome,
    )
}
