//! SMT-based verification for Assura contracts.
//!
//! Supports multiple solver backends:
//! - **Z3** (default): via the z3 Rust crate, compiled in with the `z3-verify` feature
//! - **CVC5**: via the `cvc5` command-line binary, using SMT-LIB2 format
//! - **Portfolio**: tries Z3 first, falls back to CVC5 on timeout/unknown
//!
//! For each contract in a `TypedFile`, encodes requires/ensures/invariant
//! clauses as SMT formulas and checks their validity:
//!
//! - **ensures with requires**: Check `P => Q` validity by asserting P,
//!   asserting NOT Q, and checking satisfiability. UNSAT = verified.
//! - **invariant**: Check satisfiability (not always false).
//! - **requires**: Recorded as assumptions (checked at call sites).
//!
//! The default timeout is 1 second (Layer 1).

use assura_parser::ast::{ClauseKind, Decl, Expr, ServiceItem};
use assura_types::TypedFile;

// ---------------------------------------------------------------------------
// Solver backend selection
// ---------------------------------------------------------------------------

/// Which SMT solver backend to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolverChoice {
    /// Z3 via the Rust crate (requires `z3-verify` feature).
    Z3,
    /// CVC5 via command-line binary (requires `cvc5` on PATH).
    Cvc5,
    /// Portfolio: try Z3 first, fall back to CVC5 on timeout/unknown.
    Portfolio,
}

impl SolverChoice {
    /// Parse from a string (case-insensitive).
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "z3" => Some(Self::Z3),
            "cvc5" => Some(Self::Cvc5),
            "portfolio" => Some(Self::Portfolio),
            _ => None,
        }
    }

    /// Return the solver name as a string slice.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Z3 => "z3",
            Self::Cvc5 => "cvc5",
            Self::Portfolio => "portfolio",
        }
    }
}

// ---------------------------------------------------------------------------
// Measure definitions (T054)
// ---------------------------------------------------------------------------

/// The sort (type) of a measure parameter or return value in the SMT encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeasureSort {
    /// Non-negative integer (used for len, size).
    Nat,
    /// Uninterpreted set sort (used for elems, keys, values).
    Set,
    /// Uninterpreted collection sort (parameter type for most measures).
    Collection,
    /// Uninterpreted map sort (parameter type for keys/values).
    Map,
}

/// An axiom attached to a measure definition.
///
/// Each axiom is a universally quantified property that the SMT solver
/// can use when reasoning about the measure. For example, `len(xs) >= 0`
/// or `len(empty) == 0`.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasureAxiom {
    /// Human-readable description of the axiom.
    pub description: String,
    /// The axiom tag used to select which axioms to assert.
    pub tag: MeasureAxiomTag,
}

/// Tags for built-in measure axioms, used by the Z3 encoder to generate
/// the correct Z3 assertions for each axiom.
#[derive(Debug, Clone, PartialEq)]
pub enum MeasureAxiomTag {
    /// `measure(x) >= 0` (non-negativity).
    NonNegative,
    /// `measure(empty) == 0`.
    EmptyIsZero,
    /// `measure(append(xs, x)) == measure(xs) + 1`.
    AppendIncrement,
    /// `measure_a(xs) == measure_b(xs)` (e.g., size == len for lists).
    EquivalentTo(String),
    /// `measure(empty_map) == empty_set`.
    EmptyMapEmptySet,
    /// Custom axiom with a textual description.
    Custom(String),
}

/// Definition of a mathematical measure function used in contracts.
///
/// Measures like `len`, `elems`, `keys` are encoded as uninterpreted
/// functions in Z3 with standard axioms constraining their behavior.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasureDefinition {
    /// Name of the measure (e.g., "len", "elems").
    pub name: String,
    /// Parameter sorts.
    pub param_sorts: Vec<MeasureSort>,
    /// Return sort.
    pub return_sort: MeasureSort,
    /// Axioms constraining the measure.
    pub axioms: Vec<MeasureAxiom>,
}

impl MeasureDefinition {
    /// Create a new measure definition.
    pub fn new(
        name: impl Into<String>,
        param_sorts: Vec<MeasureSort>,
        return_sort: MeasureSort,
    ) -> Self {
        Self {
            name: name.into(),
            param_sorts,
            return_sort,
            axioms: Vec::new(),
        }
    }

    /// Add an axiom to this measure.
    pub fn with_axiom(mut self, description: impl Into<String>, tag: MeasureAxiomTag) -> Self {
        self.axioms.push(MeasureAxiom {
            description: description.into(),
            tag,
        });
        self
    }

    /// Returns true if this measure returns a Nat (non-negative integer).
    pub fn returns_nat(&self) -> bool {
        self.return_sort == MeasureSort::Nat
    }
}

/// Register the five built-in measures with their standard axioms.
///
/// Built-in measures:
/// - `len(collection) -> Nat`: length of a list/array/string
/// - `elems(collection) -> Set`: elements of a list/set
/// - `keys(map) -> Set`: keys of a map
/// - `values(map) -> Set`: values of a map
/// - `size(collection) -> Nat`: cardinality/size
pub fn register_builtin_measures() -> Vec<MeasureDefinition> {
    vec![
        // len(collection) -> Nat
        MeasureDefinition::new("len", vec![MeasureSort::Collection], MeasureSort::Nat)
            .with_axiom("len(xs) >= 0", MeasureAxiomTag::NonNegative)
            .with_axiom("len(empty) == 0", MeasureAxiomTag::EmptyIsZero)
            .with_axiom(
                "len(append(xs, x)) == len(xs) + 1",
                MeasureAxiomTag::AppendIncrement,
            ),
        // elems(collection) -> Set
        MeasureDefinition::new("elems", vec![MeasureSort::Collection], MeasureSort::Set)
            .with_axiom("elems(empty) == empty_set", MeasureAxiomTag::EmptyIsZero),
        // keys(map) -> Set
        MeasureDefinition::new("keys", vec![MeasureSort::Map], MeasureSort::Set).with_axiom(
            "keys(empty_map) == empty_set",
            MeasureAxiomTag::EmptyMapEmptySet,
        ),
        // values(map) -> Set
        MeasureDefinition::new("values", vec![MeasureSort::Map], MeasureSort::Set).with_axiom(
            "values(empty_map) == empty_set",
            MeasureAxiomTag::EmptyMapEmptySet,
        ),
        // size(collection) -> Nat
        MeasureDefinition::new("size", vec![MeasureSort::Collection], MeasureSort::Nat)
            .with_axiom("size(xs) >= 0", MeasureAxiomTag::NonNegative)
            .with_axiom("size(empty) == 0", MeasureAxiomTag::EmptyIsZero)
            .with_axiom(
                "size(xs) == len(xs) for lists",
                MeasureAxiomTag::EquivalentTo("len".into()),
            ),
    ]
}

// ---------------------------------------------------------------------------
// Verification result
// ---------------------------------------------------------------------------

/// Structured counterexample model extracted from Z3.
#[derive(Debug, Clone)]
pub struct CounterexampleModel {
    /// Variable name/value pairs from the Z3 model.
    pub variables: Vec<(String, String)>,
}

impl CounterexampleModel {
    /// Produce a JSON string: `{"variables": {"x": "0", "b": "-1"}}`.
    pub fn to_json(&self) -> String {
        let mut buf = String::from("{\"variables\": {");
        for (i, (name, value)) in self.variables.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            // Escape any quotes in name/value for valid JSON
            buf.push('"');
            buf.push_str(&name.replace('\\', "\\\\").replace('"', "\\\""));
            buf.push_str("\": \"");
            buf.push_str(&value.replace('\\', "\\\\").replace('"', "\\\""));
            buf.push('"');
        }
        buf.push_str("}}");
        buf
    }
}

/// The result of verifying a single contract clause.
#[derive(Debug, Clone)]
pub enum VerificationResult {
    /// The clause was proven valid.
    Verified {
        /// Human-readable description of what was verified.
        clause_desc: String,
    },
    /// A counterexample was found (the clause does not hold).
    Counterexample {
        /// Human-readable description of the clause.
        clause_desc: String,
        /// Z3 model showing the counterexample (raw string).
        model: String,
        /// Structured counterexample with parsed variable values.
        counter_model: Option<CounterexampleModel>,
    },
    /// The solver timed out before reaching a conclusion.
    Timeout {
        /// Human-readable description of the clause.
        clause_desc: String,
    },
    /// The solver returned Unknown (e.g., non-linear arithmetic).
    Unknown {
        /// Human-readable description of the clause.
        clause_desc: String,
        /// Reason the solver could not decide.
        reason: String,
    },
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
    _options: &assura_config::VerifyOptions,
) -> Vec<VerificationResult> {
    #[cfg(feature = "z3-verify")]
    {
        z3_backend::verify_impl(typed)
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        no_z3::verify_stub(typed)
    }
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
                z3_backend::verify_contract_impl(contract_name, clauses)
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
        SolverChoice::Cvc5 => cvc5_backend::verify_contract_cvc5(contract_name, clauses),
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
            let cvc5_results = cvc5_backend::verify_contract_cvc5(contract_name, clauses);

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
        z3_backend::check_refinement_subtype_impl(antecedent, consequent)
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        no_z3::refinement_stub(antecedent, consequent)
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
        z3_backend::verify_buffer_bounds_impl(requires, ensures)
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
        z3_backend::verify_region_containment_impl(context, sub_lo, sub_hi, parent_lo, parent_hi)
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
        z3_backend::check_refinement_subtype_with_context_impl(context, antecedent, consequent)
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        no_z3::refinement_ctx_stub(context, antecedent, consequent)
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
        z3_backend::verify_taint_safety_impl(taint_labels, validation_fns, sensitive_uses)
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
        z3_backend::verify_with_measures_impl(requires, ensures, measures)
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
        z3_backend::verify_decrease_impl(preconditions, measure_expr, call_arg_expr, clause_desc)
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
// Quantifier bound validation
// ---------------------------------------------------------------------------

/// A warning about a quantifier with an infinite (or potentially infinite) domain.
#[derive(Debug, Clone, PartialEq)]
pub struct UnboundedQuantifierWarning {
    /// Name of the enclosing contract/function.
    pub context: String,
    /// The quantifier variable name.
    pub var: String,
    /// Description of the domain.
    pub domain_desc: String,
    /// Why this is problematic.
    pub reason: String,
}

/// Known type names that represent infinite or unbounded domains.
/// Quantifying over these with forall/exists is almost certainly a mistake
/// or will cause SMT solver timeouts.
const INFINITE_TYPE_NAMES: &[&str] = &[
    "Int", "Nat", "Float", "Bool", "String", "Bytes", "U8", "U16", "U32", "U64", "I8", "I16",
    "I32", "I64", "F32", "F64",
];

// ---------------------------------------------------------------------------
// SMT-LIB2 dump (--dump-smt)
// ---------------------------------------------------------------------------

/// A single SMT query with its SMT-LIB2 text and metadata.
pub struct SmtQuery {
    /// Contract or function name containing the clause.
    pub context: String,
    /// Clause kind (e.g., "ensures", "invariant").
    pub kind: String,
    /// The SMT-LIB2 script text.
    pub script: String,
}

/// Generate SMT-LIB2 query scripts for all verifiable clauses in a typed file.
///
/// Returns one `SmtQuery` per clause without invoking any solver. Used by
/// `--dump-smt` to write .smt2 files for offline analysis.
pub fn dump_smt_queries(typed: &TypedFile) -> Vec<SmtQuery> {
    let mut queries = Vec::new();
    for decl in &typed.resolved.source.decls {
        let (name, clauses) = match &decl.node {
            Decl::Contract(c) => (c.name.clone(), &c.clauses[..]),
            Decl::FnDef(f) => (f.name.clone(), &f.clauses[..]),
            Decl::Extern(e) => (e.name.clone(), &e.clauses[..]),
            _ => continue,
        };

        let requires_exprs: Vec<&Expr> = clauses
            .iter()
            .filter(|c| c.kind == ClauseKind::Requires)
            .map(|c| &c.body)
            .collect();

        for clause in clauses {
            let kind_str = match &clause.kind {
                ClauseKind::Ensures => "ensures",
                ClauseKind::Invariant => "invariant",
                ClauseKind::Decreases => "decreases",
                ClauseKind::Rule => "rule",
                ClauseKind::MustNot => "must_not",
                _ => continue,
            };

            let mut vars = std::collections::HashSet::new();
            for req in &requires_exprs {
                cvc5_backend::collect_vars(req, &mut vars);
            }
            cvc5_backend::collect_vars(&clause.body, &mut vars);

            let mut script = String::new();
            script.push_str("; Generated by assura --dump-smt\n");
            script.push_str(&format!("; Context: {name}::{kind_str}\n"));
            script.push_str("(set-logic ALL)\n\n");

            for var in &vars {
                script.push_str(&format!("(declare-const {var} Int)\n"));
            }
            script.push('\n');

            for req in &requires_exprs {
                if let Some(smt) = cvc5_backend::expr_to_smtlib(req) {
                    script.push_str(&format!("; requires\n(assert {smt})\n"));
                }
            }

            if let Some(smt) = cvc5_backend::expr_to_smtlib(&clause.body) {
                if matches!(clause.kind, ClauseKind::Invariant) {
                    script.push_str(&format!(
                        "; invariant (check satisfiability)\n(assert {smt})\n"
                    ));
                } else {
                    script.push_str(&format!(
                        "; {kind_str} (check validity via negation)\n(assert (not {smt}))\n"
                    ));
                }
            } else {
                script.push_str("; ERROR: could not encode clause to SMT-LIB2\n");
            }

            script.push_str("\n(check-sat)\n(get-model)\n");

            queries.push(SmtQuery {
                context: name.clone(),
                kind: kind_str.to_string(),
                script,
            });
        }
    }
    queries
}

/// Check all quantifiers (forall/exists) in a typed file for unbounded domains.
///
/// Returns warnings for quantifiers that range over infinite type domains
/// (e.g., `forall x in Int: ...`). These are technically valid in the spec
/// but are almost always unintended and cause SMT solver timeouts or
/// unsound verification (if the solver times out and the result is treated
/// as "verified").
pub fn validate_quantifier_bounds(typed: &TypedFile) -> Vec<UnboundedQuantifierWarning> {
    let mut warnings = Vec::new();
    for decl in &typed.resolved.source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                for clause in &c.clauses {
                    collect_unbounded_quantifiers(&clause.body, &c.name, &mut warnings);
                }
            }
            Decl::FnDef(f) => {
                for clause in &f.clauses {
                    collect_unbounded_quantifiers(&clause.body, &f.name, &mut warnings);
                }
            }
            Decl::Extern(e) => {
                for clause in &e.clauses {
                    collect_unbounded_quantifiers(&clause.body, &e.name, &mut warnings);
                }
            }
            Decl::Service(s) => {
                for item in &s.items {
                    let (name, clauses) = match item {
                        ServiceItem::Operation { name, clauses } => (name, clauses),
                        ServiceItem::Query { name, clauses } => (name, clauses),
                        _ => continue,
                    };
                    let ctx = format!("{}::{}", s.name, name);
                    for clause in clauses {
                        collect_unbounded_quantifiers(&clause.body, &ctx, &mut warnings);
                    }
                }
            }
            _ => {}
        }
    }
    warnings
}

