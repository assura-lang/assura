//! Recursive-descent grammar rules for the Assura language.
//!
//! Each function takes a `&mut Parser` and drives it through the
//! corresponding grammar production, emitting markers and events
//! that `build_tree` converts into a rowan green tree.

mod clauses;
mod expressions;
mod items;
mod params;

use crate::cst::Parser;
use crate::syntax_kind::SyntaxKind;

/// Parse a complete source file.
///
/// source_file = project_decl? module_decl? import_decl* decl*
pub(crate) fn source_file(p: &mut Parser) {
    let m = p.open();

    // Optional project declaration
    if p.at(SyntaxKind::PROJECT_KW) {
        project_decl(p);
    }

    // Optional module declaration
    if p.at(SyntaxKind::MODULE_KW) {
        module_decl(p);
    }

    // Import declarations
    while !p.eof() && p.at(SyntaxKind::IMPORT_KW) {
        import_decl(p);
    }

    // Top-level declarations
    while !p.eof() {
        if at_decl_start(p) {
            items::decl(p);
        } else {
            p.err_and_bump("expected declaration");
        }
    }

    // Emit any trailing trivia and remaining under SOURCE to ensure Advance
    // count matches token count (prevents builder children >1 at finish).
    p.bump_trivia();
    while !p.eof_raw() {
        p.bump_raw();
    }
    m.complete(p, SyntaxKind::SOURCE_FILE);
}

/// project name { profile: [p1, p2] }
fn project_decl(p: &mut Parser) {
    let m = p.open();
    p.expect(SyntaxKind::PROJECT_KW);
    p.expect(SyntaxKind::IDENT);
    if p.at(SyntaxKind::L_BRACE) {
        p.bump(); // {
                  // profile: [...]
        if p.at(SyntaxKind::PROFILE_KW) {
            p.bump();
            p.expect(SyntaxKind::COLON);
            if p.at(SyntaxKind::L_BRACKET) {
                let pl = p.open();
                p.bump(); // [
                while !p.eof() && !p.at(SyntaxKind::R_BRACKET) {
                    let before = p.pos();
                    p.expect(SyntaxKind::IDENT);
                    p.eat(SyntaxKind::COMMA);
                    if p.pos() == before {
                        p.err_and_bump("expected profile name or `]`");
                    }
                }
                p.expect(SyntaxKind::R_BRACKET);
                pl.complete(p, SyntaxKind::PROFILE_LIST);
            }
        }
        p.expect(SyntaxKind::R_BRACE);
    }
    m.complete(p, SyntaxKind::PROJECT_DECL);
}

/// module path.to.mod;
fn module_decl(p: &mut Parser) {
    let m = p.open();
    p.expect(SyntaxKind::MODULE_KW);
    dotted_path(p);
    p.eat(SyntaxKind::SEMICOLON);
    m.complete(p, SyntaxKind::MODULE_DECL);
}

/// import path.to.thing [as alias] [{item1, item2}] [;]
fn import_decl(p: &mut Parser) {
    let m = p.open();
    p.expect(SyntaxKind::IMPORT_KW);
    dotted_path(p);

    // Optional alias
    if p.at(SyntaxKind::AS_KW) {
        p.bump();
        p.expect(SyntaxKind::IDENT);
    }

    // Optional item list
    if p.at(SyntaxKind::L_BRACE) {
        let il = p.open();
        p.bump(); // {
        while !p.eof() && !p.at(SyntaxKind::R_BRACE) {
            let before = p.pos();
            p.expect(SyntaxKind::IDENT);
            if !p.at(SyntaxKind::R_BRACE) {
                p.eat(SyntaxKind::COMMA);
            }
            if p.pos() == before {
                p.err_and_bump("expected import name or `}`");
            }
        }
        p.expect(SyntaxKind::R_BRACE);
        il.complete(p, SyntaxKind::IMPORT_ITEM_LIST);
    }

    p.eat(SyntaxKind::SEMICOLON);
    m.complete(p, SyntaxKind::IMPORT_DECL);
}

