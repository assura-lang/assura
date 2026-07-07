//! Core `verify()` / `Verifier` / contract and standalone verification APIs.

use assura_ast::{
    BindDecl, BlockKind, Clause, ClauseKind, ContractDecl, ExternDecl, FnDef, SpExpr,
};
use assura_types::TypedFile;

use crate::SolverChoice;
use crate::cache::{SessionCache, VerificationCache};
use crate::measures::MeasureDefinition;
use crate::result::VerificationResult;
use crate::verify_context::ContractVerifyContext;

use super::advanced_passes::verify_file_with_cvc5;
#[cfg(feature = "z3-verify")]
use super::advanced_passes::{merge_portfolio_results, verify_portfolio_parallel};
use super::helpers::{VerifyFileExtras, build_verify_extras};
use super::jobs::collect_verification_jobs;

// ---------------------------------------------------------------------------
// Builder API (consolidates file-level verify entry points)
// ---------------------------------------------------------------------------

/// Builder for SMT verification. Consolidates 7 file-level `verify*` functions
/// into a single composable API:
///
/// ```ignore
/// Verifier::new(&typed)
///     .source(path)            // auto-load IR sidecars
///     .solver(SolverChoice::Z3)
///     .cache(&cache)           // enable result caching
///     .parallel()              // enable rayon parallelism
///     .verify()
/// ```
pub struct Verifier<'a> {
    typed: &'a TypedFile,
    source: Option<&'a std::path::Path>,
    inline_extras: Option<&'a crate::ir_loader::LoadedVerifyExtras>,
    options: assura_config::VerifyOptions,
    cache: Option<&'a VerificationCache>,
    parallel: bool,
    include_decrease_checks: bool,
}

impl<'a> Verifier<'a> {
    /// Create a new verifier for a type-checked file.
    pub fn new(typed: &'a TypedFile) -> Self {
        Self {
            typed,
            source: None,
            inline_extras: None,
            options: assura_config::VerifyOptions::default(),
            cache: None,
            parallel: false,
            include_decrease_checks: false,
        }
    }

    /// Set the source file path (auto-loads IR sidecars).
    pub fn source(mut self, path: &'a std::path::Path) -> Self {
        self.source = Some(path);
        self
    }

    /// Set verification options (solver, timeout, layer, parallel, decrease checks).
    ///
    /// Equivalent to [`Self::apply_options`]; prefer that name at new call sites.
    pub fn options(self, options: assura_config::VerifyOptions) -> Self {
        self.apply_options(options)
    }

    /// Set the solver backend.
    pub fn solver(mut self, solver: SolverChoice) -> Self {
        self.options.solver = solver;
        self
    }

    /// Set the per-query solver timeout in milliseconds.
    pub fn timeout_ms(mut self, ms: u64) -> Self {
        self.options.timeout_ms = ms;
        self
    }

