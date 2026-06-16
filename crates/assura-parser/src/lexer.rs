use logos::Logos;

#[derive(Logos, Debug, Clone, PartialEq, Eq, Hash)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip(r"//[^\n]*", allow_greedy = true))]
#[logos(skip r"/\*([^*]|\*[^/])*\*/")]
pub enum Token {
    // ===================================================================
    // Keywords — full spec coverage (~199 keywords, Appendix A)
    // Organized by category for maintainability.
    // ===================================================================

    // --- Section 1.1: Core language keywords ---
    #[token("and")]
    And,
    #[token("api_compat")]
    ApiCompat,
    #[token("as")]
    As,
    #[token("audit")]
    Audit,
    #[token("bind")]
    Bind,
    #[token("compliance")]
    Compliance,
    #[token("concurrency")]
    Concurrency,
    #[token("contract")]
    Contract,
    #[token("data-flow")]
    DataFlow,
    #[token("effects")]
    Effects,
    #[token("else")]
    Else,
    #[token("enum")]
    Enum,
    #[token("ensures")]
    Ensures,
    #[token("errors")]
    Errors,
    #[token("evolution")]
    Evolution,
    #[token("extends")]
    Extends,
    #[token("exists")]
    Exists,
    #[token("extern")]
    Extern,
    #[token("false")]
    False,
    #[token("fn")]
    Fn,
    #[token("forall")]
    Forall,
    #[token("idempotent")]
    Idempotent,
    #[token("if")]
    If,
    #[token("import")]
    Import,
    #[token("in")]
    In,
    #[token("input")]
    Input,
    #[token("invariant")]
    Invariant,
    #[token("is")]
    Is,
    #[token("let")]
    Let,
    #[token("match")]
    Match,
    #[token("module")]
    Module,
    #[token("must-not")]
    MustNot,
    #[token("not")]
    Not,
    #[token("observe")]
    Observe,
    #[token("old")]
    Old,
    #[token("operation")]
    Operation,
    #[token("or")]
    Or,
    #[token("output")]
    Output,
    #[token("partial")]
    Partial,
    #[token("performance")]
    Performance,
    #[token("privacy")]
    Privacy,
    #[token("profile")]
    Profile,
    #[token("project")]
    Project,
    #[token("protocol")]
    Protocol,
    #[token("pub")]
    Pub,
    #[token("pure")]
    Pure,
    #[token("query")]
    Query,
    #[token("requires")]
    Requires,
    #[token("result")]
    Result_,
    #[token("retention")]
    Retention,
    #[token("rule")]
    Rule,
    #[token("self")]
    Self_,
    #[token("serialization")]
    Serialization,
    #[token("service")]
    Service,
    #[token("states")]
    States,
    #[token("then")]
    Then,
    #[token("transaction")]
    Transaction,
    #[token("transition")]
    Transition,
    #[token("true")]
    True,
    #[token("type")]
    Type,
    #[token("where")]
    Where,

    // --- CORE: verification infrastructure (Section 14.CORE) ---
    #[token("apply")]
    Apply,
    #[token("auto_trigger")]
    AutoTrigger,
    #[token("axiom")]
    Axiom,
    #[token("cases")]
    Cases,
    #[token("decreases")]
    Decreases,
    #[token("define")]
    Define,
    #[token("eventually")]
    Eventually,
    #[token("eventually_always")]
    EventuallyAlways,
    #[token("eventually_within")]
    EventuallyWithin,
    #[token("fair")]
    Fair,
    #[token("ghost")]
    Ghost,
    #[token("induction")]
    Induction,
    #[token("leads_to")]
    LeadsTo,
    #[token("lemma")]
    Lemma,
    #[token("liveness")]
    Liveness,
    #[token("modifies")]
    Modifies,
    #[token("opaque")]
    Opaque,
    #[token("prophecy")]
    Prophecy,
    #[token("property")]
    Property,
    #[token("reads")]
    Reads,
    #[token("resolve")]
    Resolve,
    #[token("reveal")]
    Reveal,
    #[token("trigger")]
    Trigger,

    // --- MEM: memory safety (Section 14.MEM) ---
    #[token("allocator")]
    Allocator,
    #[token("atomic")]
    Atomic,
    #[token("atomic_load")]
    AtomicLoad,
    #[token("circular_buffer")]
    CircularBuffer,
    #[token("layout")]
    Layout,
    #[token("region")]
    Region,
    #[token("shared_memory")]
    SharedMemory,
    #[token("slide")]
    Slide,
    #[token("valid_count")]
    ValidCount,
    #[token("write_pos")]
    WritePos,

    // --- TYPE: types and contracts (Section 14.TYPE) ---
    #[token("error_policy")]
    ErrorPolicy,
    #[token("impl")]
    Impl,
    #[token("interface")]
    Interface,
    #[token("must_not_mask")]
    MustNotMask,
    #[token("must_propagate")]
    MustPropagate,
    #[token("structural_invariant")]
    StructuralInvariant,

