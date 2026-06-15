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
        assert!(result.file.is_some());
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
        assert!(file_old.is_some());
        assert!(result.file.is_some());
        assert_eq!(errs_old.len(), result.parse_errors.len());
        assert_eq!(
            file_old.unwrap().decls.len(),
            result.file.unwrap().decls.len()
        );
    }
}