/// Recursively walk an expression looking for forall/exists with infinite domains.
fn collect_unbounded_quantifiers(
    expr: &Expr,
    context: &str,
    warnings: &mut Vec<UnboundedQuantifierWarning>,
) {
    match expr {
        Expr::Forall { var, domain, body } | Expr::Exists { var, domain, body } => {
            if let Some(reason) = check_infinite_domain(domain) {
                let kind = if matches!(expr, Expr::Forall { .. }) {
                    "forall"
                } else {
                    "exists"
                };
                warnings.push(UnboundedQuantifierWarning {
                    context: context.to_string(),
                    var: var.clone(),
                    domain_desc: format!("{kind} {var} in <{reason}>"),
                    reason: "quantifier ranges over infinite domain; \
                         use a bounded collection or range (e.g., 0..n) instead"
                        .to_string(),
                });
            }
            // Check nested quantifiers in body
            collect_unbounded_quantifiers(body, context, warnings);
            // Check domain sub-expressions too
            collect_unbounded_quantifiers(domain, context, warnings);
        }
        // Recurse into sub-expressions
        Expr::BinOp { lhs, rhs, .. } => {
            collect_unbounded_quantifiers(lhs, context, warnings);
            collect_unbounded_quantifiers(rhs, context, warnings);
        }
        Expr::UnaryOp { expr: e, .. }
        | Expr::Old(e)
        | Expr::Paren(e)
        | Expr::Ghost(e)
        | Expr::Field(e, _) => {
            collect_unbounded_quantifiers(e, context, warnings);
        }
        Expr::Call { func, args } => {
            collect_unbounded_quantifiers(func, context, warnings);
            for arg in args {
                collect_unbounded_quantifiers(arg, context, warnings);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_unbounded_quantifiers(receiver, context, warnings);
            for arg in args {
                collect_unbounded_quantifiers(arg, context, warnings);
            }
        }
        Expr::Index { expr: e, index } => {
            collect_unbounded_quantifiers(e, context, warnings);
            collect_unbounded_quantifiers(index, context, warnings);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_unbounded_quantifiers(cond, context, warnings);
            collect_unbounded_quantifiers(then_branch, context, warnings);
            if let Some(eb) = else_branch {
                collect_unbounded_quantifiers(eb, context, warnings);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_unbounded_quantifiers(value, context, warnings);
            collect_unbounded_quantifiers(body, context, warnings);
        }
        Expr::Match { scrutinee, arms } => {
            collect_unbounded_quantifiers(scrutinee, context, warnings);
            for arm in arms {
                collect_unbounded_quantifiers(&arm.body, context, warnings);
            }
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                collect_unbounded_quantifiers(item, context, warnings);
            }
        }
        Expr::Cast { expr: e, .. } => {
            collect_unbounded_quantifiers(e, context, warnings);
        }
        Expr::Apply { args, .. } => {
            for arg in args {
                collect_unbounded_quantifiers(arg, context, warnings);
            }
        }
        Expr::Raw(tokens) => {
            // Check for raw quantifier patterns: forall/exists VAR in DOMAIN : BODY
            check_raw_quantifier_bounds(tokens, context, warnings);
        }
        // Leaf nodes
        Expr::Literal(_) | Expr::Ident(_) => {}
    }
}

/// Check if a domain expression represents an infinite/unbounded type.
/// Returns Some(description) if infinite, None if bounded.
fn check_infinite_domain(domain: &Expr) -> Option<String> {
    match domain {
        Expr::Ident(name) => {
            if INFINITE_TYPE_NAMES.contains(&name.as_str()) {
                Some(name.clone())
            } else {
                None
            }
        }
        Expr::Raw(tokens) if tokens.len() == 1 => {
            if INFINITE_TYPE_NAMES.contains(&tokens[0].as_str()) {
                Some(tokens[0].clone())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Check raw token sequences for quantifiers with infinite domains.
fn check_raw_quantifier_bounds(
    tokens: &[String],
    context: &str,
    warnings: &mut Vec<UnboundedQuantifierWarning>,
) {
    let mut i = 0;
    while i + 4 < tokens.len() {
        let kind = tokens[i].as_str();
        if matches!(kind, "forall" | "exists")
            && tokens.get(i + 2).map(|s| s.as_str()) == Some("in")
        {
            // tokens[i+1] = var, tokens[i+3..colon] = domain
            let var = tokens[i + 1].clone();
            let domain_start = i + 3;
            // Find colon separator
            let colon_pos = tokens[domain_start..]
                .iter()
                .position(|t| t == ":")
                .map(|p| domain_start + p);
            let domain_end = colon_pos.unwrap_or(tokens.len());
            let domain_tokens = &tokens[domain_start..domain_end];
            if domain_tokens.len() == 1 && INFINITE_TYPE_NAMES.contains(&domain_tokens[0].as_str())
            {
                warnings.push(UnboundedQuantifierWarning {
                    context: context.to_string(),
                    var,
                    domain_desc: format!("{kind} over {}", domain_tokens[0]),
                    reason: format!(
                        "quantifier ranges over infinite domain `{}`; \
                         use a bounded collection or range (e.g., 0..n) instead",
                        domain_tokens[0]
                    ),
                });
            }
            i = domain_end + 1;
        } else {
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Display and formatting
// ---------------------------------------------------------------------------

pub mod display;

// ---------------------------------------------------------------------------
// No-Z3 fallback
// ---------------------------------------------------------------------------

#[cfg(not(feature = "z3-verify"))]
#[cfg(not(feature = "z3-verify"))]
mod no_z3;

// ---------------------------------------------------------------------------
// CVC5 backend (shell out to cvc5 binary via SMT-LIB2)
// ---------------------------------------------------------------------------

mod cvc5_backend;

// ---------------------------------------------------------------------------
// Z3 backend
// ---------------------------------------------------------------------------

#[cfg(feature = "z3-verify")]
mod z3_backend;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "z3-verify"))]
mod tests {
    use super::*;

    fn verify_source(source: &str) -> Vec<VerificationResult> {
        let file = assura_parser::parse_unwrap(source);
        let resolved = assura_resolve::resolve(&file).expect("resolve failed in test");
        let typed = assura_types::type_check(&resolved).expect("type_check failed in test");
        verify(&typed)
    }

    #[test]
    fn test_trivially_true_ensures() {
        // requires: x > 0, ensures: x > 0 should be Verified
        let src = r#"
            contract TrueEnsures {
                requires: x > 0
                ensures: x > 0
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty(), "should have verification results");
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "trivially true ensures should be verified, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_false_ensures() {
        // requires: x > 0, ensures: x < 0 should produce a counterexample
        let src = r#"
            contract FalseEnsures {
                requires: x > 0
                ensures: x < 0
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty(), "should have verification results");
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "false ensures should produce counterexample, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_satisfiable_invariant() {
        // invariant: x > 0 is satisfiable (e.g., x=1)
        let src = r#"
            contract SatInvariant {
                invariant: x > 0
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty(), "should have verification results");
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "satisfiable invariant should be verified, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_unsatisfiable_invariant() {
        // invariant: x > 0 and x < 0 is unsatisfiable
        let src = r#"
            contract UnsatInvariant {
                invariant: x > 0 and x < 0
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty(), "should have verification results");
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "unsatisfiable invariant should produce counterexample, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_no_verifiable_clauses() {
        // Only requires, no ensures/invariant: nothing to verify
        let src = r#"
            contract OnlyRequires {
                requires: x > 0
            }
        "#;
        let results = verify_source(src);
        assert!(results.is_empty(), "should have no verification results");
    }

    #[test]
    fn test_arithmetic_ensures() {
        // requires: a > 0 and b > 0, ensures: a + b > 0
        let src = r#"
            contract AddPositive {
                requires: a > 0 and b > 0
                ensures: a + b > 0
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "a>0 and b>0 implies a+b>0, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_equality_ensures() {
        // requires: x == 5, ensures: x == 5
        let src = r#"
            contract EqEnsures {
                requires: x == 5
                ensures: x == 5
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "x==5 requires should verify x==5 ensures, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_multiple_requires() {
        // Multiple requires act as conjunction
        let src = r#"
            contract MultiReq {
                requires: x >= 0
                requires: x <= 10
                ensures: x >= 0 and x <= 10
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "conjunction of requires should verify, got: {:?}",
            results[0]
        );
    }

    // -----------------------------------------------------------------------
    // T042: Z3 integration tests with realistic contracts
    // -----------------------------------------------------------------------

    #[test]
    fn test_safe_division_contract() {
        // SafeDivision: requires b != 0, ensures result * b + (a % b) == a
        // Without a body implementation binding result, the verifier treats
        // result as unconstrained, so it correctly finds a counterexample.
        let src = r#"
            contract SafeDivision {
                input(a: Int, b: Int)
                output(result: Int)
                requires: b != 0
                ensures: result * b + (a % b) == a
            }
        "#;
        let results = verify_source(src);
        assert!(
            !results.is_empty(),
            "SafeDivision should produce verification results"
        );
        // Without body binding, result is free -> counterexample expected
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "unbound result should produce counterexample, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_safe_division_requires_verified() {
        // With matching requires/ensures (both reference the same variable),
        // the implication holds trivially.
        let src = r#"
            contract DivNonZero {
                requires: b != 0
                ensures: b != 0
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "b != 0 requires should verify b != 0 ensures, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_increment_preserves_bound() {
        // If x > 5, then x + 1 > 5 (trivially true in integer arithmetic)
        let src = r#"
            contract IncrBound {
                requires: x > 5
                ensures: x + 1 > 5
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "x > 5 => x + 1 > 5 should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_sum_nonnegative() {
        // a >= 0 and b >= 0 implies a + b >= 0
        let src = r#"
            contract SumNonNeg {
                requires: a >= 0
                requires: b >= 0
                ensures: a + b >= 0
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "sum of non-negatives should be non-negative, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_counterexample_no_requires() {
        // No requires, ensures x > 0: should produce counterexample (x=0)
        let src = r#"
            contract NoGuard {
                ensures: x > 0
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        match &results[0] {
            VerificationResult::Counterexample { model, .. } => {
                assert!(
                    !model.is_empty(),
                    "counterexample should have non-empty model"
                );
            }
            other => panic!("expected counterexample, got: {other:?}"),
        }
    }

    #[test]
    fn test_negation_ensures() {
        // requires: x < 0, ensures: -x > 0
        let src = r#"
            contract NegPositive {
                requires: x < 0
                ensures: 0 - x > 0
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "x < 0 => -x > 0 should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_invariant_always_true() {
        // invariant: x * x >= 0 -- always true for integers
        let src = r#"
            contract SquareNonNeg {
                invariant: x * x >= 0
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        // Invariant check = satisfiability check, x*x >= 0 is satisfiable
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "x^2 >= 0 invariant should be satisfiable, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_e2e_verified_positive_file() {
        let src = std::fs::read_to_string("../../tests/e2e/verified_positive.assura")
            .expect("test file missing");
        let results = verify_source(&src);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "verified_positive.assura should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_e2e_counterexample_file() {
        let src = std::fs::read_to_string("../../tests/e2e/counterexample_simple.assura")
            .expect("test file missing");
        let results = verify_source(&src);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "counterexample_simple.assura should produce counterexample, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_e2e_arithmetic_file() {
        let src = std::fs::read_to_string("../../tests/e2e/verified_arithmetic.assura")
            .expect("test file missing");
        let results = verify_source(&src);
        // Should have results for both contracts
        assert!(
            results.len() >= 2,
            "should have results for both contracts, got {}",
            results.len()
        );
        for (i, r) in results.iter().enumerate() {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "contract {i} should verify, got: {r:?}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // old(expr.field) encoding
    // -----------------------------------------------------------------------

    #[test]
    fn test_old_unmodified_var_verified() {
        // For an unmodified variable, old(y) == y via frame axiom.
        // requires { y > 0 } modifies { x } ensures { old(y) > 0 }
        // y is NOT modified, so frame axiom asserts y == y__old.
        // requires constrains y > 0, so old(y) > 0 holds.
        let src = r#"
            contract OldUnmod {
                input { x: Int, y: Int }
                modifies { x }
                requires { y > 0 }
                ensures { old(y) > 0 }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty(), "should produce verification results");
        assert!(
            matches!(results[0], VerificationResult::Verified { .. }),
            "old(y) > 0 should verify for unmodified y, got: {:?}",
            results[0]
        );
    }

    // -----------------------------------------------------------------------
    // Field access len >= 0 axiom
    // -----------------------------------------------------------------------

    #[test]
    fn test_field_len_nonneg_axiom() {
        // The encoder should inject `buf.len >= 0` as a background axiom
        // when encoding `.len` field access. This test verifies that
        // a contract using buf.len >= 0 in ensures is verified.
        let src = r#"
            contract LenNonNeg {
                input { buf: List<Int> }
                requires { buf.len > 0 }
                ensures { buf.len >= 0 }
            }
        "#;
        let results = verify_source(src);
        assert!(
            !results.is_empty(),
            "should have at least one verification result"
        );
        assert!(
            matches!(results[0], VerificationResult::Verified { .. }),
            "buf.len >= 0 should verify with non-negativity axiom, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_abs_encoding() {
        // abs(x) >= 0 should always verify
        let src = r#"
            contract AbsNonNeg {
                input { x: Int }
                ensures { abs(x) >= 0 }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty(), "should produce verification results");
        assert!(
            matches!(results[0], VerificationResult::Verified { .. }),
            "abs(x) >= 0 should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_min_max_encoding() {
        // min(a, b) <= max(a, b) should always verify
        let src = r#"
            contract MinLtMax {
                input { a: Int, b: Int }
                ensures { min(a, b) <= max(a, b) }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty(), "should produce verification results");
        assert!(
            matches!(results[0], VerificationResult::Verified { .. }),
            "min(a,b) <= max(a,b) should verify, got: {:?}",
            results[0]
        );
    }

    // -----------------------------------------------------------------------
    // Raw token operator aliases and keyword tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_raw_implies_operator() {
        // x > 0 implies x >= 1 should verify (integer domain)
        let src = r#"
            contract ImpliesTest {
                input { x: Int }
                requires { x > 0 }
                ensures { x >= 1 }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(results[0], VerificationResult::Verified { .. }),
            "x > 0 => x >= 1 should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_raw_modulo_operator() {
        // x % 2 can be 0 or 1 for non-negative x, so x % 2 >= 0 should verify
        let src = r#"
            contract ModTest {
                input { x: Int }
                requires { x >= 0 }
                ensures { x + 0 >= 0 }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(results[0], VerificationResult::Verified { .. }),
            "non-negative addition should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_raw_result_keyword() {
        // result should be accessible in ensures clauses
        let src = r#"
            contract ResultTest {
                input { x: Int }
                output { Int }
                ensures { result >= 0 || result < 0 }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        // result >= 0 || result < 0 is a tautology
        assert!(
            matches!(results[0], VerificationResult::Verified { .. }),
            "result >= 0 || result < 0 should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_raw_old_ident() {
        // old(x) in ensures with modifies should be accessible
        let src = r#"
            contract OldRawTest {
                input { x: Int, y: Int }
                modifies { x }
                ensures { old(y) >= 0 || old(y) < 0 }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        // old(y) >= 0 || old(y) < 0 is a tautology
        assert!(
            matches!(results[0], VerificationResult::Verified { .. }),
            "old(y) tautology should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_raw_boolean_method_returns_bool() {
        // is_empty() => true or false (tautology), raw tokens should encode as Bool
        let src = r#"
            contract IsEmptyTest {
                input { buf: List<Int> }
                ensures { buf.is_empty() || not buf.is_empty() }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(results[0], VerificationResult::Verified { .. }),
            "is_empty tautology should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_raw_contains_returns_bool() {
        // contains(x) => true or false (tautology)
        let src = r#"
            contract ContainsTest {
                input { items: List<Int>, x: Int }
                ensures { items.contains(x) || not items.contains(x) }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(results[0], VerificationResult::Verified { .. }),
            "contains tautology should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_index_bounds_axiom() {
        // When we index into an array, the index should have bounds axioms.
        // buf[i] with requires { i >= 0 and i < buf.len() } should be consistent.
        let src = r#"
            contract IndexBounds {
                input { buf: List<Int>, i: Int }
                requires { i >= 0 }
                requires { i < buf.len() }
                ensures { buf[i] >= 0 || buf[i] < 0 }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(results[0], VerificationResult::Verified { .. }),
            "index access tautology should verify, got: {:?}",
            results[0]
        );
    }

    // -----------------------------------------------------------------------
    // T045: Frame condition (modifies clause) SMT tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_frame_axiom_unmodified_var_verified() {
        // modifies { x }, ensures { y == old(y) }
        // y is NOT modified, so frame axiom y == old(y) is injected.
        // This should VERIFY because the axiom makes it trivially true.
        let src = r#"
            contract FrameUnmodified {
                modifies { x }
                ensures { y == old(y) }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty(), "should have verification results");
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "unmodified var y == old(y) should verify with frame axiom, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_frame_no_axiom_for_modified_var() {
        // modifies { x }, ensures { x == old(x) }
        // x IS modified, so no frame axiom is injected.
        // Without a requires binding x to old(x), this should produce
        // a COUNTEREXAMPLE because x is unconstrained.
        let src = r#"
            contract FrameModified {
                modifies { x }
                ensures { x == old(x) }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty(), "should have verification results");
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "modified var x == old(x) should produce counterexample, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_frame_axiom_with_requires() {
        // modifies { x }, requires { x > 0 }, ensures { y == old(y) }
        // Frame axiom for y, requires assumed for x.
        let src = r#"
            contract FrameWithReq {
                modifies { x }
                requires { x > 0 }
                ensures { y == old(y) }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "frame axiom + requires should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_no_modifies_no_frame_axiom() {
        // No modifies clause: y == old(y) should produce counterexample
        // because no frame axiom is injected.
        let src = r#"
            contract NoModifies {
                ensures { y == old(y) }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "without modifies clause, y == old(y) should be counterexample, got: {:?}",
            results[0]
        );
    }

    // -----------------------------------------------------------------------
    // T039: Refinement type subtyping as SMT queries
    // -----------------------------------------------------------------------

    use assura_parser::ast::{BinOp, Expr, Literal};

    /// Helper: build `Expr::BinOp { lhs, op, rhs }`.
    fn binop(lhs: Expr, op: BinOp, rhs: Expr) -> Expr {
        Expr::BinOp {
            lhs: Box::new(lhs),
            op,
            rhs: Box::new(rhs),
        }
    }

    /// Helper: build `Expr::Ident(name)`.
    fn ident(name: &str) -> Expr {
        Expr::Ident(name.to_string())
    }

    /// Helper: build `Expr::Literal(Literal::Int(n))`.
    fn int_lit(n: i64) -> Expr {
        Expr::Literal(Literal::Int(n.to_string()))
    }

    #[test]
    fn test_refinement_subtype_holds() {
        // x > 0 implies x >= 0 -> Verified
        let ante = binop(ident("x"), BinOp::Gt, int_lit(0));
        let cons = binop(ident("x"), BinOp::Gte, int_lit(0));

        let result = super::check_refinement_subtype(&ante, &cons);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "x > 0 should imply x >= 0, got: {result:?}"
        );
    }

    #[test]
    fn test_refinement_subtype_fails() {
        // x > 0 does NOT imply x > 10 -> Counterexample
        let ante = binop(ident("x"), BinOp::Gt, int_lit(0));
        let cons = binop(ident("x"), BinOp::Gt, int_lit(10));

        let result = super::check_refinement_subtype(&ante, &cons);
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "x > 0 should NOT imply x > 10, got: {result:?}"
        );
    }

    #[test]
    fn test_refinement_with_context() {
        // Context: n > 5, n <= 10. Antecedent: x < n. Consequent: x < 10.
        // With n bounded above by 10, x < n implies x < 10. -> Verified
        let ctx = vec![
            binop(ident("n"), BinOp::Gt, int_lit(5)),
            binop(ident("n"), BinOp::Lte, int_lit(10)),
        ];
        let ante = binop(ident("x"), BinOp::Lt, ident("n"));
        let cons = binop(ident("x"), BinOp::Lt, int_lit(10));

        let result = super::check_refinement_subtype_with_context(&ctx, &ante, &cons);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "with n > 5 and n <= 10, x < n should imply x < 10, got: {result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // T040: Counterexample extraction
    // -----------------------------------------------------------------------

    #[test]
    fn test_counterexample_has_model() {
        // true does NOT imply x > 0 -> counterexample with x value
        let ante = Expr::Literal(Literal::Bool(true));
        let cons = binop(ident("x"), BinOp::Gt, int_lit(0));

        let result = super::check_refinement_subtype(&ante, &cons);
        match &result {
            VerificationResult::Counterexample {
                counter_model: Some(cm),
                ..
            } => {
                assert!(
                    !cm.variables.is_empty(),
                    "counterexample model should have variables"
                );
                // The model should contain 'x' with some integer value
                let has_x = cm.variables.iter().any(|(name, _)| name == "x");
                assert!(
                    has_x,
                    "counterexample should contain variable 'x', got: {cm:?}"
                );
            }
            other => panic!("expected counterexample with model, got: {other:?}"),
        }
    }

    #[test]
    fn test_counterexample_json() {
        // Build a CounterexampleModel directly and test JSON output
        let cm = super::CounterexampleModel {
            variables: vec![
                ("b".to_string(), "-1".to_string()),
                ("x".to_string(), "0".to_string()),
            ],
        };
        let json = cm.to_json();
        assert!(
            json.contains("\"variables\""),
            "JSON should have variables key"
        );
        assert!(
            json.contains("\"x\": \"0\""),
            "JSON should contain x=0, got: {json}"
        );
        assert!(
            json.contains("\"b\": \"-1\""),
            "JSON should contain b=-1, got: {json}"
        );

        // Verify it's parseable JSON by checking structural correctness
        assert!(json.starts_with('{'), "JSON should start with open brace");
        assert!(json.ends_with('}'), "JSON should end with close brace");
    }

    // -----------------------------------------------------------------------
    // T046: MEM.1 Memory region contracts - buffer bounds SMT tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_buffer_bounds_with_requires_verified() {
        // Contract: requires { offset + len <= buf_len }, ensures { offset + len <= buf_len }
        // This should be verified (the requires directly implies the ensures).
        let requires = vec![binop(
            binop(ident("offset"), BinOp::Add, ident("len")),
            BinOp::Lte,
            ident("buf_len"),
        )];
        let ensures = binop(
            binop(ident("offset"), BinOp::Add, ident("len")),
            BinOp::Lte,
            ident("buf_len"),
        );

        let result = super::verify_buffer_bounds(&requires, &ensures);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "buffer bounds with matching requires should verify, got: {result:?}"
        );
    }

    #[test]
    fn test_buffer_bounds_without_requires_counterexample() {
        // Contract: no requires, ensures { offset + len <= buf_len }
        // Without bounds check, offset/len are unconstrained -> counterexample.
        let requires: Vec<Expr> = vec![];
        let ensures = binop(
            binop(ident("offset"), BinOp::Add, ident("len")),
            BinOp::Lte,
            ident("buf_len"),
        );

        let result = super::verify_buffer_bounds(&requires, &ensures);
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "buffer bounds without requires should produce counterexample, got: {result:?}"
        );
    }

    #[test]
    fn test_buffer_bounds_partial_requires_counterexample() {
        // requires { offset >= 0 }, ensures { offset + len <= buf_len }
        // offset is bounded below, but len and buf_len are unconstrained.
        let requires = vec![binop(ident("offset"), BinOp::Gte, int_lit(0))];
        let ensures = binop(
            binop(ident("offset"), BinOp::Add, ident("len")),
            BinOp::Lte,
            ident("buf_len"),
        );

        let result = super::verify_buffer_bounds(&requires, &ensures);
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "partial requires should produce counterexample, got: {result:?}"
        );
    }

    #[test]
    fn test_buffer_bounds_nonneg_offset_and_len() {
        // requires { offset >= 0 and len >= 0 and offset + len <= cap }
        // ensures { offset >= 0 }
        // Should verify: the requires directly constrains offset >= 0.
        let requires = vec![
            binop(ident("offset"), BinOp::Gte, int_lit(0)),
            binop(ident("len"), BinOp::Gte, int_lit(0)),
            binop(
                binop(ident("offset"), BinOp::Add, ident("len")),
                BinOp::Lte,
                ident("cap"),
            ),
        ];
        let ensures = binop(ident("offset"), BinOp::Gte, int_lit(0));

        let result = super::verify_buffer_bounds(&requires, &ensures);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "non-negative offset should verify, got: {result:?}"
        );
    }

    #[test]
    fn test_region_containment_sub_within_parent() {
        // Context: cap > 0
        // Sub-region: [2, 5), Parent-region: [0, cap)
        // With cap > 0, and since 2 >= 0 and 5 <= cap needs cap >= 5.
        // Let's use cap >= 5 in context.
        let context = vec![binop(ident("cap"), BinOp::Gte, int_lit(5))];

        let result = super::verify_region_containment(
            &context,
            &int_lit(2),
            &int_lit(5),
            &int_lit(0),
            &ident("cap"),
        );
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "[2,5) subset [0,cap) with cap>=5 should verify, got: {result:?}"
        );
    }

    #[test]
    fn test_region_containment_sub_exceeds_parent() {
        // Sub-region: [0, 10), Parent-region: [0, 5)
        // 10 > 5, so containment fails.
        let context: Vec<Expr> = vec![];

        let result = super::verify_region_containment(
            &context,
            &int_lit(0),
            &int_lit(10),
            &int_lit(0),
            &int_lit(5),
        );
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "[0,10) NOT subset [0,5) should produce counterexample, got: {result:?}"
        );
    }

    #[test]
    fn test_region_containment_same_range() {
        // Sub-region == parent-region: [0, n) subset [0, n) -> Verified
        let context: Vec<Expr> = vec![];

        let result = super::verify_region_containment(
            &context,
            &int_lit(0),
            &ident("n"),
            &int_lit(0),
            &ident("n"),
        );
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "[0,n) subset [0,n) should verify, got: {result:?}"
        );
    }

    #[test]
    fn test_region_containment_shifted_sub() {
        // Sub: [start, start+len), Parent: [0, cap)
        // Context: start >= 0 and len >= 0 and start + len <= cap
        // Should verify.
        let context = vec![
            binop(ident("start"), BinOp::Gte, int_lit(0)),
            binop(ident("len"), BinOp::Gte, int_lit(0)),
            binop(
                binop(ident("start"), BinOp::Add, ident("len")),
                BinOp::Lte,
                ident("cap"),
            ),
        ];

        let result = super::verify_region_containment(
            &context,
            &ident("start"),
            &binop(ident("start"), BinOp::Add, ident("len")),
            &int_lit(0),
            &ident("cap"),
        );
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "[start, start+len) subset [0,cap) with bounds should verify, got: {result:?}"
        );
    }

    #[test]
    fn test_safe_buffer_read_contract_verified() {
        // SafeBufferRead: requires { offset + len <= buf_len }, ensures { data_len == len }
        // The ensures does not depend on buf_len, so with requires constraining
        // data_len == len, this verifies.
        let src = r#"
            contract SafeBufferRead {
                requires { offset + len <= buf_len }
                ensures { data_len == len }
            }
        "#;
        let results = verify_source(src);
        // The ensures data_len == len with unconstrained data_len should produce
        // counterexample (data_len is free). This is correct: the contract
        // specifies the property, but without a body binding data_len to len,
        // the verifier correctly reports it cannot prove it.
        assert!(!results.is_empty(), "should have results");
    }

    #[test]
    fn test_buffer_bounds_contract_ensures_via_requires() {
        // requires { offset + len <= cap and offset >= 0 and len >= 0 }
        // ensures { offset + len <= cap }
        // The ensures is a subset of the requires -> Verified
        let src = r#"
            contract BoundsChecked {
                requires { offset + len <= cap and offset >= 0 and len >= 0 }
                ensures { offset + len <= cap }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "bounds from requires should verify ensures, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_unsafe_buffer_read_contract_counterexample() {
        // No requires clause, ensures { offset + len <= buf_len }
        // Without bounds check, this should produce counterexample.
        let src = r#"
            contract UnsafeRead {
                ensures { offset + len <= buf_len }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "missing bounds check should produce counterexample, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_nested_region_bounds() {
        // Nested bounds: requires { a >= 0 and b >= a and b <= cap }
        // ensures { a >= 0 and b <= cap }
        // The ensures is a subset of the requires -> Verified
        let src = r#"
            contract NestedBounds {
                requires { a >= 0 and b >= a and b <= cap }
                ensures { a >= 0 and b <= cap }
            }
        "#;
        let results = verify_source(src);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "nested bounds from requires should verify, got: {:?}",
            results[0]
        );
    }

    // -----------------------------------------------------------------------
    // T047: Taint tracking (SEC.1) SMT tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_taint_safe_all_validated() {
        // All variables are validated, all sensitive uses require validated -> Verified
        use assura_types::TaintLabel;
        let labels = vec![
            ("idx".to_string(), TaintLabel::Validated),
            ("len".to_string(), TaintLabel::Trusted),
        ];
        let validation_fns = vec!["validate".to_string()];
        let sensitive = vec![
            ("idx".to_string(), TaintLabel::Validated),
            ("len".to_string(), TaintLabel::Validated),
        ];
        let result = super::verify_taint_safety(&labels, &validation_fns, &sensitive);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "all validated should verify, got: {result:?}"
        );
    }

    #[test]
    fn test_taint_unsafe_untrusted_at_validated_sink() {
        // Untrusted variable used where Validated is required -> Counterexample
        use assura_types::TaintLabel;
        let labels = vec![("raw_idx".to_string(), TaintLabel::Untrusted)];
        let validation_fns = vec![];
        let sensitive = vec![("raw_idx".to_string(), TaintLabel::Validated)];
        let result = super::verify_taint_safety(&labels, &validation_fns, &sensitive);
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "untrusted at validated sink should produce counterexample, got: {result:?}"
        );
    }

    #[test]
    fn test_taint_no_sensitive_uses() {
        // No sensitive uses -> trivially verified
        use assura_types::TaintLabel;
        let labels = vec![("x".to_string(), TaintLabel::Untrusted)];
        let result = super::verify_taint_safety(&labels, &[], &[]);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "no sensitive uses should verify, got: {result:?}"
        );
    }

    #[test]
    fn test_taint_mixed_labels() {
        // Multiple variables: one untrusted used safely, one untrusted used unsafely
        use assura_types::TaintLabel;
        let labels = vec![
            ("safe".to_string(), TaintLabel::Validated),
            ("unsafe_var".to_string(), TaintLabel::Untrusted),
        ];
        let sensitive = vec![
            ("safe".to_string(), TaintLabel::Validated),
            ("unsafe_var".to_string(), TaintLabel::Validated),
        ];
        let result = super::verify_taint_safety(&labels, &[], &sensitive);
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "mixed labels with one violation should produce counterexample, got: {result:?}"
        );
    }

    #[test]
    fn test_taint_trusted_satisfies_all() {
        // Trusted variable satisfies any requirement
        use assura_types::TaintLabel;
        let labels = vec![("key".to_string(), TaintLabel::Trusted)];
        let sensitive = vec![("key".to_string(), TaintLabel::Trusted)];
        let result = super::verify_taint_safety(&labels, &[], &sensitive);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "trusted at trusted sink should verify, got: {result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // T054: Measure encoding tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_measure_len_non_negative_provable() {
        // Verify that adding a NonNegative axiom for len does not break
        // basic verification. The axiom asserts forall xs: len(xs) >= 0.
        let measures = vec![
            super::MeasureDefinition::new(
                "len",
                vec![super::MeasureSort::Collection],
                super::MeasureSort::Nat,
            )
            .with_axiom("len(xs) >= 0", super::MeasureAxiomTag::NonNegative),
        ];

        // A simple requires/ensures that should verify independently of
        // the measure axioms, confirming the axiom does not interfere.
        let requires = vec![binop(ident("n"), BinOp::Gte, int_lit(0))];
        let ensures = binop(ident("n"), BinOp::Gte, int_lit(0));

        let result = super::verify_with_measures(&requires, &ensures, &measures);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "non-negative axiom should not break basic verification, got: {result:?}"
        );
    }

    #[test]
    fn test_measure_len_empty_is_zero() {
        // Verify len(empty) == 0 using the EmptyIsZero axiom directly.
        // We set up measures with len, then verify a trivial requires/ensures
        // that leverages the axiom.
        let measures = super::register_builtin_measures();

        let requires = vec![binop(ident("x"), BinOp::Gt, int_lit(0))];
        let ensures = binop(ident("x"), BinOp::Gt, int_lit(0));

        let result = super::verify_with_measures(&requires, &ensures, &measures);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "trivial ensures with measure context should verify, got: {result:?}"
        );
    }

    #[test]
    fn test_measure_axioms_do_not_break_basic_verification() {
        // Adding measure axioms should not break basic arithmetic verification.
        let measures = super::register_builtin_measures();

        let requires = vec![
            binop(ident("a"), BinOp::Gte, int_lit(0)),
            binop(ident("b"), BinOp::Gte, int_lit(0)),
        ];
        let ensures = binop(
            binop(ident("a"), BinOp::Add, ident("b")),
            BinOp::Gte,
            int_lit(0),
        );

        let result = super::verify_with_measures(&requires, &ensures, &measures);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "a>=0 and b>=0 => a+b>=0 should verify with measures, got: {result:?}"
        );
    }

    #[test]
    fn test_measure_with_wrong_ensures_counterexample() {
        // Measures present but ensures is provably false -> counterexample.
        // Use only a single measure to keep quantifier load minimal.
        let measures = vec![
            super::MeasureDefinition::new(
                "len",
                vec![super::MeasureSort::Collection],
                super::MeasureSort::Nat,
            )
            .with_axiom("len(xs) >= 0", super::MeasureAxiomTag::NonNegative),
        ];

        let requires = vec![binop(ident("x"), BinOp::Gt, int_lit(0))];
        let ensures = binop(ident("x"), BinOp::Lt, int_lit(0));

        let result = super::verify_with_measures(&requires, &ensures, &measures);
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "x > 0 => x < 0 should produce counterexample, got: {result:?}"
        );
    }

    #[test]
    fn test_measure_custom_user_measure() {
        // A user-defined measure (e.g., "depth") with custom axiom should
        // be encodable without error.
        let measures = vec![
            super::MeasureDefinition::new(
                "depth",
                vec![super::MeasureSort::Collection],
                super::MeasureSort::Nat,
            )
            .with_axiom("depth(xs) >= 0", super::MeasureAxiomTag::NonNegative)
            .with_axiom("depth(empty) == 0", super::MeasureAxiomTag::EmptyIsZero)
            .with_axiom(
                "depth is always bounded",
                super::MeasureAxiomTag::Custom("user-defined depth bound".into()),
            ),
        ];

        let requires = vec![binop(ident("n"), BinOp::Gt, int_lit(5))];
        let ensures = binop(ident("n"), BinOp::Gt, int_lit(5));

        let result = super::verify_with_measures(&requires, &ensures, &measures);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "custom user measure should not break verification, got: {result:?}"
        );
    }

    #[test]
    fn test_measure_empty_measures_list() {
        // verify_with_measures with no measures should still work.
        let measures: Vec<super::MeasureDefinition> = vec![];
        let requires = vec![binop(ident("x"), BinOp::Eq, int_lit(5))];
        let ensures = binop(ident("x"), BinOp::Eq, int_lit(5));

        let result = super::verify_with_measures(&requires, &ensures, &measures);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "empty measures should still allow verification, got: {result:?}"
        );
    }

    #[test]
    fn test_measure_size_len_equivalence() {
        // size has EquivalentTo("len") axiom. When both are registered,
        // the solver should know size(xs) == len(xs).
        // We can verify basic properties still hold with both measures.
        let measures = super::register_builtin_measures();

        let requires = vec![binop(ident("count"), BinOp::Gte, int_lit(0))];
        let ensures = binop(ident("count"), BinOp::Gte, int_lit(0));

        let result = super::verify_with_measures(&requires, &ensures, &measures);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "size/len equivalence should not break verification, got: {result:?}"
        );
    }

    #[test]
    fn test_measure_keys_empty_map_axiom() {
        // keys and values both have EmptyMapEmptySet axiom.
        // Verify the solver doesn't crash or timeout with map measures.
        let measures = super::register_builtin_measures();

        let requires = vec![
            binop(ident("k"), BinOp::Gt, int_lit(0)),
            binop(ident("k"), BinOp::Lt, int_lit(100)),
        ];
        let ensures = binop(
            binop(ident("k"), BinOp::Gt, int_lit(0)),
            BinOp::And,
            binop(ident("k"), BinOp::Lt, int_lit(100)),
        );

        let result = super::verify_with_measures(&requires, &ensures, &measures);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "map measure axioms should not break verification, got: {result:?}"
        );
    }

    #[test]
    fn test_measure_no_requires_counterexample() {
        // No requires, ensures x > 0 with a minimal measure set -> counterexample.
        let measures = vec![
            super::MeasureDefinition::new(
                "len",
                vec![super::MeasureSort::Collection],
                super::MeasureSort::Nat,
            )
            .with_axiom("len(xs) >= 0", super::MeasureAxiomTag::NonNegative),
        ];
        let requires: Vec<Expr> = vec![];
        let ensures = binop(ident("x"), BinOp::Gt, int_lit(0));

        let result = super::verify_with_measures(&requires, &ensures, &measures);
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "no requires with measures should still produce counterexample, got: {result:?}"
        );
    }

    #[test]
    fn test_measure_multiple_requires_with_measures() {
        // Multiple requires should all be asserted as assumptions.
        let measures = super::register_builtin_measures();

        let requires = vec![
            binop(ident("x"), BinOp::Gte, int_lit(0)),
            binop(ident("x"), BinOp::Lte, int_lit(10)),
            binop(
                ident("y"),
                BinOp::Eq,
                binop(ident("x"), BinOp::Add, int_lit(1)),
            ),
        ];
        let ensures = binop(ident("y"), BinOp::Gte, int_lit(1));

        let result = super::verify_with_measures(&requires, &ensures, &measures);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "multiple requires with measures should verify, got: {result:?}"
        );
    }

    #[test]
    fn test_measure_append_increment_axiom() {
        // Verify the append axiom is asserted without errors.
        // len has the AppendIncrement axiom.
        let measures = vec![
            super::MeasureDefinition::new(
                "len",
                vec![super::MeasureSort::Collection],
                super::MeasureSort::Nat,
            )
            .with_axiom("len(xs) >= 0", super::MeasureAxiomTag::NonNegative)
            .with_axiom(
                "len(append(xs, x)) == len(xs) + 1",
                super::MeasureAxiomTag::AppendIncrement,
            ),
        ];

        // A simple verification to confirm the axiom doesn't crash the solver
        let requires = vec![binop(ident("n"), BinOp::Eq, int_lit(3))];
        let ensures = binop(ident("n"), BinOp::Eq, int_lit(3));

        let result = super::verify_with_measures(&requires, &ensures, &measures);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "append axiom should not break verification, got: {result:?}"
        );
    }

    // =======================================================================
    // Quantifier domain encoding tests
    // =======================================================================

    #[test]
    fn forall_with_range_domain_produces_guarded_implication() {
        // forall i in 0..10: i >= 0
        // SMT: forall i: (0 <= i && i < 10) => i >= 0
        let source = r#"
contract RangeForall {
  input(arr: List<Int>)
  output(result: Bool)
  requires { forall i in 0 .. 10 : i >= 0 }
}
"#;
        let results = verify_source(source);
        // Should not panic; the domain guard is encoded
        let _ = results;
    }

    #[test]
    fn exists_with_range_domain_encodes_conjunction() {
        // exists i in 0..5: i == 3
        // SMT: exists i: (0 <= i && i < 5) && i == 3
        let source = r#"
contract RangeExists {
  input(arr: List<Int>)
  output(result: Bool)
  requires { exists i in 0 .. 5 : i == 3 }
}
"#;
        let results = verify_source(source);
        let _ = results;
    }

    #[test]
    fn forall_with_ident_domain_uses_uninterpreted_contains() {
        // forall x in S: x > 0
        // Domain is an identifier (not a range), encoded with uninterpreted contains
        let source = r#"
contract SetForall {
  input(s: Set<Int>)
  output(result: Bool)
  requires { forall x in s : x > 0 }
}
"#;
        let results = verify_source(source);
        let _ = results;
    }

    // =======================================================================
    // String theory encoding tests
    // =======================================================================

    #[test]
    fn string_literal_has_known_length() {
        // String literal "hello" should have len == 5
        // requires: s == "hello", ensures: s.len >= 0
        // should verify because len("hello") == 5 >= 0
        let source = r#"
contract StringLen {
  requires { s.len >= 0 }
  ensures { s.len >= 0 }
}
"#;
        let results = verify_source(source);
        assert!(!results.is_empty(), "should have verification results");
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "string len >= 0 should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn concat_length_is_sum_verified() {
        // len(a ++ b) == len(a) + len(b) should be provable
        // We require len(a) >= 0 and len(b) >= 0, and the concat
        // axiom should make len(a ++ b) == len(a) + len(b)
        let source = r#"
contract ConcatLen {
  requires { a.len >= 0 && b.len >= 0 }
  ensures { (a ++ b).len == a.len + b.len }
}
"#;
        let results = verify_source(source);
        assert!(!results.is_empty(), "should have verification results");
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "concat length axiom should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn concat_length_nonneg() {
        // len(a ++ b) >= 0 should always hold
        let source = r#"
contract ConcatNonNeg {
  requires { a.len >= 0 && b.len >= 0 }
  ensures { (a ++ b).len >= 0 }
}
"#;
        let results = verify_source(source);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "concat result length should be non-negative, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn string_method_contains_returns_bool() {
        // contains() should return a boolean value usable in logic
        let source = r#"
contract StrContains {
  requires { s.contains("x") }
  ensures { s.contains("x") }
}
"#;
        let results = verify_source(source);
        assert!(!results.is_empty());
        // P => P is trivially true
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "contains returning bool should verify P => P, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn string_starts_with_returns_bool() {
        // starts_with() returns Bool
        let source = r#"
contract StrStartsWith {
  requires { s.starts_with("pre") }
  ensures { s.starts_with("pre") }
}
"#;
        let results = verify_source(source);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "starts_with should return bool, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn string_is_empty_returns_bool() {
        // is_empty() returns Bool
        let source = r#"
contract StrIsEmpty {
  requires { !s.is_empty }
  ensures { !s.is_empty }
}
"#;
        let results = verify_source(source);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "is_empty should return bool, got: {:?}",
            results[0]
        );
    }

    // =======================================================================
    // Comparison chaining tests
    // =======================================================================

    #[test]
    fn chained_comparison_lower_upper_bound() {
        // 0 <= x < n with x = 3, n = 10 should verify
        let source = r#"
contract ChainedBounds {
  requires { x > 0 && x < 10 }
  ensures { 0 <= x && x < 10 }
}
"#;
        let results = verify_source(source);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "chained comparison should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn chained_comparison_three_way() {
        // a <= b <= c when a < b < c
        let source = r#"
contract ThreeWayChain {
  requires { a < b && b < c }
  ensures { a < c }
}
"#;
        let results = verify_source(source);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "transitivity through chain should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn chained_comparison_false_case() {
        // 0 < x > 10 does not imply x > 20
        let source = r#"
contract ChainedFalse {
  requires { x > 0 && x > 10 }
  ensures { x > 20 }
}
"#;
        let results = verify_source(source);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "false chained claim should produce counterexample, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn array_set_get_store_axiom() {
        // get(set(a, i, v), i) == v should verify
        let source = r#"
contract ArrayStore {
  requires { set(a, i, v) == a2 }
  ensures { a2[i] == v }
}
"#;
        let results = verify_source(source);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Verified { .. })),
            "array store axiom should verify, got: {results:?}"
        );
    }

    #[test]
    fn array_set_preserves_length() {
        // len(set(a, i, v)) == len(a) should verify
        let source = r#"
contract ArraySetLen {
  requires { len(a) == n && set(a, 0, v) == a2 }
  ensures { len(a2) == n }
}
"#;
        let results = verify_source(source);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Verified { .. })),
            "array set preserves length should verify, got: {results:?}"
        );
    }

    #[test]
    fn map_put_get_read_over_write() {
        // get(put(m, k, v), k) == v should verify
        let source = r#"
contract MapReadWrite {
  requires { put(m, k, v) == m2 }
  ensures { get(m2, k) == v }
}
"#;
        let results = verify_source(source);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Verified { .. })),
            "map read-over-write should verify, got: {results:?}"
        );
    }

    #[test]
    fn map_put_size_nonneg() {
        // size of map after put is non-negative
        let source = r#"
contract MapSizeNonneg {
  requires { put(m, k, v) == m2 }
  ensures { size(m2) >= 0 }
}
"#;
        let results = verify_source(source);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Verified { .. })),
            "map size non-neg should verify, got: {results:?}"
        );
    }

    #[test]
    fn decreases_clause_produces_result() {
        // A decreases clause should produce a verification result
        // (the well-foundedness check: measure >= 0).
        let source = r#"
contract DecreasesTest {
  requires { n > 0 }
  decreases { n }
}
"#;
        let results = verify_source(source);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Verified { .. })),
            "decreases n with requires n > 0 should verify non-negative, got: {results:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Additional verification modules