    // --- SEC: trust and security (Section 14.SEC) ---
    #[token("algorithm")]
    Algorithm,
    #[token("boundary")]
    Boundary,
    #[token("axiom_spec")]
    AxiomSpec,
    #[token("callee_guarantees")]
    CalleeGuarantees,
    #[token("caller_guarantees")]
    CallerGuarantees,
    #[token("conforms")]
    Conforms,
    #[token("constant_time")]
    ConstantTime,
    #[token("erase")]
    Erase,
    #[token("error_convention")]
    ErrorConvention,
    #[token("export")]
    Export,
    #[token("ffi")]
    Ffi,
    #[token("secret")]
    Secret,
    #[token("secure_erase")]
    SecureErase,
    #[token("spec")]
    Spec,
    #[token("trust")]
    Trust,

    // --- CONC: concurrency (Section 14.CONC) ---
    #[token("acq_rel")]
    AcqRel,
    #[token("acquire")]
    Acquire,
    #[token("callback")]
    Callback,
    #[token("deadline")]
    Deadline,
    #[token("deterministic")]
    Deterministic,
    #[token("fence")]
    Fence,
    #[token("lock_order")]
    LockOrder,
    #[token("lock_rank")]
    LockRank,
    #[token("may_call")]
    MayCall,
    #[token("merge")]
    Merge,
    #[token("must_be")]
    MustBe,
    #[token("must_not_call")]
    MustNotCall,
    #[token("must_not_reenter")]
    MustNotReenter,
    #[token("ordering")]
    Ordering,
    #[token("relaxed")]
    Relaxed,
    #[token("release")]
    Release,
    #[token("seq_cst")]
    SeqCst,
    #[token("stale_view")]
    StaleView,
    #[token("timeout")]
    Timeout,
    #[token("view")]
    View,

    // --- STOR: storage and durability (Section 14.STOR) ---
    #[token("cache")]
    Cache,
    #[token("crash_point")]
    CrashPoint,
    #[token("durable_state")]
    DurableState,
    #[token("erase_value")]
    EraseValue,
    #[token("monotonic")]
    Monotonic,
    #[token("on_crash_during")]
    OnCrashDuring,
    #[token("pinned")]
    Pinned,
    #[token("prog_idempotent")]
    ProgIdempotent,
    #[token("recovers_to")]
    RecoversTo,
    #[token("recovery")]
    Recovery,
    #[token("snapshot")]
    Snapshot,
    #[token("storage_model")]
    StorageModel,

    // --- FMT: data formats and parsing (Section 14.FMT) ---
    #[token("accepts")]
    Accepts,
    #[token("bit_format")]
    BitFormat,
    #[token("bits")]
    Bits,
    #[token("codec")]
    Codec,
    #[token("codec_registry")]
    CodecRegistry,
    #[token("deviation")]
    Deviation,
    #[token("encoding_matches")]
    EncodingMatches,
    #[token("format")]
    Format,
    #[token("integrity")]
    Integrity,
    #[token("magic")]
    Magic,
    #[token("rejects")]
    Rejects,
    #[token("rfc")]
    Rfc,

    // --- NUM: numerical and precision (Section 14.NUM) ---
    #[token("max_abs_error")]
    MaxAbsError,
    #[token("max_ulp_error")]
    MaxUlpError,
    #[token("precompute")]
    Precompute,
    #[token("precision")]
    Precision,
    #[token("table")]
    Table,
    #[token("verify_against")]
    VerifyAgainst,

    // --- PLAT: platform and configuration (Section 14.PLAT) ---
    #[token("cfg")]
    Cfg,
    #[token("feature")]
    Feature,
    #[token("limit")]
    Limit,
    #[token("on_exceed")]
    OnExceed,
    #[token("platform")]
    Platform,
    #[token("variant")]
    Variant,

    // --- PERF: performance (Section 14.PERF) ---
    #[token("amortized_time")]
    AmortizedTime,
    #[token("bounds")]
    Bounds,
    #[token("complexity")]
    Complexity,
    #[token("unsafe_escape")]
    UnsafeEscape,

    // --- TEST: testing and verification (Section 14.TEST) ---
    #[token("convergence")]
    Convergence,
    #[token("equivalent")]
    Equivalent,
    #[token("generate_tests")]
    GenerateTests,
    #[token("passes")]
    Passes,
    #[token("quality")]
    Quality,
    #[token("refinement")]
    Refinement,

    // --- MISC: specialized (Section 14.MISC) ---
    #[token("extensible")]
    Extensible,
    #[token("frozen")]
    Frozen,
    #[token("incremental")]
    Incremental,
    #[token("yields")]
    Yields,

    // --- Literals ---
    #[regex(r"-?[0-9][0-9_]*\.[0-9][0-9_]*", |lex| lex.slice().to_string())]
    Float(String),

