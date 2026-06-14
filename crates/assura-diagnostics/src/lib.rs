//! Unified diagnostic types for the Assura compiler.
//!
//! All compiler passes (parser, resolver, type checker, SMT verifier)
//! emit `Diagnostic` values. The CLI renders these uniformly via
//! ariadne (human mode) or serde (JSON mode).

use std::ops::Range;

/// Source location span (byte offsets into the source file).
pub type Span = Range<usize>;

/// Diagnostic severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Informational message, not an error.
    Info,
    /// Potential problem that does not prevent compilation.
    Warning,
    /// Error that prevents compilation or verification.
    Error,
}

/// A suggested fix for a diagnostic.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    /// Human-readable description of what the fix does.
    pub message: String,
    /// The span to replace.
    pub span: Span,
    /// The replacement text.
    pub replacement: String,
}

/// A compiler diagnostic with structured location and severity.
///
/// This is the unified error type emitted by all compiler passes.
/// The CLI consumes `Vec<Diagnostic>` and renders them via ariadne
/// (for human-readable output) or serializes them (for JSON output).
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    /// Error code from the spec (e.g., "A01001", "A03005").
    pub code: String,
    /// Severity level.
    pub severity: Severity,
    /// Human-readable error message.
    pub message: String,
    /// Primary source location where the error was detected.
    pub primary: Span,
    /// Secondary spans with labels (e.g., "expected type declared here").
    pub secondary: Vec<(Span, String)>,
    /// Optional suggested fix.
    pub suggestion: Option<Suggestion>,
}

impl Diagnostic {
    /// Create a new error diagnostic with a code, message, and span.
    pub fn error(code: impl Into<String>, message: impl Into<String>, span: Span) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Error,
            message: message.into(),
            primary: span,
            secondary: Vec::new(),
            suggestion: None,
        }
    }

    /// Create a new warning diagnostic.
    pub fn warning(code: impl Into<String>, message: impl Into<String>, span: Span) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Warning,
            message: message.into(),
            primary: span,
            secondary: Vec::new(),
            suggestion: None,
        }
    }

    /// Add a secondary span with a label.
    pub fn with_secondary(mut self, span: Span, label: impl Into<String>) -> Self {
        self.secondary.push((span, label.into()));
        self
    }

    /// Add a suggested fix.
    pub fn with_suggestion(
        mut self,
        message: impl Into<String>,
        span: Span,
        replacement: impl Into<String>,
    ) -> Self {
        self.suggestion = Some(Suggestion {
            message: message.into(),
            span,
            replacement: replacement.into(),
        });
        self
    }

    /// Check if this diagnostic is an error.
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Info => write!(f, "info"),
            Severity::Warning => write!(f, "warning"),
            Severity::Error => write!(f, "error"),
        }
    }
}

// ---------------------------------------------------------------------------
// Conversions from crate-specific error types
// ---------------------------------------------------------------------------

/// Trait for types that can be converted into diagnostics.
pub trait IntoDiagnostic {
    /// Convert into a `Diagnostic`.
    fn into_diagnostic(self) -> Diagnostic;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_diagnostic_creation() {
        let d = Diagnostic::error("A03001", "type mismatch", 10..20);
        assert_eq!(d.code, "A03001");
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.primary, 10..20);
        assert!(d.is_error());
    }

    #[test]
    fn warning_diagnostic_creation() {
        let d = Diagnostic::warning("A05001", "unused variable", 5..10);
        assert_eq!(d.severity, Severity::Warning);
        assert!(!d.is_error());
    }

    #[test]
    fn diagnostic_with_secondary() {
        let d = Diagnostic::error("A03002", "expected Int", 10..20)
            .with_secondary(30..40, "declared here");
        assert_eq!(d.secondary.len(), 1);
        assert_eq!(d.secondary[0].1, "declared here");
    }

    #[test]
    fn diagnostic_with_suggestion() {
        let d = Diagnostic::error("A01001", "unexpected token", 5..8).with_suggestion(
            "try adding a semicolon",
            7..8,
            ";",
        );
        assert!(d.suggestion.is_some());
        let s = d.suggestion.unwrap();
        assert_eq!(s.replacement, ";");
    }

    #[test]
    fn diagnostic_display() {
        let d = Diagnostic::error("A03001", "type mismatch", 0..1);
        assert_eq!(format!("{d}"), "[A03001] type mismatch");
    }

    #[test]
    fn severity_ordering() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
    }
}
