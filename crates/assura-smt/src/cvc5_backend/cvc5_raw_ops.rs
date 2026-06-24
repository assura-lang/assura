//! Shared raw-token operator table and quantifier helpers for CVC5 backends.
//!
//! Shell-out (`cvc5_raw_smtlib`) and native (`cvc5_raw_native`)
//! (`encode_expr_cvc5` / `parse_raw_expr_cvc5`) share precedence, comparison
//! chaining, quantifier wrapping, comma-splitting, and AST `BinOp` tables.
//!
//! Operator/quantifier **policy** lives in [`crate::encode_raw_ops_policy`]; this
//! module re-exports stable names and keeps CVC5-native kind mapping + AST domain
//! helpers that depend on `SpExpr`.

use assura_ast::{BinOp, Expr, SpExpr};

// -------------------------------------------------------------------------
// Re-exports: encode_raw_ops_policy (encode convergence step 2)
// -------------------------------------------------------------------------
// Stable names for `cvc5_raw_smtlib` / `cvc5_raw_native` / `cvc5_quantifier_encode`.
// Types (`RawBinOp`, `RawQuantifierSlice`, `RAW_SPEC_SKIP_KEYWORDS`) stay in
// `encode_raw_ops_policy`; only functions re-exported here are crate-used.

pub(crate) use crate::encode_raw_ops_policy::{
    comma_chunk_ranges, domain_contains_guard_smtlib, find_matching_delim, format_raw_binop_smtlib,
    format_raw_quantifier_smtlib, is_raw_spec_skip_keyword, parse_raw_quantifier_slice,
    range_guard_smtlib, raw_op_info, raw_op_is_comparison, wrap_ast_quantifier_smtlib,
};

// AST binop SMT-LIB text: encode_atom_policy
pub(crate) use crate::encode_atom_policy::{
    concat_binop_smtlib, format_neq_ast_binop_smtlib, format_standard_ast_binop_smtlib,
    in_binop_smtlib, not_in_binop_smtlib, range_binop_smtlib,
};

// -------------------------------------------------------------------------
// AST domain helpers (SpExpr — not pure policy)
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

// -------------------------------------------------------------------------
// AST BinOp → CVC5 Kind (native only)
// -------------------------------------------------------------------------

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
    op: crate::encode_raw_ops_policy::RawBinOp,
    lhs: cvc5::Term<'a>,
    rhs: cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    use crate::encode_raw_ops_policy::RawBinOp;
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
    use crate::encode_raw_ops_policy::RawBinOp;

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
    }

    #[test]
    fn domain_as_range_extracts_bounds() {
        use assura_ast::Spanned;
        let lo = Spanned::no_span(Expr::Ident("a".into()));
        let hi = Spanned::no_span(Expr::Ident("b".into()));
        let domain = Spanned::no_span(Expr::BinOp {
            op: BinOp::Range,
            lhs: Box::new(lo),
            rhs: Box::new(hi),
        });
        let (l, r) = domain_as_range(&domain).expect("range");
        assert!(matches!(&l.node, Expr::Ident(n) if n == "a"));
        assert!(matches!(&r.node, Expr::Ident(n) if n == "b"));
    }
}
