//! Parser engine for building rowan CSTs via an event/marker pattern.
//!
//! This module provides the low-level machinery: a `Parser` struct that
//! consumes a flat token stream and emits `Event`s. A separate
//! `build_tree` pass converts events into a rowan `GreenNode`.
//!
//! Grammar rules live in the `grammar` module and drive this engine.

use crate::syntax_kind::SyntaxKind;

// -----------------------------------------------------------------
// Events
// -----------------------------------------------------------------

/// A single event emitted by the parser. Collected into a `Vec` and
/// then replayed by `build_tree` to produce the green tree.
#[derive(Debug, Clone)]
pub(crate) enum Event {
    /// Start a new composite node. `forward_parent` optionally points
    /// to a later `Open` event that should become this node's parent
    /// (used by `CompletedMarker::precede`).
    Open {
        kind: SyntaxKind,
        forward_parent: Option<u32>,
    },
    /// Finish the current composite node.
    Close,
    /// Consume the next token from the input.
    Advance,
}

/// Sentinel kind used as a placeholder in `Open` events before the
/// real kind is known (patched by `Marker::complete`).
const TOMBSTONE: SyntaxKind = SyntaxKind::TOMBSTONE;

// -----------------------------------------------------------------
// Markers
// -----------------------------------------------------------------

/// A marker for an in-progress node. Created by `Parser::open()`,
/// finished by `complete()` or abandoned by `abandon()`.
pub(crate) struct Marker {
    pos: u32,
    completed: bool,
}

impl Marker {
    /// Finish this node with the given kind.
    pub(crate) fn complete(mut self, p: &mut Parser, kind: SyntaxKind) -> CompletedMarker {
        self.completed = true;
        match &mut p.events[self.pos as usize] {
            Event::Open { kind: slot, .. } => *slot = kind,
            _ => unreachable!(),
        }
        p.events.push(Event::Close);
        CompletedMarker { pos: self.pos }
    }
}

impl Drop for Marker {
    fn drop(&mut self) {
        if !self.completed {
            // In debug builds, warn about uncompleted markers.
            // We must NOT panic here because a double-panic (this Drop
            // running during unwind from another panic) causes SIGABRT.
            #[cfg(debug_assertions)]
            eprintln!(
                "WARNING: Marker at position {} dropped without complete() or abandon()",
                self.pos
            );
        }
    }
}

/// A completed node marker. Can be used to retroactively wrap this
/// node in a new parent via `precede()`.
pub(crate) struct CompletedMarker {
    pos: u32,
}

impl CompletedMarker {
    /// Create a new parent node that wraps this already-completed node.
    /// Essential for Pratt parsing of binary expressions.
    pub(crate) fn precede(self, p: &mut Parser) -> Marker {
        let new_pos = p.open();
        match &mut p.events[self.pos as usize] {
            Event::Open { forward_parent, .. } => {
                *forward_parent = Some(new_pos.pos);
            }
            _ => unreachable!(),
        }
        new_pos
    }
}

// -----------------------------------------------------------------
// Parser
// -----------------------------------------------------------------

/// A token from the lexer, ready for the parser.
#[derive(Debug, Clone)]
pub struct LexedToken {
    pub kind: SyntaxKind,
    pub text: String,
}

/// The recursive-descent parser. Holds the token stream, current
/// position, event buffer, collected errors, and a fuel counter
/// to prevent infinite loops in error recovery.
pub(crate) struct Parser {
    pub(crate) tokens: Vec<LexedToken>,
    spans: Vec<TokenSpan>,
    pos: usize,
    pub(crate) events: Vec<Event>,
    fuel: u32,
    pub(crate) errors: Vec<ParseError>,
    /// Nesting depth counter to prevent stack overflow on deeply nested
    /// expressions (e.g. 500 levels of parentheses). The parser emits
    /// an error and stops recursing when this reaches zero.
    depth: u32,
}

/// A parse error with location and message.
#[derive(Debug, Clone)]
pub struct ParseError {
    /// Error code from the spec (e.g. "A01002").
    pub code: &'static str,
    /// Byte offset range in the source where the error occurred.
    pub span: std::ops::Range<usize>,
    /// Human-readable error message.
    pub message: String,
}

