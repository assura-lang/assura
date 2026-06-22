//! Central registry of Assura's 50 verification features.
//!
//! Each feature has a canonical enum variant, clause-kind aliases, category,
//! and spec identifier. The three dispatch tables (type checker, SMT verifier,
//! code generator) use `Feature::from_clause_kind()` instead of ad-hoc string
//! matching, so adding a new feature forces handling at every site.

/// One of Assura's 50 verification features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    // CORE (1-8)
    GhostErasure,
    LemmaErasure,
    FrameConditions,
    AxiomaticDefinitions,
    TriggerPatterns,
    OpaqueFunctions,
    ProphecyVariables,
    Liveness,
    // MEM (1-4)
    RegionAnnotations,
    FixedWidth,
    AllocatorContracts,
    CircularBuffer,
    // TYPE (1-3)
    InterfaceConformance,
    StructuralInvariants,
    ErrorPropagation,
    // SEC (1-5)
    TaintTracking,
    ConstantTime,
    SecureErasure,
    CryptoConformance,
    DependentTypes,
    // CONC (1-6)
    SharedMemory,
    CallbackReentrancy,
    Determinism,
    LockOrdering,
    Deadline,
    WeakMemoryOrdering,
    // STOR (1-6)
    CrashRecovery,
    PageCache,
    MvccIsolation,
    RollbackSavepoint,
    MonotonicState,
    StorageFailure,
    // FMT (1-6)
    BinaryFormat,
    BitLevel,
    StringEncoding,
    CodecRegistry,
    Checksum,
    ProtocolGrammar,
    // NUM (1-2)
    NumericalPrecision,
    PrecomputedTable,
    // PLAT (1-3)
    PlatformAbstraction,
    FeatureFlag,
    ResourceLimit,
    // PERF (1-2)
    UnsafeEscape,
    ComplexityBound,
    // TEST (1-3)
    TestGenCoverage,
    BehavioralEquiv,
    MultiPassRefinement,
    // MISC (1-2)
    IncrementalContract,
    ScopedInvariant,
}

/// Feature category grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureCategory {
    Core,
    Mem,
    Type,
    Sec,
    Conc,
    Stor,
    Fmt,
    Num,
    Plat,
    Perf,
    Test,
    Misc,
}

/// Static metadata for a verification feature.
#[derive(Debug, Clone, Copy)]
pub struct FeatureInfo {
    pub id: Feature,
    pub category: FeatureCategory,
    pub spec_id: &'static str,
    pub clause_kinds: &'static [&'static str],
    pub description: &'static str,
}

