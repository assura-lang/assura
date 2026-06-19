#![cfg(feature = "cvc5-verify")]
#![expect(dead_code, reason = "pub(crate) API re-exported via cvc5_backend")]

use std::collections::{HashMap, HashSet};

use assura_parser::ast::{BinOp, Clause, ClauseKind, Expr, Literal};

use crate::cache::SessionCache;
use crate::cvc5_collect::{
    collect_cvc5_var_names, collect_cvc5_var_names_from_assumptions,
    collect_cvc5_var_names_from_clauses,
};
use crate::cvc5_common::{
    collect_apply_refs_from_expr, is_internal_cvc5_var, sanitize_smtlib_name,
};
use crate::cvc5_native_encoder::{
    Cvc5EncoderState, apply_havoc_assume_cvc5, default_cvc5_encoder_state, encode_expr_cvc5,
};
use crate::cvc5_verify_shared::{
    cvc5_contract_shared_setup, cvc5_encode_failure, cvc5_lookup_cached_clause,
    cvc5_unmodelable_precheck, store_cvc5_clause_cache,
};
use crate::{CounterexampleModel, VerificationResult};

fn build_cvc5_var_map<'a>(
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

fn assert_cvc5_solver_prelude<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    var_map: &HashMap<String, cvc5::Term<'a>>,
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    narrowings: &[(String, i64)],
) {
    let zero = tm.mk_integer(0);
    for param in params {
        if param.ty.len() == 1 && param.ty[0] == "Nat" {
            let name = sanitize_smtlib_name(&param.name);
            if let Some(term) = var_map.get(&name) {
                solver.assert_formula(tm.mk_term(cvc5::Kind::Geq, &[term.clone(), zero.clone()]));
            }
        }
    }
    if return_ty.len() == 1 && return_ty[0] == "Nat" {
        if let Some(term) = var_map.get("__result") {
            solver.assert_formula(tm.mk_term(cvc5::Kind::Geq, &[term.clone(), zero.clone()]));
        }
        if let Some(term) = var_map.get("result") {
            solver.assert_formula(tm.mk_term(cvc5::Kind::Geq, &[term.clone(), zero]));
        }
    }
    for (name, value) in narrowings {
        let key = sanitize_smtlib_name(name);
        if let Some(var) = var_map.get(&key) {
            solver
                .assert_formula(tm.mk_term(cvc5::Kind::Leq, &[var.clone(), tm.mk_integer(*value)]));
        }
    }
}

#[derive(Default)]
struct Cvc5SolverOpts {
    incremental: bool,
    unsat_core: bool,
}

fn new_cvc5_solver<'a>(tm: &'a cvc5::TermManager, opts: Cvc5SolverOpts) -> cvc5::Solver<'a> {
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

fn assert_cvc5_axioms<'a>(solver: &mut cvc5::Solver<'a>, axioms: &[cvc5::Term<'a>]) {
    for axiom in axioms {
        solver.assert_formula(axiom.clone());
    }
}

fn assert_cvc5_axioms_since<'a>(
    solver: &mut cvc5::Solver<'a>,
    axioms: &[cvc5::Term<'a>],
    start: usize,
) {
    for axiom in &axioms[start..] {
        solver.assert_formula(axiom.clone());
    }
}

fn assert_cvc5_frame_axioms<'a>(
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

fn assert_cvc5_clause_check<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    kind: ClauseKind,
    body_term: cvc5::Term<'a>,
) {
    match kind {
        ClauseKind::Invariant | ClauseKind::MustNot => solver.assert_formula(body_term),
        _ => {
            let negated = tm.mk_term(cvc5::Kind::Not, &[body_term]);
            solver.assert_formula(negated);
        }
    }
}

fn extract_cvc5_counterexample_model<'a>(
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