impl ParseError {
    /// Returns the byte-offset span of this error.
    ///
    /// This method exists for API compatibility with downstream crates
    /// (LSP, gRPC server) that call `.span()` on parse errors.
    pub fn span(&self) -> std::ops::Range<usize> {
        self.span.clone()
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}..{}: {}",
            self.span.start, self.span.end, self.message
        )
    }
}

/// Source-span information carried alongside each token for error reporting.
#[derive(Debug, Clone)]
pub struct TokenSpan {
    pub start: usize,
    pub end: usize,
}

fn is_trivia(k: SyntaxKind) -> bool {
    k == SyntaxKind::WHITESPACE || k == SyntaxKind::COMMENT
}

impl Parser {
    /// Create a new parser from a sequence of lexed tokens with spans.
    pub(crate) fn new(tokens: Vec<LexedToken>, spans: Vec<TokenSpan>) -> Self {
        assert_eq!(tokens.len(), spans.len());
        Self {
            tokens,
            spans,
            pos: 0,
            events: Vec::new(),
            fuel: 256,
            errors: Vec::new(),
            depth: 128,
        }
    }

    /// Start a new node. Returns a `Marker` that must be completed or
    /// abandoned.
    pub(crate) fn open(&mut self) -> Marker {
        let pos = self.events.len() as u32;
        self.events.push(Event::Open {
            kind: TOMBSTONE,
            forward_parent: None,
        });
        Marker {
            pos,
            completed: false,
        }
    }

    /// Consume the current *significant* (non-trivia) token and advance.
    /// Any trivia tokens immediately before it are emitted first (so their
    /// lengths pad the source offsets for correct text_range() on nodes).
    pub(crate) fn bump(&mut self) {
        self.bump_trivia();
        if self.eof() {
            return;
        }
        self.fuel = 256;
        self.events.push(Event::Advance);
        self.pos += 1;
    }

    /// Emit all immediately following trivia tokens (ws/comments) as Advance
    /// events under the current parent. Used internally by bump() and
    /// explicitly after manual delimiter bumps in clause bodies etc.
    pub(crate) fn bump_trivia(&mut self) {
        while !self.eof_raw() && is_trivia(self.tokens[self.pos].kind) {
            self.fuel = 256;
            self.events.push(Event::Advance);
            self.pos += 1;
        }
    }

    /// Bump a delimiter (e.g. '{' or '(' after a clause keyword) and
    /// immediately emit any following trivia. This ensures child expression
    /// nodes get text_range() that point to original source content rather
    /// than being offset by leading whitespace/comments inside the braces.
    /// Added during #335 trivia/spans work to eliminate repeated boilerplate.
    pub(crate) fn bump_delim(&mut self) {
        self.bump();
        self.bump_trivia();
    }

    /// Always consume exactly the next token (trivia or not) as Advance.
    /// Used by raw token collectors (clause_body for effects etc) that want
    /// to include everything for correct offset reconstruction.
    pub(crate) fn bump_raw(&mut self) {
        if self.eof() {
            return;
        }
        self.fuel = 256;
        self.events.push(Event::Advance);
        self.pos += 1;
    }

    /// The `SyntaxKind` of the next *significant* (non-trivia) token.
    pub(crate) fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    /// Lookahead for the n-th significant (non-trivia) token ahead.
    pub(crate) fn nth(&self, n: usize) -> SyntaxKind {
        let mut seen = 0;
        let mut i = self.pos;
        while i < self.tokens.len() {
            let k = self.tokens[i].kind;
            if !is_trivia(k) {
                if seen == n {
                    return k;
                }
                seen += 1;
            }
            i += 1;
        }
        SyntaxKind::ERROR_TOKEN
    }

    /// Raw current without skipping trivia (for raw collectors and EOF checks).
    pub(crate) fn current_raw(&self) -> SyntaxKind {
        self.tokens
            .get(self.pos)
            .map(|t| t.kind)
            .unwrap_or(SyntaxKind::ERROR_TOKEN)
    }

