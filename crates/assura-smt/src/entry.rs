//! Public entry point functions for SMT verification.
//!
//! Contains `verify()`, `verify_with_options()`, `verify_parallel()`,
//! and all standalone verification functions (refinement, buffer bounds,
//! taint safety, measures, termination).

use assura_parser::ast::{Clause, ClauseKind, Expr, Param};
use assura_types::TypedFile;

use crate::SolverChoice;
use crate::cache::VerificationCache;
use crate::measures::MeasureDefinition;
use crate::result::VerificationResult;

/// Extract the return type from `output(result: Nat)` clauses in a contract.
///
/// Contracts declare their output type via `output(result: Nat)` instead of
/// a function return type. The clause body is `Expr::Raw(["result", ":", "Nat"])`.
pub(crate) fn extract_output_return_type(clauses: &[Clause]) -> Vec<String> {
    for clause in clauses {
        if clause.kind == ClauseKind::Output
            && let Expr::Raw(tokens) = &clause.body
        {
            if tokens.len() >= 3 && tokens[1] == ":" {
                return tokens[2..].to_vec();
            }
            return tokens.clone();
        }
    }
    Vec::new()
}

/// Extract parameters from `input(raw_data: Bytes)` clauses in a contract.
pub(crate) fn extract_input_params(clauses: &[Clause]) -> Vec<Param> {
    for clause in clauses {
        if clause.kind == ClauseKind::Input
            && let Expr::Raw(tokens) = &clause.body
        {
            let mut params = Vec::new();
            let mut i = 0;
            while i < tokens.len() {
                if tokens[i] == "," {
                    i += 1;
                    continue;
                }
                let name = tokens[i].clone();
                i += 1;
                if i < tokens.len() && tokens[i] == ":" {
                    i += 1;
                    let mut ty = Vec::new();
                    while i < tokens.len() && tokens[i] != "," {
                        ty.push(tokens[i].clone());
                        i += 1;
                    }
                    params.push(Param {
                        name,
                        ty,
                        parsed_type: None,
                    });
                } else {
                    params.push(Param {
                        name,
                        ty: Vec::new(),
                        parsed_type: None,
                    });
                }
            }
            return params;
        }
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Verify all contract clauses in a type-checked file.
///
/// Returns a `VerificationResult` for each verifiable clause (ensures,
/// invariant). Requires clauses are collected as assumptions but not
/// independently verified (they constrain the context for ensures).
pub fn verify(typed: &TypedFile) -> Vec<VerificationResult> {
    verify_with_options(typed, &assura_config::VerifyOptions::default())
}

/// Verify all contract clauses using the given verification options.
///
/// `options.solver` selects the SMT backend ("z3", "cvc5", "portfolio").
/// `options.timeout_ms` limits per-query solver time.
/// `options.layer` controls verification depth (0 = structural, 1+ = SMT).
pub fn verify_with_options(
    typed: &TypedFile,
    options: &assura_config::VerifyOptions,
) -> Vec<VerificationResult> {
    match options.solver {
        SolverChoice::Cvc5 => verify_file_with_cvc5(typed),
        SolverChoice::Portfolio => {
            // Try Z3 first; fall back to CVC5 on timeout/unknown
            #[cfg(feature = "z3-verify")]
            {
                let z3_results =
                    crate::z3_backend::verify_impl_with_timeout(typed, options.timeout_ms);
                let has_unknown = z3_results.iter().any(|r| {
                    matches!(
                        r,
                        VerificationResult::Timeout { .. } | VerificationResult::Unknown { .. }
                    )
                });
                if has_unknown {
                    let cvc5_results = verify_file_with_cvc5(typed);
                    merge_portfolio_results(z3_results, cvc5_results)
                } else {
                    z3_results
                }
            }
            #[cfg(not(feature = "z3-verify"))]
            {
                verify_file_with_cvc5(typed)
            }
        }
        SolverChoice::Z3 => {
            #[cfg(feature = "z3-verify")]
            {
                crate::z3_backend::verify_impl_with_timeout(typed, options.timeout_ms)
            }
            #[cfg(not(feature = "z3-verify"))]
            {
                crate::no_z3::verify_stub(typed)
            }
        }
    }
}

/// Verify all contracts in a file using the CVC5 backend.
fn verify_file_with_cvc5(typed: &TypedFile) -> Vec<VerificationResult> {
    use assura_parser::ast::{Decl, ServiceItem};
    let mut results = Vec::new();
    for decl in &typed.resolved.source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                let output_ty = extract_output_return_type(&c.clauses);
                let mut input_params = extract_input_params(&c.clauses);
                input_params.extend_from_slice(&c.fn_params);
                results.extend(crate::cvc5_backend::verify_contract_cvc5_with_types(
                    &c.name,
                    &c.clauses,
                    &input_params,
                    &output_ty,
                ));
            }
            Decl::FnDef(f) => {
                results.extend(crate::cvc5_backend::verify_contract_cvc5_with_types(
                    &f.name,
                    &f.clauses,
                    &f.params,
                    &f.return_ty,
                ));
            }
            Decl::Extern(e) => {
                results.extend(crate::cvc5_backend::verify_contract_cvc5_with_types(
                    &e.name,
                    &e.clauses,
                    &e.params,
                    &e.return_ty,
                ));
            }
            Decl::Service(s) => {
                for item in &s.items {
                    match item {
                        ServiceItem::Operation { name, clauses } => {
                            let qname = format!("{}.{}", s.name, name);
                            results
                                .extend(crate::cvc5_backend::verify_contract_cvc5(&qname, clauses));
                        }
                        ServiceItem::Query { name, clauses } => {
                            let qname = format!("{}.{}", s.name, name);
                            results
                                .extend(crate::cvc5_backend::verify_contract_cvc5(&qname, clauses));
                        }
                        ServiceItem::Invariant(expr) => {
                            let inv_clause = assura_parser::ast::Clause {
                                kind: assura_parser::ast::ClauseKind::Invariant,
                                body: expr.clone(),
                                effect_variables: vec![],
                            };
                            results.extend(crate::cvc5_backend::verify_contract_cvc5(
                                &format!("{}::invariant", s.name),
                                &[inv_clause],
                            ));
                        }
                        _ => {}
                    }
                }
            }
            Decl::Block { name, body, .. } => {
                results.extend(crate::cvc5_backend::verify_contract_cvc5(name, body));
            }
            Decl::Bind(b) => {
                results.extend(crate::cvc5_backend::verify_contract_cvc5_with_types(
                    &b.name,
                    &b.clauses,
                    &b.params,
                    &b.return_ty,
                ));
            }
            Decl::Prophecy(_) | Decl::CodecRegistry(_) | Decl::TypeDef(_) | Decl::EnumDef(_) => {}
        }
    }
    results
}

