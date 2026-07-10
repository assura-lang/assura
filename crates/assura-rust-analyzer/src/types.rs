//! Type definitions for inline contract annotations.

/// A single contract clause extracted from a doc comment.
#[derive(Debug, Clone, PartialEq)]
pub struct ContractClause {
    /// The kind of clause (requires, ensures, invariant, effects, decreases).
    pub kind: InlineClauseKind,
    /// The predicate text (everything after the `@keyword`).
    pub body: String,
    /// Byte offset of the clause within the source file (start of `@keyword`).
    pub offset: usize,
}

/// Clause kinds supported in inline annotations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InlineClauseKind {
    // -- Core contract clauses --
    Requires,
    Ensures,
    /// Postcondition on the Ok path of a Result-returning function.
    EnsuresOk,
    /// Postcondition on the Err path of a Result-returning function.
    EnsuresErr,
    Invariant,
    Effects,
    Decreases,
    /// SEC.2: FFI boundary trust annotation (`@ffi_boundary trusted|audited|untrusted`)
    FfiBoundary,
    /// SEC.2: Trust level shorthand (`@trust trusted|audited|untrusted`)
    Trust,

    // -- CORE feature annotations --
    /// CORE.1: Ghost variable/function annotation (`@ghost`)
    Ghost,
    /// CORE.2: Lemma annotation (`@lemma`)
    Lemma,
    /// CORE.3: Frame condition / modifies clause (`@modifies`)
    Modifies,
    /// CORE.6: Opaque function body (`@opaque`)
    Opaque,
    /// CORE.8: Liveness / eventual property (`@eventually`)
    Eventually,

    // -- SEC feature annotations --
    /// SEC.1: Taint tracking annotation (`@taint`)
    Taint,
    /// SEC.3: Constant-time execution (`@constant_time`)
    ConstantTime,
    /// SEC.4: Zeroize sensitive data (`@zeroize`)
    Zeroize,

    // -- MEM feature annotations --
    /// MEM.1: Memory region annotation (`@region`)
    Region,
    /// MEM.2: Bit-width constraint (`@width`)
    Width,
    /// MEM.3: Allocator annotation (`@allocator`)
    Allocator,
    /// MEM.4: Circular buffer annotation (`@circular`)
    Circular,

    // -- TYPE feature annotations --
    /// TYPE.1: Interface / trait bound (`@interface`)
    Interface,
    /// TYPE.3: Error type annotation (`@errors`)
    Errors,

    // -- CONC feature annotations --
    /// CONC.1: Shared state annotation (`@shared`)
    Shared,
    /// CONC.2: Non-reentrant function (`@no_reentrant`)
    NoReentrant,
    /// CONC.3: Deterministic execution (`@deterministic`)
    Deterministic,
    /// CONC.4: Lock ordering annotation (`@lock_order`)
    LockOrder,
    /// CONC.5: Deadline annotation (`@deadline`)
    Deadline,
    /// CONC.6: Memory ordering annotation (`@ordering`)
    MemoryOrdering,

    // -- FMT feature annotations --
    /// FMT.1: Binary format annotation (`@format`)
    Format,
    /// FMT.2: Bit-level layout (`@bits`)
    Bits,
    /// FMT.3: String/data encoding (`@encoding`)
    Encoding,
    /// FMT.5: Checksum annotation (`@checksum`)
    Checksum,

    // -- PLAT feature annotations --
    /// PLAT.1: Platform-specific annotation (`@platform`)
    Platform,
    /// PLAT.2: Feature gate annotation (`@feature`)
    Feature,
    /// PLAT.3: Resource limit annotation (`@resource`)
    Resource,

    // -- PERF feature annotations --
    /// PERF.1: Unsafe escape hatch (`@unsafe_escape`)
    UnsafeEscape,
    /// PERF.2: Complexity annotation (`@complexity`)
    Complexity,

    // -- NUM feature annotations --
    /// NUM.1: Numerical precision (`@precision`)
    Precision,

    // -- STOR feature annotations --
    /// STOR.5: Monotonic state (`@monotonic`)
    Monotonic,

    // -- MISC feature annotations --
    /// MISC.2: Suspend invariant checking (`@suspend_invariant`)
    SuspendInvariant,
}

