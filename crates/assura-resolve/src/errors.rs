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
        let error_span = e.span.clone();
        let mut d = assura_diagnostics::Diagnostic::error(e.code, e.message, e.span);
        if let Some((span, label)) = e.secondary {
            d.secondary.push(assura_diagnostics::SecondaryLabel {
                span,
                message: label,
            });
        }
        if let Some(hint) = e.suggestion {
            d = d.with_suggestion(hint, error_span, String::new());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_error_to_diagnostic() {
        let err = ResolutionError {
            code: "A02001".into(),
            message: "undefined name `x`".into(),
            span: 0..5,
            secondary: None,
            suggestion: Some("did you mean `y`?".into()),
        };
        let diag: assura_diagnostics::Diagnostic = err.into();
        assert_eq!(diag.code, "A02001");
        assert!(diag.message.contains("undefined name `x`"));
    }

    #[test]
    fn resolution_error_with_secondary() {
        let err = ResolutionError {
            code: "A02002".into(),
            message: "duplicate name `x`".into(),
            span: 10..15,
            secondary: Some((0..5, "previously defined here".into())),
            suggestion: None,
        };
        let diag: assura_diagnostics::Diagnostic = err.into();
        assert_eq!(diag.secondary.len(), 1);
        assert!(diag.secondary[0].message.contains("previously defined"));
    }
}
