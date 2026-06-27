//! `EncodeTerm` trait: unified interface for constructing solver terms.
//!
//! This trait abstracts the backend-specific term construction that Z3 and
//! CVC5 share.  The shared [`encode_expr_shared`] function handles AST
//! dispatch and policy classification, calling trait methods for term
//! construction. Each backend implements `EncodeTerm` for its native term
//! type (`Z3Value` or `cvc5::Term`).
//!
//! This module is the counterpart of [`IrTermBuilder`](crate::ir_lower::IrTermBuilder)
//! for AST expression encoding (vs. IR instruction encoding).

#[cfg(test)]
use assura_ast::Spanned;
use assura_ast::{BinOp, Expr, Literal, SpExpr};

// ---------------------------------------------------------------------------
// Trait definition
// ---------------------------------------------------------------------------

/// Solver-neutral term construction for AST expression encoding.
///
/// Backends (Z3, CVC5 native) implement this trait on a builder struct
/// that holds the solver context, variable map, and axiom accumulator.
#[cfg_attr(not(feature = "cvc5-verify"), allow(dead_code))]
pub(crate) trait EncodeTerm {
    /// The backend-specific term type.
    ///
    /// Z3: `Z3Value` (tagged enum of Bool/Int/Real/Str/Bv).
    /// CVC5: `cvc5::Term<'a>` (sort is tracked internally).
    type Term: Clone;

    // === Literals ===

    /// Construct an integer constant from a string (handles bignums).
    fn make_int_literal(&mut self, s: &str) -> Self::Term;

    /// Construct a boolean constant.
    fn make_bool_literal(&mut self, b: bool) -> Self::Term;

    /// Construct a real constant from numerator/denominator.
    fn make_real_literal(&mut self, numer: i64, denom: i64) -> Self::Term;

    /// Construct a string literal (integer encoding or native string theory).
    /// Handles distinctness axioms and length axioms internally.
    fn make_string_literal(&mut self, s: &str) -> Self::Term;

    // === Variables ===

    /// Look up a variable by name.  Returns `None` if not yet created.
    fn get_var(&self, name: &str) -> Option<Self::Term>;

    /// Bind a variable in the current scope.
    fn set_var(&mut self, name: &str, val: Self::Term);

    /// Get an existing variable or create an integer variable with the
    /// given name.  This is the default for unknown identifiers.
    fn get_or_create_int_var(&mut self, name: &str) -> Self::Term;

    // === Binary operations ===

    /// Apply a binary operator to two terms.
    ///
    /// The backend handles sort coercion (Int/Real/BV) internally.
    /// Returns `None` for genuinely unsupported operator/type combinations.
    fn apply_binop(&mut self, op: &BinOp, lhs: Self::Term, rhs: Self::Term) -> Option<Self::Term>;

    // === Unary operations ===

    /// Negate an arithmetic term.
    fn make_neg(&mut self, t: Self::Term) -> Self::Term;

    /// Boolean NOT.
    fn make_not(&mut self, t: Self::Term) -> Self::Term;

    // === Boolean combinators ===

    /// Logical AND of two terms.
    fn make_and(&mut self, a: Self::Term, b: Self::Term) -> Self::Term;

    /// Logical OR of two terms.
    fn make_or(&mut self, a: Self::Term, b: Self::Term) -> Self::Term;

    /// Logical implication (P => Q).
    fn make_implies(&mut self, lhs: Self::Term, rhs: Self::Term) -> Self::Term;

    // === Control flow ===

    /// If-then-else (ternary).
    fn make_ite(
        &mut self,
        cond: Self::Term,
        then_val: Self::Term,
        else_val: Self::Term,
    ) -> Self::Term;

    // === Quantifiers ===

    /// Create a bound integer variable for a quantifier.
    fn make_bound_int_var(&mut self, name: &str) -> Self::Term;

    /// Construct a universal quantifier with domain guard and trigger patterns.
    fn make_forall(
        &mut self,
        var_name: &str,
        bound: &Self::Term,
        body: Self::Term,
        patterns: Vec<Self::Term>,
    ) -> Self::Term;

    /// Construct an existential quantifier with domain guard and trigger patterns.
    fn make_exists(
        &mut self,
        var_name: &str,
        bound: &Self::Term,
        body: Self::Term,
        patterns: Vec<Self::Term>,
    ) -> Self::Term;

    /// Guard a quantifier body with a domain constraint.
    ///
    /// `is_forall = true`:  `domain_guard => body`
    /// `is_forall = false`: `domain_guard && body`
    fn guard_quantifier_body(
        &mut self,
        domain: &SpExpr,
        bound: &Self::Term,
        body: Self::Term,
        is_forall: bool,
    ) -> Self::Term;

