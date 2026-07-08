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
            p.err_and_sync(
                "expected type parameter or `>`",
                &[SyntaxKind::IDENT, SyntaxKind::R_ANGLE, SyntaxKind::COMMA],
            );
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
    p.bump_trivia();

    while !p.eof() && !p.at(SyntaxKind::R_PAREN) {
        let before = p.pos();
        param(p);
        if !p.at(SyntaxKind::R_PAREN) {
            p.eat(SyntaxKind::COMMA);
        }
        if p.pos() == before {
            p.err_and_sync(
                "expected parameter or `)`",
                &[SyntaxKind::IDENT, SyntaxKind::R_PAREN, SyntaxKind::COMMA],
            );
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
            p.bump_trivia();
            super::body_tokens_inner(p, SyntaxKind::R_BRACKET, &[]);
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
        let cur = p.current_raw();
        if matches!(
            cur,
            SyntaxKind::COMMA | SyntaxKind::R_PAREN | SyntaxKind::R_BRACE | SyntaxKind::R_BRACKET
        ) {
            break;
        }
        match cur {
            SyntaxKind::L_BRACE => {
                p.bump_raw();
                super::body_tokens_inner(
                    p,
                    SyntaxKind::R_BRACE,
                    &[
                        SyntaxKind::COMMA,
                        SyntaxKind::R_PAREN,
                        SyntaxKind::R_BRACE,
                        SyntaxKind::R_BRACKET,
                    ],
                );
                if p.current_raw() == SyntaxKind::R_BRACE {
                    p.bump_raw();
                }
            }
            SyntaxKind::L_PAREN => {
                p.bump_raw();
                super::body_tokens_inner(
                    p,
                    SyntaxKind::R_PAREN,
                    &[
                        SyntaxKind::COMMA,
                        SyntaxKind::R_PAREN,
                        SyntaxKind::R_BRACE,
                        SyntaxKind::R_BRACKET,
                    ],
                );
                if p.current_raw() == SyntaxKind::R_PAREN {
                    p.bump_raw();
                }
            }
            SyntaxKind::L_ANGLE => {
                p.bump_raw();
                balanced_inner_angle(p);
                if p.current_raw() == SyntaxKind::R_ANGLE {
                    p.bump_raw();
                }
            }
            SyntaxKind::L_BRACKET => {
                p.bump_raw();
                super::body_tokens_inner(
                    p,
                    SyntaxKind::R_BRACKET,
                    &[
                        SyntaxKind::COMMA,
                        SyntaxKind::R_PAREN,
                        SyntaxKind::R_BRACE,
                        SyntaxKind::R_BRACKET,
                    ],
                );
                if p.current_raw() == SyntaxKind::R_BRACKET {
                    p.bump_raw();
                }
            }
            _ => {
                p.bump_raw();
            }
        }
    }
}

/// Consume tokens until matching closer (brace/paren/bracket).
fn balanced_inner(p: &mut Parser) {
    while !p.eof() {
        let cur = p.current_raw();
        match cur {
            SyntaxKind::R_BRACE | SyntaxKind::R_PAREN | SyntaxKind::R_BRACKET => break,
            SyntaxKind::L_BRACE => {
                p.bump_raw();
                balanced_inner(p);
                if p.current_raw() == SyntaxKind::R_BRACE {
                    p.bump_raw();
                }
            }
            SyntaxKind::L_PAREN => {
                p.bump_raw();
                balanced_inner(p);
                if p.current_raw() == SyntaxKind::R_PAREN {
                    p.bump_raw();
                }
            }
            SyntaxKind::L_BRACKET => {
                p.bump_raw();
                balanced_inner(p);
                if p.current_raw() == SyntaxKind::R_BRACKET {
                    p.bump_raw();
                }
            }
            _ => {
                p.bump_raw();
            }
        }
    }
}