fn finish_cvc5_clause_check<'a>(
    desc: &str,
    kind: ClauseKind,
    solver: &mut cvc5::Solver<'a>,
    var_map: &HashMap<String, cvc5::Term<'a>>,
) -> VerificationResult {
    let sat_result = solver.check_sat();
    if sat_result.is_unsat() {
        if matches!(kind, ClauseKind::Invariant) {
            VerificationResult::Counterexample {
                clause_desc: desc.to_string(),
                model: "invariant is unsatisfiable".to_string(),
                counter_model: None,
            }
        } else {
            VerificationResult::verified(desc.to_string())
        }
    } else if sat_result.is_sat() {
        if matches!(kind, ClauseKind::Invariant) {
            VerificationResult::verified(desc.to_string())
        } else {
            let (model_str, counter_model) = extract_cvc5_counterexample_model(solver, var_map);
            VerificationResult::Counterexample {
                clause_desc: desc.to_string(),
                model: model_str,
                counter_model,
            }
        }
    } else {
        VerificationResult::Timeout {
            clause_desc: desc.to_string(),
        }
    }
}

fn inject_cvc5_lemma_assumptions<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    body: &'a Expr,
    defs: &std::collections::HashMap<String, Vec<&Expr>>,
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

fn inject_cvc5_lemma_assumptions_for_bodies<'a, I>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    bodies: I,
    defs: &std::collections::HashMap<String, Vec<&Expr>>,
    var_map: &mut HashMap<String, cvc5::Term<'a>>,
    enc_state: &mut Cvc5EncoderState<'a>,
) where
    I: IntoIterator<Item = &'a Expr>,
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

