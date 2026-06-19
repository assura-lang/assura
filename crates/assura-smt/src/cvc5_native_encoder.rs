//! Native CVC5 term encoding (feature = "cvc5-verify").
//!
//! Expression-to-term translation extracted from `cvc5_backend.rs`.

use std::collections::HashMap;

use assura_parser::ast::{Clause, Expr};

use crate::cvc5_atom_encode::{encode_apply_cvc5, encode_ident_cvc5, encode_literal_cvc5};
use crate::cvc5_binop_encode::{encode_ast_binop_cvc5, encode_ast_unary_cvc5};
use crate::cvc5_call_encode::{encode_call_cvc5, encode_method_call_cvc5};
use crate::cvc5_encoder_state::{canonical_length_cvc5, field_len_fn_cvc5};
use crate::cvc5_field_access::encode_field_cvc5;
use crate::cvc5_if_encode::encode_if_cvc5;
use crate::cvc5_index_access::encode_index_access_cvc5;
use crate::cvc5_ir_native::apply_ir_body_constraints_cvc5;
use crate::cvc5_let_block_encode::{encode_block_cvc5, encode_let_cvc5};
use crate::cvc5_list_encode::encode_list_cvc5;
use crate::cvc5_match_encode::encode_match_cvc5;
use crate::cvc5_old_access::encode_old_cvc5;
use crate::cvc5_quantifier_encode::encode_ast_quantifier_cvc5;
use crate::cvc5_raw_encode::encode_raw_expr_cvc5;

pub(crate) use crate::cvc5_encoder_state::{Cvc5EncoderState, default_cvc5_encoder_state};

use crate::cvc5_tuple_encode::encode_tuple_cvc5;
use crate::cvc5_wrapper_encode::encode_wrapper_cvc5;

// -------------------------------------------------------------------------
// Havoc+assume encoding (#267)
// -------------------------------------------------------------------------

#[cfg(feature = "cvc5-verify")]
pub(crate) fn apply_havoc_assume_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    requires: &[&Clause],
    ensures: &[&Clause],
    return_ty: &[String],
    param_names: &[String],
    ir: Option<&crate::ir::IrFunction>,
    ir_blocks: Option<&std::collections::HashMap<usize, Vec<crate::ir::IrInstr>>>,
    type_env: Option<&'a assura_types::TypeEnv>,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) {
    use crate::havoc_assume::{infer_length_identity_links, is_collection_return};

    if is_collection_return(return_ty) {
        let len = canonical_length_cvc5(tm, "result", vars, state);
        let zero = tm.mk_integer(0);
        state.axioms.push(tm.mk_term(cvc5::Kind::Geq, &[len, zero]));
    }

    for (result, input) in infer_length_identity_links(requires, ensures) {
        let len_result = canonical_length_cvc5(tm, &result, vars, state);
        let len_input = canonical_length_cvc5(tm, &input, vars, state);
        state
            .axioms
            .push(tm.mk_term(cvc5::Kind::Leq, &[len_result, len_input]));
    }

    if let Some(func) = ir {
        apply_ir_body_constraints_cvc5(
            tm,
            func,
            param_names,
            vars,
            state,
            ir_blocks,
            crate::ir_type_ctx::IrTypeContext::from_type_env(type_env),
        );
    }
}

