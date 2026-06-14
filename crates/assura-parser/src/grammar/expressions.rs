//! Expression parser using Pratt (precedence-climbing) parsing.
//!
//! Supports all 22 Expr variants from ast.rs, 11 precedence levels,
//! postfix operators, and special forms (quantifiers, let, match, if).

use crate::cst::{CompletedMarker, Parser};
use crate::syntax_kind::SyntaxKind;



/// Parse an expression at the top level (lowest precedence).
pub(crate) fn expr(p: &mut Parser) {
    // let expr has lowest precedence
    if p.at(SyntaxKind::LET_KW) {
        let_expr(p);
        return;
    }
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

    // Infix binary operators with precedence climbing
    while let Some((op_bp, _)) = infix_bp(p) {
        if op_bp < min_bp {
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
        SyntaxKind::INT_LIT | SyntaxKind::FLOAT_LIT | SyntaxKind::STRING_LIT => {
            Some(literal(p))
        }
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

/// Can this token start an atom expression?
/// Used by clause body parsing to know when expression parsing would work.
pub(crate) fn at_expr_start(p: &mut Parser) -> bool {
    matches!(
        p.current(),
        SyntaxKind::INT_LIT
            | SyntaxKind::FLOAT_LIT
            | SyntaxKind::STRING_LIT
            | SyntaxKind::TRUE_KW
            | SyntaxKind::FALSE_KW
            | SyntaxKind::SELF_KW
            | SyntaxKind::RESULT_KW
            | SyntaxKind::OLD_KW
            | SyntaxKind::FORALL_KW
            | SyntaxKind::EXISTS_KW
            | SyntaxKind::IF_KW
            | SyntaxKind::GHOST_KW
            | SyntaxKind::APPLY_KW
            | SyntaxKind::MATCH_KW
            | SyntaxKind::LET_KW
            | SyntaxKind::L_PAREN
            | SyntaxKind::L_BRACKET
            | SyntaxKind::NOT_KW
            | SyntaxKind::MINUS
            | SyntaxKind::BANG
            | SyntaxKind::IDENT
    ) || is_keyword_as_value(p.current())
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
    expr(p);
    p.expect(SyntaxKind::R_BRACE);
    m.complete(p, SyntaxKind::GHOST_EXPR)
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

    let arms = p.open();
    while !p.eof() && !p.at(SyntaxKind::R_BRACE) {
        match_arm(p);
        p.eat(SyntaxKind::COMMA);
    }
    arms.complete(p, SyntaxKind::MATCH_ARM_LIST);

    p.expect(SyntaxKind::R_BRACE);
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
            p.err_and_bump("expected pattern");
        }
    }
}

fn paren_or_tuple(p: &mut Parser) -> CompletedMarker {
    let m = p.open();
    p.bump(); // (

    if p.at(SyntaxKind::R_PAREN) {
        // Empty parens: ()
        p.bump();
        return m.complete(p, SyntaxKind::TUPLE_EXPR);
    }

    expr(p);

    if p.at(SyntaxKind::COMMA) {
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
    }
}

fn list_expr(p: &mut Parser) -> CompletedMarker {
    let m = p.open();
    p.bump(); // [

    while !p.eof() && !p.at(SyntaxKind::R_BRACKET) {
        expr(p);
        if !p.at(SyntaxKind::R_BRACKET) {
            p.eat(SyntaxKind::COMMA);
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

fn let_expr(p: &mut Parser) {
    let m = p.open();
    p.bump(); // let
    p.expect(SyntaxKind::IDENT);
    p.expect(SyntaxKind::EQUALS);
    // Value: parse at a precedence that stops before `in`
    // We use range precedence (above comparison) to avoid `in` ambiguity
    expr_bp(p, RANGE_BP);
    p.expect(SyntaxKind::IN_KW);
    expr(p);
    m.complete(p, SyntaxKind::LET_EXPR);
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
        expr(p);
        if !p.at(SyntaxKind::R_PAREN) {
            p.eat(SyntaxKind::COMMA);
        }
    }
    p.expect(SyntaxKind::R_PAREN);
    m.complete(p, SyntaxKind::ARG_LIST);
}

// ---- Postfix ----

fn postfix_dot(p: &mut Parser, lhs: CompletedMarker) -> CompletedMarker {
    let m = lhs.precede(p);
    p.bump(); // .

    // Must be followed by ident or keyword-as-ident
    if p.at_keyword_or_ident() {
        p.bump();
    } else {
        p.error_at_current("expected field name after `.`".into());
    }

    if p.at(SyntaxKind::L_PAREN) {
        // method call: expr.method(args)
        arg_list(p);
        m.complete(p, SyntaxKind::METHOD_CALL_EXPR)
    } else {
        // field access: expr.field
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

        // *, /, %
        SyntaxKind::STAR | SyntaxKind::SLASH | SyntaxKind::PERCENT => {
            Some((MUL_BP, MUL_BP + 1))
        }

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
    use crate::syntax_kind::{AssuraLanguage, SyntaxNode};
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
}