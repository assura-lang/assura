//! High-level Z3 verification dispatch: clause verification, contract
//! verification, quantified verification, lemma collection, and
//! prophecy/trigger helpers.

use super::encoder::{Encoder, collect_unmodelable_reasons, expr_has_unmodelable_features};
use super::solver::extract_counter_model;
use super::solver::{
    assert_tracked, check_satisfiability, check_validity, clause_desc, enable_unsat_cores,
};
use crate::cache::SessionCache;
use crate::*;
use assura_parser::ast::{BlockKind, Clause};
use z3::{SatResult, Solver, ast};

// -----------------------------------------------------------------------
// Contract clause verification
// -----------------------------------------------------------------------

/// Type information for parameters and return type, used to add
/// type-level Z3 constraints (e.g., `Nat` implies `>= 0`).
#[derive(Default)]
struct TypeConstraints<'a> {
    params: &'a [assura_parser::ast::Param],
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
}

/// Returns true if the given type token list represents the `Nat` type.
fn is_nat_type(ty: &[String]) -> bool {
    ty.len() == 1 && ty[0] == "Nat"
}

// Re-use extract_output_return_type and extract_input_params from entry.rs
// (single source of truth, avoids divergence between parallel and non-parallel paths).
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
    lemma_defs: &std::collections::HashMap<String, Vec<&Expr>>,
    cache: &mut SessionCache,
    results: &mut Vec<VerificationResult>,
    types: &TypeConstraints,
) {
    let requires: Vec<&Clause> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .collect();

    let verifiable: Vec<&Clause> = clauses
        .iter()
        .filter(|c| {
            matches!(
                c.kind,
                ClauseKind::Ensures
                    | ClauseKind::Invariant
                    | ClauseKind::Rule
                    | ClauseKind::MustNot
                    | ClauseKind::Decreases
            )
        })
        .collect();

    // Process feature-specific Other clauses via SMT feature dispatch.
    // Pass the clause body and sibling clauses so features with boolean
    // predicate bodies get real Z3 validity checking.
    for clause in clauses {
        if let ClauseKind::Other(kind) = &clause.kind {
            let feature_results = crate::smt_features::verify_feature_clause(
                kind,
                parent_name,
                &clause.body,
                clauses,
            );
            results.extend(feature_results);
        }
    }

    if verifiable.is_empty() {
        return;
    }

    // T045: Build frame checker from modifies clauses
    let modifies_bodies: Vec<&Expr> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Modifies)
        .map(|c| &c.body)
        .collect();
    let frame_checker = if modifies_bodies.is_empty() {
        assura_types::FrameChecker::empty()
    } else {
        let body_refs: Vec<&Expr> = modifies_bodies.to_vec();
        assura_types::FrameChecker::new(&body_refs)
    };

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
    enable_unsat_cores(&solver);
    let mut encoder = Encoder::new();
    encoder.init_adt_infrastructure();
    encoder.init_bitvector_infrastructure();
    for param in types.params {
        if let Some((width, signed)) = Encoder::fixed_width_bits(&param.ty) {
            encoder.register_fixed_width_param(&param.name, width, signed);
        }
    }

    // Bind named constants so Z3 uses concrete values, not free vars.
    for (name, value) in types.constants {
        let concrete = ast::Int::from_i64(*value);
        encoder
            .vars
            .insert(name.clone(), super::encoder::Z3Value::Int(concrete));
    }

    // Register known function names for trigger inference
    for other_clause in clauses {
        collect_function_names_for_triggers(&other_clause.body, &mut encoder.trigger_manager);
    }

    // Requires are asserted per-clause with tracking labels (below) so
    // unsat-core extraction identifies which preconditions were needed.
    // Type-level constraints and lemmas are still shared at base level.

    // Assert type-level constraints for parameters and return type.
    for param in types.params {
        if is_nat_type(&param.ty) {
            let p = encoder.get_or_create_int(&param.name);
            let zero = ast::Int::from_i64(0);
            solver.assert(p.ge(&zero));
        }
    }
    if is_nat_type(types.return_ty) {
        let result_var = encoder.get_or_create_int("result");
        let zero = ast::Int::from_i64(0);
        solver.assert(result_var.ge(&zero));
        let raw_result = encoder.get_or_create_int("__result");
        solver.assert(raw_result.ge(&zero));
    }

    // #188: Refinement narrowing from feature_max declarations.
    for (narrowed_name, bound) in types.narrowings {
        let var = encoder.get_or_create_int(narrowed_name);
        let upper = ast::Int::from_i64(*bound);
        solver.assert(var.le(&upper));
    }

    // T044: Inject lemma ensures as assumptions for any `apply` refs
    let apply_refs = collect_apply_refs(clauses);
    for lemma_name in &apply_refs {
        if let Some(ensures_bodies) = lemma_defs.get(lemma_name) {
            for ensures_body in ensures_bodies {
                let ens_val = encoder.encode_expr(ensures_body);
                let ens_bool = ens_val.as_bool();
                solver.assert(&ens_bool);
            }
        }
    }

    // Assert any background axioms from lemma encoding
    for axiom in &encoder.background_axioms {
        solver.assert(axiom);
    }
    encoder.background_axioms.clear();

    // For each verifiable clause: push, encode, check, pop
    for clause in &verifiable {
        let desc = clause_desc(parent_name, &clause.kind);

        // Skip clauses with unmodelable features
        if expr_has_unmodelable_features(&clause.body) {
            let reasons = collect_unmodelable_reasons(&clause.body);
            results.push(VerificationResult::Unknown {
                clause_desc: desc,
                reason: format!(
                    "clause uses features not yet encoded in SMT ({})",
                    reasons.join(", ")
                ),
            });
            continue;
        }

        // T113: Check verification cache before invoking Z3
        let clause_hash = format!("{desc}:{:?}", clause.body);
        if let Some(cached) = cache.lookup(&clause_hash) {
            match cached.result.as_str() {
                "verified" => results.push(VerificationResult::verified(desc)),
                "timeout" => results.push(VerificationResult::Timeout { clause_desc: desc }),
                other => results.push(VerificationResult::Unknown {
                    clause_desc: desc,
                    reason: other.to_string(),
                }),
            }
            continue;
        }

        solver.push(); // Save solver state

        let mut encoder = Encoder::with_string_theory(types.use_string_theory);
        encoder.init_adt_infrastructure();
        encoder.init_bitvector_infrastructure();
        for param in types.params {
            if let Some((width, signed)) = Encoder::fixed_width_bits(&param.ty) {
                encoder.register_fixed_width_param(&param.name, width, signed);
            }
        }

        // Bind named constants so Z3 uses concrete values, not free vars.
        for (name, value) in types.constants {
            let concrete = ast::Int::from_i64(*value);
            encoder
                .vars
                .insert(name.clone(), super::encoder::Z3Value::Int(concrete));
        }

        // Register known function names for trigger inference
        for other_clause in clauses {
            collect_function_names_for_triggers(&other_clause.body, &mut encoder.trigger_manager);
        }

        // Assert all requires as tracked assumptions for unsat-core extraction
        for (i, req) in requires.iter().enumerate() {
            let req_val = encoder.encode_expr(&req.body);
            let req_bool = req_val.as_bool();
            let label = format!("req_{i}");
            assert_tracked(&solver, &req_bool, &label);
        }
        // Assert background axioms from requires encoding (e.g., map
        // read-over-write, string length axioms)
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        encoder.background_axioms.clear();

        // Assert type-level constraints for parameters and return type.
        // Nat params get `param >= 0`; Nat return type gets `result >= 0`.
        for param in types.params {
            if is_nat_type(&param.ty) {
                let p = encoder.get_or_create_int(&param.name);
                let zero = ast::Int::from_i64(0);
                solver.assert(p.ge(&zero));
            }
        }
        if is_nat_type(types.return_ty) {
            // Constrain both "result" (AST Ident path) and "__result"
            // (raw token path) so the type constraint applies regardless
            // of which encoding path the clause body uses.
            let result_var = encoder.get_or_create_int("result");
            let zero = ast::Int::from_i64(0);
            solver.assert(result_var.ge(&zero));
            let raw_result = encoder.get_or_create_int("__result");
            solver.assert(raw_result.ge(&zero));
        }

        // #188: Refinement narrowing from feature_max declarations.
        // `feature_max max_X = V` narrows any variable named `X` with `X <= V`.
        for (narrowed_name, bound) in types.narrowings {
            let var = encoder.get_or_create_int(narrowed_name);
            let upper = ast::Int::from_i64(*bound);
            solver.assert(var.le(&upper));
        }

        // T044: Inject lemma ensures as assumptions for any `apply` refs
        let apply_refs = collect_apply_refs(clauses);
        for lemma_name in &apply_refs {
            if let Some(ensures_bodies) = lemma_defs.get(lemma_name) {
                for ensures_body in ensures_bodies {
                    let ens_val = encoder.encode_expr(ensures_body);
                    let ens_bool = ens_val.as_bool();
                    solver.assert(&ens_bool);
                }
            }
        }

        // T045: For ensures clauses with a modifies set, inject frame
        // axioms: for every variable referenced in the ensures that is
        // NOT in the modifies set, assert `var == old(var)`.
        if clause.kind == ClauseKind::Ensures && frame_checker.has_modifies() {
            let frame_vars = frame_checker.frame_axiom_vars(&clause.body);
            for var_name in &frame_vars {
                let current = encoder.get_or_create_int(var_name);
                let old_name = format!("{var_name}__old");
                let old_var = encoder.get_or_create_int(&old_name);
                let axiom = current.eq(&old_var);
                solver.assert(&axiom);
            }
        }

        // Encode the clause body
        let clause_val = encoder.encode_expr(&clause.body);
        let clause_bool = clause_val.as_bool();

        // Assert background axioms from this clause's encoding
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        encoder.background_axioms.clear();

        let result_before = results.len();
        match clause.kind {
            ClauseKind::Ensures | ClauseKind::Rule => {
                solver.assert(clause_bool.not());
                check_validity(&solver, desc, results);
            }
            ClauseKind::Invariant => {
                solver.assert(&clause_bool);
                check_satisfiability(&solver, desc, results);
            }
            ClauseKind::MustNot => {
                solver.assert(&clause_bool);
                check_validity(&solver, desc, results);
            }
            ClauseKind::Decreases => {
                let zero = ast::Int::from_i64(0);
                let measure = clause_val.as_int(&mut encoder.fresh_counter);
                let non_neg = measure.ge(&zero);
                solver.assert(non_neg.not());
                check_validity(&solver, desc, results);
            }
            _ => {}
        }

        // T113: Cache the verification result
        if let Some(result) = results.get(result_before) {
            let result_str = match result {
                VerificationResult::Verified { .. } => "verified",
                VerificationResult::Timeout { .. } => "timeout",
                VerificationResult::Unknown { reason, .. } => reason.as_str(),
                VerificationResult::Counterexample { .. } => "counterexample",
            };
            cache.insert(clause_hash, result_str.to_string(), 0);
        }

        solver.pop(1); // Restore solver state
    }
}

