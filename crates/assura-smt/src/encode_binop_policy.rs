//! Shared **AST BinOp / UnaryOp** SMT-LIB shapes (encode convergence step).
//!
//! Owns solver-neutral SMT-LIB2 text for standard and special AST operators
//! (`Neq`, `Range`, `In`, `NotIn`, `Concat`, and standard arithmetic/logic).
//! CVC5-native kind mapping and term construction stay in `cvc5_binop_encode` /
//! `cvc5_raw_ops` / Z3 `encode_binop`.
//!
//! Complements [`crate::encode_atom_policy`] (standard op names) and
//! [`crate::encode_raw_ops_policy`] (raw-token operators, not AST `BinOp`).

use assura_ast::{BinOp, UnaryOp};

use crate::encode_atom_policy::{
    concat_binop_smtlib, format_neq_ast_binop_smtlib, format_standard_ast_binop_smtlib,
    in_binop_smtlib, not_in_binop_smtlib, range_binop_smtlib,
};

/// Encode an AST binary operator as SMT-LIB2 (both operands already rendered).
pub(crate) fn encode_ast_binop_smtlib(op: &BinOp, l: &str, r: &str) -> Option<String> {
    match op {
        BinOp::Neq => Some(format_neq_ast_binop_smtlib(l, r)),
        BinOp::Range => Some(range_binop_smtlib(l, r)),
        BinOp::In => Some(in_binop_smtlib(l, r)),
        BinOp::NotIn => Some(not_in_binop_smtlib(l, r)),
        BinOp::Concat => Some(concat_binop_smtlib(l, r)),
        _ => format_standard_ast_binop_smtlib(op, l, r),
    }
}

/// Encode an AST unary operator as SMT-LIB2.
pub(crate) fn encode_ast_unary_smtlib(op: &UnaryOp, inner: &str) -> String {
    match op {
        UnaryOp::Not => format!("(not {inner})"),
        UnaryOp::Neg => format!("(- {inner})"),
    }
}

/// Kind of AST binop for planning (special forms vs standard SMT operator).
#[cfg_attr(
    not(any(test, feature = "cvc5-verify")),
    allow(
        dead_code,
        reason = "native CVC5/Z3 dispatch; shell uses encode_ast_binop_smtlib"
    )
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AstBinOpKind {
    /// `!=` → `(not (= l r))`
    Neq,
    /// `..` range (backend axioms / UF)
    Range,
    /// `in` / membership
    In,
    /// `not in`
    NotIn,
    /// `++` / concat
    Concat,
    /// Standard SMT-LIB operator (`+`, `and`, `=>`, …)
    Standard,
    /// Unsupported / no direct SMT-LIB rendering
    Unsupported,
}

/// Classify an AST `BinOp` for encode dispatch (Z3/CVC5 share this order of arms).
#[cfg_attr(
    not(any(test, feature = "cvc5-verify")),
    allow(
        dead_code,
        reason = "native CVC5/Z3 dispatch; shell uses encode_ast_binop_smtlib"
    )
)]
pub(crate) fn classify_ast_binop(op: &BinOp) -> AstBinOpKind {
    match op {
        BinOp::Neq => AstBinOpKind::Neq,
        BinOp::Range => AstBinOpKind::Range,
        BinOp::In => AstBinOpKind::In,
        BinOp::NotIn => AstBinOpKind::NotIn,
        BinOp::Concat => AstBinOpKind::Concat,
        _ if format_standard_ast_binop_smtlib(op, "a", "b").is_some() => AstBinOpKind::Standard,
        _ => AstBinOpKind::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ast_binop_smtlib_specials() {
        assert_eq!(
            encode_ast_binop_smtlib(&BinOp::Neq, "x", "y").as_deref(),
            Some("(not (= x y))")
        );
        assert_eq!(
            encode_ast_binop_smtlib(&BinOp::Add, "1", "2").as_deref(),
            Some("(+ 1 2)")
        );
        assert_eq!(encode_ast_unary_smtlib(&UnaryOp::Not, "p"), "(not p)");
        assert_eq!(encode_ast_unary_smtlib(&UnaryOp::Neg, "3"), "(- 3)");
    }

    #[test]
    fn classify_ast_binop_arms() {
        assert_eq!(classify_ast_binop(&BinOp::Neq), AstBinOpKind::Neq);
        assert_eq!(classify_ast_binop(&BinOp::Range), AstBinOpKind::Range);
        assert_eq!(classify_ast_binop(&BinOp::In), AstBinOpKind::In);
        assert_eq!(classify_ast_binop(&BinOp::And), AstBinOpKind::Standard);
    }
}
