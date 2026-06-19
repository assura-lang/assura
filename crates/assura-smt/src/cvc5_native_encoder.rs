//! Native CVC5 term encoding (feature = "cvc5-verify").
//!
//! Expression-to-term translation extracted from `cvc5_backend.rs`.

use std::collections::HashMap;

use assura_parser::ast::{Clause, Expr, Literal};

use crate::cvc5_binop_encode::{encode_ast_binop_cvc5, encode_ast_unary_cvc5};
use crate::cvc5_builtins::{is_bool_field, is_size_field};
use crate::cvc5_call_encode::{encode_call_cvc5, encode_method_call_cvc5};
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
use crate::cvc5_raw_native::encode_raw_tokens_cvc5;

pub(crate) use crate::cvc5_encoder_state::{Cvc5EncoderState, default_cvc5_encoder_state};

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
        Expr::Call { func, args } => encode_call_cvc5(tm, func, args, vars, state, |e, v, s| {
            encode_expr_cvc5(tm, e, v, s)
        }),
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
        } => encode_method_call_cvc5(tm, receiver, method, args, vars, state, |e, v, s| {
            encode_expr_cvc5(tm, e, v, s)
        }),
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
