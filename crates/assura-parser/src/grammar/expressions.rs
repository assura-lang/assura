//! Expression parser using Pratt (precedence-climbing) parsing.
//!
//! Supports all 22 Expr variants from ast.rs, 11 precedence levels,
//! postfix operators, and special forms (quantifiers, let, match, if).

use crate::cst::{CompletedMarker, Parser};
use crate::syntax_kind::SyntaxKind;

/// Parse an expression at the top level (lowest precedence).
pub(crate) fn expr(p: &mut Parser) {
    // `let` is also an atom so it can appear as the RHS of `==` / other infix.
    expr_bp(p, 0);
}

/// Parse an expression with a minimum binding power.
fn expr_bp(p: &mut Parser, min_bp: u8) {
    let Some(mut lhs) = atom(p) else {
        return;
    };

    // Postfix: .field, .method(args), [index], as Type
    loop {
        lhs = match p.current() {
            SyntaxKind::DOT => postfix_dot(p, lhs),
            SyntaxKind::L_BRACKET => postfix_index(p, lhs),
            SyntaxKind::AS_KW => postfix_cast(p, lhs),
            _ => break,
        };
    }

    // Infix binary operators with precedence climbing.
    // Limit the chain length to prevent stack overflow in downstream
    // recursive AST walkers (display, resolve, type-check, codegen)
    // which recurse on the left-leaning Expr::BinOp tree.
    const MAX_BINOP_CHAIN: usize = 128;
    let mut chain_count: usize = 0;
    while let Some((op_bp, _)) = infix_bp(p) {
        if op_bp < min_bp {
            break;
        }

        chain_count += 1;
        if chain_count > MAX_BINOP_CHAIN {
            p.error_at_current("operator chain too long (limit: 128)".to_string());
            break;
        }

        let m = lhs.precede(p);
        // Consume the operator token(s)
        bump_infix_op(p);
        // Parse right-hand side with higher binding power
        expr_bp(p, op_bp + 1);
        lhs = m.complete(p, SyntaxKind::BIN_EXPR);
    }
}

/// Parse an atom (primary expression).
fn atom(p: &mut Parser) -> Option<CompletedMarker> {
    match p.current() {
        // Literals
        SyntaxKind::INT_LIT | SyntaxKind::FLOAT_LIT | SyntaxKind::STRING_LIT => Some(literal(p)),
        SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW => Some(literal(p)),

        // self
        SyntaxKind::SELF_KW => {
            let m = p.open();
            p.bump();
            Some(m.complete(p, SyntaxKind::SELF_EXPR))
        }

        // result
        SyntaxKind::RESULT_KW => {
            let m = p.open();
            p.bump();
            Some(m.complete(p, SyntaxKind::RESULT_EXPR))
        }

        // let x = value in body (must be an atom so `result == let …` works)
        SyntaxKind::LET_KW => Some(let_expr(p)),

        // old(expr)
        SyntaxKind::OLD_KW => Some(old_expr(p)),

        // forall var in domain: body
        SyntaxKind::FORALL_KW => Some(quantifier_expr(p, SyntaxKind::FORALL_EXPR)),

        // exists var in domain: body
        SyntaxKind::EXISTS_KW => Some(quantifier_expr(p, SyntaxKind::EXISTS_EXPR)),

        // if cond then expr [else expr]
        SyntaxKind::IF_KW => Some(if_expr(p)),

        // ghost { expr }
        SyntaxKind::GHOST_KW if p.nth(1) == SyntaxKind::L_BRACE => Some(ghost_expr(p)),

        // apply lemma_name(args)
        SyntaxKind::APPLY_KW => Some(apply_expr(p)),

        // Temporal operators: eventually { expr }, eventually_within { expr }
        SyntaxKind::EVENTUALLY_KW
        | SyntaxKind::EVENTUALLY_ALWAYS_KW
        | SyntaxKind::EVENTUALLY_WITHIN_KW
        | SyntaxKind::LEADS_TO_KW => Some(temporal_expr(p)),

        // match expr { arms }
        SyntaxKind::MATCH_KW => Some(match_expr(p)),

        // Parenthesized or tuple: (expr) or (a, b, c)
        SyntaxKind::L_PAREN => Some(paren_or_tuple(p)),

        // List literal: [a, b, c]
        SyntaxKind::L_BRACKET => Some(list_expr(p)),

        // Unary prefix: not, -, !
        SyntaxKind::NOT_KW | SyntaxKind::MINUS | SyntaxKind::BANG => Some(unary_expr(p)),

        // Keywords that can appear as value-position identifiers
        k if is_keyword_as_value(k) => {
            let m = p.open();
            p.bump();
            Some(m.complete(p, SyntaxKind::IDENT_EXPR))
        }

        // Identifier or function call: name or name(args)
        SyntaxKind::IDENT => Some(ident_or_call(p)),

        // Keywords that are also valid variable/parameter names in expressions.
        // The lexer emits these as keywords, but when they appear in expression
        // context (e.g. `ensures { max_precision >= precision }`), they should
        // be treated as identifiers.
        SyntaxKind::PRECISION_KW => {
            let m = p.open();
            p.bump();
            Some(m.complete(p, SyntaxKind::IDENT_EXPR))
        }

        _ => {
            // Don't consume; let caller handle the error
            None
        }
    }
}

