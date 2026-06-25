//! CVC5 native solver setup, assertion, and result helpers.

#![cfg_attr(feature = "z3-verify", allow(dead_code))]

use std::collections::{HashMap, HashSet};

use assura_ast::{ClauseKind, SpExpr};

use crate::cvc5_native_encoder::{Cvc5EncoderState, encode_expr_cvc5};
use crate::cvc5_verify_shared::{
    Cvc5ClauseSatOutcome, Cvc5TypeConstraint, collect_cvc5_type_constraints,
    cvc5_interpret_clause_check_result,
};
use crate::encode_atom_policy::sanitize_smt_name;
use crate::{CounterexampleModel, VerificationResult};

pub(crate) fn build_cvc5_var_map<'a>(
    tm: &'a cvc5::TermManager,
    var_names: &HashSet<String>,
    constants: &[(String, i64)],
) -> HashMap<String, cvc5::Term<'a>> {
    let mut var_map = HashMap::new();
    for name in var_names {
        var_map.insert(name.clone(), tm.mk_const(tm.integer_sort(), name));
    }
    for (name, value) in constants {
        let key = sanitize_smt_name(name);
        var_map.insert(key, tm.mk_integer(*value));
    }
    var_map
}

pub(crate) fn assert_cvc5_solver_prelude<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    var_map: &HashMap<String, cvc5::Term<'a>>,
    params: &[assura_ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    narrowings: &[(String, i64)],
) {
    let vars: HashSet<String> = var_map.keys().cloned().collect();
    let constraints =
        collect_cvc5_type_constraints(&vars, params, return_ty, constants, narrowings);
    let zero = tm.mk_integer(0);
    for constraint in constraints {
        match constraint {
            Cvc5TypeConstraint::NatNonNegative(name) => {
                if let Some(term) = var_map.get(&name) {
                    solver
                        .assert_formula(tm.mk_term(cvc5::Kind::Geq, &[term.clone(), zero.clone()]));
                }
            }
            Cvc5TypeConstraint::ConstantEq(..) => {
                // Skip: build_cvc5_var_map already inserts mk_integer(value)
                // for constants, so asserting value == value is redundant (#467).
            }
            Cvc5TypeConstraint::NarrowingLe(name, value) => {
                if let Some(var) = var_map.get(&name) {
                    solver.assert_formula(
                        tm.mk_term(cvc5::Kind::Leq, &[var.clone(), tm.mk_integer(value)]),
                    );
                }
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct Cvc5SolverOpts {
    pub(crate) incremental: bool,
    pub(crate) unsat_core: bool,
}

pub(crate) fn new_cvc5_solver<'a>(
    tm: &'a cvc5::TermManager,
    opts: Cvc5SolverOpts,
) -> cvc5::Solver<'a> {
    let mut solver = cvc5::Solver::new(tm);
    solver.set_logic("ALL");
    solver.set_option("produce-models", "true");
    solver.set_option(
        "tlimit",
        crate::encode_timeout_policy::DEFAULT_SOLVER_TIMEOUT_TLIMIT,
    );
    if opts.incremental {
        solver.set_option("incremental", "true");
    }
    if opts.unsat_core {
        solver.set_option("produce-unsat-cores", "true");
        solver.set_option("produce-unsat-assumptions", "true");
    }
    solver
}

pub(crate) fn assert_cvc5_axioms<'a>(solver: &mut cvc5::Solver<'a>, axioms: &[cvc5::Term<'a>]) {
    for axiom in axioms {
        solver.assert_formula(axiom.clone());
    }
}

pub(crate) fn assert_cvc5_axioms_since<'a>(
    solver: &mut cvc5::Solver<'a>,
    axioms: &[cvc5::Term<'a>],
    start: usize,
) {
    for axiom in &axioms[start..] {
        solver.assert_formula(axiom.clone());
    }
}

pub(crate) fn assert_cvc5_frame_axioms<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    var_map: &HashMap<String, cvc5::Term<'a>>,
    frame_vars: &[String],
) {
    for var_name in frame_vars {
        let current_key = sanitize_smt_name(var_name);
        let old_key = crate::encode_atom_policy::old_snapshot_name(var_name);
        let current = var_map
            .get(&current_key)
            .cloned()
            .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &current_key));
        let old_var = var_map
            .get(&old_key)
            .cloned()
            .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &old_key));
        solver.assert_formula(tm.mk_term(cvc5::Kind::Equal, &[current, old_var]));
    }
}

