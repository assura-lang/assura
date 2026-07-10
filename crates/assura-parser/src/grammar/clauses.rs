//! Clause parsing: `requires expr`, `ensures { expr }`, `effects: pure`, etc.
//!
//! Clauses are the specification annotations attached to contracts,
//! functions, and service operations.

use crate::cst::Parser;
use crate::syntax_kind::SyntaxKind;

use super::expressions;

/// True if the current token can start a clause.
pub(crate) fn at_clause_start(p: &mut Parser) -> bool {
    matches!(
        p.current(),
        SyntaxKind::REQUIRES_KW
            | SyntaxKind::ENSURES_KW
            | SyntaxKind::EFFECTS_KW
            | SyntaxKind::INVARIANT_KW
            | SyntaxKind::MODIFIES_KW
            | SyntaxKind::INPUT_KW
            | SyntaxKind::OUTPUT_KW
            | SyntaxKind::ERRORS_KW
            | SyntaxKind::RULE_KW
            | SyntaxKind::DATA_FLOW_KW
            | SyntaxKind::MUST_NOT_KW
            | SyntaxKind::DECREASES_KW
            | SyntaxKind::GHOST_KW
            | SyntaxKind::DEFINE_KW
            | SyntaxKind::PROPERTY_KW
            | SyntaxKind::CONSTANT_TIME_KW
            | SyntaxKind::MUST_BE_KW
            | SyntaxKind::VERIFY_AGAINST_KW
            | SyntaxKind::READS_KW
            | SyntaxKind::BOUNDS_KW
            | SyntaxKind::INTERFACE_KW
            | SyntaxKind::EXTENDS_KW
            | SyntaxKind::IMPL_KW
            | SyntaxKind::CONFORMS_KW
            | SyntaxKind::ORDERING_KW
            | SyntaxKind::TRUST_KW
            | SyntaxKind::BOUNDARY_KW
            | SyntaxKind::MUST_PROPAGATE_KW
            | SyntaxKind::OPERATION_KW
            | SyntaxKind::QUERY_KW
            | SyntaxKind::STATES_KW
            | SyntaxKind::MUST_NOT_MASK_KW
            | SyntaxKind::PROTOCOL_KW
            | SyntaxKind::TRANSITION_KW
            // Feature clause keywords with dedicated SyntaxKind tokens (#716-#726)
            | SyntaxKind::LIVENESS_KW
            | SyntaxKind::EVENTUALLY_KW
            | SyntaxKind::LEADS_TO_KW
            | SyntaxKind::AUTO_TRIGGER_KW
            | SyntaxKind::TRIGGER_KW
            | SyntaxKind::SECRET_KW
            | SyntaxKind::SECURE_ERASE_KW
            | SyntaxKind::ACQ_REL_KW
            | SyntaxKind::SEQ_CST_KW
            | SyntaxKind::LOCK_RANK_KW
            | SyntaxKind::MUST_NOT_REENTER_KW
            | SyntaxKind::ERROR_POLICY_KW
            | SyntaxKind::GENERATE_TESTS_KW
    ) || is_domain_keyword_clause(p)
        || is_ident_clause_start(p)
}