/// Keywords that can appear in value position (e.g., `effects: pure`).
fn is_keyword_as_value(k: SyntaxKind) -> bool {
    matches!(
        k,
        SyntaxKind::PURE_KW
            | SyntaxKind::OPAQUE_KW
            | SyntaxKind::DETERMINISTIC_KW
            | SyntaxKind::ATOMIC_KW
            | SyntaxKind::MONOTONIC_KW
            | SyntaxKind::SECRET_KW
            | SyntaxKind::FROZEN_KW
            | SyntaxKind::PINNED_KW
            | SyntaxKind::RELAXED_KW
            | SyntaxKind::RECOVERY_KW
            | SyntaxKind::CACHE_KW
            | SyntaxKind::SNAPSHOT_KW
            | SyntaxKind::RELEASE_KW
            | SyntaxKind::ACQUIRE_KW
            | SyntaxKind::ACQ_REL_KW
            | SyntaxKind::SEQ_CST_KW
            | SyntaxKind::VIEW_KW
            | SyntaxKind::MERGE_KW
            | SyntaxKind::FAIR_KW
            | SyntaxKind::FENCE_KW
    )
}

// ---- Atom implementations ----

fn literal(p: &mut Parser) -> CompletedMarker {
    let m = p.open();
    p.bump();
    m.complete(p, SyntaxKind::LITERAL_EXPR)
}

fn old_expr(p: &mut Parser) -> CompletedMarker {
    let m = p.open();
    p.bump(); // old
    p.expect(SyntaxKind::L_PAREN);
    p.bump_trivia();
    expr(p);
    p.expect(SyntaxKind::R_PAREN);
    m.complete(p, SyntaxKind::OLD_EXPR)
}

fn quantifier_expr(p: &mut Parser, kind: SyntaxKind) -> CompletedMarker {
    let m = p.open();
    p.bump(); // forall | exists
    p.expect(SyntaxKind::IDENT);
    p.expect(SyntaxKind::IN_KW);
    expr_bp(p, 0);
    p.expect(SyntaxKind::COLON);
    expr_bp(p, 0);
    m.complete(p, kind)
}

fn if_expr(p: &mut Parser) -> CompletedMarker {
    let m = p.open();
    p.bump(); // if
    expr_bp(p, 0);
    p.expect(SyntaxKind::THEN_KW);
    expr_bp(p, 0);
    if p.at(SyntaxKind::ELSE_KW) {
        p.bump();
        expr_bp(p, 0);
    }
    m.complete(p, SyntaxKind::IF_EXPR)
}

fn ghost_expr(p: &mut Parser) -> CompletedMarker {
    let m = p.open();
    p.bump(); // ghost
    p.expect(SyntaxKind::L_BRACE);
    p.bump_trivia();
    expr(p);
    super::expect_closer(p, SyntaxKind::R_BRACE);
    m.complete(p, SyntaxKind::GHOST_EXPR)
}

