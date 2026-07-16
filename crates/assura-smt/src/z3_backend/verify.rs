//! High-level Z3 verification dispatch: clause verification, contract
//! verification, quantified verification, lemma collection, and
//! prophecy/trigger helpers.

use super::encoder::{Encoder, collect_unmodelable_reasons, expr_has_unmodelable_features};
use super::havoc_assume::apply_havoc_assume_z3;
use super::solver::{
    assert_tracked, check_satisfiability, check_validity, clause_desc, enable_unsat_cores,
    z3_clause_sat_outcome,
};
use crate::cache::SessionCache;
use crate::feature_max::{collect_feature_max_constants, derive_narrowings};
use crate::ir::{IrFunction, IrInstr};
use crate::*;
use assura_ast::{Clause, SpExpr};
use z3::{Solver, ast};

// -----------------------------------------------------------------------
// Contract clause verification
// -----------------------------------------------------------------------

/// Type information for parameters and return type, used to add
/// type-level Z3 constraints (e.g., `Nat` implies `>= 0`).
#[derive(Default)]
struct TypeConstraints<'a> {
    params: &'a [assura_ast::Param],
    return_ty: &'a [String],
    /// Named constants (from `feature_max` declarations) to bind in Z3
    /// instead of leaving as free variables.
    constants: &'a [(String, i64)],
    /// Refinement narrowing pairs from `feature_max` declarations.
    /// `feature_max max_X: Nat = V` produces `("X", V)`, meaning any
    /// variable named `X` gets `X <= V` asserted as a background axiom.
    narrowings: &'a [(String, i64)],
    /// Use native Z3 string theory (QF_S/QF_SLIA) instead of integer encoding.
    use_string_theory: bool,
    /// Optional implementation IR body for havoc+assume (#267).
    ir_body: Option<&'a IrFunction>,
    /// Optional `fn #N` block bodies from multi-function IR sidecars.
    ir_blocks: Option<&'a std::collections::HashMap<usize, Vec<IrInstr>>>,
    /// All loaded IR sidecar bodies for cross-function `call` inlining.
    ir_bodies: Option<&'a std::collections::HashMap<String, IrFunction>>,
    /// Layer-0 type environment for type-aware IR encoding.
    type_env: Option<&'a assura_types::TypeEnv>,
    /// Same-file pure callees for ensures-side call equating.
    callee_specs: Option<
        &'a std::collections::HashMap<String, crate::encode_callee_policy::CalleeFunctionalSpec>,
    >,
}

/// Convert a Param's `Option<TypeExpr>` to token vec for SMT type checking.
fn param_ty_tokens(param: &assura_ast::Param) -> Vec<String> {
    crate::prelude_policy::param_type_tokens(param)
}

// Re-use extract_output_return_type and extract_input_params from entry.rs
// (single source of truth, avoids divergence between parallel and non-parallel paths).
#[cfg(test)]
pub(crate) use crate::entry::{extract_input_params, extract_output_return_type};