/// Merge portfolio results: prefer Z3 result unless it was Timeout/Unknown,
/// in which case use CVC5 result.
#[cfg(feature = "z3-verify")]
fn merge_portfolio_results(
    z3: Vec<VerificationResult>,
    cvc5: Vec<VerificationResult>,
) -> Vec<VerificationResult> {
    let mut merged = Vec::with_capacity(z3.len());
    let mut cvc5_iter = cvc5.into_iter();
    for r in z3 {
        match &r {
            VerificationResult::Timeout { .. } | VerificationResult::Unknown { .. } => {
                // Use CVC5 result if available, otherwise keep Z3's
                merged.push(cvc5_iter.next().unwrap_or(r));
            }
            _ => merged.push(r),
        }
    }
    merged
}

/// Verify all declarations in parallel using rayon.
///
/// Each contract/function gets its own Z3 context (Z3 contexts are not
/// `Sync`). Independent declarations are verified concurrently using
/// rayon's work-stealing thread pool, achieving linear speedup on
/// multi-core machines for projects with many contracts.
///
/// Also uses the filesystem cache: cache hits are returned immediately,
/// only cache misses go to Z3 (potentially in parallel).
pub fn verify_parallel(typed: &TypedFile, cache: &VerificationCache) -> Vec<VerificationResult> {
    verify_parallel_with_solver(typed, cache, SolverChoice::Z3)
}

/// Check whether any declaration in the source file has verifiable clauses
/// (requires, ensures, invariant).  Returns false if there is nothing to
/// send to the solver, allowing callers to skip thread-pool and cache init.
pub fn has_verifiable_clauses(source: &assura_parser::ast::SourceFile) -> bool {
    use assura_parser::ast::{ClauseKind, Decl};

    let verifiable = |clauses: &[assura_parser::ast::Clause]| {
        clauses.iter().any(|c| {
            matches!(
                c.kind,
                ClauseKind::Requires | ClauseKind::Ensures | ClauseKind::Invariant
            )
        })
    };

    source.decls.iter().any(|d| match &d.node {
        Decl::Contract(c) => verifiable(&c.clauses),
        Decl::FnDef(f) => verifiable(&f.clauses),
        Decl::Extern(e) => verifiable(&e.clauses),
        Decl::Service(s) => s.items.iter().any(|item| match item {
            assura_parser::ast::ServiceItem::Operation { clauses, .. }
            | assura_parser::ast::ServiceItem::Query { clauses, .. } => verifiable(clauses),
            assura_parser::ast::ServiceItem::Invariant(_) => true,
            _ => false,
        }),
        Decl::Block { body, .. } => verifiable(body),
        Decl::Bind(b) => verifiable(&b.clauses),
        _ => false,
    })
}