/// Parse a dotted path: `ident.ident.ident`
fn dotted_path(p: &mut Parser) {
    let m = p.open();
    p.expect(SyntaxKind::IDENT);
    while p.at(SyntaxKind::DOT) {
        p.bump(); // .
        p.expect(SyntaxKind::IDENT);
    }
    m.complete(p, SyntaxKind::DOTTED_PATH);
}

/// True if the current token can start a declaration.
fn at_decl_start(p: &mut Parser) -> bool {
    p.at_any(&[
        SyntaxKind::CONTRACT_KW,
        SyntaxKind::SERVICE_KW,
        SyntaxKind::TYPE_KW,
        SyntaxKind::ENUM_KW,
        SyntaxKind::EXTERN_KW,
        SyntaxKind::FN_KW,
        SyntaxKind::AXIOM_KW,
        SyntaxKind::LEMMA_KW,
        SyntaxKind::PURE_KW,
        SyntaxKind::GHOST_KW,
        SyntaxKind::OPAQUE_KW,
        SyntaxKind::HASH,
        SyntaxKind::SPEC_KW,
    ]) || (p.current() == SyntaxKind::IDENT || p.current().is_keyword()) && !p.eof()
}

/// Collect tokens with balanced delimiters until we hit a stopper or EOF.
/// Uses raw access so that trivia are included (for correct source offsets).
fn body_tokens_inner(p: &mut Parser, stoppers: &[SyntaxKind]) {
    while !p.eof() {
        let cur = p.current_raw();
        if stoppers.contains(&cur) {
            break;
        }
        match cur {
            SyntaxKind::L_BRACE => {
                p.bump_raw();
                body_tokens_inner(p, &[]);
                if p.current_raw() == SyntaxKind::R_BRACE {
                    p.bump_raw();
                }
            }
            SyntaxKind::L_PAREN => {
                p.bump_raw();
                body_tokens_inner(p, &[]);
                if p.current_raw() == SyntaxKind::R_PAREN {
                    p.bump_raw();
                }
            }
            SyntaxKind::L_BRACKET => {
                p.bump_raw();
                body_tokens_inner(p, &[]);
                if p.current_raw() == SyntaxKind::R_BRACKET {
                    p.bump_raw();
                }
            }
            SyntaxKind::R_BRACE | SyntaxKind::R_PAREN | SyntaxKind::R_BRACKET => break,
            _ => {
                p.bump_raw();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cst::{self, build_tree, LexedToken, TokenSpan};
    use crate::lexer::Token;
    use crate::syntax_kind::SyntaxNode;
    use logos::Logos;

    /// Lex source text, create a Parser, run a grammar function, build the tree.
    fn parse_to_tree(source: &str) -> (SyntaxNode, Vec<cst::ParseError>) {
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
        source_file(&mut parser);
        let green = build_tree(parser.events, &parser.tokens);
        let node = SyntaxNode::new_root(green);
        (node, parser.errors)
    }

    /// Helper to get the SyntaxKind of a SyntaxNode.
    fn node_kind(n: &SyntaxNode) -> SyntaxKind {
        n.kind()
    }

    #[test]
    fn parse_empty_source() {
        let (root, errors) = parse_to_tree("");
        assert!(errors.is_empty());
        assert_eq!(node_kind(&root), SyntaxKind::SOURCE_FILE);
    }

    #[test]
    fn parse_project_module_import() {
        let src = r#"
            project MyApp {
                profile: [safety, security]
            }
            module mymod;
            import std.io;
            import std.collections {HashMap, Vec};
        "#;
        let (root, errors) = parse_to_tree(src);
        assert!(errors.is_empty(), "errors: {errors:?}");

        let children: Vec<_> = root.children().collect();
        let kinds: Vec<_> = children.iter().map(|c| node_kind(c)).collect();
        assert!(kinds.contains(&SyntaxKind::PROJECT_DECL));
        assert!(kinds.contains(&SyntaxKind::MODULE_DECL));
        let import_count = kinds
            .iter()
            .filter(|k| **k == SyntaxKind::IMPORT_DECL)
            .count();
        assert_eq!(import_count, 2);
    }

    #[test]
    fn parse_simple_contract() {
        let src = r#"
            contract Foo {
                requires n > 0
                ensures result >= 0
            }
        "#;
        let (root, errors) = parse_to_tree(src);
        assert!(errors.is_empty(), "errors: {errors:?}");

        let contract = root
            .children()
            .find(|c| node_kind(c) == SyntaxKind::CONTRACT_DECL);
        assert!(contract.is_some(), "should have a CONTRACT_DECL");
    }

    #[test]
    fn parse_fn_def() {
        let src = r#"
            fn factorial(n: Nat) -> Nat
                requires n >= 0
                decreases n
                ensures result >= 1
        "#;
        let (root, errors) = parse_to_tree(src);
        assert!(errors.is_empty(), "errors: {errors:?}");

        let fn_node = root.children().find(|c| node_kind(c) == SyntaxKind::FN_DEF);
        assert!(fn_node.is_some(), "should have a FN_DEF");
    }

    #[test]
    fn parse_type_and_enum() {
        let src = r#"
            type Point {
                x: Int;
                y: Int;
            }
            enum Color {
                Red,
                Green,
                Blue,
            }
        "#;
        let (root, errors) = parse_to_tree(src);
        assert!(errors.is_empty(), "errors: {errors:?}");

        let type_node = root
            .children()
            .find(|c| node_kind(c) == SyntaxKind::TYPE_DEF);
        let enum_node = root
            .children()
            .find(|c| node_kind(c) == SyntaxKind::ENUM_DEF);
        assert!(type_node.is_some(), "should have a TYPE_DEF");
        assert!(enum_node.is_some(), "should have an ENUM_DEF");
    }

    #[test]
    fn parse_trailing_trivia_does_not_infinite_loop() {
        // Regression test for #335: trailing trivia (final \n) caused
        // infinite loop in source_file's !eof_raw + bump_raw force loop,
        // because bump_trivia/bump_raw used the trivia-aware eof().
        let src = "contract C { requires { true } }\n";
        let (root, errors) = parse_to_tree(src);
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert!(root
            .children()
            .any(|c| node_kind(&c) == SyntaxKind::CONTRACT_DECL));
    }

    #[test]
    fn table_generic_with_paren_value_and_following_clause_parses() {
        // Reduced from demos/libwebp-huffman.assura.
        // Includes axiom with param list + generic in type (Sequence<Nat>),
        // fn with param, table with = precompute( for ... ) + following
        // verify_against clause. This pattern triggered "expected parameter name"
        // etc after the trivia capture changes for #335.
        let src = r#"
axiom max_secondary_table_entries(
    alphabet_size: Nat,
    root_bits: Nat,
    code_lengths: Sequence<Nat>
) : Nat = {
  define:
    let max_len = max(code_lengths)
    num_prefixes * pow(2, secondary_bits)
  property:
    forall cl in valid_huffman_codes(alphabet_size):
      max_secondary_table_entries(alphabet_size, root_bits, cl) <= 1
}

fn worst_case_table_size(alphabet_size: U16) -> U32
  requires { alphabet_size <= 280 }
{
  primary + max_secondary
}

contract X {
  table K_TABLE_SIZE: [U32; 3] = precompute(
    for i in 0..3 => worst_case_table_size(i as U16)
  )
    verify_against: worst
}
"#;
        let (root, errors) = parse_to_tree(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        assert!(root
            .children()
            .any(|c| node_kind(&c) == SyntaxKind::CONTRACT_DECL));
    }
}
