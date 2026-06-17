//! Z3 backend: encodes Assura contract clauses as Z3 ASTs and checks
//! satisfiability. Handles expression encoding, quantifiers, measures,
//! raw-token parsing, and counterexample extraction.

pub(crate) mod encoder;
mod features;
pub(crate) mod solver;
#[cfg(not(test))]
mod verify;
#[cfg(test)]
pub(crate) mod verify;

pub(crate) use features::{
    check_refinement_subtype_impl, check_refinement_subtype_with_context_impl,
    verify_buffer_bounds_impl, verify_decrease_impl, verify_region_containment_impl,
    verify_taint_safety_impl, verify_with_measures_impl,
};
pub(crate) use verify::{
    collect_feature_max_constants, verify_contract_impl, verify_contract_impl_with_types,
    verify_impl_with_timeout, verify_quantified_impl,
};
