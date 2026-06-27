//! `EncodeTerm` implementation for the CVC5 native backend.
//!
//! Wraps the existing CVC5 encoder functions to satisfy the trait interface.
//! Each trait method delegates to the corresponding `cvc5_*_encode` module
//! function. This is a thin adapter with no behavior change.

#![cfg(feature = "cvc5-verify")]

use std::collections::HashMap;

use assura_ast::{BinOp, MatchArm, SpExpr};

use crate::cvc5_binop_encode::{encode_ast_binop_cvc5, encode_ast_unary_cvc5};
use crate::cvc5_encoder_state::{
    Cvc5EncoderState, canonical_length_cvc5, field_len_fn_cvc5, intern_uf_cvc5,
};
use crate::encode_term::EncodeTerm;

/// CVC5 native term builder that bundles the TermManager, variable map, and
/// encoder state into a single struct implementing `EncodeTerm`.
pub(crate) struct Cvc5TermBuilder<'a, 'v, 's> {
    pub(crate) tm: &'a cvc5::TermManager,
    pub(crate) vars: &'v mut HashMap<String, cvc5::Term<'a>>,
    pub(crate) state: &'s mut Cvc5EncoderState<'a>,
}

impl<'a> EncodeTerm for Cvc5TermBuilder<'a, '_, '_> {
    type Term = cvc5::Term<'a>;

    // === Literals ===

