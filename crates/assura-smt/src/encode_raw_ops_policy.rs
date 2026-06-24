//! Shared **raw-token / quantifier / operator** encode policy (encode convergence step 2).
//!
//! Owns Pratt-style raw operator tables, SMT-LIB quantifier/range-guard shapes, and
//! token-slice utilities used by CVC5 shell/native raw parsers. Complements
//! [`crate::encode_atom_policy`] (atoms/names/standard AST `BinOp` SMT-LIB text).
//!
//! Still **not** full `Expr` → solver term encode: Z3 `Encoder` and CVC5 term builders
//! remain separate; only operator/quantifier **policy and text shapes** live here.

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

/// SMT-LIB2 quantifier over `Int` (raw / shell-out default sort).
pub(crate) fn format_raw_quantifier_smtlib(is_forall: bool, var: &str, body: &str) -> String {
    let quant = if is_forall { "forall" } else { "exists" };
    format!("({quant} (({var} Int)) {body})")
}

/// Collection-domain guard for AST quantifiers (`__domain_contains`).
///
/// UF name matches [`crate::encode_quantifier_policy::DOMAIN_CONTAINS_UF_NAME`].
pub(crate) fn domain_contains_guard_smtlib(domain: &str, var: &str) -> String {
    format!("(__domain_contains {domain} {var})")
}

/// Wrap an AST quantifier body with the correct guard connective.
///
/// Forall: `(forall ((var Int)) (=> guard body))`
/// Exists: `(exists ((var Int)) (and guard body))`
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_op_precedence_and_chain_flag() {
        let (p, op) = raw_op_info("&&").expect("and");
        assert_eq!(p, 3);
        assert_eq!(op, RawBinOp::And);
        assert!(raw_op_is_comparison(RawBinOp::Lt));
        assert!(!raw_op_is_comparison(RawBinOp::Add));
    }

    #[test]
    fn format_raw_binop_shapes() {
        assert_eq!(
            format_raw_binop_smtlib(RawBinOp::Neq, "a", "b"),
            "(not (= a b))"
        );
        assert_eq!(
            format_raw_binop_smtlib(RawBinOp::Implies, "p", "q"),
            "(=> p q)"
        );
    }

    #[test]
    fn quantifier_slice_and_smtlib() {
        let tokens: Vec<String> = "forall x in 0 .. n : x >= 0"
            .split_whitespace()
            .map(str::to_string)
            .collect();
        // tokens won't match because `..` may be one token in real lexer; use explicit list
        let tokens: Vec<String> = vec![
            "forall".into(),
            "x".into(),
            "in".into(),
            "0".into(),
            "..".into(),
            "n".into(),
            ":".into(),
            "x".into(),
            ">=".into(),
            "0".into(),
        ];
        let slice = parse_raw_quantifier_slice(&tokens, 0).expect("slice");
        assert!(slice.is_forall);
        assert_eq!(slice.var_token_idx, 1);
        assert_eq!(slice.body_start, 7);
        assert_eq!(
            format_raw_quantifier_smtlib(true, "x", "(>= x 0)"),
            "(forall ((x Int)) (>= x 0))"
        );
        assert_eq!(range_guard_smtlib("x", "0", "n"), "(and (>= x 0) (< x n))");
    }

    #[test]
    fn comma_chunks_and_delim() {
        let toks: Vec<String> = vec![
            "(".into(),
            "a".into(),
            ",".into(),
            "b".into(),
            ")".into(),
            ",".into(),
            "c".into(),
        ];
        let ranges = comma_chunk_ranges(&toks);
        assert_eq!(ranges.len(), 2);
        let inner = vec!["(".into(), "x".into(), ")".into()];
        assert_eq!(find_matching_delim(&inner, 0, "(", ")"), Some(2));
    }

    #[test]
    fn skip_keywords() {
        assert!(is_raw_spec_skip_keyword("ghost"));
        assert!(!is_raw_spec_skip_keyword("x"));
    }

    #[test]
    fn ast_quantifier_wrappers() {
        assert_eq!(
            wrap_ast_quantifier_smtlib(true, "x", "(and (>= x 0) (< x 10))", "(>= x 0)"),
            "(forall ((x Int)) (=> (and (>= x 0) (< x 10)) (>= x 0)))"
        );
        assert_eq!(
            domain_contains_guard_smtlib("xs", "x"),
            "(__domain_contains xs x)"
        );
    }
}
