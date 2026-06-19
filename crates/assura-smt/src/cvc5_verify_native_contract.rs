//! CVC5 native contract verification (incremental and per-clause paths).

use assura_parser::ast::{Clause, ClauseKind, Expr};

use crate::VerificationResult;
use crate::cache::SessionCache;
use crate::cvc5_collect::collect_cvc5_var_names_from_clauses;
use crate::cvc5_native_encoder::{
    apply_havoc_assume_cvc5, default_cvc5_encoder_state, encode_expr_cvc5,
};
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

#[expect(clippy::too_many_arguments)]
pub(crate) fn verify_contract_cvc5_native(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&Expr>>>,
    constants: &[(String, i64)],
    ir_body: Option<&crate::ir::IrFunction>,
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
                ir_body,
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
    assert_cvc5_solver_prelude(
        &tm,
        &mut solver,
        &var_map,
        params,
        return_ty,
        &[],
        &narrowings,
    );

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
            ir_body,
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
