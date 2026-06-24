//! CVC5 native solver setup, assertion, and result helpers.

#![cfg_attr(feature = "z3-verify", allow(dead_code))]

use std::collections::{HashMap, HashSet};

use assura_ast::{ClauseKind, SpExpr};

use crate::cvc5_common::{
    collect_apply_refs_from_expr, is_internal_cvc5_var, sanitize_smtlib_name,
};
use crate::cvc5_native_encoder::{Cvc5EncoderState, encode_expr_cvc5};
use crate::cvc5_verify_shared::{
    Cvc5ClauseSatOutcome, Cvc5TypeConstraint, collect_cvc5_type_constraints,
    cvc5_interpret_clause_check_result,
};
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
        let key = sanitize_smtlib_name(name);
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
            Cvc5TypeConstraint::ConstantEq(name, value) => {
                if let Some(term) = var_map.get(&name) {
                    solver.assert_formula(
                        tm.mk_term(cvc5::Kind::Equal, &[term.clone(), tm.mk_integer(value)]),
                    );
                }
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
    solver.set_option("tlimit", "2000");
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
        let current_key = sanitize_smtlib_name(var_name);
        let old_key = sanitize_smtlib_name(&format!("{var_name}__old"));
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
    // Polarity from shared clause_policy (CVC5 coarse path; Z3 handles Decreases via measure).
    if crate::clause_policy::cvc5_assert_negates_body(&kind) {
        let negated = tm.mk_term(cvc5::Kind::Not, &[body_term]);
        solver.assert_formula(negated);
    } else {
        solver.assert_formula(body_term);
    }
}

pub(crate) fn extract_cvc5_counterexample_model<'a>(
    solver: &cvc5::Solver<'a>,
    var_map: &HashMap<String, cvc5::Term<'a>>,
) -> (String, Option<CounterexampleModel>) {
    let mut variables: Vec<(String, String)> = var_map
        .iter()
        .filter(|(name, _)| !is_internal_cvc5_var(name))
        .map(|(name, term)| {
            let val = solver.get_value(term.clone());
            (name.clone(), val.to_string())
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

pub(crate) fn finish_cvc5_clause_check<'a>(
    desc: &str,
    kind: ClauseKind,
    solver: &mut cvc5::Solver<'a>,
    var_map: &HashMap<String, cvc5::Term<'a>>,
) -> VerificationResult {
    let sat_result = solver.check_sat();
    let outcome = if sat_result.is_unsat() {
        Cvc5ClauseSatOutcome::Unsat
    } else if sat_result.is_sat() {
        let (model_str, counter_model) = extract_cvc5_counterexample_model(solver, var_map);
        Cvc5ClauseSatOutcome::Sat {
            model_str,
            counter_model,
        }
    } else {
        Cvc5ClauseSatOutcome::Timeout
    };
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
    for body in bodies {
        let apply_refs = collect_apply_refs_from_expr(body);
        for lemma_name in &apply_refs {
            if let Some(ensures_bodies) = defs.get(lemma_name) {
                for ens_body in ensures_bodies {
                    if let Some(term) = encode_expr_cvc5(tm, ens_body, var_map, enc_state) {
                        solver.assert_formula(term);
                    }
                }
            }
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
