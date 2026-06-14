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
    ) || is_ident_clause_start(p)
}

/// Ident-based clause starters that aren't keyword tokens.
fn is_ident_clause_start(p: &mut Parser) -> bool {
    if p.current() != SyntaxKind::IDENT {
        return false;
    }
    matches!(
        p.current_text(),
        "step"
            | "resume"
            | "assume"
            | "prove"
            | "validate"
            | "taint"
            | "verify"
            | "example"
            | "strategy"
            | "promise"
            | "bound"
            | "writes"
            | "method"
            | "implements"
    )
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
            // Generic block keywords
            | SyntaxKind::TABLE_KW
            | SyntaxKind::FEATURE_KW
    )
}

/// True if the current token is a clause stopper (including ident-based ones).
pub(crate) fn is_clause_stopper(p: &mut Parser) -> bool {
    if is_clause_stopper_kind(p.current()) {
        return true;
    }
    if p.current() == SyntaxKind::IDENT {
        return matches!(
            p.current_text(),
            "step"
                | "resume"
                | "assume"
                | "prove"
                | "validate"
                | "taint"
                | "verify"
                | "example"
                | "strategy"
                | "promise"
                | "bound"
                | "writes"
                | "operation"
                | "query"
                | "states"
                | "method"
                | "implements"
                | "feature_max"
                | "incremental"
                | "liveness"
                | "safety"
                | "security"
        );
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
    )
}

/// Parse a clause: keyword + body
pub(crate) fn clause(p: &mut Parser) {
    let m = p.open();

    let is_expr = is_expr_clause_kind(p.current());

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