/// Ident-based keywords that START a new clause (used by `at_clause_start()`).
/// These are a subset of the stopper keywords: everything that starts a clause
/// also stops the previous one, but not vice versa.
const IDENT_CLAUSE_STARTERS: &[&str] = &[
    // `step` / `resume` are MISC.1 nested heads inside `incremental` blocks.
    // They are parsed via `loose_clause` in generic_block bodies rather than as
    // global starters so `transition A -> B via step` keeps `step` in the
    // transition raw body (#833).
    "resume",
    "assume",
    "prove",
    "validate",
    "taint",
    "verify",
    "example",
    "strategy",
    "promise",
    "bound",
    "writes",
    "method",
    "implements",
    "key_size",
    "nonce_size",
    "tag_verified",
    "tag_check",
    "nonce",
    "decrypt",
    "decryption",
    "spec",
    "crypto",
    // Domain checker clause keywords (used by assura-types wiring functions)
    "alloc",
    "dealloc",
    "arena",
    "circular_buffer",
    "ring_buffer",
    "push",
    "pop",
    "non_reentrant",
    "callback",
    "deterministic",
    "deadline",
    "operation",
    "crash_recovery",
    "wal",
    "write_data",
    "write_wal",
    "page_cache",
    "mvcc",
    "snapshot_isolation",
    "savepoint",
    "rollback",
    "monotonic",
    "monotonic_update",
    "storage_failure",
    "binary_format",
    "field",
    "encoding",
    "string_encoding",
    "checksum",
    "protocol",
    "state",
    "transition",
    "state_machine",
    "precision",
    "numerical_precision",
    "precomputed_table",
    "lookup_table",
    "platform",
    "target_platform",
    "abstraction",
    "feature_flag",
    "when_flag",
    "resource_limit",
    "limit",
    "use_resource",
    "obligation",
    "safety_proof",
    "complexity",
    "measured_complexity",
    "time_complexity",
    "equivalent",
    "behavioral_equiv",
    "refinement_pass",
    "discharge_pass",
    "multi_pass",
    "multi_pass_refinement",
    "version",
    "suspend_invariant",
    "restore_invariant",
    "structural_invariant",
    "must_check",
    "catch",
    "opaque",
    "reveal",
    "crypto_spec",
    "lock",
    "order",
    "acquire",
    "send",
    "message",
    "must_not_mask",
    "must_propagate",
    "must_preserve_detail",
    "update",
    "shared",
    "concurrent",
    "access_mode",
    "bit_layout",
    "bit_level",
    "bit_field",
    "strict_triggers",
    // #717: SEC.5 DependentTypes
    "dependent",
    "label",
    // #718: MEM.2 FixedWidth
    "fixed_width",
    "width",
    // #719: STOR.6 StorageFailure (primary keyword; alias storage_failure already present)
    "failure_mode",
    // #720: FMT.6 ProtocolGrammar (primary keyword; alias state_machine already present)
    "protocol_grammar",
    // #721: TEST.1 TestGenCoverage
    "test_gen",
    // #723: SEC.3 SecureErasure
    "zeroize",
    // #725: CORE.5 TriggerPatterns
    "trigger_pattern",
    // #726: Missing alias keywords across 17 features (ident-based only)
    "no_reentrant",
    "axiomatic",
    "frame",
    "byte_layout",
    "charset",
    "circular",
    "incremental_contract",
    "scoped_invariant",
    "ulp_bound",
    "complexity_bound",
    "platform_abstraction",
    "write_ahead",
    "buffer_pool",
    "behavioral_equivalence",
    // MISC.1 incremental block metadata (also used as stoppers between heads).
    // `on` is intentionally omitted: `on step { ... }` is handled by
    // `loose_clause` so the secondary label is not treated as a new clause head.
    "yields",
    "completes",
];

/// Ident-based keywords that STOP a clause body but do NOT start one.
/// These are declaration-like keywords (feature_max, incremental, etc.)
/// that terminate the current clause but are not themselves clause heads.
const IDENT_CLAUSE_STOPPERS_ONLY: &[&str] = &["feature_max", "incremental", "safety", "security"];

