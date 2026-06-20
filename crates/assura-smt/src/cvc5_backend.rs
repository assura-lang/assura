//! CVC5 backend public surface: re-exports for shell-out and native paths.

#[allow(unused_imports)]
pub use crate::cvc5_collect::collect_vars;
#[allow(unused_imports)]
pub(crate) use crate::cvc5_feature_max::{
    collect_feature_max_constants_cvc5, derive_narrowings_cvc5,
};
#[allow(unused_imports)]
pub(crate) use crate::cvc5_model::parse_smtlib_model;
#[allow(unused_imports)]
pub(crate) use crate::cvc5_verify_dispatch::{
    verify_contract_cvc5, verify_contract_cvc5_with_full_context, verify_contract_cvc5_with_lemmas,
    verify_contract_cvc5_with_types,
};
pub(crate) use crate::cvc5_verify_shared::collect_lemma_defs_for_cvc5;

#[cfg(feature = "cvc5-verify")]
#[allow(unused_imports)]
pub(crate) use crate::cvc5_verify_native::{
    check_refinement_subtype_cvc5, check_refinement_subtype_with_context_cvc5,
    check_satisfiability_cvc5, check_validity_cvc5, verify_buffer_bounds_cvc5,
    verify_decrease_cvc5, verify_feature_body_cvc5, verify_region_containment_cvc5,
    verify_structural_invariant_inductive_cvc5, verify_taint_safety_cvc5,
    verify_with_measures_cvc5,
};

#[cfg(all(test, feature = "cvc5-verify"))]
pub(crate) use crate::cvc5_adt::{
    Cvc5AdtDef, adt_accessor_smt, adt_is_constructor_smt, define_adt_cvc5,
};

#[cfg(feature = "cvc5-verify")]
#[allow(unused_imports)]
pub(crate) use crate::cvc5_adt::{
    Cvc5AdtNativeSymbols, adt_accessor_cvc5_native, adt_constructor_cvc5_native,
    adt_is_constructor_cvc5_native, define_adt_cvc5_native,
};

#[allow(unused_imports)]
pub use crate::cvc5_expr_smtlib::expr_to_smtlib;

#[cfg(test)]
#[path = "tests_cvc5_smtlib.rs"]
mod tests_cvc5_smtlib;

#[cfg(all(test, not(feature = "cvc5-verify")))]
#[path = "tests_cvc5_shell.rs"]
mod tests_cvc5_shell;

#[cfg(all(test, feature = "cvc5-verify"))]
#[path = "tests_cvc5_native.rs"]
mod tests_cvc5_native;