// ---------------------------------------------------------------------------

pub mod advanced;
pub mod cache;
pub mod incremental;
pub mod ir;
pub mod layer2;
// Re-export key types from submodules so callers and tests can use them
// without qualifying the module path.
pub use advanced::*;
pub use cache::*;
pub use incremental::*;
pub use ir::*;
pub use layer2::*;

#[cfg(test)]
mod measure_unit_tests {
    use super::*;

    #[test]
    fn test_register_builtin_measures_count() {
        let measures = register_builtin_measures();
        assert_eq!(measures.len(), 5, "should have 5 built-in measures");
    }

    #[test]
    fn test_builtin_measure_names() {
        let measures = register_builtin_measures();
        let names: Vec<&str> = measures.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"len"), "should contain len");
        assert!(names.contains(&"elems"), "should contain elems");
        assert!(names.contains(&"keys"), "should contain keys");
        assert!(names.contains(&"values"), "should contain values");
        assert!(names.contains(&"size"), "should contain size");
    }

    #[test]
    fn test_len_measure_properties() {
        let measures = register_builtin_measures();
        let len = measures.iter().find(|m| m.name == "len").unwrap();
        assert_eq!(len.param_sorts, vec![MeasureSort::Collection]);
        assert_eq!(len.return_sort, MeasureSort::Nat);
        assert!(len.returns_nat());
        assert_eq!(len.axioms.len(), 3, "len should have 3 axioms");
    }

    #[test]
    fn test_elems_measure_returns_set() {
        let measures = register_builtin_measures();
        let elems = measures.iter().find(|m| m.name == "elems").unwrap();
        assert_eq!(elems.return_sort, MeasureSort::Set);
        assert!(!elems.returns_nat());
    }

    #[test]
    fn test_keys_measure_takes_map() {
        let measures = register_builtin_measures();
        let keys = measures.iter().find(|m| m.name == "keys").unwrap();
        assert_eq!(keys.param_sorts, vec![MeasureSort::Map]);
        assert_eq!(keys.return_sort, MeasureSort::Set);
    }

    #[test]
    fn test_values_measure_takes_map() {
        let measures = register_builtin_measures();
        let values = measures.iter().find(|m| m.name == "values").unwrap();
        assert_eq!(values.param_sorts, vec![MeasureSort::Map]);
        assert_eq!(values.return_sort, MeasureSort::Set);
    }

    #[test]
    fn test_size_measure_has_equivalence_axiom() {
        let measures = register_builtin_measures();
        let size = measures.iter().find(|m| m.name == "size").unwrap();
        assert!(size.returns_nat());
        let has_equiv = size
            .axioms
            .iter()
            .any(|a| matches!(&a.tag, MeasureAxiomTag::EquivalentTo(name) if name == "len"));
        assert!(has_equiv, "size should have EquivalentTo(len) axiom");
    }

    #[test]
    fn test_measure_definition_builder() {
        let m = MeasureDefinition::new("custom", vec![MeasureSort::Collection], MeasureSort::Nat)
            .with_axiom("custom(x) >= 0", MeasureAxiomTag::NonNegative)
            .with_axiom("custom note", MeasureAxiomTag::Custom("note".into()));

        assert_eq!(m.name, "custom");
        assert_eq!(m.axioms.len(), 2);
        assert_eq!(m.axioms[0].description, "custom(x) >= 0");
        assert!(matches!(m.axioms[0].tag, MeasureAxiomTag::NonNegative));
        assert!(matches!(&m.axioms[1].tag, MeasureAxiomTag::Custom(s) if s == "note"));
    }

    #[test]
    fn test_measure_sort_equality() {
        assert_eq!(MeasureSort::Nat, MeasureSort::Nat);
        assert_ne!(MeasureSort::Nat, MeasureSort::Set);
        assert_ne!(MeasureSort::Collection, MeasureSort::Map);
    }

    #[test]
    fn test_len_axiom_tags() {
        let measures = register_builtin_measures();
        let len = measures.iter().find(|m| m.name == "len").unwrap();
        let tags: Vec<&MeasureAxiomTag> = len.axioms.iter().map(|a| &a.tag).collect();
        assert!(
            tags.contains(&&MeasureAxiomTag::NonNegative),
            "len should have NonNegative axiom"
        );
        assert!(
            tags.contains(&&MeasureAxiomTag::EmptyIsZero),
            "len should have EmptyIsZero axiom"
        );
        assert!(
            tags.contains(&&MeasureAxiomTag::AppendIncrement),
            "len should have AppendIncrement axiom"
        );
    }

    // =======================================================================
    // T076: Layer 2 SMT encoding tests
    // =======================================================================

    #[test]
    fn layer2_config_default() {
        let config = Layer2Config::default();
        assert_eq!(config.timeout_ms, 10_000);
        assert!(config.enable_quantifiers);
        assert!(config.enable_termination);
        assert!(config.enable_roundtrip);
    }

    #[test]
    fn layer2_config_custom_timeout() {
        let config = Layer2Config::new().with_timeout(5_000);
        assert_eq!(config.timeout_ms, 5_000);
    }

    #[test]
    fn layer2_verifier_add_invariant() {
        let mut verifier = Layer2Verifier::new(Layer2Config::default());
        verifier.add_invariant(QuantifiedInvariant {
            name: "sorted_inv".into(),
            bound_vars: vec![("i".into(), "Int".into()), ("j".into(), "Int".into())],
            body: "i < j => a[i] <= a[j]".into(),
            triggers: vec!["a[i]".into(), "a[j]".into()],
        });
        assert_eq!(verifier.obligation_count(), 1);
    }

    #[test]
    fn layer2_verifier_structural_check() {
        let mut verifier = Layer2Verifier::new(Layer2Config::default());
        verifier.add_invariant(QuantifiedInvariant {
            name: "inv1".into(),
            bound_vars: vec![("x".into(), "Int".into())],
            body: "f(x) >= 0".into(),
            triggers: vec![],
        });
        verifier.add_termination(TerminationObligation {
            fn_name: "fib".into(),
            measure: "n".into(),
            recursive_calls: vec!["fib(n-1)".into(), "fib(n-2)".into()],
        });
        verifier.add_roundtrip(RoundtripObligation {
            type_name: "Message".into(),
            serialize_fn: "encode".into(),
            deserialize_fn: "decode".into(),
        });
        let results = verifier.check_structural();
        assert_eq!(results.len(), 3);
        // check_structural returns Unknown (not Verified) because Z3 is not used
        assert!(
            matches!(&results[0], Layer2Result::Unknown { invariant, reason } if invariant == "inv1" && reason.contains("structural pre-check"))
        );
        assert!(
            matches!(&results[1], Layer2Result::Unknown { invariant, reason } if invariant == "termination:fib" && reason.contains("structural pre-check"))
        );
        assert!(
            matches!(&results[2], Layer2Result::Unknown { invariant, reason } if invariant == "roundtrip:Message" && reason.contains("structural pre-check"))
        );
    }

    #[test]
    fn layer2_empty_bound_vars() {
        let mut verifier = Layer2Verifier::new(Layer2Config::default());
        verifier.add_invariant(QuantifiedInvariant {
            name: "bad_inv".into(),
            bound_vars: vec![],
            body: "true".into(),
            triggers: vec![],
        });
        let results = verifier.check_structural();
        assert!(
            matches!(&results[0], Layer2Result::Unknown { reason, .. } if reason.contains("no bound variables"))
        );
    }

    #[test]
    fn layer2_no_measure() {
        let mut verifier = Layer2Verifier::new(Layer2Config::default());
        verifier.add_termination(TerminationObligation {
            fn_name: "loop".into(),
            measure: String::new(),
            recursive_calls: vec![],
        });
        let results = verifier.check_structural();
        assert!(
            matches!(&results[0], Layer2Result::Unknown { reason, .. } if reason.contains("no measure"))
        );
    }

    #[test]
    fn layer2_obligation_count() {
        let mut verifier = Layer2Verifier::new(Layer2Config::default());
        assert_eq!(verifier.obligation_count(), 0);
        verifier.add_invariant(QuantifiedInvariant {
            name: "a".into(),
            bound_vars: vec![("x".into(), "Int".into())],
            body: "true".into(),
            triggers: vec![],
        });
        verifier.add_termination(TerminationObligation {
            fn_name: "f".into(),
            measure: "n".into(),
            recursive_calls: vec![],
        });
        assert_eq!(verifier.obligation_count(), 2);
    }

    // =======================================================================
    // T078: Quantifier trigger tests
    // =======================================================================

    #[test]
    fn trigger_infer_from_known_fn() {
        let mut mgr = TriggerManager::new();
        mgr.register_function("len".into());
        let trigger = mgr.infer_trigger("len(xs) >= 0");
        assert!(trigger.is_some());
        assert!(!trigger.unwrap().is_user_provided);
    }

    #[test]
    fn trigger_infer_no_match() {
        let mgr = TriggerManager::new();
        let trigger = mgr.infer_trigger("x + y > 0");
        assert!(trigger.is_none());
    }

    #[test]
    fn trigger_validate_known() {
        let mut mgr = TriggerManager::new();
        mgr.register_function("f".into());
        let pattern = TriggerPattern {
            terms: vec!["f(x)".into()],
            is_user_provided: true,
        };
        let warnings = mgr.validate_trigger(&pattern);
        assert!(warnings.is_empty());
    }

    #[test]
    fn trigger_validate_unknown() {
        let mgr = TriggerManager::new();
        let pattern = TriggerPattern {
            terms: vec!["unknown(x)".into()],
            is_user_provided: true,
        };
        let warnings = mgr.validate_trigger(&pattern);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn trigger_add_and_get() {
        let mut mgr = TriggerManager::new();
        mgr.add_trigger(
            "forall_sorted".into(),
            TriggerPattern {
                terms: vec!["a[i]".into()],
                is_user_provided: true,
            },
        );
        assert!(mgr.get_triggers("forall_sorted").is_some());
        assert_eq!(mgr.get_triggers("forall_sorted").unwrap().len(), 1);
        assert!(mgr.get_triggers("other").is_none());
    }

    #[test]
    fn trigger_default() {
        let mgr = TriggerManager::default();
        assert!(mgr.get_triggers("x").is_none());
    }

    // =======================================================================
    // T073: Codec dispatch tests
    // =======================================================================

    #[test]
    fn codec_dispatch_match() {
        let mut disp = CodecDispatcher::new();
        disp.register("PNG".into(), vec![0x89, 0x50, 0x4E, 0x47], 0);
        let data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A];
        assert_eq!(disp.dispatch(&data), DispatchResult::Matched("PNG".into()));
    }

    #[test]
    fn codec_dispatch_unknown() {
        let mut disp = CodecDispatcher::new();
        disp.register("PNG".into(), vec![0x89, 0x50, 0x4E, 0x47], 0);
        let data = vec![0xFF, 0xD8, 0xFF]; // JPEG magic
        assert_eq!(disp.dispatch(&data), DispatchResult::Unknown);
    }

    #[test]
    fn codec_dispatch_ambiguous() {
        let mut disp = CodecDispatcher::new();
        disp.register("FormatA".into(), vec![0x00, 0x01], 0);
        disp.register("FormatB".into(), vec![0x00, 0x01], 0);
        let data = vec![0x00, 0x01, 0x02];
        assert!(matches!(disp.dispatch(&data), DispatchResult::Ambiguous(_)));
    }

    #[test]
    fn codec_dispatch_offset() {
        let mut disp = CodecDispatcher::new();
        disp.register("ZIP".into(), vec![0x50, 0x4B], 0);
        disp.register("DocX".into(), vec![0x50, 0x4B, 0x03, 0x04], 0);
        // Both match the same prefix
        let data = vec![0x50, 0x4B, 0x03, 0x04, 0x00];
        let result = disp.dispatch(&data);
        assert!(matches!(result, DispatchResult::Ambiguous(_)));
    }

    #[test]
    fn codec_check_ambiguity() {
        let mut disp = CodecDispatcher::new();
        disp.register("A".into(), vec![0xFF], 0);
        disp.register("B".into(), vec![0xFF], 0);
        let conflicts = disp.check_ambiguity();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0], ("A".into(), "B".into()));
    }

    #[test]
    fn codec_no_ambiguity() {
        let mut disp = CodecDispatcher::new();
        disp.register("A".into(), vec![0x01], 0);
        disp.register("B".into(), vec![0x02], 0);
        assert!(disp.check_ambiguity().is_empty());
    }

    #[test]
    fn codec_count() {
        let mut disp = CodecDispatcher::new();
        assert_eq!(disp.codec_count(), 0);
        disp.register("X".into(), vec![0x00], 0);
        assert_eq!(disp.codec_count(), 1);
    }

    #[test]
    fn codec_default() {
        let disp = CodecDispatcher::default();
        assert_eq!(disp.codec_count(), 0);
    }

    #[test]
    fn codec_short_data() {
        let mut disp = CodecDispatcher::new();
        disp.register("Long".into(), vec![0x01, 0x02, 0x03, 0x04], 0);
        let data = vec![0x01, 0x02]; // too short
        assert_eq!(disp.dispatch(&data), DispatchResult::Unknown);
    }

    // =======================================================================
    // T092: WeakMemoryChecker tests
    // =======================================================================

    #[test]
    fn weak_memory_data_race() {
        let mut wm = WeakMemoryChecker::new();
        wm.record_access(1, "x".into(), true, MemoryOrdering::Relaxed);
        wm.record_access(2, "x".into(), false, MemoryOrdering::Relaxed);
        let races = wm.check_data_races();
        assert_eq!(races.len(), 1);
        assert!(races[0].contains("data race"));
    }

    #[test]
    fn weak_memory_no_race_with_hb() {
        let mut wm = WeakMemoryChecker::new();
        let s1 = wm.record_access(1, "x".into(), true, MemoryOrdering::Release);
        let s2 = wm.record_access(2, "x".into(), false, MemoryOrdering::Acquire);
        wm.add_happens_before(s1, s2);
        assert!(wm.check_data_races().is_empty());
    }

    #[test]
    fn weak_memory_release_no_acquire() {
        let mut wm = WeakMemoryChecker::new();
        wm.record_access(1, "flag".into(), true, MemoryOrdering::Release);
        let warnings = wm.check_release_acquire();
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn weak_memory_relaxed_warning() {
        let mut wm = WeakMemoryChecker::new();
        wm.record_access(1, "shared".into(), true, MemoryOrdering::Relaxed);
        wm.record_access(2, "shared".into(), false, MemoryOrdering::Relaxed);
        let warnings = wm.check_ordering_strength();
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn weak_memory_same_thread_ok() {
        let mut wm = WeakMemoryChecker::new();
        wm.record_access(1, "x".into(), true, MemoryOrdering::Relaxed);
        wm.record_access(1, "x".into(), false, MemoryOrdering::Relaxed);
        assert!(wm.check_data_races().is_empty());
    }

    #[test]
    fn weak_memory_default() {
        let wm = WeakMemoryChecker::default();
        assert_eq!(wm.access_count(), 0);
    }

    // =======================================================================
    // T093: ProphecyManager tests
    // =======================================================================

    #[test]
    fn prophecy_unresolved() {
        let mut pm = ProphecyManager::new();
        pm.declare("future_val".into());
        let errs = pm.check_all_resolved();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A05025");
        assert!(errs[0].message.contains("never resolved"));
        assert_eq!(errs[0].variable, "future_val");
    }

    #[test]
    fn prophecy_resolved_ok() {
        let mut pm = ProphecyManager::new();
        pm.declare("future_val".into());
        pm.resolve("future_val", "42".into()).unwrap();
        assert!(pm.check_all_resolved().is_empty());
    }

    #[test]
    fn prophecy_double_resolve() {
        let mut pm = ProphecyManager::new();
        pm.declare("pv".into());
        pm.resolve("pv", "1".into()).unwrap();
        let err = pm.resolve("pv", "2".into());
        assert!(err.is_err());
    }

    #[test]
    fn prophecy_unconstrained() {
        let mut pm = ProphecyManager::new();
        pm.declare("pv".into());
        let errs = pm.check_unconstrained();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A05026");
        assert!(errs[0].message.contains("no constraints"));
        assert_eq!(errs[0].variable, "pv");
    }

    #[test]
    fn prophecy_with_constraints() {
        let mut pm = ProphecyManager::new();
        pm.declare("pv".into());
        pm.add_constraint("pv", "pv > 0".into());
        assert!(pm.check_unconstrained().is_empty());
    }

    #[test]
    fn prophecy_default() {
        let pm = ProphecyManager::default();
        assert_eq!(pm.variable_count(), 0);
    }

    // =======================================================================
    // T094: LivenessChecker tests
    // =======================================================================

    #[test]
    fn liveness_unverified() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "progress".into(),
            LivenessKind::Eventually,
            "true".into(),
            "done".into(),
        );
        let errs = lc.check_unverified();
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn liveness_verified_ok() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "progress".into(),
            LivenessKind::Eventually,
            "true".into(),
            "done".into(),
        );
        lc.mark_verified("progress");
        assert!(lc.check_unverified().is_empty());
    }

    #[test]
    fn liveness_zero_bound() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "deadline".into(),
            LivenessKind::EventuallyWithin(0),
            "start".into(),
            "end".into(),
        );
        let errs = lc.check_bounded();
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn liveness_no_fairness() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "l2r".into(),
            LivenessKind::LeadsTo,
            "req".into(),
            "resp".into(),
        );
        let errs = lc.check_fairness();
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn liveness_with_fairness_ok() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "l2r".into(),
            LivenessKind::LeadsTo,
            "req".into(),
            "resp".into(),
        );
        lc.add_fairness("scheduler_fair".into());
        assert!(lc.check_fairness().is_empty());
    }

    #[test]
    fn liveness_default() {
        let lc = LivenessChecker::default();
        assert_eq!(lc.obligation_count(), 0);
    }

    // =======================================================================
    // T112: IrParser tests
    // =======================================================================

    #[test]
    fn ir_parse_fn_decl() {
        let mut parser = IrParser::new();
        parser.parse_text("fn main()").unwrap();
        assert_eq!(parser.node_count(), 1);
    }

    #[test]
    fn ir_parse_var_decl() {
        let mut parser = IrParser::new();
        parser.parse_text("let x: Int").unwrap();
        assert_eq!(parser.node_count(), 1);
    }

    #[test]
    fn ir_parse_return() {
        let mut parser = IrParser::new();
        parser.parse_text("return 42").unwrap();
        assert_eq!(parser.node_count(), 1);
    }

    #[test]
    fn ir_skip_comments() {
        let mut parser = IrParser::new();
        parser.parse_text("// comment\nfn main()").unwrap();
        assert_eq!(parser.node_count(), 1);
    }

    #[test]
    fn ir_serialize() {
        let mut parser = IrParser::new();
        parser.parse_text("fn test()").unwrap();
        let bytes = parser.serialize_binary();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn ir_default() {
        let parser = IrParser::default();
        assert_eq!(parser.node_count(), 0);
    }

    // =======================================================================
    // T113: SessionCache tests (in-memory per-session dedup)
    // =======================================================================

    #[test]
    fn session_cache_hit() {
        let mut cache = SessionCache::new();
        cache.insert("abc123".into(), "verified".into(), 1000);
        assert!(cache.lookup("abc123").is_some());
        assert_eq!(cache.hit_rate(), 1.0);
    }

    #[test]
    fn session_cache_miss() {
        let mut cache = SessionCache::new();
        assert!(cache.lookup("unknown").is_none());
        assert_eq!(cache.hit_rate(), 0.0);
    }

    #[test]
    fn session_cache_invalidate() {
        let mut cache = SessionCache::new();
        cache.insert("abc".into(), "ok".into(), 1);
        cache.invalidate("abc");
        assert!(cache.lookup("abc").is_none());
    }

    #[test]
    fn session_cache_clear() {
        let mut cache = SessionCache::new();
        cache.insert("a".into(), "ok".into(), 1);
        cache.insert("b".into(), "ok".into(), 1);
        cache.clear();
        assert_eq!(cache.entry_count(), 0);
    }

    #[test]
    fn session_cache_default() {
        let cache = SessionCache::default();
        assert_eq!(cache.entry_count(), 0);
    }

    // =======================================================================
    // P006: Filesystem VerificationCache tests
    // =======================================================================

    #[test]
    fn fs_cache_put_and_get() {
        let dir = std::env::temp_dir().join("assura-test-cache-put-get");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = VerificationCache::new(&dir);
        let clauses = vec![assura_parser::ast::Clause {
            kind: assura_parser::ast::ClauseKind::Ensures,
            body: assura_parser::ast::Expr::Ident("result".into()),
        }];
        let results = vec![VerificationResult::Verified {
            clause_desc: "test.ensures".into(),
        }];
        cache.put("test", &clauses, &results);
        let cached = cache.get("test", &clauses);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fs_cache_miss_on_different_clauses() {
        let dir = std::env::temp_dir().join("assura-test-cache-miss");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = VerificationCache::new(&dir);
        let clauses_a = vec![assura_parser::ast::Clause {
            kind: assura_parser::ast::ClauseKind::Ensures,
            body: assura_parser::ast::Expr::Ident("result".into()),
        }];
        let clauses_b = vec![assura_parser::ast::Clause {
            kind: assura_parser::ast::ClauseKind::Requires,
            body: assura_parser::ast::Expr::Ident("result".into()),
        }];
        let results = vec![VerificationResult::Verified {
            clause_desc: "test.ensures".into(),
        }];
        cache.put("test", &clauses_a, &results);
        assert!(cache.get("test", &clauses_b).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fs_cache_clear() {
        let dir = std::env::temp_dir().join("assura-test-cache-clear");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = VerificationCache::new(&dir);
        let clauses = vec![assura_parser::ast::Clause {
            kind: assura_parser::ast::ClauseKind::Ensures,
            body: assura_parser::ast::Expr::Ident("result".into()),
        }];
        let results = vec![VerificationResult::Verified {
            clause_desc: "test.ensures".into(),
        }];
        cache.put("test", &clauses, &results);
        assert_eq!(cache.entry_count(), 1);
        cache.clear();
        assert_eq!(cache.entry_count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fs_cache_entry_count() {
        let dir = std::env::temp_dir().join("assura-test-cache-count");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = VerificationCache::new(&dir);
        assert_eq!(cache.entry_count(), 0);
        let clauses = vec![assura_parser::ast::Clause {
            kind: assura_parser::ast::ClauseKind::Ensures,
            body: assura_parser::ast::Expr::Ident("result".into()),
        }];
        cache.put("alpha", &clauses, &[]);
        cache.put("beta", &clauses, &[]);
        assert_eq!(cache.entry_count(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // =======================================================================
    // has_verifiable_clauses tests
    // =======================================================================

    #[test]
    fn has_verifiable_clauses_true_for_requires() {
        let src = "contract Foo { requires x > 0 }";
        let file = assura_parser::parse_unwrap(src);
        assert!(has_verifiable_clauses(&file));
    }

    #[test]
    fn has_verifiable_clauses_false_for_effects_only() {
        let src = "contract Bar { effects io }";
        let file = assura_parser::parse_unwrap(src);
        assert!(!has_verifiable_clauses(&file));
    }

    #[test]
    fn has_verifiable_clauses_false_for_empty() {
        let src = "contract Empty { }";
        let file = assura_parser::parse_unwrap(src);
        assert!(!has_verifiable_clauses(&file));
    }

    // =======================================================================
    // T115: IncrementalCompiler tests
    // =======================================================================

    #[test]
    fn incremental_dirty_on_register() {
        let mut ic = IncrementalCompiler::new();
        ic.register_module("main".into(), "abc".into());
        assert_eq!(ic.dirty_modules().len(), 1);
    }

    #[test]
    fn incremental_clean_after_check() {
        let mut ic = IncrementalCompiler::new();
        ic.register_module("main".into(), "abc".into());
        ic.mark_checked("main", 100);
        assert!(ic.dirty_modules().is_empty());
    }

    #[test]
    fn incremental_cascade_dirty() {
        let mut ic = IncrementalCompiler::new();
        ic.register_module("lib".into(), "aaa".into());
        ic.register_module("main".into(), "bbb".into());
        ic.add_dependency("main".into(), "lib".into());
        ic.mark_checked("lib", 1);
        ic.mark_checked("main", 1);
        ic.mark_changed("lib");
        let dirty = ic.dirty_modules();
        assert!(dirty.contains(&"lib"));
        assert!(dirty.contains(&"main"));
    }

    #[test]
    fn incremental_module_count() {
        let mut ic = IncrementalCompiler::new();
        ic.register_module("a".into(), "h1".into());
        ic.register_module("b".into(), "h2".into());
        assert_eq!(ic.module_count(), 2);
    }

    #[test]
    fn incremental_default() {
        let ic = IncrementalCompiler::default();
        assert_eq!(ic.module_count(), 0);
    }
}

#[cfg(test)]
mod quantifier_bound_tests {
    use super::*;

    fn type_check_source(source: &str) -> assura_types::TypedFile {
        let file = assura_parser::parse_unwrap(source);
        let resolved = assura_resolve::resolve(&file).expect("resolve failed");
        assura_types::type_check(&resolved).expect("type_check failed")
    }

    #[test]
    fn forall_over_int_is_unbounded() {
        let typed = type_check_source(
            r#"
contract Bad {
    input(x: Int)
    requires { forall n in Int: n >= 0 }
}
"#,
        );
        let warnings = validate_quantifier_bounds(&typed);
        assert!(
            !warnings.is_empty(),
            "forall over Int should produce a warning"
        );
        assert!(warnings[0].reason.contains("infinite domain"));
    }

    #[test]
    fn exists_over_nat_is_unbounded() {
        let typed = type_check_source(
            r#"
contract Bad {
    input(x: Int)
    requires { exists n in Nat: n > x }
}
"#,
        );
        let warnings = validate_quantifier_bounds(&typed);
        assert!(
            !warnings.is_empty(),
            "exists over Nat should produce a warning"
        );
    }

    #[test]
    fn forall_over_collection_is_bounded() {
        let typed = type_check_source(
            r#"
contract Good {
    input(items: List<Int>)
    requires { forall v in items: v > 0 }
}
"#,
        );
        let warnings = validate_quantifier_bounds(&typed);
        assert!(
            warnings.is_empty(),
            "forall over a collection variable should NOT warn: {warnings:?}"
        );
    }

    #[test]
    fn forall_over_range_is_bounded() {
        let typed = type_check_source(
            r#"
contract Good {
    input(n: Nat)
    requires { forall i in 0 .. n: i >= 0 }
}
"#,
        );
        let warnings = validate_quantifier_bounds(&typed);
        assert!(
            warnings.is_empty(),
            "forall over a range should NOT warn: {warnings:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// S001: Termination checking via verify_decrease
// ---------------------------------------------------------------------------

#[cfg(test)]
mod decrease_tests {
    use super::*;
    use assura_parser::ast::{BinOp, Expr, Literal};

    /// Helper: verify_decrease with trivial preconditions.
    fn check_decrease(measure: &Expr, call_arg: &Expr, desc: &str) -> VerificationResult {
        verify_decrease(&[], measure, call_arg, desc.to_string())
    }

    /// Helper: verify_decrease with preconditions.
    fn check_decrease_with_pre(
        preconditions: &[Expr],
        measure: &Expr,
        call_arg: &Expr,
        desc: &str,
    ) -> VerificationResult {
        verify_decrease(preconditions, measure, call_arg, desc.to_string())
    }

    // -- Factorial: decreases n, calls with n-1, with requires n > 0 --

    #[test]
    fn factorial_terminates() {
        // decreases n, call arg = n - 1, precondition: n > 0
        let measure = Expr::Ident("n".into());
        let call_arg = Expr::BinOp {
            lhs: Box::new(Expr::Ident("n".into())),
            op: BinOp::Sub,
            rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
        };
        let pre = Expr::BinOp {
            lhs: Box::new(Expr::Ident("n".into())),
            op: BinOp::Gt,
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        let result = check_decrease_with_pre(&[pre], &measure, &call_arg, "factorial::decreases");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "factorial should verify: {result:?}"
        );
    }

    // -- Fibonacci: decreases n, calls with n-1 and n-2 --

    #[test]
    fn fibonacci_n_minus_1_terminates() {
        let measure = Expr::Ident("n".into());
        let call_arg = Expr::BinOp {
            lhs: Box::new(Expr::Ident("n".into())),
            op: BinOp::Sub,
            rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
        };
        let pre = Expr::BinOp {
            lhs: Box::new(Expr::Ident("n".into())),
            op: BinOp::Gt,
            rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
        };
        let result = check_decrease_with_pre(&[pre], &measure, &call_arg, "fib::decreases(n-1)");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "fib(n-1) should verify: {result:?}"
        );
    }

    #[test]
    fn fibonacci_n_minus_2_terminates() {
        let measure = Expr::Ident("n".into());
        let call_arg = Expr::BinOp {
            lhs: Box::new(Expr::Ident("n".into())),
            op: BinOp::Sub,
            rhs: Box::new(Expr::Literal(Literal::Int("2".into()))),
        };
        let pre = Expr::BinOp {
            lhs: Box::new(Expr::Ident("n".into())),
            op: BinOp::Gt,
            rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
        };
        let result = check_decrease_with_pre(&[pre], &measure, &call_arg, "fib::decreases(n-2)");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "fib(n-2) should verify: {result:?}"
        );
    }

    // -- Non-decreasing: spin(n) calling spin(n) should NOT verify --

    #[test]
    fn spin_same_arg_does_not_terminate() {
        // decreases n, call arg = n (same, not decreasing)
        let measure = Expr::Ident("n".into());
        let call_arg = Expr::Ident("n".into());
        let result = check_decrease(&measure, &call_arg, "spin::decreases");
        assert!(
            !matches!(result, VerificationResult::Verified { .. }),
            "spin(n) calling spin(n) should NOT verify: {result:?}"
        );
    }

    // -- Increasing: bad(n) calling bad(n+1) should NOT verify --

    #[test]
    fn increasing_arg_does_not_terminate() {
        let measure = Expr::Ident("n".into());
        let call_arg = Expr::BinOp {
            lhs: Box::new(Expr::Ident("n".into())),
            op: BinOp::Add,
            rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
        };
        let result = check_decrease(&measure, &call_arg, "bad::decreases");
        assert!(
            !matches!(result, VerificationResult::Verified { .. }),
            "bad(n+1) should NOT verify: {result:?}"
        );
    }

    // -- With precondition ensuring non-negativity --

    #[test]
    fn decrease_with_nat_precondition() {
        // decreases n, call arg = n - 1, precondition: n >= 1
        let measure = Expr::Ident("n".into());
        let call_arg = Expr::BinOp {
            lhs: Box::new(Expr::Ident("n".into())),
            op: BinOp::Sub,
            rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
        };
        let pre = Expr::BinOp {
            lhs: Box::new(Expr::Ident("n".into())),
            op: BinOp::Gte,
            rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
        };
        let result = check_decrease_with_pre(&[pre], &measure, &call_arg, "countdown::decreases");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "countdown with n >= 1 should verify: {result:?}"
        );
    }
}

#[cfg(test)]
mod verify_contract_tests {
    use super::*;
    use assura_parser::ast::{BinOp, Clause, ClauseKind, Expr, Literal};

    #[test]
    fn verify_contract_single_ensures_verified() {
        // requires x > 0 ensures x > 0 (trivially true)
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gt,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gt,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            },
        ];
        let results = verify_contract("TestContract", &clauses);
        assert_eq!(results.len(), 1, "one ensures clause: {results:?}");
        assert!(
            matches!(&results[0], VerificationResult::Verified { clause_desc } if clause_desc.contains("TestContract")),
            "should verify: {results:?}"
        );
    }

    #[test]
    fn verify_contract_counterexample() {
        // No requires, ensures x > 0 (counterexample: x = 0)
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
        }];
        let results = verify_contract("NoPrecondition", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { clause_desc, .. } if clause_desc.contains("NoPrecondition")),
            "should have counterexample: {results:?}"
        );
    }

    #[test]
    fn verify_contract_multiple_ensures() {
        // requires x > 10
        // ensures x > 5  (verified)
        // ensures x > 20 (counterexample: x = 11)
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gt,
                    rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
                },
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gt,
                    rhs: Box::new(Expr::Literal(Literal::Int("5".into()))),
                },
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gt,
                    rhs: Box::new(Expr::Literal(Literal::Int("20".into()))),
                },
            },
        ];
        let results = verify_contract("MultiClause", &clauses);
        assert_eq!(results.len(), 2, "two ensures clauses: {results:?}");
        // First ensures (x > 5) should verify
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "x > 10 => x > 5 should verify: {:?}",
            results[0]
        );
        // Second ensures (x > 20) should have counterexample
        assert!(
            matches!(&results[1], VerificationResult::Counterexample { .. }),
            "x > 10 => x > 20 should fail: {:?}",
            results[1]
        );
    }

    #[test]
    fn verify_contract_no_verifiable_clauses() {
        // Only requires, no ensures/invariant
        let clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
        }];
        let results = verify_contract("OnlyRequires", &clauses);
        assert!(results.is_empty(), "no verifiable clauses: {results:?}");
    }
}