/// Check if a SyntaxKind is a domain keyword clause starter (without needing Parser).
///
/// Note: `INCREMENTAL_KW` is intentionally **not** a clause starter. It introduces
/// a top-level `incremental Name { ... }` block (MISC.1 / `BlockKind::Incremental`).
/// Treating it as a clause head made the following fn absorb the block as
/// `Other("incremental")` with body `Ident(Name)` and drop the block body (#833).
/// Use the ident clause form `incremental_contract { ... }` for annotations on
/// functions. `INCREMENTAL_KW` remains a clause **stopper** so prior clause bodies
/// end correctly before a following incremental block.
pub(crate) fn is_domain_keyword_clause_kind(k: SyntaxKind) -> bool {
    matches!(
        k,
        SyntaxKind::ALLOCATOR_KW
            | SyntaxKind::ATOMIC_KW
            | SyntaxKind::CIRCULAR_BUFFER_KW
            | SyntaxKind::REGION_KW
            | SyntaxKind::SHARED_MEMORY_KW
            | SyntaxKind::CALLBACK_KW
            | SyntaxKind::DEADLINE_KW
            | SyntaxKind::DETERMINISTIC_KW
            | SyntaxKind::LOCK_ORDER_KW
            | SyntaxKind::TIMEOUT_KW
            | SyntaxKind::MONOTONIC_KW
            | SyntaxKind::PRECISION_KW
            | SyntaxKind::PLATFORM_KW
            | SyntaxKind::COMPLEXITY_KW
            | SyntaxKind::EQUIVALENT_KW
            | SyntaxKind::OPAQUE_KW
            | SyntaxKind::STRUCTURAL_INVARIANT_KW
            | SyntaxKind::UNSAFE_ESCAPE_KW
            | SyntaxKind::LIMIT_KW
            | SyntaxKind::FEATURE_KW
            | SyntaxKind::FORMAT_KW
            | SyntaxKind::CODEC_KW
            | SyntaxKind::SNAPSHOT_KW
            | SyntaxKind::RECOVERY_KW
            | SyntaxKind::FENCE_KW
            | SyntaxKind::ACQUIRE_KW
            | SyntaxKind::RELEASE_KW
            | SyntaxKind::SPEC_KW
            | SyntaxKind::REFINEMENT_KW
            | SyntaxKind::LAYOUT_KW
            | SyntaxKind::MUST_NOT_MASK_KW
            | SyntaxKind::PROTOCOL_KW
            | SyntaxKind::TRANSITION_KW
    )
}

/// Check if ident text is a clause starter keyword.
pub(crate) fn is_ident_clause_text(text: &str) -> bool {
    IDENT_CLAUSE_STARTERS.contains(&text)
}

/// Domain-specific keyword tokens that can start a clause inside contract/fn bodies.
/// These have dedicated SyntaxKind variants (not plain IDENT) but are used as clause
/// keywords by the type checker's wiring functions.
///
/// `INCREMENTAL_KW` is excluded: see [`is_domain_keyword_clause_kind`].
fn is_domain_keyword_clause(p: &mut Parser) -> bool {
    is_domain_keyword_clause_kind(p.current())
}

fn is_ident_clause_start(p: &mut Parser) -> bool {
    if p.current() != SyntaxKind::IDENT {
        return false;
    }
    IDENT_CLAUSE_STARTERS.contains(&p.current_text())
}

