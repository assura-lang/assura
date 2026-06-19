//! Shared verification helpers for CVC5 native and shell-out paths.

use std::collections::HashSet;

use assura_parser::ast::{Clause, ClauseKind, Decl, Expr};

use crate::CounterexampleModel;
use crate::VerificationResult;
use crate::cache::SessionCache;
use crate::cvc5_common::{
    collect_unmodelable_reasons_cvc5, expr_has_unmodelable_features_cvc5, sanitize_smtlib_name,
};
use crate::cvc5_feature_max::derive_narrowings_cvc5;

/// Backend-neutral type/constant constraints for CVC5 solver preludes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Cvc5TypeConstraint {
    NatNonNegative(String),
    ConstantEq(String, i64),
    NarrowingLe(String, i64),
}

/// Collect Nat, constant, and feature_max narrowing constraints for declared vars.
pub(crate) fn collect_cvc5_type_constraints(
    vars: &HashSet<String>,
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    narrowings: &[(String, i64)],
) -> Vec<Cvc5TypeConstraint> {
    let mut out = Vec::new();
    for param in params {
        if param.ty.len() == 1 && param.ty[0] == "Nat" {
            let name = sanitize_smtlib_name(&param.name);
            if vars.contains(&name) {
                out.push(Cvc5TypeConstraint::NatNonNegative(name));
            }
        }
    }
    if return_ty.len() == 1 && return_ty[0] == "Nat" {
        if vars.contains("__result") {
            out.push(Cvc5TypeConstraint::NatNonNegative("__result".into()));
        }
        if vars.contains("result") {
            out.push(Cvc5TypeConstraint::NatNonNegative("result".into()));
        }
    }
    for (name, value) in constants {
        let key = sanitize_smtlib_name(name);
        if vars.contains(&key) {
            out.push(Cvc5TypeConstraint::ConstantEq(key, *value));
        }
    }
    for (name, value) in narrowings {
        let key = sanitize_smtlib_name(name);
        if vars.contains(&key) {
            out.push(Cvc5TypeConstraint::NarrowingLe(key, *value));
        }
    }
    out
}

/// Outcome of a single clause `check-sat` query.
#[derive(Debug, Clone)]
pub(crate) enum Cvc5ClauseSatOutcome {
    Unsat,
    Sat {
        model_str: String,
        counter_model: Option<CounterexampleModel>,
    },
    Timeout,
}

/// Map SAT/UNSAT/timeout to `VerificationResult` (Ensures vs Invariant semantics).
pub(crate) fn cvc5_interpret_clause_check_result(
    desc: &str,
    kind: ClauseKind,
    outcome: Cvc5ClauseSatOutcome,
) -> VerificationResult {
    match outcome {
        Cvc5ClauseSatOutcome::Unsat => {
            if matches!(kind, ClauseKind::Invariant) {
                VerificationResult::Counterexample {
                    clause_desc: desc.to_string(),
                    model: "invariant is unsatisfiable".to_string(),
                    counter_model: None,
                }
            } else {
                VerificationResult::verified(desc.to_string())
            }
        }
        Cvc5ClauseSatOutcome::Sat {
            model_str,
            counter_model,
        } => {
            if matches!(kind, ClauseKind::Invariant) {
                VerificationResult::verified(desc.to_string())
            } else {
                VerificationResult::Counterexample {
                    clause_desc: desc.to_string(),
                    model: model_str,
                    counter_model,
                }
            }
        }
        Cvc5ClauseSatOutcome::Timeout => VerificationResult::Timeout {
            clause_desc: desc.to_string(),
        },
    }
}