    /// Infer trigger patterns from function calls in the body.
    fn infer_quantifier_patterns(
        &mut self,
        body: &SpExpr,
        var_name: &str,
        bound: &Self::Term,
    ) -> Vec<Self::Term>;

    // === Uninterpreted functions ===

    /// Apply an uninterpreted function (Int -> Int).
    fn apply_uf_int(&mut self, name: &str, args: &[Self::Term]) -> Self::Term;

    /// Apply an uninterpreted function returning Bool.
    fn apply_uf_bool(&mut self, name: &str, args: &[Self::Term]) -> Self::Term;

    // === Sort coercion ===

    /// Coerce a term to boolean sort.
    fn as_bool(&mut self, term: Self::Term) -> Self::Term;

    /// Coerce a term to integer sort.
    fn as_int(&mut self, term: Self::Term) -> Self::Term;

    /// Check whether a term has Real sort.
    fn is_real_sort(&self, term: &Self::Term) -> bool;

    // === Fresh variables ===

    /// Create a fresh integer variable.
    fn fresh_int(&mut self) -> Self::Term;

    /// Create a fresh boolean variable.
    fn fresh_bool(&mut self) -> Self::Term;

    // === Axioms ===

    /// Assert a background axiom (e.g., `len >= 0`).
    fn push_axiom(&mut self, axiom: Self::Term);

    // === Trigger management ===

    /// Register a function name for quantifier e-matching.
    fn register_trigger_function(&mut self, name: &str);

    // === Collection operations ===

    /// Get or create the canonical `.length()` variable for a name.
    fn canonical_length(&mut self, name: &str) -> Self::Term;

    // === Compound expression encoding ===

    /// Encode a `Call { func, args }` expression.
    fn encode_call(
        &mut self,
        func: &SpExpr,
        args: &[SpExpr],
        encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<Self::Term>,
    ) -> Option<Self::Term>;

    /// Encode a `MethodCall { receiver, method, args }` expression.
    fn encode_method_call(
        &mut self,
        receiver: &SpExpr,
        method: &str,
        args: &[SpExpr],
        encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<Self::Term>,
    ) -> Option<Self::Term>;

    /// Encode a `Field(obj, field)` expression.
    fn encode_field(
        &mut self,
        obj: &SpExpr,
        field: &str,
        encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<Self::Term>,
    ) -> Option<Self::Term>;

    /// Encode an `old(expr)` pre-state snapshot.
    fn encode_old(
        &mut self,
        inner: &SpExpr,
        encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<Self::Term>,
    ) -> Option<Self::Term>;

    /// Encode a `Match { scrutinee, arms }` expression.
    fn encode_match(
        &mut self,
        scrutinee: &SpExpr,
        arms: &[assura_ast::MatchArm],
        encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<Self::Term>,
    ) -> Option<Self::Term>;

    /// Encode a `Let { name, value, body }` expression.
    fn encode_let(
        &mut self,
        name: &str,
        value: &SpExpr,
        body: &SpExpr,
        encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<Self::Term>,
    ) -> Option<Self::Term>;

    /// Encode a `Block(stmts)` expression (value = last statement).
    fn encode_block(
        &mut self,
        body: &[SpExpr],
        encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<Self::Term>,
    ) -> Option<Self::Term>;

    /// Encode a `Raw(tokens)` expression (raw token Pratt parsing).
    fn encode_raw(&mut self, tokens: &[String]) -> Option<Self::Term>;

    /// Encode a `Tuple(elems)` expression.
    fn encode_tuple(&mut self, elem_vals: &[Self::Term]) -> Self::Term;

    /// Encode a `List(elems)` expression.
    fn encode_list(&mut self, elem_vals: &[Self::Term]) -> Self::Term;

    /// Encode an `Index { expr, index }` expression.
    fn encode_index(&mut self, coll: Self::Term, index: Self::Term) -> Self::Term;

    /// Encode an `Apply { lemma_name, args }` expression.
    fn encode_apply(
        &mut self,
        lemma_name: &str,
        args: &[SpExpr],
        encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<Self::Term>,
    ) -> Option<Self::Term>;
}

// ---------------------------------------------------------------------------
// Shared expression encoding (generic over EncodeTerm)
// ---------------------------------------------------------------------------

