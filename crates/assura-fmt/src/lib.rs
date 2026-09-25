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

    let formatted = format_cst_tokens(&root);
    // Always walk CST tokens for minified `{...}` pairs. Newlines are
    // inserted only when the brace pair has no existing newline (a header
    // comment must not skip a compacted contract body).
    let expanded = expand_minified_braces(&root);
    let (root2, errs2) = assura_parser::parse_cst(&expanded);
    if errs2.is_empty() {
        Ok(format_cst_tokens(&root2))
    } else {
        // Keep pre-expand `formatted` (already parseable)
        Ok(formatted)
    }
}

/// Insert newlines after `{` and before `}` when a brace pair is minified.
///
/// Walks CST tokens so COMMENT / STRING_LIT braces and quotes are copied
/// unchanged and do not change depth. Already-pretty pairs (intervening
/// whitespace already contains `\n`) are left alone.
fn expand_minified_braces(root: &assura_parser::syntax_kind::SyntaxNode) -> String {
    let tokens = collect_leaf_tokens(root);
    let mut out = String::with_capacity(tokens.iter().map(|(_, t)| t.len()).sum::<usize>() * 2);
    let mut depth: i32 = 0;
    for (i, (kind, text)) in tokens.iter().enumerate() {
        match *kind {
            SyntaxKind::COMMENT | SyntaxKind::STRING_LIT => {
                out.push_str(text);
            }
            SyntaxKind::L_BRACE => {
                out.push('{');
                depth += 1;
                if peek_non_ws(&tokens, i + 1).is_some_and(|k| k != SyntaxKind::R_BRACE)
                    && !intervening_ws_has_newline(&tokens, i + 1)
                {
                    out.push('\n');
                    for _ in 0..depth {
                        out.push_str("    ");
                    }
                }
            }
            SyntaxKind::R_BRACE => {
                depth = (depth - 1).max(0);
                while out.ends_with(' ') {
                    out.pop();
                }
                if !out.ends_with('\n') && !out.ends_with('{') {
                    out.push('\n');
                    for _ in 0..depth {
                        out.push_str("    ");
                    }
                }
                out.push('}');
                // `}ensures` → put next clause on its own indented line
                if let Some((_, next_text)) = tokens[i + 1..]
                    .iter()
                    .find(|(k, _)| *k != SyntaxKind::WHITESPACE)
                {
                    if next_text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                        && !intervening_ws_has_newline(&tokens, i + 1)
                    {
                        out.push('\n');
                        for _ in 0..depth {
                            out.push_str("    ");
                        }
                    }
                }
            }
            _ => out.push_str(text),
        }
    }
    let trimmed = out.trim_end_matches('\n');
    let mut result = trimmed.to_string();
    result.push('\n');
    result
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

struct FmtTok {
    kind: SyntaxKind,
    text: String,
    /// Operator token that is a direct child of `BIN_EXPR`.
    /// Generic `<` / `>` are not.
    infix: bool,
}

fn is_infix_op(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::EQ
            | SyntaxKind::NEQ
            | SyntaxKind::L_ANGLE
            | SyntaxKind::R_ANGLE
            | SyntaxKind::LTE
            | SyntaxKind::GTE
            | SyntaxKind::PLUS
            | SyntaxKind::MINUS
            | SyntaxKind::STAR
            | SyntaxKind::SLASH
            | SyntaxKind::PERCENT
            | SyntaxKind::AND_AND
            | SyntaxKind::OR_OR
            | SyntaxKind::AND_KW
            | SyntaxKind::OR_KW
            | SyntaxKind::IN_KW
            | SyntaxKind::IS_KW
            | SyntaxKind::DOT_DOT
            | SyntaxKind::CONCAT
    )
}

fn collect_fmt_tokens(node: &assura_parser::syntax_kind::SyntaxNode, out: &mut Vec<FmtTok>) {
    let bin = node.kind() == SyntaxKind::BIN_EXPR;
    for child in node.children_with_tokens() {
        if let Some(tok) = child.as_token() {
            let kind = tok.kind();
            let text = tok.text().to_string();
            let infix = bin && (is_infix_op(kind) || (kind == SyntaxKind::IDENT && text == "mod"));
            out.push(FmtTok { kind, text, infix });
        } else if let Some(n) = child.as_node() {
            collect_fmt_tokens(n, out);
        }
    }
}

/// Collect all leaf tokens from the CST in document order.
///
/// Use `as_token` / `as_node` (not `rowan::NodeOrToken` match arms) so this
/// crate does not depend on a concrete `rowan` version. Packaging against
/// crates.io `assura-parser` would otherwise dual-link rowan 0.16 + 0.17.
fn collect_leaf_tokens(node: &assura_parser::syntax_kind::SyntaxNode) -> Vec<(SyntaxKind, String)> {
    let mut tokens = Vec::new();
    collect_leaf_tokens_into(node, &mut tokens);
    tokens
}

fn collect_leaf_tokens_into(
    node: &assura_parser::syntax_kind::SyntaxNode,
    tokens: &mut Vec<(SyntaxKind, String)>,
) {
    for child in node.children_with_tokens() {
        if let Some(tok) = child.as_token() {
            tokens.push((tok.kind(), tok.text().to_string()));
        } else if let Some(n) = child.as_node() {
            collect_leaf_tokens_into(n, tokens);
        }
    }
}

