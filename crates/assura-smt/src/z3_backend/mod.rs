//! Z3 backend: encodes Assura contract clauses as Z3 ASTs and checks
//! satisfiability. Handles expression encoding, quantifiers, measures,
//! raw-token parsing, and counterexample extraction.

pub(crate) mod encoder;
mod features;
mod havoc_assume;
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
#[cfg(test)]
pub(crate) use havoc_assume::apply_havoc_assume_z3;
pub(crate) use verify::{
    verify_contract_impl, verify_contract_impl_with_types_and_ir, verify_impl_with_timeout,
    verify_quantified_impl,
};
