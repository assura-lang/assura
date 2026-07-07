//! Encoder construction, shared helpers, and literal encoding.

use crate::*;
use assura_ast::Literal;
use std::collections::HashMap;
use z3::ast;

use super::BitvectorEncoder;
use super::value::Z3Value;
use super::{BITVECTOR_API_WIRED, Encoder};

impl Encoder {
    pub(crate) fn new() -> Self {
        Self {
            vars: HashMap::new(),
            func_arities: HashMap::new(),
            fresh_counter: 0,
            background_axioms: Vec::new(),
            trigger_manager: crate::advanced::TriggerManager::new(),
            string_constants: Vec::new(),
            use_string_theory: false,
            adt_defs: HashMap::new(),
            bv_signed: HashMap::new(),
            canonical_lengths: HashMap::new(),
            callee_specs: HashMap::new(),
        }
    }

    /// Create an encoder with native string theory enabled.
    pub(crate) fn with_string_theory(use_string_theory: bool) -> Self {
        Self {
            use_string_theory,
            ..Self::new()
        }
    }

    /// Copy shared Z3 variable state from a base encoder (#264).
    pub(crate) fn share_encoding_state_from(&mut self, base: &Encoder) {
        self.vars.clone_from(&base.vars);
        self.adt_defs.clone_from(&base.adt_defs);
        self.func_arities.clone_from(&base.func_arities);
        self.canonical_lengths.clone_from(&base.canonical_lengths);
        self.callee_specs.clone_from(&base.callee_specs);
    }

    /// Return bit width for fixed-width type tokens (`u8`, `i32`, etc.).
    ///
    /// Delegates to [`crate::prelude_policy::fixed_width_bits`] (shared with CVC5, #453).
    pub(crate) fn fixed_width_bits(ty: &[String]) -> Option<(u32, bool)> {
        crate::prelude_policy::fixed_width_bits(ty)
    }

    /// Register a parameter as a fixed-width bitvector variable (#265).
    pub(crate) fn register_fixed_width_param(&mut self, name: &str, width: u32, signed: bool) {
        let bv = BitvectorEncoder::bv_const(name, width);
        self.vars.insert(name.to_string(), Z3Value::Bv(bv, signed));
        self.bv_signed.insert(name.to_string(), signed);
    }

    /// Register `result` / `__result` when the contract return type is fixed-width (#851).
    pub(crate) fn register_fixed_width_return(&mut self, return_ty: &[String]) {
        if let Some((width, signed)) = Self::fixed_width_bits(return_ty) {
            self.register_fixed_width_param("result", width, signed);
            self.register_fixed_width_param(
                crate::encode_atom_policy::RESULT_VAR_NAME,
                width,
                signed,
            );
        }
    }

    /// Touch bitvector infrastructure (ensures helpers are linked in verify path).
    pub(crate) fn init_bitvector_infrastructure(&mut self) {
        BITVECTOR_API_WIRED.call_once(|| {
            let _ = BitvectorEncoder::wire_api_surface();
        });
    }

