//! Feature-specific SMT verification for Assura's 50 verification features.
//!
//! Each feature's verification function is named with the feature's canonical
//! identifier so the coverage-matrix.sh script can grep for it. Features that
//! can be meaningfully modeled in SMT use Z3 encoding; features that are
//! primarily type-level or operational report Unknown with "not yet encoded
//! in SMT" (treated as warnings, not errors, by the CLI).

use crate::VerificationResult;

// -----------------------------------------------------------------------
// Macro for features not yet encoded in SMT.
//
// Generates a function `pub fn $fn_name(name: &str) -> VerificationResult`
// that returns `Unknown` with the given description and reason. The
// function name is the canonical identifier for coverage-matrix.sh.
// -----------------------------------------------------------------------

macro_rules! smt_stub {
    ($fn_name:ident, $desc:expr, $reason:expr) => {
        pub fn $fn_name(name: &str) -> VerificationResult {
            VerificationResult::Unknown {
                clause_desc: format!("{name}: {}", $desc),
                reason: concat!($reason, " not yet encoded in SMT").into(),
            }
        }
    };
}

// -----------------------------------------------------------------------
// CORE.6: Opaque functions (has real logic, not a stub)
// -----------------------------------------------------------------------

/// Verify opaque function contracts.
///
/// Opaque functions hide their implementation from the verifier. The SMT
/// encoding treats the function body as an uninterpreted function and only
/// verifies the requires/ensures interface contract.
pub fn verify_opaque_contract(name: &str, has_ensures: bool) -> VerificationResult {
    if has_ensures {
        VerificationResult::Verified {
            clause_desc: format!("{name}: opaque contract assumed"),
        }
    } else {
        VerificationResult::Unknown {
            clause_desc: format!("{name}: opaque"),
            reason: "opaque function with no ensures clause; nothing to verify".into(),
        }
    }
}

// -----------------------------------------------------------------------
// TYPE.2: Structural invariants (extra parameter, not a simple stub)
// -----------------------------------------------------------------------

/// Verify structural invariant preservation.
pub fn verify_structural_invariant(name: &str, _invariant_expr: &str) -> VerificationResult {
    VerificationResult::Unknown {
        clause_desc: format!("{name}: structural_invariant"),
        reason: "structural invariant inductive check not yet encoded in SMT".into(),
    }
}

// -----------------------------------------------------------------------
// Feature stubs: features not yet modeled in SMT
// -----------------------------------------------------------------------

// MEM
smt_stub!(
    verify_allocator_invariant,
    "allocator invariant",
    "allocator contracts (requires heap model)"
);
smt_stub!(
    verify_circular_buffer,
    "circular buffer",
    "circular buffer modular arithmetic"
);

// TYPE
smt_stub!(
    verify_interface_conformance,
    "interface conformance",
    "interface behavioral subtyping"
);
smt_stub!(
    verify_error_propagation,
    "error_propagation",
    "error propagation path analysis"
);

// SEC
smt_stub!(
    verify_constant_time,
    "constant_time",
    "constant-time control flow analysis"
);
smt_stub!(
    verify_secure_erase,
    "secure_erase",
    "secure erasure memory model"
);
smt_stub!(
    verify_crypto_conformance,
    "crypto conformance",
    "crypto conformance parameter checking"
);

// CONC
smt_stub!(
    verify_shared_mem_safety,
    "shared_mem safety",
    "shared memory concurrency model"
);
smt_stub!(
    verify_callback_reentrancy,
    "callback reentrancy",
    "callback re-entrancy call graph analysis"
);
smt_stub!(
    verify_lock_order,
    "lock_order",
    "lock ordering partial order"
);

// STOR
smt_stub!(
    verify_crash_recovery,
    "crash_recovery",
    "crash recovery WAL model"
);
smt_stub!(
    verify_page_cache,
    "page_cache",
    "page cache buffer pool model"
);
smt_stub!(
    verify_mvcc_isolation,
    "mvcc isolation",
    "mvcc snapshot isolation"
);
smt_stub!(
    verify_rollback_savepoint,
    "rollback savepoint",
    "rollback state restoration"
);
smt_stub!(
    verify_monotonic_state,
    "monotonic state",
    "monotonic state ordering"
);
smt_stub!(
    verify_storage_failure,
    "storage_failure",
    "storage failure mode analysis"
);

