//! Per-clause CVC5 shell-out verification.

use std::collections::HashSet;

use assura_parser::ast::{ClauseKind, Expr};

use crate::VerificationResult;
use crate::cache::SessionCache;
use crate::cvc5_adt::cvc5_adt_prelude_lines;
use crate::cvc5_collect::collect_vars;
use crate::cvc5_expr_smtlib::expr_to_smtlib;
use crate::cvc5_model::parse_smtlib_model;
use crate::cvc5_verify_shared::{
    cvc5_clause_result_from_unsat, cvc5_lookup_cached_clause, cvc5_unmodelable_precheck,
    store_cvc5_clause_cache,
};
use crate::cvc5_verify_shell_runner::{Cvc5Result, run_cvc5_binary};
use crate::cvc5_verify_shell_script::{
    append_cvc5_shellout_clause_check, append_cvc5_shellout_constraints,
    append_cvc5_shellout_frame_axioms, append_cvc5_shellout_lemma_assumptions,
    append_cvc5_shellout_requires,
};

#[expect(clippy::too_many_arguments)]
pub(crate) fn check_clause_cvc5_shellout(
    desc: &str,
    requires: &[&Expr],
    ensures_body: &Expr,
    kind: ClauseKind,
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
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

    let mut vars = HashSet::new();
    for req in requires {
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

    append_cvc5_shellout_constraints(&mut script, &vars, params, return_ty, constants, narrowings);

    append_cvc5_shellout_requires(&mut script, requires);

    if kind == ClauseKind::Ensures && frame_checker.has_modifies() {
        let frame_vars = frame_checker.frame_axiom_vars(ensures_body);
        append_cvc5_shellout_frame_axioms(&mut script, &vars, &frame_vars);
    }

    if let Some(defs) = lemma_defs {
        append_cvc5_shellout_lemma_assumptions(&mut script, ensures_body, defs);
    }

    let Some(smt) = expr_to_smtlib(ensures_body) else {
        return VerificationResult::Unknown {
            clause_desc: desc.to_string(),
            reason: "could not encode clause to SMT-LIB2".into(),
        };
    };
    append_cvc5_shellout_clause_check(&mut script, kind.clone(), &smt);

    script.push_str("(check-sat)\n");
    script.push_str("(get-model)\n");

    let result = match run_cvc5_binary(&script) {
        Cvc5Result::Unsat => cvc5_clause_result_from_unsat(desc, kind),
        Cvc5Result::Sat(model_str) => {
            if matches!(kind, ClauseKind::Invariant) {
                VerificationResult::verified(desc.to_string())
            } else {
                let counter_model = parse_smtlib_model(&model_str);
                let filtered_model = counter_model
                    .as_ref()
                    .map(|cm| {
                        cm.variables
                            .iter()
                            .map(|(n, v)| format!("{n} = {v}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or(model_str);
                VerificationResult::Counterexample {
                    clause_desc: desc.to_string(),
                    model: filtered_model,
                    counter_model,
                }
            }
        }
        Cvc5Result::Timeout => VerificationResult::Timeout {
            clause_desc: desc.to_string(),
        },
        Cvc5Result::Error(reason) => VerificationResult::Unknown {
            clause_desc: desc.to_string(),
            reason,
        },
    };

    store_cvc5_clause_cache(cache, cache_key, &result);

    result
}