/// True if this kind is a clause stopper (starts a new clause/decl).
pub(crate) fn is_clause_stopper_kind(k: SyntaxKind) -> bool {
    matches!(
        k,
        // Clause keywords
        SyntaxKind::REQUIRES_KW
            | SyntaxKind::ENSURES_KW
            | SyntaxKind::EFFECTS_KW
            | SyntaxKind::INVARIANT_KW
            | SyntaxKind::MODIFIES_KW
            | SyntaxKind::INPUT_KW
            | SyntaxKind::OUTPUT_KW
            | SyntaxKind::ERRORS_KW
            | SyntaxKind::RULE_KW
            | SyntaxKind::DATA_FLOW_KW
            | SyntaxKind::MUST_NOT_KW
            // Block delimiters
            | SyntaxKind::L_BRACE
            | SyntaxKind::R_BRACE
            // Declaration-starting keywords
            | SyntaxKind::CONTRACT_KW
            | SyntaxKind::TYPE_KW
            | SyntaxKind::ENUM_KW
            | SyntaxKind::EXTERN_KW
            | SyntaxKind::FN_KW
            | SyntaxKind::SERVICE_KW
            | SyntaxKind::IMPORT_KW
            | SyntaxKind::MODULE_KW
            | SyntaxKind::PROJECT_KW
            | SyntaxKind::AXIOM_KW
            | SyntaxKind::LEMMA_KW
            // Clause-like keywords
            | SyntaxKind::SPEC_KW
            | SyntaxKind::DEFINE_KW
            | SyntaxKind::PROPERTY_KW
            | SyntaxKind::CONSTANT_TIME_KW
            | SyntaxKind::MUST_BE_KW
            | SyntaxKind::VERIFY_AGAINST_KW
            | SyntaxKind::READS_KW
            | SyntaxKind::BOUNDS_KW
            | SyntaxKind::DECREASES_KW
            | SyntaxKind::OPERATION_KW
            | SyntaxKind::QUERY_KW
            | SyntaxKind::STATES_KW
            // Interface
            | SyntaxKind::INTERFACE_KW
            | SyntaxKind::EXTENDS_KW
            | SyntaxKind::IMPL_KW
            // Crypto conformance
            | SyntaxKind::CONFORMS_KW
            // Memory ordering
            | SyntaxKind::ORDERING_KW
            // Trust and boundary
            | SyntaxKind::TRUST_KW
            | SyntaxKind::BOUNDARY_KW
            | SyntaxKind::MUST_PROPAGATE_KW
            // Generic block keywords
            | SyntaxKind::TABLE_KW
            | SyntaxKind::FEATURE_KW
            // Domain-specific clause keywords
            | SyntaxKind::ALLOCATOR_KW
            | SyntaxKind::ATOMIC_KW
            | SyntaxKind::CIRCULAR_BUFFER_KW
            | SyntaxKind::REGION_KW
            | SyntaxKind::SHARED_MEMORY_KW
            | SyntaxKind::CALLBACK_KW
            | SyntaxKind::DEADLINE_KW
            | SyntaxKind::DETERMINISTIC_KW
            | SyntaxKind::LOCK_ORDER_KW
            | SyntaxKind::TIMEOUT_KW
            | SyntaxKind::MONOTONIC_KW
            | SyntaxKind::PRECISION_KW
            | SyntaxKind::PLATFORM_KW
            | SyntaxKind::COMPLEXITY_KW
            | SyntaxKind::EQUIVALENT_KW
            | SyntaxKind::INCREMENTAL_KW
            | SyntaxKind::OPAQUE_KW
            | SyntaxKind::STRUCTURAL_INVARIANT_KW
            | SyntaxKind::UNSAFE_ESCAPE_KW
            | SyntaxKind::LIMIT_KW
            | SyntaxKind::FORMAT_KW
            | SyntaxKind::CODEC_KW
            | SyntaxKind::SNAPSHOT_KW
            | SyntaxKind::RECOVERY_KW
            | SyntaxKind::FENCE_KW
            | SyntaxKind::ACQUIRE_KW
            | SyntaxKind::RELEASE_KW
            | SyntaxKind::REFINEMENT_KW
            | SyntaxKind::LAYOUT_KW
            | SyntaxKind::MUST_NOT_MASK_KW
            | SyntaxKind::PROTOCOL_KW
            | SyntaxKind::TRANSITION_KW
            // Prophecy and ghost declarations
            | SyntaxKind::PROPHECY_KW
            | SyntaxKind::GHOST_KW
            | SyntaxKind::BIND_KW
            | SyntaxKind::CODEC_REGISTRY_KW
            // Feature clause keywords (#716-#726)
            | SyntaxKind::LIVENESS_KW
            | SyntaxKind::EVENTUALLY_KW
            | SyntaxKind::LEADS_TO_KW
            | SyntaxKind::AUTO_TRIGGER_KW
            | SyntaxKind::TRIGGER_KW
            | SyntaxKind::SECRET_KW
            | SyntaxKind::SECURE_ERASE_KW
            | SyntaxKind::ACQ_REL_KW
            | SyntaxKind::SEQ_CST_KW
            | SyntaxKind::LOCK_RANK_KW
            | SyntaxKind::MUST_NOT_REENTER_KW
            | SyntaxKind::ERROR_POLICY_KW
            | SyntaxKind::GENERATE_TESTS_KW
    )
}

/// True if the current token is a clause stopper (including ident-based ones).
pub(crate) fn is_clause_stopper(p: &mut Parser) -> bool {
    if is_clause_stopper_kind(p.current()) {
        return true;
    }
    if p.current() == SyntaxKind::IDENT {
        let text = p.current_text();
        return IDENT_CLAUSE_STARTERS.contains(&text) || IDENT_CLAUSE_STOPPERS_ONLY.contains(&text);
    }
    false
}

