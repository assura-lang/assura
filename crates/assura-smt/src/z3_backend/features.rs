//! Feature-specific Z3 verification: refinement subtyping, buffer bounds,
//! region containment, taint safety, measures, and termination checking.

use super::encoder::Encoder;
use super::solver::check_validity;
use crate::measures::MeasureDefinition;
use crate::*;
use assura_ast::SpExpr;

use std::collections::HashMap;
use z3::{Solver, ast};

// -----------------------------------------------------------------------
// Refinement subtype checking (T039)
// -----------------------------------------------------------------------

/// Check `{v: T | antecedent} <: {v: T | consequent}`.
///
/// Encodes: assert antecedent, assert NOT consequent, check-sat.
/// UNSAT => Verified, SAT => Counterexample.
pub(crate) fn check_refinement_subtype_impl(
    antecedent: &SpExpr,
    consequent: &SpExpr,
) -> VerificationResult {
    let solver = Solver::new();
    let mut params = z3::Params::new();
    params.set_u32("timeout", 1000);
    solver.set_params(&params);

    let mut encoder = Encoder::new();

    // Assert the antecedent (P)
    let ante_val = encoder.encode_expr(antecedent);
    let ante_bool = ante_val.as_bool();
    solver.assert(&ante_bool);

    // Assert NOT consequent (¬Q)
    let cons_val = encoder.encode_expr(consequent);
    let cons_bool = cons_val.as_bool();
    solver.assert(cons_bool.not());

    // Check satisfiability: UNSAT = P => Q always holds
    let mut results = Vec::new();
    check_validity(&solver, "refinement_subtype".into(), &mut results);
    results
        .into_iter()
        .next()
        .unwrap_or(VerificationResult::Unknown {
            clause_desc: "refinement_subtype".into(),
            reason: "no result from solver".into(),
        })
}

/// Check refinement subtyping with additional context assumptions.
pub(crate) fn check_refinement_subtype_with_context_impl(
    context: &[SpExpr],
    antecedent: &SpExpr,
    consequent: &SpExpr,
) -> VerificationResult {
    let solver = Solver::new();
    let mut params = z3::Params::new();
    params.set_u32("timeout", 1000);
    solver.set_params(&params);

    let mut encoder = Encoder::new();

    // Assert all context assumptions
    for ctx_expr in context {
        let val = encoder.encode_expr(ctx_expr);
        let bool_val = val.as_bool();
        solver.assert(&bool_val);
    }

    // Assert the antecedent (P)
    let ante_val = encoder.encode_expr(antecedent);
    let ante_bool = ante_val.as_bool();
    solver.assert(&ante_bool);

    // Assert NOT consequent (¬Q)
    let cons_val = encoder.encode_expr(consequent);
    let cons_bool = cons_val.as_bool();
    solver.assert(cons_bool.not());

    // Check satisfiability
    let mut results = Vec::new();
    check_validity(
        &solver,
        "refinement_subtype_with_context".into(),
        &mut results,
    );
    results
        .into_iter()
        .next()
        .unwrap_or(VerificationResult::Unknown {
            clause_desc: "refinement_subtype_with_context".into(),
            reason: "no result from solver".into(),
        })
}

// -----------------------------------------------------------------------
// MEM.1: Buffer bounds and region containment (T046)
// -----------------------------------------------------------------------

/// Verify buffer bounds safety.
///
/// Models buffer capacity as a non-negative integer. Asserts all
/// requires as assumptions, then checks the ensures clause validity.
pub(crate) fn verify_buffer_bounds_impl(
    requires: &[SpExpr],
    ensures: &SpExpr,
) -> VerificationResult {
    let solver = Solver::new();
    let mut params = z3::Params::new();
    params.set_u32("timeout", 1000);
    solver.set_params(&params);
    let mut encoder = Encoder::new();

    // Assert all requires as assumptions
    for req in requires {
        let val = encoder.encode_expr(req);
        let bool_val = val.as_bool();
        solver.assert(&bool_val);
    }

    // Assert NOT ensures (validity check: UNSAT = valid)
    let ensures_val = encoder.encode_expr(ensures);
    let ensures_bool = ensures_val.as_bool();
    solver.assert(ensures_bool.not());

    let mut results = Vec::new();
    check_validity(&solver, "buffer_bounds".into(), &mut results);
    results
        .into_iter()
        .next()
        .unwrap_or(VerificationResult::Unknown {
            clause_desc: "buffer_bounds".into(),
            reason: "no result from solver".into(),
        })
}

