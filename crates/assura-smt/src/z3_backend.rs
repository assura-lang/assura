//! Z3 backend: encodes Assura contract clauses as Z3 ASTs and checks
//! satisfiability. Handles expression encoding, quantifiers, measures,
//! raw-token parsing, and counterexample extraction.

use super::*;
use assura_parser::ast::{BinOp, BlockKind, Clause, Literal, UnaryOp};
use assura_types::checkers::expr_references_var;
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
    /// Trigger manager for quantifier e-matching hints
    trigger_manager: crate::advanced::TriggerManager,
}

impl<'ctx> Encoder<'ctx> {
    fn new(ctx: &'ctx Context) -> Self {
        Self {
            ctx,
            vars: HashMap::new(),
            func_arities: HashMap::new(),
            fresh_counter: 0,
            background_axioms: Vec::new(),
            trigger_manager: crate::advanced::TriggerManager::new(),
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

    /// Infer Z3 trigger patterns from function calls in a quantifier body
    /// that reference the bound variable. Returns patterns for e-matching
    /// hints that help the solver instantiate quantifiers efficiently.
    fn infer_quantifier_patterns(
        &mut self,
        body: &Expr,
        bound_var: &str,
        bound_z3: &ast::Int<'ctx>,
    ) -> Vec<z3::Pattern<'ctx>> {
        let mut patterns = Vec::new();

        // Check TriggerManager for user-provided or inferred triggers
        let body_str = format!("{body:?}");
        if let Some(trigger) = self.trigger_manager.infer_trigger(&body_str) {
            for term in &trigger.terms {
                if let Some(fname) = term.split('(').next() {
                    let int_sort = z3::Sort::int(self.ctx);
                    let func = z3::FuncDecl::new(self.ctx, fname.trim(), &[&int_sort], &int_sort);
                    let bound_dyn: &dyn z3::ast::Ast<'ctx> = bound_z3;
                    let app = func.apply(&[bound_dyn]);
                    let pat = z3::Pattern::new(self.ctx, &[&app]);
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
        bound_z3: &ast::Int<'ctx>,
        patterns: &mut Vec<z3::Pattern<'ctx>>,
    ) {
        match expr {
            Expr::Call { func, args } => {
                let refs_bound = args.iter().any(|a| expr_references_var(a, bound_var));
                if refs_bound && let Expr::Ident(fname) = func.as_ref() {
                    let int_sort = z3::Sort::int(self.ctx);
                    let arity = args.len();
                    let param_sorts: Vec<&z3::Sort<'_>> = (0..arity).map(|_| &int_sort).collect();
                    let func_decl =
                        z3::FuncDecl::new(self.ctx, fname.as_str(), &param_sorts, &int_sort);
                    let z3_args: Vec<ast::Dynamic<'ctx>> = args
                        .iter()
                        .map(|a| {
                            if expr_references_var(a, bound_var) {
                                ast::Dynamic::from_ast(bound_z3)
                            } else {
                                ast::Dynamic::from_ast(&ast::Int::new_const(
                                    self.ctx,
                                    "__trigger_other",
                                ))
                            }
                        })
                        .collect();
                    let arg_refs: Vec<&dyn z3::ast::Ast<'ctx>> = z3_args
                        .iter()
                        .map(|d| d as &dyn z3::ast::Ast<'ctx>)
                        .collect();
                    let app = func_decl.apply(&arg_refs);
                    let pat = z3::Pattern::new(self.ctx, &[&app]);
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
            let decl = z3::FuncDecl::new(self.ctx, func_name.as_str(), &[&int_sort], &bool_sort);
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
            assura_parser::ast::Pattern::Wildcard | assura_parser::ast::Pattern::Literal(_) => {}
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
                // Clamp to i32 safe range then encode as rational to avoid
                // overflow for values > 2147 (i32::MAX / 1_000_000).
                let denom = 1_000_000i32;
                let clamped = f.clamp(-2_000_000_000.0, 2_000_000_000.0);
                let numer = (clamped * denom as f64) as i32;
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
            Expr::Literal(Literal::Bool(b)) => Z3Value::Bool(ast::Bool::from_bool(self.ctx, *b)),

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
                let guarded = self.guard_quantifier_body(domain, &bound, &body_bool, true);
                // Infer trigger patterns from function calls in the body
                let patterns = self.infer_quantifier_patterns(body, var, &bound);
                let pattern_refs: Vec<&z3::Pattern<'ctx>> = patterns.iter().collect();
                let result = ast::forall_const(self.ctx, &[&bound], &pattern_refs, &guarded);
                Z3Value::Bool(result)
            }

            // --- Exists quantifier ---
            Expr::Exists { var, domain, body } => {
                let bound = ast::Int::new_const(self.ctx, var.as_str());
                self.vars.insert(var.clone(), Z3Value::Int(bound.clone()));
                let body_val = self.encode_expr(body);
                let body_bool = body_val.as_bool(self.ctx);
                let guarded = self.guard_quantifier_body(domain, &bound, &body_bool, false);
                let patterns = self.infer_quantifier_patterns(body, var, &bound);
                let pattern_refs: Vec<&z3::Pattern<'ctx>> = patterns.iter().collect();
                let result = ast::exists_const(self.ctx, &[&bound], &pattern_refs, &guarded);
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
                        (Z3Value::Bool(t), Z3Value::Bool(e)) => Z3Value::Bool(cond_bool.ite(t, e)),
                        (Z3Value::Real(t), Z3Value::Real(e)) => Z3Value::Real(cond_bool.ite(t, e)),
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
                            let pat_val =
                                Z3Value::Int(ast::Int::from_i64(self.ctx, self.pattern_hash(name)));
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
    fn apply_raw_op(&mut self, op: RawOp, lhs: Z3Value<'ctx>, rhs: Z3Value<'ctx>) -> Z3Value<'ctx> {
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
// Unmodelable feature detection
// -----------------------------------------------------------------------

/// Returns `true` if the expression tree contains features that the SMT
/// encoder cannot faithfully represent (field-access chains on `self`,
/// typestate annotations, taint annotations, validate blocks, region
/// types, etc.).
fn expr_has_unmodelable_features(expr: &Expr) -> bool {
    match expr {
        // Field access: `obj.field` is encoded as `__field_X(obj)`, an
        // uninterpreted function. This is sound only for known fields
        // (len, is_empty, etc.) where the encoder adds axioms. For all
        // other fields, Z3 treats the result as completely unconstrained,
        // leading to trivial counterexamples.
        Expr::Field(obj, field) => {
            if has_deep_field_chain(expr) || is_self_rooted(obj) {
                return true;
            }
            // Known fields that the encoder constrains with axioms
            let known = matches!(
                field.as_str(),
                "len"
                    | "length"
                    | "size"
                    | "capacity"
                    | "count"
                    | "is_empty"
                    | "is_some"
                    | "is_none"
                    | "is_ok"
                    | "is_err"
            );
            if !known {
                return true;
            }
            expr_has_unmodelable_features(obj)
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            // Known boolean methods are handled by the encoder with correct
            // return types (Bool). Unknown methods produce unconstrained
            // uninterpreted functions that lead to false counterexamples.
            let known_method = matches!(
                method.as_str(),
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
                    | "len"
                    | "length"
                    | "size"
                    | "substring"
                    | "substr"
                    | "min"
                    | "max"
                    | "abs"
            );
            if !known_method {
                return true;
            }
            expr_has_unmodelable_features(receiver)
                || args.iter().any(expr_has_unmodelable_features)
        }
        Expr::Raw(tokens) => {
            // Check for specific unmodelable keywords
            if tokens.iter().any(|t| {
                matches!(
                    t.as_str(),
                    "@" | "taint" | "validate" | "Region" | "ghost" | "untrusted" | "validated"
                )
            }) {
                return true;
            }
            // Check for dotted field access (e.g., `state.field`, `obj.a.b`).
            // The raw token encoder collapses `x.y.z` into a single flat
            // variable name with no structural constraints, so Z3 treats
            // `state.extra_bytes_copied` and `state.head.extra.extra_max`
            // as completely independent unconstrained integers.
            tokens.iter().any(|t| t == ".")
        }
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

fn collect_unmodelable_reasons(expr: &Expr) -> Vec<String> {
    let mut reasons = Vec::new();
    collect_unmodelable_reasons_inner(expr, &mut reasons);
    reasons.sort();
    reasons.dedup();
    reasons
}

fn collect_unmodelable_reasons_inner(expr: &Expr, reasons: &mut Vec<String>) {
    match expr {
        Expr::Field(obj, field) => {
            if is_self_rooted(obj) {
                reasons.push("struct field access".into());
            } else if has_deep_field_chain(expr) {
                reasons.push("deep field chain".into());
            } else {
                let known = matches!(
                    field.as_str(),
                    "len"
                        | "length"
                        | "size"
                        | "capacity"
                        | "count"
                        | "is_empty"
                        | "is_some"
                        | "is_none"
                        | "is_ok"
                        | "is_err"
                );
                if !known {
                    reasons.push("unconstrained field access".into());
                }
            }
        }
        Expr::MethodCall { method, .. } => {
            let known_method = matches!(
                method.as_str(),
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
                    | "len"
                    | "length"
                    | "size"
                    | "substring"
                    | "substr"
                    | "min"
                    | "max"
                    | "abs"
            );
            if !known_method {
                reasons.push("method call".into());
            }
        }
        Expr::Raw(tokens) => {
            for t in tokens {
                match t.as_str() {
                    "@" => reasons.push("typestate annotation".into()),
                    "taint" | "untrusted" | "validated" => {
                        reasons.push("taint annotation".into());
                    }
                    "validate" => reasons.push("validate block".into()),
                    "Region" => reasons.push("region type".into()),
                    "ghost" => reasons.push("ghost code".into()),
                    "." => reasons.push("field access in raw clause".into()),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    match expr {
        Expr::BinOp { lhs, rhs, .. } => {
            collect_unmodelable_reasons_inner(lhs, reasons);
            collect_unmodelable_reasons_inner(rhs, reasons);
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Paren(inner)
        | Expr::Old(inner)
        | Expr::Ghost(inner)
        | Expr::Cast { expr: inner, .. }
        | Expr::Field(inner, _) => {
            collect_unmodelable_reasons_inner(inner, reasons);
        }
        Expr::Call { func, args } => {
            collect_unmodelable_reasons_inner(func, reasons);
            for a in args {
                collect_unmodelable_reasons_inner(a, reasons);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_unmodelable_reasons_inner(receiver, reasons);
            for a in args {
                collect_unmodelable_reasons_inner(a, reasons);
            }
        }
        Expr::Index { expr: e, index } => {
            collect_unmodelable_reasons_inner(e, reasons);
            collect_unmodelable_reasons_inner(index, reasons);
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_unmodelable_reasons_inner(domain, reasons);
            collect_unmodelable_reasons_inner(body, reasons);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_unmodelable_reasons_inner(cond, reasons);
            collect_unmodelable_reasons_inner(then_branch, reasons);
            if let Some(eb) = else_branch {
                collect_unmodelable_reasons_inner(eb, reasons);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_unmodelable_reasons_inner(value, reasons);
            collect_unmodelable_reasons_inner(body, reasons);
        }
        Expr::Match { scrutinee, arms } => {
            collect_unmodelable_reasons_inner(scrutinee, reasons);
            for a in arms {
                collect_unmodelable_reasons_inner(&a.body, reasons);
            }
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                collect_unmodelable_reasons_inner(item, reasons);
            }
        }
        Expr::Apply { args, .. } => {
            for a in args {
                collect_unmodelable_reasons_inner(a, reasons);
            }
        }
        _ => {}
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
        ClauseKind::Ordering => "ordering",
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
        // arity > 0 produce multi-line `{ value }` blocks in the model)
        if decl.arity() > 0 {
            continue;
        }
        let name = decl.name();
        // Skip internal/fresh/coercion variables, but keep __result
        if name.starts_with("__") && name != "__result" {
            continue;
        }
        // Try to get the interpretation as a string
        let value = model
            .get_const_interp(&decl.apply(&[]))
            .map(|v| format!("{v}"))
            .unwrap_or_else(|| "?".into());
        // Strip __field_ prefix from variable names leaked by the encoder
        let clean_name = name.strip_prefix("__field_").unwrap_or(&name).to_string();
        variables.push((clean_name, value));
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
fn check_satisfiability(solver: &Solver<'_>, desc: String, results: &mut Vec<VerificationResult>) {
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

        // Skip clauses that reference features not yet encoded in SMT.
        // Sending an incomplete encoding to Z3 produces false counterexamples
        // (Z3 finds trivial models for unconstrained uninterpreted functions).
        if expr_has_unmodelable_features(&clause.body) {
            let reasons = collect_unmodelable_reasons(&clause.body);
            results.push(VerificationResult::Unknown {
                clause_desc: desc,
                reason: format!(
                    "clause uses features not yet encoded in SMT ({})",
                    reasons.join(", ")
                ),
            });
            continue;
        }

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

        // Register known function names for trigger inference
        for other_clause in clauses {
            collect_function_names_for_triggers(&other_clause.body, &mut encoder.trigger_manager);
        }

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
pub(crate) fn verify_buffer_bounds_impl(requires: &[Expr], ensures: &Expr) -> VerificationResult {
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
    let param_sorts: Vec<&z3::Sort<'_>> = measure.param_sorts.iter().map(|_| &int_sort).collect();

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
                let Some(app_int) = app.as_int() else {
                    continue;
                };
                let ge_zero = app_int.ge(&zero);
                let forall = ast::forall_const(ctx, &[&xs], &[], &ge_zero);
                solver.assert(&forall);
            }
            MeasureAxiomTag::EmptyIsZero => {
                // measure(empty) == 0, where empty is represented as a
                // distinguished constant
                let empty = ast::Int::new_const(ctx, "__empty");
                let app = func_decl.apply(&[&empty]);
                let Some(app_int) = app.as_int() else {
                    continue;
                };
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
                let Some(measure_appended_int) = measure_appended.as_int() else {
                    continue;
                };
                let Some(measure_xs_int) = measure_xs.as_int() else {
                    continue;
                };
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
                    let Some(this_int) = this_app.as_int() else {
                        continue;
                    };
                    let Some(other_int) = other_app.as_int() else {
                        continue;
                    };
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
                let Some(app_int) = app.as_int() else {
                    continue;
                };
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

/// Verify a quantified formula using Z3.
///
/// Encodes assumptions and the negated quantified body, then checks
/// satisfiability. UNSAT means the formula holds universally.
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

pub(crate) fn verify_impl_with_timeout(
    typed: &TypedFile,
    timeout_ms: u64,
) -> Vec<VerificationResult> {
    let mut cfg = Config::new();
    cfg.set_param_value("timeout", &timeout_ms.to_string());
    let ctx = Context::new(&cfg);
    let mut results = Vec::new();
    let mut cache = SessionCache::new();

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
            Decl::Bind(b) => {
                verify_clauses(
                    &ctx,
                    &b.name,
                    &b.clauses,
                    &lemma_defs,
                    &mut cache,
                    &mut results,
                );
            }
            // Prophecy variables don't have verifiable clauses directly;
            // they are used as existential witnesses in contract proofs.
            Decl::Prophecy(_) | Decl::CodecRegistry(_) | Decl::TypeDef(_) | Decl::EnumDef(_) => {}
        }
    }

    // Helper: parse a string into the SMT-local MemoryOrdering enum.
    fn parse_memory_ordering(s: &str) -> Option<MemoryOrdering> {
        match s {
            "relaxed" => Some(MemoryOrdering::Relaxed),
            "acquire" => Some(MemoryOrdering::Acquire),
            "release" => Some(MemoryOrdering::Release),
            "acqrel" | "acq_rel" => Some(MemoryOrdering::AcqRel),
            "seq_cst" => Some(MemoryOrdering::SeqCst),
            _ => None,
        }
    }

    // T092: weak memory ordering checks on concurrent contracts
    // Detects ordering from structured ClauseKind::Ordering clauses first,
    // then falls back to keyword scanning in ClauseKind::Effects bodies.
    let mut wm_checker = WeakMemoryChecker::new();
    for decl in &typed.resolved.source.decls {
        let (name, clauses) = match &decl.node {
            Decl::Contract(c) => (c.name.as_str(), &c.clauses),
            Decl::FnDef(f) => (f.name.as_str(), &f.clauses),
            _ => continue,
        };
        // Prefer structured ClauseKind::Ordering over keyword scanning
        let mut found_ordering = false;
        for clause in clauses {
            if clause.kind == ClauseKind::Ordering {
                let ordering_str = match &clause.body {
                    Expr::Ident(s) => Some(s.as_str()),
                    Expr::Raw(tokens) => tokens
                        .iter()
                        .find(|t| parse_memory_ordering(t).is_some())
                        .map(|t| t.as_str()),
                    _ => None,
                };
                if let Some(ord) = ordering_str.and_then(parse_memory_ordering) {
                    wm_checker.record_access(1, name.to_string(), true, ord);
                    found_ordering = true;
                }
            }
        }
        // Fall back to keyword scanning in effects clauses
        if !found_ordering {
            for clause in clauses {
                if clause.kind == ClauseKind::Effects
                    && (expr_references_var(&clause.body, "relaxed")
                        || expr_references_var(&clause.body, "acquire")
                        || expr_references_var(&clause.body, "release")
                        || expr_references_var(&clause.body, "seq_cst"))
                {
                    let ordering = if expr_references_var(&clause.body, "seq_cst") {
                        MemoryOrdering::SeqCst
                    } else if expr_references_var(&clause.body, "acquire") {
                        MemoryOrdering::Acquire
                    } else if expr_references_var(&clause.body, "release") {
                        MemoryOrdering::Release
                    } else {
                        MemoryOrdering::Relaxed
                    };
                    wm_checker.record_access(1, name.to_string(), true, ordering);
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
                // Resolve prophecy variables from resolve() calls
                if clause.kind == ClauseKind::Ensures || clause.kind == ClauseKind::Requires {
                    resolve_prophecy_vars(&clause.body, &f.name, &mut pm);
                }
                // Constrain prophecy variables from constraint expressions
                if clause.kind == ClauseKind::Ensures || clause.kind == ClauseKind::Requires {
                    constrain_prophecy_vars(&clause.body, &f.name, &mut pm);
                }
            }
        }
    }
    for err in pm.check_all_resolved() {
        results.push(VerificationResult::Unknown {
            clause_desc: format!("prophecy [{}]", err.code),
            reason: err.message,
        });
    }
    for err in pm.check_unconstrained() {
        results.push(VerificationResult::Unknown {
            clause_desc: format!("prophecy [{}]", err.code),
            reason: err.message,
        });
    }

    // T094: liveness obligation checks (G006)
    // Extract obligations from structured `liveness` blocks and from
    // contracts that use eventually/leads_to in ensures clauses.
    let mut lc = LivenessChecker::new();
    for decl in &typed.resolved.source.decls {
        match &decl.node {
            Decl::Block {
                kind, name, body, ..
            } if *kind == BlockKind::Liveness => {
                // Extract obligations from liveness block clauses
                for clause in body {
                    match &clause.kind {
                        ClauseKind::Other(k) if k == "assume" => {
                            // Check for fairness assumptions
                            let text = format!("{:?}", clause.body);
                            if text.contains("fair") {
                                lc.add_fairness(format!("{name}:fair"));
                            }
                        }
                        ClauseKind::Other(k) if k == "prove" => {
                            let text = format!("{:?}", clause.body);
                            let liveness_kind = if expr_references_var(&clause.body, "leads_to") {
                                LivenessKind::LeadsTo
                            } else if expr_references_var(&clause.body, "eventually_within") {
                                // Extract bound from the expression if present
                                let bound = extract_numeric_arg(&clause.body).unwrap_or(100);
                                LivenessKind::EventuallyWithin(bound)
                            } else {
                                LivenessKind::Eventually
                            };
                            lc.add_obligation(
                                format!("{name}:prove"),
                                liveness_kind,
                                text.clone(),
                                text,
                            );
                        }
                        _ => {}
                    }
                }
            }
            Decl::Contract(c) => {
                // Also scan contract ensures for legacy liveness patterns
                for clause in &c.clauses {
                    if clause.kind == ClauseKind::Ensures
                        && (expr_references_var(&clause.body, "eventually")
                            || expr_references_var(&clause.body, "leads_to"))
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
            _ => {}
        }
    }
    // Check fairness constraints for leads_to obligations
    for err in lc.check_fairness() {
        results.push(VerificationResult::Unknown {
            clause_desc: "liveness:fairness".into(),
            reason: err,
        });
    }
    // Check bounded obligations have valid bounds
    for err in lc.check_bounded() {
        results.push(VerificationResult::Unknown {
            clause_desc: "liveness:bounds".into(),
            reason: err,
        });
    }
    // BMC verification: attempt bounded model checking for each obligation
    for err in lc.check_unverified() {
        results.push(VerificationResult::Unknown {
            clause_desc: "liveness".into(),
            reason: err,
        });
    }

    // T076: Layer 2 verification (quantified invariants, termination, roundtrip)
    let l2_config = crate::layer2::Layer2Config::new().with_timeout(timeout_ms);
    let mut l2 = crate::layer2::Layer2Verifier::new(l2_config);

    for decl in &typed.resolved.source.decls {
        let (name, clauses): (&str, &[Clause]) = match &decl.node {
            Decl::Contract(c) => (&c.name, &c.clauses),
            Decl::FnDef(f) => (&f.name, &f.clauses),
            _ => continue,
        };
        // Extract invariant clauses as quantified invariants
        for clause in clauses {
            if clause.kind == ClauseKind::Invariant {
                match &clause.body {
                    Expr::Forall { var, domain, body } => {
                        let sort = format!("{domain:?}");
                        l2.add_invariant(crate::layer2::QuantifiedInvariant {
                            name: format!("{name}:invariant"),
                            bound_vars: vec![(var.clone(), sort)],
                            body: format!("{body:?}"),
                            triggers: Vec::new(),
                        });
                    }
                    Expr::Exists { var, domain, body } => {
                        let sort = format!("{domain:?}");
                        l2.add_invariant(crate::layer2::QuantifiedInvariant {
                            name: format!("{name}:invariant"),
                            bound_vars: vec![(var.clone(), sort)],
                            body: format!("{body:?}"),
                            triggers: Vec::new(),
                        });
                    }
                    _ => {}
                }
            }

            // Extract decreases clauses as termination obligations
            if clause.kind == ClauseKind::Decreases {
                l2.add_termination(crate::layer2::TerminationObligation {
                    fn_name: name.to_string(),
                    measure: format!("{:?}", clause.body),
                    recursive_calls: Vec::new(),
                });
            }
        }
    }

    if l2.obligation_count() > 0 {
        for l2r in l2.verify() {
            match l2r {
                crate::layer2::Layer2Result::Verified { invariant, .. } => {
                    results.push(VerificationResult::Verified {
                        clause_desc: format!("layer2:{invariant}"),
                    });
                }
                crate::layer2::Layer2Result::Counterexample {
                    invariant, model, ..
                } => {
                    let model_str = model
                        .iter()
                        .map(|(k, v)| format!("{k} = {v}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    results.push(VerificationResult::Counterexample {
                        clause_desc: format!("layer2:{invariant}"),
                        model: model_str,
                        counter_model: None,
                    });
                }
                crate::layer2::Layer2Result::Timeout {
                    invariant,
                    timeout_ms: t,
                } => {
                    results.push(VerificationResult::Timeout {
                        clause_desc: format!("layer2:{invariant} (timeout {t}ms)"),
                    });
                }
                crate::layer2::Layer2Result::Unknown { invariant, reason } => {
                    results.push(VerificationResult::Unknown {
                        clause_desc: format!("layer2:{invariant}"),
                        reason,
                    });
                }
            }
        }
    }

    // T073: CodecDispatcher ambiguity checking
    let mut codec_disp = crate::advanced::CodecDispatcher::new();
    for decl in &typed.resolved.source.decls {
        if let Decl::CodecRegistry(cr) = &decl.node {
            for entry in &cr.codecs {
                if let assura_parser::ast::MagicPattern::Bytes { bytes, .. } = &entry.magic {
                    codec_disp.register(entry.name.clone(), bytes.clone(), 0);
                }
            }
        }
    }
    for (a, b) in codec_disp.check_ambiguity() {
        results.push(VerificationResult::Unknown {
            clause_desc: format!("codec:ambiguity:{a}/{b}"),
            reason: format!(
                "codecs `{a}` and `{b}` share identical magic bytes at the same offset"
            ),
        });
    }

    results
}

/// Collect function names from an expression tree and register them
/// with the trigger manager for quantifier e-matching.
fn collect_function_names_for_triggers(expr: &Expr, tm: &mut crate::advanced::TriggerManager) {
    match expr {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = func.as_ref() {
                tm.register_function(name.clone());
            }
            for a in args {
                collect_function_names_for_triggers(a, tm);
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            tm.register_function(method.clone());
            collect_function_names_for_triggers(receiver, tm);
            for a in args {
                collect_function_names_for_triggers(a, tm);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_function_names_for_triggers(lhs, tm);
            collect_function_names_for_triggers(rhs, tm);
        }
        Expr::UnaryOp { expr: e, .. } | Expr::Paren(e) | Expr::Old(e) | Expr::Ghost(e) => {
            collect_function_names_for_triggers(e, tm);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_function_names_for_triggers(cond, tm);
            collect_function_names_for_triggers(then_branch, tm);
            if let Some(eb) = else_branch {
                collect_function_names_for_triggers(eb, tm);
            }
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_function_names_for_triggers(domain, tm);
            collect_function_names_for_triggers(body, tm);
        }
        Expr::Index { expr: e, index } => {
            collect_function_names_for_triggers(e, tm);
            collect_function_names_for_triggers(index, tm);
        }
        _ => {}
    }
}

/// Extract a numeric argument from an expression tree (for eventually_within bounds).
fn extract_numeric_arg(expr: &Expr) -> Option<u64> {
    match expr {
        Expr::Literal(assura_parser::ast::Literal::Int(s)) => s.parse().ok(),
        Expr::Call { args, .. } => args.iter().find_map(extract_numeric_arg),
        Expr::Raw(tokens) => tokens.iter().find_map(|t| t.parse::<u64>().ok()),
        Expr::Block(exprs) => exprs.iter().find_map(extract_numeric_arg),
        _ => None,
    }
}

/// Scan an expression for prophecy resolution calls: resolve(var, value).
fn resolve_prophecy_vars(expr: &Expr, fn_name: &str, pm: &mut ProphecyManager) {
    match expr {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = func.as_ref()
                && (name == "resolve" || name == "resolve_prophecy")
                && let Some(Expr::Ident(var_name)) = args.first()
            {
                let value = args.get(1).map(|a| format!("{a:?}")).unwrap_or_default();
                let _ = pm.resolve(&format!("{fn_name}:{var_name}"), value);
            }
            for arg in args {
                resolve_prophecy_vars(arg, fn_name, pm);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            resolve_prophecy_vars(lhs, fn_name, pm);
            resolve_prophecy_vars(rhs, fn_name, pm);
        }
        Expr::UnaryOp { expr, .. } | Expr::Paren(expr) | Expr::Old(expr) | Expr::Ghost(expr) => {
            resolve_prophecy_vars(expr, fn_name, pm)
        }
        Expr::Block(exprs) | Expr::List(exprs) => {
            for e in exprs {
                resolve_prophecy_vars(e, fn_name, pm);
            }
        }
        _ => {}
    }
}

/// Scan an expression for prophecy constraint patterns (equality with prophecy vars).
fn constrain_prophecy_vars(expr: &Expr, fn_name: &str, pm: &mut ProphecyManager) {
    match expr {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = func.as_ref()
                && (name == "constrain" || name == "constrain_prophecy")
                && let Some(Expr::Ident(var_name)) = args.first()
            {
                let constraint = args.get(1).map(|a| format!("{a:?}")).unwrap_or_default();
                pm.add_constraint(&format!("{fn_name}:{var_name}"), constraint);
            }
            for arg in args {
                constrain_prophecy_vars(arg, fn_name, pm);
            }
        }
        Expr::BinOp { lhs, rhs, op } => {
            // An equality like `prophecy(x) == expr` constrains x
            if *op == BinOp::Eq
                && let Expr::Call { func, args } = lhs.as_ref()
                && let Expr::Ident(name) = func.as_ref()
                && (name == "prophecy" || name == "prophesy")
                && let Some(Expr::Ident(var_name)) = args.first()
            {
                pm.add_constraint(&format!("{fn_name}:{var_name}"), format!("{rhs:?}"));
            }
            constrain_prophecy_vars(lhs, fn_name, pm);
            constrain_prophecy_vars(rhs, fn_name, pm);
        }
        Expr::UnaryOp { expr, .. } | Expr::Paren(expr) | Expr::Old(expr) | Expr::Ghost(expr) => {
            constrain_prophecy_vars(expr, fn_name, pm)
        }
        Expr::Block(exprs) | Expr::List(exprs) => {
            for e in exprs {
                constrain_prophecy_vars(e, fn_name, pm);
            }
        }
        _ => {}
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
        Expr::UnaryOp { expr, .. } | Expr::Paren(expr) | Expr::Old(expr) | Expr::Ghost(expr) => {
            collect_prophecy_refs(expr, fn_name, pm)
        }
        _ => {}
    }
}