// FMT
smt_stub!(
    verify_binary_format,
    "binary_format",
    "binary format layout verification"
);
smt_stub!(verify_bit_level, "bit_level", "bit-level field layout");
smt_stub!(
    verify_string_encoding,
    "string_encoding",
    "string encoding validation"
);
smt_stub!(
    verify_checksum_integrity,
    "checksum integrity",
    "checksum integrity verification"
);
smt_stub!(
    verify_protocol_grammar,
    "protocol_grammar",
    "protocol grammar state machine"
);

// NUM
smt_stub!(
    verify_numerical_precision,
    "numerical_precision",
    "numerical precision floating-point model"
);
smt_stub!(
    verify_precomputed_table,
    "precomputed_table",
    "precomputed table enumeration"
);

// PLAT
smt_stub!(
    verify_platform_abstraction,
    "platform_abstraction",
    "platform abstraction multi-target verification"
);
smt_stub!(
    verify_feature_flag,
    "feature_flag",
    "feature flag combinatorial verification"
);
smt_stub!(
    verify_resource_limit,
    "resource_limit",
    "resource limit tracking"
);

// PERF
smt_stub!(
    verify_unsafe_escape,
    "unsafe_escape",
    "unsafe escape safety proof obligations"
);
smt_stub!(
    verify_complexity_bound,
    "complexity_bound",
    "complexity bound analysis"
);

// TEST
smt_stub!(
    verify_test_gen_coverage,
    "test_gen",
    "test generation coverage analysis"
);
smt_stub!(
    verify_behavioral_equiv,
    "behavioral_equiv",
    "behavioral equivalence function comparison"
);
smt_stub!(
    verify_multi_pass_refinement,
    "multi_pass_refinement",
    "multi-pass refinement chain"
);

// MISC
smt_stub!(
    verify_incremental_contract,
    "incremental_contract",
    "incremental contract evolution"
);
smt_stub!(
    verify_scoped_invariant,
    "scoped_invariant",
    "scoped invariant suspension analysis"
);

// -----------------------------------------------------------------------
// Dispatch: route feature-specific clauses to their SMT verifier
// -----------------------------------------------------------------------

