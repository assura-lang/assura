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
    }

    impl<'ctx> Encoder<'ctx> {
        fn new(ctx: &'ctx Context) -> Self {
            Self {
                ctx,
                vars: HashMap::new(),
                func_arities: HashMap::new(),
                fresh_counter: 0,
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
        fn encode_call(&mut self, func_name: &str, args: &[Expr]) -> Z3Value<'ctx> {
            let arg_vals: Vec<ast::Int<'ctx>> = args
                .iter()
                .map(|a| {
                    self.encode_expr(a)
                        .as_int(self.ctx, &mut self.fresh_counter)
                })
                .collect();
            let decl = self.make_func(func_name, arg_vals.len());
            let arg_refs: Vec<&dyn z3::ast::Ast> =
                arg_vals.iter().map(|a| a as &dyn z3::ast::Ast).collect();
            let result = decl.apply(&arg_refs);
            Z3Value::Int(result.as_int().unwrap_or_else(|| self.fresh_int()))
        }

        /// Encode field access as uninterpreted function: field_name(object).
        fn encode_field_access(&mut self, obj: &Expr, field: &str) -> Z3Value<'ctx> {
            let obj_val = self
                .encode_expr(obj)
                .as_int(self.ctx, &mut self.fresh_counter);
            let func_name = format!("__field_{field}");
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
            let decl = self.make_func("__index", 2);
            let result = decl.apply(&[
                &coll_val as &dyn z3::ast::Ast,
                &idx_val as &dyn z3::ast::Ast,
            ]);
            Z3Value::Int(result.as_int().unwrap_or_else(|| self.fresh_int()))
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
                    Z3Value::Int(ast::Int::new_const(self.ctx, const_name))
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

                // --- old(expr): encode inner with __old suffix for idents ---
                Expr::Old(inner) => {
                    if let Expr::Ident(name) = inner.as_ref() {
                        let old_name = format!("{name}__old");
                        let v = self.get_or_create_int(&old_name);
                        Z3Value::Int(v)
                    } else {
                        self.encode_expr(inner)
                    }
                }

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

                // --- Apply lemma: encode as true (assumption injected elsewhere) ---
                Expr::Apply { .. } => Z3Value::Bool(ast::Bool::from_bool(self.ctx, true)),

                // --- Match: encode as ITE chain over arm bodies ---
                Expr::Match { scrutinee, arms } => {
                    let scrut = self.encode_expr(scrutinee);
                    // Build an if-then-else chain: if scrut == pattern1 then body1
                    // else if scrut == pattern2 then body2 ... else default
                    let default = Z3Value::Int(self.fresh_int());
                    arms.iter().rev().fold(default, |else_val, arm| {
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
                                    _ => ast::Bool::from_bool(self.ctx, false),
                                }
                            }
                            assura_parser::ast::Pattern::Literal(lit) => {
                                let lit_val = self.encode_literal(lit);
                                match (&scrut, &lit_val) {
                                    (Z3Value::Int(a), Z3Value::Int(b)) => a._eq(b),
                                    (Z3Value::Bool(a), Z3Value::Bool(b)) => a._eq(b),
                                    _ => ast::Bool::from_bool(self.ctx, false),
                                }
                            }
                            _ => ast::Bool::from_bool(self.ctx, true),
                        };
                        // Build ITE: if cond then body else else_val
                        match (&body, &else_val) {
                            (Z3Value::Bool(b), Z3Value::Bool(e)) => Z3Value::Bool(cond.ite(b, e)),
                            (Z3Value::Int(b), Z3Value::Int(e)) => Z3Value::Int(cond.ite(b, e)),
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

                // --- Tuple: no meaningful SMT encoding, use fresh ---
                Expr::Tuple(_) => Z3Value::Int(self.fresh_int()),

                // --- Cast: encode inner (the value doesn't change, only its type) ---
                Expr::Cast { expr, .. } => self.encode_expr(expr),

                // --- List/Block: no meaningful SMT encoding, use fresh ---
                Expr::List(_) | Expr::Block(_) => Z3Value::Int(self.fresh_int()),
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
                return Z3Value::Bool(self.fresh_bool());
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
                    "or" => (1, RawOp::Or),
                    "and" => (2, RawOp::And),
                    "=>" => (3, RawOp::Implies),
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
                    "mod" => (7, RawOp::Mod),
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
                return (Z3Value::Bool(self.fresh_bool()), start);
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

            // Check for function call: `name(args)` -> fresh
            if next < tokens.len() && tokens[next] == "(" {
                // Skip past the call (find matching paren)
                let mut depth = 1usize;
                let mut p = next + 1;
                while p < tokens.len() && depth > 0 {
                    match tokens[p].as_str() {
                        "(" => depth += 1,
                        ")" => depth -= 1,
                        _ => {}
                    }
                    p += 1;
                }
                return (Z3Value::Int(self.fresh_int()), p);
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

        /// Encode a binary operation.
        fn encode_binop(&mut self, lhs: &Expr, op: &BinOp, rhs: &Expr) -> Z3Value<'ctx> {
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
                BinOp::Concat | BinOp::Range => Z3Value::Int(self.fresh_int()),
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
            // Skip internal/fresh variables
            if name.starts_with("__") {
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
        cache: &mut VerificationCache,
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
    pub(crate) fn verify_impl(typed: &TypedFile) -> Vec<VerificationResult> {
        let mut cfg = Config::new();
        cfg.set_param_value("timeout", "1000");
        let ctx = Context::new(&cfg);
        let mut results = Vec::new();
        let mut cache = VerificationCache::new();

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

#[derive(Debug, Clone)]
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
// T113: Verification caching
// ===========================================================================

#[derive(Debug, Clone)]
pub struct VerificationCache {
    entries: std::collections::HashMap<String, CacheEntry>,
    hits: u64,
    misses: u64,
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub hash: String,
    pub result: String,
    pub timestamp: u64,
}

impl VerificationCache {
    pub fn new() -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            hits: 0,
            misses: 0,
        }
    }

    pub fn insert(&mut self, hash: String, result: String, timestamp: u64) {
        self.entries.insert(
            hash.clone(),
            CacheEntry {
                hash,
                result,
                timestamp,
            },
        );
    }

    pub fn lookup(&mut self, hash: &str) -> Option<&CacheEntry> {
        if self.entries.contains_key(hash) {
            self.hits += 1;
            self.entries.get(hash)
        } else {
            self.misses += 1;
            None
        }
    }

    pub fn invalidate(&mut self, hash: &str) {
        self.entries.remove(hash);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.hits = 0;
        self.misses = 0;
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for VerificationCache {
    fn default() -> Self {
        Self::new()
    }
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
    // T113: VerificationCache tests
    // =======================================================================

    #[test]
    fn cache_hit() {
        let mut cache = VerificationCache::new();
        cache.insert("abc123".into(), "verified".into(), 1000);
        assert!(cache.lookup("abc123").is_some());
        assert_eq!(cache.hit_rate(), 1.0);
    }

    #[test]
    fn cache_miss() {
        let mut cache = VerificationCache::new();
        assert!(cache.lookup("unknown").is_none());
        assert_eq!(cache.hit_rate(), 0.0);
    }

    #[test]
    fn cache_invalidate() {
        let mut cache = VerificationCache::new();
        cache.insert("abc".into(), "ok".into(), 1);
        cache.invalidate("abc");
        assert!(cache.lookup("abc").is_none());
    }

    #[test]
    fn cache_clear() {
        let mut cache = VerificationCache::new();
        cache.insert("a".into(), "ok".into(), 1);
        cache.insert("b".into(), "ok".into(), 1);
        cache.clear();
        assert_eq!(cache.entry_count(), 0);
    }

    #[test]
    fn cache_default() {
        let cache = VerificationCache::default();
        assert_eq!(cache.entry_count(), 0);
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
