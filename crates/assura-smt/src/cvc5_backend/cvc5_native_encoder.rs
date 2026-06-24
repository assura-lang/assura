//! Native CVC5 term encoding (feature = "cvc5-verify").
//!
//! Expression-to-term translation extracted from `cvc5_backend.rs`.

use std::collections::HashMap;

use assura_ast::{Expr, SpExpr};

use crate::cvc5_atom_encode::{encode_apply_cvc5, encode_ident_cvc5, encode_literal_cvc5};
use crate::cvc5_binop_encode::{encode_ast_binop_cvc5, encode_ast_unary_cvc5};
use crate::cvc5_call_encode::{encode_call_cvc5, encode_method_call_cvc5};
use crate::cvc5_encoder_state::{
    Cvc5QuantifierEncodeCtx, canonical_length_cvc5, field_len_fn_cvc5,
};
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
use crate::havoc_assume::HavocAssumeInput;

pub(crate) use crate::cvc5_encoder_state::{Cvc5EncoderState, default_cvc5_encoder_state};

use crate::cvc5_tuple_encode::encode_tuple_cvc5;
use crate::cvc5_wrapper_encode::encode_wrapper_cvc5;

// -------------------------------------------------------------------------
// Havoc+assume encoding (#267)
// -------------------------------------------------------------------------

#[cfg(feature = "cvc5-verify")]
pub(crate) fn apply_havoc_assume_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    input: &HavocAssumeInput<'a>,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) {
    use crate::havoc_assume::{HavocAssumeEffects, apply_havoc_assume_policy};
    use crate::ir::IrFunction;
    use crate::ir_encode::IrEncodeContext;

    // Structural axioms only; IR apply is done below so `IrEncodeContext<'a>`
    // shares the TermManager lifetime required by invariant `cvc5::Term<'a>`
    // / `HashMap` borrows (trait method only offers `IrEncodeContext<'_>`).
    struct Cvc5HavocEffects<'a, 'v, 's> {
        tm: &'a cvc5::TermManager,
        vars: &'v mut std::collections::HashMap<String, cvc5::Term<'a>>,
        state: &'s mut Cvc5EncoderState<'a>,
    }

    impl HavocAssumeEffects for Cvc5HavocEffects<'_, '_, '_> {
        fn collection_result_nonneg(&mut self) {
            let len = canonical_length_cvc5(self.tm, "result", self.vars, self.state);
            let zero = self.tm.mk_integer(0);
            self.state
                .axioms
                .push(self.tm.mk_term(cvc5::Kind::Geq, &[len, zero]));
        }

        fn length_identity_le(&mut self, result_name: &str, input_name: &str) {
            let len_result = canonical_length_cvc5(self.tm, result_name, self.vars, self.state);
            let len_input = canonical_length_cvc5(self.tm, input_name, self.vars, self.state);
            self.state
                .axioms
                .push(self.tm.mk_term(cvc5::Kind::Leq, &[len_result, len_input]));
        }

        fn apply_ir_body(
            &mut self,
            _func: &IrFunction,
            _param_names: &[String],
            _enc_ctx: IrEncodeContext<'_>,
        ) {
            // See apply_havoc_assume_cvc5 epilogue (lifetime alignment).
        }
    }

    let mut effects = Cvc5HavocEffects { tm, vars, state };
    apply_havoc_assume_policy(input, &mut effects);
    if let Some(func) = input.ir {
        apply_ir_body_constraints_cvc5(tm, func, input.param_names, vars, state, input.enc_ctx);
    }
}

