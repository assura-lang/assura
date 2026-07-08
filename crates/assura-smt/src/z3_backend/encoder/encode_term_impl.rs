//! `EncodeTerm` implementation for the Z3 `Encoder`.
//!
//! Wraps the existing `Encoder` methods to satisfy the trait interface.
//! This is a thin adapter: each trait method delegates to the corresponding
//! `Encoder` method. No behavior change.

use assura_ast::{BinOp, MatchArm, SpExpr};
use z3::ast;

use super::Encoder;
use super::value::Z3Value;
use crate::encode_term::EncodeTerm;

impl EncodeTerm for Encoder {
    type Term = Z3Value;

    // === Literals ===

    fn make_int_literal(&mut self, s: &str) -> Z3Value {
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

    fn make_bool_literal(&mut self, b: bool) -> Z3Value {
        Z3Value::Bool(ast::Bool::from_bool(b))
    }

    fn make_real_literal(&mut self, numer: i64, denom: i64) -> Z3Value {
        Z3Value::Real(ast::Real::from_rational(numer, denom))
    }

    fn make_string_literal(&mut self, s: &str) -> Z3Value {
        if self.use_string_theory {
            let str_val = ast::String::from(s);
            let len = str_val.length();
            let expected_len = ast::Int::from_i64(s.len() as i64);
            self.background_axioms.push(len.eq(&expected_len));
            Z3Value::Str(str_val)
        } else {
            let const_name = crate::encode_atom_policy::string_literal_const_name(s);
            let str_val = ast::Int::new_const(const_name.clone());
            if !self.string_constants.contains(&const_name) {
                for prev in &self.string_constants {
                    let prev_val = ast::Int::new_const(prev.clone());
                    self.background_axioms.push(str_val.eq(&prev_val).not());
                }
                self.string_constants.push(const_name);
            }
            let len_decl = self.make_func(crate::encode_atom_policy::FIELD_LEN_UF_NAME, 1);
            let len_result = len_decl
                .apply(&[&str_val as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| self.fresh_int());
            let str_len = ast::Int::from_i64(s.len() as i64);
            self.background_axioms.push(len_result.eq(&str_len));
            Z3Value::Int(str_val)
        }
    }

    // === Variables ===

    fn get_var(&self, name: &str) -> Option<Z3Value> {
        self.vars.get(name).cloned()
    }

    fn set_var(&mut self, name: &str, val: Z3Value) {
        self.vars.insert(name.to_string(), val);
    }

    fn get_or_create_int_var(&mut self, name: &str) -> Z3Value {
        if let Some(val) = self.vars.get(name) {
            return val.clone();
        }
        let v = ast::Int::new_const(name);
        self.vars.insert(name.to_string(), Z3Value::Int(v.clone()));
        Z3Value::Int(v)
    }

    // === Binary operations ===

    fn apply_binop(&mut self, op: &BinOp, lhs: Z3Value, rhs: Z3Value) -> Option<Z3Value> {
        // Delegate to the full encode_binop which handles BV/Real/Int
        // overloads, Neq, In, NotIn, Concat, Range.
        // We need to construct SpExpr wrappers since encode_binop takes them,
        // but the trait already received encoded values. Use a direct match
        // that mirrors the existing logic without re-encoding.
        use crate::encode_binop_policy::{AstBinOpKind, classify_ast_binop};

        match classify_ast_binop(op) {
            AstBinOpKind::Neq => {
                return Some(match (&lhs, &rhs) {
                    (Z3Value::Int(l), Z3Value::Int(r)) => Z3Value::Bool(l.eq(r).not()),
                    (Z3Value::Bool(l), Z3Value::Bool(r)) => Z3Value::Bool(l.eq(r).not()),
                    (Z3Value::Real(l), Z3Value::Real(r)) => Z3Value::Bool(l.eq(r).not()),
                    _ if Self::is_real(&lhs) || Self::is_real(&rhs) => {
                        let l = lhs.as_real(&mut self.fresh_counter);
                        let r = rhs.as_real(&mut self.fresh_counter);
                        Z3Value::Bool(l.eq(&r).not())
                    }
                    _ if Self::is_bool(&lhs) || Self::is_bool(&rhs) => {
                        let l = lhs.as_bool();
                        let r = rhs.as_bool();
                        Z3Value::Bool(l.eq(&r).not())
                    }
                    _ => {
                        let l = lhs.as_int(&mut self.fresh_counter);
                        let r = rhs.as_int(&mut self.fresh_counter);
                        Z3Value::Bool(l.eq(&r).not())
                    }
                });
            }
            AstBinOpKind::In | AstBinOpKind::NotIn => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                let decl = self.make_func(crate::encode_atom_policy::CONTAINS_UF_NAME, 2);
                let result = decl.apply(&[&r as &dyn z3::ast::Ast, &l as &dyn z3::ast::Ast]);
                let contains_int = result.as_int().unwrap_or_else(|| self.fresh_int());
                let zero = ast::Int::from_i64(0);
                let is_member = contains_int.eq(&zero).not();
                return Some(if matches!(op, BinOp::NotIn) {
                    Z3Value::Bool(is_member.not())
                } else {
                    Z3Value::Bool(is_member)
                });
            }
            AstBinOpKind::Concat => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                let result = self.fresh_int();
                let len_decl = self.make_func(crate::encode_atom_policy::FIELD_LEN_UF_NAME, 1);
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
                let zero = ast::Int::from_i64(0);
                self.background_axioms.push(len_l.ge(&zero));
                self.background_axioms.push(len_r.ge(&zero));
                let sum = ast::Int::add(&[&len_l, &len_r]);
                self.background_axioms.push(len_result.eq(&sum));
                self.background_axioms.push(len_result.ge(&zero));
                return Some(Z3Value::Int(result));
            }
            AstBinOpKind::Range => {
                return Some(Z3Value::Int(self.fresh_int()));
            }
            AstBinOpKind::Standard | AstBinOpKind::Unsupported => {}
        }

        Some(match op {
            BinOp::Add => {
                // #851: modular wrap for fixed-width BV operands (not unbounded Int).
                if let (Z3Value::Bv(l, ls), Z3Value::Bv(r, rs)) = (&lhs, &rhs) {
                    Z3Value::Bv(super::BitvectorEncoder::bvadd(l, r), *ls || *rs)
                } else if Self::is_real(&lhs) || Self::is_real(&rhs) {
                    let l = lhs.as_real(&mut self.fresh_counter);
                    let r = rhs.as_real(&mut self.fresh_counter);
                    Z3Value::Real(ast::Real::add(&[&l, &r]))
                } else {
                    let l = lhs.as_int(&mut self.fresh_counter);
                    let r = rhs.as_int(&mut self.fresh_counter);
                    Z3Value::Int(ast::Int::add(&[&l, &r]))
                }
            }
            BinOp::Sub => {
                if let (Z3Value::Bv(l, ls), Z3Value::Bv(r, rs)) = (&lhs, &rhs) {
                    Z3Value::Bv(super::BitvectorEncoder::bvsub(l, r), *ls || *rs)
                } else if Self::is_real(&lhs) || Self::is_real(&rhs) {
                    let l = lhs.as_real(&mut self.fresh_counter);
                    let r = rhs.as_real(&mut self.fresh_counter);
                    Z3Value::Real(ast::Real::sub(&[&l, &r]))
                } else {
                    let l = lhs.as_int(&mut self.fresh_counter);
                    let r = rhs.as_int(&mut self.fresh_counter);
                    Z3Value::Int(ast::Int::sub(&[&l, &r]))
                }
            }
            BinOp::Mul => {
                if let (Z3Value::Bv(l, ls), Z3Value::Bv(r, rs)) = (&lhs, &rhs) {
                    Z3Value::Bv(super::BitvectorEncoder::bvmul(l, r), *ls || *rs)
                } else if Self::is_real(&lhs) || Self::is_real(&rhs) {
                    let l = lhs.as_real(&mut self.fresh_counter);
                    let r = rhs.as_real(&mut self.fresh_counter);
                    Z3Value::Real(ast::Real::mul(&[&l, &r]))
                } else {
                    let l = lhs.as_int(&mut self.fresh_counter);
                    let r = rhs.as_int(&mut self.fresh_counter);
                    Z3Value::Int(ast::Int::mul(&[&l, &r]))
                }
            }
            BinOp::Div => {
                if Self::is_real(&lhs) || Self::is_real(&rhs) {
                    let l = lhs.as_real(&mut self.fresh_counter);
                    let r = rhs.as_real(&mut self.fresh_counter);
                    Z3Value::Real(l.div(&r))
                } else {
                    let l = lhs.as_int(&mut self.fresh_counter);
                    let r = rhs.as_int(&mut self.fresh_counter);
                    Z3Value::Int(l.div(&r))
                }
            }
            BinOp::Mod => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Int(l.rem(&r))
            }
            BinOp::Eq => match (&lhs, &rhs) {
                (Z3Value::Int(l), Z3Value::Int(r)) => Z3Value::Bool(l.eq(r)),
                (Z3Value::Bool(l), Z3Value::Bool(r)) => Z3Value::Bool(l.eq(r)),
                (Z3Value::Real(l), Z3Value::Real(r)) => Z3Value::Bool(l.eq(r)),
                (Z3Value::Bv(l, _), Z3Value::Bv(r, _)) => Z3Value::Bool(l.eq(r)),
                _ if Self::is_real(&lhs) || Self::is_real(&rhs) => {
                    let l = lhs.as_real(&mut self.fresh_counter);
                    let r = rhs.as_real(&mut self.fresh_counter);
                    Z3Value::Bool(l.eq(&r))
                }
                _ if Self::is_bool(&lhs) || Self::is_bool(&rhs) => {
                    let l = lhs.as_bool();
                    let r = rhs.as_bool();
                    Z3Value::Bool(l.eq(&r))
                }
                _ => {
                    let l = lhs.as_int(&mut self.fresh_counter);
                    let r = rhs.as_int(&mut self.fresh_counter);
                    Z3Value::Bool(l.eq(&r))
                }
            },
            BinOp::Lt => {
                if let (Z3Value::Bv(l, ls), Z3Value::Bv(r, rs)) = (&lhs, &rhs) {
                    // Signed types (I8/I32/…) use signed order; U* use unsigned (#MPI).
                    let cmp = if *ls || *rs {
                        super::BitvectorEncoder::bvslt(l, r)
                    } else {
                        super::BitvectorEncoder::bvult(l, r)
                    };
                    Z3Value::Bool(cmp)
                } else if Self::is_real(&lhs) || Self::is_real(&rhs) {
                    let l = lhs.as_real(&mut self.fresh_counter);
                    let r = rhs.as_real(&mut self.fresh_counter);
                    Z3Value::Bool(l.lt(&r))
                } else {
                    let l = lhs.as_int(&mut self.fresh_counter);
                    let r = rhs.as_int(&mut self.fresh_counter);
                    Z3Value::Bool(l.lt(&r))
                }
            }
            BinOp::Lte => {
                if let (Z3Value::Bv(l, ls), Z3Value::Bv(r, rs)) = (&lhs, &rhs) {
                    let cmp = if *ls || *rs {
                        super::BitvectorEncoder::bvsle(l, r)
                    } else {
                        super::BitvectorEncoder::bvule(l, r)
                    };
                    Z3Value::Bool(cmp)
                } else if Self::is_real(&lhs) || Self::is_real(&rhs) {
                    let l = lhs.as_real(&mut self.fresh_counter);
                    let r = rhs.as_real(&mut self.fresh_counter);
                    Z3Value::Bool(l.le(&r))
                } else {
                    let l = lhs.as_int(&mut self.fresh_counter);
                    let r = rhs.as_int(&mut self.fresh_counter);
                    Z3Value::Bool(l.le(&r))
                }
            }
            BinOp::Gt => {
                if let (Z3Value::Bv(l, ls), Z3Value::Bv(r, rs)) = (&lhs, &rhs) {
                    let cmp = if *ls || *rs {
                        super::BitvectorEncoder::bvsgt(l, r)
                    } else {
                        super::BitvectorEncoder::bvugt(l, r)
                    };
                    Z3Value::Bool(cmp)
                } else if Self::is_real(&lhs) || Self::is_real(&rhs) {
                    let l = lhs.as_real(&mut self.fresh_counter);
                    let r = rhs.as_real(&mut self.fresh_counter);
                    Z3Value::Bool(l.gt(&r))
                } else {
                    let l = lhs.as_int(&mut self.fresh_counter);
                    let r = rhs.as_int(&mut self.fresh_counter);
                    Z3Value::Bool(l.gt(&r))
                }
            }
            BinOp::Gte => {
                if let (Z3Value::Bv(l, ls), Z3Value::Bv(r, rs)) = (&lhs, &rhs) {
                    let cmp = if *ls || *rs {
                        super::BitvectorEncoder::bvsge(l, r)
                    } else {
                        super::BitvectorEncoder::bvuge(l, r)
                    };
                    Z3Value::Bool(cmp)
                } else if Self::is_real(&lhs) || Self::is_real(&rhs) {
                    let l = lhs.as_real(&mut self.fresh_counter);
                    let r = rhs.as_real(&mut self.fresh_counter);
                    Z3Value::Bool(l.ge(&r))
                } else {
                    let l = lhs.as_int(&mut self.fresh_counter);
                    let r = rhs.as_int(&mut self.fresh_counter);
                    Z3Value::Bool(l.ge(&r))
                }
            }
            BinOp::And => {
                let l = lhs.as_bool();
                let r = rhs.as_bool();
                Z3Value::Bool(ast::Bool::and(&[&l, &r]))
            }
            BinOp::Or => {
                let l = lhs.as_bool();
                let r = rhs.as_bool();
                Z3Value::Bool(ast::Bool::or(&[&l, &r]))
            }
            BinOp::Implies => {
                let l = lhs.as_bool();
                let r = rhs.as_bool();
                Z3Value::Bool(l.implies(&r))
            }
            // Neq/In/NotIn/Concat/Range handled above.
            _ => return None,
        })
    }

    // === Unary operations ===

    fn make_neg(&mut self, t: Z3Value) -> Z3Value {
        if Self::is_real(&t) {
            let r = t.as_real(&mut self.fresh_counter);
            Z3Value::Real(r.unary_minus())
        } else {
            let i = t.as_int(&mut self.fresh_counter);
            Z3Value::Int(i.unary_minus())
        }
    }

    fn make_not(&mut self, t: Z3Value) -> Z3Value {
        Z3Value::Bool(t.as_bool().not())
    }

    // === Boolean combinators ===

    fn make_and(&mut self, a: Z3Value, b: Z3Value) -> Z3Value {
        let la = a.as_bool();
        let lb = b.as_bool();
        Z3Value::Bool(ast::Bool::and(&[&la, &lb]))
    }

    fn make_or(&mut self, a: Z3Value, b: Z3Value) -> Z3Value {
        let la = a.as_bool();
        let lb = b.as_bool();
        Z3Value::Bool(ast::Bool::or(&[&la, &lb]))
    }

    fn make_implies(&mut self, lhs: Z3Value, rhs: Z3Value) -> Z3Value {
        let l = lhs.as_bool();
        let r = rhs.as_bool();
        Z3Value::Bool(l.implies(&r))
    }

    // === Control flow ===

    fn make_ite(&mut self, cond: Z3Value, then_val: Z3Value, else_val: Z3Value) -> Z3Value {
        let cond_bool = cond.as_bool();
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
    }

    // === Quantifiers ===

    fn make_bound_int_var(&mut self, name: &str) -> Z3Value {
        Z3Value::Int(ast::Int::new_const(name))
    }

    fn make_forall(
        &mut self,
        _var: &str,
        bound: &Z3Value,
        body: Z3Value,
        patterns: Vec<Z3Value>,
    ) -> Z3Value {
        let bound_int = bound.as_int(&mut self.fresh_counter);
        let body_bool = body.as_bool();
        // Convert pattern Z3Values to z3::Pattern objects.
        // Each pattern must be a function application; use the internal
        // Dynamic representation to satisfy z3::Pattern::new.
        let z3_patterns: Vec<z3::Pattern> = patterns
            .iter()
            .map(|p| {
                let pi = p.as_int(&mut self.fresh_counter);
                let dyn_ast = ast::Dynamic::from_ast(&pi);
                z3::Pattern::new(&[&dyn_ast])
            })
            .collect();
        let pat_refs: Vec<&z3::Pattern> = z3_patterns.iter().collect();
        Z3Value::Bool(ast::forall_const(&[&bound_int], &pat_refs, &body_bool))
    }

    fn make_exists(
        &mut self,
        _var: &str,
        bound: &Z3Value,
        body: Z3Value,
        patterns: Vec<Z3Value>,
    ) -> Z3Value {
        let bound_int = bound.as_int(&mut self.fresh_counter);
        let body_bool = body.as_bool();
        let z3_patterns: Vec<z3::Pattern> = patterns
            .iter()
            .map(|p| {
                let pi = p.as_int(&mut self.fresh_counter);
                let dyn_ast = ast::Dynamic::from_ast(&pi);
                z3::Pattern::new(&[&dyn_ast])
            })
            .collect();
        let pat_refs: Vec<&z3::Pattern> = z3_patterns.iter().collect();
        Z3Value::Bool(ast::exists_const(&[&bound_int], &pat_refs, &body_bool))
    }

    fn guard_quantifier_body(
        &mut self,
        domain: &SpExpr,
        bound: &Z3Value,
        body: Z3Value,
        is_forall: bool,
    ) -> Z3Value {
        // Delegate to the concrete Encoder method which handles domain
        // guard construction. Its internal encode_expr calls now route
        // through encode_expr_shared via Encoder::encode_expr.
        let bound_int = bound.as_int(&mut self.fresh_counter);
        let body_bool = body.as_bool();
        Z3Value::Bool(Encoder::guard_quantifier_body(
            self, domain, &bound_int, &body_bool, is_forall,
        ))
    }

    fn infer_quantifier_patterns(
        &mut self,
        body: &SpExpr,
        var_name: &str,
        bound: &Z3Value,
    ) -> Vec<Z3Value> {
        let bound_int = bound.as_int(&mut self.fresh_counter);
        let pats = Encoder::infer_quantifier_patterns(self, body, var_name, &bound_int);
        // The existing method returns Vec<z3::Pattern>; we cannot wrap
        // patterns directly into Z3Value. For now, return empty to match
        // the trait contract (patterns are a backend-local concern; the
        // shared dispatch in encode_expr_shared passes them through).
        // The real quantifier encoding still goes through Encoder::encode_expr
        // which calls the concrete methods directly.
        let _ = pats;
        vec![]
    }

    // === Uninterpreted functions ===

    fn apply_uf_int(&mut self, name: &str, args: &[Z3Value]) -> Z3Value {
        let arg_ints: Vec<ast::Int> = args
            .iter()
            .map(|a| a.as_int(&mut self.fresh_counter))
            .collect();
        let decl = self.make_func(name, arg_ints.len());
        let arg_refs: Vec<&dyn z3::ast::Ast> =
            arg_ints.iter().map(|a| a as &dyn z3::ast::Ast).collect();
        let result = decl.apply(&arg_refs);
        Z3Value::Int(result.as_int().unwrap_or_else(|| self.fresh_int()))
    }

    fn apply_uf_bool(&mut self, name: &str, args: &[Z3Value]) -> Z3Value {
        let arg_ints: Vec<ast::Int> = args
            .iter()
            .map(|a| a.as_int(&mut self.fresh_counter))
            .collect();
        let int_sort = z3::Sort::int();
        let bool_sort = z3::Sort::bool();
        let param_sorts: Vec<&z3::Sort> = (0..arg_ints.len()).map(|_| &int_sort).collect();
        let decl = z3::FuncDecl::new(name, &param_sorts, &bool_sort);
        let arg_refs: Vec<&dyn z3::ast::Ast> =
            arg_ints.iter().map(|a| a as &dyn z3::ast::Ast).collect();
        let result = decl.apply(&arg_refs);
        Z3Value::Bool(result.as_bool().unwrap_or_else(|| self.fresh_bool()))
    }

    // === Sort coercion ===

    fn as_bool(&mut self, term: Z3Value) -> Z3Value {
        Z3Value::Bool(term.as_bool())
    }

    fn as_int(&mut self, term: Z3Value) -> Z3Value {
        Z3Value::Int(term.as_int(&mut self.fresh_counter))
    }

    fn is_real_sort(&self, term: &Z3Value) -> bool {
        Self::is_real(term)
    }

    // === Fresh variables ===

    fn fresh_int(&mut self) -> Z3Value {
        Z3Value::Int(Encoder::fresh_int(self))
    }

    fn fresh_bool(&mut self) -> Z3Value {
        Z3Value::Bool(Encoder::fresh_bool(self))
    }

    // === Axioms ===

    fn push_axiom(&mut self, axiom: Z3Value) {
        self.background_axioms.push(axiom.as_bool());
    }

    // === Trigger management ===

    fn register_trigger_function(&mut self, name: &str) {
        self.trigger_manager.register_function(name.to_string());
    }

    // === Collection operations ===

    fn canonical_length(&mut self, name: &str) -> Z3Value {
        Z3Value::Int(Encoder::canonical_length(self, name))
    }

    // === Compound expression encoding ===

    fn encode_call(
        &mut self,
        func: &SpExpr,
        args: &[SpExpr],
        _encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<Z3Value>,
    ) -> Option<Z3Value> {
        // Delegate to Encoder::encode_call which handles all call
        // classification. Internal recursion in calls.rs goes through
        // self.encode_expr -> encode_expr_shared automatically.
        let func_name = match &func.node {
            assura_ast::Expr::Ident(name) => name.clone(),
            assura_ast::Expr::Field(_, field) => field.clone(),
            _ => crate::encode_atom_policy::call_fresh_name(self.fresh_counter),
        };
        Some(Encoder::encode_call(self, &func_name, args))
    }

    fn encode_method_call(
        &mut self,
        receiver: &SpExpr,
        method: &str,
        args: &[SpExpr],
        _encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<Z3Value>,
    ) -> Option<Z3Value> {
        let mut all_args: Vec<SpExpr> = vec![receiver.clone()];
        all_args.extend(args.iter().cloned());
        Some(Encoder::encode_call(self, method, &all_args))
    }

    fn encode_field(
        &mut self,
        obj: &SpExpr,
        field: &str,
        _encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<Z3Value>,
    ) -> Option<Z3Value> {
        Some(self.encode_field_access(obj, field))
    }

    fn encode_old(
        &mut self,
        inner: &SpExpr,
        encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<Z3Value>,
    ) -> Option<Z3Value> {
        // Inline the Old encoding logic using encode_sub for sub-expression
        // recursion, matching the logic from Encoder::encode_expr Old arm.
        match crate::encode_old_policy::plan_old_access(inner) {
            crate::encode_old_policy::OldAccessPlan::Ident(name) => {
                let old_name = crate::encode_atom_policy::old_snapshot_name(&name);
                let v = self.get_or_create_int(&old_name);
                Some(Z3Value::Int(v))
            }
            crate::encode_old_policy::OldAccessPlan::FlatField(flat) => {
                use crate::encode_field_policy::{
                    FieldValueKind, classify_field_value_kind, flat_leaf_field,
                };
                let old_name = crate::encode_atom_policy::old_snapshot_name(&flat);
                Some(match classify_field_value_kind(flat_leaf_field(&flat)) {
                    FieldValueKind::Bool => Z3Value::Bool(ast::Bool::new_const(old_name.as_str())),
                    FieldValueKind::SizeNonNeg => {
                        let v = self.get_or_create_int(&old_name);
                        let zero = ast::Int::from_i64(0);
                        self.background_axioms.push(v.ge(&zero));
                        Z3Value::Int(v)
                    }
                    FieldValueKind::Int => {
                        let v = self.get_or_create_int(&old_name);
                        Z3Value::Int(v)
                    }
                })
            }
            crate::encode_old_policy::OldAccessPlan::ShallowField { obj, field } => {
                use crate::encode_field_policy::{FieldValueKind, classify_field_value_kind};
                let old_inner = assura_ast::Spanned::no_span(assura_ast::Expr::Old(obj));
                let old_obj = encode_sub(self, &old_inner)?;
                let old_obj_int = old_obj.as_int(&mut self.fresh_counter);
                let func_name = crate::encode_field_policy::field_uf_smtlib_name(&field);
                Some(match classify_field_value_kind(&field) {
                    FieldValueKind::Bool => {
                        let bool_sort = z3::Sort::bool();
                        let int_sort = z3::Sort::int();
                        let decl = z3::FuncDecl::new(func_name.as_str(), &[&int_sort], &bool_sort);
                        let result = decl.apply(&[&old_obj_int as &dyn z3::ast::Ast]);
                        Z3Value::Bool(result.as_bool().unwrap_or_else(|| self.fresh_bool()))
                    }
                    FieldValueKind::SizeNonNeg => {
                        let decl = self.make_func(&func_name, 1);
                        let result = decl.apply(&[&old_obj_int as &dyn z3::ast::Ast]);
                        let len_val = result.as_int().unwrap_or_else(|| Encoder::fresh_int(self));
                        let zero = ast::Int::from_i64(0);
                        self.background_axioms.push(len_val.ge(&zero));
                        Z3Value::Int(len_val)
                    }
                    FieldValueKind::Int => {
                        let decl = self.make_func(&func_name, 1);
                        let result = decl.apply(&[&old_obj_int as &dyn z3::ast::Ast]);
                        Z3Value::Int(result.as_int().unwrap_or_else(|| Encoder::fresh_int(self)))
                    }
                })
            }
            crate::encode_old_policy::OldAccessPlan::MethodCall { receiver, method } => {
                let old_inner = assura_ast::Spanned::no_span(assura_ast::Expr::Old(receiver));
                let old_recv = encode_sub(self, &old_inner)?;
                let old_int = old_recv.as_int(&mut self.fresh_counter);
                let decl = self.make_func(&method, 1);
                let result = decl.apply(&[&old_int as &dyn z3::ast::Ast]);
                Some(Z3Value::Int(
                    result.as_int().unwrap_or_else(|| Encoder::fresh_int(self)),
                ))
            }
            crate::encode_old_policy::OldAccessPlan::Other => encode_sub(self, inner),
        }
    }

    fn encode_match(
        &mut self,
        scrutinee: &SpExpr,
        arms: &[MatchArm],
        encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<Z3Value>,
    ) -> Option<Z3Value> {
        use crate::encode_match_policy::{MatchArmKind, classify_match_arm};

        let scrut = encode_sub(self, scrutinee)?;
        let match_adt = self.register_match_adt_from_arms(arms);
        let default = Z3Value::Int(Encoder::fresh_int(self));
        Some(arms.iter().rev().fold(default, |else_val, arm| {
            let kind = classify_match_arm(arm);
            self.bind_pattern_vars(&arm.pattern, &scrut, match_adt.as_deref());
            let body = encode_sub(self, &arm.body)
                .unwrap_or_else(|| Z3Value::Int(Encoder::fresh_int(self)));
            match kind {
                MatchArmKind::Wildcard | MatchArmKind::BindIdent | MatchArmKind::Tuple => {
                    return body;
                }
                MatchArmKind::CtorTagIdent | MatchArmKind::Literal | MatchArmKind::Constructor => {}
            }
            let cond = match kind {
                MatchArmKind::CtorTagIdent => {
                    let assura_ast::Pattern::Ident(name) = &arm.pattern else {
                        unreachable!("CtorTagIdent requires Ident");
                    };
                    let pat_val = Z3Value::Int(ast::Int::from_i64(self.pattern_hash(name)));
                    match (&scrut, &pat_val) {
                        (Z3Value::Int(a), Z3Value::Int(b)) => a.eq(b),
                        _ => ast::Bool::from_bool(true),
                    }
                }
                MatchArmKind::Literal => {
                    let assura_ast::Pattern::Literal(lit) = &arm.pattern else {
                        unreachable!("Literal kind requires Literal");
                    };
                    // Bool params are Int 0/1 in the encoder; Bool patterns must
                    // compare as 0/1, not fall through to always-true (sort mismatch).
                    if let assura_ast::Literal::Bool(b) = lit {
                        match &scrut {
                            Z3Value::Int(a) => a.eq(ast::Int::from_i64(if *b { 1 } else { 0 })),
                            Z3Value::Bool(a) => a.eq(ast::Bool::from_bool(*b)),
                            _ => ast::Bool::from_bool(true),
                        }
                    } else {
                        let lit_val = self.encode_literal(lit);
                        match (&scrut, &lit_val) {
                            (Z3Value::Int(a), Z3Value::Int(b)) => a.eq(b),
                            (Z3Value::Bool(a), Z3Value::Bool(b)) => a.eq(b),
                            (Z3Value::Real(a), Z3Value::Real(b)) => a.eq(b),
                            (Z3Value::Int(a), Z3Value::Real(b)) => ast::Real::from_int(a).eq(b),
                            (Z3Value::Real(a), Z3Value::Int(b)) => a.eq(ast::Real::from_int(b)),
                            _ => ast::Bool::from_bool(true),
                        }
                    }
                }
                MatchArmKind::Constructor => {
                    let assura_ast::Pattern::Constructor { name, .. } = &arm.pattern else {
                        unreachable!("Constructor kind requires Constructor");
                    };
                    if let (Some(adt_name), Z3Value::Int(s)) = (match_adt.as_deref(), &scrut) {
                        self.adt_is_constructor(adt_name, name, s)
                    } else {
                        ast::Bool::from_bool(true)
                    }
                }
                MatchArmKind::Wildcard | MatchArmKind::BindIdent | MatchArmKind::Tuple => {
                    ast::Bool::from_bool(true)
                }
            };
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
                _ => body,
            }
        }))
    }

    fn encode_let(
        &mut self,
        name: &str,
        value: &SpExpr,
        body: &SpExpr,
        encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<Z3Value>,
    ) -> Option<Z3Value> {
        let val = encode_sub(self, value)?;
        self.vars.insert(name.to_string(), val);
        encode_sub(self, body)
    }

    fn encode_block(
        &mut self,
        body: &[SpExpr],
        encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<Z3Value>,
    ) -> Option<Z3Value> {
        use crate::encode_let_policy::{BlockReducePlan, classify_block};
        match classify_block(body) {
            BlockReducePlan::Empty => Some(Z3Value::Bool(ast::Bool::from_bool(true))),
            BlockReducePlan::LastExpr => {
                let mut result = Z3Value::Int(Encoder::fresh_int(self));
                for expr in body {
                    result = encode_sub(self, expr)
                        .unwrap_or_else(|| Z3Value::Int(Encoder::fresh_int(self)));
                }
                Some(result)
            }
        }
    }

    fn encode_raw(&mut self, tokens: &[String]) -> Option<Z3Value> {
        Some(self.encode_raw_tokens(tokens))
    }

    fn encode_tuple(&mut self, elem_vals: &[Z3Value]) -> Z3Value {
        use crate::encode_tuple_policy::tuple_accessor_uf_name;
        let arity = elem_vals.len();
        let tuple_val = Encoder::fresh_int(self);
        for (i, elem) in elem_vals.iter().enumerate() {
            let accessor_name = tuple_accessor_uf_name(arity, i);
            let accessor = self.make_func(&accessor_name, 1);
            let accessed = accessor
                .apply(&[&tuple_val as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| Encoder::fresh_int(self));
            let elem_int = elem.as_int(&mut self.fresh_counter);
            self.background_axioms.push(accessed.eq(&elem_int));
        }
        Z3Value::Int(tuple_val)
    }

    fn encode_list(&mut self, elem_vals: &[Z3Value]) -> Z3Value {
        use crate::encode_list_policy::list_get_uf_name;
        let list_val = Encoder::fresh_int(self);
        let get_uf = list_get_uf_name();
        for (i, elem) in elem_vals.iter().enumerate() {
            let accessor = self.make_func(get_uf, 2);
            let idx = ast::Int::from_i64(i as i64);
            let accessed = accessor
                .apply(&[&list_val as &dyn z3::ast::Ast, &idx as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| Encoder::fresh_int(self));
            let elem_int = elem.as_int(&mut self.fresh_counter);
            self.background_axioms.push(accessed.eq(&elem_int));
        }
        let len_decl = self.make_func(crate::encode_atom_policy::FIELD_LEN_UF_NAME, 1);
        let len_result = len_decl
            .apply(&[&list_val as &dyn z3::ast::Ast])
            .as_int()
            .unwrap_or_else(|| Encoder::fresh_int(self));
        let expected_len = ast::Int::from_i64(elem_vals.len() as i64);
        self.background_axioms.push(len_result.eq(&expected_len));
        Z3Value::Int(list_val)
    }

    fn encode_index(&mut self, coll: Z3Value, index: Z3Value) -> Z3Value {
        let coll_int = coll.as_int(&mut self.fresh_counter);
        let idx_int = index.as_int(&mut self.fresh_counter);
        let decl = self.make_func(crate::encode_index_policy::index_uf_name(), 2);
        let result = decl.apply(&[
            &coll_int as &dyn z3::ast::Ast,
            &idx_int as &dyn z3::ast::Ast,
        ]);
        Z3Value::Int(result.as_int().unwrap_or_else(|| Encoder::fresh_int(self)))
    }

    fn encode_apply(
        &mut self,
        lemma_name: &str,
        args: &[SpExpr],
        encode_sub: &mut dyn FnMut(&mut Self, &SpExpr) -> Option<Z3Value>,
    ) -> Option<Z3Value> {
        // Encode args for side effects (variable registration).
        for arg in args {
            let _ = encode_sub(self, arg);
        }
        Some(Z3Value::Bool(ast::Bool::new_const(
            crate::encode_atom_policy::apply_lemma_const_name(lemma_name),
        )))
    }
}
