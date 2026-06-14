// chumsky 0.9's filter_map requires returning Simple<Token> which is large;
// this is inherent to the library API and cannot be reduced.
#![allow(clippy::result_large_err)]

pub mod ast;
pub mod display;
pub mod lexer;
pub mod parser;

/// Parse source text into a SourceFile, returning the AST and any errors.
///
/// This is the primary public API: lex + parse in one call.
pub fn parse(
    source: &str,
) -> (
    Option<ast::SourceFile>,
    Vec<chumsky::error::Simple<lexer::Token>>,
) {
    use chumsky::Stream;
    use chumsky::prelude::*;
    use logos::Logos;

    let lex = lexer::Token::lexer(source);
    let mut tokens: Vec<(lexer::Token, std::ops::Range<usize>)> = Vec::new();

    for (tok, span) in lex.spanned() {
        if let Ok(t) = tok {
            tokens.push((t, span));
        }
    }

    let len = source.len();
    let stream = Stream::from_iter(len..len + 1, tokens.into_iter());
    parser::source_file().parse_recovery(stream)
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
