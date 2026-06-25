//! CVC5 native feature-specific verification entry points.

#![cfg_attr(feature = "z3-verify", allow(dead_code))]

use std::collections::HashMap;

use assura_ast::{BinOp, Clause, ClauseKind, Expr, Literal, SpExpr, Spanned};

use crate::VerificationResult;
use crate::cvc5_verify_native_checks::{check_satisfiability_cvc5, check_validity_cvc5};
use crate::cvc5_verify_native_solver::{Cvc5SolverOpts, cvc5_clause_sat_outcome, new_cvc5_solver};
use crate::measures::{MeasureAxiomTag, MeasureDefinition};

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
///
/// Mirrors Z3's `verify_with_measures_impl`:
/// 1. Creates uninterpreted functions for each measure.
/// 2. Asserts all measure axioms (NonNegative, EmptyIsZero, AppendIncrement,
///    EquivalentTo, EmptyMapEmptySet).
/// 3. Asserts all requires as assumptions.
/// 4. Checks validity of ensures (negate + check-sat).
pub(crate) fn verify_with_measures_cvc5(
    requires: &[SpExpr],
    ensures: &SpExpr,
    measures: &[MeasureDefinition],
) -> VerificationResult {
    use crate::cvc5_collect::collect_cvc5_var_names_from_assumptions;
    use crate::cvc5_native_encoder::{default_cvc5_encoder_state, encode_expr_cvc5};
    use crate::cvc5_verify_native_solver::{assert_cvc5_axioms, build_cvc5_var_map};
    use crate::cvc5_verify_shared::cvc5_encode_failure;

    let tm = cvc5::TermManager::new();
    // Measures add quantified axioms; give the solver more time (match Z3's 5000ms).
    // Enable unsat cores so cvc5_clause_sat_outcome can call get_unsat_assumptions().
    let mut solver = new_cvc5_solver(
        &tm,
        Cvc5SolverOpts {
            unsat_core: true,
            ..Default::default()
        },
    );
    solver.set_option("tlimit", "5000");

    let assumptions: Vec<&SpExpr> = requires.iter().collect();
    let var_names = collect_cvc5_var_names_from_assumptions(&assumptions, ensures);
    let mut var_map = build_cvc5_var_map(&tm, &var_names, &[]);
    let mut enc_state = default_cvc5_encoder_state();

    // Step 1: Encode all measures as uninterpreted functions
    let int_sort = tm.integer_sort();
    let mut func_decls: HashMap<String, cvc5::Term> = HashMap::new();
    for measure in measures {
        let param_sorts: Vec<cvc5::Sort> = measure
            .param_sorts
            .iter()
            .map(|_| int_sort.clone())
            .collect();
        let fun_sort = tm.mk_fun_sort(&param_sorts, int_sort.clone());
        let fun = tm.mk_const(fun_sort, &measure.name);
        func_decls.insert(measure.name.clone(), fun);
    }

    // Step 2: Assert all measure axioms
    for measure in measures {
        if let Some(func) = func_decls.get(&measure.name) {
            assert_measure_axioms_cvc5(&tm, &mut solver, measure, func, &func_decls);
        }
    }

    // Step 3: Assert all requires as assumptions
    for req in &assumptions {
        if let Some(term) = encode_expr_cvc5(&tm, req, &mut var_map, &mut enc_state) {
            solver.assert_formula(term);
        }
    }

    assert_cvc5_axioms(&mut solver, &enc_state.axioms);

    // Step 4: Negate ensures and check validity
    let body_term = match encode_expr_cvc5(&tm, ensures, &mut var_map, &mut enc_state) {
        Some(t) => t,
        None => return cvc5_encode_failure("verify_with_measures"),
    };
    let negated = tm.mk_term(cvc5::Kind::Not, &[body_term]);
    solver.assert_formula(negated);

    let sat_result = solver.check_sat();
    let outcome = cvc5_clause_sat_outcome(&sat_result, &solver, &var_map, &[]);
    crate::solver_outcome_policy::interpret_clause_check_result(
        "verify_with_measures",
        &assura_ast::ClauseKind::Ensures,
        outcome,
    )
}