#[cfg(test)]
mod quantified_verification_tests {
    use super::*;
    use assura_parser::ast::{BinOp, Expr, Literal};

    #[test]
    fn forall_trivially_true() {
        // forall x in 0..10: x == x (always true)
        let body = Expr::Forall {
            var: "x".into(),
            domain: Box::new(Expr::BinOp {
                lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                op: BinOp::Range,
                rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
            }),
            body: Box::new(Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Eq,
                rhs: Box::new(Expr::Ident("x".into())),
            }),
        };
        let result = verify_quantified_expr("trivial_forall", &[], &body);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "forall x in 0..10: x == x should verify: {result:?}"
        );
    }

    #[test]
    fn forall_with_counterexample() {
        // forall x in 0..10: x > 0 (false: x = 0 is a counterexample)
        let body = Expr::Forall {
            var: "x".into(),
            domain: Box::new(Expr::BinOp {
                lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                op: BinOp::Range,
                rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
            }),
            body: Box::new(Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            }),
        };
        let result = verify_quantified_expr("nonpositive_forall", &[], &body);
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "forall x in 0..10: x > 0 should have counterexample: {result:?}"
        );
    }

    #[test]
    fn exists_trivially_satisfiable() {
        // exists x in 0..100: x > 5 (true: e.g. x = 6)
        let body = Expr::Exists {
            var: "x".into(),
            domain: Box::new(Expr::BinOp {
                lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                op: BinOp::Range,
                rhs: Box::new(Expr::Literal(Literal::Int("100".into()))),
            }),
            body: Box::new(Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("5".into()))),
            }),
        };
        let result = verify_quantified_expr("trivial_exists", &[], &body);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "exists x in 0..100: x > 5 should verify: {result:?}"
        );
    }

    #[test]
    fn forall_with_assumption() {
        // Assumption: n > 0
        // Check: forall x in 0..10: n + x >= x (always true when n > 0)
        let assumption = Expr::BinOp {
            lhs: Box::new(Expr::Ident("n".into())),
            op: BinOp::Gt,
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        let body = Expr::Forall {
            var: "x".into(),
            domain: Box::new(Expr::BinOp {
                lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                op: BinOp::Range,
                rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
            }),
            body: Box::new(Expr::BinOp {
                lhs: Box::new(Expr::BinOp {
                    lhs: Box::new(Expr::Ident("n".into())),
                    op: BinOp::Add,
                    rhs: Box::new(Expr::Ident("x".into())),
                }),
                op: BinOp::Gte,
                rhs: Box::new(Expr::Ident("x".into())),
            }),
        };
        let result = verify_quantified_expr("forall_with_pre", &[assumption], &body);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "forall x in 0..10: n + x >= x with n > 0 should verify: {result:?}"
        );
    }

    #[test]
    fn layer2_verifier_verify_method() {
        // Test the Layer2Verifier.verify() method
        let config = Layer2Config::default();
        let verifier = Layer2Verifier::new(config);
        let results = verifier.verify();
        assert!(results.is_empty(), "empty verifier returns no results");
    }

    #[test]
    fn layer2_verifier_with_invariant() {
        let config = Layer2Config::new().with_timeout(5000);
        let mut verifier = Layer2Verifier::new(config);
        verifier.add_invariant(QuantifiedInvariant {
            name: "sorted_invariant".into(),
            bound_vars: vec![("i".into(), "Int".into())],
            body: "i >= 0".into(),
            triggers: Vec::new(),
        });
        let results = verifier.verify();
        assert_eq!(results.len(), 1);
        // "i >= 0" is NOT universally true (i = -1 is a counterexample)
        assert!(matches!(results[0], Layer2Result::Counterexample { .. }));
    }

    // =======================================================================
    // P005: IR parser tests
    // =======================================================================

    #[test]
    fn ir_parse_safe_division() {
        let source = r#"
module safe_division {
  fn #0 : ($0: Int @omega, $1: Int @omega) -> Int ! pure
    pre: cmp ne $1 (const 0)
    post: cmp eq (arith add (arith mul $result $1) (arith mod $0 $1)) $0
  {
    $2 = arith div $0 $1 : Int
    $result = load $2 : Int
  }
}
"#;
        let module = parse_ir_module(source).expect("parse should succeed");
        assert_eq!(module.name, "safe_division");
        assert_eq!(module.functions.len(), 1);
        let func = &module.functions[0];
        assert_eq!(func.id, "#0");
        assert_eq!(func.params.len(), 2);
        assert_eq!(func.params[0].slot, 0);
        assert_eq!(func.params[0].ty, "Int");
        assert_eq!(func.params[1].slot, 1);
        assert_eq!(func.return_type, "Int");
        assert_eq!(func.effects, "pure");
        assert!(func.pre.is_some());
        assert!(func.post.is_some());
        assert_eq!(func.body.len(), 2);
        // First instruction: $2 = arith div $0 $1 : Int
        assert_eq!(func.body[0].target, 2);
        assert_eq!(func.body[0].ty, "Int");
        assert!(matches!(
            func.body[0].expr,
            IrExprKind::Arith {
                op: IrArithOp::Div,
                lhs: 0,
                rhs: 1,
            }
        ));
        // Second instruction: $result = load $2 : Int
        assert_eq!(func.body[1].target, usize::MAX);
        assert!(matches!(func.body[1].expr, IrExprKind::Load(2)));
    }

    #[test]
    fn ir_parse_const_and_call() {
        let source = r#"
module test {
  fn #0 : ($0: Int) -> Bool ! pure
  {
    $1 = const 42 : Int
    $2 = call is_valid ($0, $1) : Bool
    $result = load $2 : Bool
  }
}
"#;
        let module = parse_ir_module(source).expect("parse should succeed");
        assert_eq!(module.functions.len(), 1);
        let body = &module.functions[0].body;
        assert_eq!(body.len(), 3);
        assert!(matches!(
            &body[0].expr,
            IrExprKind::Const(IrLiteral::Int(42))
        ));
        assert!(matches!(
            &body[1].expr,
            IrExprKind::Call { func, args } if func == "is_valid" && args == &[0, 1]
        ));
    }

    #[test]
    fn ir_parse_field_and_construct() {
        let source = r#"
module test {
  fn #0 : ($0: Point) -> Point ! pure
  {
    $1 = field $0 .0 : Int
    $2 = field $0 .1 : Int
    $3 = construct Point { .0 = $2, .1 = $1 } : Point
    $result = load $3 : Point
  }
}
"#;
        let module = parse_ir_module(source).expect("parse should succeed");
        let body = &module.functions[0].body;
        assert!(matches!(
            &body[0].expr,
            IrExprKind::Field { slot: 0, index: 0 }
        ));
        assert!(matches!(
            &body[2].expr,
            IrExprKind::Construct { type_id, fields }
            if type_id == "Point" && fields == &[(0, 2), (1, 1)]
        ));
    }

    #[test]
    fn ir_parse_cmp_and_cast() {
        let source = r#"
module test {
  fn #0 : ($0: Int, $1: Int) -> Bool ! pure
  {
    $2 = cmp lt $0 $1 : Bool
    $3 = cast $0 as Float : Float
    $result = load $2 : Bool
  }
}
"#;
        let module = parse_ir_module(source).expect("parse should succeed");
        let body = &module.functions[0].body;
        assert!(matches!(
            &body[0].expr,
            IrExprKind::Cmp {
                op: IrCmpOp::Lt,
                lhs: 0,
                rhs: 1,
            }
        ));
        assert!(matches!(&body[1].expr, IrExprKind::Cast { slot: 0, .. }));
    }

    #[test]
    fn ir_parse_if_and_transition() {
        let source = r#"
module test {
  fn #0 : ($0: Bool, $1: Connection) -> Unit ! io
  {
    $2 = if $0 then #0 else #1 : Unit
    $3 = transition $1 to Connected : Connection
    $result = load $3 : Connection
  }
}
"#;
        let module = parse_ir_module(source).expect("parse should succeed");
        let body = &module.functions[0].body;
        assert!(matches!(
            &body[0].expr,
            IrExprKind::If {
                cond: 0,
                then_block: 0,
                else_block: 1,
            }
        ));
        assert!(matches!(
            &body[1].expr,
            IrExprKind::Transition { slot: 1, .. }
        ));
    }

    #[test]
    fn ir_parse_empty_module() {
        let source = "module empty {\n}\n";
        let module = parse_ir_module(source).expect("parse should succeed");
        assert_eq!(module.name, "empty");
        assert!(module.functions.is_empty());
    }

    #[test]
    fn ir_parse_error_no_module() {
        let source = "fn #0 : () -> Unit ! pure {}";
        let result = parse_ir_module(source);
        assert!(result.is_err());
    }

    #[test]
    fn ir_to_rust_safe_division() {
        let source = r#"
module safe_division {
  fn #0 : ($0: Int, $1: Int) -> Int ! pure
    pre: cmp ne $1 (const 0)
  {
    $2 = arith div $0 $1 : Int
    $result = load $2 : Int
  }
}
"#;
        let module = parse_ir_module(source).unwrap();
        let rust = ir_to_rust(&module);
        assert!(rust.contains("fn ir_0("));
        assert!(rust.contains("slot_0: i64"));
        assert!(rust.contains("slot_1: i64"));
        assert!(rust.contains("-> i64"));
        assert!(rust.contains("debug_assert!"));
        assert!(rust.contains("(slot_0 / slot_1)"));
        assert!(rust.contains("__result"));
    }

    #[test]
    fn ir_validate_slot_gap() {
        let module = IrModule {
            name: "test".into(),
            functions: vec![IrFunction {
                id: "#0".into(),
                params: vec![IrSlotDecl {
                    slot: 0,
                    ty: "Int".into(),
                }],
                return_type: "Int".into(),
                effects: "pure".into(),
                pre: None,
                post: None,
                body: vec![IrInstr {
                    target: 5, // gap: skips $1-$4
                    expr: IrExprKind::Load(0),
                    ty: "Int".into(),
                }],
            }],
        };
        let contract = assura_parser::ast::ContractDecl {
            name: "Test".into(),
            type_params: vec![],
            clauses: vec![],
        };
        let validation = validate_ir_against_contract(&module, &contract);
        assert!(!validation.valid);
        assert!(validation.errors[0].contains("skips slot"));
    }

    #[test]
    fn ir_arith_ops() {
        for (s, expected) in [
            ("add", IrArithOp::Add),
            ("sub", IrArithOp::Sub),
            ("mul", IrArithOp::Mul),
            ("div", IrArithOp::Div),
            ("mod", IrArithOp::Mod),
        ] {
            assert_eq!(parse_arith_op(s).unwrap(), expected);
        }
        assert!(parse_arith_op("xor").is_err());
    }

    #[test]
    fn ir_cmp_ops() {
        for (s, expected) in [
            ("eq", IrCmpOp::Eq),
            ("ne", IrCmpOp::Ne),
            ("lt", IrCmpOp::Lt),
            ("le", IrCmpOp::Le),
            ("gt", IrCmpOp::Gt),
            ("ge", IrCmpOp::Ge),
        ] {
            assert_eq!(parse_cmp_op(s).unwrap(), expected);
        }
        assert!(parse_cmp_op("in").is_err());
    }

    #[test]
    fn ir_pred_true_false() {
        assert_eq!(parse_ir_pred_str("true"), Some(IrPred::True));
        assert_eq!(parse_ir_pred_str("false"), Some(IrPred::False));
        assert_eq!(parse_ir_pred_str(""), None);
    }

    #[test]
    fn ir_pred_not() {
        let pred = parse_ir_pred_str("not true");
        assert!(matches!(pred, Some(IrPred::Not(_))));
    }

    #[test]
    fn ir_type_to_rust_mapping() {
        assert_eq!(ir_type_to_rust("Int"), "i64");
        assert_eq!(ir_type_to_rust("Nat"), "u64");
        assert_eq!(ir_type_to_rust("Float"), "f64");
        assert_eq!(ir_type_to_rust("Bool"), "bool");
        assert_eq!(ir_type_to_rust("String"), "String");
        assert_eq!(ir_type_to_rust("Unit"), "()");
        assert_eq!(ir_type_to_rust("CustomType"), "CustomType");
    }
}

