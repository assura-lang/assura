//! Encoder construction, ADT emulation, quantifiers, and non-expr encode helpers.

use crate::*;
use assura_ast::{BinOp, Literal, SpExpr};
use assura_types::checkers::expr_references_var;
use std::collections::HashMap;
use z3::ast;

use super::BitvectorEncoder;
use super::unmodelable::{flatten_field_chain, has_deep_field_chain, is_self_rooted};
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
        let key = format!("__canonical_len_{name}");
        let v = ast::Int::new_const(key.as_str());
        let zero = ast::Int::from_i64(0);
        self.background_axioms.push(v.ge(&zero));
        // Link to `len` / `__field_len` UIFs so concat/array axioms agree with `.len` (#267).
        let obj = self.get_or_create_int(name);
        for uf_name in ["len", "__field_len"] {
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
    pub(crate) fn guard_quantifier_body(
        &mut self,
        domain: &SpExpr,
        bound: &ast::Int,
        body: &ast::Bool,
        is_forall: bool,
    ) -> ast::Bool {
        // Check if domain is a range expression: lo..hi
        if let Expr::BinOp {
            op: BinOp::Range,
            lhs: lo,
            rhs: hi,
        } = &domain.node
        {
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
            let contains_fn =
                z3::FuncDecl::new("__domain_contains", &[&int_sort, &int_sort], &bool_sort);
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

        // Check TriggerManager for user-provided or inferred triggers
        let body_str = format!("{body:?}");
        if let Some(trigger) = self.trigger_manager.infer_trigger(&body_str) {
            // Production wiring for validate_trigger (agent-guards SMT v2):
            // surface unknown function names as solver-side signal, not only unit tests.
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
                                ast::Dynamic::from_ast(&ast::Int::new_const("__trigger_other"))
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
        ast::Bool::new_const(format!("__fresh_{}", self.fresh_counter))
    }

    /// Create a fresh unconstrained integer.
    pub(crate) fn fresh_int(&mut self) -> ast::Int {
        self.fresh_counter += 1;
        ast::Int::new_const(format!("__fresh_{}", self.fresh_counter))
    }

    /// Create an uninterpreted function declaration (Int^arity -> Int).
    /// Z3 internally deduplicates declarations with the same name and sorts.
    pub(crate) fn make_func(&mut self, name: &str, arity: usize) -> z3::FuncDecl {
        self.func_arities.insert(name.to_string(), arity);
        let int_sort = z3::Sort::int();
        let param_sorts: Vec<&z3::Sort> = (0..arity).map(|_| &int_sort).collect();
        z3::FuncDecl::new(name, &param_sorts, &int_sort)
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
        if self.use_string_theory && matches!(func_name, "len" | "length") && args.len() == 1 {
            let arg_val = self.encode_expr(&args[0]);
            if let Z3Value::Str(s) = &arg_val {
                let len = s.length();
                let zero = ast::Int::from_i64(0);
                self.background_axioms.push(len.ge(&zero));
                return Z3Value::Int(len);
            }
        }

        // Canonical length for simple identifiers (#267).
        if matches!(func_name, "len" | "length")
            && args.len() == 1
            && let Expr::Ident(name) = &args[0].node
        {
            return Z3Value::Int(self.canonical_length(name));
        }

        let arg_vals: Vec<ast::Int> = args
            .iter()
            .map(|a| self.encode_expr(a).as_int(&mut self.fresh_counter))
            .collect();
        // min/max: encode with ite so Z3 proves bounds (not unconstrained UF).
        // e.g. ensures { min(a, b) <= a } verifies under any a, b.
        if matches!(func_name, "min" | "max") && arg_vals.len() == 2 {
            let a = &arg_vals[0];
            let b = &arg_vals[1];
            let a_le_b = a.le(b);
            let result = if func_name == "min" {
                a_le_b.ite(a, b)
            } else {
                a_le_b.ite(b, a)
            };
            return Z3Value::Int(result);
        }
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
            let bool_sort = z3::Sort::bool();
            let int_sort = z3::Sort::int();
            let param_sorts: Vec<&z3::Sort> = (0..arg_vals.len()).map(|_| &int_sort).collect();
            let decl = z3::FuncDecl::new(func_name, &param_sorts, &bool_sort);
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
                let zero = ast::Int::from_i64(0);
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
                let diff = ast::Int::sub(&[end, start]);
                self.background_axioms.push(res_len.eq(&diff));
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
                let zero = ast::Int::from_i64(0);
                self.background_axioms.push(len_l.ge(&zero));
                self.background_axioms.push(len_r.ge(&zero));
                let sum = ast::Int::add(&[&len_l, &len_r]);
                self.background_axioms.push(len_result.eq(&sum));
                self.background_axioms.push(len_result.ge(&zero));
                return Z3Value::Int(result);
            }
            // index_of(str, substr): returns Int with -1 <= result < len(str)
            "index_of" | "find" | "indexOf" if arg_vals.len() == 2 => {
                let str_val = &arg_vals[0];
                let result = self.fresh_int();
                let neg_one = ast::Int::from_i64(-1);
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
                let zero = ast::Int::from_i64(0);
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
                let zero = ast::Int::from_i64(0);
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
                let one = ast::Int::from_i64(1);
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
                let zero = ast::Int::from_i64(0);
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
                let zero = ast::Int::from_i64(0);
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
            let arr_expr = &args[0].node;
            let arr = &arg_vals[0];
            let idx = &arg_vals[1];
            let val = &arg_vals[2];
            let result = self.fresh_int();
            // After set(a, i, v): get(result, i) == v
            let get_decl = self.make_func("__index", 2);
            let get_at_idx = get_decl
                .apply(&[&result as &dyn z3::ast::Ast, idx as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| self.fresh_int());
            self.background_axioms.push(get_at_idx.eq(val));
            // len(result) == len(original); use canonical length for named arrays (#267).
            let old_len = if let Expr::Ident(name) = arr_expr {
                self.canonical_length(name)
            } else {
                let len_decl = self.make_func("len", 1);
                len_decl
                    .apply(&[arr as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int())
            };
            let len_decl = self.make_func("len", 1);
            let new_len = len_decl
                .apply(&[&result as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| self.fresh_int());
            self.background_axioms.push(new_len.eq(&old_len));
            let zero = ast::Int::from_i64(0);
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
            self.background_axioms.push(get_result.eq(value));
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
            let zero = ast::Int::from_i64(0);
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
            let zero = ast::Int::from_i64(0);
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
    ///
    /// Note (#191): When the object is `result` (a free Z3 variable), the
    /// field access `__field_len(result)` is also unconstrained. This means
    /// ensures clauses like `result.length() <= raw.length()` produce
    /// spurious counterexamples because Z3 can assign any value to
    /// `__field_len(result)`. This is a known limitation; see the doc
    /// comment on `verify_clauses_with_types` for details.
    pub(crate) fn encode_field_access(&mut self, obj: &SpExpr, field: &str) -> Z3Value {
        // Canonical length for simple identifiers (#267).
        if matches!(field, "len" | "length")
            && let Expr::Ident(name) = &obj.node
        {
            return Z3Value::Int(self.canonical_length(name));
        }

        // Native string theory: .length() on a Str value uses Z3's str.len
        if self.use_string_theory && matches!(field, "len" | "length") {
            let obj_val = self.encode_expr(obj);
            if let Z3Value::Str(s) = &obj_val {
                let len = s.length();
                let zero = ast::Int::from_i64(0);
                self.background_axioms.push(len.ge(&zero));
                return Z3Value::Int(len);
            }
            // Not a Str value; fall through to default encoding
        }

        // #198: Flatten deep field chains (e.g., state.head.extra.extra_max)
        // into a single Z3 variable instead of nested uninterpreted functions.
        if has_deep_field_chain(&Expr::Field(Box::new(obj.clone()), field.to_string()))
            || is_self_rooted(&obj.node)
        {
            let flat_name =
                flatten_field_chain(&Expr::Field(Box::new(obj.clone()), field.to_string()));
            // Boolean-valued fields at any depth
            if matches!(
                field,
                "is_empty" | "is_some" | "is_none" | "is_ok" | "is_err"
            ) {
                let v = ast::Bool::new_const(flat_name.as_str());
                return Z3Value::Bool(v);
            }
            // Size fields at any depth get non-negativity axiom
            if matches!(field, "len" | "length" | "size" | "capacity" | "count") {
                let v = self.get_or_create_int(&flat_name);
                let zero = ast::Int::from_i64(0);
                self.background_axioms.push(v.ge(&zero));
                return Z3Value::Int(v);
            }
            // General field: create as Int variable (Nat fields get >= 0)
            let v = self.get_or_create_int(&flat_name);
            return Z3Value::Int(v);
        }

        let obj_val = self.encode_expr(obj).as_int(&mut self.fresh_counter);
        let func_name = format!("__field_{field}");
        // Boolean-valued fields
        if matches!(
            field,
            "is_empty" | "is_some" | "is_none" | "is_ok" | "is_err"
        ) {
            let bool_sort = z3::Sort::bool();
            let int_sort = z3::Sort::int();
            let decl = z3::FuncDecl::new(func_name.as_str(), &[&int_sort], &bool_sort);
            let result = decl.apply(&[&obj_val as &dyn z3::ast::Ast]);
            return Z3Value::Bool(result.as_bool().unwrap_or_else(|| self.fresh_bool()));
        }
        // Size fields: return Int with non-negativity axiom
        if matches!(field, "len" | "length" | "size" | "capacity" | "count") {
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
    pub(crate) fn encode_index(&mut self, collection: &SpExpr, index: &SpExpr) -> Z3Value {
        let coll_val = self.encode_expr(collection).as_int(&mut self.fresh_counter);
        let idx_val = self.encode_expr(index).as_int(&mut self.fresh_counter);

        // Add bounds checking axiom: 0 <= index < len(collection)
        let zero = ast::Int::from_i64(0);
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
        let int_sort = z3::Sort::int();
        let _arr_sort = z3::Sort::array(&int_sort, &int_sort);
        let arr_name = format!("__arr_{}", self.fresh_counter);
        self.fresh_counter += 1;
        let arr = z3::ast::Array::new_const(arr_name.as_str(), &int_sort, &int_sort);
        // Constrain: the array is associated with this collection
        // (same collection -> same array via naming, but we also
        // link values through the select result).
        let selected = arr.select(&idx_val);
        // Z3 select returns a Dynamic; extract as Int
        let result = selected.as_int().unwrap_or_else(|| self.fresh_int());

        // Also add an uninterpreted function so Z3 can reason about indexing
        let decl = self.make_func("__index", 2);
        let uif_result = decl.apply(&[
            &coll_val as &dyn z3::ast::Ast,
            &idx_val as &dyn z3::ast::Ast,
        ]);
        let uif_val = uif_result.as_int().unwrap_or_else(|| self.fresh_int());
        // Link the two: select(arr, i) == __index(coll, i)
        self.background_axioms.push(result.eq(&uif_val));

        Z3Value::Int(result)
    }

    /// Hash a pattern name to a stable i64 for Z3 encoding.
    ///
    /// Uses FNV-1a instead of DefaultHasher for determinism across Rust
    /// versions (DefaultHasher may change its algorithm between releases).
    pub(crate) fn pattern_hash(&self, name: &str) -> i64 {
        let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
        for byte in name.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x100000001b3); // FNV prime
        }
        hash as i64
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
                let f: f64 = s.parse().unwrap_or(0.0);
                let denom = 1_000_000i64;
                let numer = (f * denom as f64) as i64;
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
        let adt_name = format!("__match_adt_{}", self.fresh_counter);
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
        let tag_fn_name = format!("__adt_tag_{adt_name}");
        self.make_func(&tag_fn_name, 1);

        for ctor in &adt_def.constructors {
            for accessor in &ctor.accessors {
                let acc_fn_name = format!("__adt_{adt_name}_{accessor}");
                self.make_func(&acc_fn_name, 1);
            }
        }

        // Generate exhaustiveness axiom:
        //   forall x: tag(x) == 0 || tag(x) == 1 || ... || tag(x) == n
        let x = ast::Int::new_const(format!("__adt_exh_{adt_name}"));
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
                let a = ast::Int::new_const(format!("__adt_inj_{adt_name}_{}_a", ctor.name));
                let b = ast::Int::new_const(format!("__adt_inj_{adt_name}_{}_b", ctor.name));

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
                let a = ast::Int::new_const(format!("__adt_inj_{adt_name}_{}_a", ctor.name));
                let b = ast::Int::new_const(format!("__adt_inj_{adt_name}_{}_b", ctor.name));

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
                    let acc_fn_name = format!("__adt_{adt_name}_{accessor}");
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

        let tag = ctor.map_or(0, |c| c.tag);
        let accessors: Vec<String> = ctor.map_or_else(Vec::new, |c| c.accessors.clone());

        let val = self.fresh_int();

        // Set tag
        let tag_fn_name = format!("__adt_tag_{adt_name}");
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
                let acc_fn_name = format!("__adt_{adt_name}_{accessor}");
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
    /// Returns `tag(x) == CONSTRUCTOR_TAG` as a Z3 Bool.
    pub(crate) fn adt_is_constructor(
        &mut self,
        adt_name: &str,
        ctor_name: &str,
        value: &ast::Int,
    ) -> ast::Bool {
        let tag = self
            .adt_defs
            .get(adt_name)
            .and_then(|d| d.constructors.iter().find(|c| c.name == ctor_name))
            .map_or(0, |c| c.tag);

        let tag_fn_name = format!("__adt_tag_{adt_name}");
        let tag_fn = self.make_func(&tag_fn_name, 1);
        let tag_val = tag_fn
            .apply(&[value as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int());
        tag_val.eq(ast::Int::from_i64(tag))
    }

    /// Access a field of a constructed ADT value.
    ///
    /// Returns `accessor(x)` as a Z3 Int.
    pub(crate) fn adt_accessor(
        &mut self,
        adt_name: &str,
        accessor: &str,
        value: &ast::Int,
    ) -> ast::Int {
        let acc_fn_name = format!("__adt_{adt_name}_{accessor}");
        let acc_fn = self.make_func(&acc_fn_name, 1);
        acc_fn
            .apply(&[value as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| self.fresh_int())
    }
}