pub(crate) fn assert_cvc5_clause_check<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    kind: ClauseKind,
    body_term: cvc5::Term<'a>,
) {
    use crate::clause_policy::{ClauseCheckPolarity, clause_check_polarity};

    match clause_check_polarity(&kind) {
        Some(ClauseCheckPolarity::ValidityNegateBody) => {
            let negated = tm.mk_term(cvc5::Kind::Not, &[body_term]);
            solver.assert_formula(negated);
        }
        Some(ClauseCheckPolarity::SatisfiabilityAssertBody)
        | Some(ClauseCheckPolarity::ValidityAssertBody) => {
            solver.assert_formula(body_term);
        }
        Some(ClauseCheckPolarity::DecreasesNonNeg) => {
            // Parity with Z3: extract measure as integer, assert NOT(measure >= 0).
            // UNSAT means the measure is always non-negative (verified).
            let zero = tm.mk_integer(0);
            let measure_int = if body_term.sort().is_boolean() {
                tm.mk_term(
                    cvc5::Kind::Ite,
                    &[body_term, tm.mk_integer(1), tm.mk_integer(0)],
                )
            } else {
                body_term
            };
            let non_neg = tm.mk_term(cvc5::Kind::Geq, &[measure_int, zero]);
            let negated = tm.mk_term(cvc5::Kind::Not, &[non_neg]);
            solver.assert_formula(negated);
        }
        None => {}
    }
}

pub(crate) fn extract_cvc5_counterexample_model<'a>(
    solver: &cvc5::Solver<'a>,
    var_map: &HashMap<String, cvc5::Term<'a>>,
) -> (String, Option<CounterexampleModel>) {
    let mut variables: Vec<(String, String)> = var_map
        .iter()
        .filter(|(name, _)| crate::encode_atom_policy::is_counterexample_user_var(name))
        .map(|(name, term)| {
            let val = solver.get_value(term.clone());
            let clean = crate::encode_atom_policy::counterexample_display_name(name).to_string();
            (clean, val.to_string())
        })
        .collect();
    variables.sort_by(|(a, _), (b, _)| a.cmp(b));
    let model_str = variables
        .iter()
        .map(|(n, v)| format!("{n} = {v}"))
        .collect::<Vec<_>>()
        .join(", ");
    let counter_model = if variables.is_empty() {
        None
    } else {
        Some(CounterexampleModel { variables })
    };
    (model_str, counter_model)
}

/// Convert a CVC5 `check_sat` result to a solver-neutral [`Cvc5ClauseSatOutcome`].
///
/// Shared by the contract path (`finish_cvc5_clause_check`) and the standalone
/// validity/satisfiability checks (`cvc5_verify_native_checks`).
pub(crate) fn cvc5_clause_sat_outcome<'a>(
    sat_result: &cvc5::Result,
    solver: &cvc5::Solver<'a>,
    var_map: &HashMap<String, cvc5::Term<'a>>,
    tracked_assumptions: &[cvc5::Term<'a>],
) -> Cvc5ClauseSatOutcome {
    if sat_result.is_unsat() {
        if tracked_assumptions.is_empty() {
            Cvc5ClauseSatOutcome::unsat()
        } else {
            let core = extract_cvc5_unsat_core_labels(solver, tracked_assumptions);
            Cvc5ClauseSatOutcome::unsat_with_core(core)
        }
    } else if sat_result.is_sat() {
        let (model_str, counter_model) = extract_cvc5_counterexample_model(solver, var_map);
        Cvc5ClauseSatOutcome::sat(model_str, counter_model)
    } else {
        // CVC5 returned Unknown: classify via unknown_explanation (#456).
        let explanation = sat_result.unknown_explanation();
        match explanation {
            cvc5::UnknownExplanation::Timeout
            | cvc5::UnknownExplanation::Resourceout
            | cvc5::UnknownExplanation::Memout => Cvc5ClauseSatOutcome::timeout(),
            _ => {
                let reason = cvc5::unknown_explanation_to_string(explanation);
                Cvc5ClauseSatOutcome::unknown(reason)
            }
        }
    }
}

