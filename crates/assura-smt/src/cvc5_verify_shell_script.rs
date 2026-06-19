//! SMT-LIB2 script assembly helpers for the CVC5 shell-out path.

use std::collections::HashSet;

use assura_parser::ast::{ClauseKind, Expr};

use crate::cvc5_common::{collect_apply_refs_from_expr, sanitize_smtlib_name};
use crate::cvc5_expr_smtlib::expr_to_smtlib;

pub(crate) fn append_cvc5_shellout_requires(script: &mut String, requires: &[&Expr]) {
    for req in requires {
        if let Some(smt) = expr_to_smtlib(req) {
            script.push_str(&format!("(assert {smt})\n"));
        }
    }
}

pub(crate) fn append_cvc5_shellout_frame_axioms(
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

pub(crate) fn append_cvc5_shellout_lemma_assumptions(
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

pub(crate) fn append_cvc5_shellout_clause_check(script: &mut String, kind: ClauseKind, smt: &str) {
    match kind {
        ClauseKind::Invariant | ClauseKind::MustNot => {
            script.push_str(&format!("(assert {smt})\n"));
        }
        _ => {
            script.push_str(&format!("(assert (not {smt}))\n"));
        }
    }
}

pub(crate) fn append_cvc5_shellout_constraints(
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
