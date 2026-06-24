//! SMT-LIB2 Pratt parser for multi-token `Expr::Raw` expressions (shell-out path).

use crate::cvc5_common::append_raw_dotted_segment;
use crate::cvc5_raw_ops::{
    comma_chunk_ranges, find_matching_delim, format_raw_binop_smtlib, format_raw_quantifier_smtlib,
    is_raw_spec_skip_keyword, parse_raw_quantifier_slice, raw_op_info, raw_op_is_comparison,
};
use crate::encode_atom_policy::sanitize_smt_name;

/// Encode multi-token raw expressions as SMT-LIB2.
pub(crate) fn encode_raw_tokens_smtlib(tokens: &[String]) -> Option<String> {
    let (val, _pos) = parse_raw_expr_smtlib(tokens, 0, 0)?;
    Some(val)
}

/// Precedence-climbing expression parser for raw tokens producing SMT-LIB2 text.
///
/// Returns `(smtlib_string, next_position)`.
fn parse_raw_expr_smtlib(tokens: &[String], pos: usize, min_prec: u8) -> Option<(String, usize)> {
    let (mut lhs, mut pos) = parse_raw_atom_smtlib(tokens, pos)?;

    while pos < tokens.len() {
        let Some((op_prec, op_kind)) = raw_op_info(tokens[pos].as_str()) else {
            break;
        };
        if op_prec < min_prec {
            break;
        }

        pos += 1;

        let (rhs, next_pos) = parse_raw_expr_smtlib(tokens, pos, op_prec + 1)?;
        pos = next_pos;

        if raw_op_is_comparison(op_kind)
            && pos < tokens.len()
            && let Some((next_prec, next_op)) = raw_op_info(tokens[pos].as_str())
            && raw_op_is_comparison(next_op)
            && next_prec >= min_prec
        {
            let left_cmp = format_raw_binop_smtlib(op_kind, &lhs, &rhs);
            pos += 1;
            let (rhs2, next_pos2) = parse_raw_expr_smtlib(tokens, pos, next_prec + 1)?;
            pos = next_pos2;
            let right_cmp = format_raw_binop_smtlib(next_op, &rhs, &rhs2);
            lhs = format!("(and {left_cmp} {right_cmp})");
            continue;
        }

        lhs = format_raw_binop_smtlib(op_kind, &lhs, &rhs);
    }

    Some((lhs, pos))
}

/// Parse a single atom from raw tokens into SMT-LIB2 text.
fn parse_raw_atom_smtlib(tokens: &[String], start: usize) -> Option<(String, usize)> {
    if start >= tokens.len() {
        return Some(("true".to_string(), start));
    }

    let tok = &tokens[start];

    if tok == "not" || tok == "!" {
        let (val, next) = parse_raw_atom_smtlib(tokens, start + 1)?;
        return Some((format!("(not {val})"), next));
    }

    if tok == "-" {
        let (val, next) = parse_raw_atom_smtlib(tokens, start + 1)?;
        return Some((format!("(- {val})"), next));
    }

    if tok == "(" {
        let (val, end) = parse_raw_expr_smtlib(tokens, start + 1, 0)?;
        let next = if end < tokens.len() && tokens[end] == ")" {
            end + 1
        } else {
            end
        };
        return Some((val, next));
    }

    if tok == "true" || tok == "false" {
        return Some((tok.clone(), start + 1));
    }

    if tok == "result" {
        return Some((
            crate::encode_atom_policy::RESULT_VAR_NAME.to_string(),
            start + 1,
        ));
    }

    if tok == "old" && start + 1 < tokens.len() && tokens[start + 1] == "(" {
        let p = find_matching_delim(tokens, start + 1, "(", ")")?;
        let end = p + 1;
        let inner = &tokens[start + 2..p];

        if inner.len() == 1 {
            let old_name = crate::encode_atom_policy::old_snapshot_name(&inner[0]);
            return Some((old_name, end));
        }
        if let Some((val, _)) = parse_raw_expr_smtlib(inner, 0, 0) {
            return Some((val, end));
        }
        return Some(("__old_fresh".to_string(), end));
    }

    if let Some(slice) = parse_raw_quantifier_slice(tokens, start) {
        let var_name = sanitize_smt_name(&tokens[slice.var_token_idx]);
        let body_tokens = &tokens[slice.body_start..slice.body_end];
        if let Some((body_val, _)) = parse_raw_expr_smtlib(body_tokens, 0, 0) {
            return Some((
                format_raw_quantifier_smtlib(slice.is_forall, &var_name, &body_val),
                slice.final_pos,
            ));
        }
        return Some((
            format_raw_quantifier_smtlib(slice.is_forall, &var_name, "true"),
            slice.final_pos,
        ));
    }

    if tok.parse::<i64>().is_ok() {
        return Some((tok.clone(), start + 1));
    }

    if is_raw_spec_skip_keyword(tok) {
        return parse_raw_atom_smtlib(tokens, start + 1);
    }

    let mut name = sanitize_smt_name(tok);
    let mut next = start + 1;
    while next + 1 < tokens.len() && tokens[next] == "." {
        append_raw_dotted_segment(&mut name, &tokens[next + 1]);
        next += 2;
    }

    if next < tokens.len() && tokens[next] == "(" {
        let p = find_matching_delim(tokens, next, "(", ")")?;
        let arg_tokens = &tokens[next + 1..p];
        let mut arg_strs: Vec<String> = Vec::new();
        for (lo, hi) in comma_chunk_ranges(arg_tokens) {
            let chunk = &arg_tokens[lo..hi];
            if !chunk.is_empty()
                && let Some((v, _)) = parse_raw_expr_smtlib(chunk, 0, 0)
            {
                arg_strs.push(v);
            }
        }
        let end = p + 1;

        if arg_strs.is_empty() {
            return Some((name, end));
        }
        return Some((format!("({name} {})", arg_strs.join(" ")), end));
    }

    Some((name, next))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_climbing_mul_over_add() {
        let tokens = vec!["a".into(), "+".into(), "b".into(), "*".into(), "c".into()];
        assert_eq!(
            encode_raw_tokens_smtlib(&tokens),
            Some("(+ a (* b c))".into())
        );
    }

    #[test]
    fn comparison_chain_desugars_to_and() {
        let tokens = vec!["a".into(), "<".into(), "b".into(), "<".into(), "c".into()];
        assert_eq!(
            encode_raw_tokens_smtlib(&tokens),
            Some("(and (< a b) (< b c))".into())
        );
    }

    #[test]
    fn dotted_raw_token_chain_uses_single_underscore() {
        let tokens = vec!["state".into(), ".".into(), "field".into()];
        assert_eq!(
            encode_raw_tokens_smtlib(&tokens),
            Some("state_field".into())
        );
    }

    #[test]
    fn old_single_ident_suffixes_old() {
        let tokens = vec![
            "old".into(),
            "(".into(),
            "x".into(),
            ")".into(),
            "+".into(),
            "1".into(),
        ];
        assert_eq!(
            encode_raw_tokens_smtlib(&tokens),
            Some("(+ x__old 1)".into())
        );
    }
}
