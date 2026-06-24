//! `encode_expr`, raw-token parsing, and binary operators.

use crate::*;
use assura_ast::{BinOp, Literal, SpExpr, Spanned};
use z3::ast;

use super::BitvectorEncoder;
use super::Encoder;
use super::value::{RawOp, Z3Value};

impl Encoder {
    /// Encode an AST expression into a Z3 value.
    pub(crate) fn encode_expr(&mut self, expr: &SpExpr) -> Z3Value {
        match &expr.node {
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
                // Encode as Z3 Real via shared rational parts (encode_atom_policy).
                let (numer, denom) = crate::encode_atom_policy::float_to_rational_parts(s);
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
                    let const_name = crate::encode_atom_policy::string_literal_const_name(s);
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

            // --- Unary operations (plan via encode_binop_policy; Z3 terms here) ---
            Expr::UnaryOp { op, expr: inner } => {
                use crate::encode_binop_policy::{AstUnaryKind, classify_ast_unary};

                let val = self.encode_expr(inner);
                match classify_ast_unary(op) {
                    AstUnaryKind::Neg => {
                        if Self::is_real(&val) {
                            let r = val.as_real(&mut self.fresh_counter);
                            Z3Value::Real(r.unary_minus())
                        } else {
                            let i = val.as_int(&mut self.fresh_counter);
                            Z3Value::Int(i.unary_minus())
                        }
                    }
                    AstUnaryKind::Not => {
                        let b = val.as_bool();
                        Z3Value::Bool(b.not())
                    }
                }
            }

            // --- old(expr): pre-state snapshot (plan via encode_old_policy; Z3 terms here) ---
            // Z3 uses old_snapshot_name for idents (source `result` stays `result__old`, not
            // `__result__old`; CVC5 uses old_ident_name via encode_old_smtlib/cvc5).
            Expr::Old(inner) => match crate::encode_old_policy::plan_old_access(inner.as_ref()) {
                crate::encode_old_policy::OldAccessPlan::Ident(name) => {
                    let old_name = crate::encode_atom_policy::old_snapshot_name(&name);
                    let v = self.get_or_create_int(&old_name);
                    Z3Value::Int(v)
                }
                crate::encode_old_policy::OldAccessPlan::FlatField(flat) => {
                    let old_name = crate::encode_atom_policy::old_snapshot_name(&flat);
                    let v = self.get_or_create_int(&old_name);
                    Z3Value::Int(v)
                }
                crate::encode_old_policy::OldAccessPlan::ShallowField { obj, field } => {
                    // Pre-state shallow field via same FieldValueKind as live encode_field_access.
                    use crate::encode_field_policy::{FieldValueKind, classify_field_value_kind};
                    let old_obj = self.encode_expr(&Spanned::no_span(Expr::Old(obj)));
                    let old_obj_int = old_obj.as_int(&mut self.fresh_counter);
                    let func_name = crate::encode_field_policy::field_uf_smtlib_name(&field);
                    match classify_field_value_kind(&field) {
                        FieldValueKind::Bool => {
                            let bool_sort = z3::Sort::bool();
                            let int_sort = z3::Sort::int();
                            let decl =
                                z3::FuncDecl::new(func_name.as_str(), &[&int_sort], &bool_sort);
                            let result = decl.apply(&[&old_obj_int as &dyn z3::ast::Ast]);
                            Z3Value::Bool(result.as_bool().unwrap_or_else(|| self.fresh_bool()))
                        }
                        FieldValueKind::SizeNonNeg => {
                            let decl = self.make_func(&func_name, 1);
                            let result = decl.apply(&[&old_obj_int as &dyn z3::ast::Ast]);
                            let len_val = result.as_int().unwrap_or_else(|| self.fresh_int());
                            let zero = ast::Int::from_i64(0);
                            self.background_axioms.push(len_val.ge(&zero));
                            Z3Value::Int(len_val)
                        }
                        FieldValueKind::Int => {
                            let decl = self.make_func(&func_name, 1);
                            let result = decl.apply(&[&old_obj_int as &dyn z3::ast::Ast]);
                            Z3Value::Int(result.as_int().unwrap_or_else(|| self.fresh_int()))
                        }
                    }
                }
                crate::encode_old_policy::OldAccessPlan::MethodCall { receiver, method } => {
                    let old_recv = self.encode_expr(&Spanned::no_span(Expr::Old(receiver)));
                    let old_int = old_recv.as_int(&mut self.fresh_counter);
                    let decl = self.make_func(&method, 1);
                    let result = decl.apply(&[&old_int as &dyn z3::ast::Ast]);
                    Z3Value::Int(result.as_int().unwrap_or_else(|| self.fresh_int()))
                }
                crate::encode_old_policy::OldAccessPlan::Other => self.encode_expr(inner),
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

            // --- If-then-else (plan via encode_if_policy; Z3 terms here) ---
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                use crate::encode_if_policy::{IfEncodePlan, plan_if_encode};

                let cond_val = self.encode_expr(cond);
                let cond_bool = cond_val.as_bool();
                let then_val = self.encode_expr(then_branch);

                match plan_if_encode(else_branch.is_some()) {
                    IfEncodePlan::Ite => {
                        let else_br = else_branch.as_ref().expect("Ite plan requires else_branch");
                        let else_val = self.encode_expr(else_br);
                        match (&then_val, &else_val) {
                            (Z3Value::Int(t), Z3Value::Int(e)) => Z3Value::Int(cond_bool.ite(t, e)),
                            (Z3Value::Bool(t), Z3Value::Bool(e)) => {
                                Z3Value::Bool(cond_bool.ite(t, e))
                            }
                            (Z3Value::Real(t), Z3Value::Real(e)) => {
                                Z3Value::Real(cond_bool.ite(t, e))
                            }
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
                    IfEncodePlan::ImpliesThenOnly => {
                        // No else: `if P then Q` = `P => Q` (parity with CVC5 shell/native).
                        let then_bool = then_val.as_bool();
                        Z3Value::Bool(cond_bool.implies(&then_bool))
                    }
                }
            }

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
                Z3Value::Bool(ast::Bool::new_const(
                    crate::encode_atom_policy::apply_lemma_const_name(lemma_name),
                ))
            }

            // --- Match: encode as ITE chain over arm bodies ---
            // Arm kinds via encode_match_policy (parity with CVC5 shell/native).
            Expr::Match { scrutinee, arms } => {
                use crate::encode_match_policy::{MatchArmKind, classify_match_arm};

                let scrut = self.encode_expr(scrutinee);
                let match_adt = self.register_match_adt_from_arms(arms);
                // Build an if-then-else chain: if scrut == pattern1 then body1
                // else if scrut == pattern2 then body2 ... else default
                let default = Z3Value::Int(self.fresh_int());
                arms.iter().rev().fold(default, |else_val, arm| {
                    let kind = classify_match_arm(arm);
                    // Bind pattern variables before encoding the body
                    self.bind_pattern_vars(&arm.pattern, &scrut, match_adt.as_deref());
                    let body = self.encode_expr(&arm.body);
                    // Wildcard / bind-ident / tuple: arm always matches (no scrutinee test).
                    match kind {
                        MatchArmKind::Wildcard | MatchArmKind::BindIdent | MatchArmKind::Tuple => {
                            return body;
                        }
                        MatchArmKind::CtorTagIdent
                        | MatchArmKind::Literal
                        | MatchArmKind::Constructor => {}
                    }
                    let cond = match kind {
                        MatchArmKind::CtorTagIdent => {
                            let assura_ast::Pattern::Ident(name) = &arm.pattern else {
                                unreachable!("CtorTagIdent requires Ident");
                            };
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
                        MatchArmKind::Literal => {
                            let assura_ast::Pattern::Literal(lit) = &arm.pattern else {
                                unreachable!("Literal kind requires Literal");
                            };
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
                        MatchArmKind::Constructor => {
                            let assura_ast::Pattern::Constructor { name, .. } = &arm.pattern else {
                                unreachable!("Constructor kind requires Constructor");
                            };
                            if let (Some(adt_name), Z3Value::Int(s)) =
                                (match_adt.as_deref(), &scrut)
                            {
                                self.adt_is_constructor(adt_name, name, s)
                            } else {
                                ast::Bool::from_bool(true)
                            }
                        }
                        MatchArmKind::Wildcard | MatchArmKind::BindIdent | MatchArmKind::Tuple => {
                            ast::Bool::from_bool(true)
                        }
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
                let mut all_args: Vec<SpExpr> = vec![receiver.as_ref().clone()];
                all_args.extend(args.iter().cloned());
                self.encode_call(method, &all_args)
            }

            // --- Function call: uninterpreted function ---
            Expr::Call { func, args } => {
                let func_name = match &func.as_ref().node {
                    Expr::Ident(name) => name.clone(),
                    Expr::Field(_, field) => field.clone(),
                    _ => crate::encode_atom_policy::call_fresh_name(self.fresh_counter),
                };
                self.encode_call(&func_name, args)
            }

            // --- Index: uninterpreted function __index(coll, idx) ---
            Expr::Index { expr, index } => self.encode_index(expr, index),

            // --- Tuple: model as an Int with element-access axioms ---
            // Plan via encode_tuple_policy (parity with CVC5 FreshWithAccessors).
            Expr::Tuple(elems) => {
                use crate::encode_tuple_policy::{
                    TupleEncodePlan, plan_tuple_encode, tuple_accessor_uf_name,
                };

                let arity = elems.len();
                let plan = plan_tuple_encode(arity, false);
                debug_assert!(matches!(
                    plan,
                    TupleEncodePlan::FreshWithAccessors { arity: a } if a == arity
                ));
                let tuple_val = self.fresh_int();
                for (i, elem) in elems.iter().enumerate() {
                    let elem_val = self.encode_expr(elem);
                    // Assert: __tuple_{arity}_{i}(tuple) == elem_val
                    let accessor_name = tuple_accessor_uf_name(arity, i);
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
            // Plan via encode_list_policy (parity with CVC5 FreshWithElements).
            Expr::List(elems) => {
                use crate::encode_list_policy::{
                    ListEncodePlan, list_get_uf_name, plan_list_encode,
                };

                let plan = plan_list_encode(elems.len(), false);
                debug_assert!(matches!(
                    plan,
                    ListEncodePlan::FreshWithElements { len } if len == elems.len()
                ));
                let list_val = self.fresh_int();
                let get_uf = list_get_uf_name();
                for (i, elem) in elems.iter().enumerate() {
                    let elem_val = self.encode_expr(elem);
                    // Assert: __list_get(list, i) == elem_val
                    let accessor = self.make_func(get_uf, 2);
                    let idx = ast::Int::from_i64(i as i64);
                    let accessed = accessor
                        .apply(&[&list_val as &dyn z3::ast::Ast, &idx as &dyn z3::ast::Ast])
                        .as_int()
                        .unwrap_or_else(|| self.fresh_int());
                    let elem_int = elem_val.as_int(&mut self.fresh_counter);
                    self.background_axioms.push(accessed.eq(&elem_int));
                }
                // Assert length
                let len_decl = self.make_func(crate::encode_atom_policy::FIELD_LEN_UF_NAME, 1);
                let len_result = len_decl
                    .apply(&[&list_val as &dyn z3::ast::Ast])
                    .as_int()
                    .unwrap_or_else(|| self.fresh_int());
                let expected_len = ast::Int::from_i64(elems.len() as i64);
                self.background_axioms.push(len_result.eq(&expected_len));
                Z3Value::Int(list_val)
            }

            // --- Block: encode all body expressions, return last ---
            // Shape via encode_let_policy (parity with CVC5 encode_block_cvc5).
            Expr::Block(body) => {
                use crate::encode_let_policy::{BlockReducePlan, classify_block};

                match classify_block(body) {
                    BlockReducePlan::Empty => Z3Value::Int(self.fresh_int()),
                    BlockReducePlan::LastExpr => {
                        let mut result = Z3Value::Int(self.fresh_int());
                        for expr in body {
                            result = self.encode_expr(expr);
                        }
                        result
                    }
                }
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
    pub(crate) fn encode_raw_tokens(&mut self, tokens: &[String]) -> Z3Value {
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
    pub(crate) fn parse_raw_expr(&mut self, tokens: &[String], min_prec: u8) -> (Z3Value, usize) {
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
    pub(crate) fn parse_raw_atom(&mut self, tokens: &[String], start: usize) -> (Z3Value, usize) {
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
            let v = self.get_or_create_int(crate::encode_atom_policy::RESULT_VAR_NAME);
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
            // Pre-state shapes via encode_old_policy (parity with CVC5 raw SMT-LIB).
            match crate::encode_old_policy::classify_raw_old_inner(inner_tokens) {
                crate::encode_old_policy::RawOldPlan::Ident(name) => {
                    let old_name = crate::encode_old_policy::raw_old_ident_snapshot_name(&name);
                    let v = self.get_or_create_int(&old_name);
                    return (Z3Value::Int(v), end);
                }
                crate::encode_old_policy::RawOldPlan::ShallowField { base, field } => {
                    let old_name = crate::encode_old_policy::raw_old_ident_snapshot_name(&base);
                    let old_var = self.get_or_create_int(&old_name);
                    let func_name = crate::encode_field_policy::field_uf_smtlib_name(&field);
                    let decl = self.make_func(&func_name, 1);
                    let result = decl.apply(&[&old_var as &dyn z3::ast::Ast]);
                    let val = result.as_int().unwrap_or_else(|| self.fresh_int());
                    return (Z3Value::Int(val), end);
                }
                crate::encode_old_policy::RawOldPlan::Complex => {
                    let (val, _) = self.parse_raw_expr(inner_tokens, 0);
                    return (val, end);
                }
            }
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

                // Build Z3 quantifier (no e-matching patterns: Pattern::new panics
                // on bare Int bound vars like service invariant quantifiers).
                let bound_ref = &bound;
                let q = if is_forall {
                    z3::ast::forall_const(&[bound_ref as &dyn z3::ast::Ast], &[], &body_bool)
                } else {
                    z3::ast::exists_const(&[bound_ref as &dyn z3::ast::Ast], &[], &body_bool)
                };
                return (Z3Value::Bool(q), tokens.len());
            }
        }

        // --- Integer literal ---
        if let Ok(n) = tok.parse::<i64>() {
            return (Z3Value::Int(ast::Int::from_i64(n)), start + 1);
        }

        // --- Float literal ---
        if tok.contains('.') && tok.parse::<f64>().is_ok() {
            let (numer, denom) = crate::encode_atom_policy::float_to_rational_parts(tok);
            return (
                Z3Value::Real(ast::Real::from_rational(numer, denom)),
                start + 1,
            );
        }

        // #200: Skip taint/ghost/region/validate keywords in raw tokens;
        // they are specification-level annotations, not Z3 variables.
        if crate::encode_raw_ops_policy::is_raw_spec_skip_keyword(tok) {
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
            let ts_var_name = crate::encode_atom_policy::typestate_var_name(&name);
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

            // Dispatch via classify_encode_call for all known call kinds
            // (parity with CVC5 encode_uf_call_cvc5 / shell encode_call_smtlib).
            use crate::encode_call_policy::{EncodeCallKind, classify_encode_call};
            let call_kind = classify_encode_call(func_name, arg_vals.len());

            if matches!(call_kind, EncodeCallKind::Abs) {
                let x = &arg_vals[0];
                let zero = ast::Int::from_i64(0);
                let neg_x = x.unary_minus();
                let cond = x.ge(&zero);
                return (Z3Value::Int(cond.ite(x, &neg_x)), end);
            }
            if matches!(call_kind, EncodeCallKind::MinMax) {
                let (a, b) = (&arg_vals[0], &arg_vals[1]);
                let result = if func_name == "min" {
                    a.le(b).ite(a, b)
                } else {
                    a.ge(b).ite(a, b)
                };
                return (Z3Value::Int(result), end);
            }
            if matches!(call_kind, EncodeCallKind::BoolReturningUf) {
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

            if matches!(call_kind, EncodeCallKind::SizeFieldUf) {
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
    pub(crate) fn apply_raw_op(&mut self, op: RawOp, lhs: Z3Value, rhs: Z3Value) -> Z3Value {
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
    pub(crate) fn is_real(v: &Z3Value) -> bool {
        matches!(v, Z3Value::Real(_))
    }

    pub(crate) fn is_bv(v: &Z3Value) -> bool {
        matches!(v, Z3Value::Bv(_))
    }

    pub(crate) fn bv_width(v: &Z3Value) -> u32 {
        match v {
            Z3Value::Bv(b) => b.get_size(),
            _ => 32,
        }
    }

    /// Check if a BinOp is a comparison operator.
    pub(crate) fn is_comparison(op: &BinOp) -> bool {
        matches!(
            op,
            BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte | BinOp::Eq | BinOp::Neq
        )
    }

    /// Encode a binary operation.
    ///
    /// Special forms (`Neq`/`Range`/`In`/`NotIn`/`Concat`) align with
    /// [`crate::encode_binop_policy::AstBinOpKind`]; arithmetic/logic/compare
    /// remain full `BinOp` match for BV/Real/Int overloads (Z3-only).
    pub(crate) fn encode_binop(&mut self, lhs: &SpExpr, op: &BinOp, rhs: &SpExpr) -> Z3Value {
        use crate::encode_binop_policy::{AstBinOpKind, classify_ast_binop};

        // Comparison chaining: a < b < c  =>  (a < b) && (b < c)
        // The parser produces BinOp(BinOp(a, <, b), <, c). We detect
        // when a comparison's LHS is itself a comparison, extract the
        // shared middle operand, and encode as conjunction.
        if Self::is_comparison(op)
            && let Expr::BinOp {
                lhs: inner_lhs,
                op: inner_op,
                rhs: inner_rhs,
            } = &lhs.node
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

        // Policy-classified special forms first (parity with CVC5 encode_ast_binop_cvc5).
        match classify_ast_binop(op) {
            AstBinOpKind::Neq => {
                return match (&lv, &rv) {
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
                };
            }
            AstBinOpKind::In | AstBinOpKind::NotIn => {
                let l = lv.as_int(&mut self.fresh_counter);
                let r = rv.as_int(&mut self.fresh_counter);
                let decl = self.make_func(crate::encode_atom_policy::CONTAINS_UF_NAME, 2);
                let result = decl.apply(&[&r as &dyn z3::ast::Ast, &l as &dyn z3::ast::Ast]);
                let contains_int = result.as_int().unwrap_or_else(|| self.fresh_int());
                let zero = ast::Int::from_i64(0);
                let is_member = contains_int.eq(&zero).not();
                return if matches!(op, BinOp::NotIn) {
                    Z3Value::Bool(is_member.not())
                } else {
                    Z3Value::Bool(is_member)
                };
            }
            AstBinOpKind::Concat => {
                let l = lv.as_int(&mut self.fresh_counter);
                let r = rv.as_int(&mut self.fresh_counter);
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
                return Z3Value::Int(result);
            }
            AstBinOpKind::Range => {
                return Z3Value::Int(self.fresh_int());
            }
            AstBinOpKind::Standard | AstBinOpKind::Unsupported => {}
        }

        match op {
            // --- Arithmetic: produce Int or Real depending on operands ---
            BinOp::Add => {
                if Self::is_bv(&lv) || Self::is_bv(&rv) {
                    let width = Self::bv_width(if Self::is_bv(&lv) { &lv } else { &rv });
                    let l = lv.as_bv(width);
                    let r = rv.as_bv(width);
                    return Z3Value::Bv(BitvectorEncoder::bvadd(&l, &r));
                }
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
                if Self::is_bv(&lv) || Self::is_bv(&rv) {
                    let width = Self::bv_width(if Self::is_bv(&lv) { &lv } else { &rv });
                    let l = lv.as_bv(width);
                    let r = rv.as_bv(width);
                    return Z3Value::Bv(BitvectorEncoder::bvsub(&l, &r));
                }
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
                if Self::is_bv(&lv) || Self::is_bv(&rv) {
                    let width = Self::bv_width(if Self::is_bv(&lv) { &lv } else { &rv });
                    let l = lv.as_bv(width);
                    let r = rv.as_bv(width);
                    return Z3Value::Bv(BitvectorEncoder::bvmul(&l, &r));
                }
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
                (Z3Value::Bv(l), Z3Value::Bv(r)) => Z3Value::Bool(l.eq(r)),
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
            // Neq / In / NotIn / Concat / Range handled via AstBinOpKind above.
            BinOp::Neq | BinOp::In | BinOp::NotIn | BinOp::Concat | BinOp::Range => {
                unreachable!("special binop forms handled via classify_ast_binop")
            }
            BinOp::Lt => {
                if Self::is_bv(&lv) || Self::is_bv(&rv) {
                    let width = Self::bv_width(if Self::is_bv(&lv) { &lv } else { &rv });
                    let l = lv.as_bv(width);
                    let r = rv.as_bv(width);
                    return Z3Value::Bool(BitvectorEncoder::bvult(&l, &r));
                }
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
        }
    }
}