    /// Enable result caching.
    pub fn cache(mut self, cache: &'a VerificationCache) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Enable parallel verification using rayon.
    pub fn parallel(mut self) -> Self {
        self.parallel = true;
        self
    }

    /// Inject pre-built IR extras (bypassing disk sidecar loading).
    ///
    /// Used by the AI verification loop (12.01) where IR text is submitted
    /// inline via MCP/CLI rather than discovered as `.ir` sidecar files.
    pub fn with_extras(mut self, extras: &'a crate::ir_loader::LoadedVerifyExtras) -> Self {
        self.inline_extras = Some(extras);
        self
    }

    /// Include pending decrease (termination) checks from the type checker.
    pub fn with_decrease_checks(mut self) -> Self {
        self.include_decrease_checks = true;
        self
    }

    /// Apply all flags from [`assura_config::VerifyOptions`] (solver, timeout,
    /// parallel, decrease checks). Call sites that already have a
    /// `CompilerConfig` / `VerifyOptions` should prefer this over chaining
    /// individual builder methods.
    pub fn apply_options(mut self, options: assura_config::VerifyOptions) -> Self {
        self.parallel = options.parallel;
        self.include_decrease_checks = options.decrease_checks;
        self.options = options;
        self
    }

    /// Run verification and return results.
    pub fn verify(self) -> Vec<VerificationResult> {
        // Prefer inline extras (from AI verification loop) over disk sidecar loading.
        let loaded_storage = if self.inline_extras.is_some() {
            None
        } else {
            self.source
                .map(|path| crate::ir_loader::LoadedVerifyExtras::load(path, self.typed))
        };
        let effective_loaded = self.inline_extras.or(loaded_storage.as_ref());
        let ir_loading_attempted = self.source.is_some() || self.inline_extras.is_some();
        let extras = build_verify_extras(self.typed, effective_loaded, ir_loading_attempted);

        let enable_cache = self.options.enable_cache;
        let mut results = if self.parallel {
            let default_cache;
            let cache = match self.cache {
                Some(c) => c,
                None if enable_cache => {
                    let dir = self
                        .source
                        .and_then(|p| p.parent())
                        .unwrap_or_else(|| std::path::Path::new("."));
                    default_cache = VerificationCache::new(dir);
                    &default_cache
                }
                None => {
                    // enable_cache is off: do not read/write `./.assura-cache`
                    // (the old "ephemeral" path still used disk and served
                    // stale Unknown results after encoding changes; #833).
                    default_cache = VerificationCache::disabled();
                    &default_cache
                }
            };
            verify_parallel_with_solver(self.typed, cache, self.options.solver, Some(&extras))
        } else {
            verify_with_options_impl(self.typed, &self.options, Some(&extras))
        };

        if self.include_decrease_checks {
            results.extend(crate::display::dispatch_decrease_checks(self.typed));
        }

        results
    }
}

/// Verify all contract clauses in a type-checked file (convenience function).
///
/// For custom options, use [`Verifier`] builder instead.
pub fn verify(typed: &TypedFile) -> Vec<VerificationResult> {
    Verifier::new(typed).verify()
}

/// Internal: verify with options (non-parallel path).
fn verify_with_options_impl(
    typed: &TypedFile,
    options: &assura_config::VerifyOptions,
    extras: Option<&VerifyFileExtras<'_>>,
) -> Vec<VerificationResult> {
    match options.solver {
        SolverChoice::Cvc5 => verify_file_with_cvc5(typed, extras),
        SolverChoice::Portfolio => {
            // Run Z3 and CVC5 concurrently, take the best result (#245)
            #[cfg(feature = "z3-verify")]
            {
                verify_portfolio_parallel(typed, options.timeout_ms, extras)
            }
            #[cfg(not(feature = "z3-verify"))]
            {
                verify_file_with_cvc5(typed, extras)
            }
        }
        SolverChoice::Z3 => {
            #[cfg(feature = "z3-verify")]
            {
                crate::z3_backend::verify_impl_with_timeout(typed, options.timeout_ms, extras)
            }
            #[cfg(not(feature = "z3-verify"))]
            {
                let _ = extras;
                crate::no_z3::verify_stub(typed)
            }
        }
    }
}

pub fn has_verifiable_clauses(source: &assura_ast::SourceFile) -> bool {
    use assura_ast::{ClauseKind, DeclVisitor, ServiceDecl};

    fn clauses_verifiable(clauses: &[assura_ast::Clause]) -> bool {
        clauses.iter().any(|c| {
            matches!(
                c.kind,
                ClauseKind::Requires | ClauseKind::Ensures | ClauseKind::Invariant
            )
        })
    }

    struct HasVerifiable(bool);
    impl DeclVisitor for HasVerifiable {
        fn visit_contract(&mut self, c: &ContractDecl) {
            if clauses_verifiable(&c.clauses) {
                self.0 = true;
            }
        }
        fn visit_fn_def(&mut self, f: &FnDef) {
            if clauses_verifiable(&f.clauses) {
                self.0 = true;
            }
        }
        fn visit_extern(&mut self, e: &ExternDecl) {
            if clauses_verifiable(&e.clauses) {
                self.0 = true;
            }
        }
        fn visit_service(&mut self, s: &ServiceDecl) {
            if s.items.iter().any(|item| match item {
                assura_ast::ServiceItem::Operation { clauses, .. }
                | assura_ast::ServiceItem::Query { clauses, .. } => clauses_verifiable(clauses),
                assura_ast::ServiceItem::Invariant(_) => true,
                _ => false,
            }) {
                self.0 = true;
            }
        }
        fn visit_block(
            &mut self,
            _kind: &BlockKind,
            _name: &str,
            _value: &Option<Vec<String>>,
            body: &[Clause],
        ) {
            if clauses_verifiable(body) {
                self.0 = true;
            }
        }
        fn visit_bind(&mut self, b: &BindDecl) {
            if clauses_verifiable(&b.clauses) {
                self.0 = true;
            }
        }
    }

    let mut v = HasVerifiable(false);
    assura_ast::walk_decls(&mut v, &source.decls);
    v.0
}

/// Check if an expression references the `result` identifier.
/// Delegates to the canonical shared implementation in `assura-ast`.
pub(crate) fn expr_references_result(expr: &assura_ast::SpExpr) -> bool {
    assura_ast::expr_references_result(expr)
}

/// When IR loading was attempted but no body exists for a contract,
/// ensures clauses referencing `result` cannot be verified (Z3 treats
/// result as unconstrained). Emit `Unknown` for those clauses so
/// users see "no implementation" instead of spurious counterexamples (#703).
///
/// `ir_loading_attempted` should be `true` when a source path was provided
/// (CLI, pipeline) so IR sidecar discovery ran. When `false` (direct
/// `verify(&typed)` without a source path), the skip is not applied
/// and ensures-with-result go to the solver normally (allowing tests
/// like Nat-return-constrains-result to work).
pub(crate) fn unconstrained_result_unknowns(
    name: &str,
    clauses: &[Clause],
    has_ir: bool,
    ir_loading_attempted: bool,
) -> Vec<VerificationResult> {
    if has_ir || !ir_loading_attempted {
        return Vec::new();
    }
    clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures && expr_references_result(&c.body))
        .map(|_| {
            // #865: actionable guidance for the common postcondition shape.
            VerificationResult::unknown_not_encoded(
                format!("{name}::ensures"),
                format!(
                    "no implementation body for `{name}`; `result` is unconstrained \
                     without IR (add a `.ir` sidecar next to the source, run \
                     `assura build --auto-implement`, or write ensures only over \
                     inputs for pure proofs)"
                ),
            )
        })
        .collect()
}

