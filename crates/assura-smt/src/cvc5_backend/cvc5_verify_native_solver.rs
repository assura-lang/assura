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

pub(crate) fn build_cvc5_var_map(
    tm: &mut cvc5::TermManager,
    var_names: &HashSet<String>,
    constants: &[(String, i64)],
) -> HashMap<String, cvc5::Term> {
    let mut var_map = HashMap::new();
    for name in var_names {
        var_map.insert(
            name.clone(),
            {
                let __cvc3 = tm.integer_sort();
                tm.mk_const(__cvc3, name)
            }
            .expect("cvc5"),
        );
    }
    for (name, value) in constants {
        let key = sanitize_smt_name(name);
        var_map.insert(key, tm.mk_integer(*value));
    }
    var_map
}

/// Register fixed-width parameters as bitvector variables (#453).
///
/// Replaces `integer_sort()` entries in `var_map` with `mk_bv_sort(width)` entries
/// and records signedness in `enc_state.bv_signed`.
pub(crate) fn register_cvc5_fixed_width_params(
    tm: &mut cvc5::TermManager,
    params: &[assura_ast::Param],
    var_map: &mut HashMap<String, cvc5::Term>,
    enc_state: &mut crate::cvc5_encoder_state::Cvc5EncoderState,
) {
    for param in params {
        let pt = crate::prelude_policy::param_type_tokens(param);
        if let Some((width, signed)) = crate::prelude_policy::fixed_width_bits(&pt) {
            let bv = super::cvc5_bitvector_encode::bv_const(tm, &param.name, width);
            var_map.insert(param.name.clone(), bv);
            enc_state.bv_signed.insert(param.name.clone(), signed);
        }
    }
}

/// Register `result` / `__result` when the contract return type is fixed-width (#851).
pub(crate) fn register_cvc5_fixed_width_return(
    tm: &mut cvc5::TermManager,
    return_ty: &[String],
    var_map: &mut HashMap<String, cvc5::Term>,
    enc_state: &mut crate::cvc5_encoder_state::Cvc5EncoderState,
) {
    if let Some((width, signed)) = crate::prelude_policy::fixed_width_bits(return_ty) {
        for name in ["result", crate::encode_atom_policy::RESULT_VAR_NAME] {
            let bv = super::cvc5_bitvector_encode::bv_const(tm, name, width);
            var_map.insert(name.to_string(), bv);
            enc_state.bv_signed.insert(name.to_string(), signed);
        }
    }
}

pub(crate) fn assert_cvc5_solver_prelude(
    tm: &mut cvc5::TermManager,
    solver: &mut cvc5::Solver,
    var_map: &HashMap<String, cvc5::Term>,
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
                    let ge = tm
                        .mk_term(cvc5::Kind::Geq, &[term.clone(), zero.clone()])
                        .expect("cvc5");
                    solver.assert_formula(ge).expect("cvc5");
                    let max = tm
                        .mk_integer_from_str("18446744073709551615")
                        .expect("cvc5");
                    let le = tm
                        .mk_term(cvc5::Kind::Leq, &[term.clone(), max])
                        .expect("cvc5");
                    solver.assert_formula(le).expect("cvc5");
                }
            }
            Cvc5TypeConstraint::IntBounded(name) => {
                if let Some(term) = var_map.get(&name) {
                    let lo = tm.mk_integer(i64::MIN);
                    let hi = tm.mk_integer(i64::MAX);
                    let ge = tm
                        .mk_term(cvc5::Kind::Geq, &[term.clone(), lo])
                        .expect("cvc5");
                    solver.assert_formula(ge).expect("cvc5");
                    let le = tm
                        .mk_term(cvc5::Kind::Leq, &[term.clone(), hi])
                        .expect("cvc5");
                    solver.assert_formula(le).expect("cvc5");
                }
            }
            Cvc5TypeConstraint::BoolZeroOrOne(name) => {
                if let Some(term) = var_map.get(&name) {
                    let one = tm.mk_integer(1);
                    let ge = tm
                        .mk_term(cvc5::Kind::Geq, &[term.clone(), zero.clone()])
                        .expect("cvc5");
                    solver.assert_formula(ge).expect("cvc5");
                    let le = tm
                        .mk_term(cvc5::Kind::Leq, &[term.clone(), one])
                        .expect("cvc5");
                    solver.assert_formula(le).expect("cvc5");
                }
            }
            Cvc5TypeConstraint::ConstantEq(..) => {
                // Skip: build_cvc5_var_map already inserts mk_integer(value)
                // for constants, so asserting value == value is redundant (#467).
            }
            Cvc5TypeConstraint::NarrowingLe(name, value) => {
                if let Some(var) = var_map.get(&name) {
                    let bound = tm.mk_integer(value);
                    let le = tm
                        .mk_term(cvc5::Kind::Leq, &[var.clone(), bound])
                        .expect("cvc5");
                    solver.assert_formula(le).expect("cvc5");
                }
            }
        }
    }
}

