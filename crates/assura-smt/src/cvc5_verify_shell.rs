#![cfg(not(feature = "cvc5-verify"))]

use assura_parser::ast::{Clause, ClauseKind, Expr};

use crate::VerificationResult;
use crate::cache::SessionCache;
use crate::cvc5_verify_shared::cvc5_contract_shared_setup;
use crate::cvc5_verify_shell_clause::check_clause_cvc5_shellout;

pub(crate) fn verify_contract_cvc5_shellout(
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

    for clause in clauses {
        match &clause.kind {
            ClauseKind::Ensures
            | ClauseKind::Invariant
            | ClauseKind::Rule
            | ClauseKind::MustNot
            | ClauseKind::Decreases => {
                let desc = format!("{contract_name}::{:?}", clause.kind);
                let result = check_clause_cvc5_shellout(
                    &desc,
                    &requires_exprs,
                    &clause.body,
                    clause.kind.clone(),
                    params,
                    return_ty,
                    constants,
                    &narrowings,
                    &frame_checker,
                    lemma_defs,
                    cache,
                );
                results.push(result);
            }
            ClauseKind::Other(kind) => {
                let feature_results = crate::smt_features::verify_feature_clause(
                    kind,
                    contract_name,
                    &clause.body,
                    clauses,
                );
                results.extend(feature_results);
            }
            _ => {}
        }
    }

    results
}