impl InlineClauseKind {
    /// Parse a clause kind from its keyword string.
    pub fn from_keyword(s: &str) -> Option<Self> {
        match s {
            "requires" => Some(Self::Requires),
            "ensures" => Some(Self::Ensures),
            "ensures_ok" => Some(Self::EnsuresOk),
            "ensures_err" => Some(Self::EnsuresErr),
            "invariant" => Some(Self::Invariant),
            "effects" => Some(Self::Effects),
            "decreases" => Some(Self::Decreases),
            "ffi_boundary" => Some(Self::FfiBoundary),
            "trust" => Some(Self::Trust),
            // CORE
            "ghost" => Some(Self::Ghost),
            "lemma" => Some(Self::Lemma),
            "modifies" => Some(Self::Modifies),
            "opaque" => Some(Self::Opaque),
            "eventually" => Some(Self::Eventually),
            // CORE aliases (coverage: axiom|axiomatic, trigger|auto_trigger, prophecy)
            "axiom" | "axiomatic" => Some(Self::Invariant),
            "trigger" | "auto_trigger" => Some(Self::Decreases),
            "prophecy" => Some(Self::Ghost),
            // SEC
            "taint" => Some(Self::Taint),
            "constant_time" => Some(Self::ConstantTime),
            "zeroize" | "secure_erase" => Some(Self::Zeroize),
            // SEC aliases (coverage: conforms)
            "conforms" => Some(Self::Trust),
            // MEM
            "region" => Some(Self::Region),
            "width" | "FixedWidth" => Some(Self::Width),
            "allocator" => Some(Self::Allocator),
            "circular" | "circular_buffer" => Some(Self::Circular),
            // TYPE
            "interface" => Some(Self::Interface),
            "errors" | "error_policy" => Some(Self::Errors),
            // TYPE aliases (coverage: structural_invariant, must_propagate)
            "structural_invariant" => Some(Self::Invariant),
            "must_propagate" | "must_not_mask" => Some(Self::Errors),
            // CONC
            "shared" | "shared_memory" | "SharedMem" => Some(Self::Shared),
            "no_reentrant" | "must_not_reenter" | "callback" => Some(Self::NoReentrant),
            "deterministic" => Some(Self::Deterministic),
            "lock_order" | "lock_rank" => Some(Self::LockOrder),
            "deadline" => Some(Self::Deadline),
            "ordering" | "acquire" | "release" | "seq_cst" | "acq_rel" => {
                Some(Self::MemoryOrdering)
            }
            // FMT
            "format" | "binary_format" | "byte_layout" => Some(Self::Format),
            "bits" | "bit_layout" | "bit_level" | "bit_field" => Some(Self::Bits),
            "encoding" | "string_encoding" | "charset" => Some(Self::Encoding),
            "checksum" => Some(Self::Checksum),
            // FMT aliases (coverage: codec_registry, ProtocolGrammar|state_machine)
            "codec_registry" | "CodecRegistry" => Some(Self::Format),
            "ProtocolGrammar" | "state_machine" | "protocol_grammar" => Some(Self::Encoding),
            // PLAT
            "platform" | "platform_abstraction" => Some(Self::Platform),
            "feature" | "feature_flag" | "FeatureFlag" => Some(Self::Feature),
            "resource" | "resource_limit" | "ResourceLimit" => Some(Self::Resource),
            // PERF
            "unsafe_escape" | "UnsafeEscape" => Some(Self::UnsafeEscape),
            "complexity" | "ComplexityBound" => Some(Self::Complexity),
            // NUM
            "precision" | "ulp_bound" | "NumericalPrecision" => Some(Self::Precision),
            // NUM aliases (coverage: precomputed_table|lookup_table)
            "precomputed_table" | "lookup_table" => Some(Self::Precision),
            // STOR
            "monotonic" | "MonotonicState" => Some(Self::Monotonic),
            // STOR aliases (coverage: wal|crash_recovery, page_cache, mvcc, rollback, storage_failure)
            "wal" | "crash_recovery" | "write_ahead" => Some(Self::Monotonic),
            "page_cache" | "buffer_pool" => Some(Self::Monotonic),
            "mvcc" | "snapshot_isolation" => Some(Self::Monotonic),
            "rollback" | "savepoint" => Some(Self::Monotonic),
            "failure_mode" | "storage_failure" => Some(Self::Monotonic),
            // TEST aliases (coverage: TestGenerator|test_gen, behavioral_equiv, multi_pass)
            "TestGenerator" | "test_gen" => Some(Self::Ensures),
            "behavioral_equiv" | "BehavioralEquivalence" => Some(Self::Ensures),
            "multi_pass" | "MultiPassRefinement" | "multi_pass_refinement" => Some(Self::Ensures),
            // MISC aliases (coverage: incremental|IncrementalContract)
            "incremental" | "IncrementalContract" | "incremental_contract" => {
                Some(Self::SuspendInvariant)
            }
            "suspend_invariant" | "scoped_invariant" => Some(Self::SuspendInvariant),
            _ => None,
        }
    }

