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
pub(crate) fn verify_taint_safety_cvc5(
    taint_labels: &[(String, assura_types::TaintLabel)],
    _validation_fns: &[String],
    sensitive_uses: &[(String, assura_types::TaintLabel)],
) -> VerificationResult {
    use assura_types::TaintLabel;

    let tm = cvc5::TermManager::new();
    let mut solver = new_cvc5_solver(&tm, Cvc5SolverOpts::default());

    let mut var_map: HashMap<String, cvc5::Term> = HashMap::new();
    let zero = tm.mk_integer(0);
    let one = tm.mk_integer(1);
    let two = tm.mk_integer(2);

    // Create taint level variables
    for (name, label) in taint_labels {
        let level = match label {
            TaintLabel::Untrusted => zero.clone(),
            TaintLabel::Validated => one.clone(),
            TaintLabel::Trusted => two.clone(),
        };
        var_map.insert(name.clone(), level);
    }

    // Check sensitive uses: each must have taint level >= required
    for (name, required_label) in sensitive_uses {
        let required_level = match required_label {
            TaintLabel::Untrusted => zero.clone(),
            TaintLabel::Validated => one.clone(),
            TaintLabel::Trusted => two.clone(),
        };
        if let Some(actual) = var_map.get(name) {
            let check = tm.mk_term(cvc5::Kind::Geq, &[actual.clone(), required_level]);
            let neg = tm.mk_term(cvc5::Kind::Not, &[check]);
            // If the negation is satisfiable, the taint check fails
            solver.push(1);
            solver.assert_formula(neg);
            let result = solver.check_sat();
            solver.pop(1);
            if result.is_sat() {
                return VerificationResult::Counterexample {
                    clause_desc: "taint_safety".to_string(),
                    model: format!("{name} has insufficient taint level"),
                    counter_model: None,
                };
            }
        }
    }

    VerificationResult::verified("taint_safety".to_string())
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
        return VerificationResult::Unknown {
            clause_desc: desc,
            reason: format!("{feature_label} not yet encoded in SMT"),
        };
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
        results.push(VerificationResult::Unknown {
            clause_desc: crate::verify_labels::feature_clause_desc(
                parent_name,
                "structural_invariant",
            ),
            reason: "structural_invariant not yet encoded in SMT".into(),
        });
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