/// True if this clause kind should have an expression body.
fn is_expr_clause_kind(k: SyntaxKind) -> bool {
    matches!(
        k,
        SyntaxKind::REQUIRES_KW
            | SyntaxKind::ENSURES_KW
            | SyntaxKind::INVARIANT_KW
            | SyntaxKind::DECREASES_KW
            | SyntaxKind::RULE_KW
            | SyntaxKind::MUST_NOT_KW
            | SyntaxKind::CONFORMS_KW
            | SyntaxKind::MONOTONIC_KW
            // TRANSITION_KW intentionally uses a raw body: incremental / protocol
            // forms are `transition A -> B via step` or `transition s(a, b)`, not a
            // single expression. Expression mode stopped at the first atom and left
            // `->` / `via` as unparsable residues (#833).
            | SyntaxKind::PROTOCOL_KW
            | SyntaxKind::EQUIVALENT_KW
    )
}

/// Parse a clause: keyword + body
pub(crate) fn clause(p: &mut Parser) {
    let m = p.open();

    let is_expr = is_expr_clause_kind(p.current()) || is_ident_expr_clause(p);

    // Consume the clause keyword
    p.bump();

    // Parse the body
    if is_expr {
        clause_body_expr(p);
    } else {
        clause_body(p);
    }

    m.complete(p, SyntaxKind::CLAUSE);
}

/// Parse a clause whose head is any keyword/ident (not necessarily a registered
/// starter). Used inside generic blocks for MISC.1 metadata forms such as
/// `yields: T`, `completes: T`, and `on step { ... }`.
///
/// Always uses a raw body so tokens like `via step` and nested braces are kept.
/// Supports a secondary label before a brace body (`on step { ... }`,
/// `on abort { ... }`) even when the secondary word is itself a clause starter.
pub(crate) fn loose_clause(p: &mut Parser) {
    let m = p.open();
    if p.at_keyword_or_ident() {
        p.bump();
    } else {
        p.error_at_current("expected clause keyword".into());
    }
    // `on step { ... }` / `on abort { ... }`: absorb the event name, then the brace.
    if p.at_keyword_or_ident() && p.nth(1) == SyntaxKind::L_BRACE {
        p.bump(); // step / abort / ...
    }
    clause_body(p);
    m.complete(p, SyntaxKind::CLAUSE);
}

/// True if this ident-based clause should have an expression body.
fn is_ident_expr_clause(p: &mut Parser) -> bool {
    if p.current() != SyntaxKind::IDENT {
        return false;
    }
    matches!(
        p.current_text(),
        "key_size"
            | "nonce_size"
            | "nonce"
            | "spec"
            | "crypto"
            | "prove"
            | "validate"
            | "assume"
            | "example"
            | "update"
            | "monotonic"
            | "monotone"
            | "send"
            | "transition"
            | "behavioral_equiv"
            | "shared"
            | "concurrent"
            | "access_mode"
            | "bit_layout"
            | "bit_level"
            | "bit_field"
    )
}

/// Parse an expression clause body: try expression first, fall back to raw.
fn clause_body_expr(p: &mut Parser) {
    // Braced body: [:]{ expr [, expr]* }
    if p.at(SyntaxKind::COLON) && p.nth(1) == SyntaxKind::L_BRACE {
        p.bump(); // :
        p.bump_delim(); // { + trailing trivia for tight inner spans (see #335)
        expr_list_until(p, SyntaxKind::R_BRACE);
        super::expect_closer(p, SyntaxKind::R_BRACE);
        return;
    }
    if p.at(SyntaxKind::L_BRACE) {
        p.bump_delim(); // { + trailing trivia
        expr_list_until(p, SyntaxKind::R_BRACE);
        super::expect_closer(p, SyntaxKind::R_BRACE);
        return;
    }

    // Parened body: [:]( expr [, expr]* )
    if p.at(SyntaxKind::COLON) && p.nth(1) == SyntaxKind::L_PAREN {
        p.bump(); // :
        p.bump_delim(); // ( + trailing trivia
        expr_list_until(p, SyntaxKind::R_PAREN);
        super::expect_closer(p, SyntaxKind::R_PAREN);
        return;
    }
    if p.at(SyntaxKind::L_PAREN) {
        p.bump_delim(); // ( + trailing trivia
        expr_list_until(p, SyntaxKind::R_PAREN);
        super::expect_closer(p, SyntaxKind::R_PAREN);
        return;
    }

    // Inline: [colon] expr until next clause stopper
    p.eat(SyntaxKind::COLON);
    // Parse an inline expression. Use hard stoppers only (keywords / `}` / `)`),
    // not IDENT clause starters: names like `state` are valid expression
    // identifiers (`requires: state == Connected`) but also soft clause
    // starters for typestate. Treating them as stoppers left empty bodies.
    if !p.eof() && !is_clause_stopper_kind(p.current()) {
        expressions::expr(p);
    }
}

