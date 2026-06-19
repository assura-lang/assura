//! CVC5 shell-out contract verification (incremental and per-clause paths).

use assura_parser::ast::{Clause, ClauseKind, Expr};

use crate::VerificationResult;
use crate::cache::SessionCache;
use crate::cvc5_adt::cvc5_adt_prelude_lines;
use crate::cvc5_collect::collect_cvc5_var_names_from_clauses;
use crate::cvc5_expr_smtlib::expr_to_smtlib;
use crate::cvc5_havoc_assume_smtlib::append_havoc_assume_smtlib;
use crate::cvc5_verify_shared::{
    cvc5_contract_shared_setup, cvc5_lookup_cached_clause, cvc5_unmodelable_precheck,
    store_cvc5_clause_cache,
};
use crate::cvc5_verify_shell_clause::check_clause_cvc5_shellout;
use crate::cvc5_verify_shell_runner::{
    cvc5_shell_query_to_verification_result, run_cvc5_binary_queries,
};
use crate::cvc5_verify_shell_script::{
    append_cvc5_shellout_clause_check, append_cvc5_shellout_constraints,
    append_cvc5_shellout_frame_axioms, append_cvc5_shellout_lemma_assumptions,
    append_cvc5_shellout_requires,
};

struct PendingShellClause {
    index: usize,
    desc: String,
    kind: ClauseKind,
    cache_key: String,
}

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

    let verifiable: Vec<&Clause> = clauses
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

    if verifiable.len() <= 1 {
        for clause in &verifiable {
            let desc = format!("{contract_name}::{:?}", clause.kind);
            results.push(check_clause_cvc5_shellout(
                &desc,
                &requires_exprs,
                &requires_clauses,
                &ensures_clauses,
                &clause.body,
                clause.kind.clone(),
                params,
                return_ty,
                &param_names,
                constants,
                &narrowings,
                &frame_checker,
                lemma_defs,
                cache,
            ));
        }
        return results;
    }

    let mut resolved: Vec<(usize, VerificationResult)> = Vec::new();
    let mut pending: Vec<PendingShellClause> = Vec::new();

    for (index, clause) in verifiable.iter().enumerate() {
        let desc = format!("{contract_name}::{:?}", clause.kind);
        let cache_key = format!("{desc}::{:?}:{:?}", clause.kind, clause.body);

        if let Some(cached) = cvc5_lookup_cached_clause(cache, &cache_key, &desc) {
            resolved.push((index, cached));
            continue;
        }
        if let Some(precheck) = cvc5_unmodelable_precheck(&desc, &clause.body) {
            resolved.push((index, precheck));
            continue;
        }
        if expr_to_smtlib(&clause.body).is_none() {
            resolved.push((
                index,
                VerificationResult::Unknown {
                    clause_desc: desc,
                    reason: "could not encode clause to SMT-LIB2".into(),
                },
            ));
            continue;
        }

        pending.push(PendingShellClause {
            index,
            desc,
            kind: clause.kind.clone(),
            cache_key,
        });
    }

    if !pending.is_empty() {
        let pending_count = pending.len();
        let script = build_incremental_shell_script(
            &requires_exprs,
            &requires_clauses,
            &ensures_clauses,
            &verifiable,
            &pending,
            params,
            return_ty,
            constants,
            &narrowings,
            &frame_checker,
            lemma_defs,
        );

        match run_cvc5_binary_queries(&script) {
            Ok(query_results) if query_results.len() == pending_count => {
                for (pending_clause, query) in pending.into_iter().zip(query_results) {
                    let result = cvc5_shell_query_to_verification_result(
                        &pending_clause.desc,
                        pending_clause.kind,
                        query,
                    );
                    store_cvc5_clause_cache(cache, pending_clause.cache_key, &result);
                    resolved.push((pending_clause.index, result));
                }
            }
            Ok(query_results) => {
                for pending_clause in pending {
                    resolved.push((
                        pending_clause.index,
                        VerificationResult::Unknown {
                            clause_desc: pending_clause.desc,
                            reason: format!(
                                "cvc5 returned {} check-sat results for {} pending clauses; \
                                 verify each clause body encodes to SMT-LIB2",
                                query_results.len(),
                                pending_count
                            ),
                        },
                    ));
                }
            }
            Err(reason) => {
                for pending_clause in pending {
                    resolved.push((
                        pending_clause.index,
                        VerificationResult::Unknown {
                            clause_desc: pending_clause.desc,
                            reason: reason.clone(),
                        },
                    ));
                }
            }
        }
    }

    resolved.sort_by_key(|(index, _)| *index);
    results.extend(resolved.into_iter().map(|(_, result)| result));
    results
}