/// Complete registry of all 50 features with metadata.
static FEATURES: &[FeatureInfo] = &[
    // CORE
    FeatureInfo {
        id: Feature::GhostErasure,
        category: FeatureCategory::Core,
        spec_id: "CORE.1",
        clause_kinds: &["ghost"],
        description: "Ghost code erasure",
    },
    FeatureInfo {
        id: Feature::LemmaErasure,
        category: FeatureCategory::Core,
        spec_id: "CORE.2",
        clause_kinds: &["axiom", "axiomatic"],
        description: "Lemma/axiom erasure",
    },
    FeatureInfo {
        id: Feature::FrameConditions,
        category: FeatureCategory::Core,
        spec_id: "CORE.3",
        clause_kinds: &["modifies", "frame"],
        description: "Frame condition checking",
    },
    FeatureInfo {
        id: Feature::AxiomaticDefinitions,
        category: FeatureCategory::Core,
        spec_id: "CORE.4",
        clause_kinds: &["trigger", "auto_trigger"],
        description: "Trigger patterns",
    },
    FeatureInfo {
        id: Feature::TriggerPatterns,
        category: FeatureCategory::Core,
        spec_id: "CORE.5",
        clause_kinds: &["trigger_pattern"],
        description: "Quantifier trigger patterns",
    },
    FeatureInfo {
        id: Feature::OpaqueFunctions,
        category: FeatureCategory::Core,
        spec_id: "CORE.6",
        clause_kinds: &["opaque"],
        description: "Opaque function verification",
    },
    FeatureInfo {
        id: Feature::ProphecyVariables,
        category: FeatureCategory::Core,
        spec_id: "CORE.7",
        clause_kinds: &["prophecy"],
        description: "Prophecy variables",
    },
    FeatureInfo {
        id: Feature::Liveness,
        category: FeatureCategory::Core,
        spec_id: "CORE.8",
        clause_kinds: &["liveness", "eventually", "leads_to"],
        description: "Liveness properties",
    },
    // MEM
    FeatureInfo {
        id: Feature::RegionAnnotations,
        category: FeatureCategory::Mem,
        spec_id: "MEM.1",
        clause_kinds: &["region"],
        description: "Memory region annotations",
    },
    FeatureInfo {
        id: Feature::FixedWidth,
        category: FeatureCategory::Mem,
        spec_id: "MEM.2",
        clause_kinds: &["fixed_width", "width"],
        description: "Fixed-width integer overflow",
    },
    FeatureInfo {
        id: Feature::AllocatorContracts,
        category: FeatureCategory::Mem,
        spec_id: "MEM.3",
        clause_kinds: &["allocator"],
        description: "Allocator contracts",
    },
    FeatureInfo {
        id: Feature::CircularBuffer,
        category: FeatureCategory::Mem,
        spec_id: "MEM.4",
        clause_kinds: &["circular", "circular_buffer"],
        description: "Circular buffer modular arithmetic",
    },
    // TYPE
    FeatureInfo {
        id: Feature::InterfaceConformance,
        category: FeatureCategory::Type,
        spec_id: "TYPE.1",
        clause_kinds: &["interface"],
        description: "Interface behavioral subtyping",
    },
    FeatureInfo {
        id: Feature::StructuralInvariants,
        category: FeatureCategory::Type,
        spec_id: "TYPE.2",
        clause_kinds: &["structural_invariant"],
        description: "Structural invariants",
    },
    FeatureInfo {
        id: Feature::ErrorPropagation,
        category: FeatureCategory::Type,
        spec_id: "TYPE.3",
        clause_kinds: &["must_propagate", "must_not_mask", "error_policy"],
        description: "Error propagation analysis",
    },
    // SEC
    FeatureInfo {
        id: Feature::TaintTracking,
        category: FeatureCategory::Sec,
        spec_id: "SEC.1",
        clause_kinds: &["taint", "secret"],
        description: "Taint tracking",
    },
    FeatureInfo {
        id: Feature::ConstantTime,
        category: FeatureCategory::Sec,
        spec_id: "SEC.2",
        clause_kinds: &["constant_time"],
        description: "Constant-time execution",
    },
    FeatureInfo {
        id: Feature::SecureErasure,
        category: FeatureCategory::Sec,
        spec_id: "SEC.3",
        clause_kinds: &["zeroize", "secure_erase"],
        description: "Secure memory erasure",
    },
    FeatureInfo {
        id: Feature::CryptoConformance,
        category: FeatureCategory::Sec,
        spec_id: "SEC.4",
        clause_kinds: &["conforms", "crypto"],
        description: "Cryptographic conformance",
    },
    FeatureInfo {
        id: Feature::DependentTypes,
        category: FeatureCategory::Sec,
        spec_id: "SEC.5",
        clause_kinds: &["dependent", "label"],
        description: "Dependent types / info flow",
    },
    // CONC
    FeatureInfo {
        id: Feature::SharedMemory,
        category: FeatureCategory::Conc,
        spec_id: "CONC.1",
        clause_kinds: &["shared", "shared_memory"],
        description: "Shared memory safety",
    },
    FeatureInfo {
        id: Feature::CallbackReentrancy,
        category: FeatureCategory::Conc,
        spec_id: "CONC.2",
        clause_kinds: &["must_not_reenter", "no_reentrant", "callback"],
        description: "Callback reentrancy",
    },
    FeatureInfo {
        id: Feature::Determinism,
        category: FeatureCategory::Conc,
        spec_id: "CONC.3",
        clause_kinds: &["deterministic"],
        description: "Deterministic execution",
    },
    FeatureInfo {
        id: Feature::LockOrdering,
        category: FeatureCategory::Conc,
        spec_id: "CONC.4",
        clause_kinds: &["lock_order", "lock_rank"],
        description: "Lock ordering",
    },
    FeatureInfo {
        id: Feature::Deadline,
        category: FeatureCategory::Conc,
        spec_id: "CONC.5",
        clause_kinds: &["deadline", "timeout"],
        description: "Deadline contracts",
    },
    FeatureInfo {
        id: Feature::WeakMemoryOrdering,
        category: FeatureCategory::Conc,
        spec_id: "CONC.6",
        clause_kinds: &["ordering", "acquire", "release", "seq_cst", "acq_rel"],
        description: "Weak memory ordering",
    },
    // STOR
    FeatureInfo {
        id: Feature::CrashRecovery,
        category: FeatureCategory::Stor,
        spec_id: "STOR.1",
        clause_kinds: &["crash_recovery", "wal", "write_ahead"],
        description: "Crash recovery / WAL",
    },
    FeatureInfo {
        id: Feature::PageCache,
        category: FeatureCategory::Stor,
        spec_id: "STOR.2",
        clause_kinds: &["page_cache", "buffer_pool"],
        description: "Page cache / buffer pool",
    },
    FeatureInfo {
        id: Feature::MvccIsolation,
        category: FeatureCategory::Stor,
        spec_id: "STOR.3",
        clause_kinds: &["mvcc", "snapshot_isolation"],
        description: "MVCC snapshot isolation",
    },
    FeatureInfo {
        id: Feature::RollbackSavepoint,
        category: FeatureCategory::Stor,
        spec_id: "STOR.4",
        clause_kinds: &["rollback", "savepoint"],
        description: "Rollback / savepoint",
    },
    FeatureInfo {
        id: Feature::MonotonicState,
        category: FeatureCategory::Stor,
        spec_id: "STOR.5",
        clause_kinds: &["monotonic"],
        description: "Monotonic state ordering",
    },
    FeatureInfo {
        id: Feature::StorageFailure,
        category: FeatureCategory::Stor,
        spec_id: "STOR.6",
        clause_kinds: &["failure_mode", "storage_failure"],
        description: "Storage failure modes",
    },
    // FMT
    FeatureInfo {
        id: Feature::BinaryFormat,
        category: FeatureCategory::Fmt,
        spec_id: "FMT.1",
        clause_kinds: &["binary_format", "byte_layout"],
        description: "Binary format layout",
    },
    FeatureInfo {
        id: Feature::BitLevel,
        category: FeatureCategory::Fmt,
        spec_id: "FMT.2",
        clause_kinds: &["bit_layout", "bit_level", "bit_field"],
        description: "Bit-level field layout",
    },
    FeatureInfo {
        id: Feature::StringEncoding,
        category: FeatureCategory::Fmt,
        spec_id: "FMT.3",
        clause_kinds: &["string_encoding", "charset"],
        description: "String encoding validation",
    },
    FeatureInfo {
        id: Feature::CodecRegistry,
        category: FeatureCategory::Fmt,
        spec_id: "FMT.4",
        clause_kinds: &["codec_registry", "codec"],
        description: "Codec registry",
    },
    FeatureInfo {
        id: Feature::Checksum,
        category: FeatureCategory::Fmt,
        spec_id: "FMT.5",
        clause_kinds: &["checksum"],
        description: "Checksum integrity",
    },
    FeatureInfo {
        id: Feature::ProtocolGrammar,
        category: FeatureCategory::Fmt,
        spec_id: "FMT.6",
        clause_kinds: &["protocol_grammar", "state_machine"],
        description: "Protocol grammar / state machine",
    },
    // NUM
    FeatureInfo {
        id: Feature::NumericalPrecision,
        category: FeatureCategory::Num,
        spec_id: "NUM.1",
        clause_kinds: &["precision", "ulp_bound"],
        description: "Numerical precision",
    },
    FeatureInfo {
        id: Feature::PrecomputedTable,
        category: FeatureCategory::Num,
        spec_id: "NUM.2",
        clause_kinds: &["precomputed_table", "lookup_table"],
        description: "Precomputed table verification",
    },
    // PLAT
    FeatureInfo {
        id: Feature::PlatformAbstraction,
        category: FeatureCategory::Plat,
        spec_id: "PLAT.1",
        clause_kinds: &["platform", "platform_abstraction"],
        description: "Platform abstraction",
    },
    FeatureInfo {
        id: Feature::FeatureFlag,
        category: FeatureCategory::Plat,
        spec_id: "PLAT.2",
        clause_kinds: &["feature_flag"],
        description: "Feature flag verification",
    },
    FeatureInfo {
        id: Feature::ResourceLimit,
        category: FeatureCategory::Plat,
        spec_id: "PLAT.3",
        clause_kinds: &["resource_limit"],
        description: "Resource limit tracking",
    },
    // PERF
    FeatureInfo {
        id: Feature::UnsafeEscape,
        category: FeatureCategory::Perf,
        spec_id: "PERF.1",
        clause_kinds: &["unsafe_escape"],
        description: "Unsafe escape hatches",
    },
    FeatureInfo {
        id: Feature::ComplexityBound,
        category: FeatureCategory::Perf,
        spec_id: "PERF.2",
        clause_kinds: &["complexity", "complexity_bound"],
        description: "Complexity bound analysis",
    },
    // TEST
    FeatureInfo {
        id: Feature::TestGenCoverage,
        category: FeatureCategory::Test,
        spec_id: "TEST.1",
        clause_kinds: &["test_gen", "generate_tests"],
        description: "Test generation coverage",
    },
    FeatureInfo {
        id: Feature::BehavioralEquiv,
        category: FeatureCategory::Test,
        spec_id: "TEST.2",
        clause_kinds: &["behavioral_equiv", "behavioral_equivalence"],
        description: "Behavioral equivalence",
    },
    FeatureInfo {
        id: Feature::MultiPassRefinement,
        category: FeatureCategory::Test,
        spec_id: "TEST.3",
        clause_kinds: &["multi_pass", "multi_pass_refinement"],
        description: "Multi-pass refinement",
    },
    // MISC
    FeatureInfo {
        id: Feature::IncrementalContract,
        category: FeatureCategory::Misc,
        spec_id: "MISC.1",
        clause_kinds: &["incremental", "incremental_contract"],
        description: "Incremental contract evolution",
    },
    FeatureInfo {
        id: Feature::ScopedInvariant,
        category: FeatureCategory::Misc,
        spec_id: "MISC.2",
        clause_kinds: &["suspend_invariant", "scoped_invariant"],
        description: "Scoped invariant suspension",
    },
];

