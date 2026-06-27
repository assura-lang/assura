//! SMT-based verification for Assura contracts.
//!
//! Supports multiple solver backends:
//! - **Z3** (default): via the z3 Rust crate, compiled in with the `z3-verify` feature
//! - **CVC5**: via the `cvc5` command-line binary, using SMT-LIB2 format
//! - **Portfolio**: tries Z3 first, falls back to CVC5 on timeout/unknown
//!
//! For each contract in a `TypedFile`, encodes requires/ensures/invariant
//! clauses as SMT formulas and checks their validity:
//!
//! - **ensures with requires**: Check `P => Q` validity by asserting P,
//!   asserting NOT Q, and checking satisfiability. UNSAT = verified.
//! - **invariant**: Check satisfiability (not always false).
//! - **requires**: Recorded as assumptions (checked at call sites).
//!
//! The default timeout is 1 second (Layer 1).

#![cfg_attr(feature = "cvc5-verify", allow(dead_code, unused_imports))]

// ---------------------------------------------------------------------------
// Solver backend selection (defined in assura-config, re-exported here)
// ---------------------------------------------------------------------------

pub use assura_config::SolverChoice;

// Re-export parser/types that sub-modules import via `use super::*;`
pub(crate) use assura_ast::ClauseKind;
#[cfg(not(feature = "z3-verify"))]
pub(crate) use assura_ast::Decl;
#[cfg(any(feature = "z3-verify", test))]
pub(crate) use assura_ast::Expr;
#[cfg(test)]
pub(crate) use assura_ast::Spanned;
pub(crate) use assura_types::TypedFile;

// ---------------------------------------------------------------------------
// Extracted modules
// ---------------------------------------------------------------------------

/// Measure definitions (T054): sorts, axioms, and built-in measures.
pub mod measures;
pub use measures::{
    MeasureAxiom, MeasureAxiomTag, MeasureDefinition, MeasureSort, register_builtin_measures,
};

/// Verification result types: `VerificationResult` and `CounterexampleModel`.
pub mod result;
pub use result::{
    CounterexampleModel, KNOWN_SMT_LIMITATION_MARKER, VerificationResult, VerificationSummary,
    is_known_smt_limitation,
};

// ---------------------------------------------------------------------------
// Policy modules (solver-neutral verification policies, organized in policy/)
// ---------------------------------------------------------------------------
pub(crate) mod policy;

// Re-export all policy submodules at crate root so existing `use crate::encode_atom_policy`
// paths throughout the crate (and backends) continue to resolve without changes.
pub(crate) use policy::clause_gate_policy;
pub(crate) use policy::clause_policy;
pub(crate) use policy::encode_adt_policy;
pub(crate) use policy::encode_atom_policy;
pub(crate) use policy::encode_binop_policy;
pub(crate) use policy::encode_call_policy;
pub(crate) use policy::encode_field_policy;
pub(crate) use policy::encode_if_policy;
pub(crate) use policy::encode_index_policy;
pub(crate) use policy::encode_let_policy;
pub(crate) use policy::encode_list_policy;
pub(crate) use policy::encode_match_policy;
pub(crate) use policy::encode_method_policy;
pub(crate) use policy::encode_old_policy;
pub(crate) use policy::encode_quantifier_policy;
pub(crate) use policy::encode_raw_ops_policy;
pub(crate) use policy::encode_term;
pub(crate) use policy::encode_timeout_policy;
pub(crate) use policy::encode_tuple_policy;
pub(crate) use policy::lemma_inject_policy;
pub(crate) use policy::portfolio_policy;
pub(crate) use policy::prelude_policy;
pub(crate) use policy::solver_outcome_policy;
pub(crate) use policy::trigger_seed_policy;
pub(crate) use policy::unmodelable;
pub(crate) use policy::verify_context;
pub(crate) use policy::verify_labels;

// ---------------------------------------------------------------------------
// IR modules (implementation IR, organized in ir_modules/)
// ---------------------------------------------------------------------------
pub(crate) mod ir_modules;
pub(crate) use ir_modules::ir_codegen;
pub(crate) use ir_modules::ir_encode;
pub(crate) use ir_modules::ir_exec;
pub(crate) use ir_modules::ir_generate;
pub(crate) use ir_modules::ir_loader;
pub(crate) use ir_modules::ir_lower;
pub(crate) use ir_modules::ir_templates;
pub(crate) use ir_modules::ir_type_ctx;

