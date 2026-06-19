//! Native CVC5 term encoding (feature = "cvc5-verify").
//!
//! Expression-to-term translation extracted from `cvc5_backend.rs`.

use std::collections::HashMap;

use assura_parser::ast::{BinOp, Clause, Expr, Literal, Pattern, UnaryOp};

use crate::cvc5_binop_encode::{encode_ast_binop_cvc5, encode_ast_unary_cvc5};
use crate::cvc5_builtins::{is_bool_field, is_size_field, pattern_hash_name};
use crate::cvc5_common::{float_to_rational_parts, sanitize_smtlib_name, smtlib_result_name};
use crate::cvc5_encoder_state::{canonical_length_cvc5, field_len_fn_cvc5};
use crate::cvc5_field_access::{FieldAccessPlan, encode_shallow_field_cvc5, plan_field_access};
use crate::cvc5_if_encode::encode_if_cvc5;
use crate::cvc5_index_access::encode_index_access_cvc5;
use crate::cvc5_ir_native::apply_ir_body_constraints_cvc5;
use crate::cvc5_list_encode::encode_list_cvc5;
use crate::cvc5_match_encode::encode_match_cvc5;
use crate::cvc5_old_access::encode_old_cvc5;
use crate::cvc5_quantifier_encode::encode_ast_quantifier_cvc5;

pub(crate) use crate::cvc5_encoder_state::{Cvc5EncoderState, default_cvc5_encoder_state};
use crate::cvc5_native_builtins::{
    encode_known_builtin_cvc5, encode_uf_call_cvc5, field_len_of_receiver_cvc5,
};
use crate::cvc5_raw_ops::{
    apply_raw_op_cvc5, comma_chunk_ranges, find_matching_delim, is_raw_spec_skip_keyword,
    parse_raw_quantifier_slice, raw_op_info, raw_op_is_comparison,
};

pub(crate) use crate::cvc5_quantifier_encode::infer_quantifier_patterns_cvc5;
use crate::cvc5_tuple_encode::encode_tuple_cvc5;

// -------------------------------------------------------------------------
// Havoc+assume encoding (#267)
// -------------------------------------------------------------------------

#[cfg(feature = "cvc5-verify")]
fn get_or_create_int_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    name: &str,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
) -> cvc5::Term<'a> {
    vars.entry(name.to_string())
        .or_insert_with(|| tm.mk_const(tm.integer_sort(), name))
        .clone()
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn apply_havoc_assume_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    requires: &[&Clause],
    ensures: &[&Clause],
    return_ty: &[String],
    param_names: &[String],
    ir: Option<&crate::ir::IrFunction>,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) {
    use crate::havoc_assume::{infer_length_identity_links, is_collection_return};

    if is_collection_return(return_ty) {
        let len = canonical_length_cvc5(tm, "result", vars, state);
        let zero = tm.mk_integer(0);
        state.axioms.push(tm.mk_term(cvc5::Kind::Geq, &[len, zero]));
    }

    for (result, input) in infer_length_identity_links(requires, ensures) {
        let len_result = canonical_length_cvc5(tm, &result, vars, state);
        let len_input = canonical_length_cvc5(tm, &input, vars, state);
        state
            .axioms
            .push(tm.mk_term(cvc5::Kind::Leq, &[len_result, len_input]));
    }

    if let Some(func) = ir {
        apply_ir_body_constraints_cvc5(tm, func, param_names, vars, state);
    }
}

#[cfg(feature = "cvc5-verify")]
fn encode_length_receiver_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    receiver: &Expr,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    if let Expr::Ident(name) = receiver {
        return Some(canonical_length_cvc5(tm, name, vars, state));
    }
    let recv_val = encode_expr_cvc5(tm, receiver, vars, state)?;
    Some(field_len_of_receiver_cvc5(tm, &recv_val, state))
}

