//! Native CVC5 Pratt parser for multi-token `Expr::Raw` expressions.

use std::collections::HashMap;

use crate::cvc5_encoder_state::Cvc5EncoderState;
use crate::cvc5_raw_ops::{
    apply_raw_op_cvc5, comma_chunk_ranges, find_matching_delim, is_raw_spec_skip_keyword,
    parse_raw_quantifier_slice, raw_op_info, raw_op_is_comparison,
};
use crate::encode_atom_policy::append_raw_dotted_segment;
use crate::encode_atom_policy::sanitize_smt_name;
use crate::encode_method_policy::pattern_hash_name;

/// Encode multi-token raw expressions for the native CVC5 backend.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_raw_tokens_cvc5(
    tm: &mut cvc5::TermManager,
    tokens: &[String],
    vars: &mut HashMap<String, cvc5::Term>,
    state: &mut Cvc5EncoderState,
) -> Option<cvc5::Term> {
    if tokens.is_empty() {
        return Some(tm.mk_boolean(true));
    }
    let (val, _pos) = parse_raw_expr_cvc5(tm, tokens, 0, 0, vars, state)?;
    Some(val)
}

/// Precedence-climbing expression parser for raw CVC5 tokens.
#[cfg(feature = "cvc5-verify")]
fn parse_raw_expr_cvc5(
    tm: &mut cvc5::TermManager,
    tokens: &[String],
    pos: usize,
    min_prec: u8,
    vars: &mut HashMap<String, cvc5::Term>,
    state: &mut Cvc5EncoderState,
) -> Option<(cvc5::Term, usize)> {
    let (mut lhs, mut pos) = parse_raw_atom_cvc5(tm, tokens, pos, vars, state)?;

    while pos < tokens.len() {
        let Some((op_prec, op_kind)) = raw_op_info(tokens[pos].as_str()) else {
            break;
        };
        if op_prec < min_prec {
            break;
        }

        pos += 1;

        let (rhs, next_pos) = parse_raw_expr_cvc5(tm, tokens, pos, op_prec + 1, vars, state)?;
        pos = next_pos;

        if raw_op_is_comparison(op_kind)
            && pos < tokens.len()
            && let Some((next_prec, next_op)) = raw_op_info(tokens[pos].as_str())
            && raw_op_is_comparison(next_op)
            && next_prec >= min_prec
        {
            let left_cmp = apply_raw_op_cvc5(tm, op_kind, lhs, rhs.clone());
            pos += 1;
            let (rhs2, next_pos2) =
                parse_raw_expr_cvc5(tm, tokens, pos, next_prec + 1, vars, state)?;
            pos = next_pos2;
            let right_cmp = apply_raw_op_cvc5(tm, next_op, rhs, rhs2);
            lhs = tm
                .mk_term(cvc5::Kind::And, &[left_cmp, right_cmp])
                .expect("cvc5");
            continue;
        }

        lhs = apply_raw_op_cvc5(tm, op_kind, lhs, rhs);
    }

    Some((lhs, pos))
}

