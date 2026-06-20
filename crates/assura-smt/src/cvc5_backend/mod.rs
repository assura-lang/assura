//! CVC5 backend: shell-out (SMT-LIB2) and native (cvc5 crate) verification paths.
//!
//! This module contains all CVC5-related verification code, organized into
//! sub-modules for encoding, dispatching, and solver interaction.

// ---------------------------------------------------------------------------
// Sub-modules (preserving original cfg gates from lib.rs)
// ---------------------------------------------------------------------------

pub(crate) mod cvc5_adt;
pub(crate) mod cvc5_atom_encode;
pub(crate) mod cvc5_binop_encode;
pub(crate) mod cvc5_builtins;
pub(crate) mod cvc5_call_encode;
pub(crate) mod cvc5_collect;
pub(crate) mod cvc5_common;
#[cfg(feature = "cvc5-verify")]
pub(crate) mod cvc5_encoder_state;
pub(crate) mod cvc5_expr_smtlib;
pub(crate) mod cvc5_field_access;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) mod cvc5_havoc_assume_smtlib;
pub(crate) mod cvc5_if_encode;
pub(crate) mod cvc5_index_access;
#[cfg(feature = "cvc5-verify")]
pub(crate) mod cvc5_ir_native;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) mod cvc5_ir_smtlib;
pub(crate) mod cvc5_let_block_encode;
pub(crate) mod cvc5_list_encode;
pub(crate) mod cvc5_match_encode;
pub(crate) mod cvc5_model;
#[cfg(feature = "cvc5-verify")]
pub(crate) mod cvc5_native_binops;
#[cfg(feature = "cvc5-verify")]
pub(crate) mod cvc5_native_builtins;
#[cfg(feature = "cvc5-verify")]
pub(crate) mod cvc5_native_encoder;
pub(crate) mod cvc5_old_access;
pub(crate) mod cvc5_quantifier_encode;
pub(crate) mod cvc5_raw_encode;
#[cfg(feature = "cvc5-verify")]
pub(crate) mod cvc5_raw_native;
pub(crate) mod cvc5_raw_ops;
pub(crate) mod cvc5_raw_smtlib;
pub(crate) mod cvc5_tuple_encode;
pub(crate) mod cvc5_verify_dispatch;
#[cfg(feature = "cvc5-verify")]
pub(crate) mod cvc5_verify_native;
#[cfg(feature = "cvc5-verify")]
pub(crate) mod cvc5_verify_native_checks;
#[cfg(feature = "cvc5-verify")]
pub(crate) mod cvc5_verify_native_clause;
#[cfg(feature = "cvc5-verify")]
pub(crate) mod cvc5_verify_native_contract;
#[cfg(feature = "cvc5-verify")]
pub(crate) mod cvc5_verify_native_features;
#[cfg(feature = "cvc5-verify")]
pub(crate) mod cvc5_verify_native_solver;
pub(crate) mod cvc5_verify_shared;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) mod cvc5_verify_shell;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) mod cvc5_verify_shell_clause;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) mod cvc5_verify_shell_contract;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) mod cvc5_verify_shell_runner;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) mod cvc5_verify_shell_script;
pub(crate) mod cvc5_wrapper_encode;

// ---------------------------------------------------------------------------
// Public surface re-exports (formerly cvc5_backend.rs)
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
pub use self::cvc5_collect::collect_vars;
#[allow(unused_imports)]
pub(crate) use self::cvc5_model::parse_smtlib_model;
#[allow(unused_imports)]
pub(crate) use self::cvc5_verify_dispatch::{
    verify_contract_cvc5, verify_contract_cvc5_with_full_context, verify_contract_cvc5_with_lemmas,
    verify_contract_cvc5_with_types,
};
pub(crate) use self::cvc5_verify_shared::collect_lemma_defs_for_cvc5;
#[allow(unused_imports)]
pub(crate) use crate::feature_max::{collect_feature_max_constants, derive_narrowings};

#[cfg(feature = "cvc5-verify")]
#[allow(unused_imports)]
pub(crate) use self::cvc5_verify_native::{
    check_refinement_subtype_cvc5, check_refinement_subtype_with_context_cvc5,
    check_satisfiability_cvc5, check_validity_cvc5, verify_buffer_bounds_cvc5,
    verify_decrease_cvc5, verify_feature_body_cvc5, verify_region_containment_cvc5,
    verify_structural_invariant_inductive_cvc5, verify_taint_safety_cvc5,
    verify_with_measures_cvc5,
};

#[cfg(all(test, feature = "cvc5-verify"))]
pub(crate) use self::cvc5_adt::{
    Cvc5AdtDef, adt_accessor_smt, adt_is_constructor_smt, define_adt_cvc5,
};

#[cfg(feature = "cvc5-verify")]
#[allow(unused_imports)]
pub(crate) use self::cvc5_adt::{
    Cvc5AdtNativeSymbols, adt_accessor_cvc5_native, adt_constructor_cvc5_native,
    adt_is_constructor_cvc5_native, define_adt_cvc5_native,
};

#[allow(unused_imports)]
pub use self::cvc5_expr_smtlib::expr_to_smtlib;

// ---------------------------------------------------------------------------
// Test modules (formerly included via #[path] in cvc5_backend.rs)
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "../tests_cvc5_smtlib.rs"]
mod tests_cvc5_smtlib;

#[cfg(all(test, not(feature = "cvc5-verify")))]
#[path = "../tests_cvc5_shell.rs"]
mod tests_cvc5_shell;

#[cfg(all(test, feature = "cvc5-verify"))]
#[path = "../tests_cvc5_native.rs"]
mod tests_cvc5_native;