impl Feature {
    /// Look up a feature by its clause-kind string. Returns `None` for
    /// clause kinds that don't map to a feature (e.g., `"requires"`).
    pub fn from_clause_kind(kind: &str) -> Option<Feature> {
        FEATURES
            .iter()
            .find(|f| f.clause_kinds.contains(&kind))
            .map(|f| f.id)
    }

    /// Return metadata for this feature.
    pub fn info(&self) -> &'static FeatureInfo {
        FEATURES.iter().find(|f| f.id == *self).unwrap()
    }

    /// All 50 features in registry order.
    pub fn all() -> &'static [FeatureInfo] {
        FEATURES
    }

    /// Number of registered features.
    pub fn count() -> usize {
        FEATURES.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_50_features() {
        assert_eq!(Feature::count(), 50);
    }

    #[test]
    fn all_features_unique() {
        let mut seen = std::collections::HashSet::new();
        for f in Feature::all() {
            assert!(seen.insert(f.id), "duplicate feature: {:?}", f.id);
        }
    }

    #[test]
    fn clause_kind_lookup_canonical() {
        assert_eq!(
            Feature::from_clause_kind("ghost"),
            Some(Feature::GhostErasure)
        );
        assert_eq!(
            Feature::from_clause_kind("mvcc"),
            Some(Feature::MvccIsolation)
        );
        assert_eq!(
            Feature::from_clause_kind("unsafe_escape"),
            Some(Feature::UnsafeEscape)
        );
    }

    #[test]
    fn clause_kind_lookup_aliases() {
        assert_eq!(
            Feature::from_clause_kind("wal"),
            Some(Feature::CrashRecovery)
        );
        assert_eq!(
            Feature::from_clause_kind("write_ahead"),
            Some(Feature::CrashRecovery)
        );
        assert_eq!(
            Feature::from_clause_kind("crash_recovery"),
            Some(Feature::CrashRecovery)
        );
    }

    #[test]
    fn clause_kind_lookup_unknown() {
        assert_eq!(Feature::from_clause_kind("requires"), None);
        assert_eq!(Feature::from_clause_kind("nonexistent"), None);
    }

    #[test]
    fn feature_info_roundtrip() {
        for f in Feature::all() {
            assert_eq!(f.id.info().spec_id, f.spec_id);
        }
    }

    #[test]
    fn no_duplicate_clause_kinds() {
        let mut all_kinds = Vec::new();
        for f in Feature::all() {
            for kind in f.clause_kinds {
                assert!(
                    !all_kinds.contains(kind),
                    "duplicate clause kind '{}' in {:?} and another feature",
                    kind,
                    f.id
                );
                all_kinds.push(*kind);
            }
        }
    }
}
