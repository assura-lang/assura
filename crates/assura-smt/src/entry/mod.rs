//! Public entry point functions for SMT verification.
//!
//! Split by agent edit surface:
//! - [`helpers`] — clause/param extraction, `VerifyFileExtras`
//! - [`jobs`] — `collect_verification_jobs`
//! - [`verify`] — `verify()`, `Verifier`, contract/standalone APIs
//! - [`advanced_passes`] — prophecy/weak memory/liveness/layer2/codec/portfolio
//! - [`evolution`] — incremental contract evolution
//!
//! Contains `verify()`, `verify_with_options()`, `verify_parallel()`,
//! and all standalone verification functions (refinement, buffer bounds,
//! taint safety, measures, termination).

mod advanced_passes;
mod evolution;
mod helpers;
mod jobs;
mod layer_dispatch;
pub(crate) mod verify;

#[cfg(test)]
mod tests;

// Re-exports consumed by sibling assura-smt modules (`crate::entry::…`).
// Submodules use `super::foo` / `super::helpers::` internally; do not add
// re-exports here unless another file outside `entry/` needs them.
// `verify_parallel_with_solver` lives at `entry::verify::` (module is pub(crate)).
#[cfg(feature = "z3-verify")]
pub(crate) use advanced_passes::run_advanced_passes;
pub use evolution::{EvolutionResult, verify_evolution, verify_file_evolution};
pub use helpers::VerifyFileExtras;
pub(crate) use helpers::{
    extract_input_params, extract_output_return_type, type_expr_to_token_vec,
};
pub(crate) use jobs::collect_verification_jobs;
pub use layer_dispatch::{verify_layer2, verify_layer3};
pub use verify::{
    Verifier, check_refinement_subtype, check_refinement_subtype_with_context,
    has_verifiable_clauses, verify, verify_buffer_bounds, verify_contract,
    verify_contract_with_solver, verify_decrease, verify_region_containment, verify_taint_safety,
    verify_with_measures,
};