/// Public entry point functions for SMT verification.
mod entry;
pub use entry::{
    EvolutionResult, Verifier, VerifyFileExtras, check_refinement_subtype,
    check_refinement_subtype_with_context, has_verifiable_clauses, verify, verify_buffer_bounds,
    verify_contract, verify_contract_with_solver, verify_decrease, verify_evolution,
    verify_file_evolution, verify_layer2, verify_layer3, verify_region_containment,
    verify_taint_safety, verify_with_measures,
};
pub use feature_max::{collect_feature_max_constants, derive_narrowings};
pub use ir_generate::{EnsuresShape, classify_ensures_shape, generate_ir_sidecar_text};
pub use ir_loader::{
    LoadedVerifyExtras, collect_verification_job_names, ir_search_dirs_for_source,
    load_ir_bodies_for_contracts, load_ir_bodies_for_typed, stub_ir_sidecars_for_typed,
};
pub use ir_templates::{
    IrPromptContext, IrPromptPattern, ir_prompt_contexts_for_typed, render_ir_prompt,
    resolve_ir_pattern, suggest_ir_pattern,
};
pub use verify_context::{ContractVerifyContext, LoadedIrContext};

/// SMT-LIB2 dump and quantifier bound validation.
pub mod smt_dump;
pub use smt_dump::{
    SmtQuery, UnboundedQuantifierWarning, dump_smt_queries, validate_quantifier_bounds,
};

// ---------------------------------------------------------------------------
// Display and formatting
// ---------------------------------------------------------------------------

/// Human-readable display formatting for verification results.
pub mod display;

// ---------------------------------------------------------------------------
// No-Z3 fallback
// ---------------------------------------------------------------------------

#[cfg(not(feature = "z3-verify"))]
mod no_z3;

// ---------------------------------------------------------------------------
// CVC5 backend (organized in cvc5_backend/ subdirectory)
// ---------------------------------------------------------------------------

pub(crate) mod cvc5_backend;

// Re-export CVC5 sub-modules at crate root so existing `use crate::cvc5_*`
// paths throughout the crate continue to resolve without changes.
pub(crate) use cvc5_backend::cvc5_adt;
pub(crate) use cvc5_backend::cvc5_atom_encode;
#[allow(
    unused_imports,
    reason = "stable crate::cvc5_binop_encode path; cvc5-verify / tests"
)]
pub(crate) use cvc5_backend::cvc5_binop_encode;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_bitvector_encode;
// Encode paths import encode_*_policy directly (no cvc5_builtins compatibility surface).
pub(crate) use cvc5_backend::cvc5_call_encode;
pub(crate) use cvc5_backend::cvc5_collect;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_encode_term_impl;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_encoder_state;
pub(crate) use cvc5_backend::cvc5_expr_smtlib;
// Re-exports keep historical `crate::cvc5_*` paths; some shells now use encode_*_policy directly.
#[allow(
    unused_imports,
    reason = "stable crate::cvc5_field_access path; cvc5-verify / tests"
)]
pub(crate) use cvc5_backend::cvc5_field_access;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) use cvc5_backend::cvc5_havoc_assume_smtlib;
#[allow(
    unused_imports,
    reason = "stable crate::cvc5_if_encode path; cvc5-verify / tests"
)]
pub(crate) use cvc5_backend::cvc5_if_encode;
#[allow(
    unused_imports,
    reason = "stable crate::cvc5_index_access path; cvc5-verify / tests"
)]
pub(crate) use cvc5_backend::cvc5_index_access;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_ir_native;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) use cvc5_backend::cvc5_ir_smtlib;
#[allow(
    unused_imports,
    reason = "stable crate::cvc5_let_block_encode path; cvc5-verify / tests"
)]
pub(crate) use cvc5_backend::cvc5_let_block_encode;
pub(crate) use cvc5_backend::cvc5_list_encode;
pub(crate) use cvc5_backend::cvc5_match_encode;
pub(crate) use cvc5_backend::cvc5_model;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_native_binops;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_native_builtins;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_native_encoder;
pub(crate) use cvc5_backend::cvc5_old_access;
#[allow(
    unused_imports,
    reason = "stable crate::cvc5_quantifier_encode path; tests / cvc5-verify"
)]
pub(crate) use cvc5_backend::cvc5_quantifier_encode;
pub(crate) use cvc5_backend::cvc5_raw_encode;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_raw_native;
pub(crate) use cvc5_backend::cvc5_raw_ops;
pub(crate) use cvc5_backend::cvc5_raw_smtlib;
pub(crate) use cvc5_backend::cvc5_tuple_encode;
#[allow(unused_imports)]
pub(crate) use cvc5_backend::cvc5_verify_dispatch;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_verify_native;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_verify_native_checks;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_verify_native_clause;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_verify_native_contract;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_verify_native_features;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_verify_native_solver;
pub(crate) use cvc5_backend::cvc5_verify_shared;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) use cvc5_backend::cvc5_verify_shell;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) use cvc5_backend::cvc5_verify_shell_clause;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) use cvc5_backend::cvc5_verify_shell_contract;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) use cvc5_backend::cvc5_verify_shell_runner;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) use cvc5_backend::cvc5_verify_shell_script;
pub(crate) use cvc5_backend::cvc5_wrapper_encode;
mod feature_max;

