//! Parameter, type parameter, field, and return type parsing.

use crate::cst::Parser;
use crate::syntax_kind::SyntaxKind;

/// Parse optional type parameters: `<T, U: Trait>`
pub(crate) fn type_params(p: &mut Parser) {
    if !p.at(SyntaxKind::L_ANGLE) {
        return;
    }
    let m = p.open();
    p.bump(); // <

    while !p.eof() && !p.at(SyntaxKind::R_ANGLE) {
        let before = p.pos();
        // name [: Bound]
        p.expect(SyntaxKind::IDENT);
        if p.at(SyntaxKind::COLON) {
            p.bump(); // :
            // Consume bound tokens until comma or >
            while !p.eof() && !matches!(p.current(), SyntaxKind::COMMA | SyntaxKind::R_ANGLE) {
                p.bump();
            }
        }
        if !p.at(SyntaxKind::R_ANGLE) {
            p.expect(SyntaxKind::COMMA);
        }
        if p.pos() == before {
            p.err_and_bump("expected type parameter or `>`");
        }
    }
    p.expect(SyntaxKind::R_ANGLE);
    m.complete(p, SyntaxKind::TYPE_PARAM_LIST);
}

/// Parse a parameter list: `(name: Type, name: Type)`
pub(crate) fn param_list(p: &mut Parser) {
    if !p.at(SyntaxKind::L_PAREN) {
        return;
    }
    let m = p.open();
    p.bump(); // (

    while !p.eof() && !p.at(SyntaxKind::R_PAREN) {
        let before = p.pos();
        param(p);
        if !p.at(SyntaxKind::R_PAREN) {
            p.eat(SyntaxKind::COMMA);
        }
        if p.pos() == before {
            p.err_and_bump("expected parameter or `)`");
        }
    }
    p.expect(SyntaxKind::R_PAREN);
    m.complete(p, SyntaxKind::PARAM_LIST);
}

/// Parse a single parameter: `[#[attr]] name: Type`
fn param(p: &mut Parser) {
    let m = p.open();

    // Skip #[...] attributes
    while p.at(SyntaxKind::HASH) {
        p.bump(); // #
        if p.at(SyntaxKind::L_BRACKET) {
            p.bump(); // [
            super::body_tokens_inner(p, &[]);
            p.eat(SyntaxKind::R_BRACKET);
        }
    }

    // name (keyword or ident)
    if p.at_keyword_or_ident() {
        p.bump();
    } else {
        p.error_at_current("expected parameter name".into());
    }

    p.expect(SyntaxKind::COLON);

    // Type tokens with balanced delimiters
    param_type_tokens(p);

    m.complete(p, SyntaxKind::PARAM);
}

/// Collect type tokens for a parameter, handling balanced delimiters.
fn param_type_tokens(p: &mut Parser) {
    while !p.eof() {
        let cur = p.current();
        if matches!(
            cur,
            SyntaxKind::COMMA | SyntaxKind::R_PAREN | SyntaxKind::R_BRACE | SyntaxKind::R_BRACKET
        ) {
            break;
        }
        match cur {
            SyntaxKind::L_BRACE => {
                p.bump();
                balanced_inner(p);
                p.eat(SyntaxKind::R_BRACE);
            }
            SyntaxKind::L_PAREN => {
                p.bump();
                balanced_inner(p);
                p.eat(SyntaxKind::R_PAREN);
            }
            SyntaxKind::L_ANGLE => {
                p.bump();
                balanced_inner_angle(p);
                p.eat(SyntaxKind::R_ANGLE);
            }
            SyntaxKind::L_BRACKET => {
                p.bump();
                balanced_inner(p);
                p.eat(SyntaxKind::R_BRACKET);
            }
            _ => {
                p.bump();
            }
        }
    }
}

/// Consume tokens until matching closer (brace/paren/bracket).
fn balanced_inner(p: &mut Parser) {
    while !p.eof() {
        let cur = p.current();
        match cur {
            SyntaxKind::R_BRACE | SyntaxKind::R_PAREN | SyntaxKind::R_BRACKET => break,
            SyntaxKind::L_BRACE => {
                p.bump();
                balanced_inner(p);
                p.eat(SyntaxKind::R_BRACE);
            }
            SyntaxKind::L_PAREN => {
                p.bump();
                balanced_inner(p);
                p.eat(SyntaxKind::R_PAREN);
            }
            SyntaxKind::L_BRACKET => {
                p.bump();
                balanced_inner(p);
                p.eat(SyntaxKind::R_BRACKET);
            }
            _ => {
                p.bump();
            }
        }
    }
}

/// Balanced inner for angle brackets (also stops at R_ANGLE).
fn balanced_inner_angle(p: &mut Parser) {
    while !p.eof() {
        let cur = p.current();
        match cur {
            SyntaxKind::R_ANGLE
            | SyntaxKind::R_BRACE
            | SyntaxKind::R_PAREN
            | SyntaxKind::R_BRACKET => break,
            SyntaxKind::L_ANGLE => {
                p.bump();
                balanced_inner_angle(p);
                p.eat(SyntaxKind::R_ANGLE);
            }
            SyntaxKind::L_BRACE => {
                p.bump();
                balanced_inner(p);
                p.eat(SyntaxKind::R_BRACE);
            }
            SyntaxKind::L_PAREN => {
                p.bump();
                balanced_inner(p);
                p.eat(SyntaxKind::R_PAREN);
            }
            _ => {
                p.bump();
            }
        }
    }
}

