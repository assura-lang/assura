//! Encoder construction, ADT emulation, quantifiers, and non-expr encode helpers.

use crate::*;
use assura_ast::{Literal, SpExpr};
use assura_types::checkers::expr_references_var;
use std::collections::HashMap;
use z3::ast;

use super::BitvectorEncoder;

use super::value::Z3Value;
use super::{AdtConstructor, AdtDef, BITVECTOR_API_WIRED, Encoder};

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
    }

    /// Register baseline ADT infrastructure used by match-pattern encoding.
    pub(crate) fn init_adt_infrastructure(&mut self) {
        if !self.adt_defs.contains_key("Option") {
            self.define_adt("Option", &[("Some", &["value"]), ("None", &[])]);
        }
    }

    /// Register a struct-like ADT for IR `field` / `construct` encoding.
    pub(crate) fn ensure_struct_adt(&mut self, type_name: &str, field_names: &[String]) {
        if field_names.is_empty() || self.adt_defs.contains_key(type_name) {
            return;
        }
        let accessors: Vec<&str> = field_names.iter().map(String::as_str).collect();
        self.define_adt(type_name, &[(type_name, accessors.as_slice())]);
    }

    /// Return bit width for fixed-width type tokens (`u8`, `i32`, etc.).
    pub(crate) fn fixed_width_bits(ty: &[String]) -> Option<(u32, bool)> {
        if ty.len() != 1 {
            return None;
        }
        match ty[0].as_str() {
            "u8" => Some((8, false)),
            "u16" => Some((16, false)),
            "u32" => Some((32, false)),
            "u64" => Some((64, false)),
            "i8" => Some((8, true)),
            "i16" => Some((16, true)),
            "i32" => Some((32, true)),
            "i64" => Some((64, true)),
            _ => None,
        }
    }

    /// Register a parameter as a fixed-width bitvector variable (#265).
    pub(crate) fn register_fixed_width_param(&mut self, name: &str, width: u32, signed: bool) {
        let bv = BitvectorEncoder::bv_const(name, width);
        self.vars.insert(name.to_string(), Z3Value::Bv(bv));
        self.bv_signed.insert(name.to_string(), signed);
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

    /// Build a domain guard for quantifier bodies.
    ///
    /// For range domains (`lo..hi`):
    /// - `is_forall=true`:  `(lo <= x && x < hi) => body`
    /// - `is_forall=false`: `(lo <= x && x < hi) && body`
    ///
    /// For non-range domains (collections, identifiers), encode
    /// membership as an uninterpreted `contains(domain, x)` predicate.
    ///
    /// Domain classification shares [`crate::encode_quantifier_policy::domain_as_range`];
    /// term construction stays Z3-local (mirrors CVC5 `guard_quantifier_body_cvc5`).
    pub(crate) fn guard_quantifier_body(
        &mut self,
        domain: &SpExpr,
        bound: &ast::Int,
        body: &ast::Bool,
        is_forall: bool,
    ) -> ast::Bool {
        if let Some((lo, hi)) = crate::encode_quantifier_policy::domain_as_range(domain) {
            let lo_val = self.encode_expr(lo).as_int(&mut self.fresh_counter);
            let hi_val = self.encode_expr(hi).as_int(&mut self.fresh_counter);
            let ge_lo = bound.ge(&lo_val);
            let lt_hi = bound.lt(&hi_val);
            let in_range = ast::Bool::and(&[&ge_lo, &lt_hi]);
            if is_forall {
                in_range.implies(body)
            } else {
                ast::Bool::and(&[&in_range, body])
            }
        } else {
            // Non-range domain: encode as uninterpreted contains(domain, x)
            let int_sort = z3::Sort::int();
            let bool_sort = z3::Sort::bool();
            let contains_fn = z3::FuncDecl::new(
                crate::encode_quantifier_policy::DOMAIN_CONTAINS_UF_NAME,
                &[&int_sort, &int_sort],
                &bool_sort,
            );
            let domain_val = self.encode_expr(domain).as_int(&mut self.fresh_counter);
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
                ast::Bool::and(&[&membership, body])
            }
        }
    }

    /// Infer Z3 trigger patterns from function calls in a quantifier body
    /// that reference the bound variable. Returns patterns for e-matching
    /// hints that help the solver instantiate quantifiers efficiently.
    pub(crate) fn infer_quantifier_patterns(
        &mut self,
        body: &SpExpr,
        bound_var: &str,
        bound_z3: &ast::Int,
    ) -> Vec<z3::Pattern> {
        let mut patterns = Vec::new();

        // Prefer AST-based trigger inference (Tier A2), then string fallback.
        if let Some(trigger) = self
            .trigger_manager
            .infer_trigger_from_expr(body, bound_var)
        {
            // Production wiring for validate_trigger: record warnings on the manager.
            let _trigger_warnings = self.trigger_manager.validate_trigger(&trigger);
            for term in &trigger.terms {
                if let Some(fname) = term.split('(').next() {
                    let int_sort = z3::Sort::int();
                    let func = z3::FuncDecl::new(fname.trim(), &[&int_sort], &int_sort);
                    let app = func.apply(&[bound_z3 as &dyn z3::ast::Ast]);
                    let pat = z3::Pattern::new(&[&app]);
                    patterns.push(pat);
                }
            }
        }

        // Direct scan: look for Call expressions that reference the bound variable
        if patterns.is_empty() {
            self.collect_trigger_calls(body, bound_var, bound_z3, &mut patterns);
        }

        patterns
    }

    /// Recursively scan an expression for function calls containing the
    /// bound variable, and create Z3 trigger patterns from them.
    pub(crate) fn collect_trigger_calls(
        &self,
        expr: &SpExpr,
        bound_var: &str,
        bound_z3: &ast::Int,
        patterns: &mut Vec<z3::Pattern>,
    ) {
        match &expr.node {
            Expr::Call { func, args } => {
                let refs_bound = args.iter().any(|a| expr_references_var(a, bound_var));
                if refs_bound && let Expr::Ident(fname) = &func.as_ref().node {
                    let int_sort = z3::Sort::int();
                    let arity = args.len();
                    let param_sorts: Vec<&z3::Sort> = (0..arity).map(|_| &int_sort).collect();
                    let func_decl = z3::FuncDecl::new(fname.as_str(), &param_sorts, &int_sort);
                    let z3_args: Vec<ast::Dynamic> = args
                        .iter()
                        .map(|a| {
                            if expr_references_var(a, bound_var) {
                                ast::Dynamic::from_ast(bound_z3)
                            } else {
                                ast::Dynamic::from_ast(&ast::Int::new_const(
                                    crate::encode_atom_policy::TRIGGER_OTHER_NAME,
                                ))
                            }
                        })
                        .collect();
                    let arg_refs: Vec<&dyn z3::ast::Ast> =
                        z3_args.iter().map(|d| d as &dyn z3::ast::Ast).collect();
                    let app = func_decl.apply(&arg_refs);
                    let pat = z3::Pattern::new(&[&app]);
                    patterns.push(pat);
                }
                for a in args {
                    self.collect_trigger_calls(a, bound_var, bound_z3, patterns);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.collect_trigger_calls(receiver, bound_var, bound_z3, patterns);
                for a in args {
                    self.collect_trigger_calls(a, bound_var, bound_z3, patterns);
                }
            }
            Expr::BinOp { lhs, rhs, .. } => {
                self.collect_trigger_calls(lhs, bound_var, bound_z3, patterns);
                self.collect_trigger_calls(rhs, bound_var, bound_z3, patterns);
            }
            Expr::UnaryOp { expr: e, .. } | Expr::Old(e) | Expr::Ghost(e) => {
                self.collect_trigger_calls(e, bound_var, bound_z3, patterns);
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.collect_trigger_calls(cond, bound_var, bound_z3, patterns);
                self.collect_trigger_calls(then_branch, bound_var, bound_z3, patterns);
                if let Some(eb) = else_branch {
                    self.collect_trigger_calls(eb, bound_var, bound_z3, patterns);
                }
            }
            Expr::Index { expr: e, index } => {
                self.collect_trigger_calls(e, bound_var, bound_z3, patterns);
                self.collect_trigger_calls(index, bound_var, bound_z3, patterns);
            }
            _ => {}
        }
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

    /// Encode a function call as an uninterpreted function application.
    /// Known boolean methods return Bool; everything else returns Int.
    pub(crate) fn adt_for_constructor(&self, ctor_name: &str) -> Option<String> {
        self.adt_defs.iter().find_map(|(adt_name, def)| {
            def.constructors
                .iter()
                .any(|c| c.name == ctor_name)
                .then_some(adt_name.clone())
        })
    }

    /// Encode a function/method call to Z3 (integer-encoding mode).
    ///
    /// Dispatch order for non-ADT / non-string-theory special cases is documented
    /// by [`crate::encode_call_policy::classify_encode_call`] (min/max → bool UF →
    /// sequence/string builtins → abs → get/set/put → size UF → uninterpreted).
    /// Guards use [`crate::encode_method_policy`]; term construction stays here.
    pub(crate) fn encode_call(&mut self, func_name: &str, args: &[SpExpr]) -> Z3Value {
        if func_name.chars().next().is_some_and(|c| c.is_uppercase()) {
            self.init_adt_infrastructure();
            let arg_vals: Vec<ast::Int> = args
                .iter()
                .map(|a| self.encode_expr(a).as_int(&mut self.fresh_counter))
                .collect();
            if let Some(adt_name) = self.adt_for_constructor(func_name) {
                return Z3Value::Int(self.adt_constructor(&adt_name, func_name, &arg_vals));
            }
        }

        // Native string theory: length(str_val) uses Z3's str.len
        if self.use_string_theory
            && crate::encode_atom_policy::is_length_method_name(func_name)
            && args.len() == 1
        {
            let arg_val = self.encode_expr(&args[0]);
            if let Z3Value::Str(s) = &arg_val {
                let len = s.length();
                let zero = ast::Int::from_i64(0);
                self.background_axioms.push(len.ge(&zero));
                return Z3Value::Int(len);
            }
        }

        // Canonical length for simple identifiers (#267).
        if crate::encode_atom_policy::is_length_method_name(func_name)
            && args.len() == 1
            && let Expr::Ident(name) = &args[0].node
        {
            return Z3Value::Int(self.canonical_length(name));
        }

        let arg_vals: Vec<ast::Int> = args
            .iter()
            .map(|a| self.encode_expr(a).as_int(&mut self.fresh_counter))
            .collect();
        // Single classify pass (parity with CVC5 / encode_call_policy order); term
        // bodies stay in each arm. Guards use `call_kind` instead of repeating
        // `is_*_builtin` + `debug_assert_encode_call_kind` pairs.
        use crate::encode_call_policy::{EncodeCallKind, classify_encode_call};
        let call_kind = classify_encode_call(func_name, arg_vals.len());

        // min/max: encode with ite so Z3 proves bounds (not unconstrained UF).
        // e.g. ensures { min(a, b) <= a } verifies under any a, b.
        if matches!(call_kind, EncodeCallKind::MinMax) {
            let a = &arg_vals[0];
            let b = &arg_vals[1];
            let a_le_b = a.le(b);
            let result = if crate::encode_method_policy::is_min_builtin(func_name, arg_vals.len()) {
                a_le_b.ite(a, b)
            } else {
                debug_assert!(crate::encode_method_policy::is_max_builtin(
                    func_name,
                    arg_vals.len()
                ));
                a_le_b.ite(b, a)
            };
            return Z3Value::Int(result);
        }
        // Methods known to return Bool (UF with optional length / size links below).
        // Table lives in encode_method_policy (parity with CVC5 / methods.rs).
        if matches!(call_kind, EncodeCallKind::BoolReturningUf) {
            let bool_sort = z3::Sort::bool();
            let int_sort = z3::Sort::int();
            let param_sorts: Vec<&z3::Sort> = (0..arg_vals.len()).map(|_| &int_sort).collect();
            let decl = z3::FuncDecl::new(func_name, &param_sorts, &bool_sort);
            let arg_refs: Vec<&dyn z3::ast::Ast> =
                arg_vals.iter().map(|a| a as &dyn z3::ast::Ast).collect();
            let result = decl.apply(&arg_refs);
            let b = result.as_bool().unwrap_or_else(|| self.fresh_bool());
            // is_empty(x) <=> len(x) == 0 (sound for sequences/maps with size).
            if func_name == "is_empty" && arg_vals.len() == 1 {
                let coll = &arg_vals[0];
                let coll_expr = &args[0].node;
                let len_val = self.collection_len_of(coll_expr, coll, "len");
                let zero = ast::Int::from_i64(0);
                let len_is_zero = len_val.eq(&zero);
                // Both directions: empty iff length zero.
                self.background_axioms.push(b.implies(&len_is_zero));
                self.background_axioms.push(len_is_zero.implies(&b));
            }
            // contains(s, sub) => len(s) >= len(sub) (contiguous substring; sound).
            if func_name == "contains" && arg_vals.len() == 2 {
                let hay_expr = &args[0].node;
                let needle_expr = &args[1].node;
                let hay_len = self.collection_len_of(hay_expr, &arg_vals[0], "len");
                let needle_len = self.collection_len_of(needle_expr, &arg_vals[1], "len");
                let zero = ast::Int::from_i64(0);
                self.background_axioms.push(hay_len.ge(&zero));
                self.background_axioms.push(needle_len.ge(&zero));
                let hay_ge_needle = hay_len.ge(&needle_len);
                self.background_axioms.push(b.implies(&hay_ge_needle));
            }
            // starts_with(s, pre) / ends_with(s, suf) => len(s) >= len(pre/suf) (sound).
            if matches!(func_name, "starts_with" | "ends_with") && arg_vals.len() == 2 {
                let s_expr = &args[0].node;
                let aff_expr = &args[1].node;
                let s_len = self.collection_len_of(s_expr, &arg_vals[0], "len");
                let aff_len = self.collection_len_of(aff_expr, &arg_vals[1], "len");
                let zero = ast::Int::from_i64(0);
                self.background_axioms.push(s_len.ge(&zero));
                self.background_axioms.push(aff_len.ge(&zero));
                let s_ge_aff = s_len.ge(&aff_len);
                self.background_axioms.push(b.implies(&s_ge_aff));
                // Empty affix: starts_with/ends_with always hold (prefix/suffix of length 0).
                let aff_is_zero = aff_len.eq(&zero);
                self.background_axioms.push(aff_is_zero.implies(&b));
            }
            // contains_key(m, k) => size(m) >= 1 (key present implies non-empty map; sound).
            if func_name == "contains_key" && arg_vals.len() == 2 {
                let map_expr = &args[0].node;
                let map_size = self.collection_len_of(map_expr, &arg_vals[0], "size");
                // Also link size <-> len for maps (size method vs len).
                let map_len = self.collection_len_of(map_expr, &arg_vals[0], "len");
                self.background_axioms.push(map_size.eq(&map_len));
                let one = ast::Int::from_i64(1);
                let zero = ast::Int::from_i64(0);
                self.background_axioms.push(map_size.ge(&zero));
                let size_ge_one = map_size.ge(&one);
                self.background_axioms.push(b.implies(&size_ge_one));
            }
            return Z3Value::Bool(b);
        }
        // String / sequence methods with known semantics (arity via encode_method_policy).
        if matches!(call_kind, EncodeCallKind::Substring) {
            // substring(str, start, end): length == end - start; 0 <= start <= end <= len(str)
            let str_expr = &args[0].node;
            let str_val = &arg_vals[0];
            let start = &arg_vals[1];
            let end = &arg_vals[2];
            let result = self.fresh_int();
            let zero = ast::Int::from_i64(0);
            self.background_axioms.push(start.ge(&zero));
            self.background_axioms.push(start.le(end));
            let str_len = self.collection_len_of(
                str_expr,
                str_val,
                crate::encode_atom_policy::FIELD_LEN_UF_NAME,
            );
            self.background_axioms.push(end.le(&str_len));
            let diff = ast::Int::sub(&[end, start]);
            self.assert_collection_len_eq(
                &result,
                &diff,
                crate::encode_atom_policy::FIELD_LEN_UF_NAME,
            );
            return Z3Value::Int(result);
        }
        if matches!(call_kind, EncodeCallKind::ConcatAppend) {
            // concat(a, b) / append(a, b): len(result) == len(a) + len(b)
            let l_expr = &args[0].node;
            let r_expr = &args[1].node;
            let l = &arg_vals[0];
            let r = &arg_vals[1];
            let result = self.fresh_int();
            let len_l =
                self.collection_len_of(l_expr, l, crate::encode_atom_policy::FIELD_LEN_UF_NAME);
            let len_r =
                self.collection_len_of(r_expr, r, crate::encode_atom_policy::FIELD_LEN_UF_NAME);
            let zero = ast::Int::from_i64(0);
            self.background_axioms.push(len_l.ge(&zero));
            self.background_axioms.push(len_r.ge(&zero));
            let sum = ast::Int::add(&[&len_l, &len_r]);
            self.assert_collection_len_eq(
                &result,
                &sum,
                crate::encode_atom_policy::FIELD_LEN_UF_NAME,
            );
            // Also result length >= each operand (redundant but helps some goals).
            self.background_axioms.push(sum.ge(&len_l));
            self.background_axioms.push(sum.ge(&len_r));
            return Z3Value::Int(result);
        }
        if matches!(call_kind, EncodeCallKind::IndexOf) {
            // index_of(str, substr): -1 <= result < len(str)
            let str_expr = &args[0].node;
            let str_val = &arg_vals[0];
            let result = self.fresh_int();
            let neg_one = ast::Int::from_i64(-1);
            self.background_axioms.push(result.ge(&neg_one));
            let str_len = self.collection_len_of(
                str_expr,
                str_val,
                crate::encode_atom_policy::FIELD_LEN_UF_NAME,
            );
            // result < len(str) covers both found indices and -1 when len >= 0.
            self.background_axioms.push(result.lt(&str_len));
            return Z3Value::Int(result);
        }
        if matches!(call_kind, EncodeCallKind::CharAt) {
            // char_at(str, idx): 0 <= idx < len(str)
            let str_expr = &args[0].node;
            let str_val = &arg_vals[0];
            let idx = &arg_vals[1];
            let zero = ast::Int::from_i64(0);
            self.background_axioms.push(idx.ge(&zero));
            let str_len = self.collection_len_of(
                str_expr,
                str_val,
                crate::encode_atom_policy::FIELD_LEN_UF_NAME,
            );
            self.background_axioms.push(idx.lt(&str_len));
            return Z3Value::Int(self.fresh_int());
        }
        if matches!(call_kind, EncodeCallKind::Replace) {
            // replace(str, old, new): result length >= 0 (weak; no exact length)
            let result = self.fresh_int();
            let res_len = self.fresh_int();
            let zero = ast::Int::from_i64(0);
            self.background_axioms.push(res_len.ge(&zero));
            self.assert_collection_len_eq(
                &result,
                &res_len,
                crate::encode_atom_policy::FIELD_LEN_UF_NAME,
            );
            return Z3Value::Int(result);
        }
        // Remaining collection/string methods (dispatch arity/name via encode_method_policy).
        if matches!(call_kind, EncodeCallKind::Split) {
            // split(str, delim): returns collection with len >= 1
            let result = self.fresh_int();
            let one = ast::Int::from_i64(1);
            let len_decl = self.make_func(crate::encode_atom_policy::LEN_UF_NAME, 1);
            let res_len = len_decl
                .apply(&[&result as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| self.fresh_int());
            self.background_axioms.push(res_len.ge(&one));
            return Z3Value::Int(result);
        }
        if matches!(call_kind, EncodeCallKind::TrimOrCaseFold) {
            // trim/to_lower/to_upper: result length <= input length
            let str_expr = &args[0].node;
            let str_val = &arg_vals[0];
            let result = self.fresh_int();
            let str_len = self.collection_len_of(
                str_expr,
                str_val,
                crate::encode_atom_policy::FIELD_LEN_UF_NAME,
            );
            let len_decl = self.make_func(crate::encode_atom_policy::FIELD_LEN_UF_NAME, 1);
            let res_len = len_decl
                .apply(&[&result as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| self.fresh_int());
            let zero = ast::Int::from_i64(0);
            self.background_axioms.push(res_len.ge(&zero));
            self.background_axioms.push(res_len.le(&str_len));
            return Z3Value::Int(result);
        }
        if matches!(call_kind, EncodeCallKind::CloneOrReverse) {
            // Length-preserving views/copies / reverse.
            let src_expr = &args[0].node;
            let src = &arg_vals[0];
            let result = self.fresh_int();
            let old_len = self.collection_len_of(src_expr, src, "len");
            self.assert_collection_len_eq(&result, &old_len, "len");
            return Z3Value::Int(result);
        }
        if matches!(call_kind, EncodeCallKind::Clear) {
            // clear(seq): length == 0
            let result = self.fresh_int();
            let zero = ast::Int::from_i64(0);
            self.assert_collection_len_eq(&result, &zero, "len");
            return Z3Value::Int(result);
        }
        if matches!(call_kind, EncodeCallKind::Push) {
            // push(seq, elem) / push_back: length = old + 1
            let src_expr = &args[0].node;
            let src = &arg_vals[0];
            let result = self.fresh_int();
            let one = ast::Int::from_i64(1);
            let old_len = self.collection_len_of(src_expr, src, "len");
            let new_len = ast::Int::add(&[&old_len, &one]);
            self.assert_collection_len_eq(&result, &new_len, "len");
            return Z3Value::Int(result);
        }
        if matches!(call_kind, EncodeCallKind::PopOrTail) {
            // pop / tail / rest: length = max(0, old - 1)
            let src_expr = &args[0].node;
            let src = &arg_vals[0];
            let result = self.fresh_int();
            let zero = ast::Int::from_i64(0);
            let one = ast::Int::from_i64(1);
            let old_len = self.collection_len_of(src_expr, src, "len");
            let dec = ast::Int::sub(&[&old_len, &one]);
            let new_len = old_len.ge(&one).ite(&dec, &zero);
            self.assert_collection_len_eq(&result, &new_len, "len");
            return Z3Value::Int(result);
        }
        if matches!(call_kind, EncodeCallKind::Insert) {
            // insert(seq, idx, val): length = old + 1; get(result, idx) == val
            let src_expr = &args[0].node;
            let src = &arg_vals[0];
            let idx = &arg_vals[1];
            let val = &arg_vals[2];
            let result = self.fresh_int();
            let one = ast::Int::from_i64(1);
            let zero = ast::Int::from_i64(0);
            let old_len = self.collection_len_of(src_expr, src, "len");
            let new_len = ast::Int::add(&[&old_len, &one]);
            self.assert_collection_len_eq(&result, &new_len, "len");
            self.background_axioms.push(idx.ge(&zero));
            self.background_axioms.push(idx.le(&old_len));
            let get_decl = self.make_func(crate::encode_atom_policy::INDEX_UF_NAME, 2);
            let at_idx = get_decl
                .apply(&[&result as &dyn z3::ast::Ast, idx as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| self.fresh_int());
            self.background_axioms.push(at_idx.eq(val));
            return Z3Value::Int(result);
        }
        if matches!(call_kind, EncodeCallKind::Remove) {
            // remove(seq, idx) / remove_at: length = max(0, old - 1)
            let src_expr = &args[0].node;
            let src = &arg_vals[0];
            let result = self.fresh_int();
            let zero = ast::Int::from_i64(0);
            let one = ast::Int::from_i64(1);
            let old_len = self.collection_len_of(src_expr, src, "len");
            let dec = ast::Int::sub(&[&old_len, &one]);
            let new_len = old_len.ge(&one).ite(&dec, &zero);
            self.assert_collection_len_eq(&result, &new_len, "len");
            return Z3Value::Int(result);
        }
        if matches!(call_kind, EncodeCallKind::Slice) {
            // slice(seq, start, end)
            let src_expr = &args[0].node;
            let src = &arg_vals[0];
            let start = &arg_vals[1];
            let end = &arg_vals[2];
            let result = self.fresh_int();
            let zero = ast::Int::from_i64(0);
            let old_len = self.collection_len_of(src_expr, src, "len");
            self.background_axioms.push(start.ge(&zero));
            self.background_axioms.push(start.le(end));
            self.background_axioms.push(end.le(&old_len));
            let diff = ast::Int::sub(&[end, start]);
            self.assert_collection_len_eq(&result, &diff, "len");
            return Z3Value::Int(result);
        }
        if matches!(call_kind, EncodeCallKind::Take) {
            let src_expr = &args[0].node;
            let src = &arg_vals[0];
            let n = &arg_vals[1];
            let result = self.fresh_int();
            let zero = ast::Int::from_i64(0);
            let old_len = self.collection_len_of(src_expr, src, "len");
            self.background_axioms.push(n.ge(&zero));
            let taken = n.le(&old_len).ite(n, &old_len);
            self.assert_collection_len_eq(&result, &taken, "len");
            return Z3Value::Int(result);
        }
        if matches!(call_kind, EncodeCallKind::Drop) {
            let src_expr = &args[0].node;
            let src = &arg_vals[0];
            let n = &arg_vals[1];
            let result = self.fresh_int();
            let zero = ast::Int::from_i64(0);
            let old_len = self.collection_len_of(src_expr, src, "len");
            self.background_axioms.push(n.ge(&zero));
            let rem = ast::Int::sub(&[&old_len, n]);
            let dropped = n.le(&old_len).ite(&rem, &zero);
            self.assert_collection_len_eq(&result, &dropped, "len");
            return Z3Value::Int(result);
        }
        if matches!(call_kind, EncodeCallKind::First) {
            // first/last/head: weak (no value); length of source > 0 if used in requires separately
            return Z3Value::Int(self.fresh_int());
        }
        // abs(x) => if x >= 0 then x else -x (policy arity; min/max handled above).
        if matches!(call_kind, EncodeCallKind::Abs) {
            let x = &arg_vals[0];
            let zero = ast::Int::from_i64(0);
            let neg_x = x.unary_minus();
            let cond = x.ge(&zero);
            return Z3Value::Int(cond.ite(x, &neg_x));
        }
        // get(coll, key_or_idx): uninterpreted; unify `get` with `__index` for arrays.
        if matches!(call_kind, EncodeCallKind::Get) {
            let coll = &arg_vals[0];
            let key = &arg_vals[1];
            let get_decl = self.make_func(crate::encode_atom_policy::GET_UF_NAME, 2);
            let via_get = get_decl
                .apply(&[coll as &dyn z3::ast::Ast, key as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| self.fresh_int());
            let idx_decl = self.make_func(crate::encode_atom_policy::INDEX_UF_NAME, 2);
            let via_idx = idx_decl
                .apply(&[coll as &dyn z3::ast::Ast, key as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| self.fresh_int());
            self.background_axioms.push(via_get.eq(&via_idx));
            return Z3Value::Int(via_get);
        }
        // Array set(arr, index, value): store axiom + length preserve.
        // set(a, i, v) returns a new array where get(result, i) == v.
        if matches!(call_kind, EncodeCallKind::Set) {
            let arr_expr = &args[0].node;
            let arr = &arg_vals[0];
            let idx = &arg_vals[1];
            let val = &arg_vals[2];
            let result = self.fresh_int();
            let zero = ast::Int::from_i64(0);
            // Weak index non-negativity (callers often require i >= 0 separately).
            self.background_axioms.push(idx.ge(&zero));
            // Read-over-write via both get and __index (keep aliases aligned).
            let get_decl = self.make_func(crate::encode_atom_policy::GET_UF_NAME, 2);
            let get_at_idx = get_decl
                .apply(&[&result as &dyn z3::ast::Ast, idx as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| self.fresh_int());
            self.background_axioms.push(get_at_idx.eq(val));
            let idx_decl = self.make_func(crate::encode_atom_policy::INDEX_UF_NAME, 2);
            let via_idx = idx_decl
                .apply(&[&result as &dyn z3::ast::Ast, idx as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| self.fresh_int());
            self.background_axioms.push(via_idx.eq(val));
            // len(result) == len(original); use canonical length for named arrays (#267).
            let old_len = self.collection_len_of(arr_expr, arr, "len");
            self.assert_collection_len_eq(&result, &old_len, "len");
            return Z3Value::Int(result);
        }
        // Map put(map, key, value): get(put(m,k,v), k) == v; size non-decreasing.
        if matches!(call_kind, EncodeCallKind::Put) {
            let map_expr = &args[0].node;
            let map_val = &arg_vals[0];
            let key = &arg_vals[1];
            let value = &arg_vals[2];
            let new_map = self.fresh_int();
            // Read-over-write axiom: get(put(m, k, v), k) == v
            let get_decl = self.make_func(crate::encode_atom_policy::GET_UF_NAME, 2);
            let get_result = get_decl
                .apply(&[&new_map as &dyn z3::ast::Ast, key as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| self.fresh_int());
            self.background_axioms.push(get_result.eq(value));
            // contains_key(put(m, k, v), k) always holds (write implies key present).
            let bool_sort = z3::Sort::bool();
            let int_sort = z3::Sort::int();
            let ck_decl = z3::FuncDecl::new("contains_key", &[&int_sort, &int_sort], &bool_sort);
            let ck = ck_decl
                .apply(&[&new_map as &dyn z3::ast::Ast, key as &dyn z3::ast::Ast])
                .as_bool()
                .unwrap_or_else(|| self.fresh_bool());
            self.background_axioms.push(ck);
            // size(new_map) >= size(map); link size <-> len on both maps.
            let old_size = self.collection_len_of(map_expr, map_val, "size");
            let old_len = self.collection_len_of(map_expr, map_val, "len");
            self.background_axioms.push(old_size.eq(&old_len));
            let size_decl = self.make_func(crate::encode_atom_policy::SIZE_UF_NAME, 1);
            let new_size = size_decl
                .apply(&[&new_map as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| self.fresh_int());
            let zero = ast::Int::from_i64(0);
            let one = ast::Int::from_i64(1);
            self.background_axioms.push(new_size.ge(&old_size));
            self.background_axioms.push(new_size.ge(&zero));
            // Key present => size at least 1.
            self.background_axioms.push(new_size.ge(&one));
            let len_decl = self.make_func(crate::encode_atom_policy::LEN_UF_NAME, 1);
            let new_len = len_decl
                .apply(&[&new_map as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| self.fresh_int());
            self.background_axioms.push(new_len.eq(&new_size));
            return Z3Value::Int(new_map);
        }
        // Size-like methods get non-negativity axiom; unify len/length/size/__field_len.
        if matches!(call_kind, EncodeCallKind::SizeFieldUf) && arg_vals.len() == 1 {
            let coll_expr = &args[0].node;
            let coll = &arg_vals[0];
            // Named collections: always use the canonical length variable.
            if let Expr::Ident(name) = coll_expr {
                return Z3Value::Int(self.canonical_length(name));
            }
            let len_val = self.collection_len_of(coll_expr, coll, "len");
            let zero = ast::Int::from_i64(0);
            self.background_axioms.push(len_val.ge(&zero));
            // Link the requested method UF to the same length value.
            if func_name != "len" {
                let decl = self.make_func(func_name, 1);
                let via_method = decl
                    .apply(&[coll as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                self.background_axioms.push(via_method.eq(&len_val));
            }
            // Keep __field_len aligned (string/method `.length()` on temporaries).
            let fl = self.make_func(crate::encode_atom_policy::FIELD_LEN_UF_NAME, 1);
            let via_fl = fl
                .apply(&[coll as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| self.fresh_int());
            self.background_axioms.push(via_fl.eq(&len_val));
            return Z3Value::Int(len_val);
        }
        if matches!(call_kind, EncodeCallKind::SizeFieldUf) {
            let decl = self.make_func(func_name, arg_vals.len());
            let arg_refs: Vec<&dyn z3::ast::Ast> =
                arg_vals.iter().map(|a| a as &dyn z3::ast::Ast).collect();
            let result = decl.apply(&arg_refs);
            let len_val = result.as_int().unwrap_or_else(|| self.fresh_int());
            let zero = ast::Int::from_i64(0);
            self.background_axioms.push(len_val.ge(&zero));
            return Z3Value::Int(len_val);
        }
        debug_assert!(
            matches!(call_kind, EncodeCallKind::UninterpretedUf),
            "encode_call fallthrough unexpected kind {call_kind:?} for {func_name}"
        );
        let decl = self.make_func(func_name, arg_vals.len());
        let arg_refs: Vec<&dyn z3::ast::Ast> =
            arg_vals.iter().map(|a| a as &dyn z3::ast::Ast).collect();
        let result = decl.apply(&arg_refs);
        Z3Value::Int(result.as_int().unwrap_or_else(|| self.fresh_int()))
    }

    /// Encode field access as uninterpreted function: field_name(object).
    /// Known boolean fields return Bool; size fields return non-negative Int.
    ///
    /// Note (#191): When the object is `result` (a free Z3 variable), the
    /// field access `__field_len(result)` is also unconstrained. This means
    /// ensures clauses like `result.length() <= raw.length()` produce
    /// spurious counterexamples because Z3 can assign any value to
    /// `__field_len(result)`. This is a known limitation; see the doc
    /// comment on `verify_clauses_with_types` for details.
    pub(crate) fn encode_field_access(&mut self, obj: &SpExpr, field: &str) -> Z3Value {
        // Canonical length for simple identifiers (#267).
        if crate::encode_atom_policy::is_length_method_name(field)
            && let Expr::Ident(name) = &obj.node
        {
            return Z3Value::Int(self.canonical_length(name));
        }

        // Native string theory: .length() on a Str value uses Z3's str.len
        if self.use_string_theory && crate::encode_atom_policy::is_length_method_name(field) {
            let obj_val = self.encode_expr(obj);
            if let Z3Value::Str(s) = &obj_val {
                let len = s.length();
                let zero = ast::Int::from_i64(0);
                self.background_axioms.push(len.ge(&zero));
                return Z3Value::Int(len);
            }
            // Not a Str value; fall through to default encoding
        }

        // #198: flatten vs shallow UF via encode_field_policy (parity with CVC5).
        match crate::encode_field_policy::plan_field_access(obj, field) {
            crate::encode_field_policy::FieldAccessPlan::Flatten(flat_name) => {
                // Boolean-valued fields at any depth (table in encode_method_policy).
                if crate::encode_method_policy::is_bool_field_name(field) {
                    let v = ast::Bool::new_const(flat_name.as_str());
                    return Z3Value::Bool(v);
                }
                // Size fields at any depth get non-negativity axiom
                if crate::encode_atom_policy::is_size_field_name(field) {
                    let v = self.get_or_create_int(&flat_name);
                    let zero = ast::Int::from_i64(0);
                    self.background_axioms.push(v.ge(&zero));
                    return Z3Value::Int(v);
                }
                // General field: create as Int variable (Nat fields get >= 0)
                let v = self.get_or_create_int(&flat_name);
                return Z3Value::Int(v);
            }
            crate::encode_field_policy::FieldAccessPlan::ShallowUf { .. } => {}
        }

        let obj_val = self.encode_expr(obj).as_int(&mut self.fresh_counter);
        let func_name = crate::encode_field_policy::field_uf_smtlib_name(field);
        // Boolean-valued fields (table in encode_method_policy).
        if crate::encode_method_policy::is_bool_field_name(field) {
            let bool_sort = z3::Sort::bool();
            let int_sort = z3::Sort::int();
            let decl = z3::FuncDecl::new(func_name.as_str(), &[&int_sort], &bool_sort);
            let result = decl.apply(&[&obj_val as &dyn z3::ast::Ast]);
            return Z3Value::Bool(result.as_bool().unwrap_or_else(|| self.fresh_bool()));
        }
        // Size fields: return Int with non-negativity axiom
        if crate::encode_atom_policy::is_size_field_name(field) {
            let decl = self.make_func(&func_name, 1);
            let result = decl.apply(&[&obj_val as &dyn z3::ast::Ast]);
            let len_val = result.as_int().unwrap_or_else(|| self.fresh_int());
            // Assert len >= 0 as a background axiom
            let zero = ast::Int::from_i64(0);
            self.background_axioms.push(len_val.ge(&zero));
            return Z3Value::Int(len_val);
        }
        let decl = self.make_func(&func_name, 1);
        let result = decl.apply(&[&obj_val as &dyn z3::ast::Ast]);
        Z3Value::Int(result.as_int().unwrap_or_else(|| self.fresh_int()))
    }

    /// Encode indexing as uninterpreted function: __index(collection, index).
    ///
    /// Plan via [`crate::encode_index_policy::plan_index_access`] (bounds on,
    /// parity with CVC5 native). Z3-only: array `select` linked to `__index`.
    pub(crate) fn encode_index(&mut self, collection: &SpExpr, index: &SpExpr) -> Z3Value {
        use crate::encode_index_policy::{
            IndexAccessPlan, index_bounds_len_uf_name, index_uf_name, plan_index_access,
        };

        let plan = plan_index_access(true);
        debug_assert!(matches!(
            plan,
            IndexAccessPlan::UfWithOptionalBounds {
                emit_bounds_axioms: true
            }
        ));

        let coll_val = self.encode_expr(collection).as_int(&mut self.fresh_counter);
        let idx_val = self.encode_expr(index).as_int(&mut self.fresh_counter);

        let zero = ast::Int::from_i64(0);
        if matches!(
            plan,
            IndexAccessPlan::UfWithOptionalBounds {
                emit_bounds_axioms: true
            }
        ) {
            // Bounds: 0 <= index < __len(collection), __len >= 0
            let ge_zero = idx_val.ge(&zero);
            let len_decl = self.make_func(index_bounds_len_uf_name(), 1);
            let len_result = len_decl.apply(&[&coll_val as &dyn z3::ast::Ast]);
            let len_val = len_result.as_int().unwrap_or_else(|| self.fresh_int());
            self.background_axioms.push(len_val.ge(&zero));
            self.background_axioms.push(ge_zero);
            self.background_axioms.push(idx_val.lt(&len_val));
        }

        // Z3 Array theory: select(array, index) linked to __index UF (backend-local).
        let int_sort = z3::Sort::int();
        let _arr_sort = z3::Sort::array(&int_sort, &int_sort);
        let arr_name = crate::encode_atom_policy::arr_fresh_name(self.fresh_counter);
        self.fresh_counter += 1;
        let arr = z3::ast::Array::new_const(arr_name.as_str(), &int_sort, &int_sort);
        let selected = arr.select(&idx_val);
        let result = selected.as_int().unwrap_or_else(|| self.fresh_int());

        let decl = self.make_func(index_uf_name(), 2);
        let uif_result = decl.apply(&[
            &coll_val as &dyn z3::ast::Ast,
            &idx_val as &dyn z3::ast::Ast,
        ]);
        let uif_val = uif_result.as_int().unwrap_or_else(|| self.fresh_int());
        self.background_axioms.push(result.eq(&uif_val));

        Z3Value::Int(result)
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

    /// Bind pattern variables as fresh Z3 integer constants so they
    /// are available in the arm body.
    pub(crate) fn bind_pattern_vars(
        &mut self,
        pattern: &assura_ast::Pattern,
        scrutinee: &Z3Value,
        match_adt: Option<&str>,
    ) {
        match pattern {
            assura_ast::Pattern::Ident(name) => {
                // Ident patterns in match bind the variable to the scrutinee,
                // but for SMT we use a fresh variable since we cannot always
                // decompose the scrutinee.
                if !self.vars.contains_key(name) {
                    let v = ast::Int::new_const(name.as_str());
                    self.vars.insert(name.clone(), Z3Value::Int(v));
                }
            }
            assura_ast::Pattern::Constructor { name, fields } => {
                if let (Some(adt_name), Z3Value::Int(s)) = (match_adt, scrutinee) {
                    let accessors: Vec<String> = self
                        .adt_defs
                        .get(adt_name)
                        .and_then(|def| {
                            def.constructors
                                .iter()
                                .find(|c| c.name == *name)
                                .map(|c| c.accessors.clone())
                        })
                        .unwrap_or_default();
                    for (i, field) in fields.iter().enumerate() {
                        if let assura_ast::Pattern::Ident(bind_name) = field {
                            let accessor = accessors.get(i).map(String::as_str).unwrap_or("value");
                            let val = self.adt_accessor(adt_name, accessor, s);
                            self.vars.insert(bind_name.clone(), Z3Value::Int(val));
                        } else {
                            self.bind_pattern_vars(field, scrutinee, match_adt);
                        }
                    }
                } else {
                    for field in fields {
                        self.bind_pattern_vars(field, scrutinee, match_adt);
                    }
                }
            }
            assura_ast::Pattern::Tuple(pats) => {
                for pat in pats {
                    self.bind_pattern_vars(pat, scrutinee, match_adt);
                }
            }
            assura_ast::Pattern::Wildcard | assura_ast::Pattern::Literal(_) => {}
        }
    }

    /// Register a synthetic ADT for constructor patterns in a match expression.
    pub(crate) fn register_match_adt_from_arms(
        &mut self,
        arms: &[assura_ast::MatchArm],
    ) -> Option<String> {
        let mut ctor_specs: Vec<(String, Vec<String>)> = Vec::new();
        for arm in arms {
            if let assura_ast::Pattern::Constructor { name, fields } = &arm.pattern {
                let accessors: Vec<String> = fields
                    .iter()
                    .enumerate()
                    .map(|(i, field)| match field {
                        assura_ast::Pattern::Ident(n) => n.clone(),
                        _ => format!("f{i}"),
                    })
                    .collect();
                ctor_specs.push((name.clone(), accessors));
            }
        }
        if ctor_specs.is_empty() {
            return None;
        }
        let adt_name = crate::encode_adt_policy::match_adt_fresh_name(self.fresh_counter);
        self.fresh_counter += 1;
        let accessor_refs: Vec<Vec<&str>> = ctor_specs
            .iter()
            .map(|(_, accessors)| accessors.iter().map(|s| s.as_str()).collect())
            .collect();
        let spec: Vec<(&str, &[&str])> = ctor_specs
            .iter()
            .zip(accessor_refs.iter())
            .map(|((name, _), refs)| (name.as_str(), refs.as_slice()))
            .collect();
        self.define_adt(&adt_name, &spec);
        Some(adt_name)
    }

    // -------------------------------------------------------------------
    // ADT (algebraic data type) emulation
    // -------------------------------------------------------------------

    /// Define an ADT with named constructors and their accessor fields.
    ///
    /// Each constructor gets a unique integer tag (sequential, starting at 0).
    /// The method registers:
    /// 1. A tag function `__adt_tag_<adt_name>` (uninterpreted, Int -> Int)
    /// 2. Accessor functions `__adt_<adt_name>_<field>` (uninterpreted, Int -> Int)
    /// 3. Exhaustiveness axiom: for any value x, tag(x) is one of the defined tags
    /// 4. Injectivity axioms: Ctor(a1, ..., an) == Ctor(b1, ..., bn) => ai == bi
    ///
    /// Returns the registered `AdtDef`.
    pub(crate) fn define_adt(
        &mut self,
        adt_name: &str,
        constructors: &[(&str, &[&str])],
    ) -> AdtDef {
        let mut adt_ctors = Vec::new();
        for (tag, (ctor_name, accessors)) in constructors.iter().enumerate() {
            adt_ctors.push(AdtConstructor {
                name: ctor_name.to_string(),
                tag: tag as i64,
                accessors: accessors.iter().map(|a| a.to_string()).collect(),
            });
        }
        let adt_def = AdtDef {
            name: adt_name.to_string(),
            constructors: adt_ctors,
        };

        // Register uninterpreted functions for the tag and accessors
        let tag_fn_name = crate::encode_atom_policy::adt_tag_uf_name(adt_name);
        self.make_func(&tag_fn_name, 1);

        for ctor in &adt_def.constructors {
            for accessor in &ctor.accessors {
                let acc_fn_name =
                    crate::encode_atom_policy::adt_accessor_uf_name(adt_name, accessor);
                self.make_func(&acc_fn_name, 1);
            }
        }

        // Generate exhaustiveness axiom:
        //   forall x: tag(x) == 0 || tag(x) == 1 || ... || tag(x) == n
        let x =
            ast::Int::new_const(crate::encode_atom_policy::adt_exhaust_var_name(adt_name).as_str());
        let tag_fn = self.make_func(&tag_fn_name, 1);
        let tag_x = tag_fn
            .apply(&[&x as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());

        let tag_eqs: Vec<ast::Bool> = adt_def
            .constructors
            .iter()
            .map(|c| tag_x.eq(ast::Int::from_i64(c.tag)))
            .collect();
        let tag_eq_refs: Vec<&ast::Bool> = tag_eqs.iter().collect();
        let exhaustive = ast::Bool::or(&tag_eq_refs);
        let forall_exhaustive = ast::forall_const(&[&x as &dyn z3::ast::Ast], &[], &exhaustive);
        self.background_axioms.push(forall_exhaustive);

        // Generate injectivity axioms for each constructor with fields:
        //   forall a1..an, b1..bn:
        //     (tag(x) == TAG && acc_i(x) == ai) &&
        //     (tag(y) == TAG && acc_i(y) == bi) &&
        //     x == y
        //     => a1 == b1 && ... && an == bn
        //
        // Simplified form: for each constructor with accessors,
        //   forall x, y: x == y => acc_i(x) == acc_i(y)
        //
        // This is trivially true for UFs, so instead we encode the
        // more useful injectivity:
        //   forall x, y: (tag(x) == tag(y) == TAG &&
        //     acc_1(x) == acc_1(y) && ... && acc_n(x) == acc_n(y))
        //     => x == y
        for ctor in &adt_def.constructors {
            if ctor.accessors.is_empty() {
                // Nullary constructor: any two values with this tag are equal
                let a = ast::Int::new_const(
                    crate::encode_atom_policy::adt_inject_var_name(adt_name, &ctor.name, 'a')
                        .as_str(),
                );
                let b = ast::Int::new_const(
                    crate::encode_atom_policy::adt_inject_var_name(adt_name, &ctor.name, 'b')
                        .as_str(),
                );

                let tag_a = tag_fn
                    .apply(&[&a as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                let tag_b = tag_fn
                    .apply(&[&b as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                let tag_val = ast::Int::from_i64(ctor.tag);
                let both_tagged = ast::Bool::and(&[&tag_a.eq(&tag_val), &tag_b.eq(&tag_val)]);
                let eq_ab = a.eq(&b);
                let axiom = ast::forall_const(
                    &[&a as &dyn z3::ast::Ast, &b as &dyn z3::ast::Ast],
                    &[],
                    &both_tagged.implies(&eq_ab),
                );
                self.background_axioms.push(axiom);
            } else {
                // Constructor with fields: matching all accessors implies equality
                let a = ast::Int::new_const(
                    crate::encode_atom_policy::adt_inject_var_name(adt_name, &ctor.name, 'a')
                        .as_str(),
                );
                let b = ast::Int::new_const(
                    crate::encode_atom_policy::adt_inject_var_name(adt_name, &ctor.name, 'b')
                        .as_str(),
                );

                let tag_a = tag_fn
                    .apply(&[&a as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                let tag_b = tag_fn
                    .apply(&[&b as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                let tag_val = ast::Int::from_i64(ctor.tag);

                let mut conjuncts = vec![tag_a.eq(&tag_val), tag_b.eq(&tag_val)];
                for accessor in &ctor.accessors {
                    let acc_fn_name =
                        crate::encode_atom_policy::adt_accessor_uf_name(adt_name, accessor);
                    let acc_fn = self.make_func(&acc_fn_name, 1);
                    let acc_a = acc_fn
                        .apply(&[&a as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let acc_b = acc_fn
                        .apply(&[&b as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    conjuncts.push(acc_a.eq(&acc_b));
                }
                let conjunct_refs: Vec<&ast::Bool> = conjuncts.iter().collect();
                let premise = ast::Bool::and(&conjunct_refs);
                let eq_ab = a.eq(&b);
                let axiom = ast::forall_const(
                    &[&a as &dyn z3::ast::Ast, &b as &dyn z3::ast::Ast],
                    &[],
                    &premise.implies(&eq_ab),
                );
                self.background_axioms.push(axiom);
            }
        }

        self.adt_defs.insert(adt_def.name.clone(), adt_def.clone());
        adt_def
    }

    /// Build a constructor application: create a fresh Int value, set its
    /// tag to the constructor's tag, and bind accessor values to the
    /// provided arguments.
    ///
    /// Tag resolution uses [`crate::encode_adt_policy::adt_ctor_tag_or_zero`]
    /// when the ADT registry is present (parity with CVC5 shell); unknown ctor
    /// still defaults to tag `0`. Accessor UF names come from
    /// [`crate::encode_atom_policy`].
    ///
    /// Returns the fresh Int representing the constructed value.
    pub(crate) fn adt_constructor(
        &mut self,
        adt_name: &str,
        ctor_name: &str,
        args: &[ast::Int],
    ) -> ast::Int {
        let adt_def = self.adt_defs.get(adt_name).cloned();
        let ctor = adt_def
            .as_ref()
            .and_then(|d| d.constructors.iter().find(|c| c.name == ctor_name));

        // Prefer sequential tag from the registered ctor; fall back to
        // name-order policy (unknown ctor → 0) when the registry entry is
        // missing or incomplete (parity with CVC5 adt_is_constructor_smtlib).
        let tag = if let Some(c) = ctor {
            c.tag
        } else if let Some(def) = adt_def.as_ref() {
            let names: Vec<&str> = def.constructors.iter().map(|c| c.name.as_str()).collect();
            crate::encode_adt_policy::adt_ctor_tag_or_zero(&names, ctor_name)
        } else {
            0
        };
        let accessors: Vec<String> = ctor.map_or_else(Vec::new, |c| c.accessors.clone());

        let val = self.fresh_int();

        // Set tag
        let tag_fn_name = crate::encode_atom_policy::adt_tag_uf_name(adt_name);
        let tag_fn = self.make_func(&tag_fn_name, 1);
        let tag_applied = tag_fn
            .apply(&[&val as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());
        self.background_axioms
            .push(tag_applied.eq(ast::Int::from_i64(tag)));

        // Bind accessor values
        for (i, accessor) in accessors.iter().enumerate() {
            if let Some(arg) = args.get(i) {
                let acc_fn_name =
                    crate::encode_atom_policy::adt_accessor_uf_name(adt_name, accessor);
                let acc_fn = self.make_func(&acc_fn_name, 1);
                let acc_applied = acc_fn
                    .apply(&[&val as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                self.background_axioms.push(acc_applied.eq(arg));
            }
        }

        val
    }

    /// Test whether a value was built with a specific constructor.
    ///
    /// Returns `tag(x) == CONSTRUCTOR_TAG` as a Z3 Bool. Tag lookup mirrors
    /// [`crate::encode_adt_policy::adt_ctor_tag_or_zero`] / CVC5 shell when the
    /// ctor is not in the local registry entry.
    pub(crate) fn adt_is_constructor(
        &mut self,
        adt_name: &str,
        ctor_name: &str,
        value: &ast::Int,
    ) -> ast::Bool {
        let adt_def = self.adt_defs.get(adt_name);
        let tag = if let Some(c) =
            adt_def.and_then(|d| d.constructors.iter().find(|c| c.name == ctor_name))
        {
            c.tag
        } else if let Some(def) = adt_def {
            let names: Vec<&str> = def.constructors.iter().map(|c| c.name.as_str()).collect();
            crate::encode_adt_policy::adt_ctor_tag_or_zero(&names, ctor_name)
        } else {
            0
        };

        let tag_fn_name = crate::encode_atom_policy::adt_tag_uf_name(adt_name);
        let tag_fn = self.make_func(&tag_fn_name, 1);
        let tag_val = tag_fn
            .apply(&[value as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());
        tag_val.eq(ast::Int::from_i64(tag))
    }

    /// Access a field of a constructed ADT value.
    ///
    /// Returns `accessor(x)` as a Z3 Int. UF name via
    /// [`crate::encode_atom_policy::adt_accessor_uf_name`] (parity with
    /// [`crate::encode_adt_policy::adt_accessor_smtlib`]).
    pub(crate) fn adt_accessor(
        &mut self,
        adt_name: &str,
        accessor: &str,
        value: &ast::Int,
    ) -> ast::Int {
        let acc_fn_name = crate::encode_atom_policy::adt_accessor_uf_name(adt_name, accessor);
        let acc_fn = self.make_func(&acc_fn_name, 1);
        acc_fn
            .apply(&[value as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int())
    }
}