/// Collect lemma definitions from a typed file's declarations.
///
/// Maps each lemma name to its ensures clause bodies. This mirrors
/// `z3_backend::collect_lemma_defs` but is available without the
/// `z3-verify` feature.
pub(crate) fn collect_lemma_defs_for_cvc5(
    typed: &assura_types::TypedFile,
) -> std::collections::HashMap<String, Vec<&Expr>> {
    let mut lemmas = std::collections::HashMap::new();
    for decl in &typed.resolved.source.decls {
        if let Decl::FnDef(f) = &decl.node
            && f.is_lemma
        {
            let ensures: Vec<&Expr> = f
                .clauses
                .iter()
                .filter(|c| c.kind == ClauseKind::Ensures)
                .map(|c| &c.body)
                .collect();
            lemmas.insert(f.name.clone(), ensures);
        }
    }
    lemmas
}

/// Shared contract setup for native and shell-out CVC5 verify paths.
pub(crate) fn cvc5_contract_shared_setup<'a>(
    clauses: &'a [Clause],
    constants: &[(String, i64)],
) -> (
    Vec<(String, i64)>,
    Vec<&'a Expr>,
    assura_types::FrameChecker,
) {
    let narrowings = derive_narrowings_cvc5(constants);
    let requires_exprs: Vec<&Expr> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let modifies_bodies: Vec<&Expr> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Modifies)
        .map(|c| &c.body)
        .collect();
    let frame_checker = if modifies_bodies.is_empty() {
        assura_types::FrameChecker::empty()
    } else {
        assura_types::FrameChecker::new(&modifies_bodies)
    };
    (narrowings, requires_exprs, frame_checker)
}

pub(crate) fn cvc5_lookup_cached_clause(
    cache: &mut SessionCache,
    cache_key: &str,
    desc: &str,
) -> Option<VerificationResult> {
    cache
        .lookup(cache_key)
        .map(|entry| match entry.result.as_str() {
            "verified" => VerificationResult::verified(desc.to_string()),
            other => VerificationResult::Unknown {
                clause_desc: desc.to_string(),
                reason: format!("cached: {other}"),
            },
        })
}

pub(crate) fn cvc5_unmodelable_precheck(desc: &str, body: &Expr) -> Option<VerificationResult> {
    if !expr_has_unmodelable_features_cvc5(body) {
        return None;
    }
    let reasons = collect_unmodelable_reasons_cvc5(body);
    Some(VerificationResult::Unknown {
        clause_desc: desc.to_string(),
        reason: format!(
            "clause uses features not yet encoded in SMT ({})",
            reasons.join(", ")
        ),
    })
}

pub(crate) fn store_cvc5_clause_cache(
    cache: &mut SessionCache,
    cache_key: String,
    result: &VerificationResult,
) {
    let result_str = match result {
        VerificationResult::Verified { .. } => "verified",
        VerificationResult::Counterexample { .. } => "counterexample",
        VerificationResult::Timeout { .. } => "timeout",
        VerificationResult::Unknown { .. } => "unknown",
    };
    cache.insert(cache_key, result_str.to_string(), 0);
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn cvc5_encode_failure(desc: &str) -> VerificationResult {
    VerificationResult::Unknown {
        clause_desc: desc.to_string(),
        reason: "could not encode clause to CVC5 terms".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::ClauseKind;
    use std::collections::HashSet;

    #[test]
    fn collect_type_constraints_nat_and_narrowing() {
        let mut vars = HashSet::new();
        vars.insert("n".into());
        vars.insert("size".into());
        let params = vec![assura_parser::ast::Param {
            name: "n".into(),
            ty: vec!["Nat".into()],
            parsed_type: None,
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
            "C::Ensures",
            ClauseKind::Ensures,
            Cvc5ClauseSatOutcome::Unsat,
        );
        assert!(matches!(result, VerificationResult::Verified { .. }));
    }

    #[test]
    fn interpret_invariant_unsat_is_counterexample() {
        let result = cvc5_interpret_clause_check_result(
            "C::Invariant",
            ClauseKind::Invariant,
            Cvc5ClauseSatOutcome::Unsat,
        );
        assert!(matches!(result, VerificationResult::Counterexample { .. }));
    }
}