/// Temporal operator expression: `eventually(expr)` or `eventually { expr }`.
fn temporal_expr(p: &mut Parser) -> CompletedMarker {
    let m = p.open();
    p.bump(); // eventually | eventually_always | eventually_within | leads_to

    // Accept either parenthesized or braced argument
    if p.at(SyntaxKind::L_PAREN) {
        arg_list(p);
    } else if p.at(SyntaxKind::L_BRACE) {
        p.bump_delim(); // { + trivia
        while !p.eof() && !p.at(SyntaxKind::R_BRACE) {
            let before = p.pos();
            expr_bp(p, 0);
            if p.pos() == before {
                p.bump(); // skip unrecognized tokens
            }
            p.eat(SyntaxKind::COMMA);
            p.bump_trivia();
        }
        super::expect_closer(p, SyntaxKind::R_BRACE);
    } else {
        // Inline: eventually expr
        expr_bp(p, 0);
    }
    m.complete(p, SyntaxKind::CALL_EXPR)
}

fn apply_expr(p: &mut Parser) -> CompletedMarker {
    let m = p.open();
    p.bump(); // apply
    p.expect(SyntaxKind::IDENT);
    if p.at(SyntaxKind::L_PAREN) {
        arg_list(p);
    }
    m.complete(p, SyntaxKind::APPLY_EXPR)
}

fn match_expr(p: &mut Parser) -> CompletedMarker {
    let m = p.open();
    p.bump(); // match
    expr_bp(p, 0);
    p.expect(SyntaxKind::L_BRACE);
    p.bump_trivia();

    let arms = p.open();
    // IMPORTANT: after eat(COMMA) (or any separator that does not itself call bump_trivia),
    // you MUST bump_trivia() before the next match_arm/pattern/expr call.
    // Otherwise p.current() will not see the next real token (INT_LIT etc.),
    // pattern() will err_and_sync, the arm will have no PAT child,
    // and lower_match_arm will fall back to Wildcard.
    // This has caused silent missing diagnostics (e.g. A10002) in downstream checkers.
    while !p.eof() && !p.at(SyntaxKind::R_BRACE) {
        match_arm(p);
        p.eat(SyntaxKind::COMMA);
        p.bump_trivia();
    }
    arms.complete(p, SyntaxKind::MATCH_ARM_LIST);

    super::expect_closer(p, SyntaxKind::R_BRACE);
    m.complete(p, SyntaxKind::MATCH_EXPR)
}

fn match_arm(p: &mut Parser) {
    let m = p.open();
    pattern(p);
    p.expect(SyntaxKind::FAT_ARROW);
    expr(p);
    m.complete(p, SyntaxKind::MATCH_ARM);
}

fn pattern(p: &mut Parser) {
    match p.current() {
        SyntaxKind::INT_LIT | SyntaxKind::STRING_LIT => {
            let m = p.open();
            p.bump();
            m.complete(p, SyntaxKind::LITERAL_PAT);
        }
        SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW => {
            let m = p.open();
            p.bump();
            m.complete(p, SyntaxKind::LITERAL_PAT);
        }
        SyntaxKind::L_PAREN => {
            // Tuple pattern: (a, b, c) or single wildcard check
            let m = p.open();
            p.bump(); // (
            while !p.eof() && !p.at(SyntaxKind::R_PAREN) {
                pattern(p);
                if !p.at(SyntaxKind::R_PAREN) {
                    p.eat(SyntaxKind::COMMA);
                    p.bump_trivia();
                }
            }
            p.expect(SyntaxKind::R_PAREN);
            m.complete(p, SyntaxKind::TUPLE_PAT);
        }
        SyntaxKind::IDENT => {
            let text = p.current_text().to_string();
            if text == "_" {
                let m = p.open();
                p.bump();
                m.complete(p, SyntaxKind::WILDCARD_PAT);
            } else if p.nth(1) == SyntaxKind::L_PAREN {
                // Constructor pattern: Name(field1, field2)
                let m = p.open();
                p.bump(); // Name
                p.bump(); // (
                while !p.eof() && !p.at(SyntaxKind::R_PAREN) {
                    pattern(p);
                    if !p.at(SyntaxKind::R_PAREN) {
                        p.eat(SyntaxKind::COMMA);
                        p.bump_trivia();
                    }
                }
                p.expect(SyntaxKind::R_PAREN);
                m.complete(p, SyntaxKind::CONSTRUCTOR_PAT);
            } else {
                let m = p.open();
                p.bump();
                m.complete(p, SyntaxKind::IDENT_PAT);
            }
        }
        _ => {
            p.err_and_sync(
                "expected pattern",
                &[
                    SyntaxKind::FAT_ARROW,
                    SyntaxKind::R_BRACE,
                    SyntaxKind::COMMA,
                ],
            );
        }
    }
}