/// Like `verify_clauses` but also asserts type-level constraints from
/// parameter and return type declarations (e.g., `Nat` → `>= 0`).
///
/// # Limitation: result-field properties (#191)
///
/// The SMT encoder cannot verify ensures clauses that constrain properties
/// of `result` (e.g., `result.length() <= raw.length()`). When the encoder
/// encounters `result`, it creates an unconstrained Z3 integer variable.
/// When it encounters `result.length()`, it creates an uninterpreted
/// function application `__field_len(result)`. Since both are free Z3
/// variables with no relationship to each other or to the function's
/// actual semantics, the solver trivially finds counterexamples.
///
/// This is a fundamental limitation of deductive contract verification
/// without an implementation body: the verifier can only prove ensures
/// clauses that are logical consequences of the requires clauses and
/// type constraints, not arbitrary input-output relationships. Contract
/// languages like Dafny and Verus solve this with havoc+assume encoding,
/// which requires an implementation body to constrain `result`.
///
/// **Workaround**: Write ensures clauses that reference only input
/// variables. See the "Writing Demo Contracts That Z3 Can Verify"
/// section in AGENTS.md for details and examples.
fn verify_clauses_with_types(
    parent_name: &str,
    clauses: &[Clause],
    lemma_defs: &std::collections::HashMap<String, Vec<&SpExpr>>,
    cache: &mut SessionCache,
    results: &mut Vec<VerificationResult>,
    types: &TypeConstraints,
    timeout_ms: u32,
) {
    // One compiler brain: clause partitioning / feature dispatch / frame setup
    // (shared with CVC5 via `clause_policy`; not full expr-encode unification).
    // Step order documented in `prelude_policy::VERIFY_PRELUDE_ORDER`.
    let _order = crate::prelude_policy::verify_prelude_order();
    let (feature_results, prep) = crate::clause_policy::prepare_contract_clauses(
        parent_name,
        clauses,
        types.params,
        types.constants,
    );
    results.extend(feature_results);

    let requires = &prep.requires_clauses;
    let verifiable = &prep.verifiable;
    let ensures_clauses = &prep.ensures_clauses;
    let frame_checker = &prep.frame_checker;
    let param_names = &prep.param_names;

    if verifiable.is_empty() {
        return;
    }

    // ---------------------------------------------------------------
    // Incremental solving: create ONE solver, assert shared requires
    // ONCE, then use push/pop for each clause (#264).
    //
    // For single-clause contracts, the overhead is identical (one
    // push/pop pair). For multi-clause contracts sharing requires
    // assumptions, learned lemmas from earlier clauses benefit later
    // ones.
    // ---------------------------------------------------------------

    let solver = Solver::new();
    let mut solver_params = z3::Params::new();
    solver_params.set_u32("timeout", timeout_ms);
    solver.set_params(&solver_params);
    if crate::prelude_policy::track_requires_unsat_cores(requires.len()) {
        enable_unsat_cores(&solver);
    }
    let mut base_encoder = Encoder::with_string_theory(types.use_string_theory);
    // ADT axioms (forall over uninterpreted tag UFs) are initialized lazily
    // during encoding when a match/constructor pattern needs them (#262).
    base_encoder.init_bitvector_infrastructure();
    if let Some(specs) = types.callee_specs {
        base_encoder.callee_specs.clone_from(specs);
    }
    for param in types.params {
        let pt = param_ty_tokens(param);
        if let Some((width, signed)) = Encoder::fixed_width_bits(&pt) {
            base_encoder.register_fixed_width_param(&param.name, width, signed);
        }
    }
    // #851: result / __result must share fixed-width sort with output type.
    base_encoder.register_fixed_width_return(types.return_ty);

    // Bind named constants so Z3 uses concrete values, not free vars.
    for (name, value) in types.constants {
        let concrete = ast::Int::from_i64(*value);
        base_encoder
            .vars
            .insert(name.clone(), super::encoder::Z3Value::Int(concrete));
    }

    // Register known function names for trigger inference
    for other_clause in clauses {
        crate::trigger_seed_policy::register_trigger_functions_from_expr(
            &other_clause.body,
            &mut base_encoder.trigger_manager,
        );
    }

    let havoc_input = crate::havoc_assume::HavocAssumeInput {
        requires,
        ensures: ensures_clauses,
        return_ty: types.return_ty,
        param_names,
        ir: types.ir_body,
        enc_ctx: crate::ir_encode::IrEncodeContext::new(
            types.type_env,
            types.ir_bodies,
            types.ir_blocks,
        ),
    };
    apply_havoc_assume_z3(&mut base_encoder, &havoc_input);
    for axiom in &base_encoder.background_axioms {
        solver.assert(axiom);
    }
    base_encoder.background_axioms.clear();

    // Assert requires ONCE (#264). Use tracked assertions only when there
    // are multiple requires so unsat-core extraction (#266) can identify
    // which preconditions matter; a single require does not need tracking.
    for (i, req) in requires.iter().enumerate() {
        let req_val = base_encoder.encode_expr(&req.body);
        let req_bool = req_val.as_bool();
        if crate::prelude_policy::track_requires_unsat_cores(requires.len()) {
            let label = format!("req_{i}");
            assert_tracked(&solver, &req_bool, &label);
        } else {
            solver.assert(&req_bool);
        }
    }
    for axiom in &base_encoder.background_axioms {
        solver.assert(axiom);
    }
    base_encoder.background_axioms.clear();

    // Type/constant/narrowing prelude (shared brain with CVC5; Z3 asserts all names).
    use crate::prelude_policy::PreludeConstraint;
    for constraint in crate::prelude_policy::collect_prelude_constraints(
        types.params,
        types.return_ty,
        types.constants,
        types.narrowings,
    ) {
        match constraint {
            PreludeConstraint::NatNonNegative(name) => {
                let p = base_encoder.get_or_create_int(&name);
                let zero = ast::Int::from_i64(0);
                solver.assert(p.ge(&zero));
            }
            PreludeConstraint::BoolZeroOrOne(name) => {
                let p = base_encoder.get_or_create_int(&name);
                let zero = ast::Int::from_i64(0);
                let one = ast::Int::from_i64(1);
                solver.assert(p.ge(&zero));
                solver.assert(p.le(&one));
            }
            // Constants are already bound as concrete ints in the encoder var map at init;
            // do not re-assert equality here (would duplicate prior Z3 behavior).
            PreludeConstraint::ConstantEq(_, _) => {}
            PreludeConstraint::NarrowingLe(name, bound) => {
                let var = base_encoder.get_or_create_int(&name);
                let upper = ast::Int::from_i64(bound);
                solver.assert(var.le(&upper));
            }
        }
    }

    // T044: Inject lemma ensures as assumptions for any `apply` refs (shared selection policy).
    for ensures_body in
        crate::lemma_inject_policy::lemma_ensures_bodies_for_clauses(clauses, lemma_defs)
    {
        let ens_val = base_encoder.encode_expr(ensures_body);
        let ens_bool = ens_val.as_bool();
        solver.assert(&ens_bool);
    }

    // Assert any background axioms from lemma encoding
    for axiom in &base_encoder.background_axioms {
        solver.assert(axiom);
    }
    base_encoder.background_axioms.clear();

    // For each verifiable clause: unmodelable → cache → solver → store (clause_gate_policy order)
    let _gate_order = crate::clause_gate_policy::clause_gate_order();
    for clause in verifiable {
        let desc = clause_desc(parent_name, &clause.kind);

        let has_unmodelable = expr_has_unmodelable_features(&clause.body);
        let reasons = if has_unmodelable {
            collect_unmodelable_reasons(&clause.body)
        } else {
            Vec::new()
        };
        if let Some(skip) =
            crate::clause_gate_policy::unmodelable_precheck_if(&desc, has_unmodelable, &reasons)
        {
            results.push(skip);
            continue;
        }

        let clause_hash =
            crate::clause_gate_policy::clause_session_cache_key(&desc, &clause.kind, &clause.body);
        if let Some(cached) =
            crate::clause_gate_policy::lookup_clause_session_cache(cache, &clause_hash, &desc)
        {
            results.push(cached);
            continue;
        }

        let use_push_pop = crate::prelude_policy::use_incremental_clause_push_pop(verifiable.len());
        if use_push_pop {
            solver.push(); // Save solver state
        }

        let mut clause_encoder = Encoder::with_string_theory(types.use_string_theory);
        clause_encoder.share_encoding_state_from(&base_encoder);
        clause_encoder.init_bitvector_infrastructure();
        for param in types.params {
            let pt = param_ty_tokens(param);
            if let Some((width, signed)) = Encoder::fixed_width_bits(&pt) {
                clause_encoder.register_fixed_width_param(&param.name, width, signed);
            }
        }
        clause_encoder.register_fixed_width_return(types.return_ty);
        for (name, value) in types.constants {
            let concrete = ast::Int::from_i64(*value);
            clause_encoder
                .vars
                .insert(name.clone(), super::encoder::Z3Value::Int(concrete));
        }
        crate::trigger_seed_policy::seed_trigger_manager_from_clauses(
            clauses,
            &mut clause_encoder.trigger_manager,
        );

        // T045 / Tier A3: frame axioms from shared clause_policy (ensures + modifies).
        for var_name in crate::clause_policy::frame_axiom_vars_for_clause(
            frame_checker,
            &clause.kind,
            &clause.body,
            param_names,
        ) {
            let current = clause_encoder.get_or_create_int(&var_name);
            let old_name = crate::encode_atom_policy::old_snapshot_name(&var_name);
            let old_var = clause_encoder.get_or_create_int(&old_name);
            let axiom = current.eq(&old_var);
            solver.assert(&axiom);
        }

        // Encode the clause body
        let clause_val = clause_encoder.encode_expr(&clause.body);
        let clause_bool = clause_val.as_bool();

        // Assert background axioms from this clause's encoding
        for axiom in &clause_encoder.background_axioms {
            solver.assert(axiom);
        }
        clause_encoder.background_axioms.clear();

        let result_before = results.len();
        use crate::clause_policy::ClauseCheckPolarity;
        match crate::clause_policy::clause_check_polarity(&clause.kind) {
            Some(ClauseCheckPolarity::ValidityNegateBody) => {
                solver.assert(clause_bool.not());
                check_validity(&solver, desc, results);
            }
            Some(ClauseCheckPolarity::SatisfiabilityAssertBody) => {
                solver.assert(&clause_bool);
                check_satisfiability(&solver, desc, results);
            }
            // must_not { P }: assert P; UNSAT means P is impossible (verified).
            Some(ClauseCheckPolarity::ValidityAssertBody) => {
                solver.assert(&clause_bool);
                check_validity(&solver, desc, results);
            }
            Some(ClauseCheckPolarity::DecreasesNonNeg) => {
                let zero = ast::Int::from_i64(0);
                let measure = clause_val.as_int(&mut clause_encoder.fresh_counter);
                let non_neg = measure.ge(&zero);
                solver.assert(non_neg.not());
                check_validity(&solver, desc, results);
            }
            None => {}
        }

        // T113: Cache the verification result (coarse tag via clause_gate_policy)
        if let Some(result) = results.get(result_before) {
            crate::clause_gate_policy::store_clause_session_cache(cache, clause_hash, result);
        }

        if use_push_pop {
            solver.pop(1); // Restore solver state
        }
    }
}

