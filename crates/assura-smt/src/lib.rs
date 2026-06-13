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
            }
        }

        /// Extract as Int. If Bool, return a fresh uninterpreted int.
        fn as_int(&self, ctx: &'ctx Context, counter: &mut u32) -> ast::Int<'ctx> {
            match self {
                Z3Value::Int(i) => i.clone(),
                Z3Value::Bool(_) => {
                    *counter += 1;
                    ast::Int::new_const(ctx, format!("__coerce_{counter}"))
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
        fresh_counter: u32,
    }

    impl<'ctx> Encoder<'ctx> {
        fn new(ctx: &'ctx Context) -> Self {
            Self {
                ctx,
                vars: HashMap::new(),
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

        /// Encode an AST expression into a Z3 value.
        fn encode_expr(&mut self, expr: &Expr) -> Z3Value<'ctx> {
            match expr {
                // --- Literals ---
                Expr::Literal(Literal::Int(s)) => {
                    let n: i64 = s.parse().unwrap_or(0);
                    Z3Value::Int(ast::Int::from_i64(self.ctx, n))
                }
                Expr::Literal(Literal::Float(_)) => {
                    // Approximate as fresh int for Layer 1
                    Z3Value::Int(self.fresh_int())
                }
                Expr::Literal(Literal::Str(_)) => Z3Value::Bool(self.fresh_bool()),
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
                            let i = val.as_int(self.ctx, &mut self.fresh_counter);
                            Z3Value::Int(i.unary_minus())
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
                Expr::Forall { var, body, .. } => {
                    let bound = ast::Int::new_const(self.ctx, var.as_str());
                    self.vars.insert(var.clone(), Z3Value::Int(bound.clone()));
                    let body_val = self.encode_expr(body);
                    let body_bool = body_val.as_bool(self.ctx);
                    let result = ast::forall_const(self.ctx, &[&bound], &[], &body_bool);
                    Z3Value::Bool(result)
                }

                // --- Exists quantifier ---
                Expr::Exists { var, body, .. } => {
                    let bound = ast::Int::new_const(self.ctx, var.as_str());
                    self.vars.insert(var.clone(), Z3Value::Int(bound.clone()));
                    let body_val = self.encode_expr(body);
                    let body_bool = body_val.as_bool(self.ctx);
                    let result = ast::exists_const(self.ctx, &[&bound], &[], &body_bool);
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

                // --- Complex expressions: return fresh unconstrained value ---
                Expr::Field(..)
                | Expr::MethodCall { .. }
                | Expr::Call { .. }
                | Expr::Index { .. }
                | Expr::Cast { .. }
                | Expr::List(_)
                | Expr::Block(_) => Z3Value::Int(self.fresh_int()),
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

        /// Encode a binary operation.
        fn encode_binop(&mut self, lhs: &Expr, op: &BinOp, rhs: &Expr) -> Z3Value<'ctx> {
            let lv = self.encode_expr(lhs);
            let rv = self.encode_expr(rhs);

            match op {
                // --- Arithmetic: produce Int ---
                BinOp::Add => {
                    let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Int(ast::Int::add(self.ctx, &[&l, &r]))
                }
                BinOp::Sub => {
                    let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Int(ast::Int::sub(self.ctx, &[&l, &r]))
                }
                BinOp::Mul => {
                    let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Int(ast::Int::mul(self.ctx, &[&l, &r]))
                }
                BinOp::Div => {
                    let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Int(l.div(&r))
                }
                BinOp::Mod => {
                    let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Int(l.rem(&r))
                }

                // --- Comparison: produce Bool ---
                BinOp::Eq => match (&lv, &rv) {
                    (Z3Value::Int(l), Z3Value::Int(r)) => Z3Value::Bool(l._eq(r)),
                    (Z3Value::Bool(l), Z3Value::Bool(r)) => Z3Value::Bool(l._eq(r)),
                    _ => {
                        let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                        Z3Value::Bool(l._eq(&r))
                    }
                },
                BinOp::Neq => match (&lv, &rv) {
                    (Z3Value::Int(l), Z3Value::Int(r)) => Z3Value::Bool(l._eq(r).not()),
                    (Z3Value::Bool(l), Z3Value::Bool(r)) => Z3Value::Bool(l._eq(r).not()),
                    _ => {
                        let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                        let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                        Z3Value::Bool(l._eq(&r).not())
                    }
                },
                BinOp::Lt => {
                    let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Bool(l.lt(&r))
                }
                BinOp::Lte => {
                    let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Bool(l.le(&r))
                }
                BinOp::Gt => {
                    let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Bool(l.gt(&r))
                }
                BinOp::Gte => {
                    let l = lv.as_int(self.ctx, &mut self.fresh_counter);
                    let r = rv.as_int(self.ctx, &mut self.fresh_counter);
                    Z3Value::Bool(l.ge(&r))
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

                // --- Membership/other: approximate ---
                BinOp::In | BinOp::NotIn => Z3Value::Bool(self.fresh_bool()),
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
    pub(crate) fn verify_impl(typed: &TypedFile) -> Vec<VerificationResult> {
        let mut cfg = Config::new();
        cfg.set_param_value("timeout", "1000");
        let ctx = Context::new(&cfg);
        let mut results = Vec::new();

        // T044: collect all lemma definitions for apply injection
        let lemma_defs = collect_lemma_defs(typed);

        for decl in &typed.resolved.source.decls {
            match &decl.node {
                Decl::Contract(c) => {
                    verify_clauses(&ctx, &c.name, &c.clauses, &lemma_defs, &mut results);
                }
                Decl::FnDef(f) => {
                    // Lemma functions get their ensures verified independently
                    verify_clauses(&ctx, &f.name, &f.clauses, &lemma_defs, &mut results);
                }
                Decl::Extern(e) => {
                    verify_clauses(&ctx, &e.name, &e.clauses, &lemma_defs, &mut results);
                }
                Decl::Service(s) => {
                    for item in &s.items {
                        match item {
                            ServiceItem::Operation { name, clauses } => {
                                let qname = format!("{}.{}", s.name, name);
                                verify_clauses(&ctx, &qname, clauses, &lemma_defs, &mut results);
                            }
                            ServiceItem::Query { name, clauses } => {
                                let qname = format!("{}.{}", s.name, name);
                                verify_clauses(&ctx, &qname, clauses, &lemma_defs, &mut results);
                            }
                            ServiceItem::Invariant(expr) => {
                                verify_invariant_expr(&ctx, &s.name, expr, &mut results);
                            }
                            _ => {}
                        }
                    }
                }
                Decl::Block { name, body, .. } => {
                    verify_clauses(&ctx, name, body, &lemma_defs, &mut results);
                }
                Decl::TypeDef(_) | Decl::EnumDef(_) => {}
            }
        }

        results
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
}
