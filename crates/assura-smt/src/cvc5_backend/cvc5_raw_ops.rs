//! Shared raw-token operator table and quantifier helpers for CVC5 backends.
//!
//! Shell-out (`cvc5_raw_smtlib`) and native (`cvc5_raw_native`)
//! (`encode_expr_cvc5` / `parse_raw_expr_cvc5`) share precedence, comparison
//! chaining, quantifier wrapping, comma-splitting, and AST `BinOp` tables.

use assura_ast::{BinOp, Expr, SpExpr};

/// Binary operators recognized by the raw-token Pratt parsers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Leq,
    Gt,
    Geq,
    And,
    Or,
    Implies,
}

/// Return precedence and operator kind for a raw token.
pub(crate) fn raw_op_info(tok: &str) -> Option<(u8, RawBinOp)> {
    match tok {
        "||" | "or" => Some((1, RawBinOp::Or)),
        "&&" | "and" => Some((3, RawBinOp::And)),
        "=>" | "==>" | "implies" => Some((3, RawBinOp::Implies)),
        "==" | "=" => Some((5, RawBinOp::Eq)),
        "!=" => Some((5, RawBinOp::Neq)),
        "<" => Some((7, RawBinOp::Lt)),
        ">" => Some((7, RawBinOp::Gt)),
        "<=" => Some((7, RawBinOp::Leq)),
        ">=" => Some((7, RawBinOp::Geq)),
        "+" => Some((9, RawBinOp::Add)),
        "-" => Some((9, RawBinOp::Sub)),
        "*" => Some((11, RawBinOp::Mul)),
        "/" | "div" => Some((11, RawBinOp::Div)),
        "%" | "mod" => Some((11, RawBinOp::Mod)),
        _ => None,
    }
}

/// Whether the operator participates in comparison chaining (`a < b < c`).
pub(crate) fn raw_op_is_comparison(op: RawBinOp) -> bool {
    matches!(
        op,
        RawBinOp::Lt | RawBinOp::Leq | RawBinOp::Gt | RawBinOp::Geq | RawBinOp::Eq | RawBinOp::Neq
    )
}

/// Format a binary operation as SMT-LIB2 prefix notation.
pub(crate) fn format_raw_binop_smtlib(op: RawBinOp, lhs: &str, rhs: &str) -> String {
    match op {
        RawBinOp::Neq => format!("(not (= {lhs} {rhs}))"),
        RawBinOp::Add => format!("(+ {lhs} {rhs})"),
        RawBinOp::Sub => format!("(- {lhs} {rhs})"),
        RawBinOp::Mul => format!("(* {lhs} {rhs})"),
        RawBinOp::Div => format!("(div {lhs} {rhs})"),
        RawBinOp::Mod => format!("(mod {lhs} {rhs})"),
        RawBinOp::Eq => format!("(= {lhs} {rhs})"),
        RawBinOp::Lt => format!("(< {lhs} {rhs})"),
        RawBinOp::Gt => format!("(> {lhs} {rhs})"),
        RawBinOp::Leq => format!("(<= {lhs} {rhs})"),
        RawBinOp::Geq => format!("(>= {lhs} {rhs})"),
        RawBinOp::And => format!("(and {lhs} {rhs})"),
        RawBinOp::Or => format!("(or {lhs} {rhs})"),
        RawBinOp::Implies => format!("(=> {lhs} {rhs})"),
    }
}

/// Specification keywords skipped by raw-token atom parsers.
pub(crate) const RAW_SPEC_SKIP_KEYWORDS: &[&str] = &[
    "taint",
    "untrusted",
    "validated",
    "ghost",
    "Region",
    "validate",
];

pub(crate) fn is_raw_spec_skip_keyword(tok: &str) -> bool {
    RAW_SPEC_SKIP_KEYWORDS.contains(&tok)
}

/// Given `tokens[start] == open`, find the index of the matching `close` token.
pub(crate) fn find_matching_delim(
    tokens: &[String],
    start: usize,
    open: &str,
    close: &str,
) -> Option<usize> {
    if start >= tokens.len() || tokens[start] != open {
        return None;
    }
    let mut depth = 1usize;
    let mut pos = start + 1;
    while pos < tokens.len() && depth > 0 {
        match tokens[pos].as_str() {
            s if s == open => depth += 1,
            s if s == close => depth -= 1,
            _ => {}
        }
        if depth > 0 {
            pos += 1;
        }
    }
    if depth == 0 { Some(pos) } else { None }
}

/// Body slice for `forall|exists VAR in DOMAIN :|{ BODY`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawQuantifierSlice {
    pub is_forall: bool,
    pub var_token_idx: usize,
    pub body_start: usize,
    pub body_end: usize,
    pub final_pos: usize,
}

