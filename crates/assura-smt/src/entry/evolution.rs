//! Incremental contract evolution checks (old vs new clauses).

use assura_ast::{
    BindDecl, Clause, ClauseKind, ContractDecl, DeclVisitor, ExternDecl, FnDef, SpExpr,
};

use crate::result::VerificationResult;

// ---------------------------------------------------------------------------
// Incremental contract evolution (#199)
// ---------------------------------------------------------------------------

/// Result of a contract evolution check.
#[derive(Debug, Clone)]
pub struct EvolutionResult {
    /// Name of the contract being checked.
    pub contract_name: String,
    /// Precondition weakening check: every input valid under the old contract
    /// must be valid under the new contract.
    pub precondition_weakening: VerificationResult,
    /// Postcondition strengthening check: every guarantee of the new contract
    /// must imply the old guarantee.
    pub postcondition_strengthening: VerificationResult,
}

/// Verify that a contract evolution is backward-compatible.
///
/// Given an old and new version of a contract's clauses, checks:
/// 1. **Precondition weakening**: `old_requires => new_requires`
///    (the new contract accepts at least everything the old one did)
/// 2. **Postcondition strengthening**: `new_ensures => old_ensures`
///    (the new contract's guarantees are at least as strong)
///
/// Both are standard Z3 validity checks.
pub fn verify_evolution(
    contract_name: &str,
    old_clauses: &[Clause],
    new_clauses: &[Clause],
) -> EvolutionResult {
    // Collect requires and ensures from both versions
    let old_requires: Vec<&SpExpr> = old_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let new_requires: Vec<&SpExpr> = new_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let old_ensures: Vec<&SpExpr> = old_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .map(|c| &c.body)
        .collect();
    let new_ensures: Vec<&SpExpr> = new_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .map(|c| &c.body)
        .collect();

    // #855: reuse input params / output type from either version for prelude.
    let mut typed_params = crate::entry::extract_input_params(old_clauses);
    if typed_params.is_empty() {
        typed_params = crate::entry::extract_input_params(new_clauses);
    }
    let return_ty = {
        let old_ty = crate::entry::extract_output_return_type(old_clauses);
        if old_ty.is_empty() {
            crate::entry::extract_output_return_type(new_clauses)
        } else {
            old_ty
        }
    };

    // ---- Precondition weakening: old_requires => new_requires ----
    // All old preconditions must imply all new preconditions.
    // If old has no requires, it accepts everything, so new must also accept
    // everything (new_requires must be trivially true).
    // If new has no requires, it accepts everything, so weakening holds trivially.
    let precondition_weakening = if new_requires.is_empty() {
        // New accepts everything; weakening holds trivially
        VerificationResult::verified(format!("{contract_name}: precondition weakening"))
    } else {
        check_implication(
            &old_requires,
            &new_requires,
            &format!("{contract_name}: precondition weakening"),
            &typed_params,
            &return_ty,
        )
    };

    // ---- Postcondition strengthening: new_ensures => old_ensures ----
    // All new postconditions must imply all old postconditions.
    // If old has no ensures, there are no guarantees to maintain, so
    // strengthening holds trivially.
    // If new has no ensures but old does, strengthening fails (lost guarantees).
    let postcondition_strengthening = if old_ensures.is_empty() {
        // Old had no guarantees; any new guarantees are fine
        VerificationResult::verified(format!("{contract_name}: postcondition strengthening"))
    } else if new_ensures.is_empty() {
        // Old had guarantees, new dropped them
        VerificationResult::Counterexample {
            clause_desc: format!("{contract_name}: postcondition strengthening"),
            model: "new contract drops all ensures clauses from old contract".into(),
            counter_model: None,
        }
    } else {
        check_implication(
            &new_ensures,
            &old_ensures,
            &format!("{contract_name}: postcondition strengthening"),
            &typed_params,
            &return_ty,
        )
    };

    EvolutionResult {
        contract_name: contract_name.to_string(),
        precondition_weakening,
        postcondition_strengthening,
    }
}