/// Peek ahead past whitespace tokens to find the next significant kind.
fn peek_non_ws(tokens: &[(SyntaxKind, String)], start: usize) -> Option<SyntaxKind> {
    tokens[start..]
        .iter()
        .find(|(k, _)| *k != SyntaxKind::WHITESPACE)
        .map(|(k, _)| *k)
}

fn peek_fmt_non_ws(tokens: &[FmtTok], start: usize) -> Option<SyntaxKind> {
    tokens[start..]
        .iter()
        .find(|t| t.kind != SyntaxKind::WHITESPACE)
        .map(|t| t.kind)
}

/// True when whitespace between `start` and the next non-ws token already
/// contains a newline (the pair is not minified).
fn intervening_ws_has_newline(tokens: &[(SyntaxKind, String)], start: usize) -> bool {
    for (k, t) in &tokens[start..] {
        if *k != SyntaxKind::WHITESPACE {
            return false;
        }
        if t.contains('\n') {
            return true;
        }
    }
    false
}

/// Space that must exist even when the source omitted it.
///
/// `x:Int` and `x: Int` were both stable. `::` stays tight. `->` and `=>`
/// get a space on each side.
fn needs_space_before(kind: SyntaxKind, out: &str) -> bool {
    if out.is_empty() {
        return false;
    }
    // A single `:` starts a type. `::` is a path separator and stays tight.
    if out.ends_with(':') && !out.ends_with("::") && kind != SyntaxKind::COLON {
        return true;
    }
    if matches!(kind, SyntaxKind::ARROW | SyntaxKind::FAT_ARROW)
        && out.chars().next_back().is_some_and(|c| !c.is_whitespace())
    {
        return true;
    }
    (out.ends_with("->") || out.ends_with("=>")) && kind != SyntaxKind::WHITESPACE
}

/// Walk CST tokens and emit formatted output.
///
/// Indentation is tracked via brace depth (`{` increments, `}` decrements).
/// Whitespace tokens containing newlines are replaced with normalized
/// indentation. Horizontal whitespace between two tokens is one space.
/// A horizontal run at the start or end of the file is dropped.
fn format_cst_tokens(root: &assura_parser::syntax_kind::SyntaxNode) -> String {
    let mut tokens = Vec::new();
    collect_fmt_tokens(root, &mut tokens);
    let mut out = String::new();
    let mut brace_depth: i32 = 0;
    let mut space_after_infix = false;

    for (i, tok) in tokens.iter().enumerate() {
        let kind = tok.kind;
        let text = tok.text.as_str();
        match kind {
            SyntaxKind::COMMENT => {
                // Trivia does not consume the space owed to the next operand.
                let owed = space_after_infix
                    && out.chars().next_back().is_some_and(|c| !c.is_whitespace());
                if owed || needs_space_before(kind, &out) {
                    out.push(' ');
                }
                out.push_str(text);
            }
            SyntaxKind::STRING_LIT => {
                if space_after_infix && out.chars().next_back().is_some_and(|c| !c.is_whitespace())
                {
                    out.push(' ');
                }
                space_after_infix = false;
                if needs_space_before(kind, &out) {
                    out.push(' ');
                }
                out.push_str(text);
            }
            SyntaxKind::L_BRACE => {
                if out.chars().next_back().is_some_and(|c| !c.is_whitespace()) {
                    out.push(' ');
                }
                out.push('{');
                brace_depth += 1;
                space_after_infix = false;
            }
            SyntaxKind::R_BRACE => {
                brace_depth = (brace_depth - 1).max(0);
                space_after_infix = false;
                out.push('}');
            }
            SyntaxKind::WHITESPACE => {
                if text.contains('\n') {
                    // Cap consecutive blank lines at 2
                    let newlines = text.matches('\n').count().min(3);

                    // Peek ahead: if the next non-whitespace token is `}`,
                    // dedent by one level for the closing brace line.
                    let next_is_rbrace =
                        peek_fmt_non_ws(&tokens, i + 1).is_some_and(|k| k == SyntaxKind::R_BRACE);
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
                } else if !text.is_empty() {
                    // One space between tokens. Skip a run at the start or
                    // end of the file, and skip a run that follows a newline
                    // indent (that indent is already the separator).
                    let prev_is_token = out.chars().next_back().is_some_and(|c| !c.is_whitespace());
                    let next_is_token = peek_fmt_non_ws(&tokens, i + 1).is_some();
                    if prev_is_token && next_is_token {
                        out.push(' ');
                    }
                }
            }
            _ if tok.infix => {
                if out.chars().next_back().is_some_and(|c| !c.is_whitespace()) {
                    out.push(' ');
                }
                out.push_str(text);
                space_after_infix = true;
            }
            _ => {
                if space_after_infix && out.chars().next_back().is_some_and(|c| !c.is_whitespace())
                {
                    out.push(' ');
                }
                space_after_infix = false;
                if needs_space_before(kind, &out) {
                    out.push(' ');
                }
                out.push_str(text);
                if matches!(kind, SyntaxKind::ARROW | SyntaxKind::FAT_ARROW) {
                    space_after_infix = true;
                }
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