    fn make_int_literal(&mut self, s: &str) -> cvc5::Term<'a> {
        if let Ok(n) = s.parse::<i64>() {
            self.tm.mk_integer(n)
        } else if let Some(rest) = s.strip_prefix('-') {
            let abs = self.tm.mk_integer_from_str(rest);
            self.tm.mk_term(cvc5::Kind::Neg, &[abs])
        } else {
            self.tm.mk_integer_from_str(s)
        }
    }

    fn make_bool_literal(&mut self, b: bool) -> cvc5::Term<'a> {
        if b {
            self.tm.mk_true()
        } else {
            self.tm.mk_false()
        }
    }

    fn make_real_literal(&mut self, numer: i64, denom: i64) -> cvc5::Term<'a> {
        self.tm.mk_real_from_rational(numer, denom)
    }

    fn make_string_literal(&mut self, s: &str) -> cvc5::Term<'a> {
        if self.state.use_string_theory {
            self.tm.mk_string(s, false)
        } else {
            let const_name = crate::encode_atom_policy::string_literal_const_name(s);
            let v = self.tm.mk_const(self.tm.integer_sort(), &const_name);
            // Pairwise distinctness axioms
            if !self.state.string_constants.contains(&const_name) {
                for prev in &self.state.string_constants {
                    let prev_val = self.tm.mk_const(self.tm.integer_sort(), prev);
                    let eq = self.tm.mk_term(cvc5::Kind::Equal, &[v.clone(), prev_val]);
                    self.state
                        .axioms
                        .push(self.tm.mk_term(cvc5::Kind::Not, &[eq]));
                }
                self.state.string_constants.push(const_name);
            }
            // Length axiom
            let len_func = field_len_fn_cvc5(self.tm, self.state);
            let len_result = self.tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, v.clone()]);
            let str_len = self.tm.mk_integer(s.len() as i64);
            self.state
                .axioms
                .push(self.tm.mk_term(cvc5::Kind::Equal, &[len_result, str_len]));
            v
        }
    }

    // === Variables ===

    fn get_var(&self, name: &str) -> Option<cvc5::Term<'a>> {
        self.vars.get(name).cloned()
    }

    fn set_var(&mut self, name: &str, val: cvc5::Term<'a>) {
        self.vars.insert(name.to_string(), val);
    }

    fn get_or_create_int_var(&mut self, name: &str) -> cvc5::Term<'a> {
        if let Some(val) = self.vars.get(name) {
            return val.clone();
        }
        let v = self.tm.mk_const(self.tm.integer_sort(), name);
        self.vars.insert(name.to_string(), v.clone());
        v
    }

    // === Binary operations ===

    fn apply_binop(
        &mut self,
        op: &BinOp,
        lhs: cvc5::Term<'a>,
        rhs: cvc5::Term<'a>,
    ) -> Option<cvc5::Term<'a>> {
        encode_ast_binop_cvc5(self.tm, op, lhs, rhs, self.state)
    }

    // === Unary operations ===

    fn make_neg(&mut self, t: cvc5::Term<'a>) -> cvc5::Term<'a> {
        encode_ast_unary_cvc5(self.tm, &assura_ast::UnaryOp::Neg, t)
    }

    fn make_not(&mut self, t: cvc5::Term<'a>) -> cvc5::Term<'a> {
        encode_ast_unary_cvc5(self.tm, &assura_ast::UnaryOp::Not, t)
    }

    // === Boolean combinators ===

    fn make_and(&mut self, a: cvc5::Term<'a>, b: cvc5::Term<'a>) -> cvc5::Term<'a> {
        self.tm.mk_term(cvc5::Kind::And, &[a, b])
    }

    fn make_or(&mut self, a: cvc5::Term<'a>, b: cvc5::Term<'a>) -> cvc5::Term<'a> {
        self.tm.mk_term(cvc5::Kind::Or, &[a, b])
    }

    fn make_implies(&mut self, lhs: cvc5::Term<'a>, rhs: cvc5::Term<'a>) -> cvc5::Term<'a> {
        self.tm.mk_term(cvc5::Kind::Implies, &[lhs, rhs])
    }

    // === Control flow ===

    fn make_ite(
        &mut self,
        cond: cvc5::Term<'a>,
        then_val: cvc5::Term<'a>,
        else_val: cvc5::Term<'a>,
    ) -> cvc5::Term<'a> {
        self.tm
            .mk_term(cvc5::Kind::Ite, &[cond, then_val, else_val])
    }

    // === Quantifiers ===

    fn make_bound_int_var(&mut self, name: &str) -> cvc5::Term<'a> {
        self.tm.mk_var(self.tm.integer_sort(), name)
    }

    fn make_forall(
        &mut self,
        _var: &str,
        bound: &cvc5::Term<'a>,
        body: cvc5::Term<'a>,
        _patterns: Vec<cvc5::Term<'a>>,
    ) -> cvc5::Term<'a> {
        let var_list = self.tm.mk_term(cvc5::Kind::VariableList, std::slice::from_ref(bound));
        self.tm.mk_term(cvc5::Kind::Forall, &[var_list, body])
    }

    fn make_exists(
        &mut self,
        _var: &str,
        bound: &cvc5::Term<'a>,
        body: cvc5::Term<'a>,
        _patterns: Vec<cvc5::Term<'a>>,
    ) -> cvc5::Term<'a> {
        let var_list = self.tm.mk_term(cvc5::Kind::VariableList, std::slice::from_ref(bound));
        self.tm.mk_term(cvc5::Kind::Exists, &[var_list, body])
    }

    fn guard_quantifier_body(
        &mut self,
        domain: &SpExpr,
        bound: &cvc5::Term<'a>,
        body: cvc5::Term<'a>,
        is_forall: bool,
    ) -> cvc5::Term<'a> {
        use crate::encode_quantifier_policy::domain_as_range;

        if let Some((lo, hi)) = domain_as_range(domain) {
            let lo_t = crate::encode_term::encode_expr_shared(self, lo)
                .unwrap_or_else(|| self.tm.mk_integer(0));
            let hi_t = crate::encode_term::encode_expr_shared(self, hi)
                .unwrap_or_else(|| self.tm.mk_integer(0));
            let ge_lo = self.tm.mk_term(cvc5::Kind::Geq, &[bound.clone(), lo_t]);
            let lt_hi = self.tm.mk_term(cvc5::Kind::Lt, &[bound.clone(), hi_t]);
            let guard = self.tm.mk_term(cvc5::Kind::And, &[ge_lo, lt_hi]);
            if is_forall {
                self.tm.mk_term(cvc5::Kind::Implies, &[guard, body])
            } else {
                self.tm.mk_term(cvc5::Kind::And, &[guard, body])
            }
        } else {
            body
        }
    }

    fn infer_quantifier_patterns(
        &mut self,
        _body: &SpExpr,
        _var_name: &str,
        _bound: &cvc5::Term<'a>,
    ) -> Vec<cvc5::Term<'a>> {
        // CVC5 native triggers use InstPattern kind which requires solver
        // integration. For now, return empty (same as Z3 impl); real
        // quantifier encoding still goes through encode_ast_quantifier_cvc5.
        vec![]
    }

    // === Uninterpreted functions ===

    fn apply_uf_int(&mut self, name: &str, args: &[cvc5::Term<'a>]) -> cvc5::Term<'a> {
        let func = intern_uf_cvc5(self.tm, self.state, name, args.len(), false);
        let mut apply_args = vec![func];
        apply_args.extend(args.iter().cloned());
        self.tm.mk_term(cvc5::Kind::ApplyUf, &apply_args)
    }

    fn apply_uf_bool(&mut self, name: &str, args: &[cvc5::Term<'a>]) -> cvc5::Term<'a> {
        let func = intern_uf_cvc5(self.tm, self.state, name, args.len(), true);
        let mut apply_args = vec![func];
        apply_args.extend(args.iter().cloned());
        self.tm.mk_term(cvc5::Kind::ApplyUf, &apply_args)
    }

    // === Sort coercion ===

    fn as_bool(&mut self, term: cvc5::Term<'a>) -> cvc5::Term<'a> {
        // CVC5 terms have intrinsic sorts. If already Bool, return as-is.
        // If Int, treat non-zero as true: (distinct term 0)
        let sort = term.sort();
        if sort == self.tm.boolean_sort() {
            term
        } else {
            let zero = self.tm.mk_integer(0);
            self.tm.mk_term(cvc5::Kind::Distinct, &[term, zero])
        }
    }

    fn as_int(&mut self, term: cvc5::Term<'a>) -> cvc5::Term<'a> {
        let sort = term.sort();
        if sort == self.tm.integer_sort() {
            term
        } else if sort == self.tm.boolean_sort() {
            // Bool -> Int: ite(b, 1, 0)
            let one = self.tm.mk_integer(1);
            let zero = self.tm.mk_integer(0);
            self.tm.mk_term(cvc5::Kind::Ite, &[term, one, zero])
        } else {
            // Real or unknown: just pass through (CVC5 handles coercion)
            term
        }
    }

    fn is_real_sort(&self, term: &cvc5::Term<'a>) -> bool {
        term.sort() == self.tm.real_sort()
    }

    // === Fresh variables ===

    fn fresh_int(&mut self) -> cvc5::Term<'a> {
        let name = crate::encode_atom_policy::fresh_temp_name(self.state.fresh_counter);
        self.state.fresh_counter += 1;
        self.tm.mk_const(self.tm.integer_sort(), &name)
    }

    fn fresh_bool(&mut self) -> cvc5::Term<'a> {
        let name = crate::encode_atom_policy::fresh_temp_name(self.state.fresh_counter);
        self.state.fresh_counter += 1;
        self.tm.mk_const(self.tm.boolean_sort(), &name)
    }

    // === Axioms ===

    fn push_axiom(&mut self, axiom: cvc5::Term<'a>) {
        self.state.axioms.push(axiom);
    }

    // === Trigger management ===

    fn register_trigger_function(&mut self, name: &str) {
        self.state
            .trigger_manager
            .register_function(name.to_string());
    }

    // === Collection operations ===

    fn canonical_length(&mut self, name: &str) -> cvc5::Term<'a> {
        canonical_length_cvc5(self.tm, name, self.vars, self.state)
    }

    // === Compound expression encoding ===

    fn encode_call(
        &mut self,
        func: &SpExpr,
        args: &[SpExpr],
        _encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<cvc5::Term<'a>>,
    ) -> Option<cvc5::Term<'a>> {
        // Delegate to existing CVC5 call encoder. Internal recursion in the
        // call encoder goes through encode_expr_cvc5 -> encode_expr_shared.
        use crate::cvc5_call_encode::encode_call_cvc5;
        use crate::cvc5_native_encoder::encode_expr_cvc5;
        encode_call_cvc5(self.tm, func, args, self.vars, self.state, |e, v, s| {
            encode_expr_cvc5(self.tm, e, v, s)
        })
    }

    fn encode_method_call(
        &mut self,
        receiver: &SpExpr,
        method: &str,
        args: &[SpExpr],
        _encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<cvc5::Term<'a>>,
    ) -> Option<cvc5::Term<'a>> {
        use crate::cvc5_call_encode::encode_method_call_cvc5;
        use crate::cvc5_native_encoder::encode_expr_cvc5;
        encode_method_call_cvc5(
            self.tm,
            receiver,
            method,
            args,
            self.vars,
            self.state,
            |e, v, s| encode_expr_cvc5(self.tm, e, v, s),
        )
    }

    fn encode_field(
        &mut self,
        obj: &SpExpr,
        field: &str,
        _encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<cvc5::Term<'a>>,
    ) -> Option<cvc5::Term<'a>> {
        use crate::cvc5_field_access::encode_field_cvc5;
        use crate::cvc5_native_encoder::encode_expr_cvc5;
        encode_field_cvc5(self.tm, obj, field, self.vars, self.state, |e, v, s| {
            encode_expr_cvc5(self.tm, e, v, s)
        })
    }

    fn encode_old(
        &mut self,
        inner: &SpExpr,
        _encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<cvc5::Term<'a>>,
    ) -> Option<cvc5::Term<'a>> {
        use crate::cvc5_native_encoder::encode_expr_cvc5;
        use crate::cvc5_old_access::encode_old_cvc5;
        encode_old_cvc5(self.tm, inner, self.vars, self.state, |e, v, s| {
            encode_expr_cvc5(self.tm, e, v, s)
        })
    }

    fn encode_match(
        &mut self,
        scrutinee: &SpExpr,
        arms: &[MatchArm],
        _encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<cvc5::Term<'a>>,
    ) -> Option<cvc5::Term<'a>> {
        use crate::cvc5_match_encode::encode_match_cvc5;
        use crate::cvc5_native_encoder::encode_expr_cvc5;
        encode_match_cvc5(
            self.tm,
            scrutinee,
            arms,
            self.vars,
            self.state,
            |e, v, s| encode_expr_cvc5(self.tm, e, v, s),
        )
    }

    fn encode_let(
        &mut self,
        name: &str,
        value: &SpExpr,
        body: &SpExpr,
        _encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<cvc5::Term<'a>>,
    ) -> Option<cvc5::Term<'a>> {
        use crate::cvc5_let_block_encode::encode_let_cvc5;
        use crate::cvc5_native_encoder::encode_expr_cvc5;
        encode_let_cvc5(
            self.tm,
            name,
            value,
            body,
            self.vars,
            self.state,
            |e, v, s| encode_expr_cvc5(self.tm, e, v, s),
        )
    }

    fn encode_block(
        &mut self,
        body: &[SpExpr],
        _encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<cvc5::Term<'a>>,
    ) -> Option<cvc5::Term<'a>> {
        use crate::cvc5_let_block_encode::encode_block_cvc5;
        use crate::cvc5_native_encoder::encode_expr_cvc5;
        encode_block_cvc5(self.tm, body, self.vars, self.state, |e, v, s| {
            encode_expr_cvc5(self.tm, e, v, s)
        })
    }

    fn encode_raw(&mut self, tokens: &[String]) -> Option<cvc5::Term<'a>> {
        use crate::cvc5_raw_encode::encode_raw_expr_cvc5;
        encode_raw_expr_cvc5(self.tm, tokens, self.vars, self.state)
    }

    fn encode_tuple(&mut self, elem_vals: &[cvc5::Term<'a>]) -> cvc5::Term<'a> {
        use crate::cvc5_tuple_encode::encode_tuple_cvc5;
        encode_tuple_cvc5(
            self.tm,
            elem_vals,
            &mut self.state.axioms,
            &mut self.state.fresh_counter,
        )
    }

    fn encode_list(&mut self, elem_vals: &[cvc5::Term<'a>]) -> cvc5::Term<'a> {
        use crate::cvc5_list_encode::encode_list_cvc5;
        let len_func = field_len_fn_cvc5(self.tm, self.state);
        encode_list_cvc5(
            self.tm,
            elem_vals,
            &mut self.state.axioms,
            &mut self.state.fresh_counter,
            &len_func,
        )
    }

    fn encode_index(&mut self, coll: cvc5::Term<'a>, index: cvc5::Term<'a>) -> cvc5::Term<'a> {
        use crate::cvc5_index_access::encode_index_access_cvc5;
        encode_index_access_cvc5(self.tm, coll, index, &mut self.state.axioms)
    }

    fn encode_apply(
        &mut self,
        lemma_name: &str,
        args: &[SpExpr],
        _encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<cvc5::Term<'a>>,
    ) -> Option<cvc5::Term<'a>> {
        use crate::cvc5_atom_encode::encode_apply_cvc5;
        use crate::cvc5_native_encoder::encode_expr_cvc5;
        encode_apply_cvc5(
            self.tm,
            lemma_name,
            args,
            self.vars,
            self.state,
            |e, v, s| encode_expr_cvc5(self.tm, e, v, s),
        )
    }
}