/// Verify a standalone invariant expression (e.g., service invariant).
fn verify_invariant_expr(parent_name: &str, expr: &Expr, results: &mut Vec<VerificationResult>) {
    let desc = format!("{parent_name}::invariant");
    let solver = Solver::new();
    let mut encoder = Encoder::new();
    encoder.init_adt_infrastructure();
    let val = encoder.encode_expr(expr);
    let bool_val = val.as_bool();
    solver.assert(&bool_val);
    check_satisfiability(&solver, desc, results);
}

// -----------------------------------------------------------------------
// Entry point
// -----------------------------------------------------------------------

/// Collect `feature_max` constants from the source AST.
///
/// Returns a vec of (name, value) pairs. Only declarations with a
/// parseable integer value are included; non-integer or missing values
/// are silently skipped (they remain free Z3 variables).
pub(crate) fn collect_feature_max_constants(typed: &TypedFile) -> Vec<(String, i64)> {
    let mut constants = Vec::new();
    for decl in &typed.resolved.source.decls {
        // Value tokens include type annotation: [":", "Nat", "=", "65536"]
        // Find the token after "=" for the actual integer value.
        if let Decl::Block {
            kind,
            name,
            value: Some(tokens),
            ..
        } = &decl.node
            && *kind == BlockKind::FeatureMax
            && let Some(eq_pos) = tokens.iter().position(|t| t == "=")
            && let Some(val_str) = tokens.get(eq_pos + 1)
            && let Ok(v) = val_str.parse::<i64>()
        {
            constants.push((name.clone(), v));
        }
    }
    constants
}

