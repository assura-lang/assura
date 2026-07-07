//! CVC5 shell-out contract verification (incremental and per-clause paths).

use assura_ast::{ClauseKind, SpExpr};

use crate::VerificationResult;
use crate::cvc5_adt::cvc5_adt_prelude_lines;
use crate::cvc5_collect::collect_cvc5_var_names_from_clauses;
use crate::cvc5_expr_smtlib::expr_to_smtlib;
use crate::cvc5_havoc_assume_smtlib::append_havoc_assume_smtlib;
use crate::cvc5_verify_shared::{
    Cvc5ContractPrepared, cvc5_clause_cache_key, cvc5_lookup_cached_clause,
    cvc5_unmodelable_precheck, store_cvc5_clause_cache,
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
use crate::havoc_assume::HavocAssumeSmtlibTarget;
use crate::verify_context::{
    ContractVerifyContext, Cvc5ClauseVerifyInput, Cvc5ContractVerifySession,
};

struct PendingShellClause {
    index: usize,
    desc: String,
    kind: ClauseKind,
    cache_key: String,
}

struct IncrementalShellScriptInput<'a> {
    prepared: &'a Cvc5ContractPrepared<'a>,
    contract: &'a ContractVerifyContext<'a>,
    lemma_defs: Option<&'a std::collections::HashMap<String, Vec<&'a SpExpr>>>,
    pending: &'a [PendingShellClause],
}

pub(crate) fn verify_contract_cvc5_shellout(
    session: &mut Cvc5ContractVerifySession<'_>,
) -> Vec<VerificationResult> {
    let contract_name = session.contract.contract_name;
    let verifiable = session.prepared.verifiable.clone();
    let mut results = Vec::new();

    if verifiable.len() <= 1 {
        for clause in &verifiable {
            let desc = crate::cvc5_verify_shared::cvc5_clause_desc(contract_name, &clause.kind);
            let input = Cvc5ClauseVerifyInput {
                desc: &desc,
                body: &clause.body,
                kind: clause.kind.clone(),
            };
            results.push(check_clause_cvc5_shellout(&input, session));
        }
        return results;
    }

    results.extend(verify_contract_cvc5_shellout_incremental(session));
    results
}