/// Verify all declarations in parallel using the specified solver.
pub fn verify_parallel_with_solver(
    typed: &TypedFile,
    cache: &VerificationCache,
    solver: SolverChoice,
) -> Vec<VerificationResult> {
    use assura_parser::ast::Decl;
    use rayon::prelude::*;

    // #180: collect feature_max constants so the encoder binds them
    // to concrete values instead of creating free Z3 variables.
    #[cfg(feature = "z3-verify")]
    let constants = crate::z3_backend::collect_feature_max_constants(typed);
    #[cfg(not(feature = "z3-verify"))]
    let constants: Vec<(String, i64)> = Vec::new();

    // Collect verification jobs with type info for return-type constraints
    type Job = (
        String,
        Vec<assura_parser::ast::Clause>,
        Vec<assura_parser::ast::Param>,
        Vec<String>,
    );
    let mut jobs: Vec<Job> = Vec::new();

    for decl in &typed.resolved.source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                // #190: Extract type constraints from output() and input()
                // clauses. Contracts use `output(result: Nat)` instead of
                // function return types, so we parse those clauses to get
                // the same Nat >= 0 constraints that fn defs get.
                let output_ty = extract_output_return_type(&c.clauses);
                let mut input_params = extract_input_params(&c.clauses);
                input_params.extend_from_slice(&c.fn_params);
                jobs.push((c.name.clone(), c.clauses.clone(), input_params, output_ty));
            }
            Decl::FnDef(f) => {
                jobs.push((
                    f.name.clone(),
                    f.clauses.clone(),
                    f.params.clone(),
                    f.return_ty.clone(),
                ));
            }
            Decl::Extern(e) => {
                jobs.push((
                    e.name.clone(),
                    e.clauses.clone(),
                    e.params.clone(),
                    e.return_ty.clone(),
                ));
            }
            Decl::Service(s) => {
                for item in &s.items {
                    match item {
                        assura_parser::ast::ServiceItem::Operation { name, clauses } => {
                            jobs.push((
                                format!("{}.{}", s.name, name),
                                clauses.clone(),
                                vec![],
                                vec![],
                            ));
                        }
                        assura_parser::ast::ServiceItem::Query { name, clauses } => {
                            jobs.push((
                                format!("{}.{}", s.name, name),
                                clauses.clone(),
                                vec![],
                                vec![],
                            ));
                        }
                        assura_parser::ast::ServiceItem::Invariant(expr) => {
                            let inv_clause = assura_parser::ast::Clause {
                                kind: assura_parser::ast::ClauseKind::Invariant,
                                body: expr.clone(),
                                effect_variables: vec![],
                            };
                            jobs.push((
                                format!("{}::invariant", s.name),
                                vec![inv_clause],
                                vec![],
                                vec![],
                            ));
                        }
                        _ => {}
                    }
                }
            }
            Decl::Block { name, body, .. } => {
                jobs.push((name.clone(), body.clone(), vec![], vec![]));
            }
            Decl::Bind(b) => {
                jobs.push((
                    b.name.clone(),
                    b.clauses.clone(),
                    b.params.clone(),
                    b.return_ty.clone(),
                ));
            }
            Decl::Prophecy(_) | Decl::CodecRegistry(_) | Decl::TypeDef(_) | Decl::EnumDef(_) => {}
        }
    }

    // Verify in parallel: each job gets its own solver context
    let per_job_results: Vec<Vec<VerificationResult>> = jobs
        .par_iter()
        .map(|(name, clauses, params, return_ty)| {
            // Check cache first
            if let Some(cached) = cache.get(name, clauses) {
                return cached;
            }
            // Cache miss: run solver with type constraints
            let results = verify_contract_with_types_and_solver(
                name, clauses, params, return_ty, &constants, solver,
            );
            cache.put(name, clauses, &results);
            results
        })
        .collect();

    // Flatten into a single results vec
    per_job_results.into_iter().flatten().collect()
}

/// Verify a single contract's clauses using the default solver (Z3).
///
/// Unlike `verify()` which processes all declarations in a `TypedFile`,
/// this function verifies just the given contract's clauses. Each
/// ensures/invariant clause gets its own solver query with all requires
/// clauses asserted as assumptions.
///
/// Returns one `VerificationResult` per verifiable clause.
pub fn verify_contract(
    contract_name: &str,
    clauses: &[assura_parser::ast::Clause],
) -> Vec<VerificationResult> {
    verify_contract_with_solver(contract_name, clauses, SolverChoice::Z3)
}

