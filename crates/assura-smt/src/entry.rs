//! Public entry point functions for SMT verification.
//!
//! Contains `verify()`, `verify_with_options()`, `verify_parallel()`,
//! and all standalone verification functions (refinement, buffer bounds,
//! taint safety, measures, termination).

use assura_parser::ast::Expr;
use assura_types::TypedFile;

use crate::SolverChoice;
use crate::cache::VerificationCache;
use crate::measures::MeasureDefinition;
use crate::result::VerificationResult;

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
    use assura_parser::ast::Decl;
    let mut results = Vec::new();
    for decl in &typed.resolved.source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                results.extend(crate::cvc5_backend::verify_contract_cvc5(
                    &c.name, &c.clauses,
                ));
            }
            Decl::FnDef(f) => {
                results.extend(crate::cvc5_backend::verify_contract_cvc5(
                    &f.name, &f.clauses,
                ));
            }
            Decl::Extern(e) => {
                results.extend(crate::cvc5_backend::verify_contract_cvc5(
                    &e.name, &e.clauses,
                ));
            }
            _ => {}
        }
    }
    results
}

/// Merge portfolio results: prefer Z3 result unless it was Timeout/Unknown,
/// in which case use CVC5 result.
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

    // Collect all verification jobs: (name, clauses) pairs
    let mut jobs: Vec<(String, Vec<assura_parser::ast::Clause>)> = Vec::new();

    for decl in &typed.resolved.source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                jobs.push((c.name.clone(), c.clauses.clone()));
            }
            Decl::FnDef(f) => {
                jobs.push((f.name.clone(), f.clauses.clone()));
            }
            Decl::Extern(e) => {
                jobs.push((e.name.clone(), e.clauses.clone()));
            }
            _ => {}
        }
    }

    // Verify in parallel: each job gets its own solver context
    let per_job_results: Vec<Vec<VerificationResult>> = jobs
        .par_iter()
        .map(|(name, clauses)| {
            // Check cache first
            if let Some(cached) = cache.get(name, clauses) {
                return cached;
            }
            // Cache miss: run solver
            let results = verify_contract_with_solver(name, clauses, solver);
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
