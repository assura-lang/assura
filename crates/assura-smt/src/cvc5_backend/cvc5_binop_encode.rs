//! Shared AST BinOp/UnaryOp encoding for CVC5 shell-out and native backends.
//!
//! SMT-LIB **policy** lives in [`crate::encode_binop_policy`]; this module re-exports
//! it and keeps CVC5-native term construction.

// Stable import paths for historical `cvc5_binop_encode::*` callers (shell may use policy directly).
#[allow(
    unused_imports,
    reason = "re-export surface; cvc5_expr_smtlib prefers encode_binop_policy"
)]
pub(crate) use crate::encode_binop_policy::{encode_ast_binop_smtlib, encode_ast_unary_smtlib};

#[cfg(feature = "cvc5-verify")]
use crate::cvc5_encoder_state::{Cvc5EncoderState, field_len_fn_cvc5};
#[cfg(feature = "cvc5-verify")]
use crate::cvc5_native_binops::{
    encode_concat_binop_cvc5, encode_contains_binop_cvc5, encode_range_binop_cvc5,
};
#[cfg(feature = "cvc5-verify")]
use crate::encode_binop_policy::{AstBinOpKind, classify_ast_binop};
#[cfg(feature = "cvc5-verify")]
use assura_ast::{BinOp, UnaryOp};

/// Encode an AST binary operator as a native CVC5 term.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_ast_binop_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    op: &BinOp,
    l: cvc5::Term<'a>,
    r: cvc5::Term<'a>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    match classify_ast_binop(op) {
        AstBinOpKind::Neq => {
            let eq = tm.mk_term(cvc5::Kind::Equal, &[l, r]);
            Some(tm.mk_term(cvc5::Kind::Not, &[eq]))
        }
        AstBinOpKind::Range => Some(encode_range_binop_cvc5(
            tm,
            &mut state.axioms,
            &mut state.fresh_counter,
            l,
            r,
        )),
        AstBinOpKind::In => Some(encode_contains_binop_cvc5(tm, r, l)),
        AstBinOpKind::NotIn => {
            let in_result = encode_contains_binop_cvc5(tm, r, l);
            Some(tm.mk_term(cvc5::Kind::Not, &[in_result]))
        }
        AstBinOpKind::Concat => {
            let len_func = field_len_fn_cvc5(tm, state);
            Some(encode_concat_binop_cvc5(
                tm,
                &mut state.axioms,
                &mut state.fresh_counter,
                &len_func,
                l,
                r,
            ))
        }
        AstBinOpKind::Standard => {
            // Bitvector dispatch: if either operand is BV-sorted, use BV operations (#453).
            if (super::cvc5_bitvector_encode::is_bv(&l) || super::cvc5_bitvector_encode::is_bv(&r))
                && let Some(bv_kind) = bv_ast_binop_cvc5_kind(op)
            {
                return Some(tm.mk_term(bv_kind, &[l, r]));
            }
            let kind = crate::cvc5_raw_ops::standard_ast_binop_cvc5_kind(op)?;
            Some(tm.mk_term(kind, &[l, r]))
        }
        AstBinOpKind::Unsupported => None,
    }
}

/// Map an AST binary operator to its CVC5 bitvector Kind (if applicable).
#[cfg(feature = "cvc5-verify")]
fn bv_ast_binop_cvc5_kind(op: &BinOp) -> Option<cvc5::Kind> {
    match op {
        BinOp::Add => Some(cvc5::Kind::BitvectorAdd),
        BinOp::Sub => Some(cvc5::Kind::BitvectorSub),
        BinOp::Mul => Some(cvc5::Kind::BitvectorMult),
        // Default to unsigned comparisons (parity with Z3 which uses bvult/bvule).
        BinOp::Lt => Some(cvc5::Kind::BitvectorUlt),
        BinOp::Lte => Some(cvc5::Kind::BitvectorUle),
        BinOp::Gt => Some(cvc5::Kind::BitvectorUgt),
        BinOp::Gte => Some(cvc5::Kind::BitvectorUge),
        BinOp::Eq => Some(cvc5::Kind::Equal),
        _ => None,
    }
}

/// Encode an AST unary operator as a native CVC5 term.
///
/// Arm order via [`crate::encode_binop_policy::classify_ast_unary`] (parity with Z3).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_ast_unary_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    op: &UnaryOp,
    inner: cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    use crate::encode_binop_policy::{AstUnaryKind, classify_ast_unary};

    match classify_ast_unary(op) {
        AstUnaryKind::Not => {
            if super::cvc5_bitvector_encode::is_bv(&inner) {
                tm.mk_term(cvc5::Kind::BitvectorNot, &[inner])
            } else {
                tm.mk_term(cvc5::Kind::Not, &[inner])
            }
        }
        AstUnaryKind::Neg => {
            if super::cvc5_bitvector_encode::is_bv(&inner) {
                tm.mk_term(cvc5::Kind::BitvectorNeg, &[inner])
            } else {
                tm.mk_term(cvc5::Kind::Neg, &[inner])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use assura_ast::{BinOp, UnaryOp};

    use super::*;

    #[test]
    fn binop_add_smtlib() {
        assert_eq!(
            encode_ast_binop_smtlib(&BinOp::Add, "x", "1"),
            Some("(+ x 1)".into())
        );
    }

    #[test]
    fn unary_not_smtlib() {
        assert_eq!(encode_ast_unary_smtlib(&UnaryOp::Not, "flag"), "(not flag)");
    }
}