/// Parse a raw quantifier header starting at `tokens[start]` (`forall` or `exists`).
pub(crate) fn parse_raw_quantifier_slice(
    tokens: &[String],
    start: usize,
) -> Option<RawQuantifierSlice> {
    if start >= tokens.len() {
        return None;
    }
    let tok = tokens[start].as_str();
    if (tok != "forall" && tok != "exists")
        || start + 4 >= tokens.len()
        || tokens[start + 2] != "in"
    {
        return None;
    }

    let mut delim_pos = start + 3;
    let mut depth = 0usize;
    while delim_pos < tokens.len() {
        match tokens[delim_pos].as_str() {
            "(" => depth += 1,
            ")" => depth = depth.saturating_sub(1),
            ":" | "{" if depth == 0 => break,
            _ => {}
        }
        delim_pos += 1;
    }

    if delim_pos >= tokens.len() || (tokens[delim_pos] != ":" && tokens[delim_pos] != "{") {
        return None;
    }

    let body_start = delim_pos + 1;
    let (body_end, final_pos) = if tokens[delim_pos] == "{" {
        let mut brace_depth = 1usize;
        let mut end = body_start;
        while end < tokens.len() && brace_depth > 0 {
            match tokens[end].as_str() {
                "{" => brace_depth += 1,
                "}" => brace_depth -= 1,
                _ => {}
            }
            if brace_depth > 0 {
                end += 1;
            }
        }
        (end, end + 1)
    } else {
        (tokens.len(), tokens.len())
    };

    Some(RawQuantifierSlice {
        is_forall: tok == "forall",
        var_token_idx: start + 1,
        body_start,
        body_end,
        final_pos,
    })
}

/// Split a parenthesized argument token list at top-level commas.
pub(crate) fn comma_chunk_ranges(tokens: &[String]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut arg_start_idx = 0usize;
    let mut depth = 0usize;
    for (i, t) in tokens.iter().enumerate() {
        match t.as_str() {
            "(" => depth += 1,
            ")" => depth = depth.saturating_sub(1),
            "," if depth == 0 => {
                if i > arg_start_idx {
                    ranges.push((arg_start_idx, i));
                }
                arg_start_idx = i + 1;
            }
            _ => {}
        }
    }
    if arg_start_idx < tokens.len() {
        ranges.push((arg_start_idx, tokens.len()));
    }
    ranges
}

/// Range-domain guard for AST quantifiers: `lo <= var < hi`.
pub(crate) fn range_guard_smtlib(var: &str, lo: &str, hi: &str) -> String {
    format!("(and (>= {var} {lo}) (< {var} {hi}))")
}

/// Collection-domain guard for AST quantifiers.
pub(crate) fn domain_contains_guard_smtlib(domain: &str, var: &str) -> String {
    format!("(__domain_contains {domain} {var})")
}

/// Wrap an AST quantifier body with the correct guard connective.
pub(crate) fn wrap_ast_quantifier_smtlib(
    is_forall: bool,
    var: &str,
    guard: &str,
    body: &str,
) -> String {
    let quant = if is_forall { "forall" } else { "exists" };
    let inner = if is_forall {
        format!("(=> {guard} {body})")
    } else {
        format!("(and {guard} {body})")
    };
    format!("({quant} (({var} Int)) {inner})")
}

/// Format a raw-token quantifier without domain encoding (body only).
pub(crate) fn format_raw_quantifier_smtlib(is_forall: bool, var: &str, body: &str) -> String {
    let quant = if is_forall { "forall" } else { "exists" };
    format!("({quant} (({var} Int)) {body})")
}

// -------------------------------------------------------------------------
// AST BinOp helpers (shared shell-out + native kind mapping)
// -------------------------------------------------------------------------

/// Extract `(lo, hi)` when a quantifier domain is a range expression.
pub(crate) fn domain_as_range(domain: &SpExpr) -> Option<(&SpExpr, &SpExpr)> {
    match &domain.node {
        Expr::BinOp {
            op: BinOp::Range,
            lhs,
            rhs,
        } => Some((lhs, rhs)),
        _ => None,
    }
}

/// Format a standard (non-special) AST binary operator as SMT-LIB2.
pub(crate) fn format_standard_ast_binop_smtlib(op: &BinOp, l: &str, r: &str) -> Option<String> {
    crate::encode_atom_policy::format_standard_ast_binop_smtlib(op, l, r)
}

pub(crate) fn format_neq_ast_binop_smtlib(l: &str, r: &str) -> String {
    crate::encode_atom_policy::format_neq_ast_binop_smtlib(l, r)
}

pub(crate) fn range_binop_smtlib(l: &str, r: &str) -> String {
    crate::encode_atom_policy::range_binop_smtlib(l, r)
}

pub(crate) fn in_binop_smtlib(elem: &str, coll: &str) -> String {
    crate::encode_atom_policy::in_binop_smtlib(elem, coll)
}

pub(crate) fn not_in_binop_smtlib(elem: &str, coll: &str) -> String {
    crate::encode_atom_policy::not_in_binop_smtlib(elem, coll)
}

pub(crate) fn concat_binop_smtlib(l: &str, r: &str) -> String {
    crate::encode_atom_policy::concat_binop_smtlib(l, r)
}

