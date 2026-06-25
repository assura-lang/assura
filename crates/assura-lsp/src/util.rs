//! Span/position conversion utilities for the LSP server.

use ropey::Rope;
use tower_lsp::lsp_types::{Position, Range};

/// Convert a byte span (Range<usize>) to an LSP Range (line/col).
pub(crate) fn byte_span_to_range(rope: &Rope, span: &std::ops::Range<usize>) -> Range {
    let start = byte_to_position(rope, span.start);
    let end = byte_to_position(rope, span.end);
    Range { start, end }
}

/// Convert a byte offset to an LSP Position (0-based line and character).
pub(crate) fn byte_to_position(rope: &Rope, byte_offset: usize) -> Position {
    let clamped = byte_offset.min(rope.len_bytes());
    let line = rope.byte_to_line(clamped);
    let line_start = rope.line_to_byte(line);
    let col = clamped - line_start;
    Position::new(line as u32, col as u32)
}

/// Convert an LSP Position to a byte offset.
pub(crate) fn position_to_offset(rope: &Rope, pos: Position) -> usize {
    let line = (pos.line as usize).min(rope.len_lines().saturating_sub(1));
    let line_start = rope.line_to_byte(line);
    let line_len = rope.line(line).len_bytes();
    let col = (pos.character as usize).min(line_len);
    line_start + col
}

/// Extract the word (identifier) at a given byte offset.
pub(crate) fn word_at_offset(source: &str, offset: usize) -> Option<String> {
    if offset > source.len() {
        return None;
    }
    let bytes = source.as_bytes();

    // Find start of word
    let mut start = offset;
    while start > 0 && is_ident_char(bytes[start - 1]) {
        start -= 1;
    }

    // Find end of word
    let mut end = offset;
    while end < bytes.len() && is_ident_char(bytes[end]) {
        end += 1;
    }

    if start == end {
        return None;
    }

    Some(source[start..end].to_string())
}

pub(crate) fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
