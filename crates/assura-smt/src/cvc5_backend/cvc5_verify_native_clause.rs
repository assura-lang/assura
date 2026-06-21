//! Per-clause CVC5 native verification (non-incremental path).

use assura_ast::ClauseKind;

use crate::VerificationResult;
use crate::cvc5_collect::collect_cvc5_var_names;
use crate::cvc5_native_encoder::{
    apply_havoc_assume_cvc5, default_cvc5_encoder_state, encode_expr_cvc5,
};
use crate::cvc5_verify_native_solver::{
    Cvc5SolverOpts, assert_cvc5_axioms, assert_cvc5_clause_check, assert_cvc5_frame_axioms,
    assert_cvc5_requires, assert_cvc5_solver_prelude, build_cvc5_var_map, finish_cvc5_clause_check,
    inject_cvc5_lemma_assumptions, new_cvc5_solver,
};
use crate::cvc5_verify_shared::{
    cvc5_encode_failure, cvc5_lookup_cached_clause, cvc5_unmodelable_precheck,
    store_cvc5_clause_cache,
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

    let cache_key = format!("{desc}::{kind:?}:{ensures_body:?}");
    if let Some(result) = cvc5_lookup_cached_clause(session.cache, &cache_key, desc) {
        return result;
    }

    if let Some(result) = cvc5_unmodelable_precheck(desc, ensures_body) {
        return result;
    }

    let tm = cvc5::TermManager::new();
    let mut solver = new_cvc5_solver(&tm, Cvc5SolverOpts::default());

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
    let havoc_input = session.havoc_assume_input();
    apply_havoc_assume_cvc5(&tm, &havoc_input, &mut var_map, &mut enc_state);

    assert_cvc5_requires(
        &tm,
        &mut solver,
        &prepared.requires_exprs,
        &mut var_map,
        &mut enc_state,
    );

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

    if kind == ClauseKind::Ensures && prepared.frame_checker.has_modifies() {
        let frame_vars = prepared.frame_checker.frame_axiom_vars(ensures_body);
        assert_cvc5_frame_axioms(&tm, &mut solver, &var_map, &frame_vars);
    }

    assert_cvc5_clause_check(&tm, &mut solver, kind.clone(), body_term);

    let result = finish_cvc5_clause_check(desc, kind, &mut solver, &var_map);
    store_cvc5_clause_cache(session.cache, cache_key, &result);

    result
}
