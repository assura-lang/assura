//! Resolution error types and the resolved file result.

use assura_parser::ast::{SourceFile, Span};

use crate::imports::ResolvedImport;
use crate::symbols::SymbolTable;

/// An error produced during name resolution.
#[derive(Debug, Clone)]
pub struct ResolutionError {
    pub code: assura_diagnostics::ErrorCode,
    pub message: String,
    pub span: Span,
    /// Optional secondary span (e.g., previous definition site).
    pub secondary: Option<(Span, String)>,
    /// Optional "did you mean?" suggestion.
    pub suggestion: Option<String>,
}

impl From<ResolutionError> for assura_diagnostics::Diagnostic {
    fn from(e: ResolutionError) -> Self {
        let mut d = assura_diagnostics::Diagnostic::error(e.code, e.message, e.span);
        if let Some((span, label)) = e.secondary {
            d.secondary.push(assura_diagnostics::SecondaryLabel {
                span,
                message: label,
            });
        }
        if let Some(hint) = e.suggestion {
            d = d.with_suggestion(hint, 0..0, String::new());
        }
        d
    }
}

/// The result of successful name resolution: the original AST plus the
/// symbol table and resolved imports.
#[derive(Debug, Clone)]
pub struct ResolvedFile {
    pub source: SourceFile,
    pub symbols: SymbolTable,
    /// All import declarations with their resolution status.
    pub imports: Vec<ResolvedImport>,
    /// Non-fatal warnings (e.g., unused imports). These don't prevent
    /// resolution from succeeding.
    pub warnings: Vec<ResolutionError>,
}