pub(crate) struct Cvc5SolverOpts {
    pub(crate) incremental: bool,
    pub(crate) unsat_core: bool,
    /// Per-clause wall-clock budget in ms (CVC5 `tlimit` option).
    ///
    /// Defaults to [`crate::encode_timeout_policy::DEFAULT_SOLVER_TIMEOUT_MS`].
    /// File-level verify passes the value from [`clause_timeout_ms`].
    pub(crate) tlimit_ms: u32,
}

impl Default for Cvc5SolverOpts {
    fn default() -> Self {
        Self {
            incremental: false,
            unsat_core: false,
            tlimit_ms: crate::encode_timeout_policy::DEFAULT_SOLVER_TIMEOUT_MS,
        }
    }
}

pub(crate) fn new_cvc5_solver(tm: &mut cvc5::TermManager, opts: Cvc5SolverOpts) -> cvc5::Solver {
    let mut solver = cvc5::Solver::new(tm);
    solver.set_logic("ALL").expect("cvc5");
    solver.set_option("produce-models", "true").expect("cvc5");
    // Re-resolve so short values still hit the shared floor (idempotent for
    // already-floored session budgets).
    let tlimit = crate::encode_timeout_policy::clause_timeout_tlimit(opts.tlimit_ms as u64);
    solver.set_option("tlimit", &tlimit).expect("cvc5");
    if opts.incremental {
        solver.set_option("incremental", "true").expect("cvc5");
    }
    if opts.unsat_core {
        solver
            .set_option("produce-unsat-cores", "true")
            .expect("cvc5");
        solver
            .set_option("produce-unsat-assumptions", "true")
            .expect("cvc5");
    }
    solver
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode_timeout_policy::{DEFAULT_SOLVER_TIMEOUT_MS, clause_timeout_ms};

    #[test]
    fn default_opts_use_clause_timeout_floor() {
        let opts = Cvc5SolverOpts::default();
        assert_eq!(opts.tlimit_ms, DEFAULT_SOLVER_TIMEOUT_MS);
        assert!(!opts.incremental);
        assert!(!opts.unsat_core);
    }

    #[test]
    fn opts_honor_resolved_longer_timeout() {
        let opts = Cvc5SolverOpts {
            tlimit_ms: clause_timeout_ms(45_000),
            ..Default::default()
        };
        assert_eq!(opts.tlimit_ms, 45_000);
        assert_ne!(opts.tlimit_ms, DEFAULT_SOLVER_TIMEOUT_MS);
    }
}

pub(crate) fn assert_cvc5_axioms(solver: &mut cvc5::Solver, axioms: &[cvc5::Term]) {
    for axiom in axioms {
        solver.assert_formula(axiom.clone()).expect("cvc5");
    }
}

pub(crate) fn assert_cvc5_axioms_since(
    solver: &mut cvc5::Solver,
    axioms: &[cvc5::Term],
    start: usize,
) {
    for axiom in &axioms[start..] {
        solver.assert_formula(axiom.clone()).expect("cvc5");
    }
}