fn verify_contract_cvc5_shellout_incremental(
    session: &mut Cvc5ContractVerifySession<'_>,
) -> Vec<VerificationResult> {
    let contract_name = session.contract.contract_name;
    let prepared = &session.prepared;
    let mut results = Vec::new();
    let mut resolved: Vec<(usize, VerificationResult)> = Vec::new();
    let mut pending: Vec<PendingShellClause> = Vec::new();

    for (index, clause) in prepared.verifiable.iter().enumerate() {
        let desc = crate::cvc5_verify_shared::cvc5_clause_desc(contract_name, &clause.kind);
        let cache_key = cvc5_clause_cache_key(&desc, &clause.kind, &clause.body);

        if let Some(cached) = cvc5_lookup_cached_clause(session.cache, &cache_key, &desc) {
            resolved.push((index, cached));
            continue;
        }
        if let Some(precheck) = cvc5_unmodelable_precheck(&desc, &clause.body) {
            resolved.push((index, precheck));
            continue;
        }
        if crate::cvc5_expr_smtlib::with_smtlib_side_effects(|| expr_to_smtlib(&clause.body))
            .0
            .is_none()
        {
            resolved.push((
                index,
                crate::clause_gate_policy::clause_encode_failure(&desc, "SMT-LIB2"),
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
        let script_input = IncrementalShellScriptInput {
            prepared,
            contract: session.contract,
            lemma_defs: session.lemma_defs,
            pending: &pending,
        };
        let script = build_incremental_shell_script(&script_input, session.havoc_assume_input());

        match run_cvc5_binary_queries(&script) {
            Ok(query_results) if query_results.len() == pending_count => {
                for (pending_clause, query) in pending.into_iter().zip(query_results) {
                    let result = cvc5_shell_query_to_verification_result(
                        &pending_clause.desc,
                        pending_clause.kind,
                        query,
                    );
                    store_cvc5_clause_cache(session.cache, pending_clause.cache_key, &result);
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

fn build_incremental_shell_script(
    input: &IncrementalShellScriptInput<'_>,
    havoc_input: crate::havoc_assume::HavocAssumeInput<'_>,
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

    let mut vars = collect_cvc5_var_names_from_clauses(
        &input.prepared.requires_exprs,
        &input.prepared.verifiable,
    );
    for var in &vars {
        script.push_str(&format!("(declare-const {var} Int)\n"));
    }

    append_cvc5_shellout_constraints(
        &mut script,
        &vars,
        input.contract.params,
        input.contract.return_ty,
        input.contract.constants,
        &input.prepared.narrowings,
    );
    append_cvc5_shellout_requires(&mut script, &input.prepared.requires_exprs);

    if let Some(defs) = input.lemma_defs {
        for clause in &input.prepared.verifiable {
            append_cvc5_shellout_lemma_assumptions(&mut script, &clause.body, defs);
        }
    }

    let pending_indices: std::collections::HashSet<usize> =
        input.pending.iter().map(|p| p.index).collect();

    for (index, clause) in input.prepared.verifiable.iter().enumerate() {
        if !pending_indices.contains(&index) {
            continue;
        }

        script.push_str("(push 1)\n");
        let mut havoc_target = HavocAssumeSmtlibTarget {
            script: &mut script,
            vars: &mut vars,
        };
        append_havoc_assume_smtlib(&mut havoc_target, &havoc_input);

        let frame_vars = crate::clause_policy::frame_axiom_vars_for_clause(
            &input.prepared.frame_checker,
            &clause.kind,
            &clause.body,
            &input.prepared.param_names,
        );
        if !frame_vars.is_empty() {
            append_cvc5_shellout_frame_axioms(&mut script, &vars, &frame_vars);
        }

        let (encoded, effects) =
            crate::cvc5_expr_smtlib::with_smtlib_side_effects(|| expr_to_smtlib(&clause.body));
        if let Some(smt) = encoded {
            for decl in &effects.declarations {
                script.push_str(decl);
                script.push('\n');
            }
            for axiom in &effects.assertions {
                script.push_str(axiom);
                script.push('\n');
            }
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
    use crate::cvc5_verify_shared::prepare_cvc5_contract_verification;
    use assura_ast::{BinOp, Clause, Expr, Literal, Spanned};

    #[test]
    fn non_ident_call_is_unencodable_but_not_unmodelable() {
        let body = Spanned::no_span(Expr::Call {
            func: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
            args: vec![],
        });
        assert!(expr_to_smtlib(&body).is_none());
        assert!(cvc5_unmodelable_precheck("T::Ensures", &body).is_none());
    }

    #[test]
    fn incremental_script_emits_one_check_sat_per_pending_clause() {
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gte,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gte,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Lte,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
                }),
                effect_variables: vec![],
            },
        ];
        let (feature_results, prepared) =
            prepare_cvc5_contract_verification("T", &clauses, &[], &[]);
        assert!(feature_results.is_empty());
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
        let ctx = ContractVerifyContext {
            contract_name: "T",
            clauses: &clauses,
            params: &[],
            return_ty: &["Int".into()],
            constants: &[],
            ir: None,
            callee_specs: None,
        };
        let input = IncrementalShellScriptInput {
            prepared: &prepared,
            contract: &ctx,
            lemma_defs: None,
            pending: &pending,
        };
        let havoc_input = crate::havoc_assume::HavocAssumeInput {
            requires: &prepared.requires_clauses,
            ensures: &prepared.ensures_clauses,
            return_ty: &["Int".into()],
            param_names: &prepared.param_names,
            ir: None,
            enc_ctx: crate::ir_encode::IrEncodeContext::default(),
        };
        let script = build_incremental_shell_script(&input, havoc_input);
        let check_count = script.matches("(check-sat)").count();
        assert_eq!(check_count, 2, "expected one check-sat per pending clause");
        assert_eq!(script.matches("(push 1)").count(), 2);
        assert_eq!(script.matches("(pop 1)").count(), 2);
    }
}