/// Encode an AST expression to a solver term using the shared dispatch.
///
/// This is the unified replacement for `Encoder::encode_expr` (Z3) and
/// `encode_expr_cvc5` (CVC5).  It handles AST variant dispatch and policy
/// classification, delegating term construction to `EncodeTerm` methods.
#[cfg_attr(not(feature = "cvc5-verify"), allow(dead_code))]
pub(crate) fn encode_expr_shared<B: EncodeTerm>(b: &mut B, expr: &SpExpr) -> Option<B::Term> {
    match &expr.node {
        // --- Literals ---
        Expr::Literal(Literal::Int(s)) => Some(b.make_int_literal(s)),
        Expr::Literal(Literal::Float(s)) => {
            let (numer, denom) = crate::encode_atom_policy::float_to_rational_parts(s);
            Some(b.make_real_literal(numer, denom))
        }
        Expr::Literal(Literal::Str(s)) => Some(b.make_string_literal(s)),
        Expr::Literal(Literal::Bool(v)) => Some(b.make_bool_literal(*v)),

        // --- Identifiers ---
        Expr::Ident(name) => {
            if name == "true" {
                return Some(b.make_bool_literal(true));
            }
            if name == "false" {
                return Some(b.make_bool_literal(false));
            }
            if let Some(val) = b.get_var(name) {
                return Some(val);
            }
            Some(b.get_or_create_int_var(name))
        }

        // --- Binary operations ---
        Expr::BinOp { lhs, op, rhs } => {
            // Comparison chaining: a < b < c => (a < b) && (b < c)
            if crate::encode_binop_policy::is_comparison_ast_binop(op)
                && let Expr::BinOp {
                    lhs: inner_lhs,
                    op: inner_op,
                    rhs: inner_rhs,
                } = &lhs.node
                && crate::encode_binop_policy::is_comparison_ast_binop(inner_op)
            {
                let il = encode_expr_shared(b, inner_lhs)?;
                let mid = encode_expr_shared(b, inner_rhs)?;
                let r_val = encode_expr_shared(b, rhs)?;
                let mid2 = encode_expr_shared(b, inner_rhs)?;
                let left_cmp = b.apply_binop(inner_op, il, mid)?;
                let right_cmp = b.apply_binop(op, mid2, r_val)?;
                return Some(b.make_and(left_cmp, right_cmp));
            }
            let l = encode_expr_shared(b, lhs)?;
            let r = encode_expr_shared(b, rhs)?;
            b.apply_binop(op, l, r)
        }

        // --- Unary operations ---
        Expr::UnaryOp { op, expr: inner } => {
            use crate::encode_binop_policy::{AstUnaryKind, classify_ast_unary};
            let val = encode_expr_shared(b, inner)?;
            match classify_ast_unary(op) {
                AstUnaryKind::Neg => Some(b.make_neg(val)),
                AstUnaryKind::Not => Some(b.make_not(val)),
            }
        }

        // --- old(expr) ---
        Expr::Old(inner) => b.encode_old(inner, &mut |b, e| encode_expr_shared(b, e)),

        // --- Quantifiers ---
        Expr::Forall { var, domain, body } => {
            let bound = b.make_bound_int_var(var);
            b.set_var(var, bound.clone());
            let body_val = encode_expr_shared(b, body)?;
            let body_bool = b.as_bool(body_val);
            let guarded = b.guard_quantifier_body(domain, &bound, body_bool, true);
            let patterns = b.infer_quantifier_patterns(body, var, &bound);
            Some(b.make_forall(var, &bound, guarded, patterns))
        }
        Expr::Exists { var, domain, body } => {
            let bound = b.make_bound_int_var(var);
            b.set_var(var, bound.clone());
            let body_val = encode_expr_shared(b, body)?;
            let body_bool = b.as_bool(body_val);
            let guarded = b.guard_quantifier_body(domain, &bound, body_bool, false);
            let patterns = b.infer_quantifier_patterns(body, var, &bound);
            Some(b.make_exists(var, &bound, guarded, patterns))
        }

        // --- If-then-else ---
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            use crate::encode_if_policy::{IfEncodePlan, plan_if_encode};

            let cond_val = encode_expr_shared(b, cond)?;
            let cond_bool = b.as_bool(cond_val);
            let then_val = encode_expr_shared(b, then_branch)?;

            match plan_if_encode(else_branch.is_some()) {
                IfEncodePlan::Ite => {
                    let else_br = else_branch.as_ref()?;
                    let else_val = encode_expr_shared(b, else_br)?;
                    Some(b.make_ite(cond_bool, then_val, else_val))
                }
                IfEncodePlan::ImpliesThenOnly => {
                    let then_bool = b.as_bool(then_val);
                    Some(b.make_implies(cond_bool, then_bool))
                }
            }
        }

        // --- Raw token sequence ---
        Expr::Raw(tokens) => b.encode_raw(tokens),

        // --- Ghost block: transparent ---
        Expr::Ghost(inner) => encode_expr_shared(b, inner),

        // --- Cast: transparent ---
        Expr::Cast { expr: inner, .. } => encode_expr_shared(b, inner),

        // --- Apply lemma ---
        Expr::Apply { lemma_name, args } => {
            b.encode_apply(lemma_name, args, &mut |b, e| encode_expr_shared(b, e))
        }

        // --- Match ---
        Expr::Match {
            scrutinee, arms, ..
        } => b.encode_match(scrutinee, arms, &mut |b, e| encode_expr_shared(b, e)),

        // --- Let ---
        Expr::Let {
            name, value, body, ..
        } => b.encode_let(name, value, body, &mut |b, e| encode_expr_shared(b, e)),

        // --- Field access ---
        Expr::Field(obj, field) => b.encode_field(obj, field, &mut |b, e| encode_expr_shared(b, e)),

        // --- Index ---
        Expr::Index { expr: coll, index } => {
            let coll_val = encode_expr_shared(b, coll)?;
            let idx_val = encode_expr_shared(b, index)?;
            Some(b.encode_index(coll_val, idx_val))
        }

        // --- Block ---
        Expr::Block(body) => b.encode_block(body, &mut |b, e| encode_expr_shared(b, e)),

        // --- Tuple ---
        Expr::Tuple(elems) => {
            let elem_vals: Option<Vec<_>> =
                elems.iter().map(|e| encode_expr_shared(b, e)).collect();
            Some(b.encode_tuple(&elem_vals?))
        }

        // --- MethodCall ---
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            b.register_trigger_function(method);
            b.encode_method_call(receiver, method, args, &mut |b, e| encode_expr_shared(b, e))
        }

        // --- List ---
        Expr::List(elems) => {
            let elem_vals: Option<Vec<_>> =
                elems.iter().map(|e| encode_expr_shared(b, e)).collect();
            Some(b.encode_list(&elem_vals?))
        }

        // --- Call ---
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = &func.as_ref().node {
                b.register_trigger_function(name);
            }
            b.encode_call(func, args, &mut |b, e| encode_expr_shared(b, e))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Minimal mock implementation for testing the shared dispatch.
    struct MockEncoder {
        vars: HashMap<String, String>,
        axioms: Vec<String>,
        fresh_counter: u32,
    }

    impl MockEncoder {
        fn new() -> Self {
            Self {
                vars: HashMap::new(),
                axioms: Vec::new(),
                fresh_counter: 0,
            }
        }
    }

    impl EncodeTerm for MockEncoder {
        type Term = String;

        fn make_int_literal(&mut self, s: &str) -> String {
            format!("(int {s})")
        }
        fn make_bool_literal(&mut self, b: bool) -> String {
            format!("(bool {b})")
        }
        fn make_real_literal(&mut self, n: i64, d: i64) -> String {
            format!("(real {n}/{d})")
        }
        fn make_string_literal(&mut self, s: &str) -> String {
            format!("(str \"{s}\")")
        }

        fn get_var(&self, name: &str) -> Option<String> {
            self.vars.get(name).cloned()
        }
        fn set_var(&mut self, name: &str, val: String) {
            self.vars.insert(name.into(), val);
        }
        fn get_or_create_int_var(&mut self, name: &str) -> String {
            if let Some(v) = self.vars.get(name) {
                return v.clone();
            }
            let v = format!("(var {name})");
            self.vars.insert(name.into(), v.clone());
            v
        }

        fn apply_binop(&mut self, op: &BinOp, lhs: String, rhs: String) -> Option<String> {
            Some(format!("({} {} {})", op.as_str(), lhs, rhs))
        }

        fn make_neg(&mut self, t: String) -> String {
            format!("(neg {t})")
        }
        fn make_not(&mut self, t: String) -> String {
            format!("(not {t})")
        }
        fn make_and(&mut self, a: String, b: String) -> String {
            format!("(and {a} {b})")
        }
        fn make_or(&mut self, a: String, b: String) -> String {
            format!("(or {a} {b})")
        }
        fn make_implies(&mut self, a: String, b: String) -> String {
            format!("(=> {a} {b})")
        }
        fn make_ite(&mut self, c: String, t: String, e: String) -> String {
            format!("(ite {c} {t} {e})")
        }

        fn make_bound_int_var(&mut self, name: &str) -> String {
            format!("(bound {name})")
        }
        fn make_forall(
            &mut self,
            _var: &str,
            _bound: &String,
            body: String,
            _pats: Vec<String>,
        ) -> String {
            format!("(forall {body})")
        }
        fn make_exists(
            &mut self,
            _var: &str,
            _bound: &String,
            body: String,
            _pats: Vec<String>,
        ) -> String {
            format!("(exists {body})")
        }
        fn guard_quantifier_body(
            &mut self,
            _dom: &SpExpr,
            _bound: &String,
            body: String,
            _is_forall: bool,
        ) -> String {
            body
        }
        fn infer_quantifier_patterns(
            &mut self,
            _body: &SpExpr,
            _var: &str,
            _bound: &String,
        ) -> Vec<String> {
            vec![]
        }

        fn apply_uf_int(&mut self, name: &str, args: &[String]) -> String {
            format!("(uf {name} {})", args.join(" "))
        }
        fn apply_uf_bool(&mut self, name: &str, args: &[String]) -> String {
            format!("(uf-bool {name} {})", args.join(" "))
        }

        fn as_bool(&mut self, t: String) -> String {
            t
        }
        fn as_int(&mut self, t: String) -> String {
            t
        }
        fn is_real_sort(&self, _t: &String) -> bool {
            false
        }

        fn fresh_int(&mut self) -> String {
            self.fresh_counter += 1;
            format!("(fresh-int {})", self.fresh_counter)
        }
        fn fresh_bool(&mut self) -> String {
            self.fresh_counter += 1;
            format!("(fresh-bool {})", self.fresh_counter)
        }

        fn push_axiom(&mut self, a: String) {
            self.axioms.push(a);
        }
        fn register_trigger_function(&mut self, _name: &str) {}
        fn canonical_length(&mut self, name: &str) -> String {
            format!("(len {name})")
        }

        fn encode_call(
            &mut self,
            func: &SpExpr,
            args: &[SpExpr],
            encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<String>,
        ) -> Option<String> {
            let f = encode_sub(self, func)?;
            let a: Option<Vec<_>> = args.iter().map(|e| encode_sub(self, e)).collect();
            Some(format!("(call {f} {})", a?.join(" ")))
        }
        fn encode_method_call(
            &mut self,
            recv: &SpExpr,
            method: &str,
            args: &[SpExpr],
            encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<String>,
        ) -> Option<String> {
            let r = encode_sub(self, recv)?;
            let a: Option<Vec<_>> = args.iter().map(|e| encode_sub(self, e)).collect();
            Some(format!("(method {r} {method} {})", a?.join(" ")))
        }
        fn encode_field(
            &mut self,
            obj: &SpExpr,
            field: &str,
            encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<String>,
        ) -> Option<String> {
            let o = encode_sub(self, obj)?;
            Some(format!("(field {o} {field})"))
        }
        fn encode_old(
            &mut self,
            inner: &SpExpr,
            encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<String>,
        ) -> Option<String> {
            let v = encode_sub(self, inner)?;
            Some(format!("(old {v})"))
        }
        fn encode_match(
            &mut self,
            scrutinee: &SpExpr,
            _arms: &[assura_ast::MatchArm],
            encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<String>,
        ) -> Option<String> {
            let s = encode_sub(self, scrutinee)?;
            Some(format!("(match {s})"))
        }
        fn encode_let(
            &mut self,
            name: &str,
            value: &SpExpr,
            body: &SpExpr,
            encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<String>,
        ) -> Option<String> {
            let v = encode_sub(self, value)?;
            self.set_var(name, v);
            encode_sub(self, body)
        }
        fn encode_block(
            &mut self,
            body: &[SpExpr],
            encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<String>,
        ) -> Option<String> {
            body.iter().map(|e| encode_sub(self, e)).last()?
        }
        fn encode_raw(&mut self, tokens: &[String]) -> Option<String> {
            Some(format!("(raw {})", tokens.join(" ")))
        }
        fn encode_tuple(&mut self, elems: &[String]) -> String {
            format!("(tuple {})", elems.join(" "))
        }
        fn encode_list(&mut self, elems: &[String]) -> String {
            format!("(list {})", elems.join(" "))
        }
        fn encode_index(&mut self, coll: String, idx: String) -> String {
            format!("(index {coll} {idx})")
        }
        fn encode_apply(
            &mut self,
            name: &str,
            args: &[SpExpr],
            encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<String>,
        ) -> Option<String> {
            for a in args {
                encode_sub(self, a);
            }
            Some(format!("(apply {name})"))
        }
    }

    #[test]
    fn shared_encode_literal_int() {
        let mut enc = MockEncoder::new();
        let expr = Spanned::no_span(Expr::Literal(Literal::Int("42".into())));
        let result = encode_expr_shared(&mut enc, &expr);
        assert_eq!(result, Some("(int 42)".into()));
    }

    #[test]
    fn shared_encode_literal_bool() {
        let mut enc = MockEncoder::new();
        let expr = Spanned::no_span(Expr::Literal(Literal::Bool(true)));
        let result = encode_expr_shared(&mut enc, &expr);
        assert_eq!(result, Some("(bool true)".into()));
    }

    #[test]
    fn shared_encode_ident_true() {
        let mut enc = MockEncoder::new();
        let expr = Spanned::no_span(Expr::Ident("true".into()));
        let result = encode_expr_shared(&mut enc, &expr);
        assert_eq!(result, Some("(bool true)".into()));
    }

    #[test]
    fn shared_encode_ident_var() {
        let mut enc = MockEncoder::new();
        let expr = Spanned::no_span(Expr::Ident("x".into()));
        let result = encode_expr_shared(&mut enc, &expr);
        assert_eq!(result, Some("(var x)".into()));
    }

    #[test]
    fn shared_encode_binop() {
        let mut enc = MockEncoder::new();
        let lhs = Box::new(Spanned::no_span(Expr::Ident("x".into())));
        let rhs = Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into()))));
        let expr = Spanned::no_span(Expr::BinOp {
            lhs,
            op: BinOp::Add,
            rhs,
        });
        let result = encode_expr_shared(&mut enc, &expr);
        assert_eq!(result, Some("(+ (var x) (int 1))".into()));
    }

    #[test]
    fn shared_encode_if_then_else() {
        let mut enc = MockEncoder::new();
        let cond = Box::new(Spanned::no_span(Expr::Ident("c".into())));
        let then_br = Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into()))));
        let else_br = Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into()))));
        let expr = Spanned::no_span(Expr::If {
            cond,
            then_branch: then_br,
            else_branch: Some(else_br),
        });
        let result = encode_expr_shared(&mut enc, &expr);
        assert_eq!(result, Some("(ite (var c) (int 1) (int 0))".into()));
    }

    #[test]
    fn shared_encode_ghost_transparent() {
        let mut enc = MockEncoder::new();
        let inner = Box::new(Spanned::no_span(Expr::Literal(Literal::Int("7".into()))));
        let expr = Spanned::no_span(Expr::Ghost(inner));
        let result = encode_expr_shared(&mut enc, &expr);
        assert_eq!(result, Some("(int 7)".into()));
    }

    #[test]
    fn shared_encode_comparison_chaining() {
        let mut enc = MockEncoder::new();
        // a < b < c => (a < b) && (b < c)
        let a = Spanned::no_span(Expr::Ident("a".into()));
        let b_expr = Spanned::no_span(Expr::Ident("b".into()));
        let c = Spanned::no_span(Expr::Ident("c".into()));
        let ab = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(a),
            op: BinOp::Lt,
            rhs: Box::new(b_expr),
        });
        let expr = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(ab),
            op: BinOp::Lt,
            rhs: Box::new(c),
        });
        let result = encode_expr_shared(&mut enc, &expr).unwrap();
        assert!(
            result.starts_with("(and "),
            "expected chained comparison: {result}"
        );
        assert!(result.contains("(< (var a) (var b))"));
        assert!(result.contains("(< (var b) (var c))"));
    }
}