/// Derive refinement narrowing pairs from `feature_max` constant names.
///
/// Per spec Section 14 (PLAT.2): `feature_max max_page_size = 4096` narrows
/// all variables named `page_size` with `page_size <= 4096`. The rule strips
/// the `max_` prefix (case-insensitive) from the constant name to produce the
/// narrowed variable name.
///
/// Also handles `MAX_` all-caps prefix (e.g., `MAX_CONTENT_LEN` -> `content_len`
/// lowercased won't match, but `CONTENT_LEN` -> narrowing for `CONTENT_LEN`).
/// The narrowing matches both the stripped suffix as-is and its lowercase form.
pub(crate) fn derive_narrowings(constants: &[(String, i64)]) -> Vec<(String, i64)> {
    let mut narrowings = Vec::new();
    for (name, value) in constants {
        // Strip `max_` or `MAX_` prefix to get the narrowed variable name
        let narrowed = name
            .strip_prefix("max_")
            .or_else(|| name.strip_prefix("MAX_"));
        if let Some(narrowed) = narrowed.filter(|s| !s.is_empty()) {
            // Add the suffix as-is (preserving case)
            narrowings.push((narrowed.to_string(), *value));
            // Also add lowercase variant if different
            let lower = narrowed.to_lowercase();
            if lower != narrowed {
                narrowings.push((lower, *value));
            }
        }
    }
    narrowings
}

