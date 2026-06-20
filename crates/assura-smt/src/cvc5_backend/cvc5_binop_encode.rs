//! Shared AST BinOp/UnaryOp encoding for CVC5 shell-out and native backends.

use assura_parser::ast::{BinOp, UnaryOp};

use crate::cvc5_raw_ops::{
    concat_binop_smtlib, format_neq_ast_binop_smtlib, format_standard_ast_binop_smtlib,
    in_binop_smtlib, not_in_binop_smtlib, range_binop_smtlib,
};

#[cfg(feature = "cvc5-verify")]
use crate::cvc5_encoder_state::{Cvc5EncoderState, field_len_fn_cvc5};
#[cfg(feature = "cvc5-verify")]
use crate::cvc5_native_binops::{
    encode_concat_binop_cvc5, encode_contains_binop_cvc5, encode_range_binop_cvc5,
};

/// Encode an AST binary operator as SMT-LIB2.
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

/// Encode an AST binary operator as a native CVC5 term.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_ast_binop_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    op: &BinOp,
    l: cvc5::Term<'a>,
    r: cvc5::Term<'a>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    match op {
        BinOp::Neq => {
            let eq = tm.mk_term(cvc5::Kind::Equal, &[l, r]);
            Some(tm.mk_term(cvc5::Kind::Not, &[eq]))
        }
        BinOp::Range => Some(encode_range_binop_cvc5(
            tm,
            &mut state.axioms,
            &mut state.fresh_counter,
            l,
            r,
        )),
        BinOp::In => Some(encode_contains_binop_cvc5(tm, r, l)),
        BinOp::NotIn => {
            let in_result = encode_contains_binop_cvc5(tm, r, l);
            Some(tm.mk_term(cvc5::Kind::Not, &[in_result]))
        }
        BinOp::Concat => {
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
        _ => {
            let kind = crate::cvc5_raw_ops::standard_ast_binop_cvc5_kind(op)?;
            Some(tm.mk_term(kind, &[l, r]))
        }
    }
}

/// Encode an AST unary operator as a native CVC5 term.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_ast_unary_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    op: &UnaryOp,
    inner: cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    match op {
        UnaryOp::Not => tm.mk_term(cvc5::Kind::Not, &[inner]),
        UnaryOp::Neg => tm.mk_term(cvc5::Kind::Neg, &[inner]),
    }
}

#[cfg(test)]
mod tests {
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