/// Verify all declarations in parallel using the specified solver.
pub(crate) fn verify_parallel_with_solver(
    typed: &TypedFile,
    cache: &VerificationCache,
    solver: SolverChoice,
    extras: Option<&VerifyFileExtras<'_>>,
) -> Vec<VerificationResult> {
    use rayon::prelude::*;

    let constants = crate::feature_max::collect_feature_max_constants(typed);

    // Collect verification jobs (#213: shared with CVC5 and Z3 paths)
    let jobs = collect_verification_jobs(typed);
    let callee_specs = crate::encode_callee_policy::collect_callee_functional_specs(&jobs);

    // Verify in parallel: each job gets its own solver context
    let per_job_results: Vec<Vec<VerificationResult>> = jobs
        .par_iter()
        .map(|(name, clauses, params, return_ty)| {
            let has_ir = extras
                .and_then(|e| e.ir_bodies)
                .is_some_and(|m| m.contains_key(name.as_str()));
            let ir_fp_owned = extras
                .and_then(|e| e.ir_bodies)
                .and_then(|m| m.get(name.as_str()))
                .map(|f| format!("{f:?}"));
            let ir_fp = ir_fp_owned.as_deref();

            // #703: Skip ensures clauses referencing result when no IR body
            // is loaded. Emit Unknown instead of sending to Z3 where result
            // is unconstrained and produces spurious counterexamples.
            // Only applies when IR loading was attempted (source path provided).
            let ir_loading_attempted = extras.is_some_and(|e| e.ir_loading_attempted);
            let skip_results =
                unconstrained_result_unknowns(name, clauses, has_ir, ir_loading_attempted);
            if !skip_results.is_empty() {
                // Filter out ensures-with-result clauses so the solver only
                // sees clauses it can meaningfully verify.
                let filtered: Vec<Clause> = clauses
                    .iter()
                    .filter(|c| !(c.kind == ClauseKind::Ensures && expr_references_result(&c.body)))
                    .cloned()
                    .collect();
                if filtered
                    .iter()
                    .any(|c| matches!(c.kind, ClauseKind::Ensures | ClauseKind::Invariant))
                {
                    // Still have verifiable clauses after filtering
                    let ctx = ContractVerifyContext {
                        contract_name: name,
                        clauses: &filtered,
                        params,
                        return_ty,
                        constants: &constants,
                        ir: crate::verify_context::LoadedIrContext::for_contract(
                            name,
                            extras,
                            Some(&typed.type_env),
                        ),
                        callee_specs: Some(&callee_specs),
                    };
                    let mut results = verify_contract_with_types_and_solver(&ctx, solver);
                    results.extend(skip_results);
                    // No cache.put: same clauses will always retake the skip
                    // path (unconstrained result with no IR), so cached results
                    // would never be read back (#712).
                    return results;
                }
                return skip_results;
            }

            // Check cache first (IR fingerprint prevents stale hits when only
            // the `.ir` sidecar changed, not the contract clauses).
            if let Some(cached) = cache.get(name, clauses, ir_fp) {
                return cached;
            }
            let ctx = ContractVerifyContext {
                contract_name: name,
                clauses,
                params,
                return_ty,
                constants: &constants,
                ir: crate::verify_context::LoadedIrContext::for_contract(
                    name,
                    extras,
                    Some(&typed.type_env),
                ),
                callee_specs: Some(&callee_specs),
            };
            let results = verify_contract_with_types_and_solver(&ctx, solver);
            cache.put(name, clauses, ir_fp, &results);
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
    clauses: &[assura_ast::Clause],
) -> Vec<VerificationResult> {
    verify_contract_with_solver(contract_name, clauses, SolverChoice::Z3)
}

/// Verify a single contract's clauses using the specified solver.
pub fn verify_contract_with_solver(
    contract_name: &str,
    clauses: &[assura_ast::Clause],
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
                            assura_ast::ClauseKind::Ensures
                                | assura_ast::ClauseKind::Invariant
                                | assura_ast::ClauseKind::Rule
                                | assura_ast::ClauseKind::MustNot
                                | assura_ast::ClauseKind::Decreases
                        )
                    })
                    .map(|c| {
                        let desc = crate::verify_labels::clause_desc(contract_name, &c.kind);
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
            // Run Z3 and CVC5 concurrently per-contract (#245)
            let z3_results = verify_contract_with_solver(contract_name, clauses, SolverChoice::Z3);
            let cvc5_results = crate::cvc5_backend::verify_contract_cvc5(contract_name, clauses);
            #[cfg(feature = "z3-verify")]
            {
                merge_portfolio_results(z3_results, cvc5_results)
            }
            #[cfg(not(feature = "z3-verify"))]
            {
                let _ = z3_results;
                cvc5_results
            }
        }
    }
}

