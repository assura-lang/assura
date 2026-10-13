//! SMT-LIB2 script assembly helpers for the CVC5 shell-out path.

use std::collections::HashSet;

use assura_ast::{ClauseKind, SpExpr};

use crate::cvc5_expr_smtlib::expr_to_smtlib;
use crate::cvc5_verify_shared::{Cvc5TypeConstraint, collect_cvc5_type_constraints};
use crate::encode_atom_policy::sanitize_smt_name;
use crate::lemma_inject_policy::collect_apply_refs_from_expr;

/// Quantified Option axioms make CVC5 answer `unknown` on otherwise
/// quantifier-free sat queries. Insert them only when the script uses
/// an `__adt_` symbol, and only once.
pub(crate) fn insert_cvc5_adt_prelude_if_used(script: &mut String) {
    if script.contains("; adt: Option") || !script.contains("__adt_") {
        return;
    }
    let mut prelude = String::new();
    for line in crate::cvc5_adt::cvc5_adt_prelude_lines() {
        prelude.push_str(&line);
        if !line.ends_with('\n') {
            prelude.push('\n');
        }
    }
    if let Some(idx) = script.find('\n') {
        script.insert_str(idx + 1, &prelude);
    } else {
        script.insert_str(0, &prelude);
    }
}

pub(crate) fn append_cvc5_shellout_requires(script: &mut String, requires: &[&SpExpr]) {
    for req in requires {
        let (result, effects) =
            crate::cvc5_expr_smtlib::with_smtlib_side_effects(|| expr_to_smtlib(req));
        inject_side_effects(script, &effects);
        if let Some(smt) = result {
            script.push_str(&format!("(assert {smt})\n"));
        }
    }
}

/// Inject accumulated declarations and axioms from tuple/list encoding.
fn inject_side_effects(script: &mut String, effects: &crate::cvc5_expr_smtlib::SmtlibSideEffects) {
    for decl in &effects.declarations {
        script.push_str(decl);
        script.push('\n');
    }
    for axiom in &effects.assertions {
        script.push_str(axiom);
        script.push('\n');
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
                let (result, effects) =
                    crate::cvc5_expr_smtlib::with_smtlib_side_effects(|| expr_to_smtlib(ens_body));
                inject_side_effects(script, &effects);
                if let Some(smt) = result {
                    script.push_str(&format!("(assert {smt})\n"));
                }
            }
        }
    }
}

pub(crate) fn append_cvc5_shellout_clause_check(script: &mut String, kind: ClauseKind, smt: &str) {
    use crate::clause_policy::ClauseCheckPolarity;

    // Same table as Z3 and CVC5 native (`clause_check_polarity`).
    // `decreases` is not `(not <measure>)`; that term has the wrong sort.
    match crate::clause_policy::clause_check_polarity(&kind) {
        Some(ClauseCheckPolarity::ValidityNegateBody) => {
            script.push_str(&format!("(assert (not {smt}))\n"));
        }
        Some(
            ClauseCheckPolarity::SatisfiabilityAssertBody | ClauseCheckPolarity::ValidityAssertBody,
        ) => {
            script.push_str(&format!("(assert {smt})\n"));
        }
        Some(ClauseCheckPolarity::DecreasesNonNeg) => {
            script.push_str(&format!("(assert (not (>= {smt} 0)))\n"));
        }
        None => {}
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
                script.push_str(&format!(
                    "(assert (and (>= {name} 0) (<= {name} 18446744073709551615)))\n"
                ));
            }
            Cvc5TypeConstraint::IntBounded(name) => {
                script.push_str(&format!(
                    "(assert (and (>= {name} {min}) (<= {name} {max})))\n",
                    min = i64::MIN,
                    max = i64::MAX
                ));
            }
            Cvc5TypeConstraint::BoolZeroOrOne(name) => {
                script.push_str(&format!("(assert (and (>= {name} 0) (<= {name} 1)))\n"));
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

#[cfg(test)]
mod tests {
    use super::{append_cvc5_shellout_clause_check, insert_cvc5_adt_prelude_if_used};
    use assura_ast::ClauseKind;

    fn script_for(kind: ClauseKind) -> String {
        let mut script = String::new();
        append_cvc5_shellout_clause_check(&mut script, kind, "m");
        script
    }

    #[test]
    fn decreases_asserts_measure_nonnegative() {
        assert_eq!(
            script_for(ClauseKind::Decreases),
            "(assert (not (>= m 0)))\n"
        );
    }

    #[test]
    fn adt_prelude_is_omitted_until_an_adt_symbol_appears() {
        let mut script =
            String::from("(set-logic ALL)\n(declare-const n Int)\n(assert (> n 100))\n");
        insert_cvc5_adt_prelude_if_used(&mut script);
        assert!(
            !script.contains("forall"),
            "quantified Option axioms must stay out of a quantifier-free query: {script}"
        );
        script.push_str("(assert (__adt_tag_Option n))\n");
        insert_cvc5_adt_prelude_if_used(&mut script);
        assert!(script.contains("forall"), "{script}");
        let once = script.clone();
        insert_cvc5_adt_prelude_if_used(&mut script);
        assert_eq!(script, once, "prelude must not be inserted twice");
    }

    #[test]
    fn ensures_negates_and_invariant_asserts() {
        assert_eq!(script_for(ClauseKind::Ensures), "(assert (not m))\n");
        assert_eq!(script_for(ClauseKind::Rule), "(assert (not m))\n");
        assert_eq!(script_for(ClauseKind::Invariant), "(assert m)\n");
        assert_eq!(script_for(ClauseKind::MustNot), "(assert m)\n");
    }
}
