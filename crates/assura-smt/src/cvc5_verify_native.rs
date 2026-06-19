#![cfg(feature = "cvc5-verify")]
#![expect(dead_code, reason = "pub(crate) API re-exported via cvc5_backend")]

use std::collections::HashMap;

use assura_parser::ast::{BinOp, Clause, ClauseKind, Expr, Literal};

use crate::VerificationResult;
use crate::cache::SessionCache;
use crate::cvc5_collect::collect_cvc5_var_names_from_clauses;
use crate::cvc5_native_encoder::{
    apply_havoc_assume_cvc5, default_cvc5_encoder_state, encode_expr_cvc5,
};
pub(crate) use crate::cvc5_verify_native_checks::{check_satisfiability_cvc5, check_validity_cvc5};
use crate::cvc5_verify_native_clause::check_clause_cvc5_native;
use crate::cvc5_verify_native_solver::{
    Cvc5SolverOpts, assert_cvc5_axioms, assert_cvc5_axioms_since, assert_cvc5_clause_check,
    assert_cvc5_frame_axioms, assert_cvc5_requires, assert_cvc5_solver_prelude, build_cvc5_var_map,
    finish_cvc5_clause_check, inject_cvc5_lemma_assumptions_for_bodies, new_cvc5_solver,
};
use crate::cvc5_verify_shared::{
    cvc5_contract_shared_setup, cvc5_encode_failure, cvc5_lookup_cached_clause,
    cvc5_unmodelable_precheck, store_cvc5_clause_cache,
};

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