    /// True if the current token matches `kind`. Decrements fuel.
    pub(crate) fn at(&mut self, kind: SyntaxKind) -> bool {
        if self.fuel == 0 {
            // Parser is stuck in an infinite loop. Force EOF state so
            // all `while !p.eof()` loops terminate gracefully.
            self.error_at_current(
                "parser stuck: infinite loop detected (fuel exhausted)".to_string(),
            );
            self.pos = self.tokens.len();
            return false;
        }
        self.fuel -= 1;
        self.current() == kind
    }

    /// True if the current token matches any kind in `kinds`.
    pub(crate) fn at_any(&mut self, kinds: &[SyntaxKind]) -> bool {
        if self.fuel == 0 {
            self.error_at_current(
                "parser stuck: infinite loop detected (fuel exhausted)".to_string(),
            );
            self.pos = self.tokens.len();
            return false;
        }
        self.fuel -= 1;
        kinds.contains(&self.current())
    }

    /// Consume the current token if it matches `kind`. Returns true
    /// if consumed.
    pub(crate) fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.current() == kind {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Consume the current token if it matches `kind`, otherwise emit
    /// an error.
    pub(crate) fn expect(&mut self, kind: SyntaxKind) {
        if !self.eat(kind) {
            self.error_at_current(format!("expected {kind:?}"));
        }
    }

    /// True if we've consumed all *significant* (non-trivia) tokens.
    /// Trivia at the end do not count as content.
    pub(crate) fn eof(&self) -> bool {
        let mut i = self.pos;
        while i < self.tokens.len() {
            if !is_trivia(self.tokens[i].kind) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// True if pos reached end of token list (raw, no trivia skip).
    pub(crate) fn eof_raw(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    /// Enter a nesting level. Returns false (and emits an error) if
    /// the maximum nesting depth (128) has been reached, signaling the
    /// caller to stop recursing.
    pub(crate) fn enter_nesting(&mut self) -> bool {
        if self.depth == 0 {
            self.error_at_current("nesting depth limit exceeded".to_string());
            return false;
        }
        self.depth -= 1;
        true
    }

    /// Leave a nesting level (restores one level of depth).
    pub(crate) fn leave_nesting(&mut self) {
        self.depth += 1;
    }

    /// Current position in the token stream.
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    /// Emit an error at the current token's location.
    pub(crate) fn error_at_current(&mut self, message: String) {
        let span = self.current_span();
        self.errors.push(ParseError {
            code: "A01002",
            span: span.start..span.end,
            message,
        });
    }

    /// Consume the parser, returning events, tokens, and collected errors.
    pub(crate) fn finish(self) -> (Vec<Event>, Vec<LexedToken>, Vec<ParseError>) {
        (self.events, self.tokens, self.errors)
    }

    /// The text of the current *significant* (non-trivia) token.
    /// Matches the token returned by current()/nth().
    pub(crate) fn current_text(&self) -> &str {
        let mut i = self.pos;
        while i < self.tokens.len() {
            let t = &self.tokens[i];
            if !is_trivia(t.kind) {
                return t.text.as_str();
            }
            i += 1;
        }
        ""
    }

    /// True if the current token is a keyword that can appear as an
    /// identifier in certain positions (field names, block kind names, etc.)
    pub(crate) fn at_keyword_or_ident(&self) -> bool {
        let k = self.current();
        k == SyntaxKind::IDENT || k.is_keyword()
    }

    /// The source span of the current *significant* token (skips trivia for error location).
    pub(crate) fn current_span(&self) -> TokenSpan {
        let mut i = self.pos;
        while i < self.spans.len() {
            let k = self
                .tokens
                .get(i)
                .map(|t| t.kind)
                .unwrap_or(SyntaxKind::ERROR_TOKEN);
            if !is_trivia(k) {
                return self.spans[i].clone();
            }
            i += 1;
        }
        // EOF or only trivia: end of last
        self.spans
            .last()
            .map(|s| TokenSpan {
                start: s.end,
                end: s.end,
            })
            .unwrap_or(TokenSpan { start: 0, end: 0 })
    }

    /// Wrap the current token in an ERROR node and skip it (error recovery).
    pub(crate) fn err_and_bump(&mut self, message: &str) {
        self.error_at_current(message.to_string());
        let m = self.open();
        self.bump();
        m.complete(self, SyntaxKind::ERROR);
    }
}

// -----------------------------------------------------------------
// Green tree construction
// -----------------------------------------------------------------

/// Build a rowan `GreenNode` from parser events and the token stream.
///
/// Forward-parent links are resolved so that `precede()`-created
/// wrapper nodes open at the correct position in the tree.
pub(crate) fn build_tree(mut events: Vec<Event>, tokens: &[LexedToken]) -> rowan::GreenNode {
    let mut builder = rowan::GreenNodeBuilder::new();
    let mut token_idx: usize = 0;
    let mut chain_buf = Vec::new();
    let mut depth: u32 = 0;

    for idx in 0..events.len() {
        // Take the event, leaving a tombstone so forward-parent
        // targets that we consume early are skipped.
        let event = std::mem::replace(
            &mut events[idx],
            Event::Open {
                kind: TOMBSTONE,
                forward_parent: None,
            },
        );

        match event {
            Event::Open {
                kind,
                forward_parent,
            } => {
                // Walk the forward-parent chain, collecting kinds
                // from innermost (self) to outermost (final parent).
                chain_buf.clear();
                if kind != TOMBSTONE {
                    chain_buf.push(kind);
                }
                let mut fp = forward_parent;
                while let Some(target) = fp {
                    let target_event = std::mem::replace(
                        &mut events[target as usize],
                        Event::Open {
                            kind: TOMBSTONE,
                            forward_parent: None,
                        },
                    );
                    if let Event::Open {
                        kind: fk,
                        forward_parent: next_fp,
                    } = target_event
                    {
                        if fk != TOMBSTONE {
                            chain_buf.push(fk);
                        }
                        fp = next_fp;
                    } else {
                        break;
                    }
                }
                // Open from outermost (last) to innermost (first).
                for &k in chain_buf.iter().rev() {
                    builder.start_node(k.into());
                    depth += 1;
                }
            }
            Event::Close => {
                if depth > 0 {
                    builder.finish_node();
                    depth -= 1;
                }
            }
            Event::Advance => {
                if token_idx < tokens.len() {
                    let tok = &tokens[token_idx];
                    builder.token(tok.kind.into(), &tok.text);
                    token_idx += 1;
                }
            }
        }
    }

    // Emit any unconsumed tokens inside the current open node
    while token_idx < tokens.len() {
        let tok = &tokens[token_idx];
        builder.token(tok.kind.into(), &tok.text);
        token_idx += 1;
    }

    // Close any remaining open nodes
    while depth > 0 {
        builder.finish_node();
        depth -= 1;
    }

    builder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_complete_produces_open_close() {
        let tokens = vec![LexedToken {
            kind: SyntaxKind::IDENT,
            text: "x".into(),
        }];
        let spans = vec![TokenSpan { start: 0, end: 1 }];
        let mut p = Parser::new(tokens, spans);

        let m = p.open();
        p.bump();
        m.complete(&mut p, SyntaxKind::IDENT_EXPR);

        assert_eq!(p.events.len(), 3); // Open + Advance + Close
        assert!(matches!(
            &p.events[0],
            Event::Open {
                kind: SyntaxKind::IDENT_EXPR,
                ..
            }
        ));
        assert!(matches!(&p.events[1], Event::Advance));
        assert!(matches!(&p.events[2], Event::Close));
    }

    #[test]
    fn precede_creates_forward_parent() {
        let tokens = vec![
            LexedToken {
                kind: SyntaxKind::INT_LIT,
                text: "1".into(),
            },
            LexedToken {
                kind: SyntaxKind::PLUS,
                text: "+".into(),
            },
            LexedToken {
                kind: SyntaxKind::INT_LIT,
                text: "2".into(),
            },
        ];
        let spans = vec![
            TokenSpan { start: 0, end: 1 },
            TokenSpan { start: 2, end: 3 },
            TokenSpan { start: 4, end: 5 },
        ];
        let mut p = Parser::new(tokens, spans);

        // Parse: literal "1"
        let m = p.open();
        p.bump();
        let lhs = m.complete(&mut p, SyntaxKind::LITERAL_EXPR);

        // Wrap in BIN_EXPR using precede
        let m2 = lhs.precede(&mut p);
        p.bump(); // +
        let m3 = p.open();
        p.bump(); // 2
        m3.complete(&mut p, SyntaxKind::LITERAL_EXPR);
        m2.complete(&mut p, SyntaxKind::BIN_EXPR);

        // Build tree and verify structure
        let green = build_tree(p.events, &p.tokens);

        // The root should be a BIN_EXPR containing LITERAL_EXPR + LITERAL_EXPR
        let root = crate::syntax_kind::SyntaxNode::new_root(green);
        assert_eq!(root.kind(), SyntaxKind::BIN_EXPR.into());
    }

    #[test]
    fn marker_dropped_without_complete_does_not_panic() {
        // After the fuzz-crash fix, dropping an uncompleted Marker
        // must NOT panic (it prints a debug warning instead).  This
        // prevents double-panic SIGABRT during unwind.
        let tokens = vec![];
        let spans = vec![];
        let mut p = Parser::new(tokens, spans);
        let _m = p.open(); // dropped without complete/abandon -- must not panic
    }

    #[test]
    fn fuel_exhaustion_forces_eof() {
        // Create 300 identical tokens (more than the 256 fuel limit).
        // A loop that only calls p.at(SOME_OTHER_KIND) without advancing
        // must hit fuel exhaustion and gracefully enter EOF state.
        let n = 300;
        let tokens: Vec<LexedToken> = (0..n)
            .map(|_| LexedToken {
                kind: SyntaxKind::IDENT,
                text: "x".to_string(),
            })
            .collect();
        let spans: Vec<TokenSpan> = (0..n)
            .map(|i| TokenSpan {
                start: i,
                end: i + 1,
            })
            .collect();
        let mut p = Parser::new(tokens, spans);

        // Simulate a stuck loop: keep calling at() for a non-matching
        // kind without ever bumping. Fuel should run out after 256 calls.
        let m = p.open();
        let mut iterations = 0;
        while !p.eof() {
            // at() decrements fuel; since PLUS never matches IDENT tokens,
            // we never bump. After 256 calls, fuel hits 0, at() forces EOF.
            p.at(SyntaxKind::PLUS);
            iterations += 1;
            if iterations > 500 {
                panic!("loop did not terminate via fuel exhaustion");
            }
        }
        m.complete(&mut p, SyntaxKind::SOURCE_FILE);

        // Verify: parser is at EOF
        assert!(p.eof());
        // Verify: a "fuel exhausted" error was emitted
        assert!(
            p.errors
                .iter()
                .any(|e| e.message.contains("fuel exhausted")),
            "expected fuel exhaustion error, got: {:?}",
            p.errors
        );
    }

    #[test]
    fn deeply_nested_parens_do_not_crash() {
        // 200 levels of nested parentheses should hit the depth limit
        // (128) and produce an error instead of a stack overflow.
        let input = format!(
            "contract T {{ input(x: Int) requires {{ {}x > 0{} }} }}",
            "(".repeat(200),
            ")".repeat(200),
        );
        let (_file, errors) = crate::parse(&input);
        assert!(
            !errors.is_empty(),
            "deeply nested parens should produce parse errors"
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("nesting depth limit")),
            "expected nesting depth limit error, got: {errors:?}",
        );
    }

    #[test]
    fn long_operator_chain_does_not_crash() {
        // 500 chained && operators should hit the chain limit (256)
        // and produce an error instead of a stack overflow.
        let chain = "true && ".repeat(500);
        let input = format!("contract T {{ requires {{ {chain}true }} }}");
        let (_file, errors) = crate::parse(&input);
        assert!(
            !errors.is_empty(),
            "500 chained operators should produce parse errors"
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("operator chain too long")),
            "expected operator chain limit error, got: {errors:?}",
        );
    }
}