/// Optional return type: `-> Type` or `: Type` (for axiom-style)
pub(crate) fn opt_return_type(p: &mut Parser) {
    if p.at(SyntaxKind::ARROW) {
        fn_return_type(p);
    }
}

/// Parse a function return type: `-> Type` or `: Type`
pub(crate) fn fn_return_type(p: &mut Parser) {
    let m = p.open();
    p.bump(); // -> or :

    // First element can be a refinement type `{...}`
    if p.at(SyntaxKind::L_BRACE) {
        p.bump(); // {
        super::body_tokens_inner(p, &[]);
        p.eat(SyntaxKind::R_BRACE);
    }

    // Remaining return type tokens
    while !p.eof() {
        let cur = p.current();
        if is_return_type_stopper(cur, p) {
            break;
        }
        p.bump();
    }
    m.complete(p, SyntaxKind::RETURN_TYPE);
}

/// Tokens that stop return type collection.
///
/// Any token that starts a clause (keyword or ident-based) must also stop
/// return type collection, otherwise the clause tokens get consumed as
/// return type tokens and are never parsed as clauses.
fn is_return_type_stopper(k: SyntaxKind, p: &Parser) -> bool {
    if matches!(
        k,
        SyntaxKind::L_BRACE
            | SyntaxKind::R_BRACE
            | SyntaxKind::REQUIRES_KW
            | SyntaxKind::ENSURES_KW
            | SyntaxKind::EFFECTS_KW
            | SyntaxKind::MODIFIES_KW
            | SyntaxKind::EQUALS
            | SyntaxKind::INVARIANT_KW
            | SyntaxKind::INPUT_KW
            | SyntaxKind::OUTPUT_KW
            | SyntaxKind::RULE_KW
            | SyntaxKind::DATA_FLOW_KW
            | SyntaxKind::MUST_NOT_KW
            | SyntaxKind::MUST_BE_KW
            | SyntaxKind::BOUNDS_KW
            | SyntaxKind::SEMICOLON
            | SyntaxKind::CONTRACT_KW
            | SyntaxKind::TYPE_KW
            | SyntaxKind::ENUM_KW
            | SyntaxKind::EXTERN_KW
            | SyntaxKind::FN_KW
            | SyntaxKind::SERVICE_KW
            | SyntaxKind::AXIOM_KW
            | SyntaxKind::LEMMA_KW
            | SyntaxKind::DECREASES_KW
    ) {
        return true;
    }
    // Clause stopper kinds (includes CONSTANT_TIME_KW, SPEC_KW, etc.)
    if super::clauses::is_clause_stopper_kind(k) {
        return true;
    }
    // Domain keyword clause starters (SyntaxKind-based)
    if super::clauses::is_domain_keyword_clause_kind(k) {
        return true;
    }
    // Ident-based clause starters
    if k == SyntaxKind::IDENT {
        let text = p.tokens.get(p.pos()).map(|t| t.text.as_str()).unwrap_or("");
        return super::clauses::is_ident_clause_text(text);
    }
    false
}

/// Parse a field definition: `[pub] [ghost] [var] name: Type [;|,]`
pub(crate) fn field_def(p: &mut Parser) {
    let m = p.open();

    // Optional pub
    p.eat(SyntaxKind::PUB_KW);

    // Optional modifiers: ghost, pure, opaque
    while matches!(
        p.current(),
        SyntaxKind::GHOST_KW | SyntaxKind::PURE_KW | SyntaxKind::OPAQUE_KW
    ) {
        p.bump();
    }

    // Optional `var`
    if p.current() == SyntaxKind::IDENT && p.current_text() == "var" {
        p.bump();
    }

    // Field name
    if p.at_keyword_or_ident() {
        p.bump();
    } else {
        p.error_at_current("expected field name".into());
    }

    p.expect(SyntaxKind::COLON);

    // Field type with balanced delimiters
    field_type_tokens(p);

    // Optional terminator
    p.eat(SyntaxKind::SEMICOLON);
    p.eat(SyntaxKind::COMMA);

    m.complete(p, SyntaxKind::FIELD_DEF);
}

/// Collect field type tokens until semicolon, comma, or unbalanced closer.
fn field_type_tokens(p: &mut Parser) {
    while !p.eof() {
        let cur = p.current();
        if matches!(
            cur,
            SyntaxKind::SEMICOLON | SyntaxKind::COMMA | SyntaxKind::R_BRACE
        ) {
            break;
        }
        match cur {
            SyntaxKind::L_BRACE => {
                p.bump();
                balanced_inner(p);
                p.eat(SyntaxKind::R_BRACE);
            }
            SyntaxKind::L_PAREN => {
                p.bump();
                balanced_inner(p);
                p.eat(SyntaxKind::R_PAREN);
            }
            SyntaxKind::L_BRACKET => {
                p.bump();
                balanced_inner(p);
                p.eat(SyntaxKind::R_BRACKET);
            }
            SyntaxKind::R_PAREN | SyntaxKind::R_BRACKET => break,
            _ => {
                p.bump();
            }
        }
    }
}
