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
const TOMBSTONE: SyntaxKind = SyntaxKind::ERROR;

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

    /// Abandon this marker without producing a node.
    pub(crate) fn abandon(mut self, p: &mut Parser) {
        self.completed = true;
        if self.pos as usize == p.events.len() - 1 {
            match p.events.pop() {
                Some(Event::Open { .. }) => {}
                _ => unreachable!(),
            }
        }
        // If not the last event, leave the tombstone; build_tree skips it.
    }
}

impl Drop for Marker {
    fn drop(&mut self) {
        if !self.completed {
            panic!("Marker dropped without complete() or abandon()");
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
pub(crate) struct LexedToken {
    pub kind: SyntaxKind,
    pub text: String,
}

/// The recursive-descent parser. Holds the token stream, current
/// position, event buffer, collected errors, and a fuel counter
/// to prevent infinite loops in error recovery.
pub(crate) struct Parser {
    pub(crate) tokens: Vec<LexedToken>,
    pos: usize,
    pub(crate) events: Vec<Event>,
    fuel: u32,
    pub(crate) errors: Vec<ParseError>,
}

/// A parse error with location and message.
#[derive(Debug, Clone)]
pub struct ParseError {
    /// Byte offset range in the source where the error occurred.
    pub span: std::ops::Range<usize>,
    /// Human-readable error message.
    pub message: String,
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
pub(crate) struct TokenSpan {
    pub start: usize,
    pub end: usize,
}

impl Parser {
    /// Create a new parser from a sequence of lexed tokens with spans.
    pub(crate) fn new(tokens: Vec<LexedToken>, spans: Vec<TokenSpan>) -> Self {
        assert_eq!(tokens.len(), spans.len());
        Self {
            tokens,
            pos: 0,
            events: Vec::new(),
            fuel: 256,
            errors: Vec::new(),
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

    /// Consume the current token and advance.
    pub(crate) fn bump(&mut self) {
        assert!(!self.eof());
        self.fuel = 256;
        self.events.push(Event::Advance);
        self.pos += 1;
    }

    /// The `SyntaxKind` of the current token, or `ERROR_TOKEN` at EOF.
    pub(crate) fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    /// Lookahead: the kind of the token `n` positions ahead.
    pub(crate) fn nth(&self, n: usize) -> SyntaxKind {
        self.tokens
            .get(self.pos + n)
            .map(|t| t.kind)
            .unwrap_or(SyntaxKind::ERROR_TOKEN)
    }

    /// True if the current token matches `kind`.
    pub(crate) fn at(&mut self, kind: SyntaxKind) -> bool {
        if self.fuel == 0 {
            panic!(
                "parser stuck: infinite loop detected at token {:?}",
                self.current()
            );
        }
        self.fuel -= 1;
        self.current() == kind
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

    /// True if we've consumed all tokens.
    pub(crate) fn eof(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    /// Emit an error at the current token's location.
    pub(crate) fn error_at_current(&mut self, message: String) {
        let span = self.current_span();
        self.errors.push(ParseError {
            span: span.start..span.end,
            message,
        });
    }

    /// Emit an error with a specific span.
    pub(crate) fn error(&mut self, message: String, span: std::ops::Range<usize>) {
        self.errors.push(ParseError { span, message });
    }

    /// The text of the current token.
    pub(crate) fn current_text(&self) -> &str {
        self.tokens
            .get(self.pos)
            .map(|t| t.text.as_str())
            .unwrap_or("")
    }

    /// The source span of the current token.
    fn current_span(&self) -> TokenSpan {
        // Spans are stored separately; we need access to them.
        // The spans vec is kept in the outer parse driver.
        // For now, provide a synthetic span at position.
        TokenSpan {
            start: self.pos,
            end: self.pos + 1,
        }
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
                }
            }
            Event::Close => {
                builder.finish_node();
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
    #[should_panic(expected = "Marker dropped without complete")]
    fn marker_must_be_completed() {
        let tokens = vec![];
        let spans = vec![];
        let mut p = Parser::new(tokens, spans);
        let _m = p.open(); // dropped without complete/abandon
    }
}