// ---------------------------------------------------------------------------
// Z3 conformance tests (EncodeTerm for Encoder)
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "z3-verify"))]
mod tests_z3_encode_term {
    use super::*;
    use crate::z3_backend::encoder::Encoder;

    #[test]
    fn z3_encode_term_int_literal() {
        z3::with_z3_config(&z3::Config::new(), || {
            let mut enc = Encoder::new();
            let expr = Spanned::no_span(Expr::Literal(Literal::Int("42".into())));
            let result = encode_expr_shared(&mut enc, &expr);
            assert!(result.is_some(), "Z3 should encode int literal");
        });
    }

    #[test]
    fn z3_encode_term_bool_literal() {
        z3::with_z3_config(&z3::Config::new(), || {
            let mut enc = Encoder::new();
            let expr = Spanned::no_span(Expr::Literal(Literal::Bool(true)));
            let result = encode_expr_shared(&mut enc, &expr);
            assert!(result.is_some());
        });
    }

    #[test]
    fn z3_encode_term_ident() {
        z3::with_z3_config(&z3::Config::new(), || {
            let mut enc = Encoder::new();
            let expr = Spanned::no_span(Expr::Ident("x".into()));
            let result = encode_expr_shared(&mut enc, &expr);
            assert!(result.is_some());
            // Variable should be registered
            assert!(enc.vars.contains_key("x"));
        });
    }

