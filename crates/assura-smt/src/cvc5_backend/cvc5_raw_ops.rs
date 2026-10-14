//! Shared raw-token operator table and quantifier helpers for CVC5 backends.
//!
//! Shell-out (`cvc5_raw_smtlib`) and native (`cvc5_raw_native`)
//! (`encode_expr_cvc5` / `parse_raw_expr_cvc5`) share precedence, comparison
//! chaining, quantifier wrapping, comma-splitting, and AST `BinOp` tables.
//!
//! Operator/quantifier **policy** lives in [`crate::encode_raw_ops_policy`]; this
//! module re-exports stable names and keeps CVC5-native kind mapping + AST domain
//! helpers that depend on `SpExpr`.

#[cfg(feature = "cvc5-verify")]
use assura_ast::BinOp;

// -------------------------------------------------------------------------
// Re-exports: encode_raw_ops_policy (encode convergence step 2)
// -------------------------------------------------------------------------
// Stable names for `cvc5_raw_smtlib` / `cvc5_raw_native` / `cvc5_quantifier_encode`.
// Types (`RawBinOp`, `RawQuantifierSlice`, `RAW_SPEC_SKIP_KEYWORDS`) stay in
// `encode_raw_ops_policy`; only functions re-exported here are crate-used.

pub(crate) use crate::encode_raw_ops_policy::{
    comma_chunk_ranges, find_matching_delim, format_raw_binop_smtlib, format_raw_quantifier_smtlib,
    is_raw_spec_skip_keyword, parse_raw_quantifier_slice, raw_op_info, raw_op_is_comparison,
};

// Test-only re-export (policy owns impls; production callers use encode_quantifier_policy).
#[cfg(test)]
pub(crate) use crate::encode_raw_ops_policy::wrap_ast_quantifier_smtlib;

// AST binop SMT-LIB text: prefer `encode_binop_policy` / `cvc5_binop_encode` re-exports.
// Keep atom helpers available only when CVC5-native binop tests/kinds need direct access.
#[cfg(any(test, feature = "cvc5-verify"))]
#[allow(
    unused_imports,
    reason = "cvc5-verify / tests; shell goes through encode_binop_policy"
)]
pub(crate) use crate::encode_atom_policy::{
    concat_binop_smtlib, format_neq_ast_binop_smtlib, format_standard_ast_binop_smtlib,
    in_binop_smtlib, not_in_binop_smtlib, range_binop_smtlib,
};

// domain_as_range: import encode_quantifier_policy directly (not via this shim).

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
pub(crate) fn combine_quantifier_guard_cvc5(
    tm: &mut cvc5::TermManager,
    is_forall: bool,
    guard: cvc5::Term,
    body: cvc5::Term,
) -> cvc5::Term {
    if is_forall {
        tm.mk_term(cvc5::Kind::Implies, &[guard, body])
            .expect("cvc5")
    } else {
        tm.mk_term(cvc5::Kind::And, &[guard, body]).expect("cvc5")
    }
}