// -----------------------------------------------------------------------
// Entry point
// -----------------------------------------------------------------------

/// Verify a quantified formula using Z3.
///
/// Encodes assumptions and the negated quantified body, then checks
/// satisfiability. UNSAT means the formula holds universally.
pub(crate) fn verify_quantified_impl(
    name: &str,
    assumptions: &[SpExpr],
    quantified_body: &SpExpr,
) -> VerificationResult {
    let solver = Solver::new();
    let mut params = z3::Params::new();
    params.set_u32(
        "timeout",
        crate::encode_timeout_policy::DEFAULT_SOLVER_TIMEOUT_MS,
    );
    solver.set_params(&params);

    let mut encoder = Encoder::new();
    encoder.init_adt_infrastructure();

    // Assert assumptions
    for assumption in assumptions {
        let val = encoder.encode_expr(assumption);
        let bool_val = val.as_bool();
        solver.assert(&bool_val);
    }

    // Encode the quantified body
    let body_val = encoder.encode_expr(quantified_body);
    let body_bool = body_val.as_bool();

    // Negate and check: UNSAT means the formula holds
    solver.assert(body_bool.not());

    let outcome = z3_clause_sat_outcome(&solver);
    crate::solver_outcome_policy::interpret_clause_check_result(
        name,
        &assura_ast::ClauseKind::Ensures,
        outcome,
    )
}