pub(crate) fn verify_contract_cvc5_native(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&Expr>>>,
    constants: &[(String, i64)],
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    let (narrowings, requires_exprs, frame_checker) =
        cvc5_contract_shared_setup(clauses, constants);

    // Collect verifiable clauses
    let verifiable: Vec<&assura_parser::ast::Clause> = clauses
        .iter()
        .filter(|c| {
            matches!(
                c.kind,
                ClauseKind::Ensures
                    | ClauseKind::Invariant
                    | ClauseKind::Rule
                    | ClauseKind::MustNot
                    | ClauseKind::Decreases
            )
        })
        .collect();

    // Process feature-specific Other clauses
    for clause in clauses {
        if let ClauseKind::Other(kind) = &clause.kind {
            let feature_results = crate::smt_features::verify_feature_clause(
                kind,
                contract_name,
                &clause.body,
                clauses,
            );
            results.extend(feature_results);
        }
    }

    let requires_clauses: Vec<&Clause> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .collect();
    let ensures_clauses: Vec<&Clause> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .collect();
    let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();

    // For 0 or 1 verifiable clauses, fall back to per-clause solver
    // (incremental push/pop has no benefit with a single query).
    if verifiable.len() <= 1 {
        for clause in &verifiable {
            let desc = format!("{contract_name}::{:?}", clause.kind);
            let result = check_clause_cvc5_native(
                &desc,
                &requires_exprs,
                &requires_clauses,
                &ensures_clauses,
                &clause.body,
                clause.kind.clone(),
                params,
                return_ty,
                &param_names,
                None,
                constants,
                &narrowings,
                &frame_checker,
                lemma_defs,
                cache,
            );
            results.push(result);
        }
        return results;
    }

    // ---------------------------------------------------------------
    // Incremental solving: create ONE solver, assert shared requires
    // ONCE, then use push/pop for each clause (#264).
    // ---------------------------------------------------------------

    let tm = cvc5::TermManager::new();
    let mut solver = new_cvc5_solver(
        &tm,
        Cvc5SolverOpts {
            incremental: true,
            ..Default::default()
        },
    );

    let var_names = collect_cvc5_var_names_from_clauses(&requires_exprs, &verifiable);
    let mut var_map = build_cvc5_var_map(&tm, &var_names, constants);
    assert_cvc5_solver_prelude(&tm, &mut solver, &var_map, params, return_ty, &narrowings);

    let mut enc_state = default_cvc5_encoder_state();

    assert_cvc5_requires(
        &tm,
        &mut solver,
        &requires_exprs,
        &mut var_map,
        &mut enc_state,
    );

    assert_cvc5_axioms(&mut solver, &enc_state.axioms);
    let requires_axiom_count = enc_state.axioms.len();

    if let Some(defs) = lemma_defs {
        inject_cvc5_lemma_assumptions_for_bodies(
            &tm,
            &mut solver,
            verifiable.iter().map(|c| &c.body),
            defs,
            &mut var_map,
            &mut enc_state,
        );
        assert_cvc5_axioms_since(&mut solver, &enc_state.axioms, requires_axiom_count);
    }

    // For each verifiable clause: push, encode, check, pop
    for clause in &verifiable {
        let desc = format!("{contract_name}::{:?}", clause.kind);

        let cache_key = format!("{desc}::{:?}:{:?}", clause.kind, clause.body);
        if let Some(cached_result) = cvc5_lookup_cached_clause(cache, &cache_key, &desc) {
            results.push(cached_result);
            continue;
        }

        if let Some(result) = cvc5_unmodelable_precheck(&desc, &clause.body) {
            results.push(result);
            continue;
        }

        solver.push(1); // Save solver state

        // Track axiom count before havoc+assume and clause encoding
        let axiom_base = enc_state.axioms.len();

        apply_havoc_assume_cvc5(
            &tm,
            &requires_clauses,
            &ensures_clauses,
            return_ty,
            &param_names,
            None,
            &mut var_map,
            &mut enc_state,
        );
        assert_cvc5_axioms_since(&mut solver, &enc_state.axioms, axiom_base);
        let havoc_axiom_end = enc_state.axioms.len();

        let body_term = match encode_expr_cvc5(&tm, &clause.body, &mut var_map, &mut enc_state) {
            Some(t) => t,
            None => {
                solver.pop(1);
                enc_state.axioms.truncate(axiom_base);
                results.push(cvc5_encode_failure(&desc));
                continue;
            }
        };

        assert_cvc5_axioms_since(&mut solver, &enc_state.axioms, havoc_axiom_end);

        if clause.kind == ClauseKind::Ensures && frame_checker.has_modifies() {
            let frame_vars = frame_checker.frame_axiom_vars(&clause.body);
            assert_cvc5_frame_axioms(&tm, &mut solver, &var_map, &frame_vars);
        }

        assert_cvc5_clause_check(&tm, &mut solver, clause.kind.clone(), body_term);

        let result = finish_cvc5_clause_check(&desc, clause.kind.clone(), &mut solver, &var_map);
        store_cvc5_clause_cache(cache, cache_key, &result);

        results.push(result);

        solver.pop(1); // Restore solver state

        // Truncate havoc+assume and clause-specific axioms (removed from
        // the solver by pop).
        enc_state.axioms.truncate(axiom_base);
    }

    results
}

