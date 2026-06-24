//! Shared verification helpers for CVC5 native and shell-out paths.

use std::collections::HashSet;

use assura_ast::{Clause, ClauseKind, SpExpr};

use crate::VerificationResult;
use crate::cache::SessionCache;
use crate::cvc5_common::{
    collect_unmodelable_reasons_cvc5, expr_has_unmodelable_features_cvc5, sanitize_smtlib_name,
};
use crate::cvc5_model::parse_smtlib_model;

/// CVC5 prelude constraint (alias of shared [`crate::prelude_policy::PreludeConstraint`]).
pub(crate) type Cvc5TypeConstraint = crate::prelude_policy::PreludeConstraint;

/// Collect Nat, constant, and feature_max narrowing constraints for declared vars.
///
/// Delegates to [`crate::prelude_policy`] (one brain with Z3; filtered by SMT-LIB var set).
pub(crate) fn collect_cvc5_type_constraints(
    vars: &HashSet<String>,
    params: &[assura_ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    narrowings: &[(String, i64)],
) -> Vec<Cvc5TypeConstraint> {
    crate::prelude_policy::collect_prelude_constraints_for_vars(
        vars,
        params,
        return_ty,
        constants,
        narrowings,
        sanitize_smtlib_name,
    )
}

/// Outcome of a single clause `check-sat` query (alias of shared policy type).
pub(crate) type Cvc5ClauseSatOutcome = crate::solver_outcome_policy::ClauseSatOutcome;

/// Build a SAT outcome from raw SMT-LIB2 `(get-model)` stdout (shell-out path).
#[cfg_attr(
    all(feature = "cvc5-verify", not(test)),
    allow(dead_code, reason = "shell-out model parsing only")
)]
pub(crate) fn cvc5_sat_outcome_from_smtlib_model(raw_model: String) -> Cvc5ClauseSatOutcome {
    let counter_model = parse_smtlib_model(&raw_model);
    let model_str = counter_model
        .as_ref()
        .map(|cm| {
            cm.variables
                .iter()
                .map(|(n, v)| format!("{n} = {v}"))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or(raw_model);
    crate::solver_outcome_policy::ClauseSatOutcome::sat(model_str, counter_model)
}

/// Map SAT/UNSAT/timeout to `VerificationResult` (delegates to [`crate::solver_outcome_policy`]).
pub(crate) fn cvc5_interpret_clause_check_result(
    desc: &str,
    kind: ClauseKind,
    outcome: Cvc5ClauseSatOutcome,
) -> VerificationResult {
    crate::solver_outcome_policy::interpret_clause_check_result(desc, &kind, outcome)
}

/// Collect lemma definitions from a typed file (shared [`crate::verify_labels`]).
pub(crate) fn collect_lemma_defs_for_cvc5(
    typed: &assura_types::TypedFile,
) -> std::collections::HashMap<String, Vec<&SpExpr>> {
    crate::verify_labels::collect_lemma_defs(typed)
}

/// CVC5 clause descriptor (stable `parent::kind` labels, not `Debug` of `ClauseKind`).
pub(crate) fn cvc5_clause_desc(contract_name: &str, kind: &ClauseKind) -> String {
    crate::verify_labels::clause_desc(contract_name, kind)
}

/// Prepared state shared by native and shell-out CVC5 contract verification.
///
/// Thin alias of [`crate::clause_policy::ContractClausePrep`] so CVC5 modules
/// keep stable names while Z3 and CVC5 share one preparation brain.
pub(crate) type Cvc5ContractPrepared<'a> = crate::clause_policy::ContractClausePrep<'a>;

/// Collect feature-clause results and shared contract state for CVC5 backends.
pub(crate) fn prepare_cvc5_contract_verification<'a>(
    contract_name: &str,
    clauses: &'a [Clause],
    params: &[assura_ast::Param],
    constants: &[(String, i64)],
) -> (Vec<VerificationResult>, Cvc5ContractPrepared<'a>) {
    crate::clause_policy::prepare_contract_clauses(contract_name, clauses, params, constants)
}

pub(crate) fn cvc5_lookup_cached_clause(
    cache: &mut SessionCache,
    cache_key: &str,
    desc: &str,
) -> Option<VerificationResult> {
    crate::clause_gate_policy::lookup_clause_session_cache(cache, cache_key, desc)
}

pub(crate) fn cvc5_unmodelable_precheck(desc: &str, body: &SpExpr) -> Option<VerificationResult> {
    let has = expr_has_unmodelable_features_cvc5(body);
    let reasons = if has {
        collect_unmodelable_reasons_cvc5(body)
    } else {
        Vec::new()
    };
    crate::clause_gate_policy::unmodelable_precheck_if(desc, has, &reasons)
}

pub(crate) fn store_cvc5_clause_cache(
    cache: &mut SessionCache,
    cache_key: String,
    result: &VerificationResult,
) {
    crate::clause_gate_policy::store_clause_session_cache(cache, cache_key, result);
}

/// CVC5 session cache key (delegates to shared gate policy; includes clause kind).
pub(crate) fn cvc5_clause_cache_key(desc: &str, kind: &ClauseKind, body: &SpExpr) -> String {
    crate::clause_gate_policy::clause_session_cache_key(desc, kind, body)
}

/// Native CVC5 encode miss (only called from `cvc5-verify` / incremental-native paths).
#[cfg_attr(
    not(feature = "cvc5-verify"),
    allow(
        dead_code,
        reason = "callers are cfg(cvc5-verify); shell uses clause_encode_failure"
    )
)]
pub(crate) fn cvc5_encode_failure(desc: &str) -> VerificationResult {
    crate::clause_gate_policy::clause_encode_failure(desc, "CVC5 terms")
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::ClauseKind;
    use std::collections::HashSet;

    #[test]
    fn collect_type_constraints_nat_and_narrowing() {
        let mut vars = HashSet::new();
        vars.insert("n".into());
        vars.insert("size".into());
        let params = vec![assura_ast::Param {
            name: "n".into(),
            ty: Some(assura_ast::TypeExpr::Named("Nat".into())),
        }];
        let constraints = collect_cvc5_type_constraints(
            &vars,
            &params,
            &["Int".into()],
            &[],
            &[("size".into(), 100)],
        );
        assert!(constraints.contains(&Cvc5TypeConstraint::NatNonNegative("n".into())));
        assert!(constraints.contains(&Cvc5TypeConstraint::NarrowingLe("size".into(), 100)));
    }

    #[test]
    fn interpret_ensures_unsat_is_verified() {
        let result = cvc5_interpret_clause_check_result(
            "C::ensures",
            ClauseKind::Ensures,
            Cvc5ClauseSatOutcome::unsat(),
        );
        assert!(matches!(result, VerificationResult::Verified { .. }));
    }

    #[test]
    fn sat_outcome_from_smtlib_model_parses_variables() {
        let outcome = cvc5_sat_outcome_from_smtlib_model("(define-fun x () Int 7)".into());
        match outcome {
            Cvc5ClauseSatOutcome::Sat {
                model_str,
                counter_model,
            } => {
                assert_eq!(model_str, "x = 7");
                let cm = counter_model.expect("counter model");
                assert_eq!(cm.variables, vec![("x".into(), "7".into())]);
            }
            other => panic!("expected Sat outcome, got {other:?}"),
        }
    }

    #[test]
    fn interpret_invariant_unsat_is_counterexample() {
        let result = cvc5_interpret_clause_check_result(
            "C::invariant",
            ClauseKind::Invariant,
            Cvc5ClauseSatOutcome::unsat(),
        );
        assert!(matches!(result, VerificationResult::Counterexample { .. }));
    }
}