/// Apply a shared raw binary operator in the native CVC5 API.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn apply_raw_op_cvc5(
    tm: &mut cvc5::TermManager,
    op: crate::encode_raw_ops_policy::RawBinOp,
    lhs: cvc5::Term,
    rhs: cvc5::Term,
) -> cvc5::Term {
    use crate::encode_raw_ops_policy::RawBinOp;
    match op {
        RawBinOp::Add => tm.mk_term(cvc5::Kind::Add, &[lhs, rhs]).expect("cvc5"),
        RawBinOp::Sub => tm.mk_term(cvc5::Kind::Sub, &[lhs, rhs]).expect("cvc5"),
        RawBinOp::Mul => tm.mk_term(cvc5::Kind::Mult, &[lhs, rhs]).expect("cvc5"),
        RawBinOp::Div if lhs.sort().is_integer() && rhs.sort().is_integer() => {
            rust_trunc_div_cvc5(tm, lhs, rhs)
        }
        RawBinOp::Mod if lhs.sort().is_integer() && rhs.sort().is_integer() => {
            rust_trunc_mod_cvc5(tm, lhs, rhs)
        }
        RawBinOp::Div => tm
            .mk_term(cvc5::Kind::IntsDivision, &[lhs, rhs])
            .expect("cvc5"),
        RawBinOp::Mod => tm
            .mk_term(cvc5::Kind::IntsModulus, &[lhs, rhs])
            .expect("cvc5"),
        RawBinOp::Eq => tm.mk_term(cvc5::Kind::Equal, &[lhs, rhs]).expect("cvc5"),
        RawBinOp::Neq => {
            let eq = tm.mk_term(cvc5::Kind::Equal, &[lhs, rhs]).expect("cvc5");
            tm.mk_term(cvc5::Kind::Not, &[eq]).expect("cvc5")
        }
        RawBinOp::Lt => tm.mk_term(cvc5::Kind::Lt, &[lhs, rhs]).expect("cvc5"),
        RawBinOp::Leq => tm.mk_term(cvc5::Kind::Leq, &[lhs, rhs]).expect("cvc5"),
        RawBinOp::Gt => tm.mk_term(cvc5::Kind::Gt, &[lhs, rhs]).expect("cvc5"),
        RawBinOp::Geq => tm.mk_term(cvc5::Kind::Geq, &[lhs, rhs]).expect("cvc5"),
        RawBinOp::And => tm.mk_term(cvc5::Kind::And, &[lhs, rhs]).expect("cvc5"),
        RawBinOp::Or => tm.mk_term(cvc5::Kind::Or, &[lhs, rhs]).expect("cvc5"),
        RawBinOp::Implies => tm.mk_term(cvc5::Kind::Implies, &[lhs, rhs]).expect("cvc5"),
    }
}

/// Toward-zero quotient. `IntsDivision` floors; add one when signs differ.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn rust_trunc_div_cvc5(
    tm: &mut cvc5::TermManager,
    lhs: cvc5::Term,
    rhs: cvc5::Term,
) -> cvc5::Term {
    let q = tm
        .mk_term(cvc5::Kind::IntsDivision, &[lhs.clone(), rhs.clone()])
        .expect("cvc5");
    let remainder = tm
        .mk_term(cvc5::Kind::IntsModulus, &[lhs.clone(), rhs.clone()])
        .expect("cvc5");
    let zero = tm.mk_integer(0);
    let exact = tm
        .mk_term(cvc5::Kind::Equal, &[remainder, zero.clone()])
        .expect("cvc5");
    let lhs_neg = tm
        .mk_term(cvc5::Kind::Lt, &[lhs, zero.clone()])
        .expect("cvc5");
    let rhs_neg = tm.mk_term(cvc5::Kind::Lt, &[rhs, zero]).expect("cvc5");
    let same_sign = tm
        .mk_term(cvc5::Kind::Equal, &[lhs_neg, rhs_neg])
        .expect("cvc5");
    let cond = tm
        .mk_term(cvc5::Kind::Or, &[exact, same_sign])
        .expect("cvc5");
    let q_plus = {
        let __cvc0 = tm.mk_integer(1);
        tm.mk_term(cvc5::Kind::Add, &[q.clone(), __cvc0])
    }
    .expect("cvc5");
    tm.mk_term(cvc5::Kind::Ite, &[cond, q, q_plus])
        .expect("cvc5")
}

/// Remainder with the sign of the dividend, matching Rust `%`.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn rust_trunc_mod_cvc5(
    tm: &mut cvc5::TermManager,
    lhs: cvc5::Term,
    rhs: cvc5::Term,
) -> cvc5::Term {
    let q = rust_trunc_div_cvc5(tm, lhs.clone(), rhs.clone());
    let prod = tm.mk_term(cvc5::Kind::Mult, &[rhs, q]).expect("cvc5");
    tm.mk_term(cvc5::Kind::Sub, &[lhs, prod]).expect("cvc5")
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
        use crate::encode_quantifier_policy::domain_as_range;
        use assura_ast::{BinOp, Expr, Spanned};
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