pub(crate) fn verify_contract_impl(
    contract_name: &str,
    clauses: &[Clause],
) -> Vec<VerificationResult> {
    verify_contract_impl_with_types(contract_name, clauses, &[], &[], &[])
}

pub(crate) fn verify_contract_impl_with_types(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
) -> Vec<VerificationResult> {
    let ctx = crate::verify_context::ContractVerifyContext {
        contract_name,
        clauses,
        params,
        return_ty,
        constants,
        ir: None,
        callee_specs: None,
    };
    verify_contract_impl_with_types_and_ir(&ctx)
}

pub(crate) fn verify_contract_impl_with_types_and_ir(
    ctx: &crate::verify_context::ContractVerifyContext<'_>,
) -> Vec<VerificationResult> {
    let mut results = Vec::new();
    let mut cache = SessionCache::new();
    // Single-contract API has no TypedFile / sibling lemmas. File-level
    // verify_impl_with_timeout collects lemma_defs via collect_lemma_defs.
    let lemma_defs = std::collections::HashMap::new();
    let narrowings = derive_narrowings(ctx.constants);
    let types = TypeConstraints {
        params: ctx.params,
        return_ty: ctx.return_ty,
        constants: ctx.constants,
        narrowings: &narrowings,
        ir_body: ctx.ir_body(),
        ir_blocks: ctx.ir_blocks(),
        ir_bodies: ctx.ir_bodies(),
        type_env: ctx.type_env(),
        callee_specs: ctx.callee_specs,
        ..Default::default()
    };
    verify_clauses_with_types(
        ctx.contract_name,
        ctx.clauses,
        &lemma_defs,
        &mut cache,
        &mut results,
        &types,
        crate::encode_timeout_policy::DEFAULT_SOLVER_TIMEOUT_MS,
    );
    results
}