/// Check that all antecedents together imply all consequents together.
///
/// Encodes: `(and antecedents) => (and consequents)` via
/// `(assert antecedents) (assert (not (and consequents))) (check-sat)`
/// UNSAT = implication holds.
///
/// Applies shared Nat / fixed-width prelude from `input` clauses present in
/// either clause set so evolution matches normal verify typing (#855).
fn check_implication(
    antecedents: &[&SpExpr],
    consequents: &[&SpExpr],
    desc: &str,
    typed_params: &[assura_ast::Param],
    return_ty: &[String],
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        use crate::z3_backend::encoder::{Encoder, expr_has_unmodelable_features};
        use crate::z3_backend::solver::check_validity;
        use z3::Solver;

        // Check if any expressions have unmodelable features
        let all_exprs: Vec<&&SpExpr> = antecedents.iter().chain(consequents.iter()).collect();
        for expr in &all_exprs {
            if expr_has_unmodelable_features(expr) {
                return VerificationResult::Unknown {
                    clause_desc: desc.to_string(),
                    reason: "clause uses features not yet encoded in SMT".into(),
                };
            }
        }

        let solver = Solver::new();
        let mut params = z3::Params::new();
        params.set_u32("timeout", 2000);
        solver.set_params(&params);
        let mut encoder = Encoder::new();
        encoder.init_bitvector_infrastructure();

        // #855: register fixed-width params/result and assert Nat non-negativity
        // from shared prelude_policy (same helpers as normal verify).
        for param in typed_params {
            let pt = crate::prelude_policy::param_type_tokens(param);
            if let Some((width, signed)) = Encoder::fixed_width_bits(&pt) {
                encoder.register_fixed_width_param(&param.name, width, signed);
            }
        }
        encoder.register_fixed_width_return(return_ty);

        let prelude =
            crate::prelude_policy::collect_prelude_constraints(typed_params, return_ty, &[], &[]);
        for c in &prelude {
            match c {
                crate::prelude_policy::PreludeConstraint::NatNonNegative(name) => {
                    let v = encoder.get_or_create_int(name);
                    let zero = z3::ast::Int::from_i64(0);
                    solver.assert(v.ge(&zero));
                }
                crate::prelude_policy::PreludeConstraint::BoolZeroOrOne(name) => {
                    let v = encoder.get_or_create_int(name);
                    let zero = z3::ast::Int::from_i64(0);
                    let one = z3::ast::Int::from_i64(1);
                    solver.assert(v.ge(&zero));
                    solver.assert(v.le(&one));
                }
                crate::prelude_policy::PreludeConstraint::ConstantEq(name, value) => {
                    let v = encoder.get_or_create_int(name);
                    solver.assert(v.eq(z3::ast::Int::from_i64(*value)));
                }
                crate::prelude_policy::PreludeConstraint::NarrowingLe(name, bound) => {
                    let v = encoder.get_or_create_int(name);
                    solver.assert(v.le(z3::ast::Int::from_i64(*bound)));
                }
            }
        }

        // Assert all antecedents
        for expr in antecedents {
            let val = encoder.encode_expr(expr);
            solver.assert(val.as_bool());
        }
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        encoder.background_axioms.clear();

        // Negate conjunction of consequents
        // If there is only one consequent, negate it directly.
        // If multiple, negate their conjunction (not(c1 && c2 && ...)).
        if consequents.len() == 1 {
            let val = encoder.encode_expr(consequents[0]);
            let bool_val = val.as_bool();
            for axiom in &encoder.background_axioms {
                solver.assert(axiom);
            }
            solver.assert(bool_val.not());
        } else {
            // Build conjunction of all consequents, then negate
            let mut conjunction_parts = Vec::new();
            for expr in consequents {
                let val = encoder.encode_expr(expr);
                conjunction_parts.push(val.as_bool());
            }
            for axiom in &encoder.background_axioms {
                solver.assert(axiom);
            }
            let refs: Vec<&z3::ast::Bool> = conjunction_parts.iter().collect();
            let conjunction = z3::ast::Bool::and(&refs);
            solver.assert(conjunction.not());
        }

        let mut results = Vec::new();
        check_validity(&solver, desc.to_string(), &mut results);
        results
            .into_iter()
            .next()
            .unwrap_or_else(|| VerificationResult::no_solver_result(desc))
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        let _ = (antecedents, consequents, typed_params, return_ty);
        VerificationResult::Unknown {
            clause_desc: desc.to_string(),
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }
}

/// Verify evolution of all matching contracts between two parsed files.
///
/// Matches contracts by name between old and new files. For each pair,
/// runs the precondition weakening and postcondition strengthening checks.
/// Returns results for all matched contracts plus warnings for removed contracts.
pub fn verify_file_evolution(
    old_source: &assura_ast::SourceFile,
    new_source: &assura_ast::SourceFile,
) -> Vec<EvolutionResult> {
    fn collect_contracts(source: &assura_ast::SourceFile) -> Vec<(String, Vec<Clause>)> {
        struct Collect(Vec<(String, Vec<Clause>)>);
        impl DeclVisitor for Collect {
            fn visit_contract(&mut self, c: &ContractDecl) {
                self.0.push((c.name.clone(), c.clauses.clone()));
            }
            fn visit_fn_def(&mut self, f: &FnDef) {
                self.0.push((f.name.clone(), f.clauses.clone()));
            }
            fn visit_extern(&mut self, e: &ExternDecl) {
                self.0.push((e.name.clone(), e.clauses.clone()));
            }
            fn visit_bind(&mut self, b: &BindDecl) {
                self.0.push((b.name.clone(), b.clauses.clone()));
            }
        }
        let mut c = Collect(Vec::new());
        assura_ast::walk_decls(&mut c, &source.decls);
        c.0
    }

    let old_contracts = collect_contracts(old_source);
    let new_contracts = collect_contracts(new_source);

    let new_map: std::collections::HashMap<&str, &[Clause]> = new_contracts
        .iter()
        .map(|(name, clauses)| (name.as_str(), clauses.as_slice()))
        .collect();

    let mut results = Vec::new();

    for (name, old_clauses) in &old_contracts {
        if let Some(new_clauses) = new_map.get(name.as_str()) {
            results.push(verify_evolution(name, old_clauses, new_clauses));
        }
        // Contracts removed in new version: no evolution check needed
        // (handled by the structural diff in the CLI)
    }

    results
}
