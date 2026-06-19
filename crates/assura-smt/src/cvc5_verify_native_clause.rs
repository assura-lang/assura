//! Per-clause CVC5 native verification (non-incremental path).

use assura_parser::ast::{Clause, ClauseKind, Expr};

use crate::VerificationResult;
use crate::cache::SessionCache;
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

#[expect(clippy::too_many_arguments)]
pub(crate) fn check_clause_cvc5_native(
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
    ir_blocks: Option<&std::collections::HashMap<usize, Vec<crate::ir::IrInstr>>>,
    type_env: Option<&assura_types::TypeEnv>,
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
    assert_cvc5_solver_prelude(
        &tm,
        &mut solver,
        &var_map,
        params,
        return_ty,
        &[],
        narrowings,
    );

    let mut enc_state = default_cvc5_encoder_state();

    apply_havoc_assume_cvc5(
        &tm,
        requires_clauses,
        ensures_clauses,
        return_ty,
        param_names,
        ir_body,
        ir_blocks,
        type_env,
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