    #[test]
    fn z3_encode_term_binop_add() {
        z3::with_z3_config(&z3::Config::new(), || {
            let mut enc = Encoder::new();
            let expr = Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Add,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
            });
            let result = encode_expr_shared(&mut enc, &expr);
            assert!(result.is_some(), "Z3 should encode addition");
        });
    }

    #[test]
    fn z3_encode_term_comparison_lt() {
        z3::with_z3_config(&z3::Config::new(), || {
            let mut enc = Encoder::new();
            let expr = Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
                op: BinOp::Lt,
                rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
            });
            let result = encode_expr_shared(&mut enc, &expr);
            assert!(result.is_some(), "Z3 should encode comparison");
        });
    }

    #[test]
    fn z3_encode_term_if_then_else() {
        z3::with_z3_config(&z3::Config::new(), || {
            let mut enc = Encoder::new();
            let expr = Spanned::no_span(Expr::If {
                cond: Box::new(Spanned::no_span(Expr::Ident("c".into()))),
                then_branch: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
                else_branch: Some(Box::new(Spanned::no_span(Expr::Literal(Literal::Int(
                    "0".into(),
                ))))),
            });
            let result = encode_expr_shared(&mut enc, &expr);
            assert!(result.is_some(), "Z3 should encode if-then-else");
        });
    }

    #[test]
    fn z3_encode_term_ghost_transparent() {
        z3::with_z3_config(&z3::Config::new(), || {
            let mut enc = Encoder::new();
            let inner = Box::new(Spanned::no_span(Expr::Literal(Literal::Int("7".into()))));
            let expr = Spanned::no_span(Expr::Ghost(inner));
            let result = encode_expr_shared(&mut enc, &expr);
            assert!(result.is_some(), "Ghost should be transparent");
        });
    }

    #[test]
    fn z3_encode_term_unary_neg() {
        z3::with_z3_config(&z3::Config::new(), || {
            let mut enc = Encoder::new();
            let expr = Spanned::no_span(Expr::UnaryOp {
                op: assura_ast::UnaryOp::Neg,
                expr: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
            });
            let result = encode_expr_shared(&mut enc, &expr);
            assert!(result.is_some(), "Z3 should encode unary neg");
        });
    }

    #[test]
    fn z3_encode_term_parity_with_encode_expr() {
        // Verify that encode_expr_shared produces the same variable
        // registrations as Encoder::encode_expr for the same AST.
        z3::with_z3_config(&z3::Config::new(), || {
            let expr = Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Add,
                rhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
            });

            let mut enc_direct = Encoder::new();
            let _direct = enc_direct.encode_expr(&expr);

            let mut enc_shared = Encoder::new();
            let _shared = encode_expr_shared(&mut enc_shared, &expr);

            // Both should have registered x and y
            assert!(enc_direct.vars.contains_key("x"));
            assert!(enc_direct.vars.contains_key("y"));
            assert!(enc_shared.vars.contains_key("x"));
            assert!(enc_shared.vars.contains_key("y"));
        });
    }
}

