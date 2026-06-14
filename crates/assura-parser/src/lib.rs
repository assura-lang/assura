pub mod ast;
pub(crate) mod cst;
pub mod display;
pub(crate) mod grammar;
pub mod lexer;
pub mod lower;
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

/// Parse source text, panicking on errors. Convenience for tests.
///
/// Returns the parsed `SourceFile`. Panics if the source has parse errors
/// or if parsing returns `None`.
pub fn parse_unwrap(source: &str) -> ast::SourceFile {
    let (file, errs) = parse(source);
    assert!(errs.is_empty(), "unexpected parse errors: {errs:?}");
    file.expect("parse returned None")
}
