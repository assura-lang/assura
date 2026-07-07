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
            // Order ops use signed kinds when either side is a signed fixed-width
            // binding (or a BV term derived from one), matching Z3 (#858).
            if super::cvc5_bitvector_encode::is_bv(&l) || super::cvc5_bitvector_encode::is_bv(&r) {
                let signed = cvc5_bv_operand_signed(&l, state) || cvc5_bv_operand_signed(&r, state);
                if let Some(bv_kind) = bv_ast_binop_cvc5_kind(op, signed) {
                    return Some(tm.mk_term(bv_kind, &[l, r]));
                }
            }
            let kind = crate::cvc5_raw_ops::standard_ast_binop_cvc5_kind(op)?;
            Some(tm.mk_term(kind, &[l, r]))
        }
        AstBinOpKind::Unsupported => None,
    }
}

/// True if `term` is (or is built from) a signed fixed-width binding in `state`.
///
/// Walks free constant symbols and subterms so `x + 1` inherits signedness of `x`
/// when `x` was registered as `I8`/`I32`/… via `bv_signed` (#858).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn cvc5_bv_operand_signed(term: &cvc5::Term<'_>, state: &Cvc5EncoderState<'_>) -> bool {
    if term.has_symbol() && state.bv_signed.get(term.symbol()) == Some(&true) {
        return true;
    }
    let n = term.num_children();
    for i in 0..n {
        if cvc5_bv_operand_signed(&term.child(i), state) {
            return true;
        }
    }
    false
}

/// Map an AST binary operator to its CVC5 bitvector Kind (if applicable).
///
/// When `signed` is true, order comparisons use signed kinds (`BitvectorSlt`, …);
/// otherwise unsigned (`BitvectorUlt`, …). Arithmetic and equality are shared.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn bv_ast_binop_cvc5_kind(op: &BinOp, signed: bool) -> Option<cvc5::Kind> {
    match op {
        BinOp::Add => Some(cvc5::Kind::BitvectorAdd),
        BinOp::Sub => Some(cvc5::Kind::BitvectorSub),
        BinOp::Mul => Some(cvc5::Kind::BitvectorMult),
        BinOp::Lt if signed => Some(cvc5::Kind::BitvectorSlt),
        BinOp::Lt => Some(cvc5::Kind::BitvectorUlt),
        BinOp::Lte if signed => Some(cvc5::Kind::BitvectorSle),
        BinOp::Lte => Some(cvc5::Kind::BitvectorUle),
        BinOp::Gt if signed => Some(cvc5::Kind::BitvectorSgt),
        BinOp::Gt => Some(cvc5::Kind::BitvectorUgt),
        BinOp::Gte if signed => Some(cvc5::Kind::BitvectorSge),
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

    /// Kind selection for BV order is pure (no solver); always runs under default features
    /// when `cvc5-verify` is off these functions are not compiled — covered in native tests.
    #[cfg(feature = "cvc5-verify")]
    #[test]
    fn bv_ast_binop_kind_signed_vs_unsigned_order() {
        use assura_ast::BinOp;

        assert_eq!(
            bv_ast_binop_cvc5_kind(&BinOp::Lt, false),
            Some(cvc5::Kind::BitvectorUlt)
        );
        assert_eq!(
            bv_ast_binop_cvc5_kind(&BinOp::Lt, true),
            Some(cvc5::Kind::BitvectorSlt)
        );
        assert_eq!(
            bv_ast_binop_cvc5_kind(&BinOp::Lte, true),
            Some(cvc5::Kind::BitvectorSle)
        );
        assert_eq!(
            bv_ast_binop_cvc5_kind(&BinOp::Gt, true),
            Some(cvc5::Kind::BitvectorSgt)
        );
        assert_eq!(
            bv_ast_binop_cvc5_kind(&BinOp::Gte, true),
            Some(cvc5::Kind::BitvectorSge)
        );
        // Arithmetic/eq ignore signed flag.
        assert_eq!(
            bv_ast_binop_cvc5_kind(&BinOp::Add, true),
            Some(cvc5::Kind::BitvectorAdd)
        );
        assert_eq!(
            bv_ast_binop_cvc5_kind(&BinOp::Eq, false),
            Some(cvc5::Kind::Equal)
        );
    }
}