pub(crate) fn assert_cvc5_frame_axioms(
    tm: &mut cvc5::TermManager,
    solver: &mut cvc5::Solver,
    var_map: &HashMap<String, cvc5::Term>,
    frame_vars: &[String],
) {
    for var_name in frame_vars {
        let current_key = sanitize_smt_name(var_name);
        let old_key = crate::encode_atom_policy::old_snapshot_name(var_name);
        // A missing current is not an Int we can invent. An old snapshot
        // must use the current term's sort; an Int stand-in corrupts CVC5
        // when the parameter is a bitvector or real.
        let Some(current) = var_map.get(&current_key).cloned() else {
            continue;
        };
        let old_var = var_map
            .get(&old_key)
            .cloned()
            .unwrap_or_else(|| tm.mk_const(current.sort(), &old_key).expect("cvc5"));
        let eq = tm
            .mk_term(cvc5::Kind::Equal, &[current, old_var])
            .expect("cvc5");
        solver.assert_formula(eq).expect("cvc5");
    }
}

pub(crate) fn assert_cvc5_clause_check(
    tm: &mut cvc5::TermManager,
    solver: &mut cvc5::Solver,
    kind: ClauseKind,
    body_term: cvc5::Term,
) {
    use crate::clause_policy::{ClauseCheckPolarity, clause_check_polarity};

    match clause_check_polarity(&kind) {
        Some(ClauseCheckPolarity::ValidityNegateBody) => {
            let negated = tm.mk_term(cvc5::Kind::Not, &[body_term]).expect("cvc5");
            solver.assert_formula(negated).expect("cvc5");
        }
        Some(ClauseCheckPolarity::SatisfiabilityAssertBody)
        | Some(ClauseCheckPolarity::ValidityAssertBody) => {
            solver.assert_formula(body_term).expect("cvc5");
        }
        Some(ClauseCheckPolarity::DecreasesNonNeg) => {
            // Parity with Z3: extract measure as integer, assert NOT(measure >= 0).
            // UNSAT means the measure is always non-negative (verified).
            let zero = tm.mk_integer(0);
            let measure_int = if body_term.sort().is_boolean() {
                {
                    let __cvc1 = tm.mk_integer(1);
                    let __cvc0 = tm.mk_integer(0);
                    tm.mk_term(cvc5::Kind::Ite, &[body_term, __cvc1, __cvc0])
                }
                .expect("cvc5")
            } else {
                body_term
            };
            let non_neg = tm
                .mk_term(cvc5::Kind::Geq, &[measure_int, zero])
                .expect("cvc5");
            let negated = tm.mk_term(cvc5::Kind::Not, &[non_neg]).expect("cvc5");
            solver.assert_formula(negated).expect("cvc5");
        }
        None => {}
    }
}

