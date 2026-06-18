//! Z3 expression encoder: translates Assura AST expressions into Z3 formulas.
//!
//! Contains the `Encoder` struct, Z3 value wrapper types, raw-token parsing,
//! and unmodelable-feature detection.

use crate::*;
use assura_parser::ast::{BinOp, Literal, UnaryOp};
use assura_types::checkers::expr_references_var;
use std::collections::HashMap;
use z3::ast;

// -----------------------------------------------------------------------
// Z3 value wrapper
// -----------------------------------------------------------------------

/// A Z3 expression that can be either an integer or boolean sort.
#[derive(Clone)]
pub(crate) enum Z3Value {
    Bool(ast::Bool),
    Int(ast::Int),
    Real(ast::Real),
    /// Native Z3 string value (only used when `use_string_theory` is enabled).
    Str(ast::String),
}

/// Binary operator kind for raw token parsing.
#[derive(Debug, Clone, Copy)]
pub(super) enum RawOp {
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

impl Z3Value {
    /// Extract as Bool. If Int, create `!= 0` comparison.
    pub(crate) fn as_bool(&self) -> ast::Bool {
        match self {
            Z3Value::Bool(b) => b.clone(),
            Z3Value::Int(i) => i.eq(ast::Int::from_i64(0)).not(),
            Z3Value::Real(r) => r.eq(ast::Real::from_rational(0, 1)).not(),
            // Str: non-empty string is truthy
            Z3Value::Str(s) => s.length().eq(ast::Int::from_i64(0)).not(),
        }
    }

    /// Extract as Int. If Bool, use `ite(b, 1, 0)` for sound coercion.
    /// If Real, truncate via `real2int`. If Str, use `str.len`.
    pub(super) fn as_int(&self, _counter: &mut u32) -> ast::Int {
        match self {
            Z3Value::Int(i) => i.clone(),
            Z3Value::Bool(b) => {
                // Sound coercion: true -> 1, false -> 0
                let one = ast::Int::from_i64(1);
                let zero = ast::Int::from_i64(0);
                b.ite(&one, &zero)
            }
            Z3Value::Real(r) => ast::Real::to_int(r),
            // Str: coerce to length for integer context
            Z3Value::Str(s) => s.length(),
        }
    }