fn paren_or_tuple(p: &mut Parser) -> CompletedMarker {
    let m = p.open();
    p.bump(); // (
    p.bump_trivia();

    if !p.enter_nesting() {
        // Depth limit exceeded: skip to matching ')' and return error node.
        while !p.eof() && !p.at(SyntaxKind::R_PAREN) {
            p.bump();
        }
        p.eat(SyntaxKind::R_PAREN);
        return m.complete(p, SyntaxKind::PAREN_EXPR);
    }

    if p.at(SyntaxKind::R_PAREN) {
        // Empty parens: ()
        p.bump();
        p.leave_nesting();
        return m.complete(p, SyntaxKind::TUPLE_EXPR);
    }

    expr(p);

    let result = if p.at(SyntaxKind::COMMA) {
        // Tuple: (a, b, c)
        while p.at(SyntaxKind::COMMA) {
            p.bump(); // ,
            if p.at(SyntaxKind::R_PAREN) {
                break; // trailing comma
            }
            expr(p);
        }
        p.expect(SyntaxKind::R_PAREN);
        m.complete(p, SyntaxKind::TUPLE_EXPR)
    } else {
        // Single expr in parens: (expr)
        p.expect(SyntaxKind::R_PAREN);
        m.complete(p, SyntaxKind::PAREN_EXPR)
    };
    p.leave_nesting();
    result
}

fn list_expr(p: &mut Parser) -> CompletedMarker {
    let m = p.open();
    p.bump(); // [
    p.bump_trivia();

    while !p.eof() && !p.at(SyntaxKind::R_BRACKET) {
        let before = p.pos();
        expr(p);
        if !p.at(SyntaxKind::R_BRACKET) {
            p.eat(SyntaxKind::COMMA);
            p.bump_trivia();
        }
        if p.pos() == before {
            p.err_and_sync(
                "expected expression or `]`",
                &[SyntaxKind::R_BRACKET, SyntaxKind::COMMA],
            );
        }
    }
    p.expect(SyntaxKind::R_BRACKET);
    m.complete(p, SyntaxKind::LIST_EXPR)
}

fn unary_expr(p: &mut Parser) -> CompletedMarker {
    let m = p.open();
    p.bump(); // not, -, !
    // Parse the operand at unary precedence (high)
    expr_bp(p, UNARY_BP);
    m.complete(p, SyntaxKind::UNARY_EXPR)
}

fn let_expr(p: &mut Parser) -> CompletedMarker {
    let m = p.open();
    p.bump(); // let
    p.expect(SyntaxKind::IDENT);
    p.expect(SyntaxKind::EQUALS);
    // Value: parse at a precedence that stops before `in`
    // We use range precedence (above comparison) to avoid `in` ambiguity
    expr_bp(p, RANGE_BP);
    p.expect(SyntaxKind::IN_KW);
    expr(p);
    m.complete(p, SyntaxKind::LET_EXPR)
}

fn ident_or_call(p: &mut Parser) -> CompletedMarker {
    if p.nth(1) == SyntaxKind::L_PAREN {
        // function call: name(args)
        let m = p.open();
        p.bump(); // name
        arg_list(p);
        m.complete(p, SyntaxKind::CALL_EXPR)
    } else {
        let m = p.open();
        p.bump(); // name
        m.complete(p, SyntaxKind::IDENT_EXPR)
    }
}