// ---------------------------------------------------------------------------
// CVC5 native conformance tests (EncodeTerm for Cvc5TermBuilder)
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "cvc5-verify"))]
mod tests_cvc5_encode_term {
    use super::*;
    use crate::cvc5_encode_term_impl::Cvc5TermBuilder;
    use crate::cvc5_encoder_state::default_cvc5_encoder_state;
    use std::collections::HashMap;

    #[test]
    fn cvc5_encode_term_int_literal() {
        let tm = cvc5::TermManager::new();
        let mut vars = HashMap::new();
        let mut state = default_cvc5_encoder_state();
        let mut builder = Cvc5TermBuilder {
            tm: &tm,
            vars: &mut vars,
            state: &mut state,
        };
        let expr = Spanned::no_span(Expr::Literal(Literal::Int("42".into())));
        let result = encode_expr_shared(&mut builder, &expr);
        assert!(result.is_some(), "CVC5 should encode int literal");
    }

    #[test]
    fn cvc5_encode_term_bool_literal() {
        let tm = cvc5::TermManager::new();
        let mut vars = HashMap::new();
        let mut state = default_cvc5_encoder_state();
        let mut builder = Cvc5TermBuilder {
            tm: &tm,
            vars: &mut vars,
            state: &mut state,
        };
        let expr = Spanned::no_span(Expr::Literal(Literal::Bool(true)));
        let result = encode_expr_shared(&mut builder, &expr);
        assert!(result.is_some(), "CVC5 should encode bool literal");
    }