    /// Extract as Real. If Int, convert via `int2real`. If Bool, use
    /// `ite(b, 1.0, 0.0)` for sound coercion.
    pub(super) fn as_real(&self, _counter: &mut u32) -> ast::Real {
        match self {
            Z3Value::Real(r) => r.clone(),
            Z3Value::Int(i) => ast::Real::from_int(i),
            Z3Value::Bool(b) => {
                let one = ast::Real::from_rational(1, 1);
                let zero = ast::Real::from_rational(0, 1);
                b.ite(&one, &zero)
            }
            // Str: coerce via length
            Z3Value::Str(s) => ast::Real::from_int(&s.length()),
        }
    }
}

// -----------------------------------------------------------------------
// Expression encoder
// -----------------------------------------------------------------------

/// A single constructor in an ADT emulation.
///
/// Each constructor has a unique integer tag, a name, and a list of named
/// accessor fields. The encoder uses uninterpreted functions for accessors
/// and integer equality for constructor testing.
#[derive(Debug, Clone)]
pub(crate) struct AdtConstructor {
    /// Display name of the constructor (e.g., "Some", "None").
    pub(crate) name: String,
    /// Unique integer tag assigned to this constructor.
    pub(crate) tag: i64,
    /// Named accessor fields. Each accessor is encoded as an uninterpreted
    /// function `Int -> Int` applied to the ADT value.
    pub(crate) accessors: Vec<String>,
}

/// An ADT (algebraic data type) definition emulated via integer tags and
/// uninterpreted functions.
///
/// Since the encoder is currently untyped (all values are Int or Bool),
/// ADTs are emulated as follows:
/// - Each constructor gets a unique integer tag.
/// - A tag function (`__adt_tag_<name>`) returns the constructor tag.
/// - Each accessor is an uninterpreted function `__adt_<adt>_<accessor>(x) -> Int`.
/// - Constructor tester: `tag(x) == CONSTRUCTOR_TAG`.
/// - Injectivity axiom: `Ctor(a, b) == Ctor(c, d) => a == c && b == d`.
/// - Exhaustiveness: `tag(x) == Tag1 || tag(x) == Tag2 || ...`.
#[derive(Debug, Clone)]
pub(crate) struct AdtDef {
    /// ADT type name (e.g., "Option", "List").
    pub(crate) name: String,
    /// Constructors in definition order.
    pub(crate) constructors: Vec<AdtConstructor>,
}

/// Translates Assura AST expressions into Z3 formulas.
pub(crate) struct Encoder {
    pub(crate) vars: HashMap<String, Z3Value>,
    /// Tracks known function arities for uninterpreted function encoding
    pub(crate) func_arities: HashMap<String, usize>,
    pub(crate) fresh_counter: u32,
    /// Background axioms collected during encoding (e.g., len >= 0).
    /// These are asserted into the solver before each verification check.
    pub(crate) background_axioms: Vec<z3::ast::Bool>,
    /// Trigger manager for quantifier e-matching hints
    pub(crate) trigger_manager: crate::advanced::TriggerManager,
    /// Track distinct string constant names for pairwise distinctness axioms
    pub(crate) string_constants: Vec<String>,
    /// When true, use native Z3 string theory (QF_S/QF_SLIA) for string
    /// literals and .length(). When false (default), use integer encoding.
    pub(crate) use_string_theory: bool,
    /// Registered ADT definitions for lightweight emulation.
    #[cfg_attr(not(test), expect(dead_code))]
    pub(crate) adt_defs: HashMap<String, AdtDef>,
}

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
        }
    }

    /// Create an encoder with native string theory enabled.
    pub(crate) fn with_string_theory(use_string_theory: bool) -> Self {
        Self {
            use_string_theory,
            ..Self::new()
        }
    }

    /// Get or create a named integer variable.
    pub(super) fn get_or_create_int(&mut self, name: &str) -> ast::Int {
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
    fn guard_quantifier_body(
        &mut self,
        domain: &Expr,
        bound: &ast::Int,
        body: &ast::Bool,
        is_forall: bool,
    ) -> ast::Bool {
        // Check if domain is a range expression: lo..hi
        if let Expr::BinOp {
            op: BinOp::Range,
            lhs: lo,
            rhs: hi,
        } = domain
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
    fn infer_quantifier_patterns(
        &mut self,
        body: &Expr,
        bound_var: &str,
        bound_z3: &ast::Int,
    ) -> Vec<z3::Pattern> {
        let mut patterns = Vec::new();

        // Check TriggerManager for user-provided or inferred triggers
        let body_str = format!("{body:?}");
        if let Some(trigger) = self.trigger_manager.infer_trigger(&body_str) {
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
    fn collect_trigger_calls(
        &self,
        expr: &Expr,
        bound_var: &str,
        bound_z3: &ast::Int,
        patterns: &mut Vec<z3::Pattern>,
    ) {
        match expr {
            Expr::Call { func, args } => {
                let refs_bound = args.iter().any(|a| expr_references_var(a, bound_var));
                if refs_bound && let Expr::Ident(fname) = func.as_ref() {
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
            Expr::UnaryOp { expr: e, .. } | Expr::Paren(e) | Expr::Old(e) | Expr::Ghost(e) => {
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
    fn fresh_bool(&mut self) -> ast::Bool {
        self.fresh_counter += 1;
        ast::Bool::new_const(format!("__fresh_{}", self.fresh_counter))
    }

    /// Create a fresh unconstrained integer.
    pub(super) fn fresh_int(&mut self) -> ast::Int {
        self.fresh_counter += 1;
        ast::Int::new_const(format!("__fresh_{}", self.fresh_counter))
    }

    /// Create an uninterpreted function declaration (Int^arity -> Int).
    /// Z3 internally deduplicates declarations with the same name and sorts.
    fn make_func(&mut self, name: &str, arity: usize) -> z3::FuncDecl {
        self.func_arities.insert(name.to_string(), arity);
        let int_sort = z3::Sort::int();
        let param_sorts: Vec<&z3::Sort> = (0..arity).map(|_| &int_sort).collect();
        z3::FuncDecl::new(name, &param_sorts, &int_sort)
    }

    /// Encode a function call as an uninterpreted function application.
    /// Known boolean methods return Bool; everything else returns Int.
    fn encode_call(&mut self, func_name: &str, args: &[Expr]) -> Z3Value {
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

        let arg_vals: Vec<ast::Int> = args
            .iter()
            .map(|a| self.encode_expr(a).as_int(&mut self.fresh_counter))
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
            self.background_axioms.push(get_at_idx.eq(val));
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
    fn encode_field_access(&mut self, obj: &Expr, field: &str) -> Z3Value {
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
            || is_self_rooted(obj)
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
    fn encode_index(&mut self, collection: &Expr, index: &Expr) -> Z3Value {
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
    fn pattern_hash(&self, name: &str) -> i64 {
        let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
        for byte in name.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x100000001b3); // FNV prime
        }
        hash as i64
    }

    /// Encode a literal value to Z3.
    fn encode_literal(&self, lit: &Literal) -> Z3Value {
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
    fn bind_pattern_vars(&mut self, pattern: &assura_parser::ast::Pattern, _scrutinee: &Z3Value) {
        match pattern {
            assura_parser::ast::Pattern::Ident(name) => {
                // Ident patterns in match bind the variable to the scrutinee,
                // but for SMT we use a fresh variable since we cannot always
                // decompose the scrutinee.
                if !self.vars.contains_key(name) {
                    let v = ast::Int::new_const(name.as_str());
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
            assura_parser::ast::Pattern::Wildcard | assura_parser::ast::Pattern::Literal(_) => {}
        }
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

        self.adt_defs.insert(adt_name.to_string(), adt_def.clone());
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

    /// Encode an AST expression into a Z3 value.
    pub(crate) fn encode_expr(&mut self, expr: &Expr) -> Z3Value {
        match expr {
            // --- Literals ---
            Expr::Literal(Literal::Int(s)) => {
                // Use Z3's string-based bignum constructor for large integers
                // that overflow i64, falling back to from_i64 for normal values.
                if let Ok(n) = s.parse::<i64>() {
                    Z3Value::Int(ast::Int::from_i64(n))
                } else {
                    // Large integer: use Z3 string parsing via FromStr.
                    // Strip leading minus, parse as positive, then negate.
                    if let Some(rest) = s.strip_prefix('-') {
                        let abs_val: ast::Int =
                            rest.parse().unwrap_or_else(|_| ast::Int::from_i64(0));
                        Z3Value::Int(abs_val.unary_minus())
                    } else {
                        let val: ast::Int = s.parse().unwrap_or_else(|_| ast::Int::from_i64(0));
                        Z3Value::Int(val)
                    }
                }
            }
            Expr::Literal(Literal::Float(s)) => {
                // Encode as Z3 Real. Parse the float string and convert
                // to a rational (numerator/denominator) for exact encoding.
                // No clamping: use full f64 range.
                let f: f64 = s.parse().unwrap_or(0.0);
                let denom = 1_000_000i64;
                let numer = (f * denom as f64) as i64;
                Z3Value::Real(ast::Real::from_rational(numer, denom))
            }
            Expr::Literal(Literal::Str(s)) => {
                if self.use_string_theory {
                    // Native Z3 string theory: use z3::ast::String directly.
                    // Z3 handles equality, length, and distinctness natively.
                    let str_val = ast::String::from(s.as_str());
                    // Background axiom: length is known at compile time
                    let len = str_val.length();
                    let expected_len = ast::Int::from_i64(s.len() as i64);
                    self.background_axioms.push(len.eq(&expected_len));
                    Z3Value::Str(str_val)
                } else {
                    // Integer encoding (default): named integer constant.
                    // Two identical string literals produce the same constant,
                    // so equality works. Different strings get different constants.
                    let const_name = format!("__str_{s}");
                    let str_val = ast::Int::new_const(const_name.clone());

                    // Track this string constant for pairwise distinctness axioms.
                    if !self.string_constants.contains(&const_name) {
                        for prev in &self.string_constants {
                            let prev_val = ast::Int::new_const(prev.clone());
                            self.background_axioms.push(str_val.eq(&prev_val).not());
                        }
                        self.string_constants.push(const_name);
                    }

                    // String length axiom: len("hello") == 5
                    let len_decl = self.make_func("__field_len", 1);
                    let len_result = len_decl
                        .apply(&[&str_val as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let str_len = ast::Int::from_i64(s.len() as i64);
                    self.background_axioms.push(len_result.eq(&str_len));
                    Z3Value::Int(str_val)
                }
            }
            Expr::Literal(Literal::Bool(b)) => Z3Value::Bool(ast::Bool::from_bool(*b)),

            // --- Identifiers ---
            Expr::Ident(name) => {
                if name == "true" {
                    return Z3Value::Bool(ast::Bool::from_bool(true));
                }
                if name == "false" {
                    return Z3Value::Bool(ast::Bool::from_bool(false));
                }
                if let Some(val) = self.vars.get(name) {
                    return val.clone();
                }
                // Default: create integer variable (most common in contracts)
                let v = ast::Int::new_const(name.as_str());
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
                            let r = val.as_real(&mut self.fresh_counter);
                            Z3Value::Real(r.unary_minus())
                        } else {
                            let i = val.as_int(&mut self.fresh_counter);
                            Z3Value::Int(i.unary_minus())
                        }
                    }
                    UnaryOp::Not => {
                        let b = val.as_bool();
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
                    let old_obj_int = old_obj.as_int(&mut self.fresh_counter);
                    let func_name = format!("__field_{field}");
                    if matches!(
                        field.as_str(),
                        "is_empty" | "is_some" | "is_none" | "is_ok" | "is_err"
                    ) {
                        let bool_sort = z3::Sort::bool();
                        let int_sort = z3::Sort::int();
                        let decl = z3::FuncDecl::new(func_name.as_str(), &[&int_sort], &bool_sort);
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
                    let old_int = old_recv.as_int(&mut self.fresh_counter);
                    let decl = self.make_func(method, 1);
                    let result = decl.apply(&[&old_int as &dyn z3::ast::Ast]);
                    Z3Value::Int(result.as_int().unwrap_or_else(|| self.fresh_int()))
                }
                // Fallback: encode the inner expression directly
                _ => self.encode_expr(inner),
            },

            // --- Forall quantifier ---
            Expr::Forall { var, domain, body } => {
                let bound = ast::Int::new_const(var.as_str());
                self.vars.insert(var.clone(), Z3Value::Int(bound.clone()));
                let body_val = self.encode_expr(body);
                let body_bool = body_val.as_bool();
                let guarded = self.guard_quantifier_body(domain, &bound, &body_bool, true);
                // Infer trigger patterns from function calls in the body
                let patterns = self.infer_quantifier_patterns(body, var, &bound);
                let pattern_refs: Vec<&z3::Pattern> = patterns.iter().collect();
                let result = ast::forall_const(&[&bound], &pattern_refs, &guarded);
                Z3Value::Bool(result)
            }

            // --- Exists quantifier ---
            Expr::Exists { var, domain, body } => {
                let bound = ast::Int::new_const(var.as_str());
                self.vars.insert(var.clone(), Z3Value::Int(bound.clone()));
                let body_val = self.encode_expr(body);
                let body_bool = body_val.as_bool();
                let guarded = self.guard_quantifier_body(domain, &bound, &body_bool, false);
                let patterns = self.infer_quantifier_patterns(body, var, &bound);
                let pattern_refs: Vec<&z3::Pattern> = patterns.iter().collect();
                let result = ast::exists_const(&[&bound], &pattern_refs, &guarded);
                Z3Value::Bool(result)
            }

            // --- If-then-else ---
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_val = self.encode_expr(cond);
                let cond_bool = cond_val.as_bool();
                let then_val = self.encode_expr(then_branch);

                if let Some(else_br) = else_branch {
                    let else_val = self.encode_expr(else_br);
                    match (&then_val, &else_val) {
                        (Z3Value::Int(t), Z3Value::Int(e)) => Z3Value::Int(cond_bool.ite(t, e)),
                        (Z3Value::Bool(t), Z3Value::Bool(e)) => Z3Value::Bool(cond_bool.ite(t, e)),
                        (Z3Value::Real(t), Z3Value::Real(e)) => Z3Value::Real(cond_bool.ite(t, e)),
                        (Z3Value::Int(t), Z3Value::Real(e)) => {
                            Z3Value::Real(cond_bool.ite(&ast::Real::from_int(t), e))
                        }
                        (Z3Value::Real(t), Z3Value::Int(e)) => {
                            Z3Value::Real(cond_bool.ite(t, &ast::Real::from_int(e)))
                        }
                        _ => {
                            let t = then_val.as_bool();
                            let e = else_val.as_bool();
                            Z3Value::Bool(cond_bool.ite(&t, &e))
                        }
                    }
                } else {
                    // No else: `if P then Q` = `P => Q`
                    let then_bool = then_val.as_bool();
                    Z3Value::Bool(cond_bool.implies(&then_bool))
                }
            }

            // --- Parenthesized ---
            Expr::Paren(inner) => self.encode_expr(inner),

            // --- Raw token sequence: parse operator expression ---
            Expr::Raw(tokens) => self.encode_raw_tokens(tokens),

            // --- Ghost block: encode inner for verification ---
            Expr::Ghost(inner) => self.encode_expr(inner),

            // --- Apply lemma: encode args for constraint propagation.
            //     The lemma's postcondition is injected at the solver level
            //     by verify_clauses. Return a fresh bool (not hardcoded true)
            //     so missing lemmas don't trivially pass. ---
            Expr::Apply { lemma_name, args } => {
                // Encode args for side effects (variable registration) only;
                // the Z3 values are not used because the lemma's result is a
                // named bool constrained via lemma injection in verify_clauses.
                for arg in args {
                    let _side_effect = self.encode_expr(arg);
                }
                // Use a named bool so the solver can constrain it via
                // lemma injection. If the lemma is missing, this stays
                // unconstrained (not trivially true).
                Z3Value::Bool(ast::Bool::new_const(format!("__apply_{lemma_name}")))
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
                            let pat_val = Z3Value::Int(ast::Int::from_i64(self.pattern_hash(name)));
                            match (&scrut, &pat_val) {
                                (Z3Value::Int(a), Z3Value::Int(b)) => a.eq(b),
                                // Overapproximate: type mismatch means we
                                // cannot compare, so assume the arm could
                                // match (sound: may produce spurious
                                // counterexamples but never hides real ones)
                                _ => ast::Bool::from_bool(true),
                            }
                        }
                        assura_parser::ast::Pattern::Literal(lit) => {
                            let lit_val = self.encode_literal(lit);
                            match (&scrut, &lit_val) {
                                (Z3Value::Int(a), Z3Value::Int(b)) => a.eq(b),
                                (Z3Value::Bool(a), Z3Value::Bool(b)) => a.eq(b),
                                (Z3Value::Real(a), Z3Value::Real(b)) => a.eq(b),
                                // Cross-sort: promote Int to Real
                                (Z3Value::Int(a), Z3Value::Real(b)) => ast::Real::from_int(a).eq(b),
                                (Z3Value::Real(a), Z3Value::Int(b)) => a.eq(ast::Real::from_int(b)),
                                // Overapproximate: unresolvable type
                                // mismatch, assume arm could match
                                _ => ast::Bool::from_bool(true),
                            }
                        }
                        // Constructor and Tuple patterns bind variables
                        // but always match in this overapproximation.
                        assura_parser::ast::Pattern::Constructor { .. }
                        | assura_parser::ast::Pattern::Tuple(_) => ast::Bool::from_bool(true),
                        _ => ast::Bool::from_bool(true),
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

            // --- Tuple: model as an Int with element-access axioms ---
            Expr::Tuple(elems) => {
                let tuple_val = self.fresh_int();
                let arity = elems.len();
                for (i, elem) in elems.iter().enumerate() {
                    let elem_val = self.encode_expr(elem);
                    // Assert: __tuple_{arity}_{i}(tuple) == elem_val
                    let accessor_name = format!("__tuple_{arity}_{i}");
                    let accessor = self.make_func(&accessor_name, 1);
                    let accessed = accessor
                        .apply(&[&tuple_val as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let elem_int = elem_val.as_int(&mut self.fresh_counter);
                    self.background_axioms.push(accessed.eq(&elem_int));
                }
                Z3Value::Int(tuple_val)
            }

            // --- Cast: encode inner (the value doesn't change, only its type) ---
            Expr::Cast { expr, .. } => self.encode_expr(expr),

            // --- List: model as an Int with element-access and length axioms ---
            Expr::List(elems) => {
                let list_val = self.fresh_int();
                for (i, elem) in elems.iter().enumerate() {
                    let elem_val = self.encode_expr(elem);
                    // Assert: __list_get(list, i) == elem_val
                    let accessor = self.make_func("__list_get", 2);
                    let idx = ast::Int::from_i64(i as i64);
                    let accessed = accessor
                        .apply(&[&list_val as &dyn z3::ast::Ast, &idx as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let elem_int = elem_val.as_int(&mut self.fresh_counter);
                    self.background_axioms.push(accessed.eq(&elem_int));
                }
                // Assert length
                let len_decl = self.make_func("__field_len", 1);
                let len_result = len_decl
                    .apply(&[&list_val as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                let expected_len = ast::Int::from_i64(elems.len() as i64);
                self.background_axioms.push(len_result.eq(&expected_len));
                Z3Value::Int(list_val)
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
    fn encode_raw_tokens(&mut self, tokens: &[String]) -> Z3Value {
        if tokens.is_empty() {
            // Empty clause body is vacuously true (e.g. an ensures
            // clause with no expression defaults to trivially satisfied).
            return Z3Value::Bool(ast::Bool::from_bool(true));
        }

        // Try to parse as a structured expression
        let parsed = self.parse_raw_expr(tokens, 0);
        parsed.0
    }

    /// Parse raw tokens with operator precedence.
    ///
    /// Returns (value, next_position).
    fn parse_raw_expr(&mut self, tokens: &[String], min_prec: u8) -> (Z3Value, usize) {
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
    fn parse_raw_atom(&mut self, tokens: &[String], start: usize) -> (Z3Value, usize) {
        if start >= tokens.len() {
            // Past end of tokens: treat as vacuously true.
            return (Z3Value::Bool(ast::Bool::from_bool(true)), start);
        }

        let tok = &tokens[start];

        // --- Unary not ---
        if tok == "not" || tok == "!" {
            let (val, next) = self.parse_raw_atom(tokens, start + 1);
            let b = val.as_bool();
            return (Z3Value::Bool(b.not()), next);
        }

        // --- Unary minus ---
        if tok == "-" {
            let (val, next) = self.parse_raw_atom(tokens, start + 1);
            let i = val.as_int(&mut self.fresh_counter);
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
            return (Z3Value::Bool(ast::Bool::from_bool(true)), start + 1);
        }
        if tok == "false" {
            return (Z3Value::Bool(ast::Bool::from_bool(false)), start + 1);
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
                let bound = ast::Int::new_const(var_name.as_str());
                self.vars
                    .insert(var_name.clone(), Z3Value::Int(bound.clone()));

                // Parse body
                let (body_val, _) = self.parse_raw_expr(body_tokens, 0);
                let body_bool = body_val.as_bool();

                // Build Z3 quantifier
                let bound_ref = &bound;
                let pattern = z3::Pattern::new(&[bound_ref as &dyn z3::ast::Ast]);
                let q = if is_forall {
                    z3::ast::forall_const(
                        &[bound_ref as &dyn z3::ast::Ast],
                        &[&pattern],
                        &body_bool,
                    )
                } else {
                    z3::ast::exists_const(
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
            return (Z3Value::Int(ast::Int::from_i64(n)), start + 1);
        }

        // --- Float literal ---
        if tok.contains('.')
            && let Ok(f) = tok.parse::<f64>()
        {
            let denom = 1_000_000i64;
            let numer = (f * denom as f64) as i64;
            return (
                Z3Value::Real(ast::Real::from_rational(numer, denom)),
                start + 1,
            );
        }

        // #200: Skip taint/ghost/region/validate keywords in raw tokens;
        // they are specification-level annotations, not Z3 variables.
        if matches!(
            tok.as_str(),
            "taint" | "untrusted" | "validated" | "ghost" | "Region" | "validate"
        ) {
            // Skip the keyword and continue parsing the next token
            return self.parse_raw_atom(tokens, start + 1);
        }

        // --- Identifier (possibly with dot-separated field access) ---
        let mut name = tok.clone();
        let mut next = start + 1;
        // #198: Collapse `x.y.z` chains into `x__y__z` for Z3 (flat variable)
        while next + 1 < tokens.len() && tokens[next] == "." {
            name.push_str("__");
            name.push_str(&tokens[next + 1]);
            next += 2;
        }

        // --- #262: Typestate annotation: `Type @ State` ---
        // After collapsing dot chains, if the next token is `@` followed
        // by a state name, encode as integer equality:
        //   __typestate_<name> == hash(state_name)
        if next + 1 < tokens.len() && tokens[next] == "@" {
            let state_name = &tokens[next + 1];
            let ts_var_name = format!("__typestate_{name}");
            let ts_var = self.get_or_create_int(&ts_var_name);
            let state_val = ast::Int::from_i64(self.pattern_hash(state_name));
            return (Z3Value::Bool(ts_var.eq(&state_val)), next + 2);
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
            let mut arg_vals: Vec<ast::Int> = Vec::new();
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
                                arg_vals.push(v.as_int(&mut self.fresh_counter));
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
                    arg_vals.push(v.as_int(&mut self.fresh_counter));
                }
            }
            let end = p + 1; // skip closing ')'

            // Extract the base function name (last segment after dots)
            let func_name = name.rsplit('.').next().unwrap_or(&name);

            // Built-in functions with known semantics
            match func_name {
                "abs" if arg_vals.len() == 1 => {
                    let x = &arg_vals[0];
                    let zero = ast::Int::from_i64(0);
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
                let bool_sort = z3::Sort::bool();
                let int_sort = z3::Sort::int();
                let arity = arg_vals.len().max(1);
                let param_sorts: Vec<&z3::Sort> = (0..arity).map(|_| &int_sort).collect();
                let decl = z3::FuncDecl::new(func_name, &param_sorts, &bool_sort);
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
                let zero = ast::Int::from_i64(0);
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
    fn apply_raw_op(&mut self, op: RawOp, lhs: Z3Value, rhs: Z3Value) -> Z3Value {
        match op {
            RawOp::Add => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Int(ast::Int::add(&[&l, &r]))
            }
            RawOp::Sub => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Int(ast::Int::sub(&[&l, &r]))
            }
            RawOp::Mul => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Int(ast::Int::mul(&[&l, &r]))
            }
            RawOp::Div => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Int(l.div(&r))
            }
            RawOp::Mod => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Int(l.rem(&r))
            }
            RawOp::Eq => match (&lhs, &rhs) {
                (Z3Value::Bool(l), Z3Value::Bool(r)) => Z3Value::Bool(l.eq(r)),
                (Z3Value::Str(l), Z3Value::Str(r)) => Z3Value::Bool(l.eq(r)),
                _ => {
                    let l = lhs.as_int(&mut self.fresh_counter);
                    let r = rhs.as_int(&mut self.fresh_counter);
                    Z3Value::Bool(l.eq(&r))
                }
            },
            RawOp::Neq => match (&lhs, &rhs) {
                (Z3Value::Bool(l), Z3Value::Bool(r)) => Z3Value::Bool(l.eq(r).not()),
                (Z3Value::Str(l), Z3Value::Str(r)) => Z3Value::Bool(l.eq(r).not()),
                _ => {
                    let l = lhs.as_int(&mut self.fresh_counter);
                    let r = rhs.as_int(&mut self.fresh_counter);
                    Z3Value::Bool(l.eq(&r).not())
                }
            },
            RawOp::Lt => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Bool(l.lt(&r))
            }
            RawOp::Lte => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Bool(l.le(&r))
            }
            RawOp::Gt => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Bool(l.gt(&r))
            }
            RawOp::Gte => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Bool(l.ge(&r))
            }
            RawOp::And => {
                let l = lhs.as_bool();
                let r = rhs.as_bool();
                Z3Value::Bool(ast::Bool::and(&[&l, &r]))
            }
            RawOp::Or => {
                let l = lhs.as_bool();
                let r = rhs.as_bool();
                Z3Value::Bool(ast::Bool::or(&[&l, &r]))
            }
            RawOp::Implies => {
                let l = lhs.as_bool();
                let r = rhs.as_bool();
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
    fn encode_binop(&mut self, lhs: &Expr, op: &BinOp, rhs: &Expr) -> Z3Value {
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
            let l = left_cmp.as_bool();
            let r = right_cmp.as_bool();
            return Z3Value::Bool(ast::Bool::and(&[&l, &r]));
        }

        let lv = self.encode_expr(lhs);
        let rv = self.encode_expr(rhs);

        match op {
            // --- Arithmetic: produce Int or Real depending on operands ---
            BinOp::Add => {
                if Self::is_real(&lv) || Self::is_real(&rv) {
                    let l = lv.as_real(&mut self.fresh_counter);
                    let r = rv.as_real(&mut self.fresh_counter);
                    Z3Value::Real(ast::Real::add(&[&l, &r]))
                } else {
                    let l = lv.as_int(&mut self.fresh_counter);
                    let r = rv.as_int(&mut self.fresh_counter);
                    Z3Value::Int(ast::Int::add(&[&l, &r]))
                }
            }
            BinOp::Sub => {
                if Self::is_real(&lv) || Self::is_real(&rv) {
                    let l = lv.as_real(&mut self.fresh_counter);
                    let r = rv.as_real(&mut self.fresh_counter);
                    Z3Value::Real(ast::Real::sub(&[&l, &r]))
                } else {
                    let l = lv.as_int(&mut self.fresh_counter);
                    let r = rv.as_int(&mut self.fresh_counter);
                    Z3Value::Int(ast::Int::sub(&[&l, &r]))
                }
            }
            BinOp::Mul => {
                if Self::is_real(&lv) || Self::is_real(&rv) {
                    let l = lv.as_real(&mut self.fresh_counter);
                    let r = rv.as_real(&mut self.fresh_counter);
                    Z3Value::Real(ast::Real::mul(&[&l, &r]))
                } else {
                    let l = lv.as_int(&mut self.fresh_counter);
                    let r = rv.as_int(&mut self.fresh_counter);
                    Z3Value::Int(ast::Int::mul(&[&l, &r]))
                }
            }
            BinOp::Div => {
                if Self::is_real(&lv) || Self::is_real(&rv) {
                    let l = lv.as_real(&mut self.fresh_counter);
                    let r = rv.as_real(&mut self.fresh_counter);
                    Z3Value::Real(l.div(&r))
                } else {
                    let l = lv.as_int(&mut self.fresh_counter);
                    let r = rv.as_int(&mut self.fresh_counter);
                    Z3Value::Int(l.div(&r))
                }
            }
            BinOp::Mod => {
                let l = lv.as_int(&mut self.fresh_counter);
                let r = rv.as_int(&mut self.fresh_counter);
                Z3Value::Int(l.rem(&r))
            }

            // --- Comparison: produce Bool (promote to Real if needed) ---
            BinOp::Eq => match (&lv, &rv) {
                (Z3Value::Int(l), Z3Value::Int(r)) => Z3Value::Bool(l.eq(r)),
                (Z3Value::Bool(l), Z3Value::Bool(r)) => Z3Value::Bool(l.eq(r)),
                (Z3Value::Real(l), Z3Value::Real(r)) => Z3Value::Bool(l.eq(r)),
                _ if Self::is_real(&lv) || Self::is_real(&rv) => {
                    let l = lv.as_real(&mut self.fresh_counter);
                    let r = rv.as_real(&mut self.fresh_counter);
                    Z3Value::Bool(l.eq(&r))
                }
                _ => {
                    let l = lv.as_int(&mut self.fresh_counter);
                    let r = rv.as_int(&mut self.fresh_counter);
                    Z3Value::Bool(l.eq(&r))
                }
            },
            BinOp::Neq => match (&lv, &rv) {
                (Z3Value::Int(l), Z3Value::Int(r)) => Z3Value::Bool(l.eq(r).not()),
                (Z3Value::Bool(l), Z3Value::Bool(r)) => Z3Value::Bool(l.eq(r).not()),
                (Z3Value::Real(l), Z3Value::Real(r)) => Z3Value::Bool(l.eq(r).not()),
                _ if Self::is_real(&lv) || Self::is_real(&rv) => {
                    let l = lv.as_real(&mut self.fresh_counter);
                    let r = rv.as_real(&mut self.fresh_counter);
                    Z3Value::Bool(l.eq(&r).not())
                }
                _ => {
                    let l = lv.as_int(&mut self.fresh_counter);
                    let r = rv.as_int(&mut self.fresh_counter);
                    Z3Value::Bool(l.eq(&r).not())
                }
            },
            BinOp::Lt => {
                if Self::is_real(&lv) || Self::is_real(&rv) {
                    let l = lv.as_real(&mut self.fresh_counter);
                    let r = rv.as_real(&mut self.fresh_counter);
                    Z3Value::Bool(l.lt(&r))
                } else {
                    let l = lv.as_int(&mut self.fresh_counter);
                    let r = rv.as_int(&mut self.fresh_counter);
                    Z3Value::Bool(l.lt(&r))
                }
            }
            BinOp::Lte => {
                if Self::is_real(&lv) || Self::is_real(&rv) {
                    let l = lv.as_real(&mut self.fresh_counter);
                    let r = rv.as_real(&mut self.fresh_counter);
                    Z3Value::Bool(l.le(&r))
                } else {
                    let l = lv.as_int(&mut self.fresh_counter);
                    let r = rv.as_int(&mut self.fresh_counter);
                    Z3Value::Bool(l.le(&r))
                }
            }
            BinOp::Gt => {
                if Self::is_real(&lv) || Self::is_real(&rv) {
                    let l = lv.as_real(&mut self.fresh_counter);
                    let r = rv.as_real(&mut self.fresh_counter);
                    Z3Value::Bool(l.gt(&r))
                } else {
                    let l = lv.as_int(&mut self.fresh_counter);
                    let r = rv.as_int(&mut self.fresh_counter);
                    Z3Value::Bool(l.gt(&r))
                }
            }
            BinOp::Gte => {
                if Self::is_real(&lv) || Self::is_real(&rv) {
                    let l = lv.as_real(&mut self.fresh_counter);
                    let r = rv.as_real(&mut self.fresh_counter);
                    Z3Value::Bool(l.ge(&r))
                } else {
                    let l = lv.as_int(&mut self.fresh_counter);
                    let r = rv.as_int(&mut self.fresh_counter);
                    Z3Value::Bool(l.ge(&r))
                }
            }

            // --- Logical: produce Bool ---
            BinOp::And => {
                let l = lv.as_bool();
                let r = rv.as_bool();
                Z3Value::Bool(ast::Bool::and(&[&l, &r]))
            }
            BinOp::Or => {
                let l = lv.as_bool();
                let r = rv.as_bool();
                Z3Value::Bool(ast::Bool::or(&[&l, &r]))
            }
            BinOp::Implies => {
                let l = lv.as_bool();
                let r = rv.as_bool();
                Z3Value::Bool(l.implies(&r))
            }

            // --- Membership: uninterpreted function __contains(set, elem) ---
            BinOp::In | BinOp::NotIn => {
                let l = lv.as_int(&mut self.fresh_counter);
                let r = rv.as_int(&mut self.fresh_counter);
                let decl = self.make_func("__contains", 2);
                let result = decl.apply(&[&r as &dyn z3::ast::Ast, &l as &dyn z3::ast::Ast]);
                let contains_int = result.as_int().unwrap_or_else(|| self.fresh_int());
                // __contains returns 0 for false, non-zero for true
                let zero = ast::Int::from_i64(0);
                let is_member = contains_int.eq(&zero).not();
                if matches!(op, BinOp::NotIn) {
                    Z3Value::Bool(is_member.not())
                } else {
                    Z3Value::Bool(is_member)
                }
            }
            BinOp::Concat => {
                // String/list concat: result is a fresh value with
                // length axiom: len(a ++ b) == len(a) + len(b)
                let l = lv.as_int(&mut self.fresh_counter);
                let r = rv.as_int(&mut self.fresh_counter);
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
                let zero = ast::Int::from_i64(0);
                self.background_axioms.push(len_l.ge(&zero));
                self.background_axioms.push(len_r.ge(&zero));
                // len(a ++ b) == len(a) + len(b)
                let sum = ast::Int::add(&[&len_l, &len_r]);
                self.background_axioms.push(len_result.eq(&sum));
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
// Unmodelable feature detection
// -----------------------------------------------------------------------

/// Returns `true` if the expression tree contains features that the SMT
/// encoder cannot faithfully represent (field-access chains on `self`,
/// typestate annotations, taint annotations, validate blocks, region
/// types, etc.).
pub(crate) fn expr_has_unmodelable_features(expr: &Expr) -> bool {
    match expr {
        // #198: Field access is now always modelable. Deep field chains
        // are flattened into single Z3 variables, and self-rooted access
        // is treated the same as any other variable.
        Expr::Field(obj, _field) => expr_has_unmodelable_features(obj),
        // #201: Method calls are now always modelable. Unknown methods
        // are encoded as uninterpreted functions (sound overapproximation).
        Expr::MethodCall {
            receiver,
            method: _,
            args,
        } => {
            expr_has_unmodelable_features(receiver)
                || args.iter().any(expr_has_unmodelable_features)
        }
        // #200, #262: Raw tokens for taint, ghost, region, validate, and
        // typestate are now modelable. Ghost vars are regular Z3 vars,
        // taint levels are encoded as integers, regions as bounded
        // constraints, dotted field access is flattened, and typestate
        // `@` annotations are encoded as integer equality checks.
        Expr::Raw(_tokens) => false,
        Expr::BinOp { lhs, rhs, .. } => {
            expr_has_unmodelable_features(lhs) || expr_has_unmodelable_features(rhs)
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Paren(inner)
        | Expr::Old(inner)
        | Expr::Ghost(inner)
        | Expr::Cast { expr: inner, .. } => expr_has_unmodelable_features(inner),
        Expr::Call { func, args } => {
            expr_has_unmodelable_features(func) || args.iter().any(expr_has_unmodelable_features)
        }
        Expr::Index { expr: e, index } => {
            expr_has_unmodelable_features(e) || expr_has_unmodelable_features(index)
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            expr_has_unmodelable_features(domain) || expr_has_unmodelable_features(body)
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_has_unmodelable_features(cond)
                || expr_has_unmodelable_features(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| expr_has_unmodelable_features(e))
        }
        Expr::Let { value, body, .. } => {
            expr_has_unmodelable_features(value) || expr_has_unmodelable_features(body)
        }
        Expr::Match { scrutinee, arms } => {
            expr_has_unmodelable_features(scrutinee)
                || arms.iter().any(|a| expr_has_unmodelable_features(&a.body))
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            items.iter().any(expr_has_unmodelable_features)
        }
        Expr::Apply { args, .. } => args.iter().any(expr_has_unmodelable_features),
        Expr::Literal(_) | Expr::Ident(_) => false,
    }
}

fn is_self_rooted(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(name) => name == "self",
        Expr::Field(obj, _) => is_self_rooted(obj),
        Expr::Paren(inner) => is_self_rooted(inner),
        _ => false,
    }
}

/// Returns `true` if `expr` is a field access chain of depth >= 2
/// (e.g., `state.head.extra`). Single-level field access (`buf.len`)
/// is handled by the encoder, but deeper chains produce unconstrained
/// nested uninterpreted functions that Z3 finds trivial counterexamples for.
fn has_deep_field_chain(expr: &Expr) -> bool {
    field_chain_depth(expr) >= 2
}

fn field_chain_depth(expr: &Expr) -> usize {
    match expr {
        Expr::Field(obj, _) => 1 + field_chain_depth(obj),
        Expr::Paren(inner) => field_chain_depth(inner),
        _ => 0,
    }
}

/// Flatten a field chain like `state.head.extra.extra_max` into a single
/// Z3 variable name `state__head__extra__extra_max`. This avoids nested
/// uninterpreted functions that produce unconstrained counterexamples.
fn flatten_field_chain(expr: &Expr) -> String {
    match expr {
        Expr::Field(obj, field) => {
            let prefix = flatten_field_chain(obj);
            format!("{prefix}__{field}")
        }
        Expr::Ident(name) => name.clone(),
        Expr::Paren(inner) => flatten_field_chain(inner),
        _ => format!("__obj_{:p}", expr as *const _),
    }
}

pub(super) fn collect_unmodelable_reasons(_expr: &Expr) -> Vec<String> {
    // #198, #200, #201, #262: All expression types are now modelable.
    // Field access, method calls, raw tokens (including typestate @),
    // taint, ghost, region, and validate are all encoded in SMT.
    // This function returns an empty list but is kept for API stability.
    Vec::new()
}
