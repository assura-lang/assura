//! CVC5 native contract verification (incremental and per-clause paths).

use assura_ast::ClauseKind;

#[cfg(feature = "cvc5-verify")]
use assura_ast::SpExpr;

use crate::VerificationResult;
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
    cvc5_encode_failure, cvc5_lookup_cached_clause, cvc5_unmodelable_precheck,
    store_cvc5_clause_cache,
};
use crate::verify_context::{Cvc5ClauseVerifyInput, Cvc5ContractVerifySession};

pub(crate) fn verify_contract_cvc5_native(
    session: &mut Cvc5ContractVerifySession<'_>,
) -> Vec<VerificationResult> {
    let contract_name = session.contract.contract_name;
    let verifiable = session.prepared.verifiable.clone();
    let mut results = Vec::new();

    if verifiable.len() <= 1 {
        for clause in &verifiable {
            let desc = format!("{contract_name}::{:?}", clause.kind);
            let input = Cvc5ClauseVerifyInput {
                desc: &desc,
                body: &clause.body,
                kind: clause.kind.clone(),
            };
            results.push(check_clause_cvc5_native(&input, session));
        }
        return results;
    }

    results.extend(verify_contract_cvc5_native_incremental(session));
    results
}

fn verify_contract_cvc5_native_incremental(
    session: &mut Cvc5ContractVerifySession<'_>,
) -> Vec<VerificationResult> {
    let contract_name = session.contract.contract_name;
    let prepared = &session.prepared;
    let contract = session.contract;
    let mut results = Vec::new();

    let tm = cvc5::TermManager::new();
    let mut solver = new_cvc5_solver(
        &tm,
        Cvc5SolverOpts {
            incremental: true,
            ..Default::default()
        },
    );

    let var_names =
        collect_cvc5_var_names_from_clauses(&prepared.requires_exprs, &prepared.verifiable);
    let mut var_map = build_cvc5_var_map(&tm, &var_names, contract.constants);
    assert_cvc5_solver_prelude(
        &tm,
        &mut solver,
        &var_map,
        contract.params,
        contract.return_ty,
        &[],
        &prepared.narrowings,
    );

    let mut enc_state = default_cvc5_encoder_state();

    assert_cvc5_requires(
        &tm,
        &mut solver,
        &prepared.requires_exprs,
        &mut var_map,
        &mut enc_state,
    );

    assert_cvc5_axioms(&mut solver, &enc_state.axioms);
    let requires_axiom_count = enc_state.axioms.len();

    if let Some(defs) = session.lemma_defs {
        inject_cvc5_lemma_assumptions_for_bodies(
            &tm,
            &mut solver,
            prepared.verifiable.iter().map(|c| &c.body),
            defs,
            &mut var_map,
            &mut enc_state,
        );
        assert_cvc5_axioms_since(&mut solver, &enc_state.axioms, requires_axiom_count);
    }

    for clause in &prepared.verifiable {
        let desc = format!("{contract_name}::{:?}", clause.kind);

        let cache_key = format!("{desc}::{:?}:{:?}", clause.kind, clause.body);
        if let Some(cached_result) = cvc5_lookup_cached_clause(session.cache, &cache_key, &desc) {
            results.push(cached_result);
            continue;
        }

        if let Some(result) = cvc5_unmodelable_precheck(&desc, &clause.body) {
            results.push(result);
            continue;
        }

        solver.push(1);

        let axiom_base = enc_state.axioms.len();

        // havoc elided (no input borrow) to allow cache access for store/lookup
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

        if clause.kind == ClauseKind::Ensures && prepared.frame_checker.has_modifies() {
            let frame_vars = prepared
                .frame_checker
                .frame_axiom_vars_with_candidates(&clause.body, &prepared.param_names);
            assert_cvc5_frame_axioms(&tm, &mut solver, &var_map, &frame_vars);
        }

        assert_cvc5_clause_check(&tm, &mut solver, clause.kind.clone(), body_term);

        let result = finish_cvc5_clause_check(&desc, clause.kind.clone(), &mut solver, &var_map);
        store_cvc5_clause_cache(session.cache, cache_key, &result);

        results.push(result);

        solver.pop(1);
        enc_state.axioms.truncate(axiom_base);
    }

    results
}
