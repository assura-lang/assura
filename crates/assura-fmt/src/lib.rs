//! Source code formatter for the Assura contract language.
//!
//! Uses the lossless CST (concrete syntax tree) so that comments, fn bodies,
//! and all declaration types are preserved. Only whitespace is normalized:
//! indentation follows brace nesting (4 spaces per level), trailing
//! whitespace is stripped, and the file ends with a single newline.

use assura_parser::syntax_kind::SyntaxKind;

/// Format Assura source text, preserving all comments and content.
///
/// Parses to a lossless CST and normalizes whitespace (indentation,
/// trailing spaces, blank lines). Returns the source unchanged if
/// there are parse errors.
pub fn format_source(source: &str) -> String {
    match try_format_source(source) {
        Ok(formatted) => formatted,
        Err(_) => source.to_string(),
    }
}

/// Try to format Assura source text, returning `Err` with the parse
/// errors if the source cannot be parsed.
pub fn try_format_source(source: &str) -> Result<String, Vec<assura_parser::ParseError>> {
    let (root, errors) = assura_parser::parse_cst(source);

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(format_cst_tokens(&root))
}

/// Deprecated: use [`format_source`] instead. This wrapper parses the
/// source from the AST's text, but the CST-based path is preferred.
#[deprecated(note = "use format_source(&str) for lossless formatting")]
pub fn format_source_file(_file: &assura_parser::ast::SourceFile) -> String {
    // Cannot recover original source from the lossy AST. Callers should
    // migrate to format_source(). This stub exists only to keep the
    // crate compiling during the transition.
    String::new()
}

// ---------------------------------------------------------------------------
// CST-based formatting engine
// ---------------------------------------------------------------------------

/// Collect all leaf tokens from the CST in document order.
fn collect_leaf_tokens(node: &assura_parser::syntax_kind::SyntaxNode) -> Vec<(SyntaxKind, String)> {
    let mut tokens = Vec::new();
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(tok) => {
                tokens.push((tok.kind(), tok.text().to_string()));
            }
            rowan::NodeOrToken::Node(n) => {
                collect_leaf_tokens_into(&n, &mut tokens);
            }
        }
    }
    tokens
}

fn collect_leaf_tokens_into(
    node: &assura_parser::syntax_kind::SyntaxNode,
    tokens: &mut Vec<(SyntaxKind, String)>,
) {
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(tok) => {
                tokens.push((tok.kind(), tok.text().to_string()));
            }
            rowan::NodeOrToken::Node(n) => {
                collect_leaf_tokens_into(&n, tokens);
            }
        }
    }
}

/// Peek ahead past whitespace tokens to find the next significant kind.
fn peek_non_ws(tokens: &[(SyntaxKind, String)], start: usize) -> Option<SyntaxKind> {
    tokens[start..]
        .iter()
        .find(|(k, _)| *k != SyntaxKind::WHITESPACE && *k != SyntaxKind::COMMENT)
        .map(|(k, _)| *k)
}

/// Walk CST tokens and emit formatted output.
///
/// Indentation is tracked via brace depth (`{` increments, `}` decrements).
/// Whitespace tokens containing newlines are replaced with normalized
/// indentation; inline whitespace is passed through.
fn format_cst_tokens(root: &assura_parser::syntax_kind::SyntaxNode) -> String {
    let tokens = collect_leaf_tokens(root);
    let mut out = String::new();
    let mut brace_depth: i32 = 0;

    for (i, (kind, text)) in tokens.iter().enumerate() {
        match *kind {
            SyntaxKind::L_BRACE => {
                out.push('{');
                brace_depth += 1;
            }
            SyntaxKind::R_BRACE => {
                brace_depth = (brace_depth - 1).max(0);
                out.push('}');
            }
            SyntaxKind::WHITESPACE => {
                if text.contains('\n') {
                    // Cap consecutive blank lines at 2
                    let newlines = text.matches('\n').count().min(3);

                    // Peek ahead: if the next non-ws/comment token is `}`,
                    // dedent by one level for the closing brace line.
                    let next_is_rbrace =
                        peek_non_ws(&tokens, i + 1).is_some_and(|k| k == SyntaxKind::R_BRACE);
                    let indent = if next_is_rbrace {
                        (brace_depth - 1).max(0) as usize
                    } else {
                        brace_depth.max(0) as usize
                    };

                    for _ in 0..newlines {
                        out.push('\n');
                    }
                    for _ in 0..indent {
                        out.push_str("    ");
                    }
                } else {
                    // Inline whitespace: preserve as-is
                    out.push_str(text);
                }
            }
            _ => {
                out.push_str(text);
            }
        }
    }

    // Ensure file ends with exactly one newline
    let trimmed = out.trim_end_matches('\n');
    let mut result = trimmed.to_string();
    result.push('\n');
    result
}

#[cfg(test)]
mod format_tests;