/// Verify region containment via SMT.
///
/// Encoding: `forall i: (sub_lo <= i and i < sub_hi) => (parent_lo <= i and i < parent_hi)`
///
/// We negate this and check for SAT. UNSAT = containment holds.
pub(crate) fn verify_region_containment_impl(
    context: &[SpExpr],
    sub_lo: &SpExpr,
    sub_hi: &SpExpr,
    parent_lo: &SpExpr,
    parent_hi: &SpExpr,
) -> VerificationResult {
    let solver = Solver::new();
    let mut params = z3::Params::new();
    params.set_u32("timeout", 1000);
    solver.set_params(&params);
    let mut encoder = Encoder::new();

    // Assert context assumptions
    for ctx_expr in context {
        let val = encoder.encode_expr(ctx_expr);
        let bool_val = val.as_bool();
        solver.assert(&bool_val);
    }

    // Encode bounds
    let sub_lo_val = encoder
        .encode_expr(sub_lo)
        .as_int(&mut encoder.fresh_counter);
    let sub_hi_val = encoder
        .encode_expr(sub_hi)
        .as_int(&mut encoder.fresh_counter);
    let parent_lo_val = encoder
        .encode_expr(parent_lo)
        .as_int(&mut encoder.fresh_counter);
    let parent_hi_val = encoder
        .encode_expr(parent_hi)
        .as_int(&mut encoder.fresh_counter);

    // Create bound variable for the quantifier
    let i = ast::Int::new_const("i");

    // sub_lo <= i and i < sub_hi
    let in_sub = ast::Bool::and(&[&sub_lo_val.le(&i), &i.lt(&sub_hi_val)]);

    // parent_lo <= i and i < parent_hi
    let in_parent = ast::Bool::and(&[&parent_lo_val.le(&i), &i.lt(&parent_hi_val)]);

    // forall i: in_sub => in_parent
    let containment = in_sub.implies(&in_parent);
    let forall = ast::forall_const(&[&i], &[], &containment);

    // Negate: exists i such that in_sub and NOT in_parent
    solver.assert(forall.not());

    let mut results = Vec::new();
    check_validity(&solver, "region_containment".into(), &mut results);
    results
        .into_iter()
        .next()
        .unwrap_or(VerificationResult::Unknown {
            clause_desc: "region_containment".into(),
            reason: "no result from solver".into(),
        })
}

// -----------------------------------------------------------------------
// SEC.1: Taint tracking (T047)
// -----------------------------------------------------------------------

/// Map a TaintLabel to its Z3 integer encoding.
///
/// Lattice: Untrusted(0) < Validated(1) < Trusted(2).
fn taint_label_to_int(label: assura_types::TaintLabel) -> i64 {
    match label {
        assura_types::TaintLabel::Untrusted => 0,
        assura_types::TaintLabel::Validated => 1,
        assura_types::TaintLabel::Trusted => 2,
    }
}

/// Verify taint safety via Z3.
///
/// Creates integer variables for each taint-labeled variable, constrains
/// them to their declared label value, and checks that every sensitive
/// use meets its required minimum taint level.
///
/// The encoding:
/// - For each `(var, label)` in `taint_labels`: assert `taint_var == label_int`
/// - For each `(var, required)` in `sensitive_uses`: assert NOT `taint_var >= required_int`
///   (if UNSAT, the taint safety holds; if SAT, there is a violation)
pub(crate) fn verify_taint_safety_impl(
    taint_labels: &[(String, assura_types::TaintLabel)],
    _validation_fns: &[String],
    sensitive_uses: &[(String, assura_types::TaintLabel)],
) -> VerificationResult {
    let solver = Solver::new();
    let mut params = z3::Params::new();
    params.set_u32("timeout", 1000);
    solver.set_params(&params);

    // Create taint level variables for each labeled variable
    let mut taint_vars: HashMap<String, ast::Int> = HashMap::new();
    for (name, label) in taint_labels {
        let v = ast::Int::new_const(format!("taint_{name}").as_str());
        let label_val = ast::Int::from_i64(taint_label_to_int(*label));
        solver.assert(v.eq(&label_val));
        taint_vars.insert(name.clone(), v);
    }

    if sensitive_uses.is_empty() {
        return VerificationResult::verified("taint_safety (no sensitive uses)");
    }

    // For each sensitive use, check taint_var >= required
    // We negate the conjunction: if all sensitive uses are safe, UNSAT
    let mut safe_constraints = Vec::new();
    for (var_name, required) in sensitive_uses {
        let required_int = ast::Int::from_i64(taint_label_to_int(*required));
        if let Some(taint_v) = taint_vars.get(var_name) {
            // Safe if taint level >= required level
            safe_constraints.push(taint_v.ge(&required_int));
        } else {
            // Unknown var: assume trusted (level 2), always safe
            let trusted = ast::Int::from_i64(2);
            safe_constraints.push(trusted.ge(&required_int));
        }
    }

    // Assert negation: at least one constraint is NOT safe
    let safe_refs: Vec<&ast::Bool> = safe_constraints.iter().collect();
    let all_safe = ast::Bool::and(&safe_refs);
    solver.assert(all_safe.not());

    let mut results = Vec::new();
    check_validity(&solver, "taint_safety".into(), &mut results);
    results
        .into_iter()
        .next()
        .unwrap_or(VerificationResult::Unknown {
            clause_desc: "taint_safety".into(),
            reason: "no result from solver".into(),
        })
}