/// Assert the standard axioms for a measure on the CVC5 solver.
///
/// Mirrors Z3's `assert_measure_axioms` using CVC5 quantifier API.
fn assert_measure_axioms_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    measure: &MeasureDefinition,
    func: &cvc5::Term<'a>,
    all_func_decls: &HashMap<String, cvc5::Term<'a>>,
) {
    let int_sort = tm.integer_sort();
    let zero = tm.mk_integer(0);
    let one = tm.mk_integer(1);

    for axiom in &measure.axioms {
        match &axiom.tag {
            MeasureAxiomTag::NonNegative => {
                // forall xs: measure(xs) >= 0
                let xs_name = crate::encode_atom_policy::measure_ax_xs_name(&measure.name);
                let xs = tm.mk_var(int_sort.clone(), &xs_name);
                let app = tm.mk_term(cvc5::Kind::ApplyUf, &[func.clone(), xs.clone()]);
                let ge_zero = tm.mk_term(cvc5::Kind::Geq, &[app, zero.clone()]);
                let bound = tm.mk_term(cvc5::Kind::VariableList, &[xs]);
                let forall = tm.mk_term(cvc5::Kind::Forall, &[bound, ge_zero]);
                solver.assert_formula(forall);
            }
            MeasureAxiomTag::EmptyIsZero => {
                // measure(empty) == 0
                let empty = tm.mk_const(
                    int_sort.clone(),
                    crate::encode_method_policy::MEASURE_EMPTY_CONST_NAME,
                );
                let app = tm.mk_term(cvc5::Kind::ApplyUf, &[func.clone(), empty]);
                let eq_zero = tm.mk_term(cvc5::Kind::Equal, &[app, zero.clone()]);
                solver.assert_formula(eq_zero);
            }
            MeasureAxiomTag::AppendIncrement => {
                // forall xs, x: measure(append(xs, x)) == measure(xs) + 1
                let append_name = crate::encode_atom_policy::measure_append_uf_name(&measure.name);
                let append_sort =
                    tm.mk_fun_sort(&[int_sort.clone(), int_sort.clone()], int_sort.clone());
                let append_fn = tm.mk_const(append_sort, &append_name);

                let xs_name = crate::encode_atom_policy::measure_ax_xs2_name(&measure.name);
                let x_name = crate::encode_atom_policy::measure_ax_x_name(&measure.name);
                let xs = tm.mk_var(int_sort.clone(), &xs_name);
                let x = tm.mk_var(int_sort.clone(), &x_name);

                let appended = tm.mk_term(cvc5::Kind::ApplyUf, &[append_fn, xs.clone(), x.clone()]);
                let measure_appended = tm.mk_term(cvc5::Kind::ApplyUf, &[func.clone(), appended]);
                let measure_xs = tm.mk_term(cvc5::Kind::ApplyUf, &[func.clone(), xs.clone()]);
                let expected = tm.mk_term(cvc5::Kind::Add, &[measure_xs, one.clone()]);
                let eq = tm.mk_term(cvc5::Kind::Equal, &[measure_appended, expected]);
                let bound = tm.mk_term(cvc5::Kind::VariableList, &[xs, x]);
                let forall = tm.mk_term(cvc5::Kind::Forall, &[bound, eq]);
                solver.assert_formula(forall);
            }
            MeasureAxiomTag::EquivalentTo(other_name) => {
                // forall xs: measure(xs) == other_measure(xs)
                if let Some(other_func) = all_func_decls.get(other_name) {
                    let xs_name = crate::encode_atom_policy::measure_ax_eq_xs_name(&measure.name);
                    let xs = tm.mk_var(int_sort.clone(), &xs_name);
                    let this_app = tm.mk_term(cvc5::Kind::ApplyUf, &[func.clone(), xs.clone()]);
                    let other_app =
                        tm.mk_term(cvc5::Kind::ApplyUf, &[other_func.clone(), xs.clone()]);
                    let eq = tm.mk_term(cvc5::Kind::Equal, &[this_app, other_app]);
                    let bound = tm.mk_term(cvc5::Kind::VariableList, &[xs]);
                    let forall = tm.mk_term(cvc5::Kind::Forall, &[bound, eq]);
                    solver.assert_formula(forall);
                }
            }
            MeasureAxiomTag::EmptyMapEmptySet => {
                // measure(empty_map) == 0
                let empty_map = tm.mk_const(
                    int_sort.clone(),
                    crate::encode_atom_policy::EMPTY_MAP_CONST_NAME,
                );
                let app = tm.mk_term(cvc5::Kind::ApplyUf, &[func.clone(), empty_map]);
                let eq_zero = tm.mk_term(cvc5::Kind::Equal, &[app, zero.clone()]);
                solver.assert_formula(eq_zero);
            }
            MeasureAxiomTag::Custom(_) => {
                // Custom axioms are documentation-only; not encoded automatically.
            }
        }
    }
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
    let outcome = crate::cvc5_verify_native_solver::cvc5_clause_sat_outcome(
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

// -----------------------------------------------------------------------
// #519 STOR.5: Monotonic state (CVC5 parity)
// -----------------------------------------------------------------------

/// CVC5 implementation of monotonic state lattice verification.
pub(crate) fn verify_monotonic_state_cvc5(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    if matches!(&body.node, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase()))
    {
        results.push(VerificationResult::unknown_not_encoded(
            crate::verify_labels::feature_clause_desc(parent_name, "monotonic"),
            "monotonic_state",
        ));
        return results;
    }

    // Step 1: No-decrease check (requires + ensures => body)
    let all_assumptions: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires || c.kind == ClauseKind::Ensures)
        .map(|c| &c.body)
        .collect();
    let desc1 = crate::verify_labels::feature_clause_desc(parent_name, "monotonic (no-decrease)");
    results.push(check_validity_cvc5(&desc1, &all_assumptions, body));

    // Step 2: Body validity under requires only
    let requires: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let desc2 = crate::verify_labels::feature_clause_desc(parent_name, "monotonic (body)");
    results.push(check_validity_cvc5(&desc2, &requires, body));

    results
}