/// Collect all lemma definitions from the source AST.
///
/// Returns a map from lemma name to its ensures clause bodies.
fn collect_lemma_defs(typed: &TypedFile) -> std::collections::HashMap<String, Vec<&Expr>> {
    let mut lemmas = std::collections::HashMap::new();
    for decl in &typed.resolved.source.decls {
        if let Decl::FnDef(f) = &decl.node
            && f.is_lemma
        {
            let ensures: Vec<&Expr> = f
                .clauses
                .iter()
                .filter(|c| c.kind == ClauseKind::Ensures)
                .map(|c| &c.body)
                .collect();
            lemmas.insert(f.name.clone(), ensures);
        }
    }
    lemmas
}

/// Scan clause bodies for `apply lemma_name(args)` expressions and
/// collect the referenced lemma names.
fn collect_apply_refs(clauses: &[Clause]) -> Vec<String> {
    let mut refs = Vec::new();
    for clause in clauses {
        collect_apply_refs_expr(&clause.body, &mut refs);
    }
    refs
}

fn collect_apply_refs_expr(expr: &Expr, refs: &mut Vec<String>) {
    match expr {
        Expr::Apply { lemma_name, args } => {
            refs.push(lemma_name.clone());
            for arg in args {
                collect_apply_refs_expr(arg, refs);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_apply_refs_expr(lhs, refs);
            collect_apply_refs_expr(rhs, refs);
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Paren(inner)
        | Expr::Old(inner)
        | Expr::Ghost(inner)
        | Expr::Field(inner, _)
        | Expr::Cast { expr: inner, .. } => {
            collect_apply_refs_expr(inner, refs);
        }
        Expr::Call { func, args } => {
            collect_apply_refs_expr(func, refs);
            for a in args {
                collect_apply_refs_expr(a, refs);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_apply_refs_expr(receiver, refs);
            for a in args {
                collect_apply_refs_expr(a, refs);
            }
        }
        Expr::Index { expr: e, index } => {
            collect_apply_refs_expr(e, refs);
            collect_apply_refs_expr(index, refs);
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_apply_refs_expr(domain, refs);
            collect_apply_refs_expr(body, refs);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_apply_refs_expr(cond, refs);
            collect_apply_refs_expr(then_branch, refs);
            if let Some(eb) = else_branch {
                collect_apply_refs_expr(eb, refs);
            }
        }
        Expr::List(items) | Expr::Block(items) => {
            for item in items {
                collect_apply_refs_expr(item, refs);
            }
        }
        _ => {}
    }
}

/// Verify a quantified formula using Z3.
///
/// Encodes assumptions and the negated quantified body, then checks
/// satisfiability. UNSAT means the formula holds universally.
pub(crate) fn verify_quantified_impl(
    name: &str,
    assumptions: &[Expr],
    quantified_body: &Expr,
) -> VerificationResult {
    let solver = Solver::new();
    // Layer 2 timeout: 10 seconds
    let mut params = z3::Params::new();
    params.set_u32("timeout", 10000);
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

    match solver.check() {
        SatResult::Unsat => VerificationResult::verified(name),
        SatResult::Sat => {
            let (model_str, counter_model) = if let Some(m) = solver.get_model() {
                let cm = extract_counter_model(&m);
                (format!("{m}"), Some(cm))
            } else {
                ("(no model)".into(), None)
            };
            VerificationResult::Counterexample {
                clause_desc: name.into(),
                model: model_str,
                counter_model,
            }
        }
        SatResult::Unknown => {
            let reason = solver
                .get_reason_unknown()
                .unwrap_or_else(|| "unknown".into());
            if reason.contains("timeout") {
                VerificationResult::Timeout {
                    clause_desc: name.into(),
                }
            } else {
                VerificationResult::Unknown {
                    clause_desc: name.into(),
                    reason,
                }
            }
        }
    }
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
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
) -> Vec<VerificationResult> {
    let mut results = Vec::new();
    let mut cache = SessionCache::new();
    let lemma_defs = std::collections::HashMap::new();
    let narrowings = derive_narrowings(constants);
    let types = TypeConstraints {
        params,
        return_ty,
        constants,
        narrowings: &narrowings,
        ..Default::default()
    };
    verify_clauses_with_types(
        contract_name,
        clauses,
        &lemma_defs,
        &mut cache,
        &mut results,
        &types,
    );
    results
}

pub(crate) fn verify_impl_with_timeout(
    typed: &TypedFile,
    timeout_ms: u64,
) -> Vec<VerificationResult> {
    let _ = timeout_ms; // timeout is set per-solver in verify_clauses
    let mut results = Vec::new();
    let mut cache = SessionCache::new();

    // T044: collect all lemma definitions for apply injection
    let lemma_defs = collect_lemma_defs(typed);

    // #180: collect feature_max constants so the encoder binds them
    // to concrete values instead of creating free Z3 variables.
    let constants = collect_feature_max_constants(typed);
    // #188: derive refinement narrowing pairs from feature_max names.
    let narrowings = derive_narrowings(&constants);

    for decl in &typed.resolved.source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                // #190: Extract type constraints from output() and input()
                // clauses. Contracts use `output(result: Nat)` instead of
                // function return types, so we need to parse those clauses
                // to get the same Nat >= 0 constraints that fn defs get.
                let output_ty = extract_output_return_type(&c.clauses);
                let input_params = extract_input_params(&c.clauses);
                // Merge input params with any fn_params from inline fn defs
                let mut all_params = input_params;
                all_params.extend_from_slice(&c.fn_params);
                verify_clauses_with_types(
                    &c.name,
                    &c.clauses,
                    &lemma_defs,
                    &mut cache,
                    &mut results,
                    &TypeConstraints {
                        params: &all_params,
                        return_ty: &output_ty,
                        constants: &constants,
                        narrowings: &narrowings,
                        ..Default::default()
                    },
                );
            }
            Decl::FnDef(f) => {
                let types = TypeConstraints {
                    params: &f.params,
                    return_ty: &f.return_ty,
                    constants: &constants,
                    narrowings: &narrowings,
                    ..Default::default()
                };
                verify_clauses_with_types(
                    &f.name,
                    &f.clauses,
                    &lemma_defs,
                    &mut cache,
                    &mut results,
                    &types,
                );
            }
            Decl::Extern(e) => {
                let types = TypeConstraints {
                    params: &e.params,
                    return_ty: &e.return_ty,
                    constants: &constants,
                    narrowings: &narrowings,
                    ..Default::default()
                };
                verify_clauses_with_types(
                    &e.name,
                    &e.clauses,
                    &lemma_defs,
                    &mut cache,
                    &mut results,
                    &types,
                );
            }
            Decl::Service(s) => {
                let svc_types = TypeConstraints {
                    constants: &constants,
                    narrowings: &narrowings,
                    ..Default::default()
                };
                for item in &s.items {
                    match item {
                        ServiceItem::Operation { name, clauses } => {
                            let qname = format!("{}.{}", s.name, name);
                            verify_clauses_with_types(
                                &qname,
                                clauses,
                                &lemma_defs,
                                &mut cache,
                                &mut results,
                                &svc_types,
                            );
                        }
                        ServiceItem::Query { name, clauses } => {
                            let qname = format!("{}.{}", s.name, name);
                            verify_clauses_with_types(
                                &qname,
                                clauses,
                                &lemma_defs,
                                &mut cache,
                                &mut results,
                                &svc_types,
                            );
                        }
                        ServiceItem::Invariant(expr) => {
                            verify_invariant_expr(&s.name, expr, &mut results);
                        }
                        _ => {}
                    }
                }
            }
            Decl::Block { name, body, .. } => {
                verify_clauses_with_types(
                    name,
                    body,
                    &lemma_defs,
                    &mut cache,
                    &mut results,
                    &TypeConstraints {
                        constants: &constants,
                        narrowings: &narrowings,
                        ..Default::default()
                    },
                );
            }
            Decl::Bind(b) => {
                let types = TypeConstraints {
                    params: &b.params,
                    return_ty: &b.return_ty,
                    constants: &constants,
                    narrowings: &narrowings,
                    ..Default::default()
                };
                verify_clauses_with_types(
                    &b.name,
                    &b.clauses,
                    &lemma_defs,
                    &mut cache,
                    &mut results,
                    &types,
                );
            }
            // Prophecy variables don't have verifiable clauses directly;
            // they are used as existential witnesses in contract proofs.
            Decl::Prophecy(_) | Decl::CodecRegistry(_) | Decl::TypeDef(_) | Decl::EnumDef(_) => {}
        }
    }

    // Run all 5 advanced passes via shared solver-agnostic functions (#214)
    results.extend(crate::entry::run_advanced_passes(typed, timeout_ms));

    results
}