pub(crate) fn finish_cvc5_clause_check<'a>(
    desc: &str,
    kind: ClauseKind,
    solver: &mut cvc5::Solver<'a>,
    var_map: &HashMap<String, cvc5::Term<'a>>,
    tracked_labels: &[cvc5::Term<'a>],
) -> VerificationResult {
    let sat_result = if tracked_labels.is_empty() {
        solver.check_sat()
    } else {
        solver.check_sat_assuming(tracked_labels)
    };
    let outcome = cvc5_clause_sat_outcome(&sat_result, solver, var_map, tracked_labels);
    cvc5_interpret_clause_check_result(desc, kind, outcome)
}

pub(crate) fn inject_cvc5_lemma_assumptions<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    body: &'a SpExpr,
    defs: &std::collections::HashMap<String, Vec<&SpExpr>>,
    var_map: &mut HashMap<String, cvc5::Term<'a>>,
    enc_state: &mut Cvc5EncoderState<'a>,
) {
    inject_cvc5_lemma_assumptions_for_bodies(
        tm,
        solver,
        std::iter::once(body),
        defs,
        var_map,
        enc_state,
    );
}

pub(crate) fn inject_cvc5_lemma_assumptions_for_bodies<'a, I>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    bodies: I,
    defs: &std::collections::HashMap<String, Vec<&SpExpr>>,
    var_map: &mut HashMap<String, cvc5::Term<'a>>,
    enc_state: &mut Cvc5EncoderState<'a>,
) where
    I: IntoIterator<Item = &'a SpExpr>,
{
    let bodies_vec: Vec<&SpExpr> = bodies.into_iter().collect();
    for ens_body in crate::lemma_inject_policy::lemma_ensures_bodies_for_exprs(bodies_vec, defs) {
        if let Some(term) = encode_expr_cvc5(tm, ens_body, var_map, enc_state) {
            solver.assert_formula(term);
        }
    }
}
pub(crate) fn extract_cvc5_unsat_core_labels(
    solver: &cvc5::Solver,
    tracked: &[cvc5::Term],
) -> Vec<String> {
    let mut labels: Vec<String> = solver
        .get_unsat_assumptions()
        .iter()
        .map(|t| cvc5_term_label(t))
        .collect();
    if labels.is_empty() {
        labels = solver
            .get_unsat_core()
            .iter()
            .map(|t| cvc5_term_label(t))
            .collect();
    }
    if labels.is_empty() && !tracked.is_empty() {
        labels = tracked.iter().map(|t| cvc5_term_label(t)).collect();
    }
    labels.sort();
    labels.dedup();
    labels
}

/// Best-effort human-readable label for a CVC5 term (tracking constants).
pub(crate) fn cvc5_term_label(term: &cvc5::Term) -> String {
    let s = term.to_string();
    if let Some(start) = s.find(' ') {
        let rest = s[start + 1..].trim();
        if !rest.is_empty() {
            return rest.to_string();
        }
    }
    s
}
pub(crate) fn assert_cvc5_requires<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    requires: &[&SpExpr],
    var_map: &mut HashMap<String, cvc5::Term<'a>>,
    enc_state: &mut Cvc5EncoderState<'a>,
) {
    for req in requires {
        if let Some(term) = encode_expr_cvc5(tm, req, var_map, enc_state) {
            solver.assert_formula(term);
        }
    }
}

/// Assert requires with tracking labels for unsat-core extraction (#455).
///
/// Mirrors Z3's `assert_tracked` pattern: each requires is guarded by a fresh
/// boolean constant (`req_0`, `req_1`, ...) via `Implies(label, body)`.
/// Returns the tracking labels for later `check_sat_assuming`.
pub(crate) fn assert_cvc5_requires_tracked<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    requires: &[&SpExpr],
    var_map: &mut HashMap<String, cvc5::Term<'a>>,
    enc_state: &mut Cvc5EncoderState<'a>,
) -> Vec<cvc5::Term<'a>> {
    let bool_sort = tm.boolean_sort();
    let mut tracked = Vec::with_capacity(requires.len());
    for (i, req) in requires.iter().enumerate() {
        if let Some(term) = encode_expr_cvc5(tm, req, var_map, enc_state) {
            let label = format!("req_{i}");
            let track = tm.mk_const(bool_sort.clone(), &label);
            tracked.push(track.clone());
            let implication = tm.mk_term(cvc5::Kind::Implies, &[track, term]);
            solver.assert_formula(implication);
        }
    }
    tracked
}