// -----------------------------------------------------------------------
// T054: Measure encoding as uninterpreted functions
// -----------------------------------------------------------------------

/// Encode a measure as an uninterpreted function in Z3.
///
/// Returns the Z3 function declaration (`FuncDecl`) for the measure.
/// The function takes one integer argument (representing the collection)
/// and returns an integer (for Nat measures) or integer (for Set measures,
/// modeled as integers in this encoding).
fn encode_measure_as_uf(measure: &MeasureDefinition) -> z3::FuncDecl {
    let int_sort = z3::Sort::int();

    // All parameters are modeled as integers (collections and maps are
    // uninterpreted, represented by integer identifiers)
    let param_sorts: Vec<&z3::Sort> = measure.param_sorts.iter().map(|_| &int_sort).collect();

    // Return sort: Nat and Set are both modeled as integers
    z3::FuncDecl::new(measure.name.as_str(), &param_sorts, &int_sort)
}

/// Assert the standard axioms for a measure on the given solver.
///
/// Uses quantified formulas over an uninterpreted integer variable to
/// express properties like non-negativity and empty-collection behavior.
fn assert_measure_axioms(
    solver: &Solver,
    measure: &MeasureDefinition,
    func_decl: &z3::FuncDecl,
    all_func_decls: &HashMap<String, z3::FuncDecl>,
) {
    let zero = ast::Int::from_i64(0);

    for axiom in &measure.axioms {
        match &axiom.tag {
            MeasureAxiomTag::NonNegative => {
                // forall xs: measure(xs) >= 0
                let xs = ast::Int::new_const(
                    crate::encode_atom_policy::measure_ax_xs_name(&measure.name).as_str(),
                );
                let app = func_decl.apply(&[&xs]);
                let Some(app_int) = app.as_int() else {
                    continue;
                };
                let ge_zero = app_int.ge(&zero);
                let forall = ast::forall_const(&[&xs], &[], &ge_zero);
                solver.assert(&forall);
            }
            MeasureAxiomTag::EmptyIsZero => {
                // measure(empty) == 0, where empty is represented as a
                // distinguished constant
                let empty = ast::Int::new_const("__empty");
                let app = func_decl.apply(&[&empty]);
                let Some(app_int) = app.as_int() else {
                    continue;
                };
                let eq_zero = app_int.eq(&zero);
                solver.assert(&eq_zero);
            }
            MeasureAxiomTag::AppendIncrement => {
                // forall xs, x: measure(append(xs, x)) == measure(xs) + 1
                // We model append as a fresh uninterpreted function
                let int_sort = z3::Sort::int();
                let append_fn = z3::FuncDecl::new(
                    crate::encode_atom_policy::measure_append_uf_name(&measure.name),
                    &[&int_sort, &int_sort],
                    &int_sort,
                );
                let xs = ast::Int::new_const(
                    crate::encode_atom_policy::measure_ax_xs2_name(&measure.name).as_str(),
                );
                let x = ast::Int::new_const(
                    crate::encode_atom_policy::measure_ax_x_name(&measure.name).as_str(),
                );
                let appended = append_fn.apply(&[&xs, &x]);
                let measure_appended = func_decl.apply(&[&appended]);
                let measure_xs = func_decl.apply(&[&xs]);
                let one = ast::Int::from_i64(1);
                let Some(measure_appended_int) = measure_appended.as_int() else {
                    continue;
                };
                let Some(measure_xs_int) = measure_xs.as_int() else {
                    continue;
                };
                let expected = ast::Int::add(&[&measure_xs_int, &one]);
                let eq = measure_appended_int.eq(&expected);
                let forall = ast::forall_const(&[&xs, &x], &[], &eq);
                solver.assert(&forall);
            }
            MeasureAxiomTag::EquivalentTo(other_name) => {
                // forall xs: measure(xs) == other_measure(xs)
                if let Some(other_decl) = all_func_decls.get(other_name) {
                    let xs = ast::Int::new_const(
                        crate::encode_atom_policy::measure_ax_eq_xs_name(&measure.name).as_str(),
                    );
                    let this_app = func_decl.apply(&[&xs]);
                    let other_app = other_decl.apply(&[&xs]);
                    let Some(this_int) = this_app.as_int() else {
                        continue;
                    };
                    let Some(other_int) = other_app.as_int() else {
                        continue;
                    };
                    let eq = this_int.eq(&other_int);
                    let forall = ast::forall_const(&[&xs], &[], &eq);
                    solver.assert(&forall);
                }
            }
            MeasureAxiomTag::EmptyMapEmptySet => {
                // measure(empty_map) == empty_set
                // Both are modeled as integers; empty_map and empty_set
                // map to the same distinguished constant __empty, so
                // measure(__empty) == 0 (using the empty constant).
                let empty_map = ast::Int::new_const("__empty_map");
                let app = func_decl.apply(&[&empty_map]);
                let Some(app_int) = app.as_int() else {
                    continue;
                };
                let eq_zero = app_int.eq(&zero);
                solver.assert(&eq_zero);
            }
            MeasureAxiomTag::Custom(_desc) => {
                // Custom axioms are not encoded automatically; they serve
                // as documentation and can be extended in the future.
            }
        }
    }
}