/// Collect function names from an expression tree and register them
/// with the trigger manager for quantifier e-matching.
fn collect_function_names_for_triggers(expr: &Expr, tm: &mut crate::advanced::TriggerManager) {
    match expr {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = func.as_ref() {
                tm.register_function(name.clone());
            }
            for a in args {
                collect_function_names_for_triggers(a, tm);
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            tm.register_function(method.clone());
            collect_function_names_for_triggers(receiver, tm);
            for a in args {
                collect_function_names_for_triggers(a, tm);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_function_names_for_triggers(lhs, tm);
            collect_function_names_for_triggers(rhs, tm);
        }
        Expr::UnaryOp { expr: e, .. } | Expr::Paren(e) | Expr::Old(e) | Expr::Ghost(e) => {
            collect_function_names_for_triggers(e, tm);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_function_names_for_triggers(cond, tm);
            collect_function_names_for_triggers(then_branch, tm);
            if let Some(eb) = else_branch {
                collect_function_names_for_triggers(eb, tm);
            }
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_function_names_for_triggers(domain, tm);
            collect_function_names_for_triggers(body, tm);
        }
        Expr::Index { expr: e, index } => {
            collect_function_names_for_triggers(e, tm);
            collect_function_names_for_triggers(index, tm);
        }
        _ => {}
    }
}

// Helper functions (extract_numeric_arg, resolve_prophecy_vars,
// constrain_prophecy_vars, collect_prophecy_refs) moved to entry.rs
// as shared solver-agnostic code used by both Z3 and CVC5 paths.

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::{BinOp, Literal};

    #[test]
    fn extract_output_return_type_nat() {
        let clauses = vec![Clause {
            kind: ClauseKind::Output,
            body: Expr::Raw(vec!["result".into(), ":".into(), "Nat".into()]),
            effect_variables: vec![],
        }];
        let ty = extract_output_return_type(&clauses);
        assert_eq!(ty, vec!["Nat"]);
        assert!(is_nat_type(&ty));
    }

    #[test]
    fn extract_output_return_type_non_nat() {
        let clauses = vec![Clause {
            kind: ClauseKind::Output,
            body: Expr::Raw(vec!["result".into(), ":".into(), "Bytes".into()]),
            effect_variables: vec![],
        }];
        let ty = extract_output_return_type(&clauses);
        assert_eq!(ty, vec!["Bytes"]);
        assert!(!is_nat_type(&ty));
    }

    #[test]
    fn extract_output_return_type_missing() {
        let clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::Literal(Literal::Bool(true)),
            effect_variables: vec![],
        }];
        let ty = extract_output_return_type(&clauses);
        assert!(ty.is_empty());
    }

    #[test]
    fn extract_input_params_basic() {
        let clauses = vec![Clause {
            kind: ClauseKind::Input,
            body: Expr::Raw(vec!["raw_data".into(), ":".into(), "Bytes".into()]),
            effect_variables: vec![],
        }];
        let params = extract_input_params(&clauses);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "raw_data");
        assert_eq!(params[0].ty, vec!["Bytes"]);
    }

    #[test]
    fn contract_output_nat_constrains_result() {
        // Verifies #190: output(result: Nat) should make `ensures { result >= 0 }` verified.
        let clauses = vec![
            Clause {
                kind: ClauseKind::Output,
                body: Expr::Raw(vec!["result".into(), ":".into(), "Nat".into()]),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("result".into())),
                    op: BinOp::Gte,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
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