/// Encode an AST expression as a CVC5 Term using the native API.
///
/// `state` collects background axioms and tracks string constants
/// so that `check_clause_cvc5_native` can assert them before check_sat.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_expr_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    expr: &Expr,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    match expr {
        Expr::Literal(Literal::Int(n)) => {
            let val: i64 = n.parse().ok()?;
            Some(tm.mk_integer(val))
        }
        Expr::Literal(Literal::Bool(b)) => Some(tm.mk_boolean(*b)),
        Expr::Literal(Literal::Float(f_str)) => {
            let (numer, denom) = float_to_rational_parts(f_str);
            Some(tm.mk_real_from_rational(numer, denom))
        }
        Expr::Literal(Literal::Str(s)) => {
            if state.use_string_theory {
                // Native CVC5 string theory: use string_sort and mk_string.
                // CVC5 handles equality, length, and distinctness natively.
                let str_val = tm.mk_string(s, false);
                // Background axiom: length is known at compile time
                let len = tm.mk_term(cvc5::Kind::StringLength, &[str_val.clone()]);
                let expected_len = tm.mk_integer(s.len() as i64);
                let len_eq = tm.mk_term(cvc5::Kind::Equal, &[len, expected_len]);
                state.axioms.push(len_eq);
                Some(str_val)
            } else {
                // Integer encoding (default): named integer constant matching Z3 pattern
                let const_name = format!("__str_{s}");
                let str_val = tm.mk_const(tm.integer_sort(), &const_name);
                // Pairwise distinctness from previously seen string constants
                if !state.string_constants.contains(&const_name) {
                    for prev in &state.string_constants {
                        let prev_val = tm.mk_const(tm.integer_sort(), prev);
                        let eq = tm.mk_term(cvc5::Kind::Equal, &[str_val.clone(), prev_val]);
                        let neq = tm.mk_term(cvc5::Kind::Not, &[eq]);
                        state.axioms.push(neq);
                    }
                    state.string_constants.push(const_name);
                }
                // String length axiom: len("hello") == 5
                let len_name = "__field_len";
                let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let len_func = tm.mk_const(len_sort, len_name);
                let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, str_val.clone()]);
                let str_len = tm.mk_integer(s.len() as i64);
                let len_eq = tm.mk_term(cvc5::Kind::Equal, &[len_result, str_len]);
                state.axioms.push(len_eq);
                Some(str_val)
            }
        }
        Expr::Ident(name) => {
            let key = if name == "result" {
                smtlib_result_name().to_string()
            } else {
                sanitize_smtlib_name(name)
            };
            vars.get(&key)
                .cloned()
                .or_else(|| Some(tm.mk_const(tm.integer_sort(), &key)))
        }
        Expr::BinOp { op, lhs, rhs } => {
            let l = encode_expr_cvc5(tm, lhs, vars, state)?;
            let r = encode_expr_cvc5(tm, rhs, vars, state)?;
            encode_ast_binop_cvc5(tm, op, l, r, state)
        }
        Expr::UnaryOp { op, expr: inner } => {
            let e = encode_expr_cvc5(tm, inner, vars, state)?;
            Some(encode_ast_unary_cvc5(tm, op, e))
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = encode_expr_cvc5(tm, cond, vars, state)?;
            let t = encode_expr_cvc5(tm, then_branch, vars, state)?;
            let e = else_branch
                .as_ref()
                .and_then(|eb| encode_expr_cvc5(tm, eb, vars, state));
            Some(encode_if_cvc5(tm, c, t, e))
        }
        Expr::Forall { var, domain, body } => {
            encode_ast_quantifier_cvc5(tm, true, var, domain, body, vars, state, |e, v, s| {
                encode_expr_cvc5(tm, e, v, s)
            })
        }
        Expr::Exists { var, domain, body } => {
            encode_ast_quantifier_cvc5(tm, false, var, domain, body, vars, state, |e, v, s| {
                encode_expr_cvc5(tm, e, v, s)
            })
        }
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = func.as_ref() {
                let f_name = sanitize_smtlib_name(name);
                if args.is_empty() {
                    return vars
                        .get(&f_name)
                        .cloned()
                        .or_else(|| Some(tm.mk_const(tm.integer_sort(), &f_name)));
                }
                let encoded_args: Option<Vec<cvc5::Term>> = args
                    .iter()
                    .map(|a| encode_expr_cvc5(tm, a, vars, state))
                    .collect();
                let encoded_args = encoded_args?;
                if let Some(term) =
                    encode_known_builtin_cvc5(tm, f_name.as_str(), &encoded_args, state)
                {
                    return Some(term);
                }
                encode_uf_call_cvc5(tm, &f_name, &encoded_args, state)
            } else {
                None
            }
        }
        // old(expr): add __old suffix for Ident, recurse for Field/MethodCall
        Expr::Old(inner) => encode_old_cvc5(tm, inner.as_ref(), vars, state, |e, v, s| {
            encode_expr_cvc5(tm, e, v, s)
        }),
        Expr::Paren(inner) | Expr::Ghost(inner) => encode_expr_cvc5(tm, inner, vars, state),
        Expr::Cast { expr: inner, .. } => encode_expr_cvc5(tm, inner, vars, state),
        Expr::Let {
            name, value, body, ..
        } => {
            let v = encode_expr_cvc5(tm, value, vars, state)?;
            let mut local_vars = vars.clone();
            local_vars.insert(sanitize_smtlib_name(name), v);
            encode_expr_cvc5(tm, body, &mut local_vars, state)
        }
        Expr::Match {
            scrutinee, arms, ..
        } => encode_match_cvc5(tm, scrutinee, arms, vars, state, |e, v, s| {
            encode_expr_cvc5(tm, e, v, s)
        }),
        // Field access: flatten deep chains or self-rooted, else UF
        Expr::Field(obj, field) => {
            if matches!(field.as_str(), "len" | "length")
                && let Expr::Ident(name) = obj.as_ref()
            {
                return Some(canonical_length_cvc5(tm, name, vars, state));
            }

            match plan_field_access(obj.as_ref(), field) {
                FieldAccessPlan::Flatten(flat_name) => {
                    if is_bool_field(field) {
                        return Some(tm.mk_const(tm.boolean_sort(), &flat_name));
                    }
                    if is_size_field(field) {
                        let v = get_or_create_int_cvc5(tm, &flat_name, vars);
                        let zero = tm.mk_integer(0);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Geq, &[v.clone(), zero]));
                        return Some(v);
                    }
                    Some(get_or_create_int_cvc5(tm, &flat_name, vars))
                }
                FieldAccessPlan::ShallowUf { field: f } => {
                    let obj_val = encode_expr_cvc5(tm, obj, vars, state)?;
                    Some(encode_shallow_field_cvc5(
                        tm,
                        &f,
                        obj_val,
                        &mut state.axioms,
                        state.use_string_theory,
                    ))
                }
            }
        }
        // Index: UF __index(collection, index) with bounds axioms
        Expr::Index { expr: coll, index } => {
            let coll_val = encode_expr_cvc5(tm, coll, vars, state)?;
            let idx_val = encode_expr_cvc5(tm, index, vars, state)?;
            Some(encode_index_access_cvc5(
                tm,
                coll_val,
                idx_val,
                &mut state.axioms,
            ))
        }
        // Block: encode all expressions, return last
        Expr::Block(body) => {
            if body.is_empty() {
                return Some(tm.mk_boolean(true));
            }
            let mut result = None;
            for e in body {
                result = encode_expr_cvc5(tm, e, vars, state);
            }
            result
        }
        // Raw tokens: basic parsing (single token bools/ints/idents)
        Expr::Raw(tokens) => {
            if tokens.is_empty() {
                return Some(tm.mk_boolean(true));
            }
            if tokens.len() == 1 {
                let t = &tokens[0];
                if t == "true" {
                    return Some(tm.mk_boolean(true));
                }
                if t == "false" {
                    return Some(tm.mk_boolean(false));
                }
                if let Ok(n) = t.parse::<i64>() {
                    return Some(tm.mk_integer(n));
                }
                let key = sanitize_smtlib_name(t);
                return vars
                    .get(&key)
                    .cloned()
                    .or_else(|| Some(tm.mk_const(tm.integer_sort(), &key)));
            }
            // Multi-token: try to parse as infix expression
            encode_raw_tokens_cvc5(tm, tokens, vars, state)
        }
        // Tuple: fresh Int with element-access axioms
        Expr::Tuple(elems) => {
            let elem_vals: Option<Vec<_>> = elems
                .iter()
                .map(|elem| encode_expr_cvc5(tm, elem, vars, state))
                .collect();
            let elem_vals = elem_vals?;
            Some(encode_tuple_cvc5(
                tm,
                &elem_vals,
                &mut state.axioms,
                &mut state.fresh_counter,
            ))
        }
        // MethodCall: prepend receiver, call UF
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            if matches!(method.as_str(), "length" | "len") && args.is_empty() {
                return encode_length_receiver_cvc5(tm, receiver, vars, state);
            }

            let recv_val = encode_expr_cvc5(tm, receiver, vars, state)?;
            let mut all_encoded = vec![recv_val];
            for arg in args {
                all_encoded.push(encode_expr_cvc5(tm, arg, vars, state)?);
            }
            let f_name = sanitize_smtlib_name(method);
            if let Some(term) = encode_known_builtin_cvc5(tm, f_name.as_str(), &all_encoded, state)
            {
                return Some(term);
            }
            encode_uf_call_cvc5(tm, &f_name, &all_encoded, state)
        }
        // List: fresh Int with element-access and length axioms
        Expr::List(elems) => {
            let elem_vals: Option<Vec<_>> = elems
                .iter()
                .map(|elem| encode_expr_cvc5(tm, elem, vars, state))
                .collect();
            let elem_vals = elem_vals?;
            let len_func = field_len_fn_cvc5(tm, state);
            Some(encode_list_cvc5(
                tm,
                &elem_vals,
                &mut state.axioms,
                &mut state.fresh_counter,
                &len_func,
            ))
        }
        // Apply: encode args for side effects, return named bool
        Expr::Apply { lemma_name, args } => {
            for arg in args {
                let _ = encode_expr_cvc5(tm, arg, vars, state);
            }
            let apply_name = format!("__apply_{lemma_name}");
            Some(tm.mk_const(tm.boolean_sort(), &apply_name))
        }
    }
}