#[expect(clippy::too_many_arguments)]
fn build_incremental_shell_script(
    requires_exprs: &[&Expr],
    requires_clauses: &[&Clause],
    ensures_clauses: &[&Clause],
    verifiable: &[&Clause],
    pending: &[PendingShellClause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    narrowings: &[(String, i64)],
    frame_checker: &assura_types::FrameChecker,
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&Expr>>>,
) -> String {
    let mut script = String::new();
    script.push_str("(set-logic ALL)\n");
    script.push_str("(set-option :incremental true)\n");
    script.push_str("(set-option :produce-models true)\n");

    for line in cvc5_adt_prelude_lines() {
        script.push_str(&line);
        if !line.ends_with('\n') {
            script.push('\n');
        }
    }

    let mut vars = collect_cvc5_var_names_from_clauses(requires_exprs, verifiable);
    for var in &vars {
        script.push_str(&format!("(declare-const {var} Int)\n"));
    }

    append_cvc5_shellout_constraints(&mut script, &vars, params, return_ty, constants, narrowings);
    append_cvc5_shellout_requires(&mut script, requires_exprs);

    if let Some(defs) = lemma_defs {
        for clause in verifiable {
            append_cvc5_shellout_lemma_assumptions(&mut script, &clause.body, defs);
        }
    }

    let pending_indices: std::collections::HashSet<usize> =
        pending.iter().map(|p| p.index).collect();

    for (index, clause) in verifiable.iter().enumerate() {
        if !pending_indices.contains(&index) {
            continue;
        }

        script.push_str("(push 1)\n");
        append_havoc_assume_smtlib(
            &mut script,
            &mut vars,
            requires_clauses,
            ensures_clauses,
            return_ty,
        );

        if clause.kind == ClauseKind::Ensures && frame_checker.has_modifies() {
            let frame_vars = frame_checker.frame_axiom_vars(&clause.body);
            append_cvc5_shellout_frame_axioms(&mut script, &vars, &frame_vars);
        }

        if let Some(smt) = expr_to_smtlib(&clause.body) {
            append_cvc5_shellout_clause_check(&mut script, clause.kind.clone(), &smt);
            script.push_str("(check-sat)\n");
            script.push_str("(get-model)\n");
        }
        script.push_str("(pop 1)\n");
    }

    script
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::{BinOp, Literal};

    #[test]
    fn non_ident_call_is_unencodable_but_not_unmodelable() {
        let body = Expr::Call {
            func: Box::new(Expr::Literal(Literal::Int("1".into()))),
            args: vec![],
        };
        assert!(expr_to_smtlib(&body).is_none());
        assert!(cvc5_unmodelable_precheck("T::Ensures", &body).is_none());
    }

    #[test]
    fn incremental_script_emits_one_check_sat_per_pending_clause() {
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gte,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gte,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Lte,
                    rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
                },
                effect_variables: vec![],
            },
        ];
        let verifiable: Vec<&Clause> = clauses
            .iter()
            .filter(|c| c.kind == ClauseKind::Ensures)
            .collect();
        let requires_exprs: Vec<&Expr> = clauses
            .iter()
            .filter(|c| c.kind == ClauseKind::Requires)
            .map(|c| &c.body)
            .collect();
        let pending = vec![
            PendingShellClause {
                index: 0,
                desc: "T::Ensures".into(),
                kind: ClauseKind::Ensures,
                cache_key: "k1".into(),
            },
            PendingShellClause {
                index: 1,
                desc: "T::Ensures".into(),
                kind: ClauseKind::Ensures,
                cache_key: "k2".into(),
            },
        ];
        let script = build_incremental_shell_script(
            &requires_exprs,
            &[],
            &verifiable,
            &verifiable,
            &pending,
            &[],
            &["Int".into()],
            &[],
            &[],
            &assura_types::FrameChecker::empty(),
            None,
        );
        let check_count = script.matches("(check-sat)").count();
        assert_eq!(check_count, 2, "expected one check-sat per pending clause");
        assert!(script.contains("(push 1)"));
        assert!(script.contains("(pop 1)"));
    }
}
