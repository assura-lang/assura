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
pub(crate) fn encode_raw_tokens_cvc5<'a>(
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
            lhs = tm.mk_term(cvc5::Kind::And, &[left_cmp, right_cmp]);
            continue;
        }

        lhs = apply_raw_op_cvc5(tm, op_kind, lhs, rhs);
    }

    Some((lhs, pos))
}

/// Parse a single atom from raw CVC5 tokens.
#[cfg(feature = "cvc5-verify")]
fn parse_raw_atom_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    tokens: &[String],
    start: usize,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<(cvc5::Term<'a>, usize)> {
    if start >= tokens.len() {
        return Some((tm.mk_boolean(true), start));
    }

    let tok = &tokens[start];

    if tok == "not" || tok == "!" {
        let (val, next) = parse_raw_atom_cvc5(tm, tokens, start + 1, vars, state)?;
        return Some((tm.mk_term(cvc5::Kind::Not, &[val]), next));
    }

    if tok == "-" {
        let (val, next) = parse_raw_atom_cvc5(tm, tokens, start + 1, vars, state)?;
        return Some((tm.mk_term(cvc5::Kind::Neg, &[val]), next));
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
            .get(key)
            .cloned()
            .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), key));
        return Some((v, start + 1));
    }

    if tok == "old" && start + 1 < tokens.len() && tokens[start + 1] == "(" {
        let p = find_matching_delim(tokens, start + 1, "(", ")")?;
        let end = p + 1;
        let inner_tokens = &tokens[start + 2..p];

        if inner_tokens.len() == 1 {
            let old_name = crate::encode_atom_policy::old_snapshot_name(&inner_tokens[0]);
            let v = vars
                .get(&old_name)
                .cloned()
                .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &old_name));
            return Some((v, end));
        }
        if inner_tokens.len() == 3 && inner_tokens[1] == "." {
            let old_name = crate::encode_atom_policy::old_snapshot_name(&inner_tokens[0]);
            let old_var = vars
                .get(&old_name)
                .cloned()
                .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &old_name));
            let field = sanitize_smt_name(&inner_tokens[2]);
            let func_name = crate::encode_atom_policy::field_uif_name(&field);
            let fun_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let func = tm.mk_const(fun_sort, &func_name);
            let result = tm.mk_term(cvc5::Kind::ApplyUf, &[func, old_var]);
            return Some((result, end));
        }

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
                let old_key = crate::encode_atom_policy::old_snapshot_name(inner_tok);
                old_vars
                    .entry(old_key.clone())
                    .or_insert_with(|| tm.mk_const(tm.integer_sort(), &old_key));
            }
        }
        if let Some((val, _)) = parse_raw_expr_cvc5(tm, inner_tokens, 0, 0, &mut old_vars, state) {
            return Some((val, end));
        }
        let fresh_name = crate::encode_atom_policy::old_fresh_temp_name(state.fresh_counter);
        state.fresh_counter += 1;
        return Some((tm.mk_const(tm.integer_sort(), &fresh_name), end));
    }

    if let Some(slice) = parse_raw_quantifier_slice(tokens, start) {
        let var_name = sanitize_smt_name(&tokens[slice.var_token_idx]);

        let bound = tm.mk_var(tm.integer_sort(), &var_name);
        let mut local_vars = vars.clone();
        local_vars.insert(var_name.clone(), bound.clone());

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

    if let Ok(n) = tok.parse::<i64>() {
        return Some((tm.mk_integer(n), start + 1));
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
            .or_insert_with(|| tm.mk_const(tm.integer_sort(), &ts_var_name))
            .clone();
        let state_val = tm.mk_integer(pattern_hash_name(state_name));
        return Some((
            tm.mk_term(cvc5::Kind::Equal, &[ts_var, state_val]),
            next + 2,
        ));
    }

    if next < tokens.len() && tokens[next] == "(" {
        let p = find_matching_delim(tokens, next, "(", ")")?;

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
        let end = p + 1;

        let func_name = name.rsplit("__").next().unwrap_or(&name);

        match func_name {
            "abs" if arg_vals.len() == 1 => {
                let x = arg_vals[0].clone();
                let zero = tm.mk_integer(0);
                let neg_x = tm.mk_term(cvc5::Kind::Neg, std::slice::from_ref(&x));
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
                let uf_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let uf = tm.mk_const(uf_sort, crate::encode_atom_policy::RAW_LENGTH_UF_NAME);
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

    let v = vars
        .get(&name)
        .cloned()
        .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &name));
    Some((v, next))
}

#[cfg(all(test, feature = "cvc5-verify"))]
mod tests {
    use super::*;

    #[test]
    fn dotted_raw_token_chain_uses_single_underscore() {
        let tm = cvc5::TermManager::new();
        let mut vars = HashMap::new();
        let mut state = crate::cvc5_encoder_state::default_cvc5_encoder_state();
        let tokens = vec!["state".into(), ".".into(), "field".into()];
        let term = encode_raw_tokens_cvc5(&tm, &tokens, &mut vars, &mut state).unwrap();
        assert_eq!(term.to_string(), "state_field");
    }
}