/// Verify a single contract's clauses using the specified solver.
pub fn verify_contract_with_solver(
    contract_name: &str,
    clauses: &[assura_parser::ast::Clause],
    solver: SolverChoice,
) -> Vec<VerificationResult> {
    match solver {
        SolverChoice::Z3 => {
            #[cfg(feature = "z3-verify")]
            {
                crate::z3_backend::verify_contract_impl(contract_name, clauses)
            }
            #[cfg(not(feature = "z3-verify"))]
            {
                let _ = contract_name;
                clauses
                    .iter()
                    .filter(|c| {
                        matches!(
                            c.kind,
                            assura_parser::ast::ClauseKind::Ensures
                                | assura_parser::ast::ClauseKind::Invariant
                                | assura_parser::ast::ClauseKind::Rule
                                | assura_parser::ast::ClauseKind::MustNot
                                | assura_parser::ast::ClauseKind::Decreases
                        )
                    })
                    .map(|c| {
                        let desc = format!("{contract_name}::{:?}", c.kind);
                        VerificationResult::Unknown {
                            clause_desc: desc,
                            reason: "Z3 not available (compiled without z3-verify feature)".into(),
                        }
                    })
                    .collect()
            }
        }
        SolverChoice::Cvc5 => crate::cvc5_backend::verify_contract_cvc5(contract_name, clauses),
        SolverChoice::Portfolio => {
            // Try Z3 first, fall back to CVC5 for timeout/unknown results
            let z3_results = verify_contract_with_solver(contract_name, clauses, SolverChoice::Z3);
            let needs_fallback = z3_results.iter().any(|r| {
                matches!(
                    r,
                    VerificationResult::Timeout { .. } | VerificationResult::Unknown { .. }
                )
            });
            if !needs_fallback {
                return z3_results;
            }
            // Re-check only the failed clauses with CVC5
            let cvc5_results = crate::cvc5_backend::verify_contract_cvc5(contract_name, clauses);

            // Merge: use CVC5 result for any Z3 timeout/unknown
            z3_results
                .into_iter()
                .map(|r| match &r {
                    VerificationResult::Timeout { clause_desc }
                    | VerificationResult::Unknown { clause_desc, .. } => {
                        // Find the matching CVC5 result
                        cvc5_results
                            .iter()
                            .find(|cr| match cr {
                                VerificationResult::Verified { clause_desc: cd }
                                | VerificationResult::Counterexample {
                                    clause_desc: cd, ..
                                }
                                | VerificationResult::Timeout { clause_desc: cd }
                                | VerificationResult::Unknown {
                                    clause_desc: cd, ..
                                } => cd == clause_desc,
                            })
                            .cloned()
                            .unwrap_or(r)
                    }
                    _ => r,
                })
                .collect()
        }
    }
}

/// Verify a contract with type-level constraints from params and return type.
fn verify_contract_with_types_and_solver(
    contract_name: &str,
    clauses: &[assura_parser::ast::Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    solver: SolverChoice,
) -> Vec<VerificationResult> {
    match solver {
        SolverChoice::Z3 => {
            #[cfg(feature = "z3-verify")]
            {
                crate::z3_backend::verify_contract_impl_with_types(
                    contract_name,
                    clauses,
                    params,
                    return_ty,
                    constants,
                )
            }
            #[cfg(not(feature = "z3-verify"))]
            {
                let _ = constants;
                verify_contract_with_solver(contract_name, clauses, solver)
            }
        }
        SolverChoice::Cvc5 | SolverChoice::Portfolio => {
            crate::cvc5_backend::verify_contract_cvc5_with_types(
                contract_name,
                clauses,
                params,
                return_ty,
            )
        }
    }
}

/// Check whether a refinement subtype relation holds:
///
/// `{v: T | antecedent} <: {v: T | consequent}`
///
/// Encodes: `(assert antecedent) (assert (not consequent)) (check-sat)`
///
/// UNSAT => subtyping holds (Verified).
/// SAT  => counterexample exists.
pub fn check_refinement_subtype(antecedent: &Expr, consequent: &Expr) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::check_refinement_subtype_impl(antecedent, consequent)
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        crate::no_z3::refinement_stub(antecedent, consequent)
    }
}

/// Verify buffer bounds safety for a contract.
///
/// Given a set of requires (assumptions) and an ensures clause that
/// references buffer access, checks whether the requires clauses are
/// sufficient to prove bounds safety. Specifically:
///
/// - Buffer capacity is modeled as an uninterpreted non-negative integer
/// - Offset and length constraints from requires are asserted
/// - The ensures clause is checked for validity under those assumptions
///
/// This is the SMT encoding for MEM.1 memory region contracts.
pub fn verify_buffer_bounds(requires: &[Expr], ensures: &Expr) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::verify_buffer_bounds_impl(requires, ensures)
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        let _ = (requires, ensures);
        VerificationResult::Unknown {
            clause_desc: "buffer_bounds".into(),
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }
}

/// Verify region containment: that all indices in sub_region are within parent_region.
///
/// SMT encoding: `forall i: sub_lo <= i < sub_hi => parent_lo <= i < parent_hi`
///
/// The `context` expressions provide additional assumptions (e.g., bounds on
/// the buffer capacity). Returns Verified if the containment holds for all
/// possible values satisfying the context, or Counterexample otherwise.
pub fn verify_region_containment(
    context: &[Expr],
    sub_lo: &Expr,
    sub_hi: &Expr,
    parent_lo: &Expr,
    parent_hi: &Expr,
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::verify_region_containment_impl(
            context, sub_lo, sub_hi, parent_lo, parent_hi,
        )
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        let _ = (context, sub_lo, sub_hi, parent_lo, parent_hi);
        VerificationResult::Unknown {
            clause_desc: "region_containment".into(),
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }
}