// -----------------------------------------------------------------------
// #517 CONC.4: Lock ordering (CVC5 parity)
// -----------------------------------------------------------------------

/// CVC5 implementation of lock ordering acyclicity verification.
pub(crate) fn verify_lock_ordering_cvc5(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    if matches!(&body.node, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase()))
    {
        results.push(VerificationResult::unknown_not_encoded(
            crate::verify_labels::feature_clause_desc(parent_name, "lock_order (acyclicity)"),
            "lock_ordering",
        ));
        return results;
    }

    // Acyclicity: check that all ordering constraints + body are satisfiable.
    // SAT = consistent (no cycle), UNSAT = cycle (potential deadlock).
    let assumptions: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();

    let desc_acyclic =
        crate::verify_labels::feature_clause_desc(parent_name, "lock_order (acyclicity)");

    // Use SAT check: assert all requires + body, check if satisfiable.
    // SAT = ordering is consistent (Verified), UNSAT = cycle (Counterexample).
    results.push(check_satisfiability_cvc5(&desc_acyclic, &assumptions, body));

    // Body validity under requires
    let requires: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let desc_body = crate::verify_labels::feature_clause_desc(parent_name, "lock_order (body)");
    results.push(check_validity_cvc5(&desc_body, &requires, body));

    results
}

// -----------------------------------------------------------------------
// #518 SEC.2: Constant-time (CVC5 parity)
// -----------------------------------------------------------------------

/// CVC5 implementation of constant-time verification.
pub(crate) fn verify_constant_time_cvc5(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    if matches!(&body.node, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase()))
    {
        results.push(VerificationResult::unknown_not_encoded(
            crate::verify_labels::feature_clause_desc(
                parent_name,
                "constant_time (secret-independence)",
            ),
            "constant_time",
        ));
        return results;
    }

    let requires: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();

    let desc1 = crate::verify_labels::feature_clause_desc(
        parent_name,
        "constant_time (secret-independence)",
    );
    results.push(check_validity_cvc5(&desc1, &requires, body));

    // Step 2: Body validity under requires + ensures.
    let all_assumptions: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires || c.kind == ClauseKind::Ensures)
        .map(|c| &c.body)
        .collect();
    let desc2 = crate::verify_labels::feature_clause_desc(parent_name, "constant_time (body)");
    results.push(check_validity_cvc5(&desc2, &all_assumptions, body));

    results
}

// -----------------------------------------------------------------------
// #520 SEC.3: Secure erasure (CVC5 parity)
// -----------------------------------------------------------------------