/// Encode an AST expression as a CVC5 Term using the native API.
///
/// `state` collects background axioms and tracks string constants
/// so that `check_clause_cvc5_native` can assert them before check_sat.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_expr_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    expr: &Expr,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    match expr {
        Expr::Literal(lit) => encode_literal_cvc5(tm, lit, state),
        Expr::Ident(name) => Some(encode_ident_cvc5(tm, name, vars)),
        Expr::BinOp { op, lhs, rhs } => {
            let l = encode_expr_cvc5(tm, lhs, vars, state)?;
            let r = encode_expr_cvc5(tm, rhs, vars, state)?;
            encode_ast_binop_cvc5(tm, op, l, r, state)
        }
        Expr::UnaryOp { op, expr: inner } => {
            let e = encode_expr_cvc5(tm, inner, vars, state)?;
            Some(encode_ast_unary_cvc5(tm, op, e))
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = encode_expr_cvc5(tm, cond, vars, state)?;
            let t = encode_expr_cvc5(tm, then_branch, vars, state)?;
            let e = else_branch
                .as_ref()
                .and_then(|eb| encode_expr_cvc5(tm, eb, vars, state));
            Some(encode_if_cvc5(tm, c, t, e))
        }
        Expr::Forall { var, domain, body } => {
            encode_ast_quantifier_cvc5(tm, true, var, domain, body, vars, state, |e, v, s| {
                encode_expr_cvc5(tm, e, v, s)
            })
        }
        Expr::Exists { var, domain, body } => {
            encode_ast_quantifier_cvc5(tm, false, var, domain, body, vars, state, |e, v, s| {
                encode_expr_cvc5(tm, e, v, s)
            })
        }
        Expr::Call { func, args } => encode_call_cvc5(tm, func, args, vars, state, |e, v, s| {
            encode_expr_cvc5(tm, e, v, s)
        }),
        // old(expr): add __old suffix for Ident, recurse for Field/MethodCall
        Expr::Old(inner) => encode_old_cvc5(tm, inner.as_ref(), vars, state, |e, v, s| {
            encode_expr_cvc5(tm, e, v, s)
        }),
        Expr::Paren(inner) | Expr::Ghost(inner) => {
            encode_wrapper_cvc5(inner, vars, state, |e, v, s| encode_expr_cvc5(tm, e, v, s))
        }
        Expr::Cast { expr: inner, .. } => {
            encode_wrapper_cvc5(inner, vars, state, |e, v, s| encode_expr_cvc5(tm, e, v, s))
        }
        Expr::Let {
            name, value, body, ..
        } => encode_let_cvc5(tm, name, value, body, vars, state, |e, v, s| {
            encode_expr_cvc5(tm, e, v, s)
        }),
        Expr::Match {
            scrutinee, arms, ..
        } => encode_match_cvc5(tm, scrutinee, arms, vars, state, |e, v, s| {
            encode_expr_cvc5(tm, e, v, s)
        }),
        Expr::Field(obj, field) => encode_field_cvc5(tm, obj, field, vars, state, |e, v, s| {
            encode_expr_cvc5(tm, e, v, s)
        }),
        // Index: UF __index(collection, index) with bounds axioms
        Expr::Index { expr: coll, index } => {
            let coll_val = encode_expr_cvc5(tm, coll, vars, state)?;
            let idx_val = encode_expr_cvc5(tm, index, vars, state)?;
            Some(encode_index_access_cvc5(
                tm,
                coll_val,
                idx_val,
                &mut state.axioms,
            ))
        }
        Expr::Block(body) => encode_block_cvc5(tm, body, vars, state, |e, v, s| {
            encode_expr_cvc5(tm, e, v, s)
        }),
        // Raw tokens: basic parsing (single token bools/ints/idents)
        Expr::Raw(tokens) => encode_raw_expr_cvc5(tm, tokens, vars, state),
        // Tuple: fresh Int with element-access axioms
        Expr::Tuple(elems) => {
            let elem_vals: Option<Vec<_>> = elems
                .iter()
                .map(|elem| encode_expr_cvc5(tm, elem, vars, state))
                .collect();
            let elem_vals = elem_vals?;
            Some(encode_tuple_cvc5(
                tm,
                &elem_vals,
                &mut state.axioms,
                &mut state.fresh_counter,
            ))
        }
        // MethodCall: prepend receiver, call UF
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => encode_method_call_cvc5(tm, receiver, method, args, vars, state, |e, v, s| {
            encode_expr_cvc5(tm, e, v, s)
        }),
        // List: fresh Int with element-access and length axioms
        Expr::List(elems) => {
            let elem_vals: Option<Vec<_>> = elems
                .iter()
                .map(|elem| encode_expr_cvc5(tm, elem, vars, state))
                .collect();
            let elem_vals = elem_vals?;
            let len_func = field_len_fn_cvc5(tm, state);
            Some(encode_list_cvc5(
                tm,
                &elem_vals,
                &mut state.axioms,
                &mut state.fresh_counter,
                &len_func,
            ))
        }
        // Apply: encode args for side effects, return named bool
        Expr::Apply { lemma_name, args } => {
            encode_apply_cvc5(tm, lemma_name, args, vars, state, |e, v, s| {
                encode_expr_cvc5(tm, e, v, s)
            })
        }
    }
}