    #[test]
    fn cvc5_encode_term_ident() {
        let tm = cvc5::TermManager::new();
        let mut vars = HashMap::new();
        let mut state = default_cvc5_encoder_state();
        let mut builder = Cvc5TermBuilder {
            tm: &tm,
            vars: &mut vars,
            state: &mut state,
        };
        let expr = Spanned::no_span(Expr::Ident("x".into()));
        let result = encode_expr_shared(&mut builder, &expr);
        assert!(result.is_some(), "CVC5 should encode ident");
        assert!(vars.contains_key("x"), "x should be registered");
    }

    #[test]
    fn cvc5_encode_term_binop_add() {
        let tm = cvc5::TermManager::new();
        let mut vars = HashMap::new();
        let mut state = default_cvc5_encoder_state();
        let mut builder = Cvc5TermBuilder {
            tm: &tm,
            vars: &mut vars,
            state: &mut state,
        };
        let expr = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Add,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        });
        let result = encode_expr_shared(&mut builder, &expr);
        assert!(result.is_some(), "CVC5 should encode addition");
    }

    #[test]
    fn cvc5_encode_term_comparison_lt() {
        let tm = cvc5::TermManager::new();
        let mut vars = HashMap::new();
        let mut state = default_cvc5_encoder_state();
        let mut builder = Cvc5TermBuilder {
            tm: &tm,
            vars: &mut vars,
            state: &mut state,
        };
        let expr = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
            op: BinOp::Lt,
            rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
        });
        let result = encode_expr_shared(&mut builder, &expr);
        assert!(result.is_some(), "CVC5 should encode comparison");
    }

    #[test]
    fn cvc5_encode_term_if_then_else() {
        let tm = cvc5::TermManager::new();
        let mut vars = HashMap::new();
        let mut state = default_cvc5_encoder_state();
        let mut builder = Cvc5TermBuilder {
            tm: &tm,
            vars: &mut vars,
            state: &mut state,
        };
        let expr = Spanned::no_span(Expr::If {
            cond: Box::new(Spanned::no_span(Expr::Literal(Literal::Bool(true)))),
            then_branch: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
            else_branch: Some(Box::new(Spanned::no_span(Expr::Literal(Literal::Int(
                "0".into(),
            ))))),
        });
        let result = encode_expr_shared(&mut builder, &expr);
        assert!(result.is_some(), "CVC5 should encode if-then-else");
    }

    #[test]
    fn cvc5_encode_term_ghost_transparent() {
        let tm = cvc5::TermManager::new();
        let mut vars = HashMap::new();
        let mut state = default_cvc5_encoder_state();
        let mut builder = Cvc5TermBuilder {
            tm: &tm,
            vars: &mut vars,
            state: &mut state,
        };
        let inner = Box::new(Spanned::no_span(Expr::Literal(Literal::Int("7".into()))));
        let expr = Spanned::no_span(Expr::Ghost(inner));
        let result = encode_expr_shared(&mut builder, &expr);
        assert!(result.is_some(), "CVC5 Ghost should be transparent");
    }

    #[test]
    fn cvc5_encode_term_unary_neg() {
        let tm = cvc5::TermManager::new();
        let mut vars = HashMap::new();
        let mut state = default_cvc5_encoder_state();
        let mut builder = Cvc5TermBuilder {
            tm: &tm,
            vars: &mut vars,
            state: &mut state,
        };
        let expr = Spanned::no_span(Expr::UnaryOp {
            op: assura_ast::UnaryOp::Neg,
            expr: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
        });
        let result = encode_expr_shared(&mut builder, &expr);
        assert!(result.is_some(), "CVC5 should encode unary neg");
    }

    #[test]
    fn cvc5_encode_term_parity_with_encode_expr_cvc5() {
        // Verify encode_expr_shared via Cvc5TermBuilder produces the same variable
        // registrations as encode_expr_cvc5 for the same AST.
        use crate::cvc5_native_encoder::encode_expr_cvc5;

        let tm = cvc5::TermManager::new();
        let expr = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Add,
            rhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
        });

        // Direct encode
        let mut vars_direct = HashMap::new();
        let mut state_direct = default_cvc5_encoder_state();
        let _direct = encode_expr_cvc5(&tm, &expr, &mut vars_direct, &mut state_direct);

        // Shared encode
        let mut vars_shared = HashMap::new();
        let mut state_shared = default_cvc5_encoder_state();
        let mut builder = Cvc5TermBuilder {
            tm: &tm,
            vars: &mut vars_shared,
            state: &mut state_shared,
        };
        let _shared = encode_expr_shared(&mut builder, &expr);

        // Both should have registered x and y
        assert!(vars_direct.contains_key("x"));
        assert!(vars_direct.contains_key("y"));
        assert!(vars_shared.contains_key("x"));
        assert!(vars_shared.contains_key("y"));
    }
}