pub(crate) fn verify_impl_with_timeout(
    typed: &TypedFile,
    timeout_ms: u64,
    extras: Option<&crate::VerifyFileExtras<'_>>,
) -> Vec<VerificationResult> {
    // Floor short config defaults (often 1s) at DEFAULT_SOLVER_TIMEOUT_MS;
    // honor longer assura.toml / CLI timeout for both clause solvers and
    // advanced passes.
    let clause_timeout = crate::encode_timeout_policy::clause_timeout_ms(timeout_ms);
    let mut results = Vec::new();
    let mut cache = SessionCache::new();
    let ir_bodies = extras.and_then(|e| e.ir_bodies);
    let ir_block_maps = extras.and_then(|e| e.ir_blocks);
    let file_type_env = extras.and_then(|e| e.type_env).or(Some(&typed.type_env));

    // T044: collect all lemma definitions for apply injection
    let lemma_defs = crate::verify_labels::collect_lemma_defs(typed);

    // #180: collect feature_max constants so the encoder binds them
    // to concrete values instead of creating free Z3 variables.
    let constants = collect_feature_max_constants(typed);
    // #188: derive refinement narrowing pairs from feature_max names.
    let narrowings = derive_narrowings(&constants);

    // #213: Use shared job collection (DeclVisitor-based) instead of
    // hand-coded match &decl.node. Same dispatch used by CVC5 and
    // parallel paths.
    let jobs = crate::entry::collect_verification_jobs(typed);
    // Ensures-side call equating: expand pure same-file helpers via their
    // functional ensures (result == expr) instead of free UFs.
    let callee_specs = crate::encode_callee_policy::collect_callee_functional_specs(&jobs);

    for (name, clauses, params, return_ty) in &jobs {
        let ir_body = ir_bodies.and_then(|m| m.get(name.as_str()));
        let ir_blocks = ir_block_maps.and_then(|m| m.get(name.as_str()));

        // #703: Skip ensures clauses referencing result when no IR body
        // is loaded. Emit Unknown instead of producing spurious counterexamples.
        // Only applies when IR loading was attempted (source path provided).
        let has_ir = ir_body.is_some();
        let ir_loading_attempted = extras.is_some_and(|e| e.ir_loading_attempted);
        let skip = crate::entry::verify::unconstrained_result_unknowns(
            name,
            clauses,
            has_ir,
            ir_loading_attempted,
        );
        if !skip.is_empty() {
            let filtered: Vec<assura_ast::Clause> = clauses
                .iter()
                .filter(|c| !crate::entry::verify::ensures_needs_ir_for_unconstrained_result(c))
                .cloned()
                .collect();
            if filtered.iter().any(|c| {
                matches!(
                    c.kind,
                    assura_ast::ClauseKind::Ensures | assura_ast::ClauseKind::Invariant
                )
            }) {
                let types = TypeConstraints {
                    params,
                    return_ty,
                    constants: &constants,
                    narrowings: &narrowings,
                    ir_body,
                    ir_blocks,
                    ir_bodies,
                    type_env: file_type_env,
                    callee_specs: Some(&callee_specs),
                    ..Default::default()
                };
                verify_clauses_with_types(
                    name,
                    &filtered,
                    &lemma_defs,
                    &mut cache,
                    &mut results,
                    &types,
                    clause_timeout,
                );
            }
            results.extend(skip);
            continue;
        }

        let types = TypeConstraints {
            params,
            return_ty,
            constants: &constants,
            narrowings: &narrowings,
            ir_body,
            ir_blocks,
            ir_bodies,
            type_env: file_type_env,
            callee_specs: Some(&callee_specs),
            ..Default::default()
        };
        verify_clauses_with_types(
            name,
            clauses,
            &lemma_defs,
            &mut cache,
            &mut results,
            &types,
            clause_timeout,
        );
    }

    // Run all 5 advanced passes via shared solver-agnostic functions (#214)
    results.extend(crate::entry::run_advanced_passes(typed, timeout_ms));

    results
}

