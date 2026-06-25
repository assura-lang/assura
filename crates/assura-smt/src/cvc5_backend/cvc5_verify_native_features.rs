//! CVC5 native feature-specific verification entry points.

#![cfg_attr(feature = "z3-verify", allow(dead_code))]

use std::collections::HashMap;

use assura_ast::{BinOp, Clause, ClauseKind, Expr, Literal, SpExpr, Spanned};

use crate::VerificationResult;
use crate::cvc5_verify_native_checks::check_validity_cvc5;
use crate::cvc5_verify_native_solver::{Cvc5SolverOpts, new_cvc5_solver};

/// CVC5 implementation of refinement subtype check.
///
/// `{v: T | antecedent} <: {v: T | consequent}`
/// Encodes: (assert antecedent) (assert (not consequent)) (check-sat)
pub(crate) fn check_refinement_subtype_cvc5(
    antecedent: &SpExpr,
    consequent: &SpExpr,
) -> VerificationResult {
    check_validity_cvc5("refinement_subtype", &[antecedent], consequent)
}

/// CVC5 implementation of refinement subtype check with extra context.
pub(crate) fn check_refinement_subtype_with_context_cvc5(
    context: &[SpExpr],
    antecedent: &SpExpr,
    consequent: &SpExpr,
) -> VerificationResult {
    let mut assumptions: Vec<&SpExpr> = context.iter().collect();
    assumptions.push(antecedent);
    check_validity_cvc5("refinement_subtype_ctx", &assumptions, consequent)
}

/// CVC5 implementation of buffer bounds verification.
pub(crate) fn verify_buffer_bounds_cvc5(
    requires: &[SpExpr],
    ensures: &SpExpr,
) -> VerificationResult {
    let assumptions: Vec<&SpExpr> = requires.iter().collect();
    check_validity_cvc5("buffer_bounds", &assumptions, ensures)
}

/// CVC5 implementation of region containment verification.
pub(crate) fn verify_region_containment_cvc5(
    context: &[SpExpr],
    sub_lo: &SpExpr,
    sub_hi: &SpExpr,
    parent_lo: &SpExpr,
    parent_hi: &SpExpr,
) -> VerificationResult {
    let lo_check = Spanned::no_span(Expr::BinOp {
        op: BinOp::Gte,
        lhs: Box::new(sub_lo.clone()),
        rhs: Box::new(parent_lo.clone()),
    });
    let hi_check = Spanned::no_span(Expr::BinOp {
        op: BinOp::Lte,
        lhs: Box::new(sub_hi.clone()),
        rhs: Box::new(parent_hi.clone()),
    });
    let combined = Spanned::no_span(Expr::BinOp {
        op: BinOp::And,
        lhs: Box::new(lo_check),
        rhs: Box::new(hi_check),
    });
    let assumptions: Vec<&SpExpr> = context.iter().collect();
    check_validity_cvc5("region_containment", &assumptions, &combined)
}

/// CVC5 implementation of measure-aware verification.
pub(crate) fn verify_with_measures_cvc5(
    requires: &[SpExpr],
    ensures: &SpExpr,
    _measures: &[crate::measures::MeasureDefinition],
) -> VerificationResult {
    let assumptions: Vec<&SpExpr> = requires.iter().collect();
    check_validity_cvc5("verify_with_measures", &assumptions, ensures)
}

/// CVC5 implementation of decrease verification.
pub(crate) fn verify_decrease_cvc5(
    preconditions: &[SpExpr],
    measure_expr: &SpExpr,
    call_arg_expr: &SpExpr,
    clause_desc: String,
) -> VerificationResult {
    let decrease_check = Spanned::no_span(Expr::BinOp {
        op: BinOp::Lt,
        lhs: Box::new(call_arg_expr.clone()),
        rhs: Box::new(measure_expr.clone()),
    });
    let non_neg = Spanned::no_span(Expr::BinOp {
        op: BinOp::Gte,
        lhs: Box::new(call_arg_expr.clone()),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int(
            "0".to_string(),
        )))),
    });
    let combined = Spanned::no_span(Expr::BinOp {
        op: BinOp::And,
        lhs: Box::new(decrease_check),
        rhs: Box::new(non_neg),
    });
    let assumptions: Vec<&SpExpr> = preconditions.iter().collect();
    check_validity_cvc5(&clause_desc, &assumptions, &combined)
}