/// CVC5 implementation of secure erasure verification.
pub(crate) fn verify_secure_erasure_cvc5(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    if matches!(&body.node, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase()))
    {
        results.push(VerificationResult::unknown_not_encoded(
            crate::verify_labels::feature_clause_desc(parent_name, "secure_erase (coverage)"),
            "secure_erasure",
        ));
        return results;
    }

    // Step 1: Erasure coverage (requires + ensures => body)
    let all_assumptions: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires || c.kind == ClauseKind::Ensures)
        .map(|c| &c.body)
        .collect();
    let desc1 = crate::verify_labels::feature_clause_desc(parent_name, "secure_erase (coverage)");
    results.push(check_validity_cvc5(&desc1, &all_assumptions, body));

    // Step 2: Body validity under requires
    let requires: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let desc2 = crate::verify_labels::feature_clause_desc(parent_name, "secure_erase (body)");
    results.push(check_validity_cvc5(&desc2, &requires, body));

    results
}

// -----------------------------------------------------------------------
// #516 STOR.1: Crash recovery (CVC5 parity)
// -----------------------------------------------------------------------

/// CVC5 implementation of crash recovery verification.
pub(crate) fn verify_crash_recovery_cvc5(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    if matches!(&body.node, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase()))
    {
        results.push(VerificationResult::unknown_not_encoded(
            crate::verify_labels::feature_clause_desc(parent_name, "crash_recovery"),
            "crash_recovery",
        ));
        return results;
    }

    // Step 1: Establishment (requires => recovery invariant)
    let requires: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let desc1 =
        crate::verify_labels::feature_clause_desc(parent_name, "crash_recovery (establishment)");
    results.push(check_validity_cvc5(&desc1, &requires, body));

    // Step 2: Preservation (requires + ensures => recovery invariant)
    let mut all_assumptions: Vec<&SpExpr> = requires;
    let ensures: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .map(|c| &c.body)
        .collect();
    all_assumptions.extend(ensures);
    let desc2 =
        crate::verify_labels::feature_clause_desc(parent_name, "crash_recovery (post-recovery)");
    results.push(check_validity_cvc5(&desc2, &all_assumptions, body));

    results
}

// -----------------------------------------------------------------------
// #521 STOR.3: MVCC isolation (CVC5 parity)
// -----------------------------------------------------------------------

/// CVC5 implementation of MVCC isolation verification.
pub(crate) fn verify_mvcc_isolation_cvc5(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    if matches!(&body.node, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase()))
    {
        results.push(VerificationResult::unknown_not_encoded(
            crate::verify_labels::feature_clause_desc(parent_name, "mvcc_isolation"),
            "mvcc_isolation",
        ));
        return results;
    }

    // Step 1: Snapshot isolation (requires => body)
    let requires: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let desc1 = crate::verify_labels::feature_clause_desc(parent_name, "mvcc_isolation (snapshot)");
    results.push(check_validity_cvc5(&desc1, &requires, body));

    // Step 2: Write-conflict detection (requires + ensures => body)
    let all_assumptions: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires || c.kind == ClauseKind::Ensures)
        .map(|c| &c.body)
        .collect();
    let desc2 =
        crate::verify_labels::feature_clause_desc(parent_name, "mvcc_isolation (write-conflict)");
    results.push(check_validity_cvc5(&desc2, &all_assumptions, body));

    results
}

// -----------------------------------------------------------------------
// #522 SEC.4: Crypto conformance (CVC5 parity)
// -----------------------------------------------------------------------

/// CVC5 implementation of crypto conformance verification.
pub(crate) fn verify_crypto_conformance_cvc5(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    if matches!(&body.node, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase()))
    {
        results.push(VerificationResult::unknown_not_encoded(
            crate::verify_labels::feature_clause_desc(parent_name, "crypto_conformance"),
            "crypto_conformance",
        ));
        return results;
    }

    // Step 1: Parameter constraints (requires => body)
    let requires: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let desc1 =
        crate::verify_labels::feature_clause_desc(parent_name, "crypto_conformance (parameters)");
    results.push(check_validity_cvc5(&desc1, &requires, body));

    // Step 2: Body with ensures context
    let all_assumptions: Vec<&SpExpr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires || c.kind == ClauseKind::Ensures)
        .map(|c| &c.body)
        .collect();
    let desc2 = crate::verify_labels::feature_clause_desc(parent_name, "crypto_conformance (body)");
    results.push(check_validity_cvc5(&desc2, &all_assumptions, body));

    results
}