/// Verify a contract with type-level constraints from params and return type.
fn verify_contract_with_types_and_solver(
    ctx: &ContractVerifyContext<'_>,
    solver: SolverChoice,
) -> Vec<VerificationResult> {
    match solver {
        SolverChoice::Z3 => {
            #[cfg(feature = "z3-verify")]
            {
                crate::z3_backend::verify_contract_impl_with_types_and_ir(ctx)
            }
            #[cfg(not(feature = "z3-verify"))]
            {
                let _ = (ctx.constants, ctx.ir_body());
                verify_contract_with_solver(ctx.contract_name, ctx.clauses, solver)
            }
        }
        SolverChoice::Cvc5 | SolverChoice::Portfolio => {
            let mut cache = SessionCache::new();
            crate::cvc5_backend::verify_contract_cvc5_with_lemmas(ctx, None, &mut cache)
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
pub fn check_refinement_subtype(antecedent: &SpExpr, consequent: &SpExpr) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::check_refinement_subtype_impl(antecedent, consequent)
    }
    #[cfg(all(not(feature = "z3-verify"), feature = "cvc5-verify"))]
    {
        crate::cvc5_backend::check_refinement_subtype_cvc5(antecedent, consequent)
    }
    #[cfg(all(not(feature = "z3-verify"), not(feature = "cvc5-verify")))]
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
pub fn verify_buffer_bounds(requires: &[SpExpr], ensures: &SpExpr) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::verify_buffer_bounds_impl(requires, ensures)
    }
    #[cfg(all(not(feature = "z3-verify"), feature = "cvc5-verify"))]
    {
        crate::cvc5_backend::verify_buffer_bounds_cvc5(requires, ensures)
    }
    #[cfg(all(not(feature = "z3-verify"), not(feature = "cvc5-verify")))]
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
    context: &[SpExpr],
    sub_lo: &SpExpr,
    sub_hi: &SpExpr,
    parent_lo: &SpExpr,
    parent_hi: &SpExpr,
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::verify_region_containment_impl(
            context, sub_lo, sub_hi, parent_lo, parent_hi,
        )
    }
    #[cfg(all(not(feature = "z3-verify"), feature = "cvc5-verify"))]
    {
        crate::cvc5_backend::verify_region_containment_cvc5(
            context, sub_lo, sub_hi, parent_lo, parent_hi,
        )
    }
    #[cfg(all(not(feature = "z3-verify"), not(feature = "cvc5-verify")))]
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
    context: &[SpExpr],
    antecedent: &SpExpr,
    consequent: &SpExpr,
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::check_refinement_subtype_with_context_impl(
            context, antecedent, consequent,
        )
    }
    #[cfg(all(not(feature = "z3-verify"), feature = "cvc5-verify"))]
    {
        crate::cvc5_backend::check_refinement_subtype_with_context_cvc5(
            context, antecedent, consequent,
        )
    }
    #[cfg(all(not(feature = "z3-verify"), not(feature = "cvc5-verify")))]
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
    #[cfg(all(not(feature = "z3-verify"), feature = "cvc5-verify"))]
    {
        crate::cvc5_backend::verify_taint_safety_cvc5(taint_labels, validation_fns, sensitive_uses)
    }
    #[cfg(all(not(feature = "z3-verify"), not(feature = "cvc5-verify")))]
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
    requires: &[SpExpr],
    ensures: &SpExpr,
    measures: &[MeasureDefinition],
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::verify_with_measures_impl(requires, ensures, measures)
    }
    #[cfg(all(not(feature = "z3-verify"), feature = "cvc5-verify"))]
    {
        crate::cvc5_backend::verify_with_measures_cvc5(requires, ensures, measures)
    }
    #[cfg(all(not(feature = "z3-verify"), not(feature = "cvc5-verify")))]
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
    preconditions: &[SpExpr],
    measure_expr: &SpExpr,
    call_arg_expr: &SpExpr,
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
    #[cfg(all(not(feature = "z3-verify"), feature = "cvc5-verify"))]
    {
        crate::cvc5_backend::verify_decrease_cvc5(
            preconditions,
            measure_expr,
            call_arg_expr,
            clause_desc,
        )
    }
    #[cfg(all(not(feature = "z3-verify"), not(feature = "cvc5-verify")))]
    {
        let _ = (preconditions, measure_expr, call_arg_expr);
        VerificationResult::Unknown {
            clause_desc,
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests for result classification and success predicates
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::result::{
        KNOWN_SMT_LIMITATION_MARKER, VerificationResult, is_known_smt_limitation,
        not_encoded_reason,
    };

    /// Inline replica of `verification_succeeded` to avoid
    /// the cross-crate type mismatch (assura-pipeline depends on assura-smt,
    /// creating two copies of `VerificationResult` in the test dep graph).
    fn verification_succeeded(results: &[VerificationResult]) -> bool {
        !results.iter().any(|r| {
            matches!(
                r,
                VerificationResult::Counterexample { .. } | VerificationResult::Timeout { .. }
            )
        })
    }

    /// Inline replica of `verification_strict_succeeded`.
    fn verification_strict_succeeded(results: &[VerificationResult]) -> bool {
        if !verification_succeeded(results) {
            return false;
        }
        !results.iter().any(|r| match r {
            VerificationResult::Unknown { reason, .. } => !is_known_smt_limitation(reason),
            _ => false,
        })
    }

    // -- VerificationResult::verified() constructor --

    #[test]
    fn verified_constructor_sets_clause_desc() {
        let r = VerificationResult::verified("SafeDiv::ensures");
        assert!(matches!(
            &r,
            VerificationResult::Verified {
                clause_desc,
                unsat_core: None,
            } if clause_desc == "SafeDiv::ensures"
        ));
    }

    #[test]
    fn verified_constructor_from_string() {
        let desc = String::from("Contract::invariant");
        let r = VerificationResult::verified(desc);
        assert_eq!(r.clause_desc(), "Contract::invariant");
    }

    // -- VerificationResult::unknown_not_encoded() --

    #[test]
    fn unknown_not_encoded_includes_marker() {
        let r = VerificationResult::unknown_not_encoded("Foo::ensures", "linear types");
        match &r {
            VerificationResult::Unknown { reason, .. } => {
                assert!(
                    reason.contains(KNOWN_SMT_LIMITATION_MARKER),
                    "reason should contain marker: {reason}"
                );
                assert!(reason.contains("linear types"));
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn unknown_not_encoded_with_empty_detail() {
        let r = VerificationResult::unknown_not_encoded("Bar::ensures", "");
        match &r {
            VerificationResult::Unknown { reason, .. } => {
                assert_eq!(reason, KNOWN_SMT_LIMITATION_MARKER);
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    // -- VerificationResult::no_solver_result() --

    #[test]
    fn no_solver_result_is_unknown() {
        let r = VerificationResult::no_solver_result("Empty::ensures");
        assert!(matches!(&r, VerificationResult::Unknown { reason, .. }
            if reason == crate::result::NO_SOLVER_RESULT_REASON
        ));
    }

    // -- clause_desc() and contract_name() accessors --

    #[test]
    fn clause_desc_accessor_all_variants() {
        let cases: Vec<VerificationResult> = vec![
            VerificationResult::verified("A::ensures"),
            VerificationResult::Counterexample {
                clause_desc: "B::invariant".into(),
                model: "x=0".into(),
                counter_model: None,
            },
            VerificationResult::Timeout {
                clause_desc: "C::ensures".into(),
            },
            VerificationResult::Unknown {
                clause_desc: "D::rule".into(),
                reason: "unknown".into(),
            },
        ];
        let expected = ["A::ensures", "B::invariant", "C::ensures", "D::rule"];
        for (r, exp) in cases.iter().zip(expected.iter()) {
            assert_eq!(r.clause_desc(), *exp);
        }
    }

    #[test]
    fn contract_name_extracts_prefix() {
        let r = VerificationResult::verified("SafeDivision::ensures");
        assert_eq!(r.contract_name(), "SafeDivision");
    }

    #[test]
    fn contract_name_no_separator() {
        let r = VerificationResult::verified("standalone");
        assert_eq!(r.contract_name(), "standalone");
    }

    // -- is_known_limitation() instance method --

    #[test]
    fn is_known_limitation_true_for_marker() {
        let r = VerificationResult::unknown_not_encoded("X::ensures", "effects");
        assert!(r.is_known_limitation());
    }

    #[test]
    fn is_known_limitation_false_for_other_unknown() {
        let r = VerificationResult::Unknown {
            clause_desc: "X::ensures".into(),
            reason: "non-linear arithmetic".into(),
        };
        assert!(!r.is_known_limitation());
    }

    #[test]
    fn is_known_limitation_false_for_non_unknown_variants() {
        assert!(!VerificationResult::verified("A").is_known_limitation());
        assert!(
            !VerificationResult::Timeout {
                clause_desc: "B".into()
            }
            .is_known_limitation()
        );
        assert!(
            !VerificationResult::Counterexample {
                clause_desc: "C".into(),
                model: String::new(),
                counter_model: None,
            }
            .is_known_limitation()
        );
    }

    // -- is_known_smt_limitation() free function --

    #[test]
    fn known_smt_limitation_with_exact_marker() {
        assert!(is_known_smt_limitation(KNOWN_SMT_LIMITATION_MARKER));
    }

    #[test]
    fn known_smt_limitation_with_marker_embedded() {
        let reason = format!("linear types {KNOWN_SMT_LIMITATION_MARKER}");
        assert!(is_known_smt_limitation(&reason));
    }

    #[test]
    fn known_smt_limitation_false_for_unrelated() {
        assert!(!is_known_smt_limitation("non-linear arithmetic"));
        assert!(!is_known_smt_limitation("timeout fallback"));
        assert!(!is_known_smt_limitation(""));
    }

    // -- not_encoded_reason() --

    #[test]
    fn not_encoded_reason_with_detail() {
        let r = not_encoded_reason("typestate transitions");
        assert!(r.contains("typestate transitions"));
        assert!(r.contains(KNOWN_SMT_LIMITATION_MARKER));
    }

    #[test]
    fn not_encoded_reason_already_contains_marker() {
        let input = format!("already {KNOWN_SMT_LIMITATION_MARKER}");
        let r = not_encoded_reason(&input);
        // Should not double the marker
        assert_eq!(r, input);
    }

    // -- verification_succeeded (lenient) from assura_pipeline --

    #[test]
    fn succeeded_empty_results() {
        assert!(verification_succeeded(&[]));
    }

    #[test]
    fn succeeded_all_verified() {
        let results = vec![
            VerificationResult::verified("A::ensures"),
            VerificationResult::verified("B::ensures"),
        ];
        let ok = verification_succeeded(&results);
        assert!(ok);
    }

    #[test]
    fn succeeded_fails_on_counterexample() {
        let results: Vec<VerificationResult> = vec![
            VerificationResult::verified("A::ensures"),
            VerificationResult::Counterexample {
                clause_desc: "B::ensures".into(),
                model: "x=0".into(),
                counter_model: None,
            },
        ];
        let ok = verification_succeeded(&results);
        assert!(!ok);
    }

    #[test]
    fn succeeded_fails_on_timeout() {
        let results: Vec<VerificationResult> = vec![VerificationResult::Timeout {
            clause_desc: "Slow::ensures".into(),
        }];
        let ok = verification_succeeded(&results);
        assert!(!ok);
    }

    #[test]
    fn succeeded_allows_unknown_with_any_reason() {
        // Lenient: Unknown does not cause failure (only CE and Timeout do)
        let results: Vec<VerificationResult> = vec![
            VerificationResult::verified("A::ensures"),
            VerificationResult::Unknown {
                clause_desc: "B::ensures".into(),
                reason: "non-linear arithmetic".into(),
            },
        ];
        let ok = verification_succeeded(&results);
        assert!(ok);
    }

    #[test]
    fn succeeded_allows_known_limitation() {
        let results: Vec<VerificationResult> = vec![VerificationResult::unknown_not_encoded(
            "C::ensures",
            "effect types",
        )];
        let ok = verification_succeeded(&results);
        assert!(ok);
    }

    // -- verification_strict_succeeded from assura_pipeline --

    #[test]
    fn strict_succeeded_empty_results() {
        assert!(verification_strict_succeeded(&[]));
    }

    #[test]
    fn strict_succeeded_all_verified() {
        let results: Vec<VerificationResult> = vec![
            VerificationResult::verified("A::ensures"),
            VerificationResult::verified("B::ensures"),
        ];
        let ok = verification_strict_succeeded(&results);
        assert!(ok);
    }

    #[test]
    fn strict_succeeded_allows_known_limitation() {
        let results: Vec<VerificationResult> = vec![
            VerificationResult::verified("A::ensures"),
            VerificationResult::unknown_not_encoded("B::ensures", "linear types"),
        ];
        let ok = verification_strict_succeeded(&results);
        assert!(ok);
    }

    #[test]
    fn strict_succeeded_rejects_non_limitation_unknown() {
        let results: Vec<VerificationResult> = vec![
            VerificationResult::verified("A::ensures"),
            VerificationResult::Unknown {
                clause_desc: "B::ensures".into(),
                reason: "non-linear arithmetic".into(),
            },
        ];
        let ok = verification_strict_succeeded(&results);
        assert!(!ok);
    }

    #[test]
    fn strict_succeeded_rejects_counterexample() {
        let results: Vec<VerificationResult> = vec![VerificationResult::Counterexample {
            clause_desc: "X::ensures".into(),
            model: "x = -1".into(),
            counter_model: None,
        }];
        let ok = verification_strict_succeeded(&results);
        assert!(!ok);
    }

    // -- Mixed vectors --

    #[test]
    fn mixed_results_lenient_vs_strict() {
        let results: Vec<VerificationResult> = vec![
            VerificationResult::verified("A::ensures"),
            VerificationResult::unknown_not_encoded("B::ensures", "effects"),
            VerificationResult::Unknown {
                clause_desc: "C::ensures".into(),
                reason: "solver gave up".into(),
            },
        ];
        // Lenient: passes (no CE or Timeout)
        let lenient = verification_succeeded(&results);
        assert!(lenient);
        // Strict: fails (C has non-limitation Unknown)
        let strict = verification_strict_succeeded(&results);
        assert!(!strict);
    }

    // -- verified_with_core() --

    #[test]
    fn verified_with_core_stores_core() {
        let r = VerificationResult::verified_with_core(
            "X::ensures",
            vec!["req_0".into(), "req_1".into()],
        );
        match &r {
            VerificationResult::Verified { unsat_core, .. } => {
                let core = unsat_core.as_ref().expect("should have unsat core");
                assert_eq!(core.len(), 2);
                assert_eq!(core[0], "req_0");
            }
            other => panic!("expected Verified, got {other:?}"),
        }
    }

    #[test]
    fn verified_with_core_empty_is_none() {
        let r = VerificationResult::verified_with_core("X::ensures", vec![]);
        match &r {
            VerificationResult::Verified { unsat_core, .. } => {
                assert!(unsat_core.is_none(), "empty core should be None");
            }
            other => panic!("expected Verified, got {other:?}"),
        }
    }

    // -- expr_references_result (#703) --

    use super::{expr_references_result, unconstrained_result_unknowns};
    use assura_ast::{BinOp, Clause, ClauseKind, Expr, Literal, Spanned};

    fn sp(e: Expr) -> assura_ast::SpExpr {
        Spanned::no_span(e)
    }

    #[test]
    fn expr_references_result_ident() {
        assert!(expr_references_result(&sp(Expr::Ident("result".into()))));
    }

    #[test]
    fn expr_references_result_other_ident() {
        assert!(!expr_references_result(&sp(Expr::Ident("x".into()))));
    }

    #[test]
    fn expr_references_result_in_binop() {
        let expr = sp(Expr::BinOp {
            lhs: Box::new(sp(Expr::Ident("result".into()))),
            op: BinOp::Gte,
            rhs: Box::new(sp(Expr::Literal(Literal::Int("0".into())))),
        });
        assert!(expr_references_result(&expr));
    }

    #[test]
    fn expr_references_result_nested_field() {
        // result.length()
        let expr = sp(Expr::MethodCall {
            receiver: Box::new(sp(Expr::Ident("result".into()))),
            method: "length".into(),
            args: vec![],
        });
        assert!(expr_references_result(&expr));
    }

    #[test]
    fn expr_references_result_absent() {
        let expr = sp(Expr::BinOp {
            lhs: Box::new(sp(Expr::Ident("x".into()))),
            op: BinOp::Add,
            rhs: Box::new(sp(Expr::Ident("y".into()))),
        });
        assert!(!expr_references_result(&expr));
    }

    #[test]
    fn expr_references_result_in_if() {
        let expr = sp(Expr::If {
            cond: Box::new(sp(Expr::Ident("flag".into()))),
            then_branch: Box::new(sp(Expr::Ident("result".into()))),
            else_branch: None,
        });
        assert!(expr_references_result(&expr));
    }

    // -- unconstrained_result_unknowns (#703) --

    fn ensures_clause(body: assura_ast::SpExpr) -> Clause {
        Clause {
            kind: ClauseKind::Ensures,
            body,
            effect_variables: vec![],
        }
    }

    fn requires_clause(body: assura_ast::SpExpr) -> Clause {
        Clause {
            kind: ClauseKind::Requires,
            body,
            effect_variables: vec![],
        }
    }

    #[test]
    fn unconstrained_result_unknowns_no_ir_emits_unknown() {
        let clauses = vec![
            requires_clause(sp(Expr::Literal(Literal::Bool(true)))),
            ensures_clause(sp(Expr::BinOp {
                lhs: Box::new(sp(Expr::Ident("result".into()))),
                op: BinOp::Gte,
                rhs: Box::new(sp(Expr::Literal(Literal::Int("0".into())))),
            })),
        ];
        // IR loading attempted but no body for this contract
        let unknowns = unconstrained_result_unknowns("Clamp", &clauses, false, true);
        assert_eq!(unknowns.len(), 1);
        assert!(unknowns[0].is_known_limitation());
        assert!(unknowns[0].clause_desc().contains("ensures"));
        let reason = match &unknowns[0] {
            VerificationResult::Unknown { reason, .. } => reason.as_str(),
            other => panic!("expected Unknown, got {other:?}"),
        };
        assert!(
            reason.contains("result") && reason.contains("unconstrained"),
            "reason should explain unconstrained result: {reason}"
        );
        assert!(
            reason.contains("auto-implement") || reason.contains(".ir"),
            "reason should point at IR / auto-implement: {reason}"
        );
    }

    #[test]
    fn unconstrained_result_unknowns_with_ir_returns_empty() {
        let clauses = vec![ensures_clause(sp(Expr::BinOp {
            lhs: Box::new(sp(Expr::Ident("result".into()))),
            op: BinOp::Gte,
            rhs: Box::new(sp(Expr::Literal(Literal::Int("0".into())))),
        }))];
        let unknowns = unconstrained_result_unknowns("Clamp", &clauses, true, true);
        assert!(unknowns.is_empty());
    }

    #[test]
    fn unconstrained_result_unknowns_skips_non_result_ensures() {
        // ensures { x >= 0 } does not reference result, should not be skipped
        let clauses = vec![ensures_clause(sp(Expr::BinOp {
            lhs: Box::new(sp(Expr::Ident("x".into()))),
            op: BinOp::Gte,
            rhs: Box::new(sp(Expr::Literal(Literal::Int("0".into())))),
        }))];
        let unknowns = unconstrained_result_unknowns("Test", &clauses, false, true);
        assert!(unknowns.is_empty());
    }

    #[test]
    fn unconstrained_result_unknowns_multiple_ensures() {
        let clauses = vec![
            ensures_clause(sp(Expr::BinOp {
                lhs: Box::new(sp(Expr::Ident("result".into()))),
                op: BinOp::Gte,
                rhs: Box::new(sp(Expr::Ident("lo".into()))),
            })),
            ensures_clause(sp(Expr::BinOp {
                lhs: Box::new(sp(Expr::Ident("result".into()))),
                op: BinOp::Lte,
                rhs: Box::new(sp(Expr::Ident("hi".into()))),
            })),
            // This one does not reference result
            ensures_clause(sp(Expr::BinOp {
                lhs: Box::new(sp(Expr::Ident("lo".into()))),
                op: BinOp::Lte,
                rhs: Box::new(sp(Expr::Ident("hi".into()))),
            })),
        ];
        // IR loading attempted, should emit 2 unknowns for result-referencing clauses
        let unknowns = unconstrained_result_unknowns("Clamp", &clauses, false, true);
        assert_eq!(unknowns.len(), 2);
    }

    #[test]
    fn unconstrained_result_unknowns_no_loading_attempted() {
        // When IR loading was not attempted (no source path), skip logic
        // should not fire even for ensures referencing result.
        let clauses = vec![ensures_clause(sp(Expr::BinOp {
            lhs: Box::new(sp(Expr::Ident("result".into()))),
            op: BinOp::Gte,
            rhs: Box::new(sp(Expr::Literal(Literal::Int("0".into())))),
        }))];
        let unknowns = unconstrained_result_unknowns("Test", &clauses, false, false);
        assert!(
            unknowns.is_empty(),
            "should not skip when IR loading was not attempted"
        );
    }
}
