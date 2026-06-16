//! Unified syntax kinds for the Assura CST (concrete syntax tree).
//!
//! A single enum covers both *token* kinds (leaves produced by the lexer)
//! and *node* kinds (interior nodes built by the parser). This is the
//! standard rowan architecture: one flat `#[repr(u16)]` enum mapped
//! through the `Language` trait.

use crate::lexer::Token;

/// Every kind of syntax element in an Assura source file.
///
/// Variants prefixed with nothing are *tokens* (leaves). Variants
/// whose names end with implied "node" semantics (e.g. `SOURCE_FILE`,
/// `CONTRACT_DECL`) are *nodes* (composites containing children).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    // =================================================================
    // Tokens — one per lexer::Token variant, same order
    // =================================================================

    // --- Core keywords ---
    AND_KW,
    API_COMPAT_KW,
    AS_KW,
    AUDIT_KW,
    BIND_KW,
    COMPLIANCE_KW,
    CONCURRENCY_KW,
    CONTRACT_KW,
    DATA_FLOW_KW,
    EFFECTS_KW,
    ELSE_KW,
    ENUM_KW,
    ENSURES_KW,
    ERRORS_KW,
    EVOLUTION_KW,
    EXTENDS_KW,
    EXISTS_KW,
    EXTERN_KW,
    FALSE_KW,
    FN_KW,
    FORALL_KW,
    IDEMPOTENT_KW,
    IF_KW,
    IMPORT_KW,
    IN_KW,
    INPUT_KW,
    INVARIANT_KW,
    IS_KW,
    LET_KW,
    MATCH_KW,
    MODULE_KW,
    MUST_NOT_KW,
    NOT_KW,
    OBSERVE_KW,
    OLD_KW,
    OPERATION_KW,
    OR_KW,
    OUTPUT_KW,
    PARTIAL_KW,
    PERFORMANCE_KW,
    PRIVACY_KW,
    PROFILE_KW,
    PROJECT_KW,
    PROTOCOL_KW,
    PUB_KW,
    PURE_KW,
    QUERY_KW,
    REQUIRES_KW,
    RESULT_KW,
    RETENTION_KW,
    RULE_KW,
    SELF_KW,
    SERIALIZATION_KW,
    SERVICE_KW,
    STATES_KW,
    THEN_KW,
    TRANSACTION_KW,
    TRANSITION_KW,
    TRUE_KW,
    TYPE_KW,
    WHERE_KW,

    // --- CORE verification ---
    APPLY_KW,
    AUTO_TRIGGER_KW,
    AXIOM_KW,
    CASES_KW,
    DECREASES_KW,
    DEFINE_KW,
    EVENTUALLY_KW,
    EVENTUALLY_ALWAYS_KW,
    EVENTUALLY_WITHIN_KW,
    FAIR_KW,
    GHOST_KW,
    INDUCTION_KW,
    LEADS_TO_KW,
    LEMMA_KW,
    LIVENESS_KW,
    MODIFIES_KW,
    OPAQUE_KW,
    PROPHECY_KW,
    PROPERTY_KW,
    READS_KW,
    RESOLVE_KW,
    REVEAL_KW,
    TRIGGER_KW,

    // --- MEM ---
    ALLOCATOR_KW,
    ATOMIC_KW,
    ATOMIC_LOAD_KW,
    CIRCULAR_BUFFER_KW,
    LAYOUT_KW,
    REGION_KW,
    SHARED_MEMORY_KW,
    SLIDE_KW,
    VALID_COUNT_KW,
    WRITE_POS_KW,

    // --- TYPE ---
    ERROR_POLICY_KW,
    IMPL_KW,
    INTERFACE_KW,
    MUST_NOT_MASK_KW,
    MUST_PROPAGATE_KW,
    STRUCTURAL_INVARIANT_KW,

    // --- SEC ---
    ALGORITHM_KW,
    BOUNDARY_KW,
    AXIOM_SPEC_KW,
    CALLEE_GUARANTEES_KW,
    CALLER_GUARANTEES_KW,
    CONFORMS_KW,
    CONSTANT_TIME_KW,
    ERASE_KW,
    ERROR_CONVENTION_KW,
    EXPORT_KW,
    FFI_KW,
    SECRET_KW,
    SECURE_ERASE_KW,
    SPEC_KW,
    TRUST_KW,

    // --- CONC ---
    ACQ_REL_KW,
    ACQUIRE_KW,
    CALLBACK_KW,
    DEADLINE_KW,
    DETERMINISTIC_KW,
    FENCE_KW,
    LOCK_ORDER_KW,
    LOCK_RANK_KW,
    MAY_CALL_KW,
    MERGE_KW,
    MUST_BE_KW,
    MUST_NOT_CALL_KW,
    MUST_NOT_REENTER_KW,
    ORDERING_KW,
    RELAXED_KW,
    RELEASE_KW,
    SEQ_CST_KW,
    STALE_VIEW_KW,
    TIMEOUT_KW,
    VIEW_KW,

    // --- STOR ---
    CACHE_KW,
    CRASH_POINT_KW,
    DURABLE_STATE_KW,
    ERASE_VALUE_KW,
    MONOTONIC_KW,
    ON_CRASH_DURING_KW,
    PINNED_KW,
    PROG_IDEMPOTENT_KW,
    RECOVERS_TO_KW,
    RECOVERY_KW,
    SNAPSHOT_KW,
    STORAGE_MODEL_KW,

    // --- FMT ---
    ACCEPTS_KW,
    BIT_FORMAT_KW,
    BITS_KW,
    CODEC_KW,
    CODEC_REGISTRY_KW,
    DEVIATION_KW,
    ENCODING_MATCHES_KW,
    FORMAT_KW,
    INTEGRITY_KW,
    MAGIC_KW,
    REJECTS_KW,
    RFC_KW,

    // --- NUM ---
    MAX_ABS_ERROR_KW,
    MAX_ULP_ERROR_KW,
    PRECOMPUTE_KW,
    PRECISION_KW,
    TABLE_KW,
    VERIFY_AGAINST_KW,

    // --- PLAT ---
    CFG_KW,
    FEATURE_KW,
    LIMIT_KW,
    ON_EXCEED_KW,
    PLATFORM_KW,
    VARIANT_KW,

    // --- PERF ---
    AMORTIZED_TIME_KW,
    BOUNDS_KW,
    COMPLEXITY_KW,
    UNSAFE_ESCAPE_KW,

    // --- TEST ---
    CONVERGENCE_KW,
    EQUIVALENT_KW,
    GENERATE_TESTS_KW,
    PASSES_KW,
    QUALITY_KW,
    REFINEMENT_KW,

    // --- MISC ---
    EXTENSIBLE_KW,
    FROZEN_KW,
    INCREMENTAL_KW,
    YIELDS_KW,

    // --- Literals ---
    FLOAT_LIT,
    INT_LIT,
    STRING_LIT,

    // --- Identifiers ---
    IDENT,

    // --- Punctuation ---
    L_BRACE,   // {
    R_BRACE,   // }
    L_PAREN,   // (
    R_PAREN,   // )
    L_BRACKET, // [
    R_BRACKET, // ]
    L_ANGLE,   // <
    R_ANGLE,   // >
    COMMA,     // ,
    COLON,     // :
    SEMICOLON, // ;
    DOT,       // .
    PIPE,      // |
    QUESTION,  // ?
    ARROW,     // ->
    FAT_ARROW, // =>
    HASH,      // #
    AT,        // @
    EQUALS,    // =

    // --- Operators ---
    CONCAT,  // ++
    PLUS,    // +
    MINUS,   // -
    STAR,    // *
    SLASH,   // /
    PERCENT, // %
    EQ,      // ==
    NEQ,     // !=
    LTE,     // <=
    GTE,     // >=
    AND_AND, // &&
    OR_OR,   // ||
    BANG,    // !
    AMP_MUT, // &mut
    AMP,     // &
    DOT_DOT, // ..
    CARET,   // ^

    // --- Synthetic tokens (not from lexer) ---
    WHITESPACE,
    COMMENT,
    ERROR_TOKEN,

    // =================================================================
    // Composite nodes — interior nodes built by the parser
    // =================================================================
    SOURCE_FILE,
    PROJECT_DECL,
    MODULE_DECL,
    IMPORT_DECL,
    IMPORT_ITEM_LIST,

    CONTRACT_DECL,
    SERVICE_DECL,
    TYPE_DEF,
    ENUM_DEF,
    EXTERN_DECL,
    BIND_DECL,
    PROPHECY_DECL,
    CODEC_REGISTRY_DECL,
    CODEC_ENTRY,
    FN_DEF,
    GENERIC_BLOCK,

    TYPE_PARAM_LIST,
    PARAM_LIST,
    PARAM,
    RETURN_TYPE,
    FIELD_DEF,
    ENUM_VARIANT,
    SERVICE_ITEM,

    CLAUSE,
    CLAUSE_LIST,

    // --- Expressions ---
    LITERAL_EXPR,
    IDENT_EXPR,
    FIELD_EXPR,
    METHOD_CALL_EXPR,
    CALL_EXPR,
    INDEX_EXPR,
    BIN_EXPR,
    UNARY_EXPR,
    OLD_EXPR,
    FORALL_EXPR,
    EXISTS_EXPR,
    IF_EXPR,
    PAREN_EXPR,
    LIST_EXPR,
    CAST_EXPR,
    BLOCK_EXPR,
    GHOST_EXPR,
    APPLY_EXPR,
    LET_EXPR,
    MATCH_EXPR,
    TUPLE_EXPR,
    RANGE_EXPR,
    RESULT_EXPR,
    SELF_EXPR,

    MATCH_ARM,
    MATCH_ARM_LIST,
    ARG_LIST,

    // --- Patterns ---
    IDENT_PAT,
    LITERAL_PAT,
    WILDCARD_PAT,
    CONSTRUCTOR_PAT,
    TUPLE_PAT,
    PAT_LIST,

    // --- Types ---
    TYPE_EXPR_NODE,
    TYPE_TOKEN_LIST,

    // --- Misc ---
    ATTR,
    NAME,
    PATH,
    DOTTED_PATH,
    PROFILE_LIST,
    BODY_TOKENS,

    /// Wrapper for error recovery: contains skipped tokens.
    ERROR,

    /// Internal sentinel for `build_tree`: marks an event slot that
    /// should be ignored. Never appears in a finished tree.
    #[doc(hidden)]
    TOMBSTONE,

    #[doc(hidden)]
    __LAST,
}