/// Check refinement subtyping with extra context assumptions.
///
/// The `context` expressions are asserted alongside the antecedent before
/// negating the consequent. Useful when the subtyping depends on
/// constraints from enclosing scopes (e.g., function parameters).
pub fn check_refinement_subtype_with_context(
    context: &[Expr],
    antecedent: &Expr,
    consequent: &Expr,
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::check_refinement_subtype_with_context_impl(
            context, antecedent, consequent,
        )
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        crate::no_z3::refinement_ctx_stub(context, antecedent, consequent)
    }
}

/// Verify taint safety for a contract: prove that tainted data cannot flow
/// to sensitive positions without validation.
///
/// The SMT encoding models taint labels as integers in the lattice:
/// `Untrusted(0) < Validated(1) < Trusted(2)`.
///
/// For each variable with a taint label, a Z3 integer represents its taint
/// level. Flow constraints assert that taint propagates through operations
/// (union semantics: result taint = min of operand taints), and sensitive
/// positions require a minimum taint level (Validated or Trusted).
///
/// Returns `Verified` if the taint constraints are satisfiable with no
/// violations, or `Counterexample` with the violating variable assignment.
pub fn verify_taint_safety(
    taint_labels: &[(String, assura_types::TaintLabel)],
    validation_fns: &[String],
    sensitive_uses: &[(String, assura_types::TaintLabel)],
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::verify_taint_safety_impl(taint_labels, validation_fns, sensitive_uses)
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        let _ = (taint_labels, validation_fns, sensitive_uses);
        VerificationResult::Unknown {
            clause_desc: "taint_safety".into(),
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }
}