#[expect(clippy::too_many_arguments)]
fn check_clause_cvc5_native(
    desc: &str,
    requires: &[&Expr],
    requires_clauses: &[&Clause],
    ensures_clauses: &[&Clause],
    ensures_body: &Expr,
    kind: ClauseKind,
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    param_names: &[String],
    ir_body: Option<&crate::ir::IrFunction>,
    constants: &[(String, i64)],
    narrowings: &[(String, i64)],
    frame_checker: &assura_types::FrameChecker,
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&Expr>>>,
    cache: &mut SessionCache,
) -> VerificationResult {
    let cache_key = format!("{desc}::{kind:?}:{ensures_body:?}");
    if let Some(result) = cvc5_lookup_cached_clause(cache, &cache_key, desc) {
        return result;
    }

    if let Some(result) = cvc5_unmodelable_precheck(desc, ensures_body) {
        return result;
    }

    let tm = cvc5::TermManager::new();
    let mut solver = new_cvc5_solver(&tm, Cvc5SolverOpts::default());

    let var_names = collect_cvc5_var_names(requires, ensures_body);
    let mut var_map = build_cvc5_var_map(&tm, &var_names, constants);
    assert_cvc5_solver_prelude(&tm, &mut solver, &var_map, params, return_ty, narrowings);

    let mut enc_state = default_cvc5_encoder_state();

    apply_havoc_assume_cvc5(
        &tm,
        requires_clauses,
        ensures_clauses,
        return_ty,
        param_names,
        ir_body,
        &mut var_map,
        &mut enc_state,
    );

    assert_cvc5_requires(&tm, &mut solver, requires, &mut var_map, &mut enc_state);

    if let Some(defs) = lemma_defs {
        inject_cvc5_lemma_assumptions(
            &tm,
            &mut solver,
            ensures_body,
            defs,
            &mut var_map,
            &mut enc_state,
        );
    }

    let body_term = match encode_expr_cvc5(&tm, ensures_body, &mut var_map, &mut enc_state) {
        Some(t) => t,
        None => return cvc5_encode_failure(desc),
    };

    assert_cvc5_axioms(&mut solver, &enc_state.axioms);

    if kind == ClauseKind::Ensures && frame_checker.has_modifies() {
        let frame_vars = frame_checker.frame_axiom_vars(ensures_body);
        assert_cvc5_frame_axioms(&tm, &mut solver, &var_map, &frame_vars);
    }

    assert_cvc5_clause_check(&tm, &mut solver, kind.clone(), body_term);

    let result = finish_cvc5_clause_check(desc, kind, &mut solver, &var_map);
    store_cvc5_clause_cache(cache, cache_key, &result);

    result
}

