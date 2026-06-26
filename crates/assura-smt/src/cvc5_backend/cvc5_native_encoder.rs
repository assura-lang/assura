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
/// Delegates to [`encode_expr_shared`] which handles AST dispatch via
/// the [`EncodeTerm`] trait. All expression encoding goes through the
/// shared path; backend-specific term construction is in
/// `cvc5_encode_term_impl.rs`.
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
    use crate::cvc5_backend::cvc5_encode_term_impl::Cvc5TermBuilder;
    let mut builder = Cvc5TermBuilder { tm, vars, state };
    crate::encode_term::encode_expr_shared(&mut builder, expr)
}