/// Verify a contract using measure-enriched SMT context.
///
/// Each measure in `measures` is encoded as an uninterpreted function in Z3,
/// with its standard axioms asserted. The `requires` expressions are asserted
/// as assumptions, and the `ensures` expression is checked for validity under
/// those assumptions plus the measure axioms.
///
/// This is the primary entry point for measure-aware verification.
pub fn verify_with_measures(
    requires: &[Expr],
    ensures: &Expr,
    measures: &[MeasureDefinition],
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::verify_with_measures_impl(requires, ensures, measures)
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        let _ = (requires, ensures, measures);
        VerificationResult::Unknown {
            clause_desc: "verify_with_measures".into(),
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Termination (decreases) verification
// ---------------------------------------------------------------------------

/// Verify that a decreases measure strictly decreases at a recursive call site.
///
/// Given:
/// - `preconditions`: the function's requires clauses (assumed true)
/// - `measure_expr`: the decreases expression in terms of function params
/// - `call_arg_expr`: the argument at the call site corresponding to the measure
/// - `clause_desc`: description for the verification result
///
/// Checks: `preconditions => measure(call_args) < measure(fn_args) && measure(call_args) >= 0`
///
/// UNSAT on the negation => verified (measure decreases).
/// SAT => counterexample (measure does not decrease).
pub fn verify_decrease(
    preconditions: &[Expr],
    measure_expr: &Expr,
    call_arg_expr: &Expr,
    clause_desc: String,
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::verify_decrease_impl(
            preconditions,
            measure_expr,
            call_arg_expr,
            clause_desc,
        )
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        let _ = (preconditions, measure_expr, call_arg_expr);
        VerificationResult::Unknown {
            clause_desc,
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }
}

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
    let old_requires: Vec<&Expr> = old_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let new_requires: Vec<&Expr> = new_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let old_ensures: Vec<&Expr> = old_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .map(|c| &c.body)
        .collect();
    let new_ensures: Vec<&Expr> = new_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .map(|c| &c.body)
        .collect();

    // ---- Precondition weakening: old_requires => new_requires ----
    // All old preconditions must imply all new preconditions.
    // If old has no requires, it accepts everything, so new must also accept
    // everything (new_requires must be trivially true).
    // If new has no requires, it accepts everything, so weakening holds trivially.
    let precondition_weakening = if new_requires.is_empty() {
        // New accepts everything; weakening holds trivially
        VerificationResult::Verified {
            clause_desc: format!("{contract_name}: precondition weakening"),
        }
    } else {
        check_implication(
            &old_requires,
            &new_requires,
            &format!("{contract_name}: precondition weakening"),
        )
    };

    // ---- Postcondition strengthening: new_ensures => old_ensures ----
    // All new postconditions must imply all old postconditions.
    // If old has no ensures, there are no guarantees to maintain, so
    // strengthening holds trivially.
    // If new has no ensures but old does, strengthening fails (lost guarantees).
    let postcondition_strengthening = if old_ensures.is_empty() {
        // Old had no guarantees; any new guarantees are fine
        VerificationResult::Verified {
            clause_desc: format!("{contract_name}: postcondition strengthening"),
        }
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
fn check_implication(
    antecedents: &[&Expr],
    consequents: &[&Expr],
    desc: &str,
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        use crate::z3_backend::encoder::{Encoder, expr_has_unmodelable_features};
        use crate::z3_backend::solver::check_validity;
        use z3::Solver;

        // Check if any expressions have unmodelable features
        let all_exprs: Vec<&&Expr> = antecedents.iter().chain(consequents.iter()).collect();
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
            .unwrap_or(VerificationResult::Unknown {
                clause_desc: desc.to_string(),
                reason: "no result from solver".into(),
            })
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        let _ = (antecedents, consequents);
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
    old_source: &assura_parser::ast::SourceFile,
    new_source: &assura_parser::ast::SourceFile,
) -> Vec<EvolutionResult> {
    use assura_parser::ast::Decl;

    fn collect_contracts(source: &assura_parser::ast::SourceFile) -> Vec<(String, Vec<Clause>)> {
        source
            .decls
            .iter()
            .filter_map(|d| match &d.node {
                Decl::Contract(c) => Some((c.name.clone(), c.clauses.clone())),
                Decl::FnDef(f) => Some((f.name.clone(), f.clauses.clone())),
                Decl::Extern(e) => Some((e.name.clone(), e.clauses.clone())),
                Decl::Bind(b) => Some((b.name.clone(), b.clauses.clone())),
                _ => None,
            })
            .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::*;

    fn make_clause(kind: ClauseKind) -> Clause {
        Clause {
            kind,
            body: Expr::Literal(Literal::Bool(true)),
            effect_variables: vec![],
        }
    }

    fn make_source(decls: Vec<Decl>) -> SourceFile {
        SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: decls
                .into_iter()
                .map(|d| Spanned {
                    node: d,
                    span: 0..1,
                })
                .collect(),
        }
    }

    // ---- has_verifiable_clauses tests ----

    #[test]
    fn has_verifiable_empty_source() {
        let source = make_source(vec![]);
        assert!(!has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_contract_with_ensures() {
        let source = make_source(vec![Decl::Contract(ContractDecl {
            name: "C".into(),
            type_params: vec![],
            clauses: vec![make_clause(ClauseKind::Ensures)],
            fn_params: vec![],
        })]);
        assert!(has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_contract_with_only_input() {
        let source = make_source(vec![Decl::Contract(ContractDecl {
            name: "C".into(),
            type_params: vec![],
            clauses: vec![make_clause(ClauseKind::Input)],
            fn_params: vec![],
        })]);
        assert!(!has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_fndef_with_requires() {
        let source = make_source(vec![Decl::FnDef(FnDef {
            name: "f".into(),
            is_ghost: false,
            is_lemma: false,
            params: vec![],
            return_ty: vec![],
            return_type_expr: None,
            clauses: vec![make_clause(ClauseKind::Requires)],
        })]);
        assert!(has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_extern_with_invariant() {
        let source = make_source(vec![Decl::Extern(ExternDecl {
            name: "e".into(),
            params: vec![],
            return_ty: vec![],
            return_type_expr: None,
            clauses: vec![make_clause(ClauseKind::Invariant)],
        })]);
        assert!(has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_service_operation() {
        let source = make_source(vec![Decl::Service(ServiceDecl {
            name: "S".into(),
            items: vec![ServiceItem::Operation {
                name: "op".into(),
                clauses: vec![make_clause(ClauseKind::Ensures)],
            }],
        })]);
        assert!(has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_service_invariant() {
        let source = make_source(vec![Decl::Service(ServiceDecl {
            name: "S".into(),
            items: vec![ServiceItem::Invariant(Expr::Literal(Literal::Bool(true)))],
        })]);
        assert!(has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_service_query_no_clauses() {
        let source = make_source(vec![Decl::Service(ServiceDecl {
            name: "S".into(),
            items: vec![ServiceItem::Query {
                name: "q".into(),
                clauses: vec![],
            }],
        })]);
        assert!(!has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_block_with_ensures() {
        let source = make_source(vec![Decl::Block {
            kind: BlockKind::Axiomatic,
            name: "b".into(),
            value: None,
            body: vec![make_clause(ClauseKind::Ensures)],
        }]);
        assert!(has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_bind_with_requires() {
        let source = make_source(vec![Decl::Bind(BindDecl {
            name: "bd".into(),
            target_path: "path".into(),
            params: vec![],
            return_ty: vec![],
            return_type_expr: None,
            clauses: vec![make_clause(ClauseKind::Requires)],
        })]);
        assert!(has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_typedef_enum_prophecy() {
        let source = make_source(vec![
            Decl::TypeDef(TypeDef {
                name: "T".into(),
                type_params: vec![],
                body: TypeBody::Alias(vec!["Int".into()]),
            }),
            Decl::EnumDef(EnumDef {
                name: "E".into(),
                type_params: vec![],
                variants: vec![],
            }),
            Decl::Prophecy(ProphecyDecl {
                name: "p".into(),
                ty_tokens: vec![],
            }),
        ]);
        assert!(!has_verifiable_clauses(&source));
    }

    // ---- verify_contract tests ----

    #[test]
    fn verify_contract_no_clauses() {
        let results = verify_contract("Test", &[]);
        assert!(results.is_empty());
    }

    #[test]
    fn verify_contract_input_only() {
        let results = verify_contract("Test", &[make_clause(ClauseKind::Input)]);
        assert!(results.is_empty());
    }

    #[test]
    fn verify_contract_ensures_returns_result() {
        let results = verify_contract("Test", &[make_clause(ClauseKind::Ensures)]);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn verify_contract_with_requires_and_ensures() {
        let results = verify_contract(
            "Test",
            &[
                make_clause(ClauseKind::Requires),
                make_clause(ClauseKind::Ensures),
            ],
        );
        // Only the ensures clause produces a verification result
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn verify_contract_multiple_ensures() {
        let results = verify_contract(
            "Test",
            &[
                make_clause(ClauseKind::Ensures),
                make_clause(ClauseKind::Invariant),
                make_clause(ClauseKind::Rule),
            ],
        );
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn verify_contract_cvc5_solver() {
        let results = verify_contract_with_solver(
            "Test",
            &[make_clause(ClauseKind::Ensures)],
            SolverChoice::Cvc5,
        );
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn verify_contract_portfolio_solver() {
        let results = verify_contract_with_solver(
            "Test",
            &[make_clause(ClauseKind::Ensures)],
            SolverChoice::Portfolio,
        );
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn verify_contract_decreases() {
        let results = verify_contract("Test", &[make_clause(ClauseKind::Decreases)]);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn verify_contract_must_not() {
        let results = verify_contract("Test", &[make_clause(ClauseKind::MustNot)]);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn verify_contract_clause_desc_format() {
        let results = verify_contract("MyContract", &[make_clause(ClauseKind::Ensures)]);
        assert_eq!(results.len(), 1);
        // The description should contain the contract name
        match &results[0] {
            VerificationResult::Verified { clause_desc }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc }
            | VerificationResult::Unknown { clause_desc, .. } => {
                assert!(
                    clause_desc.contains("MyContract"),
                    "clause_desc should contain contract name: {clause_desc}"
                );
            }
        }
    }

    // ---- extract_output_return_type tests ----

    #[test]
    fn extract_output_return_type_nat() {
        let clauses = vec![Clause {
            kind: ClauseKind::Output,
            body: Expr::Raw(vec!["result".into(), ":".into(), "Nat".into()]),
            effect_variables: vec![],
        }];
        assert_eq!(extract_output_return_type(&clauses), vec!["Nat"]);
    }

    #[test]
    fn extract_output_return_type_complex() {
        let clauses = vec![Clause {
            kind: ClauseKind::Output,
            body: Expr::Raw(vec![
                "result".into(),
                ":".into(),
                "List".into(),
                "<".into(),
                "Int".into(),
                ">".into(),
            ]),
            effect_variables: vec![],
        }];
        assert_eq!(
            extract_output_return_type(&clauses),
            vec!["List", "<", "Int", ">"]
        );
    }

    #[test]
    fn extract_output_return_type_no_colon_fallback() {
        // Fallback path: tokens without ":" at position 1 are returned as-is
        let clauses = vec![Clause {
            kind: ClauseKind::Output,
            body: Expr::Raw(vec!["Nat".into()]),
            effect_variables: vec![],
        }];
        assert_eq!(extract_output_return_type(&clauses), vec!["Nat"]);
    }

    #[test]
    fn extract_output_return_type_missing() {
        let clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::Literal(Literal::Bool(true)),
            effect_variables: vec![],
        }];
        assert!(extract_output_return_type(&clauses).is_empty());
    }

    #[test]
    fn extract_output_return_type_non_raw_body() {
        // Output clause with non-Raw body (should be skipped)
        let clauses = vec![Clause {
            kind: ClauseKind::Output,
            body: Expr::Literal(Literal::Bool(true)),
            effect_variables: vec![],
        }];
        assert!(extract_output_return_type(&clauses).is_empty());
    }

    // ---- extract_input_params tests ----

    #[test]
    fn extract_input_params_single() {
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
    fn extract_input_params_multiple() {
        let clauses = vec![Clause {
            kind: ClauseKind::Input,
            body: Expr::Raw(vec![
                "x".into(),
                ":".into(),
                "Int".into(),
                ",".into(),
                "y".into(),
                ":".into(),
                "Nat".into(),
            ]),
            effect_variables: vec![],
        }];
        let params = extract_input_params(&clauses);
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "x");
        assert_eq!(params[0].ty, vec!["Int"]);
        assert_eq!(params[1].name, "y");
        assert_eq!(params[1].ty, vec!["Nat"]);
    }

    #[test]
    fn extract_input_params_no_type() {
        // Parameter without a type annotation
        let clauses = vec![Clause {
            kind: ClauseKind::Input,
            body: Expr::Raw(vec!["x".into()]),
            effect_variables: vec![],
        }];
        let params = extract_input_params(&clauses);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "x");
        assert!(params[0].ty.is_empty());
    }

    #[test]
    fn extract_input_params_empty() {
        let clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::Literal(Literal::Bool(true)),
            effect_variables: vec![],
        }];
        assert!(extract_input_params(&clauses).is_empty());
    }

    #[test]
    fn extract_input_params_non_raw_body() {
        // Input clause with non-Raw body (should be skipped)
        let clauses = vec![Clause {
            kind: ClauseKind::Input,
            body: Expr::Literal(Literal::Bool(true)),
            effect_variables: vec![],
        }];
        assert!(extract_input_params(&clauses).is_empty());
    }

    // ---- #199: Contract evolution verification tests ----

    #[test]
    fn evolution_identical_contracts_pass() {
        // Same requires and ensures; evolution should be compatible
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gt,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gt,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
        ];
        let result = verify_evolution("TestContract", &clauses, &clauses);
        assert!(
            matches!(
                result.precondition_weakening,
                VerificationResult::Verified { .. }
            ),
            "identical preconditions should pass weakening: {:?}",
            result.precondition_weakening
        );
        assert!(
            matches!(
                result.postcondition_strengthening,
                VerificationResult::Verified { .. }
            ),
            "identical postconditions should pass strengthening: {:?}",
            result.postcondition_strengthening
        );
    }

    #[test]
    fn evolution_weakened_precondition_passes() {
        // Old: requires x > 10
        // New: requires x > 0 (weaker, accepts more inputs)
        let old_clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
            },
            effect_variables: vec![],
        }];
        let new_clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
            effect_variables: vec![],
        }];
        let result = verify_evolution("TestContract", &old_clauses, &new_clauses);
        assert!(
            matches!(
                result.precondition_weakening,
                VerificationResult::Verified { .. }
            ),
            "weakened precondition should pass: {:?}",
            result.precondition_weakening
        );
    }

    #[test]
    fn evolution_strengthened_precondition_fails() {
        // Old: requires x > 0
        // New: requires x > 10 (stronger, rejects inputs old accepted)
        let old_clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
            effect_variables: vec![],
        }];
        let new_clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
            },
            effect_variables: vec![],
        }];
        let result = verify_evolution("TestContract", &old_clauses, &new_clauses);
        assert!(
            matches!(
                result.precondition_weakening,
                VerificationResult::Counterexample { .. }
            ),
            "strengthened precondition should fail weakening: {:?}",
            result.precondition_weakening
        );
    }

    #[test]
    fn evolution_dropped_ensures_fails() {
        // Old: ensures x > 0
        // New: no ensures (lost guarantees)
        let old_clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
            effect_variables: vec![],
        }];
        let new_clauses: Vec<Clause> = vec![];
        let result = verify_evolution("TestContract", &old_clauses, &new_clauses);
        assert!(
            matches!(
                result.postcondition_strengthening,
                VerificationResult::Counterexample { .. }
            ),
            "dropping ensures should fail strengthening: {:?}",
            result.postcondition_strengthening
        );
    }

    #[test]
    fn evolution_no_requires_accepts_anything() {
        // Old: no requires (accepts everything)
        // New: requires x > 0 (restricts inputs)
        // This should FAIL weakening because old accepted x = -1 but new rejects it
        let old_clauses: Vec<Clause> = vec![];
        let new_clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
            effect_variables: vec![],
        }];
        let result = verify_evolution("TestContract", &old_clauses, &new_clauses);
        // old has no requires, so old_requires is trivially true.
        // new_requires is x > 0. Is true => x > 0 valid? No (x could be -1).
        assert!(
            matches!(
                result.precondition_weakening,
                VerificationResult::Counterexample { .. }
            ),
            "adding requires to previously unconstrained should fail: {:?}",
            result.precondition_weakening
        );
    }

    #[test]
    fn evolution_new_removes_requires_passes() {
        // Old: requires x > 0
        // New: no requires (accepts everything; strictly weaker)
        let old_clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
            effect_variables: vec![],
        }];
        let new_clauses: Vec<Clause> = vec![];
        let result = verify_evolution("TestContract", &old_clauses, &new_clauses);
        assert!(
            matches!(
                result.precondition_weakening,
                VerificationResult::Verified { .. }
            ),
            "removing requires (accepting everything) should pass: {:?}",
            result.precondition_weakening
        );
    }
}