/// Balanced inner for angle brackets (also stops at R_ANGLE).
fn balanced_inner_angle(p: &mut Parser) {
    while !p.eof() {
        let cur = p.current_raw();
        match cur {
            SyntaxKind::R_ANGLE
            | SyntaxKind::R_BRACE
            | SyntaxKind::R_PAREN
            | SyntaxKind::R_BRACKET => break,
            SyntaxKind::L_ANGLE => {
                p.bump_raw();
                balanced_inner_angle(p);
                if p.current_raw() == SyntaxKind::R_ANGLE {
                    p.bump_raw();
                }
            }
            SyntaxKind::L_BRACE => {
                p.bump_raw();
                balanced_inner(p);
                if p.current_raw() == SyntaxKind::R_BRACE {
                    p.bump_raw();
                }
            }
            SyntaxKind::L_PAREN => {
                p.bump_raw();
                balanced_inner(p);
                if p.current_raw() == SyntaxKind::R_PAREN {
                    p.bump_raw();
                }
            }
            _ => {
                p.bump_raw();
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
        p.bump_delim(); // { + trivia
        super::body_tokens_inner(p, SyntaxKind::R_BRACE, &[]);
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
    // Ident-based clause starters. Use current_text() (skips trivia), not
    // tokens[pos] which may be WHITESPACE while current() is IDENT — that
    // mismatch caused `catch` / other clause idents to be slurped into the
    // return type (#345 error_swallowed A12002 regression).
    if k == SyntaxKind::IDENT {
        let text = p.current_text();
        // "taint" appears in @taint:foo return type annotations (after the type name,
        // e.g. ") -> ValidXlen @taint:validated"). Do not stop the return type slurp
        // on it, otherwise the annotation tokens are left behind and misparsed as a
        // clause start (leading to "expected COLON" + later "expected R_BRACE" on
        // fns with annotated returns, as seen in zlib-inflate etc.).
        if text == "taint" {
            return false;
        }
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

/// Collect field type tokens until a field terminator or the next field.
///
/// Terminators (only at nesting depth 0):
/// - `;` / `,` (explicit separators)
/// - `}` (end of struct body)
/// - next field start: `name: Type` or `pub`/`ghost`/`pure`/`opaque`/`var` …
///
/// Nesting tracks `()`, `[]`, `{}`, and `<>` so `Map<String, Int>` keeps the
/// comma inside angle brackets instead of treating it as a field separator.
/// Without next-field detection, newline-separated fields without commas
/// (`x: Int\n  y: Int`) were slurped into a single field type (`Int y Int`),
/// so only the first field registered and `p.y` failed with A03005.
fn field_type_tokens(p: &mut Parser) {
    let mut angle_depth: i32 = 0;
    let mut paren_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;
    let mut brace_depth: i32 = 0;
    let mut saw_type_token = false;

    while !p.eof() {
        // Skip trivia so lookahead/depth decisions see significant tokens.
        while matches!(
            p.current_raw(),
            SyntaxKind::WHITESPACE | SyntaxKind::COMMENT
        ) {
            p.bump_raw();
        }
        if p.eof() {
            break;
        }

        let cur = p.current();
        let at_top = angle_depth == 0 && paren_depth == 0 && bracket_depth == 0 && brace_depth == 0;

        if at_top {
            if matches!(
                cur,
                SyntaxKind::SEMICOLON | SyntaxKind::COMMA | SyntaxKind::R_BRACE
            ) {
                break;
            }
            // Next field without separator: `y: Int` or `pub y: Int`.
            // Do not treat `@taint:validated` (or other `@name:…` annotations)
            // as a new field — those bind to the preceding type token.
            if saw_type_token && looks_like_field_start(p) {
                break;
            }
            // Clause after fields (requires/ensures/…).
            if saw_type_token && super::clauses::at_clause_start(p) {
                break;
            }
        }

        match cur {
            // Field / type annotations: `@taint:validated`, `@label:…`.
            // Consume `@name` and optional `:value` so `name:` is not seen as
            // a field start on the next loop iteration.
            SyntaxKind::AT => {
                p.bump();
                saw_type_token = true;
                if p.current() == SyntaxKind::IDENT || p.current().is_keyword() {
                    p.bump();
                    if p.current() == SyntaxKind::COLON {
                        p.bump();
                        // annotation value (ident, literal, or keyword)
                        let v = p.current();
                        if !matches!(
                            v,
                            SyntaxKind::SEMICOLON
                                | SyntaxKind::COMMA
                                | SyntaxKind::R_BRACE
                                | SyntaxKind::ERROR_TOKEN
                        ) && !p.eof()
                        {
                            p.bump();
                        }
                    }
                }
            }
            SyntaxKind::L_ANGLE => {
                angle_depth += 1;
                p.bump();
                saw_type_token = true;
            }
            SyntaxKind::R_ANGLE => {
                if angle_depth > 0 {
                    angle_depth -= 1;
                    p.bump();
                    saw_type_token = true;
                } else if at_top {
                    // Stray `>` ends the type.
                    break;
                } else {
                    p.bump();
                }
            }
            SyntaxKind::L_PAREN => {
                paren_depth += 1;
                p.bump();
                saw_type_token = true;
            }
            SyntaxKind::R_PAREN => {
                if paren_depth > 0 {
                    paren_depth -= 1;
                    p.bump();
                    saw_type_token = true;
                } else {
                    break;
                }
            }
            SyntaxKind::L_BRACKET => {
                bracket_depth += 1;
                p.bump();
                saw_type_token = true;
            }
            SyntaxKind::R_BRACKET => {
                if bracket_depth > 0 {
                    bracket_depth -= 1;
                    p.bump();
                    saw_type_token = true;
                } else {
                    break;
                }
            }
            SyntaxKind::L_BRACE => {
                brace_depth += 1;
                p.bump();
                saw_type_token = true;
            }
            SyntaxKind::R_BRACE => {
                if brace_depth > 0 {
                    brace_depth -= 1;
                    p.bump();
                    saw_type_token = true;
                } else {
                    break;
                }
            }
            _ => {
                p.bump();
                saw_type_token = true;
            }
        }
    }
}

/// True when the current token sequence looks like the start of another field.
///
/// Matches `name:`, `pub name:`, `ghost name:`, stacked modifiers, etc.
fn looks_like_field_start(p: &Parser) -> bool {
    let is_name = |k: SyntaxKind| k == SyntaxKind::IDENT || k.is_keyword();
    let mut i = 0usize;
    // Optional leading modifiers (pub/ghost/pure/opaque); "var" is IDENT.
    while matches!(
        p.nth(i),
        SyntaxKind::PUB_KW | SyntaxKind::GHOST_KW | SyntaxKind::PURE_KW | SyntaxKind::OPAQUE_KW
    ) {
        i += 1;
    }
    // name:
    is_name(p.nth(i)) && p.nth(i + 1) == SyntaxKind::COLON
}
