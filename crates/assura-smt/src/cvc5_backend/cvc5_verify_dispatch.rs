//! CVC5 verify entry-point dispatch (native vs shell-out).

use assura_parser::ast::Clause;

use crate::VerificationResult;
use crate::cache::SessionCache;
use crate::cvc5_verify_shared::prepare_cvc5_contract_verification;
use crate::verify_context::{ContractVerifyContext, Cvc5ContractVerifySession};

/// Verify a single contract's clauses using CVC5.
///
/// When the `cvc5-verify` feature is enabled, uses the native Rust cvc5
/// crate (direct API calls, no process spawning). Otherwise falls back to
/// generating SMT-LIB2 text and invoking the `cvc5` binary.
///
/// This variant extracts params from `input()` clauses. For function
/// definitions whose params live in `FnDef.params`, use
/// `verify_contract_cvc5_with_types` instead.
pub(crate) fn verify_contract_cvc5(
    contract_name: &str,
    clauses: &[Clause],
) -> Vec<VerificationResult> {
    let params = crate::entry::extract_input_params(clauses);
    let return_ty = crate::entry::extract_output_return_type(clauses);
    let mut cache = SessionCache::new();
    verify_contract_cvc5_with_types(contract_name, clauses, &params, &return_ty, &mut cache)
}

/// Verify a single contract's clauses using CVC5 with explicit type info.
pub(crate) fn verify_contract_cvc5_with_types(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    verify_contract_cvc5_with_full_context(contract_name, clauses, params, return_ty, &[], cache)
}

/// Verify a single contract's clauses using CVC5 with full context.
pub(crate) fn verify_contract_cvc5_with_full_context(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    let ctx = ContractVerifyContext {
        contract_name,
        clauses,
        params,
        return_ty,
        constants,
        ir: None,
    };
    verify_contract_cvc5_with_lemmas(&ctx, None, cache)
}

/// Verify a single contract's clauses using CVC5, with optional lemma defs.
pub(crate) fn verify_contract_cvc5_with_lemmas(
    ctx: &ContractVerifyContext<'_>,
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&assura_parser::ast::SpExpr>>>,
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    let (mut results, prepared) = prepare_cvc5_contract_verification(
        ctx.contract_name,
        ctx.clauses,
        ctx.params,
        ctx.constants,
    );
    let mut session = Cvc5ContractVerifySession::new(ctx, prepared, lemma_defs, cache);

    #[cfg(feature = "cvc5-verify")]
    {
        results.extend(crate::cvc5_verify_native::verify_contract_cvc5_native(
            &mut session,
        ));
    }
    #[cfg(not(feature = "cvc5-verify"))]
    {
        results.extend(crate::cvc5_verify_shell::verify_contract_cvc5_shellout(
            &mut session,
        ));
    }

    results
}