// Helper functions (extract_numeric_arg, resolve_prophecy_vars,
// constrain_prophecy_vars, collect_prophecy_refs) moved to entry.rs
// as shared solver-agnostic code used by both Z3 and CVC5 paths.
// Trigger function registration: `crate::trigger_seed_policy`.

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::{BinOp, Literal, Spanned};

    fn sp(e: Expr) -> SpExpr {
        Spanned::no_span(e)
    }
    fn spb(e: Expr) -> Box<SpExpr> {
        Box::new(sp(e))
    }

    #[test]
    fn extract_output_return_type_nat() {
        let clauses = vec![Clause {
            kind: ClauseKind::Output,
            body: sp(Expr::Raw(vec!["result".into(), ":".into(), "Nat".into()])),
            effect_variables: vec![],
        }];
        let ty = extract_output_return_type(&clauses);
        assert_eq!(ty, vec!["Nat"]);
        assert!(crate::prelude_policy::is_nat_type_tokens(&ty));
    }

    #[test]
    fn extract_output_return_type_non_nat() {
        let clauses = vec![Clause {
            kind: ClauseKind::Output,
            body: sp(Expr::Raw(vec!["result".into(), ":".into(), "Bytes".into()])),
            effect_variables: vec![],
        }];
        let ty = extract_output_return_type(&clauses);
        assert_eq!(ty, vec!["Bytes"]);
        assert!(!crate::prelude_policy::is_nat_type_tokens(&ty));
    }

    #[test]
    fn extract_output_return_type_missing() {
        let clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: sp(Expr::Literal(Literal::Bool(true))),
            effect_variables: vec![],
        }];
        let ty = extract_output_return_type(&clauses);
        assert!(ty.is_empty());
    }

    #[test]
    fn extract_input_params_basic() {
        let clauses = vec![Clause {
            kind: ClauseKind::Input,
            body: sp(Expr::Raw(vec![
                "raw_data".into(),
                ":".into(),
                "Bytes".into(),
            ])),
            effect_variables: vec![],
        }];
        let params = extract_input_params(&clauses);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "raw_data");
        assert_eq!(
            params[0].ty,
            Some(assura_ast::TypeExpr::Named("Bytes".into()))
        );
    }

    #[test]
    fn contract_output_nat_constrains_result() {
        // Verifies #190: output(result: Nat) should make `ensures { result >= 0 }` verified.
        let clauses = vec![
            Clause {
                kind: ClauseKind::Output,
                body: sp(Expr::Raw(vec!["result".into(), ":".into(), "Nat".into()])),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: sp(Expr::BinOp {
                    lhs: spb(Expr::Ident("result".into())),
                    op: BinOp::Gte,
                    rhs: spb(Expr::Literal(Literal::Int("0".into()))),
                }),
                effect_variables: vec![],
            },
        ];
        let output_ty = extract_output_return_type(&clauses);
        let results =
            verify_contract_impl_with_types("TestContract", &clauses, &[], &output_ty, &[]);
        assert_eq!(results.len(), 1, "expected 1 result, got {results:?}");
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "expected Verified, got {:?}",
            results[0]
        );
    }
}