/// Verify a contract with measure-enriched SMT context.
///
/// 1. Creates uninterpreted functions for each measure.
/// 2. Asserts all measure axioms.
/// 3. Asserts all requires as assumptions.
/// 4. Checks validity of ensures (negate + check-sat).
pub(crate) fn verify_with_measures_impl(
    requires: &[SpExpr],
    ensures: &SpExpr,
    measures: &[MeasureDefinition],
) -> VerificationResult {
    let solver = Solver::new();
    // Measures add quantified axioms; give the solver more time
    let mut params = z3::Params::new();
    params.set_u32("timeout", 5000);
    solver.set_params(&params);
    let mut encoder = Encoder::new();

    // Step 1: Encode all measures as uninterpreted functions
    let mut func_decls: HashMap<String, z3::FuncDecl> = HashMap::new();
    for measure in measures {
        let decl = encode_measure_as_uf(measure);
        func_decls.insert(measure.name.clone(), decl);
    }

    // Step 2: Assert all measure axioms
    for measure in measures {
        if let Some(decl) = func_decls.get(&measure.name) {
            assert_measure_axioms(&solver, measure, decl, &func_decls);
        }
    }

    // Step 3: Assert all requires as assumptions
    for req in requires {
        let val = encoder.encode_expr(req);
        let bool_val = val.as_bool();
        solver.assert(&bool_val);
    }

    // Step 4: Negate ensures and check validity
    let ensures_val = encoder.encode_expr(ensures);
    let ensures_bool = ensures_val.as_bool();
    solver.assert(ensures_bool.not());

    let mut results = Vec::new();
    check_validity(&solver, "verify_with_measures".into(), &mut results);
    results
        .into_iter()
        .next()
        .unwrap_or(VerificationResult::Unknown {
            clause_desc: "verify_with_measures".into(),
            reason: "no result from solver".into(),
        })
}

// -----------------------------------------------------------------------
// Termination (decreases) verification
// -----------------------------------------------------------------------

/// Verify that a measure expression strictly decreases at a call site.
///
/// Encodes: `preconditions => (call_arg < measure) && (call_arg >= 0)`
/// by asserting preconditions, then checking that `NOT (call_arg < measure && call_arg >= 0)`
/// is UNSAT.
pub(crate) fn verify_decrease_impl(
    preconditions: &[SpExpr],
    measure_expr: &SpExpr,
    call_arg_expr: &SpExpr,
    clause_desc: String,
) -> VerificationResult {
    let solver = Solver::new();
    let mut params = z3::Params::new();
    params.set_u32("timeout", 2000);
    solver.set_params(&params);
    let mut encoder = Encoder::new();

    // Assert preconditions
    for pre in preconditions {
        let val = encoder.encode_expr(pre);
        let bool_val = val.as_bool();
        solver.assert(&bool_val);
    }

    // Encode measure and call-site argument
    let measure_val = encoder.encode_expr(measure_expr);
    let call_val = encoder.encode_expr(call_arg_expr);

    let measure_int = measure_val.as_int(&mut encoder.fresh_counter);
    let call_int = call_val.as_int(&mut encoder.fresh_counter);
    let zero = z3::ast::Int::from_i64(0);

    // The property to verify: call_arg < measure AND call_arg >= 0
    let decreases = call_int.lt(&measure_int);
    let non_negative = call_int.ge(&zero);
    let property = z3::ast::Bool::and(&[&decreases, &non_negative]);

    // Negate and check
    solver.assert(property.not());

    let mut results = Vec::new();
    check_validity(&solver, clause_desc, &mut results);
    results
        .into_iter()
        .next()
        .unwrap_or(VerificationResult::Unknown {
            clause_desc: "decrease_check".into(),
            reason: "no result from solver".into(),
        })
}