/// Parse a comma-separated list of expressions until `closer`.
/// Handles tokens that aren't part of the expression grammar (like `@`)
/// by consuming them as raw tokens.
fn expr_list_until(p: &mut Parser, closer: SyntaxKind) {
    while !p.eof_raw() && !p.at(closer) {
        let before = p.pos();
        expressions::expr(p);
        // If expr() made no progress, bump tokens until comma or closer
        if p.pos() == before {
            while !p.eof_raw() && !p.at(closer) && !p.at(SyntaxKind::COMMA) {
                p.bump();
            }
        }
        if !p.at(closer) {
            p.eat(SyntaxKind::COMMA);
            p.bump_trivia();
        }
    }
}

/// Parse a raw clause body (non-expression clauses like effects, input, output).
pub(crate) fn clause_body(p: &mut Parser) {
    // Braced body: [:]{ tokens }
    if p.at(SyntaxKind::COLON) && p.nth(1) == SyntaxKind::L_BRACE {
        p.bump(); // :
        p.bump_delim();
        super::body_tokens_inner(p, SyntaxKind::R_BRACE, &[]);
        super::expect_closer(p, SyntaxKind::R_BRACE);
        return;
    }
    if p.at(SyntaxKind::L_BRACE) {
        p.bump_delim();
        super::body_tokens_inner(p, SyntaxKind::R_BRACE, &[]);
        super::expect_closer(p, SyntaxKind::R_BRACE);
        return;
    }

    // Parened body: [:]( tokens )
    if p.at(SyntaxKind::COLON) && p.nth(1) == SyntaxKind::L_PAREN {
        p.bump(); // :
        p.bump_delim();
        super::body_tokens_inner(p, SyntaxKind::R_PAREN, &[]);
        super::expect_closer(p, SyntaxKind::R_PAREN);
        return;
    }
    if p.at(SyntaxKind::L_PAREN) {
        p.bump_delim();
        super::body_tokens_inner(p, SyntaxKind::R_PAREN, &[]);
        super::expect_closer(p, SyntaxKind::R_PAREN);
        return;
    }

    // Inline raw: colon then tokens until stopper
    if p.at(SyntaxKind::COLON) {
        p.bump(); // :
    }

    // Collect tokens until clause stopper
    while !p.eof() && !is_clause_stopper(p) {
        let cur = p.current_raw();
        match cur {
            SyntaxKind::L_PAREN => {
                p.bump_raw();
                super::body_tokens_inner(p, SyntaxKind::R_PAREN, &[]);
                if p.current_raw() == SyntaxKind::R_PAREN {
                    p.bump_raw();
                }
            }
            SyntaxKind::L_BRACKET => {
                p.bump_raw();
                super::body_tokens_inner(p, SyntaxKind::R_BRACKET, &[]);
                if p.current_raw() == SyntaxKind::R_BRACKET {
                    p.bump_raw();
                }
            }
            SyntaxKind::L_BRACE
            | SyntaxKind::R_BRACE
            | SyntaxKind::R_PAREN
            | SyntaxKind::R_BRACKET => {
                break;
            }
            _ => {
                p.bump_raw();
            }
        }
    }
}