fn arg_list(p: &mut Parser) {
    let m = p.open();
    p.expect(SyntaxKind::L_PAREN);
    while !p.eof() && !p.at(SyntaxKind::R_PAREN) {
        let before = p.pos();
        expr(p);
        if !p.at(SyntaxKind::R_PAREN) {
            p.eat(SyntaxKind::COMMA);
            p.bump_trivia();
        }
        if p.pos() == before {
            p.err_and_sync(
                "expected argument or `)`",
                &[SyntaxKind::R_PAREN, SyntaxKind::COMMA],
            );
        }
    }
    p.expect(SyntaxKind::R_PAREN);
    m.complete(p, SyntaxKind::ARG_LIST);
}

// ---- Postfix ----

fn postfix_dot(p: &mut Parser, lhs: CompletedMarker) -> CompletedMarker {
    let m = lhs.precede(p);
    p.bump(); // .

    // Field/method name: ident, keyword-as-ident, or integer literal for
    // tuple projections (`t.0`, `result.1`) per SPEC § type interactions.
    //
    // Also accept FLOAT_LIT: logos glues `1.0` into a single float token, so
    // nested tuple projections `t.1.0` arrive as `.` + Float("1.0"). Lowering
    // expands that into chained Field nodes.
    if p.at_keyword_or_ident() || p.at(SyntaxKind::INT_LIT) || p.at(SyntaxKind::FLOAT_LIT) {
        p.bump();
    } else {
        p.error_at_current("expected field name after `.`".into());
    }

    if p.at(SyntaxKind::L_PAREN) {
        // method call: expr.method(args) — not valid after a bare int (t.0())
        arg_list(p);
        m.complete(p, SyntaxKind::METHOD_CALL_EXPR)
    } else {
        // field access: expr.field or expr.N (tuple)
        m.complete(p, SyntaxKind::FIELD_EXPR)
    }
}

fn postfix_index(p: &mut Parser, lhs: CompletedMarker) -> CompletedMarker {
    let m = lhs.precede(p);
    p.bump(); // [
    expr(p);
    p.expect(SyntaxKind::R_BRACKET);
    m.complete(p, SyntaxKind::INDEX_EXPR)
}

fn postfix_cast(p: &mut Parser, lhs: CompletedMarker) -> CompletedMarker {
    let m = lhs.precede(p);
    p.bump(); // as
    // Eat the type name (keyword or ident)
    if p.at_keyword_or_ident() {
        p.bump();
    } else {
        p.error_at_current("expected type name after `as`".into());
    }
    m.complete(p, SyntaxKind::CAST_EXPR)
}

// ---- Precedence table ----
// Higher number = tighter binding

const IMPLIES_BP: u8 = 2;
const OR_BP: u8 = 4;
const AND_BP: u8 = 6;
const CMP_BP: u8 = 8;
const RANGE_BP: u8 = 10;
const ADD_BP: u8 = 12;
const MUL_BP: u8 = 14;
const UNARY_BP: u8 = 16;

/// Return the binding power (precedence) for the current token if it's an
/// infix operator. Returns `(left_bp, right_bp)`.
fn infix_bp(p: &mut Parser) -> Option<(u8, u8)> {
    let cur = p.current();
    match cur {
        // => implies (lowest infix)
        SyntaxKind::FAT_ARROW => {
            // Avoid consuming => in match arms
            // Check if we're inside a match by looking if the context suggests it.
            // Simple heuristic: if there's no preceding pattern, it's implies.
            Some((IMPLIES_BP, IMPLIES_BP + 1))
        }

        // or, ||
        SyntaxKind::OR_KW | SyntaxKind::OR_OR => Some((OR_BP, OR_BP + 1)),

        // and, &&
        SyntaxKind::AND_KW | SyntaxKind::AND_AND => Some((AND_BP, AND_BP + 1)),

        // ==, !=, <, <=, >, >=, in, is
        SyntaxKind::EQ
        | SyntaxKind::NEQ
        | SyntaxKind::L_ANGLE
        | SyntaxKind::R_ANGLE
        | SyntaxKind::LTE
        | SyntaxKind::GTE
        | SyntaxKind::IN_KW
        | SyntaxKind::IS_KW => Some((CMP_BP, CMP_BP + 1)),

        // `not in` as a two-token operator
        SyntaxKind::NOT_KW if p.nth(1) == SyntaxKind::IN_KW => Some((CMP_BP, CMP_BP + 1)),

        // ..
        SyntaxKind::DOT_DOT => Some((RANGE_BP, RANGE_BP + 1)),

        // +, -, ++
        SyntaxKind::PLUS | SyntaxKind::CONCAT => Some((ADD_BP, ADD_BP + 1)),
        SyntaxKind::MINUS => {
            // Only treat as infix if there's a preceding expression
            // (the atom() call already handled prefix minus)
            Some((ADD_BP, ADD_BP + 1))
        }

        // *, /, %, mod
        SyntaxKind::STAR | SyntaxKind::SLASH | SyntaxKind::PERCENT => Some((MUL_BP, MUL_BP + 1)),

        // `mod` as infix modulo operator (lexed as IDENT)
        SyntaxKind::IDENT if p.current_text() == "mod" => Some((MUL_BP, MUL_BP + 1)),

        _ => None,
    }
}

