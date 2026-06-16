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
    ) || is_domain_keyword_clause(p)
        || is_ident_clause_start(p)
}

/// Ident-based keywords that START a new clause (used by `at_clause_start()`).
/// These are a subset of the stopper keywords: everything that starts a clause
/// also stops the previous one, but not vice versa.
const IDENT_CLAUSE_STARTERS: &[&str] = &[
    "step",
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
    "must_check",
    "must_preserve_detail",
    "update",
    "shared",
    "concurrent",
    "access_mode",
    "bit_layout",
    "bit_level",
    "bit_field",
    "strict_triggers",
];

/// Ident-based keywords that STOP a clause body but do NOT start one.
/// These are declaration-like keywords (feature_max, incremental, etc.)
/// that terminate the current clause but are not themselves clause heads.
const IDENT_CLAUSE_STOPPERS_ONLY: &[&str] = &[
    "feature_max",
    "incremental",
    "liveness",
    "safety",
    "security",
];

/// Check if a SyntaxKind is a domain keyword clause starter (without needing Parser).
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
            | SyntaxKind::INCREMENTAL_KW
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

/// Ident-based clause starters that aren't keyword tokens.
/// Domain-specific keyword tokens that can start a clause inside contract/fn bodies.
/// These have dedicated SyntaxKind variants (not plain IDENT) but are used as clause
/// keywords by the type checker's wiring functions.
fn is_domain_keyword_clause(p: &mut Parser) -> bool {
    matches!(
        p.current(),
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
            | SyntaxKind::INCREMENTAL_KW
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
    )
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
            | SyntaxKind::TRANSITION_KW
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
        p.bump(); // {
        expr_list_until(p, SyntaxKind::R_BRACE);
        p.expect(SyntaxKind::R_BRACE);
        return;
    }
    if p.at(SyntaxKind::L_BRACE) {
        p.bump(); // {
        expr_list_until(p, SyntaxKind::R_BRACE);
        p.expect(SyntaxKind::R_BRACE);
        return;
    }

    // Parened body: [:]( expr [, expr]* )
    if p.at(SyntaxKind::COLON) && p.nth(1) == SyntaxKind::L_PAREN {
        p.bump(); // :
        p.bump(); // (
        expr_list_until(p, SyntaxKind::R_PAREN);
        p.expect(SyntaxKind::R_PAREN);
        return;
    }
    if p.at(SyntaxKind::L_PAREN) {
        p.bump(); // (
        expr_list_until(p, SyntaxKind::R_PAREN);
        p.expect(SyntaxKind::R_PAREN);
        return;
    }

    // Inline: [colon] expr until next clause stopper
    p.eat(SyntaxKind::COLON);
    // Parse an inline expression
    if !p.eof() && !is_clause_stopper(p) {
        expressions::expr(p);
    }
}

/// Parse a comma-separated list of expressions until `closer`.
/// Handles tokens that aren't part of the expression grammar (like `@`)
/// by consuming them as raw tokens.
fn expr_list_until(p: &mut Parser, closer: SyntaxKind) {
    while !p.eof() && !p.at(closer) {
        let before = p.pos();
        expressions::expr(p);
        // If expr() made no progress, bump tokens until comma or closer
        if p.pos() == before {
            while !p.eof() && !p.at(closer) && !p.at(SyntaxKind::COMMA) {
                p.bump();
            }
        }
        if !p.at(closer) {
            p.eat(SyntaxKind::COMMA);
        }
    }
}

/// Parse a raw clause body (non-expression clauses like effects, input, output).
pub(crate) fn clause_body(p: &mut Parser) {
    // Braced body: [:]{ tokens }
    if p.at(SyntaxKind::COLON) && p.nth(1) == SyntaxKind::L_BRACE {
        p.bump(); // :
        p.bump(); // {
        super::body_tokens_inner(p, &[]);
        p.expect(SyntaxKind::R_BRACE);
        return;
    }
    if p.at(SyntaxKind::L_BRACE) {
        p.bump(); // {
        super::body_tokens_inner(p, &[]);
        p.expect(SyntaxKind::R_BRACE);
        return;
    }

    // Parened body: [:]( tokens )
    if p.at(SyntaxKind::COLON) && p.nth(1) == SyntaxKind::L_PAREN {
        p.bump(); // :
        p.bump(); // (
        super::body_tokens_inner(p, &[]);
        p.expect(SyntaxKind::R_PAREN);
        return;
    }
    if p.at(SyntaxKind::L_PAREN) {
        p.bump(); // (
        super::body_tokens_inner(p, &[]);
        p.expect(SyntaxKind::R_PAREN);
        return;
    }

    // Inline raw: colon then tokens until stopper
    if p.at(SyntaxKind::COLON) {
        p.bump(); // :
    }

    // Collect tokens until clause stopper
    while !p.eof() && !is_clause_stopper(p) {
        let cur = p.current();
        match cur {
            SyntaxKind::L_PAREN => {
                p.bump();
                super::body_tokens_inner(p, &[]);
                p.eat(SyntaxKind::R_PAREN);
            }
            SyntaxKind::L_BRACKET => {
                p.bump();
                super::body_tokens_inner(p, &[]);
                p.eat(SyntaxKind::R_BRACKET);
            }
            SyntaxKind::R_BRACE | SyntaxKind::R_PAREN | SyntaxKind::R_BRACKET => break,
            _ => {
                p.bump();
            }
        }
    }
}