    /// Canonical non-negative length variable for `name.length()` (#267).
    pub(crate) fn canonical_length(&mut self, name: &str) -> ast::Int {
        if let Some(v) = self.canonical_lengths.get(name) {
            return v.clone();
        }
        let key = crate::encode_atom_policy::canonical_length_name(name);
        let v = ast::Int::new_const(key.as_str());
        let zero = ast::Int::from_i64(0);
        self.background_axioms.push(v.ge(&zero));
        // Link to `len` / `__field_len` UIFs so concat/array axioms agree with `.len` (#267).
        let obj = self.get_or_create_int(name);
        for uf_name in crate::encode_atom_policy::length_uf_names() {
            let len_decl = self.make_func(uf_name, 1);
            let uif_len = len_decl
                .apply(&[&obj as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| self.fresh_int());
            self.background_axioms.push(uif_len.eq(&v));
        }
        self.canonical_lengths.insert(name.to_string(), v.clone());
        v
    }

    /// Get or create a named integer variable.
    pub(crate) fn get_or_create_int(&mut self, name: &str) -> ast::Int {
        if let Some(val) = self.vars.get(name) {
            return val.as_int(&mut self.fresh_counter);
        }
        let v = ast::Int::new_const(name);
        self.vars.insert(name.to_string(), Z3Value::Int(v.clone()));
        v
    }

    /// Create a fresh unconstrained boolean.
    pub(crate) fn fresh_bool(&mut self) -> ast::Bool {
        self.fresh_counter += 1;
        ast::Bool::new_const(
            crate::encode_atom_policy::fresh_temp_name(self.fresh_counter).as_str(),
        )
    }

    /// Create a fresh unconstrained integer.
    pub(crate) fn fresh_int(&mut self) -> ast::Int {
        self.fresh_counter += 1;
        ast::Int::new_const(crate::encode_atom_policy::fresh_temp_name(self.fresh_counter).as_str())
    }

    /// Create an uninterpreted function declaration (Int^arity -> Int).
    /// Z3 internally deduplicates declarations with the same name and sorts.
    pub(crate) fn make_func(&mut self, name: &str, arity: usize) -> z3::FuncDecl {
        self.func_arities.insert(name.to_string(), arity);
        let int_sort = z3::Sort::int();
        let param_sorts: Vec<&z3::Sort> = (0..arity).map(|_| &int_sort).collect();
        z3::FuncDecl::new(name, &param_sorts, &int_sort)
    }

    /// Length of a sequence/collection encoded as an `Int` proxy.
    /// Uses canonical length for named identifiers (`s.length()` / `len(s)`).
    pub(crate) fn collection_len_of(
        &mut self,
        coll_expr: &Expr,
        coll_int: &ast::Int,
        len_uf: &str,
    ) -> ast::Int {
        if let Expr::Ident(name) = coll_expr {
            return self.canonical_length(name);
        }
        let len_decl = self.make_func(len_uf, 1);
        len_decl
            .apply(&[coll_int as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int())
    }

    /// Assert `len_uf(obj) == val` and `val >= 0`.
    /// When `len_uf` is `len` or `__field_len`, also links the other alias.
    pub(crate) fn assert_collection_len_eq(
        &mut self,
        obj: &ast::Int,
        val: &ast::Int,
        len_uf: &str,
    ) {
        let len_decl = self.make_func(len_uf, 1);
        let got = len_decl
            .apply(&[obj as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());
        let zero = ast::Int::from_i64(0);
        self.background_axioms.push(got.eq(val));
        self.background_axioms.push(val.ge(&zero));
        if crate::encode_atom_policy::is_length_uf_name(len_uf) {
            for other in crate::encode_atom_policy::length_uf_names() {
                if other != len_uf {
                    let d = self.make_func(other, 1);
                    let o = d
                        .apply(&[obj as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    self.background_axioms.push(o.eq(val));
                }
            }
        }
    }

    /// Hash a pattern name to a stable i64 for Z3 encoding.
    ///
    /// Delegates to [`crate::encode_method_policy::pattern_hash_name`] (FNV-1a,
    /// shared with CVC5 match/IR tag encoding).
    pub(crate) fn pattern_hash(&self, name: &str) -> i64 {
        crate::encode_method_policy::pattern_hash_name(name)
    }

    /// Encode a literal value to Z3.
    pub(crate) fn encode_literal(&self, lit: &Literal) -> Z3Value {
        match lit {
            Literal::Int(s) => {
                if let Ok(n) = s.parse::<i64>() {
                    Z3Value::Int(ast::Int::from_i64(n))
                } else if let Some(rest) = s.strip_prefix('-') {
                    let abs_val: ast::Int = rest.parse().unwrap_or_else(|_| ast::Int::from_i64(0));
                    Z3Value::Int(abs_val.unary_minus())
                } else {
                    let val: ast::Int = s.parse().unwrap_or_else(|_| ast::Int::from_i64(0));
                    Z3Value::Int(val)
                }
            }
            Literal::Float(s) => {
                let (numer, denom) = crate::encode_atom_policy::float_to_rational_parts(s);
                Z3Value::Real(ast::Real::from_rational(numer, denom))
            }
            Literal::Bool(b) => Z3Value::Bool(ast::Bool::from_bool(*b)),
            Literal::Str(_) => Z3Value::Int(ast::Int::from_i64(self.fresh_counter as i64)),
        }
    }
}
