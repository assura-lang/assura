//! SMT-LIB2 script assembly helpers for the CVC5 shell-out path.

use std::collections::HashSet;

use assura_ast::{ClauseKind, SpExpr};

use crate::cvc5_common::collect_apply_refs_from_expr;
use crate::cvc5_expr_smtlib::expr_to_smtlib;
use crate::cvc5_verify_shared::{Cvc5TypeConstraint, collect_cvc5_type_constraints};
use crate::encode_atom_policy::sanitize_smt_name;

pub(crate) fn append_cvc5_shellout_requires(script: &mut String, requires: &[&SpExpr]) {
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
        let current = sanitize_smt_name(var_name);
        let old = crate::encode_atom_policy::old_snapshot_name(var_name);
        if !vars.contains(&old) {
            script.push_str(&format!("(declare-const {old} Int)\n"));
        }
        script.push_str(&format!("(assert (= {current} {old}))\n"));
    }
}

pub(crate) fn append_cvc5_shellout_lemma_assumptions(
    script: &mut String,
    body: &SpExpr,
    defs: &std::collections::HashMap<String, Vec<&SpExpr>>,
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
    if crate::clause_policy::cvc5_assert_negates_body(&kind) {
        script.push_str(&format!("(assert (not {smt}))\n"));
    } else {
        script.push_str(&format!("(assert {smt})\n"));
    }
}

pub(crate) fn append_cvc5_shellout_constraints(
    script: &mut String,
    vars: &HashSet<String>,
    params: &[assura_ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    narrowings: &[(String, i64)],
) {
    let constraints = collect_cvc5_type_constraints(vars, params, return_ty, constants, narrowings);
    for constraint in constraints {
        match constraint {
            Cvc5TypeConstraint::NatNonNegative(name) => {
                script.push_str(&format!("(assert (>= {name} 0))\n"));
            }
            Cvc5TypeConstraint::ConstantEq(name, value) => {
                script.push_str(&format!("(assert (= {name} {value}))\n"));
            }
            Cvc5TypeConstraint::NarrowingLe(name, value) => {
                script.push_str(&format!("(assert (<= {name} {value}))\n"));
            }
        }
    }
}