/// Consume the infix operator token(s).
fn bump_infix_op(p: &mut Parser) {
    if p.current() == SyntaxKind::NOT_KW && p.nth(1) == SyntaxKind::IN_KW {
        // `not in` is a two-token operator
        p.bump(); // not
        p.bump(); // in
    } else {
        p.bump();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cst::{self, LexedToken, TokenSpan, build_tree};
    use crate::lexer::Token;
    use crate::syntax_kind::SyntaxNode;
    use logos::Logos;

    fn parse_expr_to_tree(source: &str) -> (SyntaxNode, Vec<cst::ParseError>) {
        let lex = Token::lexer(source);
        let mut tokens = Vec::new();
        let mut spans = Vec::new();

        for (tok, span) in lex.spanned() {
            if let Ok(t) = tok {
                tokens.push(LexedToken {
                    kind: SyntaxKind::from(&t),
                    text: source[span.clone()].to_string(),
                });
                spans.push(TokenSpan {
                    start: span.start,
                    end: span.end,
                });
            }
        }

        let mut parser = cst::Parser::new(tokens, spans);
        // Wrap in a root node
        let m = parser.open();
        expr(&mut parser);
        m.complete(&mut parser, SyntaxKind::SOURCE_FILE);
        let green = build_tree(parser.events, &parser.tokens);
        let node = SyntaxNode::new_root(green);
        (node, parser.errors)
    }

    fn first_child_kind(node: &SyntaxNode) -> SyntaxKind {
        node.children()
            .next()
            .map(|c| c.kind())
            .unwrap_or(SyntaxKind::ERROR)
    }

    #[test]
    fn parse_binary_add() {
        let (root, errors) = parse_expr_to_tree("a + b");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::BIN_EXPR);
    }

    #[test]
    fn parse_precedence_mul_over_add() {
        let (root, errors) = parse_expr_to_tree("a + b * c");
        assert!(errors.is_empty(), "errors: {errors:?}");
        // Root should be BIN_EXPR(+) with RHS being BIN_EXPR(*)
        let bin = root.children().next().unwrap();
        assert_eq!(bin.kind(), SyntaxKind::BIN_EXPR);
    }

    #[test]
    fn parse_field_access() {
        let (root, errors) = parse_expr_to_tree("x.y.z");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::FIELD_EXPR);
    }

    #[test]
    fn parse_tuple_field_access() {
        let (root, errors) = parse_expr_to_tree("t.0");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::FIELD_EXPR);
    }

    #[test]
    fn parse_chained_tuple_field_access() {
        let (root, errors) = parse_expr_to_tree("result.0");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::FIELD_EXPR);
    }

    #[test]
    fn parse_nested_tuple_field_chain() {
        // logos glues `1.0` into FLOAT_LIT; postfix must still accept it.
        let (root, errors) = parse_expr_to_tree("t.1.0");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::FIELD_EXPR);
    }

    #[test]
    fn parse_function_call() {
        let (root, errors) = parse_expr_to_tree("f(x, y)");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::CALL_EXPR);
    }

    #[test]
    fn parse_method_call() {
        let (root, errors) = parse_expr_to_tree("x.len()");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::METHOD_CALL_EXPR);
    }

    #[test]
    fn parse_match() {
        let (root, errors) = parse_expr_to_tree("match x { A => 1, B => 2 }");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::MATCH_EXPR);
    }

    #[test]
    fn parse_forall() {
        let (root, errors) = parse_expr_to_tree("forall i in range: i > 0");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::FORALL_EXPR);
    }

    #[test]
    fn parse_if_then_else() {
        let (root, errors) = parse_expr_to_tree("if x then 1 else 2");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::IF_EXPR);
    }

    #[test]
    fn parse_old_expr() {
        let (root, errors) = parse_expr_to_tree("old(x)");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::OLD_EXPR);
    }

    #[test]
    fn parse_unary_not() {
        let (root, errors) = parse_expr_to_tree("not x");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::UNARY_EXPR);
    }

    #[test]
    fn parse_list_literal() {
        let (root, errors) = parse_expr_to_tree("[1, 2, 3]");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::LIST_EXPR);
    }

    #[test]
    fn parse_index_access() {
        let (root, errors) = parse_expr_to_tree("a[0]");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::INDEX_EXPR);
    }

    #[test]
    fn parse_cast() {
        let (root, errors) = parse_expr_to_tree("x as Int");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::CAST_EXPR);
    }

    #[test]
    fn parse_paren_expr() {
        let (root, errors) = parse_expr_to_tree("(a + b)");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::PAREN_EXPR);
    }

    #[test]
    fn parse_tuple_expr() {
        let (root, errors) = parse_expr_to_tree("(a, b, c)");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::TUPLE_EXPR);
    }

    #[test]
    fn parse_constructor_pattern() {
        let (root, errors) = parse_expr_to_tree("match r { Ok(v) => v, Err(e) => 0 }");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::MATCH_EXPR);
    }

    // --- Edge case expression tests ---

    #[test]
    fn parse_deeply_nested_arithmetic() {
        let (root, errors) = parse_expr_to_tree("((a + b) * (c - d)) / ((e + f) % g)");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::BIN_EXPR);
    }

    #[test]
    fn parse_chained_field_access() {
        let (root, errors) = parse_expr_to_tree("a.b.c.d.e");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::FIELD_EXPR);
    }

    #[test]
    fn parse_chained_method_calls() {
        let (root, errors) = parse_expr_to_tree("x.foo().bar(1, 2).baz()");
        assert!(errors.is_empty(), "errors: {errors:?}");
        // Outermost should be METHOD_CALL_EXPR (.baz())
        assert_eq!(first_child_kind(&root), SyntaxKind::METHOD_CALL_EXPR);
    }

    #[test]
    fn parse_nested_function_calls() {
        let (root, errors) = parse_expr_to_tree("f(g(h(x)))");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::CALL_EXPR);
    }

    #[test]
    fn parse_mixed_operators_all_levels() {
        // Uses all 6 binary precedence levels
        let (root, errors) = parse_expr_to_tree("a || b && c == d < e + f * g");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::BIN_EXPR);
    }

    #[test]
    fn parse_mod_operator() {
        let (root, errors) = parse_expr_to_tree("a mod b");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::BIN_EXPR);
    }

    #[test]
    fn parse_unary_chain() {
        let (root, errors) = parse_expr_to_tree("not not x");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::UNARY_EXPR);
    }

    #[test]
    fn parse_unary_minus() {
        let (root, errors) = parse_expr_to_tree("-x + y");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::BIN_EXPR);
    }

    #[test]
    fn parse_comparison_chain() {
        let (root, errors) = parse_expr_to_tree("a >= b && c <= d && e != f");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::BIN_EXPR);
    }

    #[test]
    fn parse_nested_quantifiers() {
        let (root, errors) = parse_expr_to_tree("forall i in xs: exists j in ys: i == j");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::FORALL_EXPR);
    }

    #[test]
    fn parse_nested_if_then_else() {
        let (root, errors) = parse_expr_to_tree("if a then if b then 1 else 2 else 3");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::IF_EXPR);
    }

    #[test]
    fn parse_if_with_complex_condition() {
        let (root, errors) = parse_expr_to_tree("if x > 0 && y < 10 then x + y else 0");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::IF_EXPR);
    }

    #[test]
    fn parse_index_chain() {
        let (root, errors) = parse_expr_to_tree("a[0][1][2]");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::INDEX_EXPR);
    }

    #[test]
    fn parse_index_with_expr() {
        let (root, errors) = parse_expr_to_tree("buf[i + 1]");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::INDEX_EXPR);
    }

    #[test]
    fn parse_old_in_binary() {
        let (root, errors) = parse_expr_to_tree("old(x) + 1");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::BIN_EXPR);
    }

    #[test]
    fn parse_result_in_comparison() {
        let (root, errors) = parse_expr_to_tree("result >= 0");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::BIN_EXPR);
    }

    #[test]
    fn parse_cast_in_expr() {
        let (root, errors) = parse_expr_to_tree("(x as Nat) + 1");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::BIN_EXPR);
    }

    #[test]
    fn parse_nested_list_literal() {
        let (root, errors) = parse_expr_to_tree("[1, [2, 3], [4]]");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::LIST_EXPR);
    }

    #[test]
    fn parse_empty_list() {
        let (root, errors) = parse_expr_to_tree("[]");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::LIST_EXPR);
    }

    #[test]
    fn parse_empty_tuple() {
        let (root, errors) = parse_expr_to_tree("()");
        assert!(errors.is_empty(), "errors: {errors:?}");
        // () is a paren or tuple -- either is acceptable
        let k = first_child_kind(&root);
        assert!(
            k == SyntaxKind::PAREN_EXPR || k == SyntaxKind::TUPLE_EXPR,
            "expected PAREN_EXPR or TUPLE_EXPR, got {k:?}"
        );
    }

    #[test]
    fn parse_string_literal() {
        let (root, errors) = parse_expr_to_tree("\"hello world\"");
        assert!(errors.is_empty(), "errors: {errors:?}");
        // String literals are atoms, should produce a token node
        assert!(!root.children_with_tokens().collect::<Vec<_>>().is_empty());
    }

    #[test]
    fn parse_boolean_literals() {
        let (root, errors) = parse_expr_to_tree("true && false");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::BIN_EXPR);
    }

    #[test]
    fn parse_numeric_literal_float() {
        let (root, errors) = parse_expr_to_tree("3.14 + 2.71");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::BIN_EXPR);
    }

    #[test]
    fn parse_let_as_equality_rhs() {
        // `let` must be an atom so it can appear on the RHS of `==`.
        let (root, errors) = parse_expr_to_tree("result == let y = x + 1 in y * 2");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::BIN_EXPR);
        // Right child of == should be LET_EXPR (not empty recovery).
        let bin = root.children().find(|n| n.kind() == SyntaxKind::BIN_EXPR);
        assert!(bin.is_some(), "expected BIN_EXPR root child");
        let has_let = root.descendants().any(|n| n.kind() == SyntaxKind::LET_EXPR);
        assert!(has_let, "expected LET_EXPR under equality RHS");
    }

    #[test]
    fn parse_let_expr() {
        let (root, errors) = parse_expr_to_tree("let x = 42 in x + 1");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::LET_EXPR);
    }

    #[test]
    fn parse_exists_quantifier() {
        let (root, errors) = parse_expr_to_tree("exists i in xs: i > 0");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::EXISTS_EXPR);
    }

    #[test]
    fn parse_match_multiple_arms() {
        let (root, errors) =
            parse_expr_to_tree("match state { Init => 0, Running => 1, Stopped => 2, Error => 3 }");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::MATCH_EXPR);
    }

    #[test]
    fn parse_complex_field_call_chain() {
        let (root, errors) = parse_expr_to_tree("buf.data[i].len()");
        assert!(errors.is_empty(), "errors: {errors:?}");
        // Should be method call on an index on a field
        let k = first_child_kind(&root);
        assert!(
            k == SyntaxKind::METHOD_CALL_EXPR || k == SyntaxKind::CALL_EXPR,
            "expected method/call, got {k:?}"
        );
    }

    #[test]
    fn parse_implies_operator() {
        let (root, errors) = parse_expr_to_tree("a ==> b");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(first_child_kind(&root), SyntaxKind::BIN_EXPR);
    }
}