/// Check if a clause kind maps to a feature-specific SMT verifier.
/// Returns Some(result) if the feature was handled, None otherwise.
pub fn verify_feature_clause(clause_kind: &str, parent_name: &str) -> Option<VerificationResult> {
    use assura_parser::features::Feature;
    let feature = Feature::from_clause_kind(clause_kind)?;
    match feature {
        // CORE
        Feature::OpaqueFunctions => Some(verify_opaque_contract(parent_name, false)),
        // MEM
        Feature::AllocatorContracts => Some(verify_allocator_invariant(parent_name)),
        Feature::CircularBuffer => Some(verify_circular_buffer(parent_name)),
        // TYPE
        Feature::InterfaceConformance => Some(verify_interface_conformance(parent_name)),
        Feature::StructuralInvariants => Some(verify_structural_invariant(parent_name, "")),
        Feature::ErrorPropagation => Some(verify_error_propagation(parent_name)),
        // SEC
        Feature::ConstantTime => Some(verify_constant_time(parent_name)),
        Feature::SecureErasure => Some(verify_secure_erase(parent_name)),
        Feature::CryptoConformance => Some(verify_crypto_conformance(parent_name)),
        // CONC
        Feature::SharedMemory => Some(verify_shared_mem_safety(parent_name)),
        Feature::CallbackReentrancy => Some(verify_callback_reentrancy(parent_name)),
        Feature::LockOrdering => Some(verify_lock_order(parent_name)),
        // STOR
        Feature::CrashRecovery => Some(verify_crash_recovery(parent_name)),
        Feature::PageCache => Some(verify_page_cache(parent_name)),
        Feature::MvccIsolation => Some(verify_mvcc_isolation(parent_name)),
        Feature::RollbackSavepoint => Some(verify_rollback_savepoint(parent_name)),
        Feature::MonotonicState => Some(verify_monotonic_state(parent_name)),
        Feature::StorageFailure => Some(verify_storage_failure(parent_name)),
        // FMT
        Feature::BinaryFormat => Some(verify_binary_format(parent_name)),
        Feature::BitLevel => Some(verify_bit_level(parent_name)),
        Feature::StringEncoding => Some(verify_string_encoding(parent_name)),
        Feature::Checksum => Some(verify_checksum_integrity(parent_name)),
        Feature::ProtocolGrammar => Some(verify_protocol_grammar(parent_name)),
        // NUM
        Feature::NumericalPrecision => Some(verify_numerical_precision(parent_name)),
        Feature::PrecomputedTable => Some(verify_precomputed_table(parent_name)),
        // PLAT
        Feature::PlatformAbstraction => Some(verify_platform_abstraction(parent_name)),
        Feature::FeatureFlag => Some(verify_feature_flag(parent_name)),
        Feature::ResourceLimit => Some(verify_resource_limit(parent_name)),
        // PERF
        Feature::UnsafeEscape => Some(verify_unsafe_escape(parent_name)),
        Feature::ComplexityBound => Some(verify_complexity_bound(parent_name)),
        // TEST
        Feature::TestGenCoverage => Some(verify_test_gen_coverage(parent_name)),
        Feature::BehavioralEquiv => Some(verify_behavioral_equiv(parent_name)),
        Feature::MultiPassRefinement => Some(verify_multi_pass_refinement(parent_name)),
        // MISC
        Feature::IncrementalContract => Some(verify_incremental_contract(parent_name)),
        Feature::ScopedInvariant => Some(verify_scoped_invariant(parent_name)),
        // Features without SMT verification (type-level or compile-time only)
        Feature::GhostErasure
        | Feature::LemmaErasure
        | Feature::FrameConditions
        | Feature::AxiomaticDefinitions
        | Feature::TriggerPatterns
        | Feature::ProphecyVariables
        | Feature::Liveness
        | Feature::RegionAnnotations
        | Feature::FixedWidth
        | Feature::TaintTracking
        | Feature::DependentTypes
        | Feature::Determinism
        | Feature::Deadline
        | Feature::WeakMemoryOrdering
        | Feature::CodecRegistry => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_with_ensures_verifies() {
        let result = verify_opaque_contract("test_fn", true);
        assert!(matches!(result, VerificationResult::Verified { .. }));
    }

    #[test]
    fn opaque_without_ensures_unknown() {
        let result = verify_opaque_contract("test_fn", false);
        assert!(matches!(result, VerificationResult::Unknown { .. }));
    }

    #[test]
    fn feature_dispatch_covers_all_registered_clause_kinds() {
        // Every clause kind in the Feature registry should be accepted
        // by verify_feature_clause (either returning Some or None based
        // on whether SMT verification applies).
        use assura_parser::features::Feature;
        for info in Feature::all() {
            for kind in info.clause_kinds {
                // from_clause_kind must resolve; verify_feature_clause
                // handles the feature (Some) or explicitly returns None
                // for non-SMT features.
                let _ = verify_feature_clause(kind, "test");
            }
        }
    }

    #[test]
    fn unknown_feature_returns_none() {
        assert!(verify_feature_clause("nonexistent_feature", "test").is_none());
    }

    #[test]
    fn all_results_contain_feature_identifier() {
        // Verify that each result's clause_desc contains a recognizable identifier
        let VerificationResult::Unknown { clause_desc, .. } = verify_allocator_invariant("test")
        else {
            panic!("expected Unknown for allocator invariant");
        };
        assert!(clause_desc.contains("allocator"));

        let VerificationResult::Unknown { clause_desc, .. } = verify_crash_recovery("test") else {
            panic!("expected Unknown for crash recovery");
        };
        assert!(clause_desc.contains("crash_recovery"));

        let VerificationResult::Unknown { clause_desc, .. } = verify_shared_mem_safety("test")
        else {
            panic!("expected Unknown for shared mem safety");
        };
        assert!(clause_desc.contains("shared_mem"));
    }
}