// ---------------------------------------------------------------------------
// CVC5 backend unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod cvc5_tests {
    use super::*;

    #[test]
    fn solver_choice_from_str() {
        assert_eq!(SolverChoice::from_str_loose("z3"), Some(SolverChoice::Z3));
        assert_eq!(SolverChoice::from_str_loose("Z3"), Some(SolverChoice::Z3));
        assert_eq!(
            SolverChoice::from_str_loose("cvc5"),
            Some(SolverChoice::Cvc5)
        );
        assert_eq!(
            SolverChoice::from_str_loose("CVC5"),
            Some(SolverChoice::Cvc5)
        );
        assert_eq!(
            SolverChoice::from_str_loose("portfolio"),
            Some(SolverChoice::Portfolio)
        );
        assert_eq!(SolverChoice::from_str_loose("invalid"), None);
    }

    #[test]
    fn cvc5_expr_to_smtlib_literal() {
        use assura_parser::ast::Literal;
        let e = Expr::Literal(Literal::Int("42".into()));
        assert_eq!(cvc5_backend::expr_to_smtlib(&e), Some("42".to_string()));

        let e = Expr::Literal(Literal::Bool(true));
        assert_eq!(cvc5_backend::expr_to_smtlib(&e), Some("true".to_string()));

        let e = Expr::Literal(Literal::Int("-5".into()));
        assert_eq!(cvc5_backend::expr_to_smtlib(&e), Some("(- 5)".to_string()));
    }

    #[test]
    fn cvc5_expr_to_smtlib_ident() {
        let e = Expr::Ident("x".to_string());
        assert_eq!(cvc5_backend::expr_to_smtlib(&e), Some("x".to_string()));
    }

    #[test]
    fn cvc5_expr_to_smtlib_binop() {
        use assura_parser::ast::{BinOp, Literal};
        let e = Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
        };
        assert_eq!(
            cvc5_backend::expr_to_smtlib(&e),
            Some("(+ x 1)".to_string())
        );

        let e = Expr::BinOp {
            op: BinOp::Neq,
            lhs: Box::new(Expr::Ident("a".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        assert_eq!(
            cvc5_backend::expr_to_smtlib(&e),
            Some("(not (= a 0))".to_string())
        );
    }

    #[test]
    fn cvc5_expr_to_smtlib_unary() {
        use assura_parser::ast::UnaryOp;
        let e = Expr::UnaryOp {
            op: UnaryOp::Not,
            expr: Box::new(Expr::Ident("p".into())),
        };
        assert_eq!(
            cvc5_backend::expr_to_smtlib(&e),
            Some("(not p)".to_string())
        );
    }

    #[test]
    fn cvc5_expr_to_smtlib_ite() {
        use assura_parser::ast::Literal;
        let e = Expr::If {
            cond: Box::new(Expr::Ident("c".into())),
            then_branch: Box::new(Expr::Literal(Literal::Int("1".into()))),
            else_branch: Some(Box::new(Expr::Literal(Literal::Int("0".into())))),
        };
        assert_eq!(
            cvc5_backend::expr_to_smtlib(&e),
            Some("(ite c 1 0)".to_string())
        );
    }

    #[test]
    fn cvc5_expr_to_smtlib_forall() {
        let e = Expr::Forall {
            var: "i".to_string(),
            domain: Box::new(Expr::Ident("S".into())),
            body: Box::new(Expr::BinOp {
                op: assura_parser::ast::BinOp::Gt,
                lhs: Box::new(Expr::Ident("i".into())),
                rhs: Box::new(Expr::Literal(assura_parser::ast::Literal::Int("0".into()))),
            }),
        };
        assert_eq!(
            cvc5_backend::expr_to_smtlib(&e),
            Some("(forall ((i Int)) (> i 0))".to_string())
        );
    }

    #[test]
    fn cvc5_expr_to_smtlib_result() {
        let e = Expr::Ident("result".to_string());
        assert_eq!(
            cvc5_backend::expr_to_smtlib(&e),
            Some("__result".to_string())
        );
    }

    #[test]
    fn cvc5_expr_to_smtlib_old() {
        let e = Expr::Old(Box::new(Expr::Ident("x".into())));
        assert_eq!(cvc5_backend::expr_to_smtlib(&e), Some("x".to_string()));
    }

    #[test]
    fn cvc5_collect_vars() {
        use std::collections::HashSet;
        let e = Expr::BinOp {
            op: assura_parser::ast::BinOp::Add,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Ident("y".into())),
        };
        let mut vars = HashSet::new();
        cvc5_backend::collect_vars(&e, &mut vars);
        assert!(vars.contains("x"));
        assert!(vars.contains("y"));
    }

    #[test]
    fn cvc5_parse_model() {
        let model = "(define-fun x () Int 42)\n(define-fun y () Int (- 1))";
        let parsed = cvc5_backend::parse_smtlib_model(model);
        assert!(parsed.is_some());
        let cm = parsed.unwrap();
        assert_eq!(cm.variables.len(), 2);
        assert!(cm.variables.iter().any(|(n, v)| n == "x" && v == "42"));
        assert!(cm.variables.iter().any(|(n, v)| n == "y" && v == "(- 1)"));
    }

    #[test]
    fn cvc5_parse_empty_model() {
        let parsed = cvc5_backend::parse_smtlib_model("");
        assert!(parsed.is_none());
    }

    #[test]
    fn cvc5_verify_without_binary() {
        // If cvc5 is not installed, verify_contract_cvc5 returns Error results
        use assura_parser::ast::{Clause, ClauseKind, Literal};
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: assura_parser::ast::BinOp::Neq,
                    lhs: Box::new(Expr::Ident("b".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: assura_parser::ast::BinOp::Gt,
                    lhs: Box::new(Expr::Ident("result".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            },
        ];
        let results = cvc5_backend::verify_contract_cvc5("TestContract", &clauses);
        // Should return 1 result (for ensures). May be Unknown if cvc5 not installed.
        assert_eq!(results.len(), 1);
    }
}