/// Encode multi-token raw expressions for the native CVC5 backend.
///
/// Uses a full precedence-climbing (Pratt) parser supporting:
/// - 8 precedence levels (matching the AST expression parser)
/// - Parenthesized sub-expressions
/// - `old(expr)` syntax
/// - `forall`/`exists` quantifiers: `forall x in domain { body }`
/// - Comparison chaining: `a < b < c` desugars to `a < b && b < c`
/// - Prefix operators: `!`, `-`, `not`
/// - Dot-separated field access: `x.y.z` -> `x__y__z`
/// - Function calls: `f(a, b)` with built-in semantics for abs/min/max
#[cfg(feature = "cvc5-verify")]
fn encode_raw_tokens_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    tokens: &[String],
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    if tokens.is_empty() {
        return Some(tm.mk_boolean(true));
    }
    let (val, _pos) = parse_raw_expr_cvc5(tm, tokens, 0, 0, vars, state)?;
    Some(val)
}

/// Precedence-climbing expression parser for raw CVC5 tokens.
///
/// Returns `(term, next_position)`. Recurses with higher `min_prec` for
/// tighter-binding operators. Supports comparison chaining.
#[cfg(feature = "cvc5-verify")]
fn parse_raw_expr_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    tokens: &[String],
    pos: usize,
    min_prec: u8,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<(cvc5::Term<'a>, usize)> {
    let (mut lhs, mut pos) = parse_raw_atom_cvc5(tm, tokens, pos, vars, state)?;

    while pos < tokens.len() {
        let Some((op_prec, op_kind)) = raw_op_info(tokens[pos].as_str()) else {
            break;
        };
        if op_prec < min_prec {
            break;
        }

        pos += 1; // consume operator

        let (rhs, next_pos) = parse_raw_expr_cvc5(tm, tokens, pos, op_prec + 1, vars, state)?;
        pos = next_pos;

        // Comparison chaining: if we just parsed `a < b` and the next
        // op is also a comparison, desugar `a < b < c` into `a < b && b < c`.
        if raw_op_is_comparison(op_kind)
            && pos < tokens.len()
            && let Some((next_prec, next_op)) = raw_op_info(tokens[pos].as_str())
            && raw_op_is_comparison(next_op)
            && next_prec >= min_prec
        {
            let left_cmp = apply_raw_op_cvc5(tm, op_kind, lhs, rhs.clone());
            pos += 1; // consume next operator
            let (rhs2, next_pos2) =
                parse_raw_expr_cvc5(tm, tokens, pos, next_prec + 1, vars, state)?;
            pos = next_pos2;
            let right_cmp = apply_raw_op_cvc5(tm, next_op, rhs, rhs2);
            lhs = tm.mk_term(cvc5::Kind::And, &[left_cmp, right_cmp]);
            continue;
        }

        lhs = apply_raw_op_cvc5(tm, op_kind, lhs, rhs);
    }

    Some((lhs, pos))
}

