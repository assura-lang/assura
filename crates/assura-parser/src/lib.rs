/// AST node types produced by lowering the CST.
pub mod ast;
pub(crate) mod cst;
/// Pretty-printing for AST nodes (Debug-like, human-readable).
pub mod display;
/// Central registry of Assura's 50 verification features.
pub mod features;
pub(crate) mod grammar;
/// Token definitions (logos-derived lexer).
pub mod lexer;
/// CST-to-AST lowering: converts the lossless `rowan` tree into typed AST nodes.
pub mod lower;
/// `SyntaxKind` enum (rowan `Language` trait) and `SyntaxNode`/`SyntaxToken` aliases.
pub mod syntax_kind;

/// Re-export parse errors from the CST module for downstream use.
pub use cst::ParseError;

/// Parse source text into a SourceFile, returning the AST and any errors.
///
/// This is the primary public API: lex -> build CST (rowan) -> lower to AST.
///
/// The parser uses error recovery, so the returned `SourceFile` may be
/// partial even when errors are present. `None` is returned only when
/// the tree is too broken to produce any meaningful AST.
pub fn parse(source: &str) -> (Option<ast::SourceFile>, Vec<ParseError>) {
    use logos::Logos;

    let lex = lexer::Token::lexer(source);

    let mut tokens = Vec::new();
    let mut spans = Vec::new();

    for (tok, span) in lex.spanned() {
        if let Ok(t) = tok {
            let kind = syntax_kind::SyntaxKind::from(&t);
            let text = source[span.clone()].to_string();
            tokens.push(cst::LexedToken { kind, text });
            spans.push(cst::TokenSpan {
                start: span.start,
                end: span.end,
            });
        }
        // Lexer errors (unrecognized chars) are silently skipped,
        // same as the previous chumsky-based parser.
    }

    let mut parser = cst::Parser::new(tokens, spans);
    grammar::source_file(&mut parser);
    let (events, toks, errors) = parser.finish();

    let green = cst::build_tree(events, &toks);
    let root = syntax_kind::SyntaxNode::new_root(green);
    let source_file = lower::lower_source_file(&root);

    // If there are errors and the AST has no real declarations (only
    // generic-block remnants from error recovery), treat it as unparseable.
    // This matches the old chumsky behavior of returning None for garbage input.
    if !errors.is_empty()
        && source_file.project.is_none()
        && source_file.module.is_none()
        && source_file.imports.is_empty()
        && source_file
            .decls
            .iter()
            .all(|d| matches!(d.node, ast::Decl::Block { .. }))
    {
        return (None, errors);
    }

    (Some(source_file), errors)
}

/// A lex error: an unrecognized character at the given byte range.
#[derive(Debug, Clone)]
pub struct LexError {
    pub span: std::ops::Range<usize>,
}

impl From<ParseError> for assura_diagnostics::Diagnostic {
    fn from(e: ParseError) -> Self {
        assura_diagnostics::Diagnostic::error(e.code, e.message, e.span)
    }
}

impl LexError {
    /// Convert to a `Diagnostic` using the source text for the error message.
    pub fn to_diagnostic(&self, source: &str) -> assura_diagnostics::Diagnostic {
        assura_diagnostics::Diagnostic::error(
            "A01001",
            format!("unexpected character: {:?}", &source[self.span.clone()]),
            self.span.clone(),
        )
    }
}

/// Full parse result including lex errors and token count.
pub struct ParseResult {
    pub file: Option<ast::SourceFile>,
    pub parse_errors: Vec<ParseError>,
    pub lex_errors: Vec<LexError>,
    pub token_count: usize,
}

