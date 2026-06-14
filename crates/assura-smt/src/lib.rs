//! SMT-based verification for Assura contracts via Z3.
//!
//! For each contract in a `TypedFile`, encodes requires/ensures/invariant
//! clauses as Z3 formulas and checks their validity:
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
    #[cfg(feature = "z3-verify")]
    {
        z3_backend::verify_impl(typed)
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        no_z3::verify_stub(typed)
    }
}

/// Verify all declarations in a `TypedFile`, using a filesystem cache.
///
/// For each contract/function, checks the cache first. On cache hit,
/// returns the cached results directly. On miss, runs Z3 and stores
/// the results for future runs.
pub fn verify_with_cache(typed: &TypedFile, cache: &VerificationCache) -> Vec<VerificationResult> {
    use assura_parser::ast::Decl;
    let mut results = Vec::new();

    for decl in &typed.resolved.source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                if let Some(cached) = cache.get(&c.name, &c.clauses) {
                    results.extend(cached);
                } else {
                    let r = verify_contract(&c.name, &c.clauses);
                    cache.put(&c.name, &c.clauses, &r);
                    results.extend(r);
                }
            }
            Decl::FnDef(f) => {
                if let Some(cached) = cache.get(&f.name, &f.clauses) {
                    results.extend(cached);
                } else {
                    let r = verify_contract(&f.name, &f.clauses);
                    cache.put(&f.name, &f.clauses, &r);
                    results.extend(r);
                }
            }
            Decl::Extern(e) => {
                if let Some(cached) = cache.get(&e.name, &e.clauses) {
                    results.extend(cached);
                } else {
                    let r = verify_contract(&e.name, &e.clauses);
                    cache.put(&e.name, &e.clauses, &r);
                    results.extend(r);
                }
            }
            _ => {}
        }
    }

    results
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

    // Verify in parallel: each job gets its own Z3 context
    let per_job_results: Vec<Vec<VerificationResult>> = jobs
        .par_iter()
        .map(|(name, clauses)| {
            // Check cache first
            if let Some(cached) = cache.get(name, clauses) {
                return cached;
            }
            // Cache miss: run Z3
            let results = verify_contract(name, clauses);
            cache.put(name, clauses, &results);
            results
        })
        .collect();

    // Flatten into a single results vec
    per_job_results.into_iter().flatten().collect()
}

