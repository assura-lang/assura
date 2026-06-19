#![cfg(not(feature = "cvc5-verify"))]

use std::collections::HashSet;

use assura_parser::ast::{Clause, ClauseKind, Expr};

use crate::VerificationResult;
use crate::cache::SessionCache;
use crate::cvc5_adt::cvc5_adt_prelude_lines;
use crate::cvc5_backend::expr_to_smtlib;
use crate::cvc5_collect::collect_vars;
use crate::cvc5_common::{collect_apply_refs_from_expr, sanitize_smtlib_name};
use crate::cvc5_model::parse_smtlib_model;
use crate::cvc5_verify_shared::{
    cvc5_clause_result_from_unsat, cvc5_contract_shared_setup, cvc5_lookup_cached_clause,
    cvc5_unmodelable_precheck, store_cvc5_clause_cache,
};

fn append_cvc5_shellout_requires(script: &mut String, requires: &[&Expr]) {
    for req in requires {
        if let Some(smt) = expr_to_smtlib(req) {
            script.push_str(&format!("(assert {smt})\n"));
        }
    }
}

fn append_cvc5_shellout_frame_axioms(
    script: &mut String,
    vars: &HashSet<String>,
    frame_vars: &[String],
) {
    for var_name in frame_vars {
        let current = sanitize_smtlib_name(var_name);
        let old = sanitize_smtlib_name(&format!("{var_name}__old"));
        if !vars.contains(&old) {
            script.push_str(&format!("(declare-const {old} Int)\n"));
        }
        script.push_str(&format!("(assert (= {current} {old}))\n"));
    }
}

fn append_cvc5_shellout_lemma_assumptions(
    script: &mut String,
    body: &Expr,
    defs: &std::collections::HashMap<String, Vec<&Expr>>,
) {
    let apply_refs = collect_apply_refs_from_expr(body);
    for lemma_name in &apply_refs {
        if let Some(ensures_bodies) = defs.get(lemma_name) {
            for ens_body in ensures_bodies {
                if let Some(smt) = expr_to_smtlib(ens_body) {
                    script.push_str(&format!("(assert {smt})\n"));
                }
            }
        }
    }
}

fn append_cvc5_shellout_clause_check(script: &mut String, kind: ClauseKind, smt: &str) {
    match kind {
        ClauseKind::Invariant | ClauseKind::MustNot => {
            script.push_str(&format!("(assert {smt})\n"));
        }
        _ => {
            script.push_str(&format!("(assert (not {smt}))\n"));
        }
    }
}

fn append_cvc5_shellout_constraints(
    script: &mut String,
    vars: &HashSet<String>,
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    narrowings: &[(String, i64)],
) {
    for param in params {
        if param.ty.len() == 1 && param.ty[0] == "Nat" {
            let name = sanitize_smtlib_name(&param.name);
            if vars.contains(&name) {
                script.push_str(&format!("(assert (>= {name} 0))\n"));
            }
        }
    }
    if return_ty.len() == 1 && return_ty[0] == "Nat" {
        if vars.contains("__result") {
            script.push_str("(assert (>= __result 0))\n");
        }
        if vars.contains("result") {
            script.push_str("(assert (>= result 0))\n");
        }
    }
    for (name, value) in constants {
        let key = sanitize_smtlib_name(name);
        if vars.contains(&key) {
            script.push_str(&format!("(assert (= {key} {value}))\n"));
        }
    }
    for (name, value) in narrowings {
        let key = sanitize_smtlib_name(name);
        if vars.contains(&key) {
            script.push_str(&format!("(assert (<= {key} {value}))\n"));
        }
    }
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

/// Result of running CVC5 binary on an SMT-LIB2 script.
enum Cvc5Result {
    Unsat,
    Sat(String),
    Timeout,
    Error(String),
}

#[expect(clippy::too_many_arguments)]
fn check_clause_cvc5_shellout(
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

fn run_cvc5_binary(script: &str) -> Cvc5Result {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut cmd = Command::new("cvc5");
    cmd.arg("--lang")
        .arg("smt2")
        .arg("--tlimit")
        .arg("1000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Cvc5Result::Error(format!("cvc5 not found on PATH: {e}"));
        }
    };

    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin.write_all(script.as_bytes())
    {
        return Cvc5Result::Error(format!("Failed to write SMT script to CVC5 stdin: {e}"));
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            return Cvc5Result::Error(format!("cvc5 execution failed: {e}"));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap_or("").trim();

    match first_line {
        "unsat" => Cvc5Result::Unsat,
        "sat" => {
            let model = stdout.lines().skip(1).collect::<Vec<_>>().join("\n");
            Cvc5Result::Sat(model)
        }
        "timeout" | "resourceout" => Cvc5Result::Timeout,
        "unknown" => Cvc5Result::Timeout,
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("timeout") || stderr.contains("resourceout") {
                Cvc5Result::Timeout
            } else {
                Cvc5Result::Error(format!("unexpected cvc5 output: {first_line}"))
            }
        }
    }
}