impl SyntaxKind {
    /// True if this kind represents a keyword token.
    pub fn is_keyword(self) -> bool {
        (self as u16) <= (Self::YIELDS_KW as u16)
    }

    /// True if this kind represents a token (leaf), not a composite node.
    pub fn is_token(self) -> bool {
        (self as u16) <= (Self::ERROR_TOKEN as u16)
    }
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

/// The Assura language tag for rowan's generic tree types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AssuraLanguage {}

impl rowan::Language for AssuraLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        assert!(raw.0 < SyntaxKind::__LAST as u16);
        // SAFETY: SyntaxKind is repr(u16), and we checked the bound.
        unsafe { std::mem::transmute(raw.0) }
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        kind.into()
    }
}

/// Concrete syntax tree node, parameterized by AssuraLanguage.
pub type SyntaxNode = rowan::SyntaxNode<AssuraLanguage>;
/// Concrete syntax tree token, parameterized by AssuraLanguage.
pub type SyntaxToken = rowan::SyntaxToken<AssuraLanguage>;

// -----------------------------------------------------------------
// Token -> SyntaxKind mapping
// -----------------------------------------------------------------

impl From<&Token> for SyntaxKind {
    fn from(token: &Token) -> Self {
        match token {
            // Core keywords
            Token::And => Self::AND_KW,
            Token::ApiCompat => Self::API_COMPAT_KW,
            Token::As => Self::AS_KW,
            Token::Audit => Self::AUDIT_KW,
            Token::Bind => Self::BIND_KW,
            Token::Compliance => Self::COMPLIANCE_KW,
            Token::Concurrency => Self::CONCURRENCY_KW,
            Token::Contract => Self::CONTRACT_KW,
            Token::DataFlow => Self::DATA_FLOW_KW,
            Token::Effects => Self::EFFECTS_KW,
            Token::Else => Self::ELSE_KW,
            Token::Enum => Self::ENUM_KW,
            Token::Ensures => Self::ENSURES_KW,
            Token::Errors => Self::ERRORS_KW,
            Token::Evolution => Self::EVOLUTION_KW,
            Token::Extends => Self::EXTENDS_KW,
            Token::Exists => Self::EXISTS_KW,
            Token::Extern => Self::EXTERN_KW,
            Token::False => Self::FALSE_KW,
            Token::Fn => Self::FN_KW,
            Token::Forall => Self::FORALL_KW,
            Token::Idempotent => Self::IDEMPOTENT_KW,
            Token::If => Self::IF_KW,
            Token::Import => Self::IMPORT_KW,
            Token::In => Self::IN_KW,
            Token::Input => Self::INPUT_KW,
            Token::Invariant => Self::INVARIANT_KW,
            Token::Is => Self::IS_KW,
            Token::Let => Self::LET_KW,
            Token::Match => Self::MATCH_KW,
            Token::Module => Self::MODULE_KW,
            Token::MustNot => Self::MUST_NOT_KW,
            Token::Not => Self::NOT_KW,
            Token::Observe => Self::OBSERVE_KW,
            Token::Old => Self::OLD_KW,
            Token::Operation => Self::OPERATION_KW,
            Token::Or => Self::OR_KW,
            Token::Output => Self::OUTPUT_KW,
            Token::Partial => Self::PARTIAL_KW,
            Token::Performance => Self::PERFORMANCE_KW,
            Token::Privacy => Self::PRIVACY_KW,
            Token::Profile => Self::PROFILE_KW,
            Token::Project => Self::PROJECT_KW,
            Token::Protocol => Self::PROTOCOL_KW,
            Token::Pub => Self::PUB_KW,
            Token::Pure => Self::PURE_KW,
            Token::Query => Self::QUERY_KW,
            Token::Requires => Self::REQUIRES_KW,
            Token::Result_ => Self::RESULT_KW,
            Token::Retention => Self::RETENTION_KW,
            Token::Rule => Self::RULE_KW,
            Token::Self_ => Self::SELF_KW,
            Token::Serialization => Self::SERIALIZATION_KW,
            Token::Service => Self::SERVICE_KW,
            Token::States => Self::STATES_KW,
            Token::Then => Self::THEN_KW,
            Token::Transaction => Self::TRANSACTION_KW,
            Token::Transition => Self::TRANSITION_KW,
            Token::True => Self::TRUE_KW,
            Token::Type => Self::TYPE_KW,
            Token::Where => Self::WHERE_KW,

            // CORE verification
            Token::Apply => Self::APPLY_KW,
            Token::AutoTrigger => Self::AUTO_TRIGGER_KW,
            Token::Axiom => Self::AXIOM_KW,
            Token::Cases => Self::CASES_KW,
            Token::Decreases => Self::DECREASES_KW,
            Token::Define => Self::DEFINE_KW,
            Token::Eventually => Self::EVENTUALLY_KW,
            Token::EventuallyAlways => Self::EVENTUALLY_ALWAYS_KW,
            Token::EventuallyWithin => Self::EVENTUALLY_WITHIN_KW,
            Token::Fair => Self::FAIR_KW,
            Token::Ghost => Self::GHOST_KW,
            Token::Induction => Self::INDUCTION_KW,
            Token::LeadsTo => Self::LEADS_TO_KW,
            Token::Lemma => Self::LEMMA_KW,
            Token::Liveness => Self::LIVENESS_KW,
            Token::Modifies => Self::MODIFIES_KW,
            Token::Opaque => Self::OPAQUE_KW,
            Token::Prophecy => Self::PROPHECY_KW,
            Token::Property => Self::PROPERTY_KW,
            Token::Reads => Self::READS_KW,
            Token::Resolve => Self::RESOLVE_KW,
            Token::Reveal => Self::REVEAL_KW,
            Token::Trigger => Self::TRIGGER_KW,

            // MEM
            Token::Allocator => Self::ALLOCATOR_KW,
            Token::Atomic => Self::ATOMIC_KW,
            Token::AtomicLoad => Self::ATOMIC_LOAD_KW,
            Token::CircularBuffer => Self::CIRCULAR_BUFFER_KW,
            Token::Layout => Self::LAYOUT_KW,
            Token::Region => Self::REGION_KW,
            Token::SharedMemory => Self::SHARED_MEMORY_KW,
            Token::Slide => Self::SLIDE_KW,
            Token::ValidCount => Self::VALID_COUNT_KW,
            Token::WritePos => Self::WRITE_POS_KW,

            // TYPE
            Token::ErrorPolicy => Self::ERROR_POLICY_KW,
            Token::Impl => Self::IMPL_KW,
            Token::Interface => Self::INTERFACE_KW,
            Token::MustNotMask => Self::MUST_NOT_MASK_KW,
            Token::MustPropagate => Self::MUST_PROPAGATE_KW,
            Token::StructuralInvariant => Self::STRUCTURAL_INVARIANT_KW,

            // SEC
            Token::Algorithm => Self::ALGORITHM_KW,
            Token::Boundary => Self::BOUNDARY_KW,
            Token::AxiomSpec => Self::AXIOM_SPEC_KW,
            Token::CalleeGuarantees => Self::CALLEE_GUARANTEES_KW,
            Token::CallerGuarantees => Self::CALLER_GUARANTEES_KW,
            Token::Conforms => Self::CONFORMS_KW,
            Token::ConstantTime => Self::CONSTANT_TIME_KW,
            Token::Erase => Self::ERASE_KW,
            Token::ErrorConvention => Self::ERROR_CONVENTION_KW,
            Token::Export => Self::EXPORT_KW,
            Token::Ffi => Self::FFI_KW,
            Token::Secret => Self::SECRET_KW,
            Token::SecureErase => Self::SECURE_ERASE_KW,
            Token::Spec => Self::SPEC_KW,
            Token::Trust => Self::TRUST_KW,

            // CONC
            Token::AcqRel => Self::ACQ_REL_KW,
            Token::Acquire => Self::ACQUIRE_KW,
            Token::Callback => Self::CALLBACK_KW,
            Token::Deadline => Self::DEADLINE_KW,
            Token::Deterministic => Self::DETERMINISTIC_KW,
            Token::Fence => Self::FENCE_KW,
            Token::LockOrder => Self::LOCK_ORDER_KW,
            Token::LockRank => Self::LOCK_RANK_KW,
            Token::MayCall => Self::MAY_CALL_KW,
            Token::Merge => Self::MERGE_KW,
            Token::MustBe => Self::MUST_BE_KW,
            Token::MustNotCall => Self::MUST_NOT_CALL_KW,
            Token::MustNotReenter => Self::MUST_NOT_REENTER_KW,
            Token::Ordering => Self::ORDERING_KW,
            Token::Relaxed => Self::RELAXED_KW,
            Token::Release => Self::RELEASE_KW,
            Token::SeqCst => Self::SEQ_CST_KW,
            Token::StaleView => Self::STALE_VIEW_KW,
            Token::Timeout => Self::TIMEOUT_KW,
            Token::View => Self::VIEW_KW,

            // STOR
            Token::Cache => Self::CACHE_KW,
            Token::CrashPoint => Self::CRASH_POINT_KW,
            Token::DurableState => Self::DURABLE_STATE_KW,
            Token::EraseValue => Self::ERASE_VALUE_KW,
            Token::Monotonic => Self::MONOTONIC_KW,
            Token::OnCrashDuring => Self::ON_CRASH_DURING_KW,
            Token::Pinned => Self::PINNED_KW,
            Token::ProgIdempotent => Self::PROG_IDEMPOTENT_KW,
            Token::RecoversTo => Self::RECOVERS_TO_KW,
            Token::Recovery => Self::RECOVERY_KW,
            Token::Snapshot => Self::SNAPSHOT_KW,
            Token::StorageModel => Self::STORAGE_MODEL_KW,

            // FMT
            Token::Accepts => Self::ACCEPTS_KW,
            Token::BitFormat => Self::BIT_FORMAT_KW,
            Token::Bits => Self::BITS_KW,
            Token::Codec => Self::CODEC_KW,
            Token::CodecRegistry => Self::CODEC_REGISTRY_KW,
            Token::Deviation => Self::DEVIATION_KW,
            Token::EncodingMatches => Self::ENCODING_MATCHES_KW,
            Token::Format => Self::FORMAT_KW,
            Token::Integrity => Self::INTEGRITY_KW,
            Token::Magic => Self::MAGIC_KW,
            Token::Rejects => Self::REJECTS_KW,
            Token::Rfc => Self::RFC_KW,

            // NUM
            Token::MaxAbsError => Self::MAX_ABS_ERROR_KW,
            Token::MaxUlpError => Self::MAX_ULP_ERROR_KW,
            Token::Precompute => Self::PRECOMPUTE_KW,
            Token::Precision => Self::PRECISION_KW,
            Token::Table => Self::TABLE_KW,
            Token::VerifyAgainst => Self::VERIFY_AGAINST_KW,

            // PLAT
            Token::Cfg => Self::CFG_KW,
            Token::Feature => Self::FEATURE_KW,
            Token::Limit => Self::LIMIT_KW,
            Token::OnExceed => Self::ON_EXCEED_KW,
            Token::Platform => Self::PLATFORM_KW,
            Token::Variant => Self::VARIANT_KW,

            // PERF
            Token::AmortizedTime => Self::AMORTIZED_TIME_KW,
            Token::Bounds => Self::BOUNDS_KW,
            Token::Complexity => Self::COMPLEXITY_KW,
            Token::UnsafeEscape => Self::UNSAFE_ESCAPE_KW,

            // TEST
            Token::Convergence => Self::CONVERGENCE_KW,
            Token::Equivalent => Self::EQUIVALENT_KW,
            Token::GenerateTests => Self::GENERATE_TESTS_KW,
            Token::Passes => Self::PASSES_KW,
            Token::Quality => Self::QUALITY_KW,
            Token::Refinement => Self::REFINEMENT_KW,

            // MISC
            Token::Extensible => Self::EXTENSIBLE_KW,
            Token::Frozen => Self::FROZEN_KW,
            Token::Incremental => Self::INCREMENTAL_KW,
            Token::Yields => Self::YIELDS_KW,

            // Literals
            Token::Float(_) => Self::FLOAT_LIT,
            Token::Int(_) => Self::INT_LIT,
            Token::String(_) => Self::STRING_LIT,

            // Identifiers
            Token::Ident(_) => Self::IDENT,

            // Punctuation
            Token::LBrace => Self::L_BRACE,
            Token::RBrace => Self::R_BRACE,
            Token::LParen => Self::L_PAREN,
            Token::RParen => Self::R_PAREN,
            Token::LBracket => Self::L_BRACKET,
            Token::RBracket => Self::R_BRACKET,
            Token::LAngle => Self::L_ANGLE,
            Token::RAngle => Self::R_ANGLE,
            Token::Comma => Self::COMMA,
            Token::Colon => Self::COLON,
            Token::Semicolon => Self::SEMICOLON,
            Token::Dot => Self::DOT,
            Token::Pipe => Self::PIPE,
            Token::Question => Self::QUESTION,
            Token::Arrow => Self::ARROW,
            Token::FatArrow => Self::FAT_ARROW,
            Token::Hash => Self::HASH,
            Token::At => Self::AT,
            Token::Equals => Self::EQUALS,

            // Operators
            Token::Concat => Self::CONCAT,
            Token::Plus => Self::PLUS,
            Token::Minus => Self::MINUS,
            Token::Star => Self::STAR,
            Token::Slash => Self::SLASH,
            Token::Percent => Self::PERCENT,
            Token::Eq => Self::EQ,
            Token::Neq => Self::NEQ,
            Token::Lte => Self::LTE,
            Token::Gte => Self::GTE,
            Token::AndAnd => Self::AND_AND,
            Token::OrOr => Self::OR_OR,
            Token::Bang => Self::BANG,
            Token::AmpMut => Self::AMP_MUT,
            Token::Amp => Self::AMP,
            Token::DotDot => Self::DOT_DOT,
            Token::Caret => Self::CARET,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_kind_round_trip() {
        // Verify the Language trait conversion round-trips.
        use rowan::Language;
        for raw in 0..SyntaxKind::__LAST as u16 {
            let kind = AssuraLanguage::kind_from_raw(rowan::SyntaxKind(raw));
            let back = AssuraLanguage::kind_to_raw(kind);
            assert_eq!(back.0, raw);
        }
    }

    #[test]
    fn token_to_syntax_kind_coverage() {
        // Spot-check key tokens map correctly.
        assert_eq!(SyntaxKind::from(&Token::Contract), SyntaxKind::CONTRACT_KW);
        assert_eq!(SyntaxKind::from(&Token::Fn), SyntaxKind::FN_KW);
        assert_eq!(SyntaxKind::from(&Token::LBrace), SyntaxKind::L_BRACE);
        assert_eq!(SyntaxKind::from(&Token::Plus), SyntaxKind::PLUS);
        assert_eq!(
            SyntaxKind::from(&Token::Ident("x".into())),
            SyntaxKind::IDENT
        );
        assert_eq!(
            SyntaxKind::from(&Token::Int("42".into())),
            SyntaxKind::INT_LIT
        );
    }

    #[test]
    fn is_keyword() {
        assert!(SyntaxKind::CONTRACT_KW.is_keyword());
        assert!(SyntaxKind::YIELDS_KW.is_keyword());
        assert!(!SyntaxKind::IDENT.is_keyword());
        assert!(!SyntaxKind::SOURCE_FILE.is_keyword());
    }

    #[test]
    fn is_token() {
        assert!(SyntaxKind::IDENT.is_token());
        assert!(SyntaxKind::PLUS.is_token());
        assert!(SyntaxKind::ERROR_TOKEN.is_token());
        assert!(!SyntaxKind::SOURCE_FILE.is_token());
        assert!(!SyntaxKind::CONTRACT_DECL.is_token());
    }
}
