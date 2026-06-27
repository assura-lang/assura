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

/// Kind of AST unary for planning (Z3/CVC5 share arm order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AstUnaryKind {
    /// Logical not → `(not x)` / `Not` term
    Not,
    /// Arithmetic/bitwise negate → `(- x)` / `Neg` term (Z3 promotes Real vs Int locally)
    Neg,
}

/// Classify an AST `UnaryOp` for encode dispatch.
pub(crate) fn classify_ast_unary(op: &UnaryOp) -> AstUnaryKind {
    match op {
        UnaryOp::Not => AstUnaryKind::Not,
        UnaryOp::Neg => AstUnaryKind::Neg,
    }
}

/// Encode an AST unary operator as SMT-LIB2.
pub(crate) fn encode_ast_unary_smtlib(op: &UnaryOp, inner: &str) -> String {
    match classify_ast_unary(op) {
        AstUnaryKind::Not => format!("(not {inner})"),
        AstUnaryKind::Neg => format!("(- {inner})"),
    }
}

/// Kind of AST binop for planning (special forms vs standard SMT operator).
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

/// Whether an AST `BinOp` is a comparison operator (used for comparison chaining).
///
/// Comparison chaining: `a < b < c` → `(a < b) && (b < c)`.
/// The parser produces `BinOp(BinOp(a, <, b), <, c)`.  When a comparison's LHS
/// is itself a comparison, backends extract the shared middle operand and encode
/// as conjunction.
pub(crate) fn is_comparison_ast_binop(op: &BinOp) -> bool {
    matches!(
        op,
        BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte | BinOp::Eq | BinOp::Neq
    )
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
        assert_eq!(classify_ast_unary(&UnaryOp::Not), AstUnaryKind::Not);
        assert_eq!(classify_ast_unary(&UnaryOp::Neg), AstUnaryKind::Neg);
    }

    #[test]
    fn classify_ast_binop_arms() {
        assert_eq!(classify_ast_binop(&BinOp::Neq), AstBinOpKind::Neq);
        assert_eq!(classify_ast_binop(&BinOp::Range), AstBinOpKind::Range);
        assert_eq!(classify_ast_binop(&BinOp::In), AstBinOpKind::In);
        assert_eq!(classify_ast_binop(&BinOp::And), AstBinOpKind::Standard);
    }

    #[test]
    fn is_comparison_recognizes_relational_and_equality() {
        assert!(is_comparison_ast_binop(&BinOp::Lt));
        assert!(is_comparison_ast_binop(&BinOp::Lte));
        assert!(is_comparison_ast_binop(&BinOp::Gt));
        assert!(is_comparison_ast_binop(&BinOp::Gte));
        assert!(is_comparison_ast_binop(&BinOp::Eq));
        assert!(is_comparison_ast_binop(&BinOp::Neq));
        assert!(!is_comparison_ast_binop(&BinOp::Add));
        assert!(!is_comparison_ast_binop(&BinOp::And));
        assert!(!is_comparison_ast_binop(&BinOp::Or));
    }
}