    #[regex(r"-?[0-9][0-9_]*", |lex| lex.slice().to_string(), priority = 3)]
    Int(String),

    #[regex(r#""([^"\\]|\\.)*""#, |lex| {
        let s = lex.slice();
        s[1..s.len()-1].to_string()
    })]
    String(String),

    // --- Identifiers ---
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string(), priority = 2)]
    Ident(String),

    // --- Punctuation ---
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("<")]
    LAngle,
    #[token(">")]
    RAngle,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token(";")]
    Semicolon,
    #[token(".")]
    Dot,
    #[token("|")]
    Pipe,
    #[token("?")]
    Question,
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,
    #[token("#")]
    Hash,
    #[token("@")]
    At,
    #[token("=")]
    Equals,

    // --- Operators ---
    #[token("++")]
    Concat,
    #[token("+")]
    Plus,
    #[token("-", priority = 1)]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("==")]
    Eq,
    #[token("!=")]
    Neq,
    #[token("<=")]
    Lte,
    #[token(">=")]
    Gte,
    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,
    #[token("!")]
    Bang,
    #[token("&mut")]
    AmpMut,
    #[token("&")]
    Amp,
    #[token("..")]
    DotDot,
    #[token("^")]
    Caret,
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Keywords: display as the keyword text
            Token::And => write!(f, "'and'"),
            Token::As => write!(f, "'as'"),
            Token::Contract => write!(f, "'contract'"),
            Token::Effects => write!(f, "'effects'"),
            Token::Else => write!(f, "'else'"),
            Token::Enum => write!(f, "'enum'"),
            Token::Ensures => write!(f, "'ensures'"),
            Token::Exists => write!(f, "'exists'"),
            Token::Extern => write!(f, "'extern'"),
            Token::Fn => write!(f, "'fn'"),
            Token::Forall => write!(f, "'forall'"),
            Token::Ghost => write!(f, "'ghost'"),
            Token::If => write!(f, "'if'"),
            Token::Import => write!(f, "'import'"),
            Token::In => write!(f, "'in'"),
            Token::Input => write!(f, "'input'"),
            Token::Invariant => write!(f, "'invariant'"),
            Token::Lemma => write!(f, "'lemma'"),
            Token::Let => write!(f, "'let'"),
            Token::Match => write!(f, "'match'"),
            Token::Module => write!(f, "'module'"),
            Token::Modifies => write!(f, "'modifies'"),
            Token::Operation => write!(f, "'operation'"),
            Token::Opaque => write!(f, "'opaque'"),
            Token::Output => write!(f, "'output'"),
            Token::Pure => write!(f, "'pure'"),
            Token::Query => write!(f, "'query'"),
            Token::Requires => write!(f, "'requires'"),
            Token::Service => write!(f, "'service'"),
            Token::States => write!(f, "'states'"),
            Token::Then => write!(f, "'then'"),
            Token::Type => write!(f, "'type'"),
            // Punctuation
            Token::LBrace => write!(f, "'{{'"),
            Token::RBrace => write!(f, "'}}'"),
            Token::LParen => write!(f, "'('"),
            Token::RParen => write!(f, "')'"),
            Token::LBracket => write!(f, "'['"),
            Token::RBracket => write!(f, "']'"),
            Token::LAngle => write!(f, "'<'"),
            Token::RAngle => write!(f, "'>'"),
            Token::Comma => write!(f, "','"),
            Token::Colon => write!(f, "':'"),
            Token::Semicolon => write!(f, "';'"),
            Token::Dot => write!(f, "'.'"),
            Token::Arrow => write!(f, "'->'"),
            Token::FatArrow => write!(f, "'=>'"),
            Token::Hash => write!(f, "'#'"),
            Token::Equals => write!(f, "'='"),
            // Operators
            Token::Plus => write!(f, "'+'"),
            Token::Minus => write!(f, "'-'"),
            Token::Star => write!(f, "'*'"),
            Token::Slash => write!(f, "'/'"),
            Token::Eq => write!(f, "'=='"),
            Token::Neq => write!(f, "'!='"),
            Token::Lte => write!(f, "'<='"),
            Token::Gte => write!(f, "'>='"),
            Token::AndAnd => write!(f, "'&&'"),
            Token::OrOr => write!(f, "'||'"),
            Token::Bang => write!(f, "'!'"),
            Token::DotDot => write!(f, "'..'"),
            // Literals and identifiers
            Token::Ident(s) => write!(f, "identifier '{s}'"),
            Token::Int(s) => write!(f, "integer {s}"),
            Token::Float(s) => write!(f, "float {s}"),
            Token::String(s) => write!(f, "string \"{s}\""),
            Token::True => write!(f, "'true'"),
            Token::False => write!(f, "'false'"),
            Token::Self_ => write!(f, "'self'"),
            Token::Result_ => write!(f, "'result'"),
            // Fall back to Debug for less common tokens
            other => write!(f, "{other:?}"),
        }
    }
}