    /// Return the keyword string for this clause kind.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Requires => "requires",
            Self::Ensures => "ensures",
            Self::EnsuresOk => "ensures_ok",
            Self::EnsuresErr => "ensures_err",
            Self::Invariant => "invariant",
            Self::Effects => "effects",
            Self::Decreases => "decreases",
            Self::FfiBoundary => "ffi_boundary",
            Self::Trust => "trust",
            Self::Ghost => "ghost",
            Self::Lemma => "lemma",
            Self::Modifies => "modifies",
            Self::Opaque => "opaque",
            Self::Eventually => "eventually",
            Self::Taint => "taint",
            Self::ConstantTime => "constant_time",
            Self::Zeroize => "zeroize",
            Self::Region => "region",
            Self::Width => "width",
            Self::Allocator => "allocator",
            Self::Circular => "circular",
            Self::Interface => "interface",
            Self::Errors => "errors",
            Self::Shared => "shared",
            Self::NoReentrant => "no_reentrant",
            Self::Deterministic => "deterministic",
            Self::LockOrder => "lock_order",
            Self::Deadline => "deadline",
            Self::MemoryOrdering => "ordering",
            Self::Format => "format",
            Self::Bits => "bits",
            Self::Encoding => "encoding",
            Self::Checksum => "checksum",
            Self::Platform => "platform",
            Self::Feature => "feature",
            Self::Resource => "resource",
            Self::UnsafeEscape => "unsafe_escape",
            Self::Complexity => "complexity",
            Self::Precision => "precision",
            Self::Monotonic => "monotonic",
            Self::SuspendInvariant => "suspend_invariant",
        }
    }

    /// Returns true if this is a core contract clause (requires/ensures/invariant/effects/decreases).
    pub fn is_core_clause(&self) -> bool {
        matches!(
            self,
            Self::Requires
                | Self::Ensures
                | Self::EnsuresOk
                | Self::EnsuresErr
                | Self::Invariant
                | Self::Effects
                | Self::Decreases
        )
    }

    /// Returns true if this is a feature annotation (not a core clause or FFI).
    pub fn is_annotation(&self) -> bool {
        !self.is_core_clause() && !matches!(self, Self::FfiBoundary | Self::Trust)
    }
}

/// The full set of contract clauses for an annotated item.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InlineContract {
    pub requires: Vec<ContractClause>,
    pub ensures: Vec<ContractClause>,
    pub invariants: Vec<ContractClause>,
    pub effects: Vec<ContractClause>,
    pub decreases: Vec<ContractClause>,
    /// SEC.2: FFI boundary trust annotations (`@ffi_boundary`, `@trust`).
    pub ffi_boundary: Vec<ContractClause>,
    /// Feature-specific annotations (@ghost, @taint, @region, etc.).
    /// Keyed by InlineClauseKind for all 32 feature annotation types.
    pub annotations: Vec<ContractClause>,
}

impl InlineContract {
    /// Returns true if no clauses were found.
    pub fn is_empty(&self) -> bool {
        self.requires.is_empty()
            && self.ensures.is_empty()
            && self.invariants.is_empty()
            && self.effects.is_empty()
            && self.decreases.is_empty()
            && self.ffi_boundary.is_empty()
            && self.annotations.is_empty()
    }

    /// Total number of clauses across all kinds.
    pub fn clause_count(&self) -> usize {
        self.requires.len()
            + self.ensures.len()
            + self.invariants.len()
            + self.effects.len()
            + self.decreases.len()
            + self.ffi_boundary.len()
            + self.annotations.len()
    }

    /// Get all annotations of a specific kind.
    pub fn annotations_of(&self, kind: InlineClauseKind) -> Vec<&ContractClause> {
        self.annotations.iter().filter(|c| c.kind == kind).collect()
    }

    pub(crate) fn push(&mut self, clause: ContractClause) {
        match clause.kind {
            InlineClauseKind::Requires => self.requires.push(clause),
            InlineClauseKind::Ensures
            | InlineClauseKind::EnsuresOk
            | InlineClauseKind::EnsuresErr => self.ensures.push(clause),
            InlineClauseKind::Invariant => self.invariants.push(clause),
            InlineClauseKind::Effects => self.effects.push(clause),
            InlineClauseKind::Decreases => self.decreases.push(clause),
            InlineClauseKind::FfiBoundary | InlineClauseKind::Trust => {
                self.ffi_boundary.push(clause)
            }
            _ => self.annotations.push(clause),
        }
    }
}

/// What kind of Rust item is annotated.
#[derive(Debug, Clone, PartialEq)]
pub enum AnnotatedItemKind {
    /// A free function or method.
    Function {
        name: String,
        params: Vec<ParamInfo>,
        return_type: Option<String>,
        is_unsafe: bool,
        is_async: bool,
        is_public: bool,
    },
    /// A struct definition.
    Struct {
        name: String,
        fields: Vec<FieldInfo>,
    },
    /// An impl block (contracts apply to all methods within).
    ImplBlock {
        self_type: String,
        trait_name: Option<String>,
    },
}

/// Basic parameter info extracted from a Rust function signature.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamInfo {
    pub name: String,
    pub ty: String,
}

/// Basic field info extracted from a Rust struct.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldInfo {
    pub name: String,
    pub ty: String,
}

/// An item in a Rust source file that has contract annotations.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotatedItem {
    /// The contract clauses extracted from doc comments.
    pub contract: InlineContract,
    /// What kind of item this is.
    pub kind: AnnotatedItemKind,
    /// Byte offset of the item in the source file.
    pub offset: usize,
    /// Line number (1-based) of the item in the source file.
    pub line: usize,
}