/// Encode an AST expression as a CVC5 Term using the native API.
///
/// `state` collects background axioms and tracks string constants
/// so that `check_clause_cvc5_native` can assert them before check_sat.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_expr_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    expr: &SpExpr,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    match &expr.node {
        Expr::Literal(lit) => encode_literal_cvc5(tm, lit, state),
        Expr::Ident(name) => Some(encode_ident_cvc5(tm, name, vars)),
        Expr::BinOp { op, lhs, rhs } => {
            // Comparison chaining: a < b < c  =>  (a < b) && (b < c)
            // Parity with Z3 encode_binop (uses shared is_comparison_ast_binop).
            if crate::encode_binop_policy::is_comparison_ast_binop(op)
                && let Expr::BinOp {
                    lhs: inner_lhs,
                    op: inner_op,
                    rhs: inner_rhs,
                } = &lhs.node
                && crate::encode_binop_policy::is_comparison_ast_binop(inner_op)
            {
                let il = encode_expr_cvc5(tm, inner_lhs, &mut *vars, &mut *state)?;
                let mid = encode_expr_cvc5(tm, inner_rhs, &mut *vars, &mut *state)?;
                let r_val = encode_expr_cvc5(tm, rhs, &mut *vars, &mut *state)?;
                // Re-encode middle for right comparison (terms are ref-counted).
                let mid2 = encode_expr_cvc5(tm, inner_rhs, &mut *vars, &mut *state)?;
                let left_cmp = encode_ast_binop_cvc5(tm, inner_op, il, mid, state)?;
                let right_cmp = encode_ast_binop_cvc5(tm, op, mid2, r_val, state)?;
                return Some(tm.mk_term(cvc5::Kind::And, &[left_cmp, right_cmp]));
            }
            let l = encode_expr_cvc5(tm, lhs, &mut *vars, &mut *state)?;
            let r = encode_expr_cvc5(tm, rhs, &mut *vars, &mut *state)?;
            encode_ast_binop_cvc5(tm, op, l, r, state)
        }
        Expr::UnaryOp { op, expr: inner } => {
            let e = encode_expr_cvc5(tm, inner, &mut *vars, &mut *state)?;
            Some(encode_ast_unary_cvc5(tm, op, e))
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = encode_expr_cvc5(tm, cond, &mut *vars, &mut *state)?;
            let t = encode_expr_cvc5(tm, then_branch, &mut *vars, &mut *state)?;
            let e = else_branch
                .as_ref()
                .and_then(|eb| encode_expr_cvc5(tm, eb, &mut *vars, &mut *state));
            Some(encode_if_cvc5(tm, c, t, e))
        }
        Expr::Forall { var, domain, body } => {
            let mut qctx = Cvc5QuantifierEncodeCtx { tm, vars, state };
            encode_ast_quantifier_cvc5(&mut qctx, true, var, domain, body, |e, ctx| {
                encode_expr_cvc5(ctx.tm, e, ctx.vars, ctx.state)
            })
        }
        Expr::Exists { var, domain, body } => {
            let mut qctx = Cvc5QuantifierEncodeCtx { tm, vars, state };
            encode_ast_quantifier_cvc5(&mut qctx, false, var, domain, body, |e, ctx| {
                encode_expr_cvc5(ctx.tm, e, ctx.vars, ctx.state)
            })
        }
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = &func.as_ref().node {
                state.trigger_manager.register_function(name.clone());
            }
            encode_call_cvc5(tm, func, args, vars, state, |e, v, s| {
                encode_expr_cvc5(tm, e, v, s)
            })
        }
        // old(expr): add __old suffix for Ident, recurse for Field/MethodCall
        Expr::Old(inner) => encode_old_cvc5(tm, inner.as_ref(), vars, state, |e, v, s| {
            encode_expr_cvc5(tm, e, v, s)
        }),
        Expr::Ghost(inner) => {
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
            let coll_val = encode_expr_cvc5(tm, coll, &mut *vars, &mut *state)?;
            let idx_val = encode_expr_cvc5(tm, index, &mut *vars, &mut *state)?;
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
        } => {
            state.trigger_manager.register_function(method.clone());
            encode_method_call_cvc5(tm, receiver, method, args, vars, state, |e, v, s| {
                encode_expr_cvc5(tm, e, v, s)
            })
        }
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
