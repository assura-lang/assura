//! Solver-neutral verification policies.
//!
//! Each module defines a shared "plan" or "policy" for how the SMT layer
//! handles a specific aspect of encoding or verification dispatch. Backends
//! (Z3, CVC5) consume these policies to build solver-specific terms.

pub mod clause_gate_policy;
pub mod clause_policy;
pub mod encode_adt_policy;
pub mod encode_atom_policy;
pub mod encode_binop_policy;
pub mod encode_call_policy;
pub mod encode_callee_policy;
pub mod encode_field_policy;
pub mod encode_if_policy;
pub mod encode_index_policy;
pub mod encode_let_policy;
pub mod encode_list_policy;
pub mod encode_match_policy;
pub mod encode_method_policy;
pub mod encode_old_policy;
pub mod encode_quantifier_policy;
pub mod encode_raw_ops_policy;
pub mod encode_term;
pub mod encode_timeout_policy;
pub mod encode_tuple_policy;
pub mod lemma_inject_policy;
pub mod portfolio_policy;
pub mod prelude_policy;
pub mod solver_outcome_policy;
pub mod trigger_seed_policy;
pub mod unmodelable;
pub mod verify_context;
pub mod verify_labels;