/// Parse a single atom from raw CVC5 tokens.
///
/// Handles: parenthesized expressions, `old(expr)`, `forall`/`exists`,
/// prefix operators (`!`, `-`, `not`), boolean/integer literals,
/// `result` keyword, specification keywords (skipped), dot-separated
/// field access, and function calls with built-in semantics.
#[cfg(feature = "cvc5-verify")]
fn parse_raw_atom_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    tokens: &[String],
    start: usize,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<(cvc5::Term<'a>, usize)> {
    if start >= tokens.len() {
        // Past end: vacuously true
        return Some((tm.mk_boolean(true), start));
    }

    let tok = &tokens[start];

    // --- Unary not ---
    if tok == "not" || tok == "!" {
        let (val, next) = parse_raw_atom_cvc5(tm, tokens, start + 1, vars, state)?;
        return Some((tm.mk_term(cvc5::Kind::Not, &[val]), next));
    }

    // --- Unary minus ---
    if tok == "-" {
        let (val, next) = parse_raw_atom_cvc5(tm, tokens, start + 1, vars, state)?;
        return Some((tm.mk_term(cvc5::Kind::Neg, &[val]), next));
    }

    // --- Parenthesized expression ---
    if tok == "(" {
        let (val, end) = parse_raw_expr_cvc5(tm, tokens, start + 1, 0, vars, state)?;
        // skip closing ')'
        let next = if end < tokens.len() && tokens[end] == ")" {
            end + 1
        } else {
            end
        };
        return Some((val, next));
    }

    // --- Boolean literals ---
    if tok == "true" {
        return Some((tm.mk_boolean(true), start + 1));
    }
    if tok == "false" {
        return Some((tm.mk_boolean(false), start + 1));
    }

    // --- `result` keyword ---
    if tok == "result" {
        let key = "__result";
        let v = vars
            .get(key)
            .cloned()
            .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), key));
        return Some((v, start + 1));
    }

    // --- `old(expr)` ---
    if tok == "old" && start + 1 < tokens.len() && tokens[start + 1] == "(" {
        let p = find_matching_delim(tokens, start + 1, "(", ")")?;
        let end = p + 1; // skip closing ')'
        let inner_tokens = &tokens[start + 2..p];

        // old(x) -> x__old
        if inner_tokens.len() == 1 {
            let old_name = format!("{}__old", sanitize_smtlib_name(&inner_tokens[0]));
            let v = vars
                .get(&old_name)
                .cloned()
                .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &old_name));
            return Some((v, end));
        }
        // old(x.field) -> x__old with field access UF
        if inner_tokens.len() == 3 && inner_tokens[1] == "." {
            let old_name = format!("{}__old", sanitize_smtlib_name(&inner_tokens[0]));
            let old_var = vars
                .get(&old_name)
                .cloned()
                .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &old_name));
            let field = sanitize_smtlib_name(&inner_tokens[2]);
            let func_name = format!("__field_{field}");
            let fun_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let func = tm.mk_const(fun_sort, &func_name);
            let result = tm.mk_term(cvc5::Kind::ApplyUf, &[func, old_var]);
            return Some((result, end));
        }
        // General old(expr): parse inner expression, remap vars to __old
        // (simplified: parse as-is, create __old-suffixed vars for idents)
        let mut old_vars = vars.clone();
        for inner_tok in inner_tokens {
            if inner_tok
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
                && !matches!(
                    inner_tok.as_str(),
                    "true"
                        | "false"
                        | "old"
                        | "forall"
                        | "exists"
                        | "result"
                        | "not"
                        | "and"
                        | "or"
                        | "implies"
                        | "mod"
                        | "div"
                        | "in"
                )
            {
                let sane = sanitize_smtlib_name(inner_tok);
                let old_key = format!("{sane}__old");
                if !old_vars.contains_key(&old_key) {
                    let term = tm.mk_const(tm.integer_sort(), &old_key);
                    old_vars.insert(old_key, term);
                }
            }
        }
        if let Some((val, _)) = parse_raw_expr_cvc5(tm, inner_tokens, 0, 0, &mut old_vars, state) {
            return Some((val, end));
        }
        // Fallback: fresh integer
        let fresh_name = format!("__old_fresh_{}", state.fresh_counter);
        state.fresh_counter += 1;
        return Some((tm.mk_const(tm.integer_sort(), &fresh_name), end));
    }

    // --- `forall`/`exists` quantifiers: `forall x in domain { body }` ---
    if let Some(slice) = parse_raw_quantifier_slice(tokens, start) {
        let var_name = sanitize_smtlib_name(&tokens[slice.var_token_idx]);

        // Bind quantifier variable
        let bound = tm.mk_var(tm.integer_sort(), &var_name);
        let mut local_vars = vars.clone();
        local_vars.insert(var_name.clone(), bound.clone());

        // Parse body
        let body_tokens = &tokens[slice.body_start..slice.body_end];
        if let Some((body_val, _)) =
            parse_raw_expr_cvc5(tm, body_tokens, 0, 0, &mut local_vars, state)
        {
            let var_list = tm.mk_term(cvc5::Kind::VariableList, &[bound]);
            let kind = if slice.is_forall {
                cvc5::Kind::Forall
            } else {
                cvc5::Kind::Exists
            };
            let quantified = tm.mk_term(kind, &[var_list, body_val]);
            return Some((quantified, slice.final_pos));
        }

        return Some((tm.mk_boolean(true), slice.final_pos));
    }

    // --- Integer literal ---
    if let Ok(n) = tok.parse::<i64>() {
        return Some((tm.mk_integer(n), start + 1));
    }

    // --- Skip specification keywords (taint/ghost/region/validate) ---
    if is_raw_spec_skip_keyword(tok) {
        return parse_raw_atom_cvc5(tm, tokens, start + 1, vars, state);
    }

    // --- Identifier (possibly with dot-separated field access) ---
    let mut name = sanitize_smtlib_name(tok);
    let mut next = start + 1;
    // Collapse `x.y.z` chains into `x__y__z`
    while next + 1 < tokens.len() && tokens[next] == "." {
        name.push_str("__");
        name.push_str(&sanitize_smtlib_name(&tokens[next + 1]));
        next += 2;
    }

    // --- #262: Typestate annotation: `Type @ State` ---
    // After collapsing dot chains, if the next token is `@` followed
    // by a state name, encode as integer equality:
    //   __typestate_<name> == hash(state_name)
    if next + 1 < tokens.len() && tokens[next] == "@" {
        let state_name = &tokens[next + 1];
        let ts_var_name = format!("__typestate_{name}");
        let ts_var = vars
            .entry(ts_var_name)
            .or_insert_with(|| tm.mk_const(tm.integer_sort(), &format!("__typestate_{name}")))
            .clone();
        let state_val = tm.mk_integer(pattern_hash_name(state_name));
        return Some((
            tm.mk_term(cvc5::Kind::Equal, &[ts_var, state_val]),
            next + 2,
        ));
    }

    // Check for function call: `name(args)`
    if next < tokens.len() && tokens[next] == "(" {
        let p = find_matching_delim(tokens, next, "(", ")")?;

        // Parse arguments by splitting on commas at depth 0
        let arg_tokens = &tokens[next + 1..p];
        let mut arg_vals: Vec<cvc5::Term<'a>> = Vec::new();
        for (lo, hi) in comma_chunk_ranges(arg_tokens) {
            let chunk = &arg_tokens[lo..hi];
            if !chunk.is_empty()
                && let Some((v, _)) = parse_raw_expr_cvc5(tm, chunk, 0, 0, vars, state)
            {
                arg_vals.push(v);
            }
        }
        let end = p + 1; // skip closing ')'

        // Extract base function name (last segment after dots)
        let func_name = name.rsplit("__").next().unwrap_or(&name);

        // Built-in functions
        match func_name {
            "abs" if arg_vals.len() == 1 => {
                let x = arg_vals[0].clone();
                let zero = tm.mk_integer(0);
                let neg_x = tm.mk_term(cvc5::Kind::Neg, &[x.clone()]);
                let cond = tm.mk_term(cvc5::Kind::Geq, &[x.clone(), zero]);
                return Some((tm.mk_term(cvc5::Kind::Ite, &[cond, x, neg_x]), end));
            }
            "min" if arg_vals.len() == 2 => {
                let (a, b) = (arg_vals[0].clone(), arg_vals[1].clone());
                let cond = tm.mk_term(cvc5::Kind::Leq, &[a.clone(), b.clone()]);
                return Some((tm.mk_term(cvc5::Kind::Ite, &[cond, a, b]), end));
            }
            "max" if arg_vals.len() == 2 => {
                let (a, b) = (arg_vals[0].clone(), arg_vals[1].clone());
                let cond = tm.mk_term(cvc5::Kind::Geq, &[a.clone(), b.clone()]);
                return Some((tm.mk_term(cvc5::Kind::Ite, &[cond, a, b]), end));
            }
            "length" if arg_vals.is_empty() => {
                // x.length() -> UF with length >= 0 axiom
                let uf_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let uf = tm.mk_const(uf_sort, "__length");
                let base_var = vars
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &name));
                let result = tm.mk_term(cvc5::Kind::ApplyUf, &[uf, base_var]);
                let zero = tm.mk_integer(0);
                let axiom = tm.mk_term(cvc5::Kind::Geq, &[result.clone(), zero]);
                state.axioms.push(axiom);
                return Some((result, end));
            }
            _ => {
                // Generic UF
                if arg_vals.is_empty() {
                    let v = vars
                        .get(&name)
                        .cloned()
                        .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &name));
                    return Some((v, end));
                }
                let n_args = arg_vals.len();
                let domain: Vec<_> = (0..n_args).map(|_| tm.integer_sort()).collect();
                let fun_sort = tm.mk_fun_sort(&domain, tm.integer_sort());
                let func = tm.mk_const(fun_sort, &name);
                let mut all_args = vec![func];
                all_args.extend(arg_vals);
                return Some((tm.mk_term(cvc5::Kind::ApplyUf, &all_args), end));
            }
        }
    }

    // Plain identifier
    let v = vars
        .get(&name)
        .cloned()
        .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &name));
    Some((v, next))
}
