use super::*;
use crate::cache::SessionCache;
use assura_parser::ast::{Clause, ClauseKind, Decl};

/// Collect lemma definitions from a typed file's declarations.
///
/// Maps each lemma name to its ensures clause bodies. This mirrors
/// `z3_backend::collect_lemma_defs` but is available without the
/// `z3-verify` feature.
pub(crate) fn collect_lemma_defs_for_cvc5(
    typed: &assura_types::TypedFile,
) -> std::collections::HashMap<String, Vec<&Expr>> {
    let mut lemmas = std::collections::HashMap::new();
    for decl in &typed.resolved.source.decls {
        if let Decl::FnDef(f) = &decl.node
            && f.is_lemma
        {
            let ensures: Vec<&Expr> = f
                .clauses
                .iter()
                .filter(|c| c.kind == ClauseKind::Ensures)
                .map(|c| &c.body)
                .collect();
            lemmas.insert(f.name.clone(), ensures);
        }
    }
    lemmas
}

pub use crate::cvc5_collect::collect_vars;
#[allow(unused_imports)]
pub(crate) use crate::cvc5_feature_max::{
    collect_feature_max_constants_cvc5, derive_narrowings_cvc5,
};
#[allow(unused_imports)]
pub(crate) use crate::cvc5_model::parse_smtlib_model;

#[cfg(feature = "cvc5-verify")]
#[allow(unused_imports)]
pub(crate) use crate::cvc5_verify_native::{
    check_refinement_subtype_cvc5, check_refinement_subtype_with_context_cvc5,
    check_satisfiability_cvc5, check_validity_cvc5, verify_buffer_bounds_cvc5,
    verify_decrease_cvc5, verify_feature_body_cvc5, verify_region_containment_cvc5,
    verify_structural_invariant_inductive_cvc5, verify_taint_safety_cvc5,
    verify_with_measures_cvc5,
};

#[cfg(test)]
pub(crate) use crate::cvc5_adt::{
    Cvc5AdtDef, adt_accessor_smt, adt_is_constructor_smt, define_adt_cvc5,
};

#[cfg(feature = "cvc5-verify")]
#[allow(unused_imports)]
pub(crate) use crate::cvc5_adt::{
    Cvc5AdtNativeSymbols, adt_accessor_cvc5_native, adt_constructor_cvc5_native,
    adt_is_constructor_cvc5_native, define_adt_cvc5_native,
};

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
///
/// `params` and `return_ty` supply Nat constraints that cannot be extracted
/// from clauses alone (e.g., function parameters declared outside the clause
/// list). This fixes the parity gap where the Z3 backend received Nat >= 0
/// constraints via `verify_contract_impl_with_types` but the CVC5 backend
/// only extracted them from `input()` clauses.
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
///
/// Like `verify_contract_cvc5_with_types` but also takes `feature_max`
/// constants that are bound to concrete integer values in the solver
/// (matching the Z3 backend's behavior from #180). Refinement narrowings
/// are derived from constants with `max_`/`MAX_` prefixes.
pub(crate) fn verify_contract_cvc5_with_full_context(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    verify_contract_cvc5_with_lemmas(
        contract_name,
        clauses,
        params,
        return_ty,
        None,
        constants,
        cache,
    )
}

/// Verify a single contract's clauses using CVC5, with optional lemma defs.
///
/// When `lemma_defs` is `Some`, `apply lemma_name(args)` expressions will
/// have the referenced lemma's ensures clauses injected as solver
/// assumptions (matching the Z3 backend's behavior).
///
/// `constants` binds `feature_max` names to concrete values instead of
/// leaving them as free solver variables.
pub(crate) fn verify_contract_cvc5_with_lemmas(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&Expr>>>,
    constants: &[(String, i64)],
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    #[cfg(feature = "cvc5-verify")]
    {
        crate::cvc5_verify_native::verify_contract_cvc5_native(
            contract_name,
            clauses,
            params,
            return_ty,
            lemma_defs,
            constants,
            cache,
        )
    }
    #[cfg(not(feature = "cvc5-verify"))]
    {
        crate::cvc5_verify_shell::verify_contract_cvc5_shellout(
            contract_name,
            clauses,
            params,
            return_ty,
            lemma_defs,
            constants,
            cache,
        )
    }
}

pub use crate::cvc5_expr_smtlib::expr_to_smtlib;

#[cfg(test)]
#[path = "tests_cvc5.rs"]
mod tests;
