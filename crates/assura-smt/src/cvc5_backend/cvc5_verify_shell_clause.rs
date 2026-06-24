//! Per-clause CVC5 shell-out verification.

use std::collections::HashSet;

use crate::VerificationResult;
use crate::cvc5_adt::cvc5_adt_prelude_lines;
use crate::cvc5_collect::collect_vars;
use crate::cvc5_expr_smtlib::expr_to_smtlib;
use crate::cvc5_havoc_assume_smtlib::append_havoc_assume_smtlib;
use crate::cvc5_verify_shared::{
    cvc5_clause_cache_key, cvc5_lookup_cached_clause, cvc5_unmodelable_precheck,
    store_cvc5_clause_cache,
};
use crate::cvc5_verify_shell_runner::{cvc5_shell_query_to_verification_result, run_cvc5_binary};
use crate::cvc5_verify_shell_script::{
    append_cvc5_shellout_clause_check, append_cvc5_shellout_constraints,
    append_cvc5_shellout_frame_axioms, append_cvc5_shellout_lemma_assumptions,
    append_cvc5_shellout_requires,
};
use crate::havoc_assume::HavocAssumeSmtlibTarget;
use crate::verify_context::{Cvc5ClauseVerifyInput, Cvc5ContractVerifySession};

pub(crate) fn check_clause_cvc5_shellout(
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

    let mut vars = HashSet::new();
    for req in &prepared.requires_exprs {
        collect_vars(req, &mut vars);
    }
    collect_vars(ensures_body, &mut vars);

    let mut script = String::new();
    script.push_str("(set-logic ALL)\n");

    for line in cvc5_adt_prelude_lines() {
        script.push_str(&line);
        if !line.ends_with('\n') {
            script.push('\n');
        }
    }

    for var in &vars {
        script.push_str(&format!("(declare-const {var} Int)\n"));
    }

    append_cvc5_shellout_constraints(
        &mut script,
        &vars,
        contract.params,
        contract.return_ty,
        contract.constants,
        &prepared.narrowings,
    );

    let havoc_input = session.havoc_assume_input();
    let mut havoc_target = HavocAssumeSmtlibTarget {
        script: &mut script,
        vars: &mut vars,
    };
    append_havoc_assume_smtlib(&mut havoc_target, &havoc_input);

    append_cvc5_shellout_requires(&mut script, &prepared.requires_exprs);

    if let Some(defs) = session.lemma_defs {
        append_cvc5_shellout_lemma_assumptions(&mut script, ensures_body, defs);
    }

    let frame_vars = crate::clause_policy::frame_axiom_vars_for_clause(
        &prepared.frame_checker,
        &kind,
        ensures_body,
        &prepared.param_names,
    );
    if !frame_vars.is_empty() {
        append_cvc5_shellout_frame_axioms(&mut script, &vars, &frame_vars);
    }

    let Some(smt) = expr_to_smtlib(ensures_body) else {
        return crate::clause_gate_policy::clause_encode_failure(desc, "SMT-LIB2");
    };
    append_cvc5_shellout_clause_check(&mut script, kind.clone(), &smt);

    script.push_str("(check-sat)\n");
    script.push_str("(get-model)\n");

    let result =
        cvc5_shell_query_to_verification_result(desc, kind.clone(), run_cvc5_binary(&script));

    store_cvc5_clause_cache(session.cache, cache_key, &result);

    result
}