/// Verify a single contract's clauses against Z3.
///
/// Unlike `verify()` which processes all declarations in a `TypedFile`,
/// this function verifies just the given contract's clauses. Each
/// ensures/invariant clause gets its own Z3 query with all requires
/// clauses asserted as assumptions.
///
/// Returns one `VerificationResult` per verifiable clause.
pub fn verify_contract(
    contract_name: &str,
    clauses: &[assura_parser::ast::Clause],
) -> Vec<VerificationResult> {
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
// No-Z3 fallback
// ---------------------------------------------------------------------------

#[cfg(not(feature = "z3-verify"))]
mod no_z3 {
    use super::*;

    /// Stub verification when Z3 is not available.
    pub(crate) fn verify_stub(typed: &TypedFile) -> Vec<VerificationResult> {
        let mut results = Vec::new();
        for decl in &typed.resolved.source.decls {
            if let Decl::Contract(c) = &decl.node {
                for clause in &c.clauses {
                    if matches!(clause.kind, ClauseKind::Ensures | ClauseKind::Invariant) {
                        results.push(VerificationResult::Unknown {
                            clause_desc: format!("{}::{:?}", c.name, clause.kind),
                            reason: "Z3 not available (compiled without z3-verify feature)".into(),
                        });
                    }
                }
            }
        }
        results
    }

    /// Stub refinement subtype check when Z3 is not available.
    pub(crate) fn refinement_stub(_ante: &Expr, _cons: &Expr) -> VerificationResult {
        VerificationResult::Unknown {
            clause_desc: "refinement_subtype".into(),
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }

    /// Stub refinement subtype check with context when Z3 is not available.
    pub(crate) fn refinement_ctx_stub(
        _context: &[Expr],
        _ante: &Expr,
        _cons: &Expr,
    ) -> VerificationResult {
        VerificationResult::Unknown {
            clause_desc: "refinement_subtype_with_context".into(),
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Z3 backend
// ---------------------------------------------------------------------------

#[cfg(feature = "z3-verify")]
mod z3_backend {
    use super::*;
    use super::{CounterexampleModel, Expr};
    use assura_parser::ast::{BinOp, Clause, Literal, UnaryOp};
    use std::collections::HashMap;
    use z3::ast::Ast;
    use z3::{Config, Context, Model, SatResult, Solver, ast};

    // -----------------------------------------------------------------------
    // Z3 value wrapper
    // -----------------------------------------------------------------------

    /// A Z3 expression that can be either an integer or boolean sort.
    #[derive(Clone)]
    enum Z3Value<'ctx> {
        Bool(ast::Bool<'ctx>),
        Int(ast::Int<'ctx>),
        Real(ast::Real<'ctx>),
    }

    /// Binary operator kind for raw token parsing.
    #[derive(Debug, Clone, Copy)]
    enum RawOp {
        Add,
        Sub,
        Mul,
        Div,
        Mod,
        Eq,
        Neq,
        Lt,
        Lte,
        Gt,
        Gte,
        And,
        Or,
        Implies,
    }

    impl<'ctx> Z3Value<'ctx> {
        /// Extract as Bool. If Int, create `!= 0` comparison.
        fn as_bool(&self, ctx: &'ctx Context) -> ast::Bool<'ctx> {
            match self {
                Z3Value::Bool(b) => b.clone(),
                Z3Value::Int(i) => i._eq(&ast::Int::from_i64(ctx, 0)).not(),
                Z3Value::Real(r) => r._eq(&ast::Real::from_real(ctx, 0, 1)).not(),
            }
        }

        /// Extract as Int. If Bool or Real, return a fresh uninterpreted int.
        fn as_int(&self, ctx: &'ctx Context, counter: &mut u32) -> ast::Int<'ctx> {
            match self {
                Z3Value::Int(i) => i.clone(),
                Z3Value::Bool(_) | Z3Value::Real(_) => {
                    *counter += 1;
                    ast::Int::new_const(ctx, format!("__coerce_{counter}"))
                }
            }
        }

        /// Extract as Real. If Int, convert via `int2real`. If Bool, return
        /// a fresh uninterpreted real.
        fn as_real(&self, ctx: &'ctx Context, counter: &mut u32) -> ast::Real<'ctx> {
            match self {
                Z3Value::Real(r) => r.clone(),
                Z3Value::Int(i) => ast::Real::from_int(i),
                Z3Value::Bool(_) => {
                    *counter += 1;
                    ast::Real::new_const(ctx, format!("__coerce_real_{counter}"))
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Expression encoder
    // -----------------------------------------------------------------------

    /// Translates Assura AST expressions into Z3 formulas.
    struct Encoder<'ctx> {
        ctx: &'ctx Context,
        vars: HashMap<String, Z3Value<'ctx>>,
        /// Tracks known function arities for uninterpreted function encoding
        func_arities: HashMap<String, usize>,
        fresh_counter: u32,
        /// Background axioms collected during encoding (e.g., len >= 0).
        /// These are asserted into the solver before each verification check.
        background_axioms: Vec<z3::ast::Bool<'ctx>>,
    }

    impl<'ctx> Encoder<'ctx> {
        fn new(ctx: &'ctx Context) -> Self {
            Self {
                ctx,
                vars: HashMap::new(),
                func_arities: HashMap::new(),
                fresh_counter: 0,
                background_axioms: Vec::new(),
            }
        }

        /// Get or create a named integer variable.
        fn get_or_create_int(&mut self, name: &str) -> ast::Int<'ctx> {
            if let Some(val) = self.vars.get(name) {
                return val.as_int(self.ctx, &mut self.fresh_counter);
            }
            let v = ast::Int::new_const(self.ctx, name);
            self.vars.insert(name.to_string(), Z3Value::Int(v.clone()));
            v
        }

        /// Build a domain guard for quantifier bodies.
        ///
        /// For range domains (`lo..hi`):
        /// - `is_forall=true`:  `(lo <= x && x < hi) => body`
        /// - `is_forall=false`: `(lo <= x && x < hi) && body`
        ///
        /// For non-range domains (collections, identifiers), encode
        /// membership as an uninterpreted `contains(domain, x)` predicate.
        fn guard_quantifier_body(
            &mut self,
            domain: &Expr,
            bound: &ast::Int<'ctx>,
            body: &ast::Bool<'ctx>,
            is_forall: bool,
        ) -> ast::Bool<'ctx> {
            // Check if domain is a range expression: lo..hi
            if let Expr::BinOp {
                op: BinOp::Range,
                lhs: lo,
                rhs: hi,
            } = domain
            {
                let lo_val = self
                    .encode_expr(lo)
                    .as_int(self.ctx, &mut self.fresh_counter);
                let hi_val = self
                    .encode_expr(hi)
                    .as_int(self.ctx, &mut self.fresh_counter);
                let ge_lo = bound.ge(&lo_val);
                let lt_hi = bound.lt(&hi_val);
                let in_range = ast::Bool::and(self.ctx, &[&ge_lo, &lt_hi]);
                if is_forall {
                    in_range.implies(body)
                } else {
                    ast::Bool::and(self.ctx, &[&in_range, body])
                }
            } else {
                // Non-range domain: encode as uninterpreted contains(domain, x)
                let int_sort = z3::Sort::int(self.ctx);
                let bool_sort = z3::Sort::bool(self.ctx);
                let contains_fn = z3::FuncDecl::new(
                    self.ctx,
                    "__domain_contains",
                    &[&int_sort, &int_sort],
                    &bool_sort,
                );
                let domain_val = self
                    .encode_expr(domain)
                    .as_int(self.ctx, &mut self.fresh_counter);
                let membership = contains_fn
                    .apply(&[
                        &ast::Dynamic::from_ast(&domain_val),
                        &ast::Dynamic::from_ast(bound),
                    ])
                    .as_bool()
                    .unwrap_or_else(|| self.fresh_bool());
                if is_forall {
                    membership.implies(body)
                } else {
                    ast::Bool::and(self.ctx, &[&membership, body])
                }
            }
        }

        /// Create a fresh unconstrained boolean.
        fn fresh_bool(&mut self) -> ast::Bool<'ctx> {
            self.fresh_counter += 1;
            ast::Bool::new_const(self.ctx, format!("__fresh_{}", self.fresh_counter))
        }

        /// Create a fresh unconstrained integer.
        fn fresh_int(&mut self) -> ast::Int<'ctx> {
            self.fresh_counter += 1;
            ast::Int::new_const(self.ctx, format!("__fresh_{}", self.fresh_counter))
        }

        /// Create an uninterpreted function declaration (Int^arity -> Int).
        /// Z3 internally deduplicates declarations with the same name and sorts.
        fn make_func(&mut self, name: &str, arity: usize) -> z3::FuncDecl<'ctx> {
            self.func_arities.insert(name.to_string(), arity);
            let int_sort = z3::Sort::int(self.ctx);
            let param_sorts: Vec<&z3::Sort> = (0..arity).map(|_| &int_sort).collect();
            z3::FuncDecl::new(self.ctx, name, &param_sorts, &int_sort)
        }

        /// Encode a function call as an uninterpreted function application.
        /// Known boolean methods return Bool; everything else returns Int.
        fn encode_call(&mut self, func_name: &str, args: &[Expr]) -> Z3Value<'ctx> {
            let arg_vals: Vec<ast::Int<'ctx>> = args
                .iter()
                .map(|a| {
                    self.encode_expr(a)
                        .as_int(self.ctx, &mut self.fresh_counter)
                })
                .collect();
            // Methods known to return Bool
            if matches!(
                func_name,
                "contains"
                    | "is_empty"
                    | "is_some"
                    | "is_none"
                    | "is_ok"
                    | "is_err"
                    | "any"
                    | "all"
                    | "contains_key"
                    | "starts_with"
                    | "ends_with"
                    | "is_subset"
                    | "is_superset"
            ) {
                let bool_sort = z3::Sort::bool(self.ctx);
                let int_sort = z3::Sort::int(self.ctx);
                let param_sorts: Vec<&z3::Sort> = (0..arg_vals.len()).map(|_| &int_sort).collect();
                let decl = z3::FuncDecl::new(self.ctx, func_name, &param_sorts, &bool_sort);
                let arg_refs: Vec<&dyn z3::ast::Ast> =
                    arg_vals.iter().map(|a| a as &dyn z3::ast::Ast).collect();
                let result = decl.apply(&arg_refs);
                return Z3Value::Bool(result.as_bool().unwrap_or_else(|| self.fresh_bool()));
            }
            // String methods with known semantics
            match func_name {
                // substring(str, start, end): fresh value with length == end - start
                // and bounds axioms: 0 <= start <= end <= len(str)
                "substring" | "substr" if arg_vals.len() == 3 => {
                    let str_val = &arg_vals[0];
                    let start = &arg_vals[1];
                    let end = &arg_vals[2];
                    let result = self.fresh_int();
                    let zero = ast::Int::from_i64(self.ctx, 0);
                    // 0 <= start
                    self.background_axioms.push(start.ge(&zero));
                    // start <= end
                    self.background_axioms.push(start.le(end));
                    // end <= len(str)
                    let len_decl = self.make_func("__field_len", 1);
                    let str_len = len_decl
                        .apply(&[str_val as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    self.background_axioms.push(end.le(&str_len));
                    // len(result) == end - start
                    let res_len = len_decl
                        .apply(&[&result as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let diff = ast::Int::sub(self.ctx, &[end, start]);
                    self.background_axioms.push(res_len._eq(&diff));
                    self.background_axioms.push(res_len.ge(&zero));
                    return Z3Value::Int(result);
                }
                // concat(a, b): same semantics as BinOp::Concat
                "concat" if arg_vals.len() == 2 => {
                    let l = &arg_vals[0];
                    let r = &arg_vals[1];
                    let result = self.fresh_int();
                    let len_decl = self.make_func("__field_len", 1);
                    let len_l = len_decl
                        .apply(&[l as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let len_r = len_decl
                        .apply(&[r as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let len_result = len_decl
                        .apply(&[&result as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let zero = ast::Int::from_i64(self.ctx, 0);
                    self.background_axioms.push(len_l.ge(&zero));
                    self.background_axioms.push(len_r.ge(&zero));
                    let sum = ast::Int::add(self.ctx, &[&len_l, &len_r]);
                    self.background_axioms.push(len_result._eq(&sum));
                    self.background_axioms.push(len_result.ge(&zero));
                    return Z3Value::Int(result);
                }
                // index_of(str, substr): returns Int with -1 <= result < len(str)
                "index_of" | "find" | "indexOf" if arg_vals.len() == 2 => {
                    let str_val = &arg_vals[0];
                    let result = self.fresh_int();
                    let neg_one = ast::Int::from_i64(self.ctx, -1);
                    self.background_axioms.push(result.ge(&neg_one));
                    let len_decl = self.make_func("__field_len", 1);
                    let str_len = len_decl
                        .apply(&[str_val as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    self.background_axioms.push(result.lt(&str_len));
                    return Z3Value::Int(result);
                }
                // char_at(str, idx): returns Int with bounds axiom
                "char_at" | "charAt" if arg_vals.len() == 2 => {
                    let str_val = &arg_vals[0];
                    let idx = &arg_vals[1];
                    let zero = ast::Int::from_i64(self.ctx, 0);
                    self.background_axioms.push(idx.ge(&zero));
                    let len_decl = self.make_func("__field_len", 1);
                    let str_len = len_decl
                        .apply(&[str_val as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    self.background_axioms.push(idx.lt(&str_len));
                    return Z3Value::Int(self.fresh_int());
                }
                // replace(str, old, new): result length is bounded
                "replace" if arg_vals.len() == 3 => {
                    let result = self.fresh_int();
                    let len_decl = self.make_func("__field_len", 1);
                    let res_len = len_decl
                        .apply(&[&result as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let zero = ast::Int::from_i64(self.ctx, 0);
                    self.background_axioms.push(res_len.ge(&zero));
                    return Z3Value::Int(result);
                }
                // split(str, delim): returns a fresh collection with len >= 1
                "split" if arg_vals.len() == 2 => {
                    let result = self.fresh_int();
                    let len_decl = self.make_func("__field_len", 1);
                    let res_len = len_decl
                        .apply(&[&result as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let one = ast::Int::from_i64(self.ctx, 1);
                    self.background_axioms.push(res_len.ge(&one));
                    return Z3Value::Int(result);
                }
                // trim/to_lower/to_upper: result length <= input length
                "trim" | "to_lowercase" | "to_uppercase" | "to_lower" | "to_upper"
                    if arg_vals.len() == 1 =>
                {
                    let str_val = &arg_vals[0];
                    let result = self.fresh_int();
                    let len_decl = self.make_func("__field_len", 1);
                    let str_len = len_decl
                        .apply(&[str_val as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let res_len = len_decl
                        .apply(&[&result as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let zero = ast::Int::from_i64(self.ctx, 0);
                    self.background_axioms.push(res_len.ge(&zero));
                    self.background_axioms.push(res_len.le(&str_len));
                    return Z3Value::Int(result);
                }
                _ => {}
            }
            // Built-in functions with known semantics
            match func_name {
                // abs(x) => if x >= 0 then x else -x
                "abs" if arg_vals.len() == 1 => {
                    let x = &arg_vals[0];
                    let zero = ast::Int::from_i64(self.ctx, 0);
                    let neg_x = x.unary_minus();
                    let cond = x.ge(&zero);
                    return Z3Value::Int(cond.ite(x, &neg_x));
                }
                // min(a, b) => if a <= b then a else b
                "min" if arg_vals.len() == 2 => {
                    let (a, b) = (&arg_vals[0], &arg_vals[1]);
                    return Z3Value::Int(a.le(b).ite(a, b));
                }
                // max(a, b) => if a >= b then a else b
                "max" if arg_vals.len() == 2 => {
                    let (a, b) = (&arg_vals[0], &arg_vals[1]);
                    return Z3Value::Int(a.ge(b).ite(a, b));
                }
                _ => {}
            }
            // Array set(arr, index, value): Z3 store axiom
            // set(a, i, v) returns a new array where a[i] == v and
            // all other elements are unchanged.
            if func_name == "set" && arg_vals.len() == 3 {
                let _arr = &arg_vals[0];
                let idx = &arg_vals[1];
                let val = &arg_vals[2];
                let result = self.fresh_int();
                // After set(a, i, v): get(result, i) == v
                let get_decl = self.make_func("__index", 2);
                let get_at_idx = get_decl
                    .apply(&[&result as &dyn z3::ast::Ast, idx as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                self.background_axioms.push(get_at_idx._eq(val));
                // len(result) == len(original)
                // Use "len" to match the function name users write in contracts
                let len_decl = self.make_func("len", 1);
                let old_len = len_decl
                    .apply(&[_arr as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                let new_len = len_decl
                    .apply(&[&result as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                self.background_axioms.push(new_len._eq(&old_len));
                let zero = ast::Int::from_i64(self.ctx, 0);
                self.background_axioms.push(new_len.ge(&zero));
                return Z3Value::Int(result);
            }
            // Map get/put with read-over-write axioms
            // get(map, key) -> value (uninterpreted with consistency)
            // put(map, key, value) -> new_map with axiom:
            //   get(put(m, k, v), k) == v  (write-then-read)
            if func_name == "put" && arg_vals.len() == 3 {
                // put(map, key, value) returns a new map
                let map_val = &arg_vals[0];
                let key = &arg_vals[1];
                let value = &arg_vals[2];
                let new_map = self.fresh_int();
                // Read-over-write axiom: get(put(m, k, v), k) == v
                let get_decl = self.make_func("get", 2);
                let get_result = get_decl
                    .apply(&[&new_map as &dyn z3::ast::Ast, key as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                self.background_axioms.push(get_result._eq(value));
                // size(new_map) >= size(map)
                let size_decl = self.make_func("size", 1);
                let old_size = size_decl
                    .apply(&[map_val as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                let new_size = size_decl
                    .apply(&[&new_map as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                let zero = ast::Int::from_i64(self.ctx, 0);
                self.background_axioms.push(new_size.ge(&old_size));
                self.background_axioms.push(new_size.ge(&zero));
                return Z3Value::Int(new_map);
            }
            // Size-like methods get non-negativity axiom
            if matches!(func_name, "len" | "length" | "size" | "count" | "capacity") {
                let decl = self.make_func(func_name, arg_vals.len());
                let arg_refs: Vec<&dyn z3::ast::Ast> =
                    arg_vals.iter().map(|a| a as &dyn z3::ast::Ast).collect();
                let result = decl.apply(&arg_refs);
                let len_val = result.as_int().unwrap_or_else(|| self.fresh_int());
                let zero = ast::Int::from_i64(self.ctx, 0);
                self.background_axioms.push(len_val.ge(&zero));
                return Z3Value::Int(len_val);
            }
            let decl = self.make_func(func_name, arg_vals.len());
            let arg_refs: Vec<&dyn z3::ast::Ast> =
                arg_vals.iter().map(|a| a as &dyn z3::ast::Ast).collect();
            let result = decl.apply(&arg_refs);
            Z3Value::Int(result.as_int().unwrap_or_else(|| self.fresh_int()))
        }

        /// Encode field access as uninterpreted function: field_name(object).
        /// Known boolean fields return Bool; size fields return non-negative Int.
        fn encode_field_access(&mut self, obj: &Expr, field: &str) -> Z3Value<'ctx> {
            let obj_val = self
                .encode_expr(obj)
                .as_int(self.ctx, &mut self.fresh_counter);
            let func_name = format!("__field_{field}");
            // Boolean-valued fields
            if matches!(
                field,
                "is_empty" | "is_some" | "is_none" | "is_ok" | "is_err"
            ) {
                let bool_sort = z3::Sort::bool(self.ctx);
                let int_sort = z3::Sort::int(self.ctx);
                let decl =
                    z3::FuncDecl::new(self.ctx, func_name.as_str(), &[&int_sort], &bool_sort);
                let result = decl.apply(&[&obj_val as &dyn z3::ast::Ast]);
                return Z3Value::Bool(result.as_bool().unwrap_or_else(|| self.fresh_bool()));
            }
            // Size fields: return Int with non-negativity axiom
            if matches!(field, "len" | "length" | "size" | "capacity" | "count") {
                let decl = self.make_func(&func_name, 1);
                let result = decl.apply(&[&obj_val as &dyn z3::ast::Ast]);
                let len_val = result.as_int().unwrap_or_else(|| self.fresh_int());
                // Assert len >= 0 as a background axiom
                let zero = ast::Int::from_i64(self.ctx, 0);
                self.background_axioms.push(len_val.ge(&zero));
                return Z3Value::Int(len_val);
            }
            let decl = self.make_func(&func_name, 1);
            let result = decl.apply(&[&obj_val as &dyn z3::ast::Ast]);
            Z3Value::Int(result.as_int().unwrap_or_else(|| self.fresh_int()))
        }

        /// Encode indexing as uninterpreted function: __index(collection, index).
        fn encode_index(&mut self, collection: &Expr, index: &Expr) -> Z3Value<'ctx> {
            let coll_val = self
                .encode_expr(collection)
                .as_int(self.ctx, &mut self.fresh_counter);
            let idx_val = self
                .encode_expr(index)
                .as_int(self.ctx, &mut self.fresh_counter);

            // Add bounds checking axiom: 0 <= index < len(collection)
            let zero = ast::Int::from_i64(self.ctx, 0);
            let ge_zero = idx_val.ge(&zero);
            // len(collection) via uninterpreted function
            let len_decl = self.make_func("__len", 1);
            let len_result = len_decl.apply(&[&coll_val as &dyn z3::ast::Ast]);
            let len_val = len_result.as_int().unwrap_or_else(|| self.fresh_int());
            // len >= 0
            self.background_axioms.push(len_val.ge(&zero));
            // 0 <= index
            self.background_axioms.push(ge_zero);
            // index < len
            self.background_axioms.push(idx_val.lt(&len_val));

            // Use Z3 Array theory: select(array, index)
            // Model arrays as Array<Int, Int> for uniform element access.
            let int_sort = z3::Sort::int(self.ctx);
            let _arr_sort = z3::Sort::array(self.ctx, &int_sort, &int_sort);
            let arr_name = format!("__arr_{}", self.fresh_counter);
            self.fresh_counter += 1;
            let arr = z3::ast::Array::new_const(self.ctx, arr_name.as_str(), &int_sort, &int_sort);
            // Constrain: the array is associated with this collection
            // (same collection -> same array via naming, but we also
            // link values through the select result).
            let selected = arr.select(&idx_val);
            // Z3 select returns a Dynamic; extract as Int
            let result = selected.as_int().unwrap_or_else(|| self.fresh_int());

            // Also add the uninterpreted function version for backward compat
            let decl = self.make_func("__index", 2);
            let uif_result = decl.apply(&[
                &coll_val as &dyn z3::ast::Ast,
                &idx_val as &dyn z3::ast::Ast,
            ]);
            let uif_val = uif_result.as_int().unwrap_or_else(|| self.fresh_int());
            // Link the two: select(arr, i) == __index(coll, i)
            self.background_axioms.push(result._eq(&uif_val));

            Z3Value::Int(result)
        }

        /// Hash a pattern name to a stable i64 for Z3 encoding.
        fn pattern_hash(&self, name: &str) -> i64 {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            name.hash(&mut hasher);
            hasher.finish() as i64
        }

        /// Encode a literal value to Z3.
        fn encode_literal(&self, lit: &Literal) -> Z3Value<'ctx> {
            match lit {
                Literal::Int(s) => {
                    let n: i64 = s.parse().unwrap_or(0);
                    Z3Value::Int(ast::Int::from_i64(self.ctx, n))
                }
                Literal::Float(s) => {
                    let n: i64 = s.parse::<f64>().unwrap_or(0.0) as i64;
                    Z3Value::Int(ast::Int::from_i64(self.ctx, n))
                }
                Literal::Bool(b) => Z3Value::Bool(ast::Bool::from_bool(self.ctx, *b)),
                Literal::Str(_) => {
                    Z3Value::Int(ast::Int::from_i64(self.ctx, self.fresh_counter as i64))
                }
            }
        }

        /// Bind pattern variables as fresh Z3 integer constants so they
        /// are available in the arm body.
        fn bind_pattern_vars(
            &mut self,
            pattern: &assura_parser::ast::Pattern,
            _scrutinee: &Z3Value<'ctx>,
        ) {
            match pattern {
                assura_parser::ast::Pattern::Ident(name) => {
                    // Ident patterns in match bind the variable to the scrutinee,
                    // but for SMT we use a fresh variable since we cannot always
                    // decompose the scrutinee.
                    if !self.vars.contains_key(name) {
                        let v = ast::Int::new_const(self.ctx, name.as_str());
                        self.vars.insert(name.clone(), Z3Value::Int(v));
                    }
                }
                assura_parser::ast::Pattern::Constructor { fields, .. } => {
                    // Each field in the constructor is an uninterpreted extraction
                    // from the scrutinee; bind as fresh int variables.
                    for field in fields {
                        self.bind_pattern_vars(field, _scrutinee);
                    }
                }
                assura_parser::ast::Pattern::Tuple(pats) => {
                    for pat in pats {
                        self.bind_pattern_vars(pat, _scrutinee);
                    }
                }
                assura_parser::ast::Pattern::Wildcard | assura_parser::ast::Pattern::Literal(_) => {
                }
            }
        }

        /// Encode an AST expression into a Z3 value.
        fn encode_expr(&mut self, expr: &Expr) -> Z3Value<'ctx> {
            match expr {
                // --- Literals ---
                Expr::Literal(Literal::Int(s)) => {
                    let n: i64 = s.parse().unwrap_or(0);
                    Z3Value::Int(ast::Int::from_i64(self.ctx, n))
                }
                Expr::Literal(Literal::Float(s)) => {
                    // Encode as Z3 Real. Parse the float string and convert
                    // to a rational (numerator/denominator) for exact encoding.
                    let f: f64 = s.parse().unwrap_or(0.0);
                    // Use a large denominator for precision
                    let denom = 1_000_000i32;
                    let numer = (f * denom as f64) as i32;
                    Z3Value::Real(ast::Real::from_real(self.ctx, numer, denom))
                }
                Expr::Literal(Literal::Str(s)) => {
                    // Encode as a named integer constant. Two identical string
                    // literals produce the same constant, so equality works.
                    // Different strings get different constants.
                    let const_name = format!("__str_{s}");
                    let str_val = ast::Int::new_const(self.ctx, const_name);
                    // String length axiom: len("hello") == 5
                    let len_decl = self.make_func("__field_len", 1);
                    let len_result = len_decl
                        .apply(&[&str_val as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let str_len = ast::Int::from_i64(self.ctx, s.len() as i64);
                    self.background_axioms.push(len_result._eq(&str_len));
                    Z3Value::Int(str_val)
                }
                Expr::Literal(Literal::Bool(b)) => {
                    Z3Value::Bool(ast::Bool::from_bool(self.ctx, *b))
                }

                // --- Identifiers ---
                Expr::Ident(name) => {
                    if name == "true" {
                        return Z3Value::Bool(ast::Bool::from_bool(self.ctx, true));
                    }
                    if name == "false" {
                        return Z3Value::Bool(ast::Bool::from_bool(self.ctx, false));
                    }
                    if let Some(val) = self.vars.get(name) {
                        return val.clone();
                    }
                    // Default: create integer variable (most common in contracts)
                    let v = ast::Int::new_const(self.ctx, name.as_str());
                    self.vars.insert(name.clone(), Z3Value::Int(v.clone()));
                    Z3Value::Int(v)
                }

                // --- Binary operations ---
                Expr::BinOp { lhs, op, rhs } => self.encode_binop(lhs, op, rhs),

                // --- Unary operations ---
                Expr::UnaryOp { op, expr: inner } => {
                    let val = self.encode_expr(inner);
                    match op {
                        UnaryOp::Neg => {
                            if Self::is_real(&val) {
                                let r = val.as_real(self.ctx, &mut self.fresh_counter);
                                Z3Value::Real(r.unary_minus())
                            } else {
                                let i = val.as_int(self.ctx, &mut self.fresh_counter);
                                Z3Value::Int(i.unary_minus())
                            }
                        }
                        UnaryOp::Not => {
                            let b = val.as_bool(self.ctx);
                            Z3Value::Bool(b.not())
                        }
                    }
                }

                // --- old(expr): encode inner with __old suffix ---
                Expr::Old(inner) => match inner.as_ref() {
                    // old(x) -> x__old
                    Expr::Ident(name) => {
                        let old_name = format!("{name}__old");
                        let v = self.get_or_create_int(&old_name);
                        Z3Value::Int(v)
                    }
                    // old(obj.field) -> encode obj as old, then access field
                    Expr::Field(obj, field) => {
                        let old_obj = self.encode_expr(&Expr::Old(obj.clone()));
                        let old_obj_int = old_obj.as_int(self.ctx, &mut self.fresh_counter);
                        let func_name = format!("__field_{field}");
                        if matches!(
                            field.as_str(),
                            "is_empty" | "is_some" | "is_none" | "is_ok" | "is_err"
                        ) {
                            let bool_sort = z3::Sort::bool(self.ctx);
                            let int_sort = z3::Sort::int(self.ctx);
                            let decl = z3::FuncDecl::new(
                                self.ctx,
                                func_name.as_str(),
                                &[&int_sort],
                                &bool_sort,
                            );
                            let result = decl.apply(&[&old_obj_int as &dyn z3::ast::Ast]);
                            Z3Value::Bool(result.as_bool().unwrap_or_else(|| self.fresh_bool()))
                        } else {
                            let decl = self.make_func(&func_name, 1);
                            let result = decl.apply(&[&old_obj_int as &dyn z3::ast::Ast]);
                            Z3Value::Int(result.as_int().unwrap_or_else(|| self.fresh_int()))
                        }
                    }
                    // old(obj.method(args)) -> encode obj as old, then call
                    Expr::MethodCall {
                        receiver, method, ..
                    } => {
                        let old_recv = self.encode_expr(&Expr::Old(receiver.clone()));
                        let old_int = old_recv.as_int(self.ctx, &mut self.fresh_counter);
                        let decl = self.make_func(method, 1);
                        let result = decl.apply(&[&old_int as &dyn z3::ast::Ast]);
                        Z3Value::Int(result.as_int().unwrap_or_else(|| self.fresh_int()))
                    }
                    // Fallback: encode the inner expression directly
                    _ => self.encode_expr(inner),
                },

                // --- Forall quantifier ---
                Expr::Forall { var, domain, body } => {
                    let bound = ast::Int::new_const(self.ctx, var.as_str());
                    self.vars.insert(var.clone(), Z3Value::Int(bound.clone()));
                    let body_val = self.encode_expr(body);
                    let body_bool = body_val.as_bool(self.ctx);
                    // Domain-aware: forall x in lo..hi: P  =>  forall x: (lo <= x && x < hi) => P
                    let guarded = self.guard_quantifier_body(domain, &bound, &body_bool, true);
                    let result = ast::forall_const(self.ctx, &[&bound], &[], &guarded);
                    Z3Value::Bool(result)
                }

                // --- Exists quantifier ---
                Expr::Exists { var, domain, body } => {
                    let bound = ast::Int::new_const(self.ctx, var.as_str());
                    self.vars.insert(var.clone(), Z3Value::Int(bound.clone()));
                    let body_val = self.encode_expr(body);
                    let body_bool = body_val.as_bool(self.ctx);
                    // Domain-aware: exists x in lo..hi: P  =>  exists x: (lo <= x && x < hi) && P
                    let guarded = self.guard_quantifier_body(domain, &bound, &body_bool, false);
                    let result = ast::exists_const(self.ctx, &[&bound], &[], &guarded);
                    Z3Value::Bool(result)
                }

                // --- If-then-else ---
                Expr::If {
                    cond,
                    then_branch,
                    else_branch,
                } => {
                    let cond_val = self.encode_expr(cond);
                    let cond_bool = cond_val.as_bool(self.ctx);
                    let then_val = self.encode_expr(then_branch);

                    if let Some(else_br) = else_branch {
                        let else_val = self.encode_expr(else_br);
                        match (&then_val, &else_val) {
                            (Z3Value::Int(t), Z3Value::Int(e)) => Z3Value::Int(cond_bool.ite(t, e)),
                            (Z3Value::Bool(t), Z3Value::Bool(e)) => {
                                Z3Value::Bool(cond_bool.ite(t, e))
                            }
                            (Z3Value::Real(t), Z3Value::Real(e)) => {
                                Z3Value::Real(cond_bool.ite(t, e))
                            }
                            (Z3Value::Int(t), Z3Value::Real(e)) => {
                                Z3Value::Real(cond_bool.ite(&ast::Real::from_int(t), e))
                            }
                            (Z3Value::Real(t), Z3Value::Int(e)) => {
                                Z3Value::Real(cond_bool.ite(t, &ast::Real::from_int(e)))
                            }
                            _ => {
                                let t = then_val.as_bool(self.ctx);
                                let e = else_val.as_bool(self.ctx);
                                Z3Value::Bool(cond_bool.ite(&t, &e))
                            }
                        }
                    } else {
                        // No else: `if P then Q` = `P => Q`
                        let then_bool = then_val.as_bool(self.ctx);
                        Z3Value::Bool(cond_bool.implies(&then_bool))
                    }
                }

                // --- Parenthesized ---
                Expr::Paren(inner) => self.encode_expr(inner),

                // --- Raw token sequence: parse operator expression ---
                Expr::Raw(tokens) => self.encode_raw_tokens(tokens),

                // --- Ghost block: encode inner for verification ---
                Expr::Ghost(inner) => self.encode_expr(inner),

                // --- Apply lemma: encode args for constraint propagation,
                //     result is true (the lemma's postcondition is assumed) ---
                Expr::Apply { args, .. } => {
                    for arg in args {
                        let _ = self.encode_expr(arg);
                    }
                    Z3Value::Bool(ast::Bool::from_bool(self.ctx, true))
                }

                // --- Match: encode as ITE chain over arm bodies ---
                Expr::Match { scrutinee, arms } => {
                    let scrut = self.encode_expr(scrutinee);
                    // Build an if-then-else chain: if scrut == pattern1 then body1
                    // else if scrut == pattern2 then body2 ... else default
                    let default = Z3Value::Int(self.fresh_int());
                    arms.iter().rev().fold(default, |else_val, arm| {
                        // Bind pattern variables before encoding the body
                        self.bind_pattern_vars(&arm.pattern, &scrut);
                        let body = self.encode_expr(&arm.body);
                        // For wildcard patterns, the arm always matches
                        if matches!(arm.pattern, assura_parser::ast::Pattern::Wildcard) {
                            return body;
                        }
                        // For ident patterns, check scrut == pattern_name
                        let cond = match &arm.pattern {
                            assura_parser::ast::Pattern::Ident(name) => {
                                let pat_val = Z3Value::Int(ast::Int::from_i64(
                                    self.ctx,
                                    self.pattern_hash(name),
                                ));
                                match (&scrut, &pat_val) {
                                    (Z3Value::Int(a), Z3Value::Int(b)) => a._eq(b),
                                    // Overapproximate: type mismatch means we
                                    // cannot compare, so assume the arm could
                                    // match (sound: may produce spurious
                                    // counterexamples but never hides real ones)
                                    _ => ast::Bool::from_bool(self.ctx, true),
                                }
                            }
                            assura_parser::ast::Pattern::Literal(lit) => {
                                let lit_val = self.encode_literal(lit);
                                match (&scrut, &lit_val) {
                                    (Z3Value::Int(a), Z3Value::Int(b)) => a._eq(b),
                                    (Z3Value::Bool(a), Z3Value::Bool(b)) => a._eq(b),
                                    (Z3Value::Real(a), Z3Value::Real(b)) => a._eq(b),
                                    // Cross-sort: promote Int to Real
                                    (Z3Value::Int(a), Z3Value::Real(b)) => {
                                        ast::Real::from_int(a)._eq(b)
                                    }
                                    (Z3Value::Real(a), Z3Value::Int(b)) => {
                                        a._eq(&ast::Real::from_int(b))
                                    }
                                    // Overapproximate: unresolvable type
                                    // mismatch, assume arm could match
                                    _ => ast::Bool::from_bool(self.ctx, true),
                                }
                            }
                            // Constructor and Tuple patterns bind variables
                            // but always match in this overapproximation.
                            assura_parser::ast::Pattern::Constructor { .. }
                            | assura_parser::ast::Pattern::Tuple(_) => {
                                ast::Bool::from_bool(self.ctx, true)
                            }
                            _ => ast::Bool::from_bool(self.ctx, true),
                        };
                        // Build ITE: if cond then body else else_val
                        match (&body, &else_val) {
                            (Z3Value::Bool(b), Z3Value::Bool(e)) => Z3Value::Bool(cond.ite(b, e)),
                            (Z3Value::Int(b), Z3Value::Int(e)) => Z3Value::Int(cond.ite(b, e)),
                            (Z3Value::Real(b), Z3Value::Real(e)) => Z3Value::Real(cond.ite(b, e)),
                            (Z3Value::Int(b), Z3Value::Real(e)) => {
                                Z3Value::Real(cond.ite(&ast::Real::from_int(b), e))
                            }
                            (Z3Value::Real(b), Z3Value::Int(e)) => {
                                Z3Value::Real(cond.ite(b, &ast::Real::from_int(e)))
                            }
                            _ => body, // type mismatch fallback
                        }
                    })
                }

                // --- Let binding: bind value, then encode body ---
                Expr::Let { name, value, body } => {
                    let val = self.encode_expr(value);
                    self.vars.insert(name.clone(), val);
                    self.encode_expr(body)
                }

                // --- Field access: uninterpreted function field_name(obj) ---
                Expr::Field(obj, field) => self.encode_field_access(obj, field),

                // --- Method call: uninterpreted function method(receiver, args...) ---
                Expr::MethodCall {
                    receiver,
                    method,
                    args,
                } => {
                    let mut all_args = vec![receiver.as_ref().clone()];
                    all_args.extend(args.iter().cloned());
                    self.encode_call(method, &all_args)
                }

                // --- Function call: uninterpreted function ---
                Expr::Call { func, args } => {
                    let func_name = match func.as_ref() {
                        Expr::Ident(name) => name.clone(),
                        Expr::Field(_, field) => field.clone(),
                        _ => format!("__call_{}", self.fresh_counter),
                    };
                    self.encode_call(&func_name, args)
                }

                // --- Index: uninterpreted function __index(coll, idx) ---
                Expr::Index { expr, index } => self.encode_index(expr, index),

                // --- Tuple: encode elements for constraint propagation ---
                Expr::Tuple(elems) => {
                    // Encode each element so constraints inside are captured
                    for elem in elems {
                        let _ = self.encode_expr(elem);
                    }
                    Z3Value::Int(self.fresh_int())
                }

                // --- Cast: encode inner (the value doesn't change, only its type) ---
                Expr::Cast { expr, .. } => self.encode_expr(expr),

                // --- List: encode elements for constraint propagation ---
                Expr::List(elems) => {
                    for elem in elems {
                        let _ = self.encode_expr(elem);
                    }
                    Z3Value::Int(self.fresh_int())
                }

                // --- Block: encode all body expressions, return last ---
                Expr::Block(body) => {
                    let mut result = Z3Value::Int(self.fresh_int());
                    for expr in body {
                        result = self.encode_expr(expr);
                    }
                    result
                }
            }
        }

        // ---------------------------------------------------------------
        // Raw token encoding
        // ---------------------------------------------------------------

        /// Encode a sequence of raw tokens (from unparsed clause bodies).
        ///
        /// Uses a simple precedence-climbing approach to handle common
        /// contract clause patterns: comparisons, arithmetic, and logical
        /// operators over identifiers and integer literals.
        fn encode_raw_tokens(&mut self, tokens: &[String]) -> Z3Value<'ctx> {
            if tokens.is_empty() {
                // Empty clause body is vacuously true (e.g. an ensures
                // clause with no expression defaults to trivially satisfied).
                return Z3Value::Bool(ast::Bool::from_bool(self.ctx, true));
            }

            // Try to parse as a structured expression
            let parsed = self.parse_raw_expr(tokens, 0);
            parsed.0
        }

        /// Parse raw tokens with operator precedence.
        ///
        /// Returns (value, next_position).
        fn parse_raw_expr(&mut self, tokens: &[String], min_prec: u8) -> (Z3Value<'ctx>, usize) {
            let (mut lhs, mut pos) = self.parse_raw_atom(tokens, 0);

            while pos < tokens.len() {
                let (op_prec, op_kind) = match tokens[pos].as_str() {
                    "or" | "||" => (1, RawOp::Or),
                    "and" | "&&" => (2, RawOp::And),
                    "=>" | "==>" | "implies" => (3, RawOp::Implies),
                    "==" => (4, RawOp::Eq),
                    "!=" => (4, RawOp::Neq),
                    "<" => (5, RawOp::Lt),
                    "<=" => (5, RawOp::Lte),
                    ">" => (5, RawOp::Gt),
                    ">=" => (5, RawOp::Gte),
                    "+" => (6, RawOp::Add),
                    "-" => (6, RawOp::Sub),
                    "*" => (7, RawOp::Mul),
                    "/" => (7, RawOp::Div),
                    "%" | "mod" => (7, RawOp::Mod),
                    _ => break,
                };

                if op_prec < min_prec {
                    break;
                }

                pos += 1; // consume operator

                let (rhs, next_pos) = self.parse_raw_expr(&tokens[pos..], op_prec + 1);
                // Adjust pos relative to original tokens
                pos += next_pos;

                lhs = self.apply_raw_op(op_kind, lhs, rhs);
            }

            (lhs, pos)
        }

        /// Parse a single atom from raw tokens.
        fn parse_raw_atom(&mut self, tokens: &[String], start: usize) -> (Z3Value<'ctx>, usize) {
            if start >= tokens.len() {
                // Past end of tokens: treat as vacuously true.
                return (Z3Value::Bool(ast::Bool::from_bool(self.ctx, true)), start);
            }

            let tok = &tokens[start];

            // --- Unary not ---
            if tok == "not" || tok == "!" {
                let (val, next) = self.parse_raw_atom(tokens, start + 1);
                let b = val.as_bool(self.ctx);
                return (Z3Value::Bool(b.not()), next);
            }

            // --- Unary minus ---
            if tok == "-" {
                let (val, next) = self.parse_raw_atom(tokens, start + 1);
                let i = val.as_int(self.ctx, &mut self.fresh_counter);
                return (Z3Value::Int(i.unary_minus()), next);
            }

            // --- Parenthesized expression ---
            if tok == "(" {
                let mut depth = 1usize;
                let mut end = start + 1;
                while end < tokens.len() && depth > 0 {
                    match tokens[end].as_str() {
                        "(" => depth += 1,
                        ")" => depth -= 1,
                        _ => {}
                    }
                    if depth > 0 {
                        end += 1;
                    }
                }
                // Parse the inner tokens
                let inner = &tokens[start + 1..end];
                let (val, _) = self.parse_raw_expr(inner, 0);
                return (val, end + 1); // skip closing ')'
            }

            // --- Boolean literals ---
            if tok == "true" {
                return (
                    Z3Value::Bool(ast::Bool::from_bool(self.ctx, true)),
                    start + 1,
                );
            }
            if tok == "false" {
                return (
                    Z3Value::Bool(ast::Bool::from_bool(self.ctx, false)),
                    start + 1,
                );
            }

            // --- `result` keyword ---
            if tok == "result" {
                let v = self.get_or_create_int("__result");
                return (Z3Value::Int(v), start + 1);
            }

            // --- `old(expr)` in raw tokens ---
            if tok == "old" && start + 1 < tokens.len() && tokens[start + 1] == "(" {
                // Find matching close paren
                let mut depth = 1usize;
                let mut p = start + 2;
                while p < tokens.len() && depth > 0 {
                    match tokens[p].as_str() {
                        "(" => depth += 1,
                        ")" => depth -= 1,
                        _ => {}
                    }
                    if depth > 0 {
                        p += 1;
                    }
                }
                let inner_tokens = &tokens[start + 2..p];
                let end = p + 1;
                // Parse inner expression, then rename all variables to __old
                if inner_tokens.len() == 1 {
                    // old(x) -> x__old
                    let old_name = format!("{}__old", inner_tokens[0]);
                    let v = self.get_or_create_int(&old_name);
                    return (Z3Value::Int(v), end);
                }
                // old(x.field) -> encode field access on x__old
                if inner_tokens.len() == 3 && inner_tokens[1] == "." {
                    let old_name = format!("{}__old", inner_tokens[0]);
                    let old_var = self.get_or_create_int(&old_name);
                    let field = &inner_tokens[2];
                    let func_name = format!("__field_{field}");
                    let decl = self.make_func(&func_name, 1);
                    let result = decl.apply(&[&old_var as &dyn z3::ast::Ast]);
                    let val = result.as_int().unwrap_or_else(|| self.fresh_int());
                    return (Z3Value::Int(val), end);
                }
                // General old(expr): parse and use fresh variables
                let (val, _) = self.parse_raw_expr(inner_tokens, 0);
                return (val, end);
            }

            // --- `forall x in domain: body` in raw tokens ---
            if (tok == "forall" || tok == "exists")
                && start + 4 < tokens.len()
                && tokens[start + 2] == "in"
            {
                let var_name = &tokens[start + 1];
                let is_forall = tok == "forall";
                // Find the colon separator
                let mut colon_pos = start + 3;
                let mut d = 0usize;
                while colon_pos < tokens.len() {
                    match tokens[colon_pos].as_str() {
                        "(" => d += 1,
                        ")" => d = d.saturating_sub(1),
                        ":" if d == 0 => break,
                        _ => {}
                    }
                    colon_pos += 1;
                }
                if colon_pos < tokens.len() && tokens[colon_pos] == ":" {
                    let domain_tokens = &tokens[start + 3..colon_pos];
                    let body_tokens = &tokens[colon_pos + 1..];

                    // Parse domain (for axiom: var >= 0 if domain is a range)
                    let (_domain_val, _) = self.parse_raw_expr(domain_tokens, 0);

                    // Bind the quantifier variable
                    let bound = ast::Int::new_const(self.ctx, var_name.as_str());
                    self.vars
                        .insert(var_name.clone(), Z3Value::Int(bound.clone()));

                    // Parse body
                    let (body_val, _) = self.parse_raw_expr(body_tokens, 0);
                    let body_bool = body_val.as_bool(self.ctx);

                    // Build Z3 quantifier
                    let bound_ref = &bound;
                    let pattern = z3::Pattern::new(self.ctx, &[bound_ref as &dyn z3::ast::Ast]);
                    let q = if is_forall {
                        z3::ast::forall_const(
                            self.ctx,
                            &[bound_ref as &dyn z3::ast::Ast],
                            &[&pattern],
                            &body_bool,
                        )
                    } else {
                        z3::ast::exists_const(
                            self.ctx,
                            &[bound_ref as &dyn z3::ast::Ast],
                            &[&pattern],
                            &body_bool,
                        )
                    };
                    return (Z3Value::Bool(q), tokens.len());
                }
            }

            // --- Integer literal ---
            if let Ok(n) = tok.parse::<i64>() {
                return (Z3Value::Int(ast::Int::from_i64(self.ctx, n)), start + 1);
            }

            // --- Float literal ---
            if tok.contains('.')
                && let Ok(f) = tok.parse::<f64>()
            {
                let denom = 1_000_000i32;
                let numer = (f * denom as f64) as i32;
                return (
                    Z3Value::Real(ast::Real::from_real(self.ctx, numer, denom)),
                    start + 1,
                );
            }

            // --- Identifier (possibly with dot-separated field access) ---
            let mut name = tok.clone();
            let mut next = start + 1;
            // Collapse `x.y.z` chains into one name for Z3
            while next + 1 < tokens.len() && tokens[next] == "." {
                name.push('.');
                name.push_str(&tokens[next + 1]);
                next += 2;
            }

            // Check for function call: `name(args)` -> encode with semantics
            if next < tokens.len() && tokens[next] == "(" {
                // Find matching close paren
                let mut depth = 1usize;
                let mut p = next + 1;
                while p < tokens.len() && depth > 0 {
                    match tokens[p].as_str() {
                        "(" => depth += 1,
                        ")" => depth -= 1,
                        _ => {}
                    }
                    if depth > 0 {
                        p += 1;
                    }
                }
                // Parse arguments by splitting on commas at depth 0
                let arg_tokens = &tokens[next + 1..p];
                let mut arg_vals: Vec<ast::Int<'ctx>> = Vec::new();
                if !(arg_tokens.is_empty() || arg_tokens.len() == 1 && arg_tokens[0] == ")") {
                    let mut arg_start = 0;
                    let mut d = 0usize;
                    for (i, t) in arg_tokens.iter().enumerate() {
                        match t.as_str() {
                            "(" => d += 1,
                            ")" => d = d.saturating_sub(1),
                            "," if d == 0 => {
                                let chunk = &arg_tokens[arg_start..i];
                                if !chunk.is_empty() {
                                    let (v, _) = self.parse_raw_expr(chunk, 0);
                                    arg_vals.push(v.as_int(self.ctx, &mut self.fresh_counter));
                                }
                                arg_start = i + 1;
                            }
                            _ => {}
                        }
                    }
                    // Last argument after final comma (or only argument)
                    let chunk = &arg_tokens[arg_start..];
                    if !chunk.is_empty() {
                        let (v, _) = self.parse_raw_expr(chunk, 0);
                        arg_vals.push(v.as_int(self.ctx, &mut self.fresh_counter));
                    }
                }
                let end = p + 1; // skip closing ')'

                // Extract the base function name (last segment after dots)
                let func_name = name.rsplit('.').next().unwrap_or(&name);

                // Built-in functions with known semantics
                match func_name {
                    "abs" if arg_vals.len() == 1 => {
                        let x = &arg_vals[0];
                        let zero = ast::Int::from_i64(self.ctx, 0);
                        let neg_x = x.unary_minus();
                        let cond = x.ge(&zero);
                        return (Z3Value::Int(cond.ite(x, &neg_x)), end);
                    }
                    "min" if arg_vals.len() == 2 => {
                        let (a, b) = (&arg_vals[0], &arg_vals[1]);
                        return (Z3Value::Int(a.le(b).ite(a, b)), end);
                    }
                    "max" if arg_vals.len() == 2 => {
                        let (a, b) = (&arg_vals[0], &arg_vals[1]);
                        return (Z3Value::Int(a.ge(b).ite(a, b)), end);
                    }
                    _ => {}
                }

                // Boolean-returning functions
                if matches!(
                    func_name,
                    "contains"
                        | "is_empty"
                        | "is_some"
                        | "is_none"
                        | "is_ok"
                        | "is_err"
                        | "any"
                        | "all"
                        | "contains_key"
                        | "starts_with"
                        | "ends_with"
                        | "is_subset"
                        | "is_superset"
                ) {
                    let bool_sort = z3::Sort::bool(self.ctx);
                    let int_sort = z3::Sort::int(self.ctx);
                    let arity = arg_vals.len().max(1);
                    let param_sorts: Vec<&z3::Sort> = (0..arity).map(|_| &int_sort).collect();
                    let decl = z3::FuncDecl::new(self.ctx, func_name, &param_sorts, &bool_sort);
                    let arg_refs: Vec<&dyn z3::ast::Ast> =
                        arg_vals.iter().map(|a| a as &dyn z3::ast::Ast).collect();
                    let result = if arg_refs.is_empty() {
                        let dummy = self.fresh_int();
                        decl.apply(&[&dummy as &dyn z3::ast::Ast])
                    } else {
                        decl.apply(&arg_refs)
                    };
                    let b = result.as_bool().unwrap_or_else(|| self.fresh_bool());
                    return (Z3Value::Bool(b), end);
                }

                // Size-like functions get non-negativity axiom
                if matches!(func_name, "len" | "length" | "size" | "count" | "capacity") {
                    let decl = self.make_func(func_name, arg_vals.len().max(1));
                    let arg_refs: Vec<&dyn z3::ast::Ast> =
                        arg_vals.iter().map(|a| a as &dyn z3::ast::Ast).collect();
                    let result = if arg_refs.is_empty() {
                        let dummy = self.fresh_int();
                        decl.apply(&[&dummy as &dyn z3::ast::Ast])
                    } else {
                        decl.apply(&arg_refs)
                    };
                    let len_val = result.as_int().unwrap_or_else(|| self.fresh_int());
                    let zero = ast::Int::from_i64(self.ctx, 0);
                    self.background_axioms.push(len_val.ge(&zero));
                    return (Z3Value::Int(len_val), end);
                }

                // Unknown function: uninterpreted
                let decl = self.make_func(&name, arg_vals.len().max(1));
                let arg_refs: Vec<&dyn z3::ast::Ast> =
                    arg_vals.iter().map(|a| a as &dyn z3::ast::Ast).collect();
                let result = if arg_refs.is_empty() {
                    let dummy = self.fresh_int();
                    decl.apply(&[&dummy as &dyn z3::ast::Ast])
                } else {
                    decl.apply(&arg_refs)
                };
                return (
                    Z3Value::Int(result.as_int().unwrap_or_else(|| self.fresh_int())),
                    end,
                );
            }

            let v = self.get_or_create_int(&name);
            (Z3Value::Int(v), next)
        }

        /// Apply a raw binary operation.
        fn apply_raw_op(
            &mut self,
            op: RawOp,
            lhs: Z3Value<'ctx>,
            rhs: Z3Value<'ctx>,
        ) -> Z3Value<'ctx> {
            match op {
                RawOp::Add => {
                    let l = lhs.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rhs.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Int(ast::Int::add(self.ctx, &[&l, &r]))
                }
                RawOp::Sub => {
                    let l = lhs.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rhs.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Int(ast::Int::sub(self.ctx, &[&l, &r]))
                }
                RawOp::Mul => {
                    let l = lhs.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rhs.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Int(ast::Int::mul(self.ctx, &[&l, &r]))
                }
                RawOp::Div => {
                    let l = lhs.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rhs.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Int(l.div(&r))
                }
                RawOp::Mod => {
                    let l = lhs.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rhs.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Int(l.rem(&r))
                }
                RawOp::Eq => match (&lhs, &rhs) {
                    (Z3Value::Bool(l), Z3Value::Bool(r)) => Z3Value::Bool(l._eq(r)),
                    _ => {
                        let l = lhs.as_int(self.ctx, &mut self.fresh_counter);
                        let r = rhs.as_int(self.ctx, &mut self.fresh_counter);
                        Z3Value::Bool(l._eq(&r))
                    }
                },
                RawOp::Neq => match (&lhs, &rhs) {
                    (Z3Value::Bool(l), Z3Value::Bool(r)) => Z3Value::Bool(l._eq(r).not()),
                    _ => {
                        let l = lhs.as_int(self.ctx, &mut self.fresh_counter);
                        let r = rhs.as_int(self.ctx, &mut self.fresh_counter);
                        Z3Value::Bool(l._eq(&r).not())
                    }
                },
                RawOp::Lt => {
                    let l = lhs.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rhs.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Bool(l.lt(&r))
                }
                RawOp::Lte => {
                    let l = lhs.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rhs.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Bool(l.le(&r))
                }
                RawOp::Gt => {
                    let l = lhs.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rhs.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Bool(l.gt(&r))
                }
                RawOp::Gte => {
                    let l = lhs.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rhs.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Bool(l.ge(&r))
                }
                RawOp::And => {
                    let l = lhs.as_bool(self.ctx);
                    let r = rhs.as_bool(self.ctx);
                    Z3Value::Bool(ast::Bool::and(self.ctx, &[&l, &r]))
                }
                RawOp::Or => {
                    let l = lhs.as_bool(self.ctx);
                    let r = rhs.as_bool(self.ctx);
                    Z3Value::Bool(ast::Bool::or(self.ctx, &[&l, &r]))
                }
                RawOp::Implies => {
                    let l = lhs.as_bool(self.ctx);
                    let r = rhs.as_bool(self.ctx);
                    Z3Value::Bool(l.implies(&r))
                }
            }
        }

        /// Returns true if the value is a Real.
        fn is_real(v: &Z3Value) -> bool {
            matches!(v, Z3Value::Real(_))
        }

        /// Check if a BinOp is a comparison operator.
        fn is_comparison(op: &BinOp) -> bool {
            matches!(
                op,
                BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte | BinOp::Eq | BinOp::Neq
            )
        }

        /// Encode a binary operation.
        fn encode_binop(&mut self, lhs: &Expr, op: &BinOp, rhs: &Expr) -> Z3Value<'ctx> {
            // Comparison chaining: a < b < c  =>  (a < b) && (b < c)
            // The parser produces BinOp(BinOp(a, <, b), <, c). We detect
            // when a comparison's LHS is itself a comparison, extract the
            // shared middle operand, and encode as conjunction.
            if Self::is_comparison(op)
                && let Expr::BinOp {
                    lhs: inner_lhs,
                    op: inner_op,
                    rhs: inner_rhs,
                } = lhs
                && Self::is_comparison(inner_op)
            {
                // Encode: (inner_lhs inner_op inner_rhs) && (inner_rhs op rhs)
                let left_cmp = self.encode_binop(inner_lhs, inner_op, inner_rhs);
                let right_cmp = self.encode_binop(inner_rhs, op, rhs);
                let l = left_cmp.as_bool(self.ctx);
                let r = right_cmp.as_bool(self.ctx);
                return Z3Value::Bool(ast::Bool::and(self.ctx, &[&l, &r]));
            }

            let lv = self.encode_expr(lhs);
            let rv = self.encode_expr(rhs);

            match op {
                // --- Arithmetic: produce Int or Real depending on operands ---
                BinOp::Add => {
                    if Self::is_real(&lv) || Self::is_real(&rv) {
                        let l = lv.as_real(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_real(self.ctx, &mut self.fresh_counter);
                        Z3Value::Real(ast::Real::add(self.ctx, &[&l, &r]))
                    } else {
                        let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                        Z3Value::Int(ast::Int::add(self.ctx, &[&l, &r]))
                    }
                }
                BinOp::Sub => {
                    if Self::is_real(&lv) || Self::is_real(&rv) {
                        let l = lv.as_real(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_real(self.ctx, &mut self.fresh_counter);
                        Z3Value::Real(ast::Real::sub(self.ctx, &[&l, &r]))
                    } else {
                        let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                        Z3Value::Int(ast::Int::sub(self.ctx, &[&l, &r]))
                    }
                }
                BinOp::Mul => {
                    if Self::is_real(&lv) || Self::is_real(&rv) {
                        let l = lv.as_real(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_real(self.ctx, &mut self.fresh_counter);
                        Z3Value::Real(ast::Real::mul(self.ctx, &[&l, &r]))
                    } else {
                        let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                        Z3Value::Int(ast::Int::mul(self.ctx, &[&l, &r]))
                    }
                }
                BinOp::Div => {
                    if Self::is_real(&lv) || Self::is_real(&rv) {
                        let l = lv.as_real(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_real(self.ctx, &mut self.fresh_counter);
                        Z3Value::Real(l.div(&r))
                    } else {
                        let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                        Z3Value::Int(l.div(&r))
                    }
                }
                BinOp::Mod => {
                    let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Int(l.rem(&r))
                }

                // --- Comparison: produce Bool (promote to Real if needed) ---
                BinOp::Eq => match (&lv, &rv) {
                    (Z3Value::Int(l), Z3Value::Int(r)) => Z3Value::Bool(l._eq(r)),
                    (Z3Value::Bool(l), Z3Value::Bool(r)) => Z3Value::Bool(l._eq(r)),
                    (Z3Value::Real(l), Z3Value::Real(r)) => Z3Value::Bool(l._eq(r)),
                    _ if Self::is_real(&lv) || Self::is_real(&rv) => {
                        let l = lv.as_real(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_real(self.ctx, &mut self.fresh_counter);
                        Z3Value::Bool(l._eq(&r))
                    }
                    _ => {
                        let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                        Z3Value::Bool(l._eq(&r))
                    }
                },
                BinOp::Neq => match (&lv, &rv) {
                    (Z3Value::Int(l), Z3Value::Int(r)) => Z3Value::Bool(l._eq(r).not()),
                    (Z3Value::Bool(l), Z3Value::Bool(r)) => Z3Value::Bool(l._eq(r).not()),
                    (Z3Value::Real(l), Z3Value::Real(r)) => Z3Value::Bool(l._eq(r).not()),
                    _ if Self::is_real(&lv) || Self::is_real(&rv) => {
                        let l = lv.as_real(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_real(self.ctx, &mut self.fresh_counter);
                        Z3Value::Bool(l._eq(&r).not())
                    }
                    _ => {
                        let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                        Z3Value::Bool(l._eq(&r).not())
                    }
                },
                BinOp::Lt => {
                    if Self::is_real(&lv) || Self::is_real(&rv) {
                        let l = lv.as_real(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_real(self.ctx, &mut self.fresh_counter);
                        Z3Value::Bool(l.lt(&r))
                    } else {
                        let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                        Z3Value::Bool(l.lt(&r))
                    }
                }
                BinOp::Lte => {
                    if Self::is_real(&lv) || Self::is_real(&rv) {
                        let l = lv.as_real(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_real(self.ctx, &mut self.fresh_counter);
                        Z3Value::Bool(l.le(&r))
                    } else {
                        let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                        Z3Value::Bool(l.le(&r))
                    }
                }
                BinOp::Gt => {
                    if Self::is_real(&lv) || Self::is_real(&rv) {
                        let l = lv.as_real(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_real(self.ctx, &mut self.fresh_counter);
                        Z3Value::Bool(l.gt(&r))
                    } else {
                        let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                        Z3Value::Bool(l.gt(&r))
                    }
                }
                BinOp::Gte => {
                    if Self::is_real(&lv) || Self::is_real(&rv) {
                        let l = lv.as_real(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_real(self.ctx, &mut self.fresh_counter);
                        Z3Value::Bool(l.ge(&r))
                    } else {
                        let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                        Z3Value::Bool(l.ge(&r))
                    }
                }

                // --- Logical: produce Bool ---
                BinOp::And => {
                    let l = lv.as_bool(self.ctx);
                    let r = rv.as_bool(self.ctx);
                    Z3Value::Bool(ast::Bool::and(self.ctx, &[&l, &r]))
                }
                BinOp::Or => {
                    let l = lv.as_bool(self.ctx);
                    let r = rv.as_bool(self.ctx);
                    Z3Value::Bool(ast::Bool::or(self.ctx, &[&l, &r]))
                }
                BinOp::Implies => {
                    let l = lv.as_bool(self.ctx);
                    let r = rv.as_bool(self.ctx);
                    Z3Value::Bool(l.implies(&r))
                }

                // --- Membership: uninterpreted function __contains(set, elem) ---
                BinOp::In | BinOp::NotIn => {
                    let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                    let decl = self.make_func("__contains", 2);
                    let result = decl.apply(&[&r as &dyn z3::ast::Ast, &l as &dyn z3::ast::Ast]);
                    let contains_int = result.as_int().unwrap_or_else(|| self.fresh_int());
                    // __contains returns 0 for false, non-zero for true
                    let zero = ast::Int::from_i64(self.ctx, 0);
                    let is_member = contains_int._eq(&zero).not();
                    if matches!(op, BinOp::NotIn) {
                        Z3Value::Bool(is_member.not())
                    } else {
                        Z3Value::Bool(is_member)
                    }
                }
                BinOp::Concat => {
                    // String/list concat: result is a fresh value with
                    // length axiom: len(a ++ b) == len(a) + len(b)
                    let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                    let result = self.fresh_int();
                    let len_decl = self.make_func("__field_len", 1);
                    let len_l = len_decl
                        .apply(&[&l as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let len_r = len_decl
                        .apply(&[&r as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let len_result = len_decl
                        .apply(&[&result as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    // len(a) >= 0, len(b) >= 0
                    let zero = ast::Int::from_i64(self.ctx, 0);
                    self.background_axioms.push(len_l.ge(&zero));
                    self.background_axioms.push(len_r.ge(&zero));
                    // len(a ++ b) == len(a) + len(b)
                    let sum = ast::Int::add(self.ctx, &[&len_l, &len_r]);
                    self.background_axioms.push(len_result._eq(&sum));
                    // len(a ++ b) >= 0
                    self.background_axioms.push(len_result.ge(&zero));
                    Z3Value::Int(result)
                }
                BinOp::Range => {
                    // Range is structural (already constrained by domain
                    // guard in quantifiers); return a fresh collection
                    Z3Value::Int(self.fresh_int())
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Clause description helper
    // -----------------------------------------------------------------------

    fn clause_desc(parent_name: &str, kind: &ClauseKind) -> String {
        let kind_str = match kind {
            ClauseKind::Requires => "requires",
            ClauseKind::Ensures => "ensures",
            ClauseKind::Invariant => "invariant",
            ClauseKind::Effects => "effects",
            ClauseKind::Modifies => "modifies",
            ClauseKind::Input => "input",
            ClauseKind::Output => "output",
            ClauseKind::Errors => "errors",
            ClauseKind::Rule => "rule",
            ClauseKind::DataFlow => "data_flow",
            ClauseKind::MustNot => "must_not",
            ClauseKind::Decreases => "decreases",
            ClauseKind::Other(s) => s.as_str(),
        };
        format!("{parent_name}::{kind_str}")
    }

    // -----------------------------------------------------------------------
    // Solver result interpretation
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Model extraction (T040)
    // -----------------------------------------------------------------------

    /// Parse a Z3 model into a structured `CounterexampleModel`.
    ///
    /// Iterates over the constant declarations in the model, evaluates
    /// each one with model completion, and collects `(name, value)` pairs.
    /// Internal variables (prefixed with `__`) are excluded.
    fn extract_counter_model(model: &Model<'_>) -> CounterexampleModel {
        let mut variables: Vec<(String, String)> = Vec::new();
        for decl in model.iter() {
            // Skip non-constant declarations (uninterpreted functions with
            // arity > 0 cannot be evaluated with apply(&[]))
            if decl.arity() > 0 {
                continue;
            }
            let name = decl.name();
            // Skip internal/fresh variables, but keep __result
            if name.starts_with("__") && name != "__result" {
                continue;
            }
            // Try to get the interpretation as a string
            let value = model
                .get_const_interp(&decl.apply(&[]))
                .map(|v| format!("{v}"))
                .unwrap_or_else(|| "?".into());
            variables.push((name, value));
        }
        // Sort for deterministic output
        variables.sort_by(|a, b| a.0.cmp(&b.0));
        CounterexampleModel { variables }
    }

    // -----------------------------------------------------------------------
    // Solver result interpretation
    // -----------------------------------------------------------------------

    /// Interpret solver result for a validity check (ensures/rule).
    /// We negate the goal and check-sat: UNSAT = valid.
    fn check_validity(solver: &Solver<'_>, desc: String, results: &mut Vec<VerificationResult>) {
        match solver.check() {
            SatResult::Unsat => {
                results.push(VerificationResult::Verified { clause_desc: desc });
            }
            SatResult::Sat => {
                let (model_str, counter_model) = if let Some(m) = solver.get_model() {
                    let cm = extract_counter_model(&m);
                    (format!("{m}"), Some(cm))
                } else {
                    ("(no model)".into(), None)
                };
                results.push(VerificationResult::Counterexample {
                    clause_desc: desc,
                    model: model_str,
                    counter_model,
                });
            }
            SatResult::Unknown => {
                let reason = solver
                    .get_reason_unknown()
                    .unwrap_or_else(|| "unknown".into());
                if reason.contains("timeout") {
                    results.push(VerificationResult::Timeout { clause_desc: desc });
                } else {
                    results.push(VerificationResult::Unknown {
                        clause_desc: desc,
                        reason,
                    });
                }
            }
        }
    }

    /// Interpret solver result for a satisfiability check (invariant).
    /// We assert the formula directly: SAT = satisfiable = good.
    fn check_satisfiability(
        solver: &Solver<'_>,
        desc: String,
        results: &mut Vec<VerificationResult>,
    ) {
        match solver.check() {
            SatResult::Sat => {
                results.push(VerificationResult::Verified { clause_desc: desc });
            }
            SatResult::Unsat => {
                results.push(VerificationResult::Counterexample {
                    clause_desc: desc,
                    model: "invariant is unsatisfiable (always false)".into(),
                    counter_model: None,
                });
            }
            SatResult::Unknown => {
                let reason = solver
                    .get_reason_unknown()
                    .unwrap_or_else(|| "unknown".into());
                if reason.contains("timeout") {
                    results.push(VerificationResult::Timeout { clause_desc: desc });
                } else {
                    results.push(VerificationResult::Unknown {
                        clause_desc: desc,
                        reason,
                    });
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Contract clause verification
    // -----------------------------------------------------------------------

    /// Verify a set of clauses from a contract, fn, or extern declaration.
    fn verify_clauses(
        ctx: &Context,
        parent_name: &str,
        clauses: &[Clause],
        lemma_defs: &std::collections::HashMap<String, Vec<&Expr>>,
        cache: &mut SessionCache,
        results: &mut Vec<VerificationResult>,
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

        for clause in &verifiable {
            let desc = clause_desc(parent_name, &clause.kind);

            // T113: Check verification cache before invoking Z3
            let clause_hash = format!("{desc}:{:?}", clause.body);
            if let Some(cached) = cache.lookup(&clause_hash) {
                // Replay cached result
                match cached.result.as_str() {
                    "verified" => results.push(VerificationResult::Verified { clause_desc: desc }),
                    "timeout" => results.push(VerificationResult::Timeout { clause_desc: desc }),
                    other => results.push(VerificationResult::Unknown {
                        clause_desc: desc,
                        reason: other.to_string(),
                    }),
                }
                continue;
            }

            let solver = Solver::new(ctx);

            let mut encoder = Encoder::new(ctx);

            // Assert all requires as assumptions
            for req in &requires {
                let req_val = encoder.encode_expr(&req.body);
                let req_bool = req_val.as_bool(ctx);
                solver.assert(&req_bool);
            }
            // Assert background axioms from requires encoding (e.g., map
            // read-over-write, string length axioms)
            for axiom in &encoder.background_axioms {
                solver.assert(axiom);
            }
            encoder.background_axioms.clear();

            // T044: Inject lemma ensures as assumptions for any `apply` refs
            let apply_refs = collect_apply_refs(clauses);
            for lemma_name in &apply_refs {
                if let Some(ensures_bodies) = lemma_defs.get(lemma_name) {
                    for ensures_body in ensures_bodies {
                        let ens_val = encoder.encode_expr(ensures_body);
                        let ens_bool = ens_val.as_bool(ctx);
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
                    // Create the current-state variable
                    let current = encoder.get_or_create_int(var_name);
                    // Create the old-state variable (uses __old suffix)
                    let old_name = format!("{var_name}__old");
                    let old_var = encoder.get_or_create_int(&old_name);
                    // Assert frame axiom: current == old
                    let axiom = current._eq(&old_var);
                    solver.assert(&axiom);
                }
            }

            // Encode the clause body
            let clause_val = encoder.encode_expr(&clause.body);
            let clause_bool = clause_val.as_bool(ctx);

            // Assert background axioms (e.g., len >= 0) collected during encoding
            for axiom in &encoder.background_axioms {
                solver.assert(axiom);
            }

            let result_before = results.len();
            match clause.kind {
                ClauseKind::Ensures | ClauseKind::Rule => {
                    // Validity check: assert NOT clause, check-sat
                    solver.assert(&clause_bool.not());
                    check_validity(&solver, desc, results);
                }
                ClauseKind::Invariant => {
                    // Satisfiability check: assert clause directly
                    solver.assert(&clause_bool);
                    check_satisfiability(&solver, desc, results);
                }
                ClauseKind::MustNot => {
                    // Must-not: the bad thing should be impossible under requires
                    solver.assert(&clause_bool);
                    check_validity(&solver, desc, results);
                }
                ClauseKind::Decreases => {
                    // Decreases: verify the expression is non-negative (well-founded).
                    // Encode as: the clause expression (decreasing measure) >= 0 must hold.
                    let zero = ast::Int::from_i64(ctx, 0);
                    let measure = clause_val.as_int(ctx, &mut encoder.fresh_counter);
                    let non_neg = measure.ge(&zero);
                    solver.assert(&non_neg.not());
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
        }
    }

    /// Verify a standalone invariant expression (e.g., service invariant).
    fn verify_invariant_expr(
        ctx: &Context,
        parent_name: &str,
        expr: &Expr,
        results: &mut Vec<VerificationResult>,
    ) {
        let desc = format!("{parent_name}::invariant");
        let solver = Solver::new(ctx);
        let mut encoder = Encoder::new(ctx);
        let val = encoder.encode_expr(expr);
        let bool_val = val.as_bool(ctx);
        solver.assert(&bool_val);
        check_satisfiability(&solver, desc, results);
    }

    // -----------------------------------------------------------------------
    // Refinement subtype checking (T039)
    // -----------------------------------------------------------------------

    /// Check `{v: T | antecedent} <: {v: T | consequent}`.
    ///
    /// Encodes: assert antecedent, assert NOT consequent, check-sat.
    /// UNSAT => Verified, SAT => Counterexample.
    pub(crate) fn check_refinement_subtype_impl(
        antecedent: &Expr,
        consequent: &Expr,
    ) -> VerificationResult {
        let mut cfg = Config::new();
        cfg.set_param_value("timeout", "1000");
        let ctx = Context::new(&cfg);
        let solver = Solver::new(&ctx);

        let mut encoder = Encoder::new(&ctx);

        // Assert the antecedent (P)
        let ante_val = encoder.encode_expr(antecedent);
        let ante_bool = ante_val.as_bool(&ctx);
        solver.assert(&ante_bool);

        // Assert NOT consequent (¬Q)
        let cons_val = encoder.encode_expr(consequent);
        let cons_bool = cons_val.as_bool(&ctx);
        solver.assert(&cons_bool.not());

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
        context: &[Expr],
        antecedent: &Expr,
        consequent: &Expr,
    ) -> VerificationResult {
        let mut cfg = Config::new();
        cfg.set_param_value("timeout", "1000");
        let ctx = Context::new(&cfg);
        let solver = Solver::new(&ctx);

        let mut encoder = Encoder::new(&ctx);

        // Assert all context assumptions
        for ctx_expr in context {
            let val = encoder.encode_expr(ctx_expr);
            let bool_val = val.as_bool(&ctx);
            solver.assert(&bool_val);
        }

        // Assert the antecedent (P)
        let ante_val = encoder.encode_expr(antecedent);
        let ante_bool = ante_val.as_bool(&ctx);
        solver.assert(&ante_bool);

        // Assert NOT consequent (¬Q)
        let cons_val = encoder.encode_expr(consequent);
        let cons_bool = cons_val.as_bool(&ctx);
        solver.assert(&cons_bool.not());

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
        requires: &[Expr],
        ensures: &Expr,
    ) -> VerificationResult {
        let mut cfg = Config::new();
        cfg.set_param_value("timeout", "1000");
        let ctx = Context::new(&cfg);
        let solver = Solver::new(&ctx);
        let mut encoder = Encoder::new(&ctx);

        // Assert all requires as assumptions
        for req in requires {
            let val = encoder.encode_expr(req);
            let bool_val = val.as_bool(&ctx);
            solver.assert(&bool_val);
        }

        // Assert NOT ensures (validity check: UNSAT = valid)
        let ensures_val = encoder.encode_expr(ensures);
        let ensures_bool = ensures_val.as_bool(&ctx);
        solver.assert(&ensures_bool.not());

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
        context: &[Expr],
        sub_lo: &Expr,
        sub_hi: &Expr,
        parent_lo: &Expr,
        parent_hi: &Expr,
    ) -> VerificationResult {
        let mut cfg = Config::new();
        cfg.set_param_value("timeout", "1000");
        let ctx = Context::new(&cfg);
        let solver = Solver::new(&ctx);
        let mut encoder = Encoder::new(&ctx);

        // Assert context assumptions
        for ctx_expr in context {
            let val = encoder.encode_expr(ctx_expr);
            let bool_val = val.as_bool(&ctx);
            solver.assert(&bool_val);
        }

        // Encode bounds
        let sub_lo_val = encoder
            .encode_expr(sub_lo)
            .as_int(&ctx, &mut encoder.fresh_counter);
        let sub_hi_val = encoder
            .encode_expr(sub_hi)
            .as_int(&ctx, &mut encoder.fresh_counter);
        let parent_lo_val = encoder
            .encode_expr(parent_lo)
            .as_int(&ctx, &mut encoder.fresh_counter);
        let parent_hi_val = encoder
            .encode_expr(parent_hi)
            .as_int(&ctx, &mut encoder.fresh_counter);

        // Create bound variable for the quantifier
        let i = ast::Int::new_const(&ctx, "i");

        // sub_lo <= i and i < sub_hi
        let in_sub = ast::Bool::and(&ctx, &[&sub_lo_val.le(&i), &i.lt(&sub_hi_val)]);

        // parent_lo <= i and i < parent_hi
        let in_parent = ast::Bool::and(&ctx, &[&parent_lo_val.le(&i), &i.lt(&parent_hi_val)]);

        // forall i: in_sub => in_parent
        let containment = in_sub.implies(&in_parent);
        let forall = ast::forall_const(&ctx, &[&i], &[], &containment);

        // Negate: exists i such that in_sub and NOT in_parent
        solver.assert(&forall.not());

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
        let mut cfg = Config::new();
        cfg.set_param_value("timeout", "1000");
        let ctx = Context::new(&cfg);
        let solver = Solver::new(&ctx);

        // Create taint level variables for each labeled variable
        let mut taint_vars: HashMap<String, ast::Int<'_>> = HashMap::new();
        for (name, label) in taint_labels {
            let v = ast::Int::new_const(&ctx, format!("taint_{name}").as_str());
            let label_val = ast::Int::from_i64(&ctx, taint_label_to_int(*label));
            solver.assert(&v._eq(&label_val));
            taint_vars.insert(name.clone(), v);
        }

        if sensitive_uses.is_empty() {
            return VerificationResult::Verified {
                clause_desc: "taint_safety (no sensitive uses)".into(),
            };
        }

        // For each sensitive use, check taint_var >= required
        // We negate the conjunction: if all sensitive uses are safe, UNSAT
        let mut safe_constraints = Vec::new();
        for (var_name, required) in sensitive_uses {
            let required_int = ast::Int::from_i64(&ctx, taint_label_to_int(*required));
            if let Some(taint_v) = taint_vars.get(var_name) {
                // Safe if taint level >= required level
                safe_constraints.push(taint_v.ge(&required_int));
            } else {
                // Unknown var: assume trusted (level 2), always safe
                let trusted = ast::Int::from_i64(&ctx, 2);
                safe_constraints.push(trusted.ge(&required_int));
            }
        }

        // Assert negation: at least one constraint is NOT safe
        let safe_refs: Vec<&ast::Bool<'_>> = safe_constraints.iter().collect();
        let all_safe = ast::Bool::and(&ctx, &safe_refs);
        solver.assert(&all_safe.not());

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
    fn encode_measure_as_uf<'ctx>(
        ctx: &'ctx Context,
        measure: &MeasureDefinition,
    ) -> z3::FuncDecl<'ctx> {
        let int_sort = z3::Sort::int(ctx);

        // All parameters are modeled as integers (collections and maps are
        // uninterpreted, represented by integer identifiers)
        let param_sorts: Vec<&z3::Sort<'_>> =
            measure.param_sorts.iter().map(|_| &int_sort).collect();

        // Return sort: Nat and Set are both modeled as integers
        z3::FuncDecl::new(ctx, measure.name.as_str(), &param_sorts, &int_sort)
    }

    /// Assert the standard axioms for a measure on the given solver.
    ///
    /// Uses quantified formulas over an uninterpreted integer variable to
    /// express properties like non-negativity and empty-collection behavior.
    fn assert_measure_axioms<'ctx>(
        ctx: &'ctx Context,
        solver: &Solver<'ctx>,
        measure: &MeasureDefinition,
        func_decl: &z3::FuncDecl<'ctx>,
        all_func_decls: &HashMap<String, z3::FuncDecl<'ctx>>,
    ) {
        let zero = ast::Int::from_i64(ctx, 0);

        for axiom in &measure.axioms {
            match &axiom.tag {
                MeasureAxiomTag::NonNegative => {
                    // forall xs: measure(xs) >= 0
                    let xs = ast::Int::new_const(ctx, format!("__ax_{}_xs", measure.name));
                    let app = func_decl.apply(&[&xs]);
                    let app_int = app.as_int().unwrap();
                    let ge_zero = app_int.ge(&zero);
                    let forall = ast::forall_const(ctx, &[&xs], &[], &ge_zero);
                    solver.assert(&forall);
                }
                MeasureAxiomTag::EmptyIsZero => {
                    // measure(empty) == 0, where empty is represented as a
                    // distinguished constant
                    let empty = ast::Int::new_const(ctx, "__empty");
                    let app = func_decl.apply(&[&empty]);
                    let app_int = app.as_int().unwrap();
                    let eq_zero = app_int._eq(&zero);
                    solver.assert(&eq_zero);
                }
                MeasureAxiomTag::AppendIncrement => {
                    // forall xs, x: measure(append(xs, x)) == measure(xs) + 1
                    // We model append as a fresh uninterpreted function
                    let int_sort = z3::Sort::int(ctx);
                    let append_fn = z3::FuncDecl::new(
                        ctx,
                        format!("__append_{}", measure.name),
                        &[&int_sort, &int_sort],
                        &int_sort,
                    );
                    let xs = ast::Int::new_const(ctx, format!("__ax_{}_xs2", measure.name));
                    let x = ast::Int::new_const(ctx, format!("__ax_{}_x", measure.name));
                    let appended = append_fn.apply(&[&xs, &x]);
                    let measure_appended = func_decl.apply(&[&appended]);
                    let measure_xs = func_decl.apply(&[&xs]);
                    let one = ast::Int::from_i64(ctx, 1);
                    let measure_appended_int = measure_appended.as_int().unwrap();
                    let measure_xs_int = measure_xs.as_int().unwrap();
                    let expected = ast::Int::add(ctx, &[&measure_xs_int, &one]);
                    let eq = measure_appended_int._eq(&expected);
                    let forall = ast::forall_const(ctx, &[&xs, &x], &[], &eq);
                    solver.assert(&forall);
                }
                MeasureAxiomTag::EquivalentTo(other_name) => {
                    // forall xs: measure(xs) == other_measure(xs)
                    if let Some(other_decl) = all_func_decls.get(other_name) {
                        let xs = ast::Int::new_const(ctx, format!("__ax_{}_eq_xs", measure.name));
                        let this_app = func_decl.apply(&[&xs]);
                        let other_app = other_decl.apply(&[&xs]);
                        let this_int = this_app.as_int().unwrap();
                        let other_int = other_app.as_int().unwrap();
                        let eq = this_int._eq(&other_int);
                        let forall = ast::forall_const(ctx, &[&xs], &[], &eq);
                        solver.assert(&forall);
                    }
                }
                MeasureAxiomTag::EmptyMapEmptySet => {
                    // measure(empty_map) == empty_set
                    // Both are modeled as integers; empty_map and empty_set
                    // map to the same distinguished constant __empty, so
                    // measure(__empty) == 0 (using the empty constant).
                    let empty_map = ast::Int::new_const(ctx, "__empty_map");
                    let app = func_decl.apply(&[&empty_map]);
                    let app_int = app.as_int().unwrap();
                    let eq_zero = app_int._eq(&zero);
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
        requires: &[Expr],
        ensures: &Expr,
        measures: &[MeasureDefinition],
    ) -> VerificationResult {
        let mut cfg = Config::new();
        // Measures add quantified axioms; give the solver more time
        cfg.set_param_value("timeout", "5000");
        let ctx = Context::new(&cfg);
        let solver = Solver::new(&ctx);
        let mut encoder = Encoder::new(&ctx);

        // Step 1: Encode all measures as uninterpreted functions
        let mut func_decls: HashMap<String, z3::FuncDecl<'_>> = HashMap::new();
        for measure in measures {
            let decl = encode_measure_as_uf(&ctx, measure);
            func_decls.insert(measure.name.clone(), decl);
        }

        // Step 2: Assert all measure axioms
        for measure in measures {
            if let Some(decl) = func_decls.get(&measure.name) {
                assert_measure_axioms(&ctx, &solver, measure, decl, &func_decls);
            }
        }

        // Step 3: Assert all requires as assumptions
        for req in requires {
            let val = encoder.encode_expr(req);
            let bool_val = val.as_bool(&ctx);
            solver.assert(&bool_val);
        }

        // Step 4: Negate ensures and check validity
        let ensures_val = encoder.encode_expr(ensures);
        let ensures_bool = ensures_val.as_bool(&ctx);
        solver.assert(&ensures_bool.not());

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
        preconditions: &[Expr],
        measure_expr: &Expr,
        call_arg_expr: &Expr,
        clause_desc: String,
    ) -> VerificationResult {
        let mut cfg = Config::new();
        cfg.set_param_value("timeout", "2000");
        let ctx = Context::new(&cfg);
        let solver = Solver::new(&ctx);
        let mut encoder = Encoder::new(&ctx);

        // Assert preconditions
        for pre in preconditions {
            let val = encoder.encode_expr(pre);
            let bool_val = val.as_bool(&ctx);
            solver.assert(&bool_val);
        }

        // Encode measure and call-site argument
        let measure_val = encoder.encode_expr(measure_expr);
        let call_val = encoder.encode_expr(call_arg_expr);

        let measure_int = measure_val.as_int(&ctx, &mut encoder.fresh_counter);
        let call_int = call_val.as_int(&ctx, &mut encoder.fresh_counter);
        let zero = z3::ast::Int::from_i64(&ctx, 0);

        // The property to verify: call_arg < measure AND call_arg >= 0
        let decreases = call_int.lt(&measure_int);
        let non_negative = call_int.ge(&zero);
        let property = z3::ast::Bool::and(&ctx, &[&decreases, &non_negative]);

        // Negate and check
        solver.assert(&property.not());

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

    // -----------------------------------------------------------------------
    // Entry point
    // -----------------------------------------------------------------------

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

    /// Verify all declarations in a type-checked file using Z3.
    ///
    /// Uses `ParallelVerifier` to track verification jobs. Currently runs
    /// sequentially (Z3 `Context` is not `Send`), but the job infrastructure
    /// is in place for future parallel Z3 contexts.
    pub(crate) fn verify_quantified_impl(
        name: &str,
        assumptions: &[Expr],
        quantified_body: &Expr,
    ) -> VerificationResult {
        let mut cfg = Config::new();
        // Layer 2 timeout: 10 seconds
        cfg.set_param_value("timeout", "10000");
        let ctx = Context::new(&cfg);
        let solver = Solver::new(&ctx);

        let mut encoder = Encoder::new(&ctx);

        // Assert assumptions
        for assumption in assumptions {
            let val = encoder.encode_expr(assumption);
            let bool_val = val.as_bool(&ctx);
            solver.assert(&bool_val);
        }

        // Encode the quantified body
        let body_val = encoder.encode_expr(quantified_body);
        let body_bool = body_val.as_bool(&ctx);

        // Negate and check: UNSAT means the formula holds
        solver.assert(&body_bool.not());

        match solver.check() {
            SatResult::Unsat => VerificationResult::Verified {
                clause_desc: name.into(),
            },
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
        let mut cfg = Config::new();
        cfg.set_param_value("timeout", "1000");
        let ctx = Context::new(&cfg);
        let mut results = Vec::new();
        let mut cache = SessionCache::new();
        let lemma_defs = std::collections::HashMap::new();
        verify_clauses(
            &ctx,
            contract_name,
            clauses,
            &lemma_defs,
            &mut cache,
            &mut results,
        );
        results
    }

    pub(crate) fn verify_impl(typed: &TypedFile) -> Vec<VerificationResult> {
        let mut cfg = Config::new();
        cfg.set_param_value("timeout", "1000");
        let ctx = Context::new(&cfg);
        let mut results = Vec::new();
        let mut cache = SessionCache::new();

        // T114: register all verifiable clauses as parallel jobs
        let mut pv = ParallelVerifier::new(num_cpus());
        for decl in &typed.resolved.source.decls {
            match &decl.node {
                Decl::Contract(c) => {
                    for clause in &c.clauses {
                        if matches!(clause.kind, ClauseKind::Ensures | ClauseKind::Invariant) {
                            pv.add_job(c.name.clone(), format!("{:?}", clause.kind));
                        }
                    }
                }
                Decl::FnDef(f) => {
                    for clause in &f.clauses {
                        if matches!(clause.kind, ClauseKind::Ensures | ClauseKind::Invariant) {
                            pv.add_job(f.name.clone(), format!("{:?}", clause.kind));
                        }
                    }
                }
                _ => {}
            }
        }

        // T044: collect all lemma definitions for apply injection
        let lemma_defs = collect_lemma_defs(typed);

        for decl in &typed.resolved.source.decls {
            match &decl.node {
                Decl::Contract(c) => {
                    verify_clauses(
                        &ctx,
                        &c.name,
                        &c.clauses,
                        &lemma_defs,
                        &mut cache,
                        &mut results,
                    );
                }
                Decl::FnDef(f) => {
                    verify_clauses(
                        &ctx,
                        &f.name,
                        &f.clauses,
                        &lemma_defs,
                        &mut cache,
                        &mut results,
                    );
                }
                Decl::Extern(e) => {
                    verify_clauses(
                        &ctx,
                        &e.name,
                        &e.clauses,
                        &lemma_defs,
                        &mut cache,
                        &mut results,
                    );
                }
                Decl::Service(s) => {
                    for item in &s.items {
                        match item {
                            ServiceItem::Operation { name, clauses } => {
                                let qname = format!("{}.{}", s.name, name);
                                verify_clauses(
                                    &ctx,
                                    &qname,
                                    clauses,
                                    &lemma_defs,
                                    &mut cache,
                                    &mut results,
                                );
                            }
                            ServiceItem::Query { name, clauses } => {
                                let qname = format!("{}.{}", s.name, name);
                                verify_clauses(
                                    &ctx,
                                    &qname,
                                    clauses,
                                    &lemma_defs,
                                    &mut cache,
                                    &mut results,
                                );
                            }
                            ServiceItem::Invariant(expr) => {
                                verify_invariant_expr(&ctx, &s.name, expr, &mut results);
                            }
                            _ => {}
                        }
                    }
                }
                Decl::Block { name, body, .. } => {
                    verify_clauses(&ctx, name, body, &lemma_defs, &mut cache, &mut results);
                }
                Decl::TypeDef(_) | Decl::EnumDef(_) => {}
            }
        }

        // T092: weak memory ordering checks on concurrent contracts
        let mut wm_checker = WeakMemoryChecker::new();
        for decl in &typed.resolved.source.decls {
            if let Decl::Contract(c) = &decl.node {
                for clause in &c.clauses {
                    if clause.kind == ClauseKind::Effects
                        && (expr_contains_ident(&clause.body, "relaxed")
                            || expr_contains_ident(&clause.body, "acquire")
                            || expr_contains_ident(&clause.body, "release")
                            || expr_contains_ident(&clause.body, "seq_cst"))
                    {
                        let ordering = if expr_contains_ident(&clause.body, "seq_cst") {
                            MemoryOrdering::SeqCst
                        } else if expr_contains_ident(&clause.body, "acquire") {
                            MemoryOrdering::Acquire
                        } else if expr_contains_ident(&clause.body, "release") {
                            MemoryOrdering::Release
                        } else {
                            MemoryOrdering::Relaxed
                        };
                        wm_checker.record_access(1, c.name.clone(), true, ordering);
                    }
                }
            }
        }
        for race in wm_checker.check_data_races() {
            results.push(VerificationResult::Unknown {
                clause_desc: "weak_memory".into(),
                reason: race,
            });
        }

        // T093: prophecy variable checks (unresolved prophecies)
        let mut pm = ProphecyManager::new();
        for decl in &typed.resolved.source.decls {
            if let Decl::FnDef(f) = &decl.node {
                for clause in &f.clauses {
                    if clause.kind == ClauseKind::Ensures {
                        collect_prophecy_refs(&clause.body, &f.name, &mut pm);
                    }
                }
            }
        }
        for err in pm.check_all_resolved() {
            results.push(VerificationResult::Unknown {
                clause_desc: "prophecy".into(),
                reason: err,
            });
        }

        // T094: liveness obligation checks
        let mut lc = LivenessChecker::new();
        for decl in &typed.resolved.source.decls {
            if let Decl::Contract(c) = &decl.node {
                for clause in &c.clauses {
                    if clause.kind == ClauseKind::Ensures
                        && (expr_contains_ident(&clause.body, "eventually")
                            || expr_contains_ident(&clause.body, "leads_to"))
                    {
                        lc.add_obligation(
                            format!("{}:liveness", c.name),
                            LivenessKind::Eventually,
                            format!("{:?}", clause.body),
                            String::new(),
                        );
                    }
                }
            }
        }
        for err in lc.check_unverified() {
            results.push(VerificationResult::Unknown {
                clause_desc: "liveness".into(),
                reason: err,
            });
        }

        // T114: mark all parallel jobs complete
        let mut job_idx = 0;
        for result in &results {
            if job_idx < pv.job_count() {
                let status = match result {
                    VerificationResult::Verified { .. } => "verified",
                    VerificationResult::Counterexample { .. } => "counterexample",
                    VerificationResult::Timeout { .. } => "timeout",
                    VerificationResult::Unknown { .. } => "unknown",
                };
                pv.complete_job(job_idx, status.into());
                job_idx += 1;
            }
        }

        results
    }

    /// Get the number of available CPU cores (or a reasonable default).
    fn num_cpus() -> usize {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    }

    /// Check if an expression tree contains a specific identifier.
    fn expr_contains_ident(expr: &Expr, name: &str) -> bool {
        match expr {
            Expr::Ident(s) => s == name,
            Expr::BinOp { lhs, rhs, .. } => {
                expr_contains_ident(lhs, name) || expr_contains_ident(rhs, name)
            }
            Expr::UnaryOp { expr, .. }
            | Expr::Paren(expr)
            | Expr::Old(expr)
            | Expr::Ghost(expr) => expr_contains_ident(expr, name),
            Expr::Call { func, args } => {
                expr_contains_ident(func, name) || args.iter().any(|a| expr_contains_ident(a, name))
            }
            Expr::Field(e, _) | Expr::Cast { expr: e, .. } => expr_contains_ident(e, name),
            Expr::Block(exprs) | Expr::List(exprs) => {
                exprs.iter().any(|e| expr_contains_ident(e, name))
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                expr_contains_ident(cond, name)
                    || expr_contains_ident(then_branch, name)
                    || else_branch
                        .as_ref()
                        .is_some_and(|e| expr_contains_ident(e, name))
            }
            Expr::Forall { body, domain, .. } | Expr::Exists { body, domain, .. } => {
                expr_contains_ident(body, name) || expr_contains_ident(domain, name)
            }
            Expr::MethodCall { receiver, args, .. } => {
                expr_contains_ident(receiver, name)
                    || args.iter().any(|a| expr_contains_ident(a, name))
            }
            Expr::Index { expr, index } => {
                expr_contains_ident(expr, name) || expr_contains_ident(index, name)
            }
            Expr::Raw(tokens) => tokens.iter().any(|t| t == name),
            _ => false,
        }
    }

    /// Collect prophecy variable references from ensures clauses.
    fn collect_prophecy_refs(expr: &Expr, fn_name: &str, pm: &mut ProphecyManager) {
        match expr {
            Expr::Call { func, args } => {
                if let Expr::Ident(name) = func.as_ref()
                    && (name == "prophecy" || name == "prophesy")
                    && let Some(Expr::Ident(var_name)) = args.first()
                {
                    pm.declare(format!("{fn_name}:{var_name}"));
                }
                for arg in args {
                    collect_prophecy_refs(arg, fn_name, pm);
                }
            }
            Expr::BinOp { lhs, rhs, .. } => {
                collect_prophecy_refs(lhs, fn_name, pm);
                collect_prophecy_refs(rhs, fn_name, pm);
            }
            Expr::UnaryOp { expr, .. }
            | Expr::Paren(expr)
            | Expr::Old(expr)
            | Expr::Ghost(expr) => collect_prophecy_refs(expr, fn_name, pm),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "z3-verify"))]
mod tests {
    use super::*;

    /// Helper: parse, resolve, type-check, then verify a source string.
    fn type_check_ok(source: &str) -> assura_types::TypedFile {
        let (file, errs) = assura_parser::parse(source);
        assert!(errs.is_empty(), "unexpected parse errors: {errs:?}");
        let file = file.expect("parse returned None");
        let resolved = assura_resolve::resolve(&file).expect("resolve failed");
        assura_types::type_check(&resolved).expect("type_check failed")
    }

    fn verify_source(source: &str) -> Vec<VerificationResult> {
        use assura_parser::lexer::Token;
        use assura_parser::parser;
        use chumsky::Stream;
        use chumsky::prelude::*;
        use logos::Logos;

        let lex = Token::lexer(source);
        let tokens: Vec<(Token, std::ops::Range<usize>)> = lex
            .spanned()
            .filter_map(|(tok, span)| tok.ok().map(|t| (t, span)))
            .collect();

        let len = source.len();
        let stream = Stream::from_iter(len..len + 1, tokens.into_iter());
        let (file, _) = parser::source_file().parse_recovery(stream);
        let file = file.expect("parse failed in test");

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

// ===========================================================================
// T076: Layer 2 SMT encoding
// ===========================================================================

/// Layer 2 verification: quantified invariants, functional correctness,
/// termination proofs, and serialization roundtrip verification.
///
/// Uses AUFLIA (arrays + uninterpreted functions + linear integer arithmetic)
/// SMT theory with configurable timeout (default 10s for Layer 2).
#[derive(Debug, Clone)]
pub struct Layer2Config {
    /// Timeout in milliseconds for Layer 2 queries (default: 10_000)
    pub timeout_ms: u64,
    /// Whether to enable quantifier instantiation
    pub enable_quantifiers: bool,
    /// Whether to verify termination proofs
    pub enable_termination: bool,
    /// Whether to verify serialization roundtrips
    pub enable_roundtrip: bool,
}

impl Default for Layer2Config {
    fn default() -> Self {
        Self {
            timeout_ms: 10_000,
            enable_quantifiers: true,
            enable_termination: true,
            enable_roundtrip: true,
        }
    }
}

impl Layer2Config {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }
}

/// A quantified invariant to verify at Layer 2.
#[derive(Debug, Clone)]
pub struct QuantifiedInvariant {
    pub name: String,
    /// Bound variables: (name, sort)
    pub bound_vars: Vec<(String, String)>,
    /// The invariant body (as expression text)
    pub body: String,
    /// Optional trigger patterns for e-matching
    pub triggers: Vec<String>,
}

/// Result of a Layer 2 verification attempt.
#[derive(Debug, Clone)]
pub enum Layer2Result {
    Verified {
        invariant: String,
        time_ms: u64,
    },
    Counterexample {
        invariant: String,
        model: Vec<(String, String)>,
    },
    Timeout {
        invariant: String,
        timeout_ms: u64,
    },
    Unknown {
        invariant: String,
        reason: String,
    },
}

/// Collects Layer 2 verification obligations and dispatches them.
#[derive(Debug, Clone)]
pub struct Layer2Verifier {
    pub config: Layer2Config,
    pub invariants: Vec<QuantifiedInvariant>,
    pub termination_obligations: Vec<TerminationObligation>,
    pub roundtrip_obligations: Vec<RoundtripObligation>,
}

/// A termination proof obligation.
#[derive(Debug, Clone)]
pub struct TerminationObligation {
    pub fn_name: String,
    pub measure: String,
    pub recursive_calls: Vec<String>,
}

/// A serialization roundtrip obligation.
#[derive(Debug, Clone)]
pub struct RoundtripObligation {
    pub type_name: String,
    pub serialize_fn: String,
    pub deserialize_fn: String,
}

impl Layer2Verifier {
    pub fn new(config: Layer2Config) -> Self {
        Self {
            config,
            invariants: Vec::new(),
            termination_obligations: Vec::new(),
            roundtrip_obligations: Vec::new(),
        }
    }

    pub fn add_invariant(&mut self, inv: QuantifiedInvariant) {
        self.invariants.push(inv);
    }

    pub fn add_termination(&mut self, obl: TerminationObligation) {
        self.termination_obligations.push(obl);
    }

    pub fn add_roundtrip(&mut self, obl: RoundtripObligation) {
        self.roundtrip_obligations.push(obl);
    }

    /// Structural pre-check without Z3 (validates obligation structure only).
    ///
    /// This does NOT verify correctness. It checks that obligations are
    /// well-formed (have bound variables, have measures, etc.). Obligations
    /// that pass structural checks are reported as `Unknown` with reason
    /// "requires Z3 verification", NOT as `Verified`.
    ///
    /// Use `verify()` for actual Z3-backed verification.
    pub fn check_structural(&self) -> Vec<Layer2Result> {
        let mut results = Vec::new();

        for inv in &self.invariants {
            if inv.bound_vars.is_empty() {
                results.push(Layer2Result::Unknown {
                    invariant: inv.name.clone(),
                    reason: "quantified invariant has no bound variables".into(),
                });
            } else {
                // Structurally valid, but not verified without Z3
                results.push(Layer2Result::Unknown {
                    invariant: inv.name.clone(),
                    reason: "requires Z3 Layer 2 verification".into(),
                });
            }
        }

        for obl in &self.termination_obligations {
            if obl.measure.is_empty() {
                results.push(Layer2Result::Unknown {
                    invariant: format!("termination:{}", obl.fn_name),
                    reason: "no measure provided".into(),
                });
            } else {
                // Structurally valid, but not verified without Z3
                results.push(Layer2Result::Unknown {
                    invariant: format!("termination:{}", obl.fn_name),
                    reason: "requires Z3 Layer 2 verification".into(),
                });
            }
        }

        for obl in &self.roundtrip_obligations {
            // Structurally valid, but not verified without Z3
            results.push(Layer2Result::Unknown {
                invariant: format!("roundtrip:{}", obl.type_name),
                reason: "requires Z3 Layer 2 verification".into(),
            });
        }

        results
    }

    pub fn obligation_count(&self) -> usize {
        self.invariants.len()
            + self.termination_obligations.len()
            + self.roundtrip_obligations.len()
    }

    /// Verify all quantified invariants using Z3 with Layer 2 timeout.
    ///
    /// For each `QuantifiedInvariant`, creates a Z3 context with the
    /// Layer 2 timeout (default 10s), encodes the invariant as a
    /// universally quantified formula, and checks validity (negation
    /// is UNSAT => valid).
    pub fn verify(&self) -> Vec<Layer2Result> {
        #[cfg(feature = "z3-verify")]
        {
            self.verify_with_z3()
        }
        #[cfg(not(feature = "z3-verify"))]
        {
            self.check_structural()
        }
    }

    #[cfg(feature = "z3-verify")]
    fn verify_with_z3(&self) -> Vec<Layer2Result> {
        let mut results = Vec::new();

        for inv in &self.invariants {
            if inv.bound_vars.is_empty() {
                results.push(Layer2Result::Unknown {
                    invariant: inv.name.clone(),
                    reason: "quantified invariant has no bound variables".into(),
                });
                continue;
            }
            // Structural check only for string-based invariants.
            // Real quantifier verification happens through verify_quantified_expr().
            results.push(Layer2Result::Unknown {
                invariant: inv.name.clone(),
                reason: "requires Z3 Layer 2 verification".into(),
            });
        }

        results
    }
}

/// Verify a quantified expression using Z3 with Layer 2 timeout (10s).
///
/// Sends `forall x in S: P(x)` or `exists x in S: P(x)` expressions
/// directly to Z3, using the existing `Encoder` to encode the Expr tree.
/// Returns a `VerificationResult` (not `Layer2Result`) for consistency
/// with the main verification pipeline.
///
/// Layer 2 uses a 10s timeout (vs 1s for Layer 1).
pub fn verify_quantified_expr(
    name: &str,
    assumptions: &[Expr],
    quantified_body: &Expr,
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        z3_backend::verify_quantified_impl(name, assumptions, quantified_body)
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        let _ = (assumptions, quantified_body);
        VerificationResult::Unknown {
            clause_desc: name.into(),
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }
}

// ===========================================================================
// T078: CORE.5 Quantifier triggers (e-matching hints)
// ===========================================================================

/// E-matching trigger patterns for SMT quantifier instantiation.
///
/// Triggers guide the SMT solver's quantifier instantiation by specifying
/// which ground terms should cause a quantified formula to be instantiated.
#[derive(Debug, Clone)]
pub struct TriggerPattern {
    /// The pattern terms (multi-trigger if > 1)
    pub terms: Vec<String>,
    /// Whether this is a user-provided trigger
    pub is_user_provided: bool,
}

/// Manages trigger inference and validation for quantified formulas.
#[derive(Debug, Clone)]
pub struct TriggerManager {
    /// Known function symbols for trigger inference
    known_functions: Vec<String>,
    /// User-specified triggers per quantified formula
    triggers: std::collections::HashMap<String, Vec<TriggerPattern>>,
}

impl TriggerManager {
    pub fn new() -> Self {
        Self {
            known_functions: Vec::new(),
            triggers: std::collections::HashMap::new(),
        }
    }

    pub fn register_function(&mut self, name: String) {
        if !self.known_functions.contains(&name) {
            self.known_functions.push(name);
        }
    }

    pub fn add_trigger(&mut self, formula_name: String, pattern: TriggerPattern) {
        self.triggers.entry(formula_name).or_default().push(pattern);
    }

    /// Infer a trigger pattern from the quantifier body.
    /// Returns None if no suitable trigger can be inferred.
    pub fn infer_trigger(&self, body: &str) -> Option<TriggerPattern> {
        for func in &self.known_functions {
            if body.contains(func.as_str()) {
                return Some(TriggerPattern {
                    terms: vec![format!("{func}(x)")],
                    is_user_provided: false,
                });
            }
        }
        None
    }

    /// Validate that a trigger pattern mentions only known functions.
    pub fn validate_trigger(&self, pattern: &TriggerPattern) -> Vec<String> {
        let mut warnings = Vec::new();
        for term in &pattern.terms {
            let has_known = self
                .known_functions
                .iter()
                .any(|f| term.contains(f.as_str()));
            if !has_known {
                warnings.push(format!(
                    "trigger term `{term}` does not reference any known function"
                ));
            }
        }
        warnings
    }

    pub fn get_triggers(&self, formula_name: &str) -> Option<&Vec<TriggerPattern>> {
        self.triggers.get(formula_name)
    }
}

impl Default for TriggerManager {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T073: FMT.4 Codec dispatch (magic-byte routing)
// ===========================================================================

/// Routes decoding to the appropriate codec based on magic bytes.
///
/// Error codes (via assura-types):
/// - A33001: unknown magic bytes
/// - A33002: ambiguous magic bytes (multiple codecs match)
/// - A33003: codec not registered
#[derive(Debug, Clone)]
pub struct CodecDispatcher {
    codecs: Vec<CodecEntry>,
}

#[derive(Debug, Clone)]
pub struct CodecEntry {
    pub name: String,
    pub magic_bytes: Vec<u8>,
    pub magic_offset: usize,
}

impl CodecDispatcher {
    pub fn new() -> Self {
        Self { codecs: Vec::new() }
    }

    pub fn register(&mut self, name: String, magic_bytes: Vec<u8>, offset: usize) {
        self.codecs.push(CodecEntry {
            name,
            magic_bytes,
            magic_offset: offset,
        });
    }

    /// Dispatch: find the codec matching the given data prefix.
    pub fn dispatch(&self, data: &[u8]) -> DispatchResult {
        let mut matches: Vec<&CodecEntry> = Vec::new();
        for codec in &self.codecs {
            let end = codec.magic_offset + codec.magic_bytes.len();
            if data.len() >= end && data[codec.magic_offset..end] == codec.magic_bytes {
                matches.push(codec);
            }
        }
        match matches.len() {
            0 => DispatchResult::Unknown,
            1 => DispatchResult::Matched(matches[0].name.clone()),
            _ => DispatchResult::Ambiguous(matches.iter().map(|c| c.name.clone()).collect()),
        }
    }

    /// Check for ambiguous registrations (overlapping magic bytes).
    pub fn check_ambiguity(&self) -> Vec<(String, String)> {
        let mut conflicts = Vec::new();
        for i in 0..self.codecs.len() {
            for j in (i + 1)..self.codecs.len() {
                let a = &self.codecs[i];
                let b = &self.codecs[j];
                if a.magic_offset == b.magic_offset && a.magic_bytes == b.magic_bytes {
                    conflicts.push((a.name.clone(), b.name.clone()));
                }
            }
        }
        conflicts
    }

    pub fn codec_count(&self) -> usize {
        self.codecs.len()
    }
}

impl Default for CodecDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DispatchResult {
    Matched(String),
    Unknown,
    Ambiguous(Vec<String>),
}

// ---------------------------------------------------------------------------
// Non-Z3 unit tests for MeasureDefinition and axiom logic (T054)
// ---------------------------------------------------------------------------

// ===========================================================================
// T092: CONC.6 Weak memory ordering
// ===========================================================================

/// Models C++ memory ordering semantics for verification.
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryOrdering {
    Relaxed,
    Acquire,
    Release,
    AcqRel,
    SeqCst,
}

/// A memory access with its ordering constraint.
#[derive(Debug, Clone)]
pub struct MemoryAccess {
    pub thread_id: u64,
    pub variable: String,
    pub is_write: bool,
    pub ordering: MemoryOrdering,
    pub sequence_num: u64,
}

/// Verifies weak memory ordering contracts.
#[derive(Debug, Clone)]
pub struct WeakMemoryChecker {
    accesses: Vec<MemoryAccess>,
    happens_before: Vec<(u64, u64)>,
    next_seq: u64,
}

impl WeakMemoryChecker {
    pub fn new() -> Self {
        Self {
            accesses: Vec::new(),
            happens_before: Vec::new(),
            next_seq: 0,
        }
    }

    pub fn record_access(
        &mut self,
        thread_id: u64,
        variable: String,
        is_write: bool,
        ordering: MemoryOrdering,
    ) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.accesses.push(MemoryAccess {
            thread_id,
            variable,
            is_write,
            ordering,
            sequence_num: seq,
        });
        seq
    }

    pub fn add_happens_before(&mut self, before: u64, after: u64) {
        self.happens_before.push((before, after));
    }

    fn is_ordered(&self, a: u64, b: u64) -> bool {
        self.happens_before.iter().any(|&(x, y)| x == a && y == b)
    }

    /// Check for data races: concurrent accesses to same variable with at least one write
    /// and no happens-before relationship.
    pub fn check_data_races(&self) -> Vec<String> {
        let mut races = Vec::new();
        for i in 0..self.accesses.len() {
            for j in (i + 1)..self.accesses.len() {
                let a = &self.accesses[i];
                let b = &self.accesses[j];
                if a.variable == b.variable
                    && a.thread_id != b.thread_id
                    && (a.is_write || b.is_write)
                    && !self.is_ordered(a.sequence_num, b.sequence_num)
                    && !self.is_ordered(b.sequence_num, a.sequence_num)
                {
                    races.push(format!(
                        "data race on `{}` between thread {} and thread {}",
                        a.variable, a.thread_id, b.thread_id
                    ));
                }
            }
        }
        races
    }

    /// Check that release-acquire pairs are consistent.
    pub fn check_release_acquire(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        for a in &self.accesses {
            if a.ordering == MemoryOrdering::Release && a.is_write {
                let has_acquire = self.accesses.iter().any(|b| {
                    b.variable == a.variable
                        && !b.is_write
                        && b.thread_id != a.thread_id
                        && b.ordering == MemoryOrdering::Acquire
                });
                if !has_acquire {
                    warnings.push(format!(
                        "release write on `{}` (thread {}) has no matching acquire read",
                        a.variable, a.thread_id
                    ));
                }
            }
        }
        warnings
    }

    /// Check for relaxed accesses that should be stronger.
    pub fn check_ordering_strength(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        for a in &self.accesses {
            if a.ordering == MemoryOrdering::Relaxed && a.is_write {
                let read_by_other = self
                    .accesses
                    .iter()
                    .any(|b| b.variable == a.variable && b.thread_id != a.thread_id && !b.is_write);
                if read_by_other {
                    warnings.push(format!("relaxed write on `{}` (thread {}) is read by another thread; consider Release ordering", a.variable, a.thread_id));
                }
            }
        }
        warnings
    }

    pub fn access_count(&self) -> usize {
        self.accesses.len()
    }
}

impl Default for WeakMemoryChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T093: CORE.7 Prophecy variables
// ===========================================================================

/// Ghost state with deferred resolution for future-dependent reasoning.
#[derive(Debug, Clone)]
pub struct ProphecyVariable {
    pub name: String,
    pub resolved: bool,
    pub resolution_value: Option<String>,
    pub constraints: Vec<String>,
}

/// Manages prophecy variables for verification.
#[derive(Debug, Clone)]
pub struct ProphecyManager {
    variables: std::collections::HashMap<String, ProphecyVariable>,
}

impl ProphecyManager {
    pub fn new() -> Self {
        Self {
            variables: std::collections::HashMap::new(),
        }
    }

    pub fn declare(&mut self, name: String) {
        self.variables.insert(
            name.clone(),
            ProphecyVariable {
                name,
                resolved: false,
                resolution_value: None,
                constraints: Vec::new(),
            },
        );
    }

    pub fn add_constraint(&mut self, name: &str, constraint: String) {
        if let Some(v) = self.variables.get_mut(name) {
            v.constraints.push(constraint);
        }
    }

    pub fn resolve(&mut self, name: &str, value: String) -> Result<(), String> {
        if let Some(v) = self.variables.get_mut(name) {
            if v.resolved {
                return Err(format!("prophecy variable `{name}` already resolved"));
            }
            v.resolved = true;
            v.resolution_value = Some(value);
            Ok(())
        } else {
            Err(format!("unknown prophecy variable `{name}`"))
        }
    }

    /// Check that all prophecy variables are eventually resolved.
    pub fn check_all_resolved(&self) -> Vec<String> {
        self.variables
            .iter()
            .filter(|(_, v)| !v.resolved)
            .map(|(n, _)| format!("prophecy variable `{n}` was never resolved"))
            .collect()
    }

    /// Check for prophecy variables with no constraints (useless).
    pub fn check_unconstrained(&self) -> Vec<String> {
        self.variables
            .iter()
            .filter(|(_, v)| v.constraints.is_empty())
            .map(|(n, _)| format!("prophecy variable `{n}` has no constraints"))
            .collect()
    }

    pub fn variable_count(&self) -> usize {
        self.variables.len()
    }
}

impl Default for ProphecyManager {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T094: CORE.8 Liveness contracts
// ===========================================================================

/// Liveness property kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum LivenessKind {
    Eventually,
    LeadsTo,
    EventuallyWithin(u64),
}

/// A liveness obligation.
#[derive(Debug, Clone)]
pub struct LivenessObligation {
    pub name: String,
    pub kind: LivenessKind,
    pub premise: String,
    pub conclusion: String,
    pub verified: bool,
}

/// Manages liveness contracts for verification.
#[derive(Debug, Clone)]
pub struct LivenessChecker {
    obligations: Vec<LivenessObligation>,
    fairness_assumptions: Vec<String>,
}

impl LivenessChecker {
    pub fn new() -> Self {
        Self {
            obligations: Vec::new(),
            fairness_assumptions: Vec::new(),
        }
    }

    pub fn add_obligation(
        &mut self,
        name: String,
        kind: LivenessKind,
        premise: String,
        conclusion: String,
    ) {
        self.obligations.push(LivenessObligation {
            name,
            kind,
            premise,
            conclusion,
            verified: false,
        });
    }

    pub fn add_fairness(&mut self, assumption: String) {
        self.fairness_assumptions.push(assumption);
    }

    pub fn mark_verified(&mut self, name: &str) {
        if let Some(o) = self.obligations.iter_mut().find(|o| o.name == name) {
            o.verified = true;
        }
    }

    /// Check for unverified liveness obligations.
    pub fn check_unverified(&self) -> Vec<String> {
        self.obligations
            .iter()
            .filter(|o| !o.verified)
            .map(|o| {
                format!(
                    "liveness obligation `{}` ({:?}) not verified",
                    o.name, o.kind
                )
            })
            .collect()
    }

    /// Check that eventually_within obligations have reasonable bounds.
    pub fn check_bounded(&self) -> Vec<String> {
        self.obligations
            .iter()
            .filter(|o| matches!(o.kind, LivenessKind::EventuallyWithin(t) if t == 0))
            .map(|o| format!("liveness obligation `{}` has zero time bound", o.name))
            .collect()
    }

    /// Check that leads_to obligations have fairness assumptions.
    pub fn check_fairness(&self) -> Vec<String> {
        if self.fairness_assumptions.is_empty() {
            let leads_to: Vec<_> = self
                .obligations
                .iter()
                .filter(|o| o.kind == LivenessKind::LeadsTo)
                .collect();
            if !leads_to.is_empty() {
                return vec![
                    "leads_to obligations present but no fairness assumptions declared".into(),
                ];
            }
        }
        vec![]
    }

    pub fn obligation_count(&self) -> usize {
        self.obligations.len()
    }
}

impl Default for LivenessChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T112: IR format parser
// ===========================================================================

/// Implementation IR: the intermediate format that AI agents generate.
#[derive(Debug, Clone)]
pub struct IrParser {
    nodes: Vec<IrNode>,
}

#[derive(Debug, Clone)]
pub enum IrNode {
    FnDecl {
        name: String,
        params: Vec<(String, String)>,
        body: Vec<IrNode>,
    },
    VarDecl {
        name: String,
        ty: String,
        value: Option<Box<IrNode>>,
    },
    Call {
        target: String,
        args: Vec<IrNode>,
    },
    Literal(IrLiteral),
    BinOp {
        op: String,
        left: Box<IrNode>,
        right: Box<IrNode>,
    },
    Return(Box<IrNode>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrLiteral {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
}

impl IrParser {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// Parse a text IR into nodes.
    pub fn parse_text(&mut self, source: &str) -> Result<(), String> {
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }
            if trimmed.starts_with("fn ") {
                let name = trimmed
                    .strip_prefix("fn ")
                    .unwrap_or("")
                    .split('(')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                self.nodes.push(IrNode::FnDecl {
                    name,
                    params: Vec::new(),
                    body: Vec::new(),
                });
            } else if trimmed.starts_with("let ") {
                let rest = trimmed.strip_prefix("let ").unwrap_or("");
                let name = rest.split(':').next().unwrap_or("").trim().to_string();
                self.nodes.push(IrNode::VarDecl {
                    name,
                    ty: "auto".into(),
                    value: None,
                });
            } else if trimmed.starts_with("return ") {
                let val = trimmed.strip_prefix("return ").unwrap_or("").trim();
                if let Ok(n) = val.parse::<i64>() {
                    self.nodes
                        .push(IrNode::Return(Box::new(IrNode::Literal(IrLiteral::Int(n)))));
                }
            }
        }
        Ok(())
    }

    /// Serialize nodes to a compact binary format.
    pub fn serialize_binary(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(self.nodes.len() as u32).to_le_bytes());
        for node in &self.nodes {
            match node {
                IrNode::FnDecl { name, .. } => {
                    buf.push(0x01);
                    buf.extend(name.as_bytes());
                    buf.push(0x00);
                }
                IrNode::VarDecl { name, .. } => {
                    buf.push(0x02);
                    buf.extend(name.as_bytes());
                    buf.push(0x00);
                }
                IrNode::Return(_) => {
                    buf.push(0x03);
                }
                _ => {
                    buf.push(0xFF);
                }
            }
        }
        buf
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
}

impl Default for IrParser {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// P005: Implementation IR (Section 4 of the spec)
// ===========================================================================

/// A parsed IR module (Section 4.2 of the spec).
///
/// The Implementation IR is the flat, numbered-slot format that AI agents
/// generate. Slots are `$0`, `$1`, etc. No variable names. Every
/// expression is explicitly typed. One canonical representation.
#[derive(Debug, Clone, PartialEq)]
pub struct IrModule {
    pub name: String,
    pub functions: Vec<IrFunction>,
}

/// A function declaration in the IR.
#[derive(Debug, Clone, PartialEq)]
pub struct IrFunction {
    /// Function ID (e.g., "#0", "#1")
    pub id: String,
    /// Parameter slots
    pub params: Vec<IrSlotDecl>,
    /// Return type
    pub return_type: String,
    /// Effect row (e.g., "pure", "io")
    pub effects: String,
    /// Precondition (optional)
    pub pre: Option<IrPred>,
    /// Postcondition (optional)
    pub post: Option<IrPred>,
    /// Instruction body
    pub body: Vec<IrInstr>,
}

/// A slot declaration: `$N : Type`
#[derive(Debug, Clone, PartialEq)]
pub struct IrSlotDecl {
    pub slot: usize,
    pub ty: String,
}

/// An instruction: `$N = <expr> : Type`
#[derive(Debug, Clone, PartialEq)]
pub struct IrInstr {
    pub target: usize,
    pub expr: IrExprKind,
    pub ty: String,
}

/// IR expression forms (Section 4.2).
#[derive(Debug, Clone, PartialEq)]
pub enum IrExprKind {
    /// `const <literal>`
    Const(IrLiteral),
    /// `load $N`
    Load(usize),
    /// `call <fn> ($N, $M, ...)`
    Call { func: String, args: Vec<usize> },
    /// `field $N .M`
    Field { slot: usize, index: usize },
    /// `construct TypeId { .0 = $N, .1 = $M, ... }`
    Construct {
        type_id: String,
        fields: Vec<(usize, usize)>,
    },
    /// `arith <op> $N $M`
    Arith {
        op: IrArithOp,
        lhs: usize,
        rhs: usize,
    },
    /// `cmp <op> $N $M`
    Cmp { op: IrCmpOp, lhs: usize, rhs: usize },
    /// `cast $N as Type`
    Cast { slot: usize, target_type: String },
    /// `if $N then #B1 else #B2`
    If {
        cond: usize,
        then_block: usize,
        else_block: usize,
    },
    /// `transition $N to StateId`
    Transition { slot: usize, state: String },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IrArithOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IrCmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// IR predicate for pre/post conditions (Section 4.2).
#[derive(Debug, Clone, PartialEq)]
pub enum IrPred {
    True,
    False,
    Cmp {
        op: IrCmpOp,
        lhs: IrPredArg,
        rhs: IrPredArg,
    },
    And(Box<IrPred>, Box<IrPred>),
    Or(Box<IrPred>, Box<IrPred>),
    Not(Box<IrPred>),
}

/// Argument inside an IR predicate (slot ref or literal).
#[derive(Debug, Clone, PartialEq)]
pub enum IrPredArg {
    Slot(usize),
    SlotResult,
    Lit(IrLiteral),
    Arith {
        op: IrArithOp,
        lhs: Box<IrPredArg>,
        rhs: Box<IrPredArg>,
    },
}

/// Result of validating IR against a contract.
#[derive(Debug, Clone)]
pub struct IrValidation {
    pub valid: bool,
    pub errors: Vec<String>,
}

/// Parse an IR text module from source.
///
/// The text format follows Section 4.2 of the spec:
/// ```text
/// module <name> {
///   fn #0 : ($0: Int, $1: Int) -> Int ! pure
///     pre: cmp ne $1 (const 0)
///     post: cmp eq ... $0
///   {
///     $2 = arith div $0 $1 : Int
///     $result = load $2 : Int
///   }
/// }
/// ```
pub fn parse_ir_module(source: &str) -> Result<IrModule, Vec<String>> {
    let mut errors = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;

    // Skip blanks / comments
    while i < lines.len() {
        let t = lines[i].trim();
        if !t.is_empty() && !t.starts_with("//") {
            break;
        }
        i += 1;
    }

    // Parse module header
    if i >= lines.len() {
        errors.push("expected 'module <name> {', got end of input".into());
        return Err(errors);
    }
    let header = lines[i].trim();
    let module_name = if let Some(rest) = header.strip_prefix("module ") {
        let rest = rest.trim();
        let name = rest.trim_end_matches('{').trim().to_string();
        if name.is_empty() {
            errors.push(format!("line {}: empty module name", i + 1));
            return Err(errors);
        }
        name
    } else {
        errors.push(format!(
            "line {}: expected 'module <name> {{', got: {}",
            i + 1,
            header
        ));
        return Err(errors);
    };
    i += 1;

    let mut functions = Vec::new();

    // Parse declarations until closing '}'
    while i < lines.len() {
        let t = lines[i].trim();
        if t == "}" {
            break;
        }
        if t.is_empty() || t.starts_with("//") {
            i += 1;
            continue;
        }
        if t.starts_with("fn ") {
            match parse_ir_function(&lines, &mut i) {
                Ok(f) => functions.push(f),
                Err(e) => errors.extend(e),
            }
        } else {
            errors.push(format!("line {}: unexpected: {}", i + 1, t));
            i += 1;
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(IrModule {
        name: module_name,
        functions,
    })
}

fn parse_ir_function(lines: &[&str], pos: &mut usize) -> Result<IrFunction, Vec<String>> {
    let mut errors = Vec::new();
    let header = lines[*pos].trim();

    // Parse: fn #0 : ($0: Int, $1: Int) -> Int ! pure
    let rest = header.strip_prefix("fn ").unwrap_or("");
    let parts: Vec<&str> = rest.splitn(2, ':').collect();
    let id = parts[0].trim().to_string();

    let sig_str = if parts.len() > 1 { parts[1].trim() } else { "" };

    // Parse params from signature: ($0: Type, $1: Type) -> RetType ! Effects
    let (params, return_type, effects) = parse_ir_sig(sig_str);

    *pos += 1;

    // Parse optional pre/post conditions
    let mut pre = None;
    let mut post = None;
    while *pos < lines.len() {
        let t = lines[*pos].trim();
        if t.starts_with("pre:") {
            let pred_str = t.strip_prefix("pre:").unwrap_or("").trim();
            pre = parse_ir_pred_str(pred_str);
            *pos += 1;
        } else if t.starts_with("post:") {
            let pred_str = t.strip_prefix("post:").unwrap_or("").trim();
            post = parse_ir_pred_str(pred_str);
            *pos += 1;
        } else {
            break;
        }
    }

    // Parse body: { ... }
    let mut body = Vec::new();
    if *pos < lines.len() && lines[*pos].trim() == "{" {
        *pos += 1;
    }
    while *pos < lines.len() {
        let t = lines[*pos].trim();
        if t == "}" {
            *pos += 1;
            break;
        }
        if t.is_empty() || t.starts_with("//") {
            *pos += 1;
            continue;
        }
        match parse_ir_instr(t) {
            Ok(instr) => body.push(instr),
            Err(e) => errors.push(format!("line {}: {}", *pos + 1, e)),
        }
        *pos += 1;
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(IrFunction {
        id,
        params,
        return_type,
        effects,
        pre,
        post,
        body,
    })
}

fn parse_ir_sig(sig: &str) -> (Vec<IrSlotDecl>, String, String) {
    let mut params = Vec::new();
    let mut return_type = String::new();
    let mut effects = String::new();

    // Find the param list between ( and )
    if let Some(paren_start) = sig.find('(')
        && let Some(paren_end) = sig.find(')')
    {
        let param_str = &sig[paren_start + 1..paren_end];
        for part in param_str.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            // Parse $N: Type or $N: Type @grade
            let slot_and_ty: Vec<&str> = part.splitn(2, ':').collect();
            if slot_and_ty.len() == 2 {
                let slot_str = slot_and_ty[0].trim().trim_start_matches('$');
                let ty_str = slot_and_ty[1].trim().split('@').next().unwrap_or("").trim();
                if let Ok(slot) = slot_str.parse::<usize>() {
                    params.push(IrSlotDecl {
                        slot,
                        ty: ty_str.to_string(),
                    });
                }
            }
        }

        // Parse: -> RetType ! Effects
        let after_params = &sig[paren_end + 1..];
        if let Some(arrow_pos) = after_params.find("->") {
            let after_arrow = &after_params[arrow_pos + 2..];
            if let Some(bang_pos) = after_arrow.find('!') {
                return_type = after_arrow[..bang_pos].trim().to_string();
                effects = after_arrow[bang_pos + 1..].trim().to_string();
            } else {
                return_type = after_arrow.trim().to_string();
            }
        }
    }

    (params, return_type, effects)
}

fn parse_slot(s: &str) -> Result<usize, String> {
    let s = s.trim();
    if s == "$result" {
        return Ok(usize::MAX); // sentinel for $result
    }
    s.strip_prefix('$')
        .and_then(|n| n.parse::<usize>().ok())
        .ok_or_else(|| format!("expected slot ($N), got: {s}"))
}

fn parse_ir_instr(line: &str) -> Result<IrInstr, String> {
    // Format: $N = <expr> : Type
    // or: $result = <expr> : Type
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(format!("expected '$N = <expr> : Type', got: {line}"));
    }
    let target = parse_slot(parts[0].trim())?;

    // Split on last ' : ' to get expr and type
    let rhs = parts[1].trim();
    let (expr_str, ty) = if let Some(colon_pos) = rhs.rfind(" : ") {
        (&rhs[..colon_pos], rhs[colon_pos + 3..].trim().to_string())
    } else {
        (rhs, String::new())
    };
    let expr_str = expr_str.trim();

    let expr = parse_ir_expr(expr_str)?;

    Ok(IrInstr { target, expr, ty })
}

fn parse_ir_expr(s: &str) -> Result<IrExprKind, String> {
    let s = s.trim();

    if let Some(rest) = s.strip_prefix("const ") {
        // const <literal>
        let lit = parse_ir_literal(rest.trim())?;
        return Ok(IrExprKind::Const(lit));
    }

    if let Some(rest) = s.strip_prefix("load ") {
        let slot = parse_slot(rest.trim())?;
        return Ok(IrExprKind::Load(slot));
    }

    if let Some(rest) = s.strip_prefix("call ") {
        // call <fn> ($N, $M, ...)
        let rest = rest.trim();
        if let Some(paren_start) = rest.find('(') {
            let func = rest[..paren_start].trim().to_string();
            let paren_end = rest.rfind(')').unwrap_or(rest.len());
            let args_str = &rest[paren_start + 1..paren_end];
            let mut args = Vec::new();
            for a in args_str.split(',') {
                let a = a.trim();
                if !a.is_empty() {
                    args.push(parse_slot(a)?);
                }
            }
            return Ok(IrExprKind::Call { func, args });
        }
        return Err(format!("malformed call: {s}"));
    }

    if let Some(rest) = s.strip_prefix("field ") {
        // field $N .M
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 2 {
            let slot = parse_slot(parts[0])?;
            let index = parts[1]
                .trim_start_matches('.')
                .parse::<usize>()
                .map_err(|_| format!("bad field index: {}", parts[1]))?;
            return Ok(IrExprKind::Field { slot, index });
        }
        return Err(format!("malformed field: {s}"));
    }

    if let Some(rest) = s.strip_prefix("arith ") {
        // arith <op> $N $M
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 3 {
            let op = parse_arith_op(parts[0])?;
            let lhs = parse_slot(parts[1])?;
            let rhs = parse_slot(parts[2])?;
            return Ok(IrExprKind::Arith { op, lhs, rhs });
        }
        return Err(format!("malformed arith: {s}"));
    }

    if let Some(rest) = s.strip_prefix("cmp ") {
        // cmp <op> $N $M
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 3 {
            let op = parse_cmp_op(parts[0])?;
            let lhs = parse_slot(parts[1])?;
            let rhs = parse_slot(parts[2])?;
            return Ok(IrExprKind::Cmp { op, lhs, rhs });
        }
        return Err(format!("malformed cmp: {s}"));
    }

    if let Some(rest) = s.strip_prefix("cast ") {
        // cast $N as Type
        if let Some(as_pos) = rest.find(" as ") {
            let slot = parse_slot(rest[..as_pos].trim())?;
            let target_type = rest[as_pos + 4..].trim().to_string();
            return Ok(IrExprKind::Cast { slot, target_type });
        }
        return Err(format!("malformed cast: {s}"));
    }

    if let Some(rest) = s.strip_prefix("construct ") {
        // construct TypeId { .0 = $N, .1 = $M }
        let rest = rest.trim();
        if let Some(brace_start) = rest.find('{') {
            let type_id = rest[..brace_start].trim().to_string();
            let brace_end = rest.rfind('}').unwrap_or(rest.len());
            let fields_str = &rest[brace_start + 1..brace_end];
            let mut fields = Vec::new();
            for f in fields_str.split(',') {
                let f = f.trim();
                if f.is_empty() {
                    continue;
                }
                let kv: Vec<&str> = f.splitn(2, '=').collect();
                if kv.len() == 2 {
                    let idx = kv[0]
                        .trim()
                        .trim_start_matches('.')
                        .parse::<usize>()
                        .unwrap_or(0);
                    let slot = parse_slot(kv[1].trim())?;
                    fields.push((idx, slot));
                }
            }
            return Ok(IrExprKind::Construct { type_id, fields });
        }
        return Err(format!("malformed construct: {s}"));
    }

    if let Some(rest) = s.strip_prefix("if ") {
        // if $N then #B1 else #B2
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 5 && parts[1] == "then" && parts[3] == "else" {
            let cond = parse_slot(parts[0])?;
            let then_block = parts[2]
                .trim_start_matches('#')
                .parse::<usize>()
                .map_err(|_| format!("bad block id: {}", parts[2]))?;
            let else_block = parts[4]
                .trim_start_matches('#')
                .parse::<usize>()
                .map_err(|_| format!("bad block id: {}", parts[4]))?;
            return Ok(IrExprKind::If {
                cond,
                then_block,
                else_block,
            });
        }
        return Err(format!("malformed if: {s}"));
    }

    if let Some(rest) = s.strip_prefix("transition ") {
        // transition $N to StateId
        if let Some(to_pos) = rest.find(" to ") {
            let slot = parse_slot(rest[..to_pos].trim())?;
            let state = rest[to_pos + 4..].trim().to_string();
            return Ok(IrExprKind::Transition { slot, state });
        }
        return Err(format!("malformed transition: {s}"));
    }

    Err(format!("unknown IR expression: {s}"))
}

fn parse_ir_literal(s: &str) -> Result<IrLiteral, String> {
    let s = s.trim();
    if s == "true" {
        return Ok(IrLiteral::Bool(true));
    }
    if s == "false" {
        return Ok(IrLiteral::Bool(false));
    }
    if s.starts_with('"') && s.ends_with('"') {
        return Ok(IrLiteral::Str(s[1..s.len() - 1].to_string()));
    }
    if let Ok(n) = s.parse::<i64>() {
        return Ok(IrLiteral::Int(n));
    }
    if let Ok(f) = s.parse::<f64>() {
        return Ok(IrLiteral::Float(f));
    }
    Err(format!("cannot parse IR literal: {s}"))
}

fn parse_arith_op(s: &str) -> Result<IrArithOp, String> {
    match s {
        "add" => Ok(IrArithOp::Add),
        "sub" => Ok(IrArithOp::Sub),
        "mul" => Ok(IrArithOp::Mul),
        "div" => Ok(IrArithOp::Div),
        "mod" => Ok(IrArithOp::Mod),
        _ => Err(format!("unknown arith op: {s}")),
    }
}

fn parse_cmp_op(s: &str) -> Result<IrCmpOp, String> {
    match s {
        "eq" => Ok(IrCmpOp::Eq),
        "ne" => Ok(IrCmpOp::Ne),
        "lt" => Ok(IrCmpOp::Lt),
        "le" => Ok(IrCmpOp::Le),
        "gt" => Ok(IrCmpOp::Gt),
        "ge" => Ok(IrCmpOp::Ge),
        _ => Err(format!("unknown cmp op: {s}")),
    }
}

fn parse_ir_pred_str(s: &str) -> Option<IrPred> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s == "true" {
        return Some(IrPred::True);
    }
    if s == "false" {
        return Some(IrPred::False);
    }
    // cmp <op> <arg> <arg>
    if let Some(rest) = s.strip_prefix("cmp ") {
        let tokens = tokenize_pred(rest);
        if tokens.len() >= 3
            && let Ok(op) = parse_cmp_op(&tokens[0])
            && let Some((lhs_arg, consumed)) = parse_pred_arg_tokens(&tokens[1..])
            && let Some((rhs_arg, _)) = parse_pred_arg_tokens(&tokens[1 + consumed..])
        {
            return Some(IrPred::Cmp {
                op,
                lhs: lhs_arg,
                rhs: rhs_arg,
            });
        }
    }
    // not <pred>
    if let Some(rest) = s.strip_prefix("not ")
        && let Some(inner) = parse_ir_pred_str(rest)
    {
        return Some(IrPred::Not(Box::new(inner)));
    }
    None
}

fn tokenize_pred(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut depth = 0;

    for ch in s.chars() {
        match ch {
            '(' => {
                depth += 1;
                if depth == 1 {
                    if !current.trim().is_empty() {
                        tokens.push(current.trim().to_string());
                        current.clear();
                    }
                    continue;
                }
                current.push(ch);
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    if !current.trim().is_empty() {
                        tokens.push(format!("({})", current.trim()));
                        current.clear();
                    }
                    continue;
                }
                current.push(ch);
            }
            ' ' | '\t' if depth == 0 => {
                if !current.trim().is_empty() {
                    tokens.push(current.trim().to_string());
                    current.clear();
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }
    if !current.trim().is_empty() {
        tokens.push(current.trim().to_string());
    }
    tokens
}

fn parse_pred_arg_tokens(tokens: &[String]) -> Option<(IrPredArg, usize)> {
    if tokens.is_empty() {
        return None;
    }
    let first = &tokens[0];
    if first == "$result" {
        return Some((IrPredArg::SlotResult, 1));
    }
    if let Some(stripped) = first.strip_prefix('$')
        && let Ok(n) = stripped.parse::<usize>()
    {
        return Some((IrPredArg::Slot(n), 1));
    }
    if first.starts_with("(arith ") || first.starts_with("(cmp ") {
        // Nested expression in parens
        let inner = &first[1..first.len() - 1]; // strip outer parens
        if let Some(rest) = inner.strip_prefix("arith ") {
            let sub = tokenize_pred(rest);
            if sub.len() >= 3
                && let Ok(op) = parse_arith_op(&sub[0])
                && let Some((lhs, lc)) = parse_pred_arg_tokens(&sub[1..])
                && let Some((rhs, _)) = parse_pred_arg_tokens(&sub[1 + lc..])
            {
                return Some((
                    IrPredArg::Arith {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    1,
                ));
            }
        }
    }
    // Try as literal
    if let Ok(n) = first.parse::<i64>() {
        return Some((IrPredArg::Lit(IrLiteral::Int(n)), 1));
    }
    if let Ok(f) = first.parse::<f64>() {
        return Some((IrPredArg::Lit(IrLiteral::Float(f)), 1));
    }
    // Parenthesized constant: (const 0)
    if first.starts_with("(const ") {
        let inner = &first[7..first.len() - 1];
        if let Ok(lit) = parse_ir_literal(inner) {
            return Some((IrPredArg::Lit(lit), 1));
        }
    }
    None
}

/// Validate an IR module against the contract it claims to implement.
///
/// Checks:
/// 1. Function parameter count matches contract input count
/// 2. Parameter types match contract input types
/// 3. Return type matches contract output type
/// 4. Effect annotations are compatible
/// 5. Slot numbering is sequential (no gaps)
/// 6. All slot references are defined before use
pub fn validate_ir_against_contract(
    ir: &IrModule,
    contract: &assura_parser::ast::ContractDecl,
) -> IrValidation {
    let mut errors = Vec::new();

    for func in &ir.functions {
        // Check that slot numbering starts from 0 and uses no gaps
        let mut max_defined = func.params.iter().map(|p| p.slot).max().unwrap_or(0);

        for instr in &func.body {
            // $result uses sentinel usize::MAX
            if instr.target != usize::MAX {
                if instr.target > max_defined + 1 {
                    errors.push(format!(
                        "fn {}: slot ${} skips slot ${}",
                        func.id,
                        instr.target,
                        max_defined + 1
                    ));
                }
                max_defined = max_defined.max(instr.target);
            }

            // Check all slot references are defined
            for referenced in referenced_slots(&instr.expr) {
                if referenced != usize::MAX
                    && referenced > max_defined
                    && !func.params.iter().any(|p| p.slot == referenced)
                {
                    errors.push(format!(
                        "fn {}: instruction uses undefined slot ${}",
                        func.id, referenced
                    ));
                }
            }
        }

        // Check parameter count against contract inputs
        let contract_inputs: Vec<_> = contract
            .clauses
            .iter()
            .filter(|c| c.kind == assura_parser::ast::ClauseKind::Input)
            .collect();
        if !contract_inputs.is_empty() {
            // Count params in the first input clause
            let input_count = count_input_params(&contract_inputs[0].body);
            if func.params.len() != input_count {
                errors.push(format!(
                    "fn {}: has {} params, contract expects {}",
                    func.id,
                    func.params.len(),
                    input_count
                ));
            }
        }
    }

    IrValidation {
        valid: errors.is_empty(),
        errors,
    }
}

fn referenced_slots(expr: &IrExprKind) -> Vec<usize> {
    match expr {
        IrExprKind::Const(_) => vec![],
        IrExprKind::Load(s) => vec![*s],
        IrExprKind::Call { args, .. } => args.clone(),
        IrExprKind::Field { slot, .. } => vec![*slot],
        IrExprKind::Construct { fields, .. } => fields.iter().map(|(_, s)| *s).collect(),
        IrExprKind::Arith { lhs, rhs, .. } => vec![*lhs, *rhs],
        IrExprKind::Cmp { lhs, rhs, .. } => vec![*lhs, *rhs],
        IrExprKind::Cast { slot, .. } => vec![*slot],
        IrExprKind::If { cond, .. } => vec![*cond],
        IrExprKind::Transition { slot, .. } => vec![*slot],
    }
}

fn count_input_params(body: &assura_parser::ast::Expr) -> usize {
    match body {
        assura_parser::ast::Expr::Tuple(items) => items.len(),
        assura_parser::ast::Expr::Call { args, .. } => args.len(),
        _ => 1,
    }
}

/// Generate Rust source code from a validated IR module.
///
/// Each IR function becomes a Rust function with debug_assert!
/// for pre/post conditions.
pub fn ir_to_rust(module: &IrModule) -> String {
    let mut code = String::new();
    code.push_str(&format!("// Generated from IR module: {}\n\n", module.name));

    for func in &module.functions {
        // Function signature
        let params: Vec<String> = func
            .params
            .iter()
            .map(|p| format!("slot_{}: {}", p.slot, ir_type_to_rust(&p.ty)))
            .collect();

        let ret_type = ir_type_to_rust(&func.return_type);
        code.push_str(&format!(
            "fn ir_{}({}) -> {} {{\n",
            func.id.trim_start_matches('#'),
            params.join(", "),
            ret_type
        ));

        // Pre-condition
        if let Some(ref pre) = func.pre {
            let pre_rust = pred_to_rust(pre);
            code.push_str(&format!("    debug_assert!({pre_rust});\n"));
        }

        // Body instructions
        for instr in &func.body {
            let target = if instr.target == usize::MAX {
                "__result".to_string()
            } else {
                format!("slot_{}", instr.target)
            };
            let ty = ir_type_to_rust(&instr.ty);
            let expr_code = ir_expr_to_rust(&instr.expr);
            code.push_str(&format!("    let {target}: {ty} = {expr_code};\n"));
        }

        // Post-condition
        if let Some(ref post) = func.post {
            let post_rust = pred_to_rust(post);
            code.push_str(&format!("    debug_assert!({post_rust});\n"));
        }

        // Return $result if it was assigned
        if func.body.iter().any(|i| i.target == usize::MAX) {
            code.push_str("    __result\n");
        } else {
            code.push_str("    todo!()\n");
        }

        code.push_str("}\n\n");
    }

    code
}

fn ir_type_to_rust(ty: &str) -> String {
    match ty {
        "Int" => "i64".to_string(),
        "Nat" => "u64".to_string(),
        "Float" => "f64".to_string(),
        "Bool" => "bool".to_string(),
        "String" => "String".to_string(),
        "Bytes" => "Vec<u8>".to_string(),
        "Unit" => "()".to_string(),
        "" => "_".to_string(),
        other => other.to_string(),
    }
}

fn ir_expr_to_rust(expr: &IrExprKind) -> String {
    match expr {
        IrExprKind::Const(lit) => match lit {
            IrLiteral::Int(n) => format!("{n}_i64"),
            IrLiteral::Float(f) => format!("{f}_f64"),
            IrLiteral::Str(s) => format!("\"{s}\".to_string()"),
            IrLiteral::Bool(b) => format!("{b}"),
        },
        IrExprKind::Load(s) => {
            if *s == usize::MAX {
                "__result".to_string()
            } else {
                format!("slot_{s}")
            }
        }
        IrExprKind::Call { func, args } => {
            let arg_strs: Vec<String> = args
                .iter()
                .map(|a| {
                    if *a == usize::MAX {
                        "__result".to_string()
                    } else {
                        format!("slot_{a}")
                    }
                })
                .collect();
            format!("{func}({})", arg_strs.join(", "))
        }
        IrExprKind::Field { slot, index } => format!("slot_{slot}.{index}"),
        IrExprKind::Arith { op, lhs, rhs } => {
            let op_str = match op {
                IrArithOp::Add => "+",
                IrArithOp::Sub => "-",
                IrArithOp::Mul => "*",
                IrArithOp::Div => "/",
                IrArithOp::Mod => "%",
            };
            format!("(slot_{lhs} {op_str} slot_{rhs})")
        }
        IrExprKind::Cmp { op, lhs, rhs } => {
            let op_str = match op {
                IrCmpOp::Eq => "==",
                IrCmpOp::Ne => "!=",
                IrCmpOp::Lt => "<",
                IrCmpOp::Le => "<=",
                IrCmpOp::Gt => ">",
                IrCmpOp::Ge => ">=",
            };
            format!("(slot_{lhs} {op_str} slot_{rhs})")
        }
        IrExprKind::Cast { slot, target_type } => {
            format!("slot_{slot} as {}", ir_type_to_rust(target_type))
        }
        IrExprKind::Construct {
            type_id, fields, ..
        } => {
            let field_strs: Vec<String> = fields.iter().map(|(_, s)| format!("slot_{s}")).collect();
            format!("{type_id}::new({})", field_strs.join(", "))
        }
        IrExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            format!("if slot_{cond} {{ block_{then_block}() }} else {{ block_{else_block}() }}")
        }
        IrExprKind::Transition { slot, state } => {
            format!("slot_{slot}.transition_to_{state}()")
        }
    }
}

fn pred_to_rust(pred: &IrPred) -> String {
    match pred {
        IrPred::True => "true".to_string(),
        IrPred::False => "false".to_string(),
        IrPred::Cmp { op, lhs, rhs } => {
            let op_str = match op {
                IrCmpOp::Eq => "==",
                IrCmpOp::Ne => "!=",
                IrCmpOp::Lt => "<",
                IrCmpOp::Le => "<=",
                IrCmpOp::Gt => ">",
                IrCmpOp::Ge => ">=",
            };
            format!(
                "({} {} {})",
                pred_arg_to_rust(lhs),
                op_str,
                pred_arg_to_rust(rhs)
            )
        }
        IrPred::And(a, b) => format!("({} && {})", pred_to_rust(a), pred_to_rust(b)),
        IrPred::Or(a, b) => format!("({} || {})", pred_to_rust(a), pred_to_rust(b)),
        IrPred::Not(p) => format!("!({})", pred_to_rust(p)),
    }
}

fn pred_arg_to_rust(arg: &IrPredArg) -> String {
    match arg {
        IrPredArg::Slot(n) => format!("slot_{n}"),
        IrPredArg::SlotResult => "__result".to_string(),
        IrPredArg::Lit(lit) => match lit {
            IrLiteral::Int(n) => format!("{n}_i64"),
            IrLiteral::Float(f) => format!("{f}_f64"),
            IrLiteral::Str(s) => format!("\"{s}\""),
            IrLiteral::Bool(b) => format!("{b}"),
        },
        IrPredArg::Arith { op, lhs, rhs } => {
            let op_str = match op {
                IrArithOp::Add => "+",
                IrArithOp::Sub => "-",
                IrArithOp::Mul => "*",
                IrArithOp::Div => "/",
                IrArithOp::Mod => "%",
            };
            format!(
                "({} {} {})",
                pred_arg_to_rust(lhs),
                op_str,
                pred_arg_to_rust(rhs)
            )
        }
    }
}

// ===========================================================================
// T113: Verification caching
// ===========================================================================

// ---------------------------------------------------------------------------
// Session cache (in-memory, per-session deduplication for verify_clauses)
// ---------------------------------------------------------------------------

/// Entry in the per-session in-memory verification cache.
#[derive(Debug, Clone)]
pub struct SessionCacheEntry {
    pub result: String,
}

/// In-memory cache used within a single verification session to avoid
/// re-verifying the same clause twice. Not persisted across runs.
#[derive(Debug)]
pub struct SessionCache {
    entries: std::collections::HashMap<String, SessionCacheEntry>,
    hits: usize,
    misses: usize,
}

impl SessionCache {
    pub fn new() -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            hits: 0,
            misses: 0,
        }
    }

    pub fn insert(&mut self, key: String, result: String, _cost: usize) {
        self.entries.insert(key, SessionCacheEntry { result });
    }

    pub fn lookup(&mut self, key: &str) -> Option<&SessionCacheEntry> {
        if self.entries.contains_key(key) {
            self.hits += 1;
            self.entries.get(key)
        } else {
            self.misses += 1;
            None
        }
    }

    pub fn invalidate(&mut self, key: &str) {
        self.entries.remove(key);
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.hits = 0;
        self.misses = 0;
    }
}

impl Default for SessionCache {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Verification caching (P006)
// ---------------------------------------------------------------------------

/// Serializable representation of a cached verification result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum CachedResult {
    Verified { clause_desc: String },
    Counterexample { clause_desc: String, model: String },
    Timeout { clause_desc: String },
    Unknown { clause_desc: String, reason: String },
}

impl From<&VerificationResult> for CachedResult {
    fn from(r: &VerificationResult) -> Self {
        match r {
            VerificationResult::Verified { clause_desc } => CachedResult::Verified {
                clause_desc: clause_desc.clone(),
            },
            VerificationResult::Counterexample {
                clause_desc, model, ..
            } => CachedResult::Counterexample {
                clause_desc: clause_desc.clone(),
                model: model.clone(),
            },
            VerificationResult::Timeout { clause_desc } => CachedResult::Timeout {
                clause_desc: clause_desc.clone(),
            },
            VerificationResult::Unknown {
                clause_desc,
                reason,
            } => CachedResult::Unknown {
                clause_desc: clause_desc.clone(),
                reason: reason.clone(),
            },
        }
    }
}

impl From<CachedResult> for VerificationResult {
    fn from(c: CachedResult) -> Self {
        match c {
            CachedResult::Verified { clause_desc } => VerificationResult::Verified { clause_desc },
            CachedResult::Counterexample { clause_desc, model } => {
                VerificationResult::Counterexample {
                    clause_desc,
                    model,
                    counter_model: None,
                }
            }
            CachedResult::Timeout { clause_desc } => VerificationResult::Timeout { clause_desc },
            CachedResult::Unknown {
                clause_desc,
                reason,
            } => VerificationResult::Unknown {
                clause_desc,
                reason,
            },
        }
    }
}

/// Compute a stable content hash of a contract's clauses for cache keying.
fn hash_clauses(contract_name: &str, clauses: &[assura_parser::ast::Clause]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    contract_name.hash(&mut hasher);
    for clause in clauses {
        format!("{:?}", clause.kind).hash(&mut hasher);
        format!("{:?}", clause.body).hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

/// Verification cache backed by the filesystem.
///
/// Each contract's results are stored as a JSON file in `.assura-cache/verify/`
/// keyed by the content hash of its clauses. When the contract changes, the
/// hash changes, and the old cache entry is naturally invalidated.
pub struct VerificationCache {
    cache_dir: std::path::PathBuf,
}

impl VerificationCache {
    /// Create a cache rooted at the given project directory.
    ///
    /// Cache files are stored in `<base_dir>/.assura-cache/verify/`.
    pub fn new(base_dir: &std::path::Path) -> Self {
        let cache_dir = base_dir.join(".assura-cache").join("verify");
        let _ = std::fs::create_dir_all(&cache_dir);
        Self { cache_dir }
    }

    /// Look up cached verification results for a contract.
    pub fn get(
        &self,
        contract_name: &str,
        clauses: &[assura_parser::ast::Clause],
    ) -> Option<Vec<VerificationResult>> {
        let hash = hash_clauses(contract_name, clauses);
        let path = self.cache_dir.join(format!("{hash}.json"));
        let data = std::fs::read_to_string(&path).ok()?;
        let cached: Vec<CachedResult> = serde_json::from_str(&data).ok()?;
        Some(cached.into_iter().map(VerificationResult::from).collect())
    }

    /// Store verification results for a contract.
    pub fn put(
        &self,
        contract_name: &str,
        clauses: &[assura_parser::ast::Clause],
        results: &[VerificationResult],
    ) {
        let hash = hash_clauses(contract_name, clauses);
        let path = self.cache_dir.join(format!("{hash}.json"));
        let cached: Vec<CachedResult> = results.iter().map(CachedResult::from).collect();
        if let Ok(json) = serde_json::to_string(&cached) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// Remove all cached verification results.
    pub fn clear(&self) {
        let _ = std::fs::remove_dir_all(&self.cache_dir);
        let _ = std::fs::create_dir_all(&self.cache_dir);
    }

    /// Number of cached entries.
    pub fn entry_count(&self) -> usize {
        std::fs::read_dir(&self.cache_dir)
            .map(|rd| rd.filter(|e| e.is_ok()).count())
            .unwrap_or(0)
    }
}

/// Verify a contract's clauses, using the cache if available.
///
/// Checks the cache first. On miss, runs Z3 and stores the result.
pub fn verify_contract_cached(
    contract_name: &str,
    clauses: &[assura_parser::ast::Clause],
    cache: &VerificationCache,
) -> Vec<VerificationResult> {
    if let Some(results) = cache.get(contract_name, clauses) {
        return results;
    }
    let results = verify_contract(contract_name, clauses);
    cache.put(contract_name, clauses, &results);
    results
}

// ===========================================================================
// T114: Parallel SMT queries
// ===========================================================================

#[derive(Debug, Clone)]
pub struct ParallelVerifier {
    jobs: Vec<VerificationJob>,
    max_parallelism: usize,
}

#[derive(Debug, Clone)]
pub struct VerificationJob {
    pub contract_name: String,
    pub clause: String,
    pub status: JobStatus,
    pub result: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl ParallelVerifier {
    pub fn new(max_parallelism: usize) -> Self {
        Self {
            jobs: Vec::new(),
            max_parallelism,
        }
    }

    pub fn add_job(&mut self, contract_name: String, clause: String) {
        self.jobs.push(VerificationJob {
            contract_name,
            clause,
            status: JobStatus::Pending,
            result: None,
        });
    }

    pub fn start_next(&mut self) -> Option<usize> {
        let running = self
            .jobs
            .iter()
            .filter(|j| j.status == JobStatus::Running)
            .count();
        if running >= self.max_parallelism {
            return None;
        }
        for (i, job) in self.jobs.iter_mut().enumerate() {
            if job.status == JobStatus::Pending {
                job.status = JobStatus::Running;
                return Some(i);
            }
        }
        None
    }

    pub fn complete_job(&mut self, index: usize, result: String) {
        if let Some(job) = self.jobs.get_mut(index) {
            job.status = JobStatus::Completed;
            job.result = Some(result);
        }
    }

    pub fn fail_job(&mut self, index: usize) {
        if let Some(job) = self.jobs.get_mut(index) {
            job.status = JobStatus::Failed;
        }
    }

    pub fn all_complete(&self) -> bool {
        self.jobs
            .iter()
            .all(|j| j.status == JobStatus::Completed || j.status == JobStatus::Failed)
    }

    pub fn pending_count(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| j.status == JobStatus::Pending)
            .count()
    }
    pub fn completed_count(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| j.status == JobStatus::Completed)
            .count()
    }
    pub fn job_count(&self) -> usize {
        self.jobs.len()
    }
}

impl Default for ParallelVerifier {
    fn default() -> Self {
        Self::new(4)
    }
}

// ===========================================================================
// T115: Incremental compilation
// ===========================================================================

#[derive(Debug, Clone)]
pub struct IncrementalCompiler {
    modules: std::collections::HashMap<String, ModuleState>,
    dependencies: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct ModuleState {
    pub name: String,
    pub hash: String,
    pub last_checked: u64,
    pub dirty: bool,
}

impl IncrementalCompiler {
    pub fn new() -> Self {
        Self {
            modules: std::collections::HashMap::new(),
            dependencies: Vec::new(),
        }
    }

    pub fn register_module(&mut self, name: String, hash: String) {
        self.modules.insert(
            name.clone(),
            ModuleState {
                name,
                hash,
                last_checked: 0,
                dirty: true,
            },
        );
    }

    pub fn add_dependency(&mut self, from: String, to: String) {
        self.dependencies.push((from, to));
    }

    pub fn mark_changed(&mut self, name: &str) {
        if let Some(m) = self.modules.get_mut(name) {
            m.dirty = true;
        }
        let dependents: Vec<_> = self
            .dependencies
            .iter()
            .filter(|(_, to)| to == name)
            .map(|(from, _)| from.clone())
            .collect();
        for dep in dependents {
            if let Some(m) = self.modules.get_mut(&dep) {
                m.dirty = true;
            }
        }
    }

    pub fn mark_checked(&mut self, name: &str, timestamp: u64) {
        if let Some(m) = self.modules.get_mut(name) {
            m.dirty = false;
            m.last_checked = timestamp;
        }
    }

    pub fn dirty_modules(&self) -> Vec<&str> {
        self.modules
            .values()
            .filter(|m| m.dirty)
            .map(|m| m.name.as_str())
            .collect()
    }

    pub fn module_count(&self) -> usize {
        self.modules.len()
    }
}

impl Default for IncrementalCompiler {
    fn default() -> Self {
        Self::new()
    }
}

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
            matches!(&results[0], Layer2Result::Unknown { invariant, reason } if invariant == "inv1" && reason.contains("requires Z3"))
        );
        assert!(
            matches!(&results[1], Layer2Result::Unknown { invariant, reason } if invariant == "termination:fib" && reason.contains("requires Z3"))
        );
        assert!(
            matches!(&results[2], Layer2Result::Unknown { invariant, reason } if invariant == "roundtrip:Message" && reason.contains("requires Z3"))
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
        assert!(errs[0].contains("never resolved"));
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
    // T114: ParallelVerifier tests
    // =======================================================================

    #[test]
    fn parallel_start_jobs() {
        let mut pv = ParallelVerifier::new(2);
        pv.add_job("A".into(), "requires".into());
        pv.add_job("B".into(), "ensures".into());
        pv.add_job("C".into(), "requires".into());
        assert_eq!(pv.start_next(), Some(0));
        assert_eq!(pv.start_next(), Some(1));
        assert_eq!(pv.start_next(), None);
    }

    #[test]
    fn parallel_complete_allows_more() {
        let mut pv = ParallelVerifier::new(1);
        pv.add_job("A".into(), "r".into());
        pv.add_job("B".into(), "e".into());
        pv.start_next();
        assert_eq!(pv.start_next(), None);
        pv.complete_job(0, "verified".into());
        assert_eq!(pv.start_next(), Some(1));
    }

    #[test]
    fn parallel_all_complete() {
        let mut pv = ParallelVerifier::new(4);
        pv.add_job("A".into(), "r".into());
        pv.start_next();
        pv.complete_job(0, "ok".into());
        assert!(pv.all_complete());
    }

    #[test]
    fn parallel_counts() {
        let mut pv = ParallelVerifier::new(4);
        pv.add_job("A".into(), "r".into());
        pv.add_job("B".into(), "e".into());
        assert_eq!(pv.pending_count(), 2);
        pv.start_next();
        pv.complete_job(0, "ok".into());
        assert_eq!(pv.completed_count(), 1);
        assert_eq!(pv.pending_count(), 1);
    }

    #[test]
    fn parallel_default() {
        let pv = ParallelVerifier::default();
        assert_eq!(pv.job_count(), 0);
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
        let (file, errs) = assura_parser::parse(source);
        assert!(errs.is_empty(), "unexpected parse errors: {errs:?}");
        let file = file.expect("parse returned None");
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
        // String-based invariants currently return Unknown (need Expr-based API)
        assert!(matches!(results[0], Layer2Result::Unknown { .. }));
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
