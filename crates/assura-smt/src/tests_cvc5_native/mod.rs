use crate::VerificationResult;
use crate::cache::SessionCache;
use crate::cvc5_backend::*;
use crate::cvc5_quantifier_encode::infer_quantifier_patterns_cvc5;
use crate::verify_context::{ContractVerifyContext, LoadedIrContext};
use assura_ast::{BinOp, Clause, ClauseKind, Expr, Literal, Param, Pattern, Spanned, UnaryOp};
use std::collections::HashSet;

#[cfg(feature = "cvc5-verify")]
fn verify_lemmas_test(
    contract_name: &str,
    clauses: &[Clause],
    params: &[Param],
    return_ty: &[String],
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&assura_ast::SpExpr>>>,
    ir_body: Option<&crate::ir::IrFunction>,
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    let ctx = ContractVerifyContext {
        contract_name,
        clauses,
        params,
        return_ty,
        constants: &[],
        ir: ir_body.map(LoadedIrContext::with_body),
    };
    verify_contract_cvc5_with_lemmas(&ctx, lemma_defs, cache)
}

/// Shared test helper: build a [`Clause`] from kind and body expression.
#[cfg(feature = "cvc5-verify")]
fn make_clause(kind: ClauseKind, body: Expr) -> Clause {
    Clause {
        kind,
        body: Spanned::no_span(body),
        effect_variables: vec![],
    }
}

mod native;
mod match_patterns;
mod frame;
mod measures;
mod batch2_policy;
mod bitvector;
mod parity_468;
mod theory_parity;