fn extract_cvc5_unsat_core_labels(solver: &cvc5::Solver, tracked: &[cvc5::Term]) -> Vec<String> {
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
fn cvc5_term_label(term: &cvc5::Term) -> String {
    let s = term.to_string();
    if let Some(start) = s.find(' ') {
        let rest = s[start + 1..].trim();
        if !rest.is_empty() {
            return rest.to_string();
        }
    }
    s
}

pub(crate) fn check_validity_cvc5(
    desc: &str,
    assumptions: &[&Expr],
    body: &Expr,
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
    if sat_result.is_unsat() {
        let core = extract_cvc5_unsat_core_labels(&solver, &tracked_assumptions);
        VerificationResult::verified_with_core(desc.to_string(), core)
    } else if sat_result.is_sat() {
        let (model_str, counter_model) = extract_cvc5_counterexample_model(&solver, &var_map);
        VerificationResult::Counterexample {
            clause_desc: desc.to_string(),
            model: model_str,
            counter_model,
        }
    } else {
        VerificationResult::Timeout {
            clause_desc: desc.to_string(),
        }
    }
}

/// Check satisfiability of `body` under `assumptions` using CVC5.
///
/// For invariants: assert all assumptions + body, check-sat.
/// SAT = invariant is satisfiable (Verified), UNSAT = unsatisfiable (Counterexample).
pub(crate) fn check_satisfiability_cvc5(
    desc: &str,
    assumptions: &[&Expr],
    body: &Expr,
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
    if sat_result.is_sat() {
        VerificationResult::verified(desc.to_string())
    } else if sat_result.is_unsat() {
        VerificationResult::Counterexample {
            clause_desc: desc.to_string(),
            model: "invariant is unsatisfiable".to_string(),
            counter_model: None,
        }
    } else {
        VerificationResult::Timeout {
            clause_desc: desc.to_string(),
        }
    }
}

/// CVC5 implementation of refinement subtype check.
///
/// `{v: T | antecedent} <: {v: T | consequent}`
/// Encodes: (assert antecedent) (assert (not consequent)) (check-sat)
pub(crate) fn check_refinement_subtype_cvc5(
    antecedent: &Expr,
    consequent: &Expr,
) -> VerificationResult {
    check_validity_cvc5("refinement_subtype", &[antecedent], consequent)
}

/// CVC5 implementation of refinement subtype check with extra context.
pub(crate) fn check_refinement_subtype_with_context_cvc5(
    context: &[Expr],
    antecedent: &Expr,
    consequent: &Expr,
) -> VerificationResult {
    let mut assumptions: Vec<&Expr> = context.iter().collect();
    assumptions.push(antecedent);
    check_validity_cvc5("refinement_subtype_ctx", &assumptions, consequent)
}

/// CVC5 implementation of buffer bounds verification.
pub(crate) fn verify_buffer_bounds_cvc5(requires: &[Expr], ensures: &Expr) -> VerificationResult {
    let assumptions: Vec<&Expr> = requires.iter().collect();
    check_validity_cvc5("buffer_bounds", &assumptions, ensures)
}

/// CVC5 implementation of region containment verification.
pub(crate) fn verify_region_containment_cvc5(
    context: &[Expr],
    sub_lo: &Expr,
    sub_hi: &Expr,
    parent_lo: &Expr,
    parent_hi: &Expr,
) -> VerificationResult {
    // Build: forall i: sub_lo <= i < sub_hi => parent_lo <= i < parent_hi
    // Encode as two validity checks:
    // 1. context => sub_lo >= parent_lo
    // 2. context => sub_hi <= parent_hi
    let lo_check = Expr::BinOp {
        op: BinOp::Gte,
        lhs: Box::new(sub_lo.clone()),
        rhs: Box::new(parent_lo.clone()),
    };
    let hi_check = Expr::BinOp {
        op: BinOp::Lte,
        lhs: Box::new(sub_hi.clone()),
        rhs: Box::new(parent_hi.clone()),
    };
    let combined = Expr::BinOp {
        op: BinOp::And,
        lhs: Box::new(lo_check),
        rhs: Box::new(hi_check),
    };
    let assumptions: Vec<&Expr> = context.iter().collect();
    check_validity_cvc5("region_containment", &assumptions, &combined)
}

/// CVC5 implementation of measure-aware verification.
pub(crate) fn verify_with_measures_cvc5(
    requires: &[Expr],
    ensures: &Expr,
    _measures: &[crate::measures::MeasureDefinition],
) -> VerificationResult {
    // Measures are encoded as uninterpreted functions with axioms.
    // For CVC5, we encode as plain validity check (measure axioms
    // would need to be threaded through the encoder state).
    let assumptions: Vec<&Expr> = requires.iter().collect();
    check_validity_cvc5("verify_with_measures", &assumptions, ensures)
}

/// CVC5 implementation of decrease verification.
pub(crate) fn verify_decrease_cvc5(
    preconditions: &[Expr],
    measure_expr: &Expr,
    call_arg_expr: &Expr,
    clause_desc: String,
) -> VerificationResult {
    // Check: preconditions => measure(call_args) < measure(fn_args) && measure(call_args) >= 0
    let decrease_check = Expr::BinOp {
        op: BinOp::Lt,
        lhs: Box::new(call_arg_expr.clone()),
        rhs: Box::new(measure_expr.clone()),
    };
    let non_neg = Expr::BinOp {
        op: BinOp::Gte,
        lhs: Box::new(call_arg_expr.clone()),
        rhs: Box::new(Expr::Literal(Literal::Int("0".to_string()))),
    };
    let combined = Expr::BinOp {
        op: BinOp::And,
        lhs: Box::new(decrease_check),
        rhs: Box::new(non_neg),
    };
    let assumptions: Vec<&Expr> = preconditions.iter().collect();
    check_validity_cvc5(&clause_desc, &assumptions, &combined)
}

/// CVC5 implementation of taint safety verification.
pub(crate) fn verify_taint_safety_cvc5(
    taint_labels: &[(String, assura_types::TaintLabel)],
    _validation_fns: &[String],
    sensitive_uses: &[(String, assura_types::TaintLabel)],
) -> VerificationResult {
    use assura_types::TaintLabel;

    let tm = cvc5::TermManager::new();
    let mut solver = new_cvc5_solver(&tm, Cvc5SolverOpts::default());

    let mut var_map: HashMap<String, cvc5::Term> = HashMap::new();
    let zero = tm.mk_integer(0);
    let one = tm.mk_integer(1);
    let two = tm.mk_integer(2);

    // Create taint level variables
    for (name, label) in taint_labels {
        let level = match label {
            TaintLabel::Untrusted => zero.clone(),
            TaintLabel::Validated => one.clone(),
            TaintLabel::Trusted => two.clone(),
        };
        var_map.insert(name.clone(), level);
    }

    // Check sensitive uses: each must have taint level >= required
    for (name, required_label) in sensitive_uses {
        let required_level = match required_label {
            TaintLabel::Untrusted => zero.clone(),
            TaintLabel::Validated => one.clone(),
            TaintLabel::Trusted => two.clone(),
        };
        if let Some(actual) = var_map.get(name) {
            let check = tm.mk_term(cvc5::Kind::Geq, &[actual.clone(), required_level]);
            let neg = tm.mk_term(cvc5::Kind::Not, &[check]);
            // If the negation is satisfiable, the taint check fails
            solver.push(1);
            solver.assert_formula(neg);
            let result = solver.check_sat();
            solver.pop(1);
            if result.is_sat() {
                return VerificationResult::Counterexample {
                    clause_desc: "taint_safety".to_string(),
                    model: format!("{name} has insufficient taint level"),
                    counter_model: None,
                };
            }
        }
    }

    VerificationResult::verified("taint_safety".to_string())
}

/// CVC5 implementation of feature clause body verification.
///
/// Used by `smt_features::verify_feature_body` when the CVC5 solver is
/// selected. Collects sibling requires as assumptions, checks body validity.
pub(crate) fn verify_feature_body_cvc5(
    parent_name: &str,
    feature_label: &str,
    body: &Expr,
    sibling_clauses: &[Clause],
) -> VerificationResult {
    let desc = format!("{parent_name}: {feature_label}");

    // Skip declarative feature clauses (bare uppercase ident)
    if matches!(body, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase())) {
        return VerificationResult::Unknown {
            clause_desc: desc,
            reason: format!("{feature_label} not yet encoded in SMT"),
        };
    }

    let requires: Vec<&Expr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();

    check_validity_cvc5(&desc, &requires, body)
}