/// Parse source text in a single lex pass, returning lex errors, parse
/// errors, token count, and the AST.  This avoids the double-lexing that
/// occurs when calling `Token::lexer()` externally followed by `parse()`.
pub fn parse_full(source: &str) -> ParseResult {
    use logos::Logos;

    let lex = lexer::Token::lexer(source);

    let mut tokens = Vec::new();
    let mut spans = Vec::new();
    let mut lex_errors = Vec::new();

    for (tok, span) in lex.spanned() {
        match tok {
            Ok(t) => {
                let kind = syntax_kind::SyntaxKind::from(&t);
                let text = source[span.clone()].to_string();
                tokens.push(cst::LexedToken { kind, text });
                spans.push(cst::TokenSpan {
                    start: span.start,
                    end: span.end,
                });
            }
            Err(()) => {
                lex_errors.push(LexError { span });
            }
        }
    }

    let token_count = tokens.len();
    let mut parser = cst::Parser::new(tokens, spans);
    grammar::source_file(&mut parser);
    let (events, toks, errors) = parser.finish();

    let green = cst::build_tree(events, &toks);
    let root = syntax_kind::SyntaxNode::new_root(green);
    let source_file = lower::lower_source_file(&root);

    if !errors.is_empty()
        && source_file.project.is_none()
        && source_file.module.is_none()
        && source_file.imports.is_empty()
        && source_file
            .decls
            .iter()
            .all(|d| matches!(d.node, ast::Decl::Block { .. }))
    {
        return ParseResult {
            file: None,
            parse_errors: errors,
            lex_errors,
            token_count,
        };
    }

    ParseResult {
        file: Some(source_file),
        parse_errors: errors,
        lex_errors,
        token_count,
    }
}

/// Parse source text to a lossless CST (concrete syntax tree).
///
/// Returns the root `SyntaxNode` and any parse errors. Unlike [`parse`],
/// this does NOT lower to AST. Every character of the source is
/// represented as a token in the tree (including comments and whitespace).
///
/// Primary use: the formatter, which needs all tokens to avoid destroying
/// comments and content that the AST does not preserve.
pub fn parse_cst(source: &str) -> (syntax_kind::SyntaxNode, Vec<ParseError>) {
    use logos::Logos;

    let lex = lexer::Token::lexer(source);

    let mut tokens = Vec::new();
    let mut spans = Vec::new();

    for (tok, span) in lex.spanned() {
        if let Ok(t) = tok {
            let kind = syntax_kind::SyntaxKind::from(&t);
            let text = source[span.clone()].to_string();
            tokens.push(cst::LexedToken { kind, text });
            spans.push(cst::TokenSpan {
                start: span.start,
                end: span.end,
            });
        }
    }

    let mut parser = cst::Parser::new(tokens, spans);
    grammar::source_file(&mut parser);
    let (events, toks, errors) = parser.finish();

    let green = cst::build_tree(events, &toks);
    let root = syntax_kind::SyntaxNode::new_root(green);
    (root, errors)
}