/// Map standard AST `BinOp` variants to native CVC5 kinds.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn standard_ast_binop_cvc5_kind(op: &BinOp) -> Option<cvc5::Kind> {
    match op {
        BinOp::Add => Some(cvc5::Kind::Add),
        BinOp::Sub => Some(cvc5::Kind::Sub),
        BinOp::Mul => Some(cvc5::Kind::Mult),
        BinOp::Div => Some(cvc5::Kind::IntsDivision),
        BinOp::Mod => Some(cvc5::Kind::IntsModulus),
        BinOp::Eq => Some(cvc5::Kind::Equal),
        BinOp::Lt => Some(cvc5::Kind::Lt),
        BinOp::Lte => Some(cvc5::Kind::Leq),
        BinOp::Gt => Some(cvc5::Kind::Gt),
        BinOp::Gte => Some(cvc5::Kind::Geq),
        BinOp::And => Some(cvc5::Kind::And),
        BinOp::Or => Some(cvc5::Kind::Or),
        BinOp::Implies => Some(cvc5::Kind::Implies),
        BinOp::Neq | BinOp::Range | BinOp::In | BinOp::NotIn | BinOp::Concat => None,
    }
}

/// Combine a quantifier domain guard with its body (native API).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn combine_quantifier_guard_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    is_forall: bool,
    guard: cvc5::Term<'a>,
    body: cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    if is_forall {
        tm.mk_term(cvc5::Kind::Implies, &[guard, body])
    } else {
        tm.mk_term(cvc5::Kind::And, &[guard, body])
    }
}

/// Apply a shared raw binary operator in the native CVC5 API.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn apply_raw_op_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    op: RawBinOp,
    lhs: cvc5::Term<'a>,
    rhs: cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    match op {
        RawBinOp::Add => tm.mk_term(cvc5::Kind::Add, &[lhs, rhs]),
        RawBinOp::Sub => tm.mk_term(cvc5::Kind::Sub, &[lhs, rhs]),
        RawBinOp::Mul => tm.mk_term(cvc5::Kind::Mult, &[lhs, rhs]),
        RawBinOp::Div => tm.mk_term(cvc5::Kind::IntsDivision, &[lhs, rhs]),
        RawBinOp::Mod => tm.mk_term(cvc5::Kind::IntsModulus, &[lhs, rhs]),
        RawBinOp::Eq => tm.mk_term(cvc5::Kind::Equal, &[lhs, rhs]),
        RawBinOp::Neq => {
            let eq = tm.mk_term(cvc5::Kind::Equal, &[lhs, rhs]);
            tm.mk_term(cvc5::Kind::Not, &[eq])
        }
        RawBinOp::Lt => tm.mk_term(cvc5::Kind::Lt, &[lhs, rhs]),
        RawBinOp::Leq => tm.mk_term(cvc5::Kind::Leq, &[lhs, rhs]),
        RawBinOp::Gt => tm.mk_term(cvc5::Kind::Gt, &[lhs, rhs]),
        RawBinOp::Geq => tm.mk_term(cvc5::Kind::Geq, &[lhs, rhs]),
        RawBinOp::And => tm.mk_term(cvc5::Kind::And, &[lhs, rhs]),
        RawBinOp::Or => tm.mk_term(cvc5::Kind::Or, &[lhs, rhs]),
        RawBinOp::Implies => tm.mk_term(cvc5::Kind::Implies, &[lhs, rhs]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_op_precedence_matches_shell_and_native() {
        assert_eq!(raw_op_info("||"), Some((1, RawBinOp::Or)));
        assert_eq!(raw_op_info("mod"), Some((11, RawBinOp::Mod)));
        assert_eq!(raw_op_info("unknown"), None);
    }

    #[test]
    fn neq_smtlib_uses_not_eq() {
        assert_eq!(
            format_raw_binop_smtlib(RawBinOp::Neq, "a", "b"),
            "(not (= a b))"
        );
    }

    #[test]
    fn comma_chunks_respect_nesting() {
        let nested: Vec<String> = vec![
            "(".into(),
            "x".into(),
            ",".into(),
            "y".into(),
            ")".into(),
            ",".into(),
            "z".into(),
        ];
        let ranges = comma_chunk_ranges(&nested);
        assert_eq!(ranges, vec![(0, 5), (6, 7)]);
    }

    #[test]
    fn parse_quantifier_brace_body() {
        let tokens: Vec<String> = "forall x in 0..10 { x >= 0 }"
            .split_whitespace()
            .map(String::from)
            .collect();
        let slice = parse_raw_quantifier_slice(&tokens, 0).unwrap();
        assert!(slice.is_forall);
        assert_eq!(slice.var_token_idx, 1);
        assert_eq!(&tokens[slice.body_start..slice.body_end], &["x", ">=", "0"]);
    }

    #[test]
    fn ast_quantifier_wrappers() {
        assert_eq!(
            wrap_ast_quantifier_smtlib(true, "x", "(and (>= x 0) (< x 10))", "(>= x 0)"),
            "(forall ((x Int)) (=> (and (>= x 0) (< x 10)) (>= x 0)))"
        );
        assert_eq!(
            wrap_ast_quantifier_smtlib(false, "x", "(and (>= x 0) (< x 10))", "(= x 5)"),
            "(exists ((x Int)) (and (and (>= x 0) (< x 10)) (= x 5)))"
        );
    }
}
