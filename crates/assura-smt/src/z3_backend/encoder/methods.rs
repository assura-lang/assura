//! `encode_expr`, raw-token parsing, and binary operators.

use assura_ast::SpExpr;
use z3::ast;

use super::Encoder;
use super::value::Z3Value;
use crate::encode_raw_ops_policy::RawBinOp;

impl Encoder {
    /// Encode an AST expression into a Z3 value.
    ///
    /// Delegates to [`encode_expr_shared`] which handles AST dispatch via
    /// the [`EncodeTerm`] trait. All expression encoding goes through the
    /// shared path; backend-specific term construction is in
    /// `encode_term_impl.rs`.
    pub(crate) fn encode_expr(&mut self, expr: &SpExpr) -> Z3Value {
        crate::encode_term::encode_expr_shared(self, expr)
            .unwrap_or_else(|| Z3Value::Int(self.fresh_int()))
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
            let (op_prec, op_kind) =
                match crate::encode_raw_ops_policy::raw_op_info(tokens[pos].as_str()) {
                    Some(pair) => pair,
                    None => break,
                };

            if op_prec < min_prec {
                break;
            }

            pos += 1; // consume operator

            let (rhs, next_pos) = self.parse_raw_expr(&tokens[pos..], op_prec + 1);
            // Adjust pos relative to original tokens
            pos += next_pos;

            // Comparison chaining: `a < b < c` => `(a < b) AND (b < c)` (fixes #460,
            // parity with CVC5 raw parser).
            if crate::encode_raw_ops_policy::raw_op_is_comparison(op_kind)
                && pos < tokens.len()
                && let Some((next_prec, next_op)) =
                    crate::encode_raw_ops_policy::raw_op_info(tokens[pos].as_str())
                && crate::encode_raw_ops_policy::raw_op_is_comparison(next_op)
                && next_prec >= min_prec
            {
                let left_cmp = self.apply_raw_op(op_kind, lhs, rhs.clone());
                pos += 1;
                let (rhs2, next_pos2) = self.parse_raw_expr(&tokens[pos..], next_prec + 1);
                pos += next_pos2;
                let right_cmp = self.apply_raw_op(next_op, rhs, rhs2);
                let l = left_cmp.as_bool();
                let r = right_cmp.as_bool();
                lhs = Z3Value::Bool(ast::Bool::and(&[&l, &r]));
                continue;
            }

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

            // Extract the base function name (last segment after collapsed dots).
            let func_name = crate::encode_atom_policy::extract_raw_base_name(&name);

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
            if matches!(call_kind, EncodeCallKind::Clamp) && arg_vals.len() == 3 {
                let (x, lo, hi) = (&arg_vals[0], &arg_vals[1], &arg_vals[2]);
                let raised = x.ge(lo).ite(x, lo);
                return (Z3Value::Int(raised.le(hi).ite(&raised, hi)), end);
            }
            if matches!(call_kind, EncodeCallKind::Signum) && arg_vals.len() == 1 {
                let x = &arg_vals[0];
                let one = ast::Int::from_i64(1);
                let neg1 = ast::Int::from_i64(-1);
                let capped = x.le(&one).ite(x, &one);
                return (Z3Value::Int(capped.ge(&neg1).ite(&capped, &neg1)), end);
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

    /// Apply a raw binary operation using the shared [`RawBinOp`] policy enum.
    pub(crate) fn apply_raw_op(&mut self, op: RawBinOp, lhs: Z3Value, rhs: Z3Value) -> Z3Value {
        match op {
            RawBinOp::Add => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Int(ast::Int::add(&[&l, &r]))
            }
            RawBinOp::Sub => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Int(ast::Int::sub(&[&l, &r]))
            }
            RawBinOp::Mul => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Int(ast::Int::mul(&[&l, &r]))
            }
            RawBinOp::Div => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Int(l.div(&r))
            }
            RawBinOp::Mod => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Int(l.rem(&r))
            }
            RawBinOp::Eq => match (&lhs, &rhs) {
                (Z3Value::Bool(l), Z3Value::Bool(r)) => Z3Value::Bool(l.eq(r)),
                (Z3Value::Str(l), Z3Value::Str(r)) => Z3Value::Bool(l.eq(r)),
                // Mixed Bool/Int: coerce to Bool (#511).
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
            RawBinOp::Neq => match (&lhs, &rhs) {
                (Z3Value::Bool(l), Z3Value::Bool(r)) => Z3Value::Bool(l.eq(r).not()),
                (Z3Value::Str(l), Z3Value::Str(r)) => Z3Value::Bool(l.eq(r).not()),
                // Mixed Bool/Int: coerce to Bool (#511).
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
            },
            RawBinOp::Lt => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Bool(l.lt(&r))
            }
            RawBinOp::Leq => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Bool(l.le(&r))
            }
            RawBinOp::Gt => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Bool(l.gt(&r))
            }
            RawBinOp::Geq => {
                let l = lhs.as_int(&mut self.fresh_counter);
                let r = rhs.as_int(&mut self.fresh_counter);
                Z3Value::Bool(l.ge(&r))
            }
            RawBinOp::And => {
                let l = lhs.as_bool();
                let r = rhs.as_bool();
                Z3Value::Bool(ast::Bool::and(&[&l, &r]))
            }
            RawBinOp::Or => {
                let l = lhs.as_bool();
                let r = rhs.as_bool();
                Z3Value::Bool(ast::Bool::or(&[&l, &r]))
            }
            RawBinOp::Implies => {
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

    pub(crate) fn is_bool(v: &Z3Value) -> bool {
        matches!(v, Z3Value::Bool(_))
    }
}