/// Parse source text, panicking on errors. Convenience for tests.
///
/// Returns the parsed `SourceFile`. Panics if the source has parse errors
/// or if parsing returns `None`.
pub fn parse_unwrap(source: &str) -> ast::SourceFile {
    let (file, errs) = parse(source);
    assert!(errs.is_empty(), "unexpected parse errors: {errs:?}");
    file.expect("parse returned None")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_returns_token_count() {
        let src = "contract Foo { requires x > 0 }";
        let result = parse_full(src);
        result.file.unwrap();
        assert!(result.lex_errors.is_empty());
        assert!(result.token_count > 0);
    }

    #[test]
    fn parse_full_captures_lex_errors() {
        // '$' is not a valid token
        let src = "contract Foo { requires $ > 0 }";
        let result = parse_full(src);
        assert!(!result.lex_errors.is_empty());
        assert!(result.lex_errors[0].span.start < src.len());
    }

    #[test]
    fn parse_full_matches_parse() {
        let src = "contract SafeDiv { requires divisor != 0 ensures result * divisor == dividend }";
        let (file_old, errs_old) = parse(src);
        let result = parse_full(src);
        // Both should produce a valid file with the same number of declarations
        assert_eq!(errs_old.len(), result.parse_errors.len());
        assert_eq!(
            file_old.unwrap().decls.len(),
            result.file.unwrap().decls.len()
        );
    }

    // ---- Grammar: items ----

    #[test]
    fn parse_contract_with_clauses() {
        let src = "contract SafeDiv {\n  input(a: Int, b: Int)\n  requires { b != 0 }\n  ensures { result: Int }\n}";
        let file = parse_unwrap(src);
        assert_eq!(file.decls.len(), 1);
        match &file.decls[0].node {
            ast::Decl::Contract(c) => {
                assert_eq!(c.name, "SafeDiv");
                assert!(c.clauses.len() >= 3);
            }
            other => panic!("expected Contract, got {other:?}"),
        }
    }

    #[test]
    fn parse_type_definition() {
        let src = "type Point { x: Int, y: Int }";
        let file = parse_unwrap(src);
        assert_eq!(file.decls.len(), 1);
        match &file.decls[0].node {
            ast::Decl::TypeDef(td) => assert_eq!(td.name, "Point"),
            other => panic!("expected TypeDef, got {other:?}"),
        }
    }

    #[test]
    fn parse_enum_definition() {
        let src = "enum Color { Red, Green, Blue }";
        let file = parse_unwrap(src);
        assert_eq!(file.decls.len(), 1);
        match &file.decls[0].node {
            ast::Decl::EnumDef(e) => {
                assert_eq!(e.name, "Color");
                assert_eq!(e.variants.len(), 3);
            }
            other => panic!("expected EnumDef, got {other:?}"),
        }
    }

    #[test]
    fn parse_fn_definition() {
        let src = "fn add(a: Int, b: Int) -> Int\n  effects: pure";
        let file = parse_unwrap(src);
        assert_eq!(file.decls.len(), 1);
        match &file.decls[0].node {
            ast::Decl::FnDef(f) => assert_eq!(f.name, "add"),
            other => panic!("expected FnDef, got {other:?}"),
        }
    }

    #[test]
    fn parse_service_declaration() {
        let src = "service OrderService {\n  type Order { id: Int }\n  states: Created -> Paid -> Shipped\n}";
        let file = parse_unwrap(src);
        assert_eq!(file.decls.len(), 1);
        match &file.decls[0].node {
            ast::Decl::Service(s) => assert_eq!(s.name, "OrderService"),
            other => panic!("expected Service, got {other:?}"),
        }
    }

    #[test]
    fn parse_extern_declaration() {
        let src = "extern fn malloc(size: Nat) -> Bytes";
        let file = parse_unwrap(src);
        assert_eq!(file.decls.len(), 1);
        match &file.decls[0].node {
            ast::Decl::Extern(e) => assert_eq!(e.name, "malloc"),
            other => panic!("expected ExternFn, got {other:?}"),
        }
    }

    #[test]
    fn parse_bind_declaration() {
        let src =
            "bind \"std::collections::HashMap\" as HashMap {\n  input(key: String, value: Int)\n}";
        let file = parse_unwrap(src);
        assert_eq!(file.decls.len(), 1);
        match &file.decls[0].node {
            ast::Decl::Bind(b) => assert_eq!(b.name, "HashMap"),
            other => panic!("expected Bind, got {other:?}"),
        }
    }

    #[test]
    fn parse_prophecy_declaration() {
        let src = "prophecy future_val: Int\ncontract X { requires { true } }";
        let file = parse_unwrap(src);
        assert!(
            file.decls
                .iter()
                .any(|d| matches!(&d.node, ast::Decl::Prophecy(_)))
        );
    }

    // ---- Grammar: imports ----

    #[test]
    fn parse_simple_import() {
        let src = "import std.math\ncontract X { requires { true } }";
        let file = parse_unwrap(src);
        assert_eq!(file.imports.len(), 1);
        assert_eq!(file.imports[0].path.join("."), "std.math");
    }

    #[test]
    fn parse_selective_import() {
        let src = "import std.collections { HashMap, HashSet }\ncontract X { requires { true } }";
        let file = parse_unwrap(src);
        assert_eq!(file.imports.len(), 1);
        assert!(!file.imports[0].items.is_empty());
    }

    // ---- Grammar: params ----

    #[test]
    fn parse_generic_type_params() {
        let src = "type Pair<A, B> { first: A, second: B }";
        let file = parse_unwrap(src);
        match &file.decls[0].node {
            ast::Decl::TypeDef(td) => {
                assert_eq!(td.name, "Pair");
                assert!(td.type_params.len() >= 2);
            }
            other => panic!("expected TypeDef, got {other:?}"),
        }
    }

    #[test]
    fn parse_multiple_declarations() {
        let src = "type Point { x: Int, y: Int }\n\ncontract MovePoint {\n  input(p: Point, dx: Int)\n  ensures { result: Point }\n}\n\nenum Direction { North, South, East, West }";
        let file = parse_unwrap(src);
        assert_eq!(file.decls.len(), 3);
    }

    // ---- Grammar: clauses ----

    #[test]
    fn parse_requires_ensures_invariant() {
        let src = "contract Buffer {\n  input(data: Bytes, idx: Nat)\n  requires { idx < 256 }\n  ensures { result >= 0 }\n  invariant { idx < 256 }\n}";
        let file = parse_unwrap(src);
        match &file.decls[0].node {
            ast::Decl::Contract(c) => {
                let kinds: Vec<_> = c.clauses.iter().map(|cl| &cl.kind).collect();
                assert!(kinds.iter().any(|k| matches!(k, ast::ClauseKind::Input)));
                assert!(kinds.iter().any(|k| matches!(k, ast::ClauseKind::Requires)));
                assert!(kinds.iter().any(|k| matches!(k, ast::ClauseKind::Ensures)));
                assert!(
                    kinds
                        .iter()
                        .any(|k| matches!(k, ast::ClauseKind::Invariant))
                );
            }
            other => panic!("expected Contract, got {other:?}"),
        }
    }

    #[test]
    fn parse_effects_clause() {
        let src = "fn read_file(path: String) -> Bytes\n  effects: io";
        let file = parse_unwrap(src);
        match &file.decls[0].node {
            ast::Decl::FnDef(f) => {
                assert!(
                    f.clauses
                        .iter()
                        .any(|c| matches!(c.kind, ast::ClauseKind::Effects))
                );
            }
            other => panic!("expected FnDef, got {other:?}"),
        }
    }

    // ---- Error recovery ----

    #[test]
    fn parse_recovers_from_errors() {
        let src = "contract Good { requires { true } }\n@@@ invalid\ncontract AlsoGood { ensures { true } }";
        let (file, errors) = parse(src);
        assert!(!errors.is_empty());
        // Parser should recover and find at least one valid declaration
        if let Some(f) = file {
            assert!(!f.decls.is_empty());
        }
    }

    #[test]
    fn parse_garbage_returns_none() {
        let (file, errors) = parse("!!! @@@ ### $$$");
        assert!(!errors.is_empty());
        // Pure garbage should return None
        assert!(file.is_none());
    }

    // --- Error recovery sync tests (#632) ---

    #[test]
    fn recovery_sync_skips_garbage_between_declarations() {
        // Multiple garbage tokens between two valid declarations.
        // With sync-based recovery the parser skips to the next
        // declaration keyword in one step instead of one token at a time.
        let src =
            "contract A { requires { true } }\nfoo bar baz qux\ncontract B { ensures { true } }";
        let (file, errors) = parse(src);
        let file = file.expect("should recover and return a file");
        // Both declarations survive despite the garbage in between.
        assert!(
            file.decls.len() >= 2,
            "expected at least 2 decls, got {}",
            file.decls.len()
        );
        // Sync-based recovery produces exactly 1 error for the garbage
        // region, not one per garbage token.
        assert!(
            errors.len() <= 2,
            "expected at most 2 errors from sync recovery, got {}: {:?}",
            errors.len(),
            errors
        );
    }

    #[test]
    fn recovery_sync_inside_contract_body() {
        // Garbage tokens inside a contract body should sync to the
        // next clause keyword, not cascade one-by-one.
        let src = "contract C {\n  ??? !!!\n  requires { x > 0 }\n  ensures { true }\n}";
        let (file, errors) = parse(src);
        let file = file.expect("should recover inside contract body");
        assert!(
            !file.decls.is_empty(),
            "should parse the contract declaration"
        );
        // The requires and ensures clauses after the garbage should
        // still be reachable.
        assert!(
            errors.len() <= 3,
            "expected limited cascading errors, got {}: {:?}",
            errors.len(),
            errors
        );
    }

    #[test]
    fn recovery_sync_preserves_later_declarations() {
        // A broken first declaration should not prevent parsing the rest.
        let src =
            "contract Bad { ??? ??? }\ntype Point { x: Int, y: Int }\nenum Color { Red, Green }";
        let (file, errors) = parse(src);
        let file = file.expect("should recover across declarations");
        // The valid type and enum after the broken contract must parse.
        assert!(
            file.decls.len() >= 2,
            "expected at least 2 decls after recovery, got {}: {:?}",
            file.decls.len(),
            file.decls
                .iter()
                .map(|d| d.node.summary_label())
                .collect::<Vec<_>>()
        );
        assert!(!errors.is_empty(), "should report errors for the garbage");
    }
}