pub(crate) fn extract_cvc5_counterexample_model(
    solver: &cvc5::Solver,
    var_map: &HashMap<String, cvc5::Term>,
) -> (String, Option<CounterexampleModel>) {
    let mut variables: Vec<(String, String)> = var_map
        .iter()
        .filter_map(|(name, term)| {
            let val = solver.get_value(term.clone()).expect("cvc5").to_string();
            if crate::encode_atom_policy::is_requires_track_noise(name, &val) {
                return None;
            }
            if !crate::encode_atom_policy::is_counterexample_user_var(name) {
                return None;
            }
            let clean = crate::encode_atom_policy::counterexample_display_name(name).to_string();
            Some((clean, val))
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
pub(crate) fn cvc5_clause_sat_outcome(
    sat_result: &cvc5::SmtResult,
    solver: &cvc5::Solver,
    var_map: &HashMap<String, cvc5::Term>,
    tracked_assumptions: &[cvc5::Term],
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

pub(crate) fn finish_cvc5_clause_check(
    desc: &str,
    kind: ClauseKind,
    solver: &mut cvc5::Solver,
    var_map: &HashMap<String, cvc5::Term>,
    tracked_labels: &[cvc5::Term],
) -> VerificationResult {
    let sat_result = if tracked_labels.is_empty() {
        solver.check_sat().expect("cvc5")
    } else {
        solver.check_sat_assuming(tracked_labels).expect("cvc5")
    };
    let outcome = cvc5_clause_sat_outcome(&sat_result, solver, var_map, tracked_labels);
    cvc5_interpret_clause_check_result(desc, kind, outcome)
}

pub(crate) fn inject_cvc5_lemma_assumptions(
    tm: &mut cvc5::TermManager,
    solver: &mut cvc5::Solver,
    body: &SpExpr,
    defs: &std::collections::HashMap<String, Vec<&SpExpr>>,
    var_map: &mut HashMap<String, cvc5::Term>,
    enc_state: &mut Cvc5EncoderState,
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
    tm: &mut cvc5::TermManager,
    solver: &mut cvc5::Solver,
    bodies: I,
    defs: &std::collections::HashMap<String, Vec<&SpExpr>>,
    var_map: &mut HashMap<String, cvc5::Term>,
    enc_state: &mut Cvc5EncoderState,
) where
    I: IntoIterator<Item = &'a SpExpr>,
{
    let bodies_vec: Vec<&SpExpr> = bodies.into_iter().collect();
    for ens_body in crate::lemma_inject_policy::lemma_ensures_bodies_for_exprs(bodies_vec, defs) {
        if let Some(term) = encode_expr_cvc5(tm, ens_body, var_map, enc_state) {
            solver.assert_formula(term).expect("cvc5");
        }
    }
}
pub(crate) fn extract_cvc5_unsat_core_labels(
    solver: &cvc5::Solver,
    tracked: &[cvc5::Term],
) -> Vec<String> {
    let mut labels: Vec<String> = solver
        .get_unsat_assumptions()
        .expect("cvc5")
        .iter()
        .map(cvc5_term_label)
        .collect();
    if labels.is_empty() {
        labels = solver
            .get_unsat_core()
            .expect("cvc5")
            .iter()
            .map(cvc5_term_label)
            .collect();
    }
    if labels.is_empty() && !tracked.is_empty() {
        labels = tracked.iter().map(cvc5_term_label).collect();
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
pub(crate) fn assert_cvc5_requires(
    tm: &mut cvc5::TermManager,
    solver: &mut cvc5::Solver,
    requires: &[&SpExpr],
    var_map: &mut HashMap<String, cvc5::Term>,
    enc_state: &mut Cvc5EncoderState,
) {
    for req in requires {
        if let Some(term) = encode_expr_cvc5(tm, req, var_map, enc_state) {
            solver.assert_formula(term).expect("cvc5");
        }
    }
}

/// Assert requires with tracking labels for unsat-core extraction (#455).
///
/// Mirrors Z3's `assert_tracked` pattern: each requires is guarded by a fresh
/// boolean constant (`req_0`, `req_1`, ...) via `Implies(label, body)`.
/// Returns the tracking labels for later `check_sat_assuming`.
pub(crate) fn assert_cvc5_requires_tracked(
    tm: &mut cvc5::TermManager,
    solver: &mut cvc5::Solver,
    requires: &[&SpExpr],
    var_map: &mut HashMap<String, cvc5::Term>,
    enc_state: &mut Cvc5EncoderState,
) -> Vec<cvc5::Term> {
    let bool_sort = tm.boolean_sort();
    let mut tracked = Vec::with_capacity(requires.len());
    for (i, req) in requires.iter().enumerate() {
        if let Some(term) = encode_expr_cvc5(tm, req, var_map, enc_state) {
            let label = format!(
                "{}{i}",
                crate::encode_atom_policy::REQUIRES_TRACK_LABEL_PREFIX
            );
            let track = tm.mk_const(bool_sort.clone(), &label).expect("cvc5");
            tracked.push(track.clone());
            let implication = tm
                .mk_term(cvc5::Kind::Implies, &[track, term])
                .expect("cvc5");
            solver.assert_formula(implication).expect("cvc5");
        }
    }
    tracked
}