/// CVC5 implementation of taint safety verification.
///
/// Matches Z3's `verify_taint_safety_impl` strategy: collects all safety
/// constraints into a single conjunction, negates, and checks once via
/// `solver_outcome_policy`.
pub(crate) fn verify_taint_safety_cvc5(
    taint_labels: &[(String, assura_types::TaintLabel)],
    _validation_fns: &[String],
    sensitive_uses: &[(String, assura_types::TaintLabel)],
) -> VerificationResult {
    use crate::encode_atom_policy::taint_label_to_int;

    if sensitive_uses.is_empty() {
        return VerificationResult::verified("taint_safety (no sensitive uses)");
    }

    let tm = cvc5::TermManager::new();
    let mut solver = new_cvc5_solver(&tm, Cvc5SolverOpts::default());

    let mut var_map: HashMap<String, cvc5::Term> = HashMap::new();

    // Create taint level variables and assert their label values
    for (name, label) in taint_labels {
        let taint_var = tm.mk_const(tm.integer_sort(), &format!("taint_{name}"));
        let label_val = tm.mk_integer(taint_label_to_int(*label));
        let eq = tm.mk_term(cvc5::Kind::Equal, &[taint_var.clone(), label_val]);
        solver.assert_formula(eq);
        var_map.insert(name.clone(), taint_var);
    }

    // Collect safety constraints: each sensitive use must have taint >= required
    let mut safe_terms: Vec<cvc5::Term> = Vec::new();
    for (var_name, required) in sensitive_uses {
        let required_int = tm.mk_integer(taint_label_to_int(*required));
        if let Some(taint_v) = var_map.get(var_name) {
            safe_terms.push(tm.mk_term(cvc5::Kind::Geq, &[taint_v.clone(), required_int]));
        } else {
            // Unknown var: assume trusted (level 2), always safe (matches Z3)
            let trusted = tm.mk_integer(2);
            safe_terms.push(tm.mk_term(cvc5::Kind::Geq, &[trusted, required_int]));
        }
    }

    // Negate the conjunction: if all safe, UNSAT
    let all_safe = if safe_terms.len() == 1 {
        safe_terms.remove(0)
    } else {
        tm.mk_term(cvc5::Kind::And, &safe_terms)
    };
    let negated = tm.mk_term(cvc5::Kind::Not, &[all_safe]);
    solver.assert_formula(negated);

    let sat_result = solver.check_sat();
    let outcome = crate::cvc5_verify_native_checks::cvc5_clause_sat_outcome(
        &sat_result,
        &solver,
        &var_map,
        &[],
    );
    crate::solver_outcome_policy::interpret_clause_check_result(
        "taint_safety",
        &assura_ast::ClauseKind::Ensures,
        outcome,
    )
}

/// CVC5 implementation of feature clause body verification.
///
/// Used by `smt_features::verify_feature_body` when the CVC5 solver is
/// selected. Collects sibling requires as assumptions, checks body validity.
pub(crate) fn verify_feature_body_cvc5(
    parent_name: &str,
    feature_label: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> VerificationResult {
    let desc = crate::verify_labels::feature_clause_desc(parent_name, feature_label);

    // Skip declarative feature clauses (bare uppercase ident)
    if matches!(&body.node, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase()))
    {
        return VerificationResult::unknown_not_encoded(desc, feature_label);
    }

    let requires: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();

    check_validity_cvc5(&desc, &requires, body)
}

/// CVC5 implementation of structural invariant inductive checking.
pub(crate) fn verify_structural_invariant_inductive_cvc5(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    // Skip bare uppercase ident
    if matches!(&body.node, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase()))
    {
        results.push(VerificationResult::unknown_not_encoded(
            crate::verify_labels::feature_clause_desc(parent_name, "structural_invariant"),
            "structural_invariant",
        ));
        return results;
    }

    // Step 1: Establishment (requires => invariant)
    let requires: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let desc1 = crate::verify_labels::feature_clause_desc(
        parent_name,
        "structural_invariant (establishment)",
    );
    results.push(check_validity_cvc5(&desc1, &requires, body));

    // Step 2: Preservation (requires + ensures => invariant)
    let mut assumptions: Vec<&SpExpr> = requires;
    let ensures: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .map(|c| &c.body)
        .collect();
    assumptions.extend(ensures);
    let desc2 = crate::verify_labels::feature_clause_desc(
        parent_name,
        "structural_invariant (preservation)",
    );
    results.push(check_validity_cvc5(&desc2, &assumptions, body));

    results
}
