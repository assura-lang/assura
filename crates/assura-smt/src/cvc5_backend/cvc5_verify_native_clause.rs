//! Per-clause CVC5 native verification (non-incremental path).

use assura_ast::ClauseKind;

#[cfg(feature = "cvc5-verify")]
use assura_ast::SpExpr;

use crate::VerificationResult;
use crate::cvc5_collect::collect_cvc5_var_names;
use crate::cvc5_encoder_state::seed_cvc5_trigger_manager_from_clauses;
use crate::cvc5_native_encoder::{
    apply_havoc_assume_cvc5, default_cvc5_encoder_state, encode_expr_cvc5,
};
use crate::cvc5_verify_native_solver::{
    Cvc5SolverOpts, assert_cvc5_axioms, assert_cvc5_clause_check, assert_cvc5_frame_axioms,
    assert_cvc5_requires, assert_cvc5_requires_tracked, assert_cvc5_solver_prelude,
    build_cvc5_var_map, finish_cvc5_clause_check, inject_cvc5_lemma_assumptions, new_cvc5_solver,
    register_cvc5_fixed_width_params, register_cvc5_fixed_width_return,
};
use crate::cvc5_verify_shared::{
    cvc5_clause_cache_key, cvc5_encode_failure, cvc5_lookup_cached_clause,
    cvc5_unmodelable_precheck, store_cvc5_clause_cache,
};
use crate::verify_context::{Cvc5ClauseVerifyInput, Cvc5ContractVerifySession};

pub(crate) fn check_clause_cvc5_native(
    input: &Cvc5ClauseVerifyInput<'_>,
    session: &mut Cvc5ContractVerifySession<'_>,
) -> VerificationResult {
    let desc = input.desc;
    let ensures_body = input.body;
    let kind = input.kind.clone();
    let prepared = &session.prepared;
    let contract = session.contract;

    let cache_key = cvc5_clause_cache_key(desc, &kind, ensures_body);
    if let Some(result) = cvc5_lookup_cached_clause(session.cache, &cache_key, desc) {
        return result;
    }

    if let Some(result) = cvc5_unmodelable_precheck(desc, ensures_body) {
        return result;
    }

    let use_cores =
        crate::prelude_policy::track_requires_unsat_cores(prepared.requires_exprs.len());
    let result = {
        let tm = cvc5::TermManager::new();
        let mut solver = new_cvc5_solver(
            &tm,
            Cvc5SolverOpts {
                unsat_core: use_cores,
                ..Default::default()
            },
        );

        let var_names = collect_cvc5_var_names(&prepared.requires_exprs, ensures_body);
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
        if let Some(specs) = contract.callee_specs {
            enc_state.callee_specs.clone_from(specs);
        }
        register_cvc5_fixed_width_params(&tm, contract.params, &mut var_map, &mut enc_state);
        register_cvc5_fixed_width_return(&tm, contract.return_ty, &mut var_map, &mut enc_state);
        seed_cvc5_trigger_manager_from_clauses(&mut enc_state, contract.clauses);
        {
            let havoc_input = session.havoc_assume_input();
            apply_havoc_assume_cvc5(&tm, &havoc_input, &mut var_map, &mut enc_state);
        }

        let tracked_labels = if use_cores {
            assert_cvc5_requires_tracked(
                &tm,
                &mut solver,
                &prepared.requires_exprs,
                &mut var_map,
                &mut enc_state,
            )
        } else {
            assert_cvc5_requires(
                &tm,
                &mut solver,
                &prepared.requires_exprs,
                &mut var_map,
                &mut enc_state,
            );
            Vec::new()
        };

        if let Some(defs) = session.lemma_defs {
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

        let frame_vars = crate::clause_policy::frame_axiom_vars_for_clause(
            &prepared.frame_checker,
            &kind,
            ensures_body,
            &prepared.param_names,
        );
        if !frame_vars.is_empty() {
            assert_cvc5_frame_axioms(&tm, &mut solver, &var_map, &frame_vars);
        }

        assert_cvc5_clause_check(&tm, &mut solver, kind.clone(), body_term);

        finish_cvc5_clause_check(desc, kind, &mut solver, &var_map, &tracked_labels)
    };
    store_cvc5_clause_cache(session.cache, cache_key, &result);

    result
}