/// CVC5 implementation of structural invariant inductive checking.
pub(crate) fn verify_structural_invariant_inductive_cvc5(
    parent_name: &str,
    body: &Expr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    // Skip bare uppercase ident
    if matches!(body, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase())) {
        results.push(VerificationResult::Unknown {
            clause_desc: format!("{parent_name}: structural_invariant"),
            reason: "structural_invariant not yet encoded in SMT".into(),
        });
        return results;
    }

    // Step 1: Establishment (requires => invariant)
    let requires: Vec<&Expr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let desc1 = format!("{parent_name}: structural_invariant (establishment)");
    results.push(check_validity_cvc5(&desc1, &requires, body));

    // Step 2: Preservation (requires + ensures => invariant)
    let mut assumptions: Vec<&Expr> = requires;
    let ensures: Vec<&Expr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .map(|c| &c.body)
        .collect();
    assumptions.extend(ensures);
    let desc2 = format!("{parent_name}: structural_invariant (preservation)");
    results.push(check_validity_cvc5(&desc2, &assumptions, body));

    results
}

fn assert_cvc5_requires<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    requires: &[&Expr],
    var_map: &mut HashMap<String, cvc5::Term<'a>>,
    enc_state: &mut Cvc5EncoderState<'a>,
) {
    for req in requires {
        if let Some(term) = encode_expr_cvc5(tm, req, var_map, enc_state) {
            solver.assert_formula(term);
        }
    }
}