/// Parse a single atom from raw CVC5 tokens.
#[cfg(feature = "cvc5-verify")]
fn parse_raw_atom_cvc5(
    tm: &mut cvc5::TermManager,
    tokens: &[String],
    start: usize,
    vars: &mut HashMap<String, cvc5::Term>,
    state: &mut Cvc5EncoderState,
) -> Option<(cvc5::Term, usize)> {
    if start >= tokens.len() {
        return Some((tm.mk_boolean(true), start));
    }

    let tok = &tokens[start];

    if tok == "not" || tok == "!" {
        let (val, next) = parse_raw_atom_cvc5(tm, tokens, start + 1, vars, state)?;
        return Some((tm.mk_term(cvc5::Kind::Not, &[val]).expect("cvc5"), next));
    }

    if tok == "-" {
        let (val, next) = parse_raw_atom_cvc5(tm, tokens, start + 1, vars, state)?;
        return Some((tm.mk_term(cvc5::Kind::Neg, &[val]).expect("cvc5"), next));
    }

    if tok == "(" {
        let (val, end) = parse_raw_expr_cvc5(tm, tokens, start + 1, 0, vars, state)?;
        let next = if end < tokens.len() && tokens[end] == ")" {
            end + 1
        } else {
            end
        };
        return Some((val, next));
    }

    if tok == "true" {
        return Some((tm.mk_boolean(true), start + 1));
    }
    if tok == "false" {
        return Some((tm.mk_boolean(false), start + 1));
    }

    if tok == "result" {
        let key = crate::encode_atom_policy::RESULT_VAR_NAME;
        let v = vars
            .entry(key.to_string())
            .or_insert_with(|| {
                {
                    let __cvc14 = tm.integer_sort();
                    tm.mk_const(__cvc14, key)
                }
                .expect("cvc5")
            })
            .clone();
        return Some((v, start + 1));
    }

    if tok == "old" && start + 1 < tokens.len() && tokens[start + 1] == "(" {
        let p = find_matching_delim(tokens, start + 1, "(", ")")?;
        let end = p + 1;
        let inner_tokens = &tokens[start + 2..p];

        // Pre-state shapes via encode_old_policy (parity with shell SMT-LIB / Z3 raw).
        match crate::encode_old_policy::classify_raw_old_inner(inner_tokens) {
            crate::encode_old_policy::RawOldPlan::Ident(name) => {
                let old_name = crate::encode_old_policy::raw_old_ident_snapshot_name(&name);
                let v = vars.get(&old_name).cloned().unwrap_or_else(|| {
                    {
                        let __cvc13 = tm.integer_sort();
                        tm.mk_const(__cvc13, &old_name)
                    }
                    .expect("cvc5")
                });
                return Some((v, end));
            }
            crate::encode_old_policy::RawOldPlan::ShallowField { base, field } => {
                let old_name = crate::encode_old_policy::raw_old_ident_snapshot_name(&base);
                let old_var = vars.get(&old_name).cloned().unwrap_or_else(|| {
                    {
                        let __cvc12 = tm.integer_sort();
                        tm.mk_const(__cvc12, &old_name)
                    }
                    .expect("cvc5")
                });
                let field = sanitize_smt_name(&field);
                let func_name = crate::encode_atom_policy::field_uif_name(&field);
                let fun_sort = {
                    let __cvc11 = tm.integer_sort();
                    let __cvc10 = tm.integer_sort();
                    tm.mk_fun_sort(&[__cvc11], __cvc10)
                }
                .expect("cvc5");
                let func = tm.mk_const(fun_sort, &func_name).expect("cvc5");
                let result = tm
                    .mk_term(cvc5::Kind::ApplyUf, &[func, old_var])
                    .expect("cvc5");
                return Some((result, end));
            }
            crate::encode_old_policy::RawOldPlan::Complex => {
                let rewritten =
                    crate::encode_old_policy::rewrite_raw_tokens_under_old(inner_tokens);
                if let Some((val, _)) = parse_raw_expr_cvc5(tm, &rewritten, 0, 0, vars, state) {
                    return Some((val, end));
                }
                let fresh_name =
                    crate::encode_old_policy::allocate_old_complex_fresh(&mut state.fresh_counter);
                return Some((
                    {
                        let __cvc9 = tm.integer_sort();
                        tm.mk_const(__cvc9, &fresh_name)
                    }
                    .expect("cvc5"),
                    end,
                ));
            }
        }
    }

    if let Some(slice) = parse_raw_quantifier_slice(tokens, start) {
        let var_name = sanitize_smt_name(&tokens[slice.var_token_idx]);

        let bound = {
            let __cvc8 = tm.integer_sort();
            tm.mk_var(__cvc8, &var_name)
        }
        .expect("cvc5");
        let mut local_vars = vars.clone();
        local_vars.insert(var_name.clone(), bound.clone());

        let body_tokens = &tokens[slice.body_start..slice.body_end];
        if let Some((body_val, _)) =
            parse_raw_expr_cvc5(tm, body_tokens, 0, 0, &mut local_vars, state)
        {
            let var_list = tm
                .mk_term(cvc5::Kind::VariableList, &[bound])
                .expect("cvc5");
            let kind = if slice.is_forall {
                cvc5::Kind::Forall
            } else {
                cvc5::Kind::Exists
            };
            let quantified = tm.mk_term(kind, &[var_list, body_val]).expect("cvc5");
            return Some((quantified, slice.final_pos));
        }

        return Some((tm.mk_boolean(true), slice.final_pos));
    }

    if let Ok(n) = tok.parse::<i64>() {
        return Some((tm.mk_integer(n), start + 1));
    }

    // Float literal: "3.14" → rational (numer/denom) on Real sort.
    // Uses mk_real_from_rational for correct Real sort (fixes #459,
    // parity with cvc5_atom_encode.rs and Z3 parse_raw_atom).
    if tok.contains('.') && tok.parse::<f64>().is_ok() {
        let (numer, denom) = crate::encode_atom_policy::float_to_rational_parts(tok);
        return Some((
            tm.mk_real_from_rational(numer, denom).expect("cvc5"),
            start + 1,
        ));
    }

    if is_raw_spec_skip_keyword(tok) {
        return parse_raw_atom_cvc5(tm, tokens, start + 1, vars, state);
    }

    let mut name = sanitize_smt_name(tok);
    let mut next = start + 1;
    while next + 1 < tokens.len() && tokens[next] == "." {
        append_raw_dotted_segment(&mut name, &tokens[next + 1]);
        next += 2;
    }

    if next + 1 < tokens.len() && tokens[next] == "@" {
        let state_name = &tokens[next + 1];
        let ts_var_name = crate::encode_atom_policy::typestate_var_name(&name);
        let ts_var = vars
            .entry(ts_var_name.clone())
            .or_insert_with(|| {
                {
                    let __cvc7 = tm.integer_sort();
                    tm.mk_const(__cvc7, &ts_var_name)
                }
                .expect("cvc5")
            })
            .clone();
        let state_val = tm.mk_integer(pattern_hash_name(state_name));
        return Some((
            tm.mk_term(cvc5::Kind::Equal, &[ts_var, state_val])
                .expect("cvc5"),
            next + 2,
        ));
    }

    if next < tokens.len() && tokens[next] == "(" {
        let p = find_matching_delim(tokens, next, "(", ")")?;

        let arg_tokens = &tokens[next + 1..p];
        let mut arg_vals: Vec<cvc5::Term> = Vec::new();
        for (lo, hi) in comma_chunk_ranges(arg_tokens) {
            let chunk = &arg_tokens[lo..hi];
            if !chunk.is_empty()
                && let Some((v, _)) = parse_raw_expr_cvc5(tm, chunk, 0, 0, vars, state)
            {
                arg_vals.push(v);
            }
        }
        let end = p + 1;

        // Extract base function name via shared policy (fixes #464).
        let func_name = crate::encode_atom_policy::extract_raw_base_name(&name);

        // Dispatch via classify_encode_call (parity with Z3 parse_raw_atom).
        use crate::encode_call_policy::{EncodeCallKind, classify_encode_call};
        let call_kind = classify_encode_call(func_name, arg_vals.len());

        if matches!(call_kind, EncodeCallKind::Abs) && arg_vals.len() == 1 {
            let x = arg_vals[0].clone();
            let zero = tm.mk_integer(0);
            let neg_x = tm
                .mk_term(cvc5::Kind::Neg, std::slice::from_ref(&x))
                .expect("cvc5");
            let cond = tm
                .mk_term(cvc5::Kind::Geq, &[x.clone(), zero])
                .expect("cvc5");
            return Some((
                tm.mk_term(cvc5::Kind::Ite, &[cond, x, neg_x])
                    .expect("cvc5"),
                end,
            ));
        }
        if matches!(call_kind, EncodeCallKind::MinMax) && arg_vals.len() == 2 {
            let (a, b) = (arg_vals[0].clone(), arg_vals[1].clone());
            let cond = if func_name == "min" {
                tm.mk_term(cvc5::Kind::Leq, &[a.clone(), b.clone()])
                    .expect("cvc5")
            } else {
                tm.mk_term(cvc5::Kind::Geq, &[a.clone(), b.clone()])
                    .expect("cvc5")
            };
            return Some((
                tm.mk_term(cvc5::Kind::Ite, &[cond, a, b]).expect("cvc5"),
                end,
            ));
        }
        if matches!(call_kind, EncodeCallKind::BoolReturningUf) {
            let arity = arg_vals.len().max(1);
            let domain: Vec<_> = (0..arity).map(|_| tm.integer_sort()).collect();
            let fun_sort = {
                let __cvc6 = tm.boolean_sort();
                tm.mk_fun_sort(&domain, __cvc6)
            }
            .expect("cvc5");
            let func = tm.mk_const(fun_sort, func_name).expect("cvc5");
            let args = if arg_vals.is_empty() {
                vec![
                    func.clone(),
                    {
                        let __cvc5 = tm.integer_sort();
                        tm.mk_const(__cvc5, "__dummy")
                    }
                    .expect("cvc5"),
                ]
            } else {
                let mut a = vec![func.clone()];
                a.extend(arg_vals);
                a
            };
            return Some((tm.mk_term(cvc5::Kind::ApplyUf, &args).expect("cvc5"), end));
        }
        if matches!(call_kind, EncodeCallKind::SizeFieldUf) {
            let arity = arg_vals.len().max(1);
            let domain: Vec<_> = (0..arity).map(|_| tm.integer_sort()).collect();
            let fun_sort = {
                let __cvc4 = tm.integer_sort();
                tm.mk_fun_sort(&domain, __cvc4)
            }
            .expect("cvc5");
            let func = tm.mk_const(fun_sort, func_name).expect("cvc5");
            let args = if arg_vals.is_empty() {
                vec![
                    func.clone(),
                    {
                        let __cvc3 = tm.integer_sort();
                        tm.mk_const(__cvc3, "__dummy")
                    }
                    .expect("cvc5"),
                ]
            } else {
                let mut a = vec![func.clone()];
                a.extend(arg_vals);
                a
            };
            let result = tm.mk_term(cvc5::Kind::ApplyUf, &args).expect("cvc5");
            let zero = tm.mk_integer(0);
            let axiom = tm
                .mk_term(cvc5::Kind::Geq, &[result.clone(), zero])
                .expect("cvc5");
            state.axioms.push(axiom);
            return Some((result, end));
        }

        // Fallback: uninterpreted function or zero-arg variable lookup.
        if arg_vals.is_empty() {
            let v = vars
                .entry(name.clone())
                .or_insert_with(|| {
                    {
                        let __cvc2 = tm.integer_sort();
                        tm.mk_const(__cvc2, &name)
                    }
                    .expect("cvc5")
                })
                .clone();
            return Some((v, end));
        }
        let n_args = arg_vals.len();
        let domain: Vec<_> = (0..n_args).map(|_| tm.integer_sort()).collect();
        let fun_sort = {
            let __cvc1 = tm.integer_sort();
            tm.mk_fun_sort(&domain, __cvc1)
        }
        .expect("cvc5");
        let func = tm.mk_const(fun_sort, &name).expect("cvc5");
        let mut all_args = vec![func];
        all_args.extend(arg_vals);
        return Some((
            tm.mk_term(cvc5::Kind::ApplyUf, &all_args).expect("cvc5"),
            end,
        ));
    }

    // Cache newly created variables (fixes #457: parity with Z3 var caching).
    let v = vars
        .entry(name.clone())
        .or_insert_with(|| {
            {
                let __cvc0 = tm.integer_sort();
                tm.mk_const(__cvc0, &name)
            }
            .expect("cvc5")
        })
        .clone();
    Some((v, next))
}

#[cfg(all(test, feature = "cvc5-verify"))]
mod tests {
    use super::*;

    #[test]
    fn dotted_raw_token_chain_uses_single_underscore() {
        let mut tm = cvc5::TermManager::new();
        let mut vars = HashMap::new();
        let mut state = crate::cvc5_encoder_state::default_cvc5_encoder_state();
        let tokens = vec!["state".into(), ".".into(), "field".into()];
        let term = encode_raw_tokens_cvc5(&mut tm, &tokens, &mut vars, &mut state).unwrap();
        assert_eq!(term.to_string(), "state_field");
    }
}