// ---------------------------------------------------------------------------
// Z3 backend
// ---------------------------------------------------------------------------

#[cfg(feature = "z3-verify")]
mod z3_backend;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "z3-verify"))]
#[path = "tests_z3.rs"]
mod tests_z3;

#[cfg(all(test, feature = "z3-verify"))]
#[path = "tests_havoc_assume.rs"]
mod tests_havoc_assume;

#[cfg(test)]
#[path = "tests_tier_a.rs"]
mod tests_tier_a;

// ---------------------------------------------------------------------------
// Additional verification modules
// ---------------------------------------------------------------------------

/// Advanced verification: prophecy variables, triggers, quantifier strategies.
pub mod advanced;
/// Bounded model checking (Layer 3): state unrolling + lasso detection.
pub mod bmc;
/// Verification result caching (content-addressed by clause hash).
pub mod cache;
/// Incremental verification: skip re-checking unchanged clauses.
pub mod incremental;
/// K-induction for unbounded proofs (Layer 3).
pub mod k_induction;
/// Layer 2 verification: cross-contract and module-level properties.
pub mod layer2;
/// Feature-specific SMT verification for all 50 features.
pub mod smt_features;
// Re-export key types from submodules so callers and tests can use them
// without qualifying the module path.
pub use advanced::{
    BmcComponents, CodecDispatcher, CodecEntry, DispatchResult, LivenessChecker, LivenessKind,
    LivenessObligation, MemoryAccess, MemoryOrdering, MonitorReduction, ProphecyError,
    ProphecyManager, ProphecyVariable, TriggerManager, TriggerPattern, WeakMemoryChecker,
};
pub use bmc::{BmcConfig, BmcEngine, BmcProperty, BmcResult, BmcSort, BmcTraceStep, StateVariable};
pub use cache::{SessionCache, SessionCacheEntry, VerificationCache};
pub use incremental::{IncrementalCompiler, ModuleState};
pub use k_induction::{KInduction, KInductionConfig, KInductionObligation, KInductionResult};
pub use layer2::{
    Layer2Config, Layer2Result, Layer2Verifier, QuantifiedInvariant, RoundtripObligation,
    TerminationObligation, verify_quantified_expr,
};

/// Havoc+assume helpers for result-field verification (#267).
pub mod havoc_assume;

/// Implementation IR (Section 4): parser, codegen, and `assura ir` CLI command.
pub mod ir;
pub use ir::{
    IrArithOp, IrCmpOp, IrExprKind, IrFunction, IrInstr, IrLiteral, IrMatchPattern, IrModule,
    IrNode, IrParser, IrPred, IrPredArg, IrSlotDecl, IrValidation, parse_ir_module,
    validate_ir_against_contract,
};
#[cfg(test)]
pub(crate) use ir::{parse_arith_op, parse_cmp_op, parse_ir_pred_str};
#[cfg(test)]
pub(crate) use ir_codegen::ir_type_to_rust;
pub use ir_codegen::{
    ir_function_body_to_rust, ir_module_to_body_map, ir_to_rust, stub_ir_sidecar_text,
};

#[cfg(test)]
#[path = "tests_measures.rs"]
mod tests_measures;

#[cfg(test)]
#[path = "tests_quantifier_bounds.rs"]
mod tests_quantifier_bounds;

// ---------------------------------------------------------------------------
// S001: Termination checking via verify_decrease
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "z3-verify"))]
#[path = "tests_decrease.rs"]
mod tests_decrease;

#[cfg(all(test, feature = "z3-verify"))]
#[path = "tests_verify_contract.rs"]
mod tests_verify_contract;

#[cfg(all(test, feature = "z3-verify"))]
#[path = "tests_quantified.rs"]
mod tests_quantified;

#[cfg(test)]
#[path = "tests_cvc5_unit.rs"]
mod tests_cvc5_unit;

// Inline test modules (quantifier_bound, decrease, verify_contract,
// quantified_verification, IR, cvc5_unit) extracted to dedicated files
// above. See #610.
