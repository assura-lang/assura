//! Unified diagnostic types for the Assura compiler.
//!
//! All compiler passes (parser, resolver, type checker, SMT verifier)
//! emit `Diagnostic` values. The CLI renders these uniformly via
//! ariadne (human mode) or serde (JSON mode).

use std::ops::Range;

mod catalog;
mod render;

pub use catalog::{error_catalog, explain};
pub use render::{render_diagnostic, report_diagnostics_human};

/// Source location span (byte offsets into the source file).
pub type Span = Range<usize>;

/// A strongly-typed error code from the Assura specification.
///
/// Wraps the raw code string (e.g. `"A03001"`) so that error code
/// fields are distinguishable from arbitrary strings at the type level.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct ErrorCode(String);

impl ErrorCode {
    /// Return the code as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ErrorCode {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ErrorCode {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for ErrorCode {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl PartialEq<str> for ErrorCode {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for ErrorCode {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<String> for ErrorCode {
    fn eq(&self, other: &String) -> bool {
        self.0 == *other
    }
}

/// Diagnostic severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational message, not an error.
    Info,
    /// Potential problem that does not prevent compilation.
    Warning,
    /// Error that prevents compilation or verification.
    Error,
}

/// A secondary span with a label, used for additional context in diagnostics.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SecondaryLabel {
    /// The source span for this secondary label.
    pub span: Span,
    /// A description of what this secondary location refers to.
    pub message: String,
}

/// A suggested fix for a diagnostic.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
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
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Diagnostic {
    /// Error code from the spec (e.g., "A01001", "A03005").
    pub code: ErrorCode,
    /// Severity level.
    pub severity: Severity,
    /// Human-readable error message.
    pub message: String,
    /// Source file name (may be empty for in-memory compilations).
    pub file: String,
    /// Primary source location where the error was detected.
    pub primary: Span,
    /// Secondary spans with labels (e.g., "expected type declared here").
    pub secondary: Vec<SecondaryLabel>,
    /// Optional suggested fix.
    pub suggestion: Option<Suggestion>,
}

impl Diagnostic {
    /// Create a new error diagnostic with a code, message, and span.
    pub fn error(code: impl Into<ErrorCode>, message: impl Into<String>, span: Span) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Error,
            message: message.into(),
            file: String::new(),
            primary: span,
            secondary: Vec::new(),
            suggestion: None,
        }
    }

    /// Create a new warning diagnostic.
    pub fn warning(code: impl Into<ErrorCode>, message: impl Into<String>, span: Span) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Warning,
            message: message.into(),
            file: String::new(),
            primary: span,
            secondary: Vec::new(),
            suggestion: None,
        }
    }

    /// Set the source file name for this diagnostic.
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = file.into();
        self
    }

    /// Add a secondary span with a label.
    pub fn with_secondary(mut self, span: Span, label: impl Into<String>) -> Self {
        self.secondary.push(SecondaryLabel {
            span,
            message: label.into(),
        });
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

/// A human-readable explanation of a specific error code.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorInfo {
    /// The error code (e.g. "A01001").
    pub code: &'static str,
    /// Short descriptive name.
    pub name: &'static str,
    /// Multi-line explanation of the error.
    pub description: &'static str,
    /// Example source code that triggers the error.
    pub example: &'static str,
    /// How to fix the error.
    pub fix: &'static str,
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
        assert_eq!(d.secondary[0].message, "declared here");
    }

    #[test]
    fn diagnostic_with_suggestion() {
        let d = Diagnostic::error("A01001", "unexpected token", 5..8).with_suggestion(
            "try adding a semicolon",
            7..8,
            ";",
        );
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

    #[test]
    fn test_error_diagnostic_is_error() {
        let d = Diagnostic::error("A01001", "syntax error", 0..5);
        assert!(d.is_error());
        assert_eq!(d.severity, Severity::Error);
    }

    #[test]
    fn test_warning_diagnostic_is_not_error() {
        let d = Diagnostic::warning("A02007", "unused import", 10..20);
        assert!(!d.is_error());
        assert_eq!(d.severity, Severity::Warning);
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(format!("{}", Severity::Info), "info");
        assert_eq!(format!("{}", Severity::Warning), "warning");
        assert_eq!(format!("{}", Severity::Error), "error");
    }

    #[test]
    fn test_diagnostic_with_file() {
        let d = Diagnostic::error("A03001", "type mismatch", 0..10).with_file("test.assura");
        assert_eq!(d.file, "test.assura");
    }

    #[test]
    fn test_diagnostic_multiple_secondary_spans() {
        let d = Diagnostic::error("A03001", "type mismatch", 10..20)
            .with_secondary(30..40, "expected type here")
            .with_secondary(50..60, "found type here");
        assert_eq!(d.secondary.len(), 2);
        assert_eq!(d.secondary[0].message, "expected type here");
        assert_eq!(d.secondary[0].span, 30..40);
        assert_eq!(d.secondary[1].message, "found type here");
        assert_eq!(d.secondary[1].span, 50..60);
    }

    #[test]
    fn test_diagnostic_suggestion_fields() {
        let d = Diagnostic::error("A01002", "unexpected token", 5..8).with_suggestion(
            "add a colon",
            7..8,
            ":",
        );
        let s = d.suggestion.as_ref().unwrap();
        assert_eq!(s.message, "add a colon");
        assert_eq!(s.span, 7..8);
        assert_eq!(s.replacement, ":");
    }

    #[test]
    fn test_diagnostic_json_serialization() {
        let d = Diagnostic::error("A03001", "type mismatch", 10..20)
            .with_file("main.assura")
            .with_secondary(30..40, "declared here");
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("A03001"));
        assert!(json.contains("type mismatch"));
        assert!(json.contains("main.assura"));
        assert!(json.contains("declared here"));
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["code"], "A03001");
        assert_eq!(val["severity"], "error");
        assert_eq!(val["message"], "type mismatch");
    }

    #[test]
    fn test_diagnostic_collection() {
        let diags = vec![
            Diagnostic::error("A01001", "unexpected char", 0..1),
            Diagnostic::warning("A02007", "unused import", 10..20),
            Diagnostic::error("A03001", "type mismatch", 30..40),
        ];
        assert_eq!(diags.len(), 3);
        let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
        assert_eq!(errors.len(), 2);
        let warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn test_diagnostic_empty_secondary_spans() {
        let d = Diagnostic::error("A03001", "error", 0..5);
        assert!(d.secondary.is_empty());
        assert!(d.suggestion.is_none());
    }

    #[test]
    fn test_error_code_formatting_display() {
        let d = Diagnostic::error("A05001", "linear variable used twice", 0..10);
        let display = format!("{d}");
        assert_eq!(display, "[A05001] linear variable used twice");
    }

    #[test]
    fn test_error_catalog_not_empty() {
        let catalog = error_catalog();
        assert!(!catalog.is_empty());
        for entry in &catalog {
            assert!(!entry.code.is_empty());
            assert!(!entry.name.is_empty());
            assert!(!entry.description.is_empty());
            assert!(!entry.example.is_empty());
            assert!(!entry.fix.is_empty());
        }
    }

    #[test]
    fn test_explain_known_code() {
        let info = explain("A01001");
        let info = info.unwrap();
        assert_eq!(info.code, "A01001");
        assert_eq!(info.name, "Unexpected character");
    }

    #[test]
    fn test_explain_unknown_code() {
        let info = explain("A00000");
        assert!(info.is_none());
    }

    #[test]
    fn test_explain_all_catalog_codes() {
        let catalog = error_catalog();
        for entry in &catalog {
            let found = explain(entry.code);
            assert!(found.is_some(), "should find {}", entry.code);
            assert_eq!(found.unwrap().code, entry.code);
        }
    }

    #[test]
    fn test_warning_serialization() {
        let d = Diagnostic::warning("A02007", "unused import", 5..15);
        let json = serde_json::to_string(&d).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["severity"], "warning");
    }

    #[test]
    fn test_suggestion_serialization() {
        let s = Suggestion {
            message: "add semicolon".to_string(),
            span: 10..11,
            replacement: ";".to_string(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("add semicolon"));
    }

    #[test]
    fn test_secondary_label_equality() {
        let a = SecondaryLabel {
            span: 0..5,
            message: "here".to_string(),
        };
        let b = SecondaryLabel {
            span: 0..5,
            message: "here".to_string(),
        };
        assert_eq!(a, b);
    }

    /// Every error code in the catalog must be unique.
    #[test]
    fn test_no_duplicate_error_codes() {
        let catalog = error_catalog();
        let mut seen = std::collections::HashSet::new();
        for entry in &catalog {
            assert!(
                seen.insert(entry.code),
                "duplicate error code in catalog: {}",
                entry.code
            );
        }
    }

    /// Regression test for #179: error codes must match spec Section 7.2.
    #[test]
    fn test_spec_aligned_error_codes() {
        // A02002 = "Undefined type" per spec (was "Ambiguous name")
        let a02002 = explain("A02002").expect("A02002 should exist");
        assert_eq!(a02002.name, "Undefined type");

        // A02004 = "Ambiguous import" per spec (was "Visibility violation")
        let a02004 = explain("A02004").expect("A02004 should exist");
        assert_eq!(a02004.name, "Ambiguous import");

        // A19001 must exist (audit trail, was entirely missing)
        assert!(explain("A19001").is_some(), "A19001 should exist");

        // A26002 must exist (i18n completeness, was missing)
        assert!(explain("A26002").is_some(), "A26002 should exist");
    }

    #[test]
    fn test_render_diagnostic_does_not_panic() {
        // Ensure render_diagnostic does not panic on valid and edge-case inputs
        let d = Diagnostic::error("A01001", "unexpected char", 0..1);
        render_diagnostic(&d, "test.assura", "x");

        let d = Diagnostic::warning("A02007", "unused import", 0..5)
            .with_secondary(6..10, "imported here");
        render_diagnostic(&d, "test.assura", "import std.math;");
    }

    #[test]
    fn test_report_diagnostics_human_multiple() {
        let diags = vec![
            Diagnostic::error("A01001", "bad char", 0..1),
            Diagnostic::warning("A02007", "unused", 2..5),
        ];
        // Must not panic
        report_diagnostics_human(&diags, "multi.assura", "x = 42;");
    }

    #[test]
    fn test_error_code_as_str() {
        let code = ErrorCode::from("A03001");
        assert_eq!(code.as_str(), "A03001");
    }

    #[test]
    fn test_error_code_from_string() {
        let code = ErrorCode::from(String::from("A05001"));
        assert_eq!(code, "A05001");
    }

    #[test]
    fn test_error_code_partial_eq_str() {
        let code = ErrorCode::from("A07003");
        assert!(code == "A07003");
        assert!(code == *"A07003");
    }

    #[test]
    fn test_error_code_as_ref() {
        let code = ErrorCode::from("A01002");
        let s: &str = code.as_ref();
        assert_eq!(s, "A01002");
    }

    #[test]
    fn test_error_code_display() {
        let code = ErrorCode::from("A03005");
        assert_eq!(format!("{code}"), "A03005");
    }

    #[test]
    fn test_error_code_ordering() {
        let a = ErrorCode::from("A01001");
        let b = ErrorCode::from("A03001");
        assert!(a < b);
    }

    #[test]
    fn test_error_catalog_entries_have_fields() {
        let catalog = error_catalog();
        for entry in &catalog {
            assert!(!entry.code.is_empty(), "code must not be empty");
            assert!(
                !entry.name.is_empty(),
                "name must not be empty for {}",
                entry.code
            );
            assert!(
                !entry.description.is_empty(),
                "description must not be empty for {}",
                entry.code
            );
            assert!(
                !entry.fix.is_empty(),
                "fix must not be empty for {}",
                entry.code
            );
        }
    }

    #[test]
    fn test_diagnostic_chaining() {
        let d = Diagnostic::error("A03001", "mismatch", 10..20)
            .with_file("test.assura")
            .with_secondary(30..40, "defined here")
            .with_suggestion("use Int", 10..20, "Int");
        assert_eq!(d.file, "test.assura");
        assert_eq!(d.secondary.len(), 1);
        d.suggestion.unwrap();
    }

    #[test]
    fn test_severity_serde() {
        let json = serde_json::to_string(&Severity::Error).unwrap();
        assert_eq!(json, "\"error\"");
        let json = serde_json::to_string(&Severity::Warning).unwrap();
        assert_eq!(json, "\"warning\"");
        let json = serde_json::to_string(&Severity::Info).unwrap();
        assert_eq!(json, "\"info\"");
    }

    // ---- ErrorCode edge cases ----

    #[test]
    fn test_error_code_eq_string_owned() {
        let code = ErrorCode::from("A03001");
        assert!(code == String::from("A03001"));
    }

    #[test]
    fn test_error_code_ne() {
        let a = ErrorCode::from("A01001");
        let b = ErrorCode::from("A03001");
        assert_ne!(a, b);
    }

    #[test]
    fn test_error_code_clone_eq() {
        let code = ErrorCode::from("A05001");
        let cloned = code.clone();
        assert_eq!(code, cloned);
    }

    #[test]
    fn test_error_code_hash_consistent() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ErrorCode::from("A01001"));
        set.insert(ErrorCode::from("A01001")); // duplicate
        set.insert(ErrorCode::from("A03001"));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_error_code_empty() {
        let code = ErrorCode::from("");
        assert_eq!(code.as_str(), "");
        assert_eq!(format!("{code}"), "");
    }

    // ---- Catalog validation ----

    #[test]
    fn test_error_catalog_all_codes_valid_format() {
        let catalog = error_catalog();
        for entry in &catalog {
            assert_eq!(
                entry.code.len(),
                6,
                "error code '{}' should be 6 chars (Axxxxx)",
                entry.code
            );
            assert!(
                entry.code.starts_with('A'),
                "error code '{}' should start with 'A'",
                entry.code
            );
            assert!(
                entry.code[1..].chars().all(|c| c.is_ascii_digit()),
                "error code '{}' should have 5 digits after 'A'",
                entry.code
            );
        }
    }

    #[test]
    fn test_error_catalog_has_major_categories() {
        let catalog = error_catalog();
        let codes: Vec<&str> = catalog.iter().map(|e| e.code).collect();
        // Must have at least one code in each major category
        assert!(
            codes.iter().any(|c| c.starts_with("A01")),
            "missing A01xxx (syntax)"
        );
        assert!(
            codes.iter().any(|c| c.starts_with("A02")),
            "missing A02xxx (resolve)"
        );
        assert!(
            codes.iter().any(|c| c.starts_with("A03")),
            "missing A03xxx (type)"
        );
        assert!(
            codes.iter().any(|c| c.starts_with("A05")),
            "missing A05xxx (linear)"
        );
        assert!(
            codes.iter().any(|c| c.starts_with("A07")),
            "missing A07xxx (effect)"
        );
    }

    #[test]
    fn test_error_catalog_size_reasonable() {
        let catalog = error_catalog();
        assert!(
            catalog.len() >= 200,
            "catalog should have 200+ entries, got {}",
            catalog.len()
        );
    }

    #[test]
    fn test_explain_empty_string() {
        assert!(explain("").is_none());
    }

    #[test]
    fn test_explain_partial_code() {
        assert!(explain("A01").is_none());
        assert!(explain("A").is_none());
    }

    #[test]
    fn test_explain_nonexistent_category() {
        assert!(explain("A88888").is_none());
    }

    // ---- Diagnostic edge cases ----

    #[test]
    fn test_diagnostic_zero_length_span() {
        let d = Diagnostic::error("A01001", "at position", 5..5);
        assert_eq!(d.primary, 5..5);
        assert!(d.primary.is_empty());
    }

    #[test]
    fn test_diagnostic_large_span() {
        let d = Diagnostic::error("A01001", "whole file", 0..100_000);
        assert_eq!(d.primary, 0..100_000);
    }

    #[test]
    fn test_diagnostic_empty_message() {
        let d = Diagnostic::error("A01001", "", 0..1);
        assert_eq!(d.message, "");
        assert_eq!(format!("{d}"), "[A01001] ");
    }

    #[test]
    fn test_diagnostic_default_file_empty() {
        let d = Diagnostic::error("A01001", "err", 0..1);
        assert!(d.file.is_empty());
    }

    #[test]
    fn test_diagnostic_with_file_overwrites() {
        let d = Diagnostic::error("A01001", "err", 0..1)
            .with_file("first.assura")
            .with_file("second.assura");
        assert_eq!(d.file, "second.assura");
    }

    #[test]
    fn test_render_diagnostic_with_suggestion() {
        let d = Diagnostic::error("A01002", "missing colon", 8..9).with_suggestion(
            "add colon",
            8..9,
            ":",
        );
        // Must not panic
        render_diagnostic(&d, "test.assura", "requires x > 0");
    }

    #[test]
    fn test_report_diagnostics_human_empty() {
        // Empty list should not panic
        report_diagnostics_human(&[], "empty.assura", "");
    }

    #[test]
    fn test_render_diagnostic_info_severity() {
        let d = Diagnostic {
            code: ErrorCode::from("A99999"),
            severity: Severity::Info,
            message: "informational".into(),
            file: String::new(),
            primary: 0..1,
            secondary: Vec::new(),
            suggestion: None,
        };
        // Must not panic
        render_diagnostic(&d, "test.assura", "x");
    }

    // ---- Severity edge cases ----

    #[test]
    fn test_severity_equality() {
        assert_eq!(Severity::Error, Severity::Error);
        assert_ne!(Severity::Error, Severity::Warning);
        assert_ne!(Severity::Warning, Severity::Info);
    }

    #[test]
    fn test_severity_copy() {
        let s = Severity::Error;
        let s2 = s; // Copy
        assert_eq!(s, s2);
    }

    // ---- SecondaryLabel ----

    #[test]
    fn test_secondary_label_inequality() {
        let a = SecondaryLabel {
            span: 0..5,
            message: "here".to_string(),
        };
        let b = SecondaryLabel {
            span: 0..5,
            message: "there".to_string(),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn test_secondary_label_serialization() {
        let label = SecondaryLabel {
            span: 10..20,
            message: "declared here".to_string(),
        };
        let json = serde_json::to_string(&label).unwrap();
        assert!(json.contains("declared here"));
        assert!(json.contains("10"));
    }

    // ---- Suggestion ----

    #[test]
    fn test_suggestion_equality() {
        let a = Suggestion {
            message: "fix".into(),
            span: 0..1,
            replacement: ";".into(),
        };
        let b = Suggestion {
            message: "fix".into(),
            span: 0..1,
            replacement: ";".into(),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn test_suggestion_inequality() {
        let a = Suggestion {
            message: "fix".into(),
            span: 0..1,
            replacement: ";".into(),
        };
        let b = Suggestion {
            message: "fix".into(),
            span: 0..1,
            replacement: ":".into(),
        };
        assert_ne!(a, b);
    }

    // ---- Full diagnostic JSON roundtrip ----

    #[test]
    fn test_diagnostic_full_json_structure() {
        let d = Diagnostic::error("A03001", "type mismatch", 10..20)
            .with_file("test.assura")
            .with_secondary(30..40, "expected here")
            .with_secondary(50..60, "found here")
            .with_suggestion("change type", 10..20, "Int");
        let json = serde_json::to_string_pretty(&d).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        // Check top-level fields
        assert_eq!(val["code"], "A03001");
        assert_eq!(val["severity"], "error");
        assert_eq!(val["file"], "test.assura");
        // Check secondary array
        assert!(val["secondary"].is_array());
        assert_eq!(val["secondary"].as_array().unwrap().len(), 2);
        // Check suggestion
        assert!(val["suggestion"].is_object());
        assert_eq!(val["suggestion"]["replacement"], "Int");
    }

    #[test]
    fn test_diagnostic_json_no_suggestion() {
        let d = Diagnostic::warning("A02007", "unused", 0..5);
        let json = serde_json::to_string(&d).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(val["suggestion"].is_null());
    }

    // ---- ErrorInfo ----

    #[test]
    fn test_error_info_equality() {
        let a = ErrorInfo {
            code: "A01001",
            name: "Unexpected character",
            description: "desc",
            example: "ex",
            fix: "fix",
        };
        let b = ErrorInfo {
            code: "A01001",
            name: "Unexpected character",
            description: "desc",
            example: "ex",
            fix: "fix",
        };
        assert_eq!(a, b);
    }

    #[test]
    fn test_error_info_clone() {
        let a = ErrorInfo {
            code: "A01001",
            name: "test",
            description: "desc",
            example: "ex",
            fix: "fix",
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    // ---- Catalog code lookup coverage ----

    #[test]
    fn test_explain_returns_same_as_catalog_entry() {
        let catalog = error_catalog();
        // Spot-check several specific codes
        for code in &["A01001", "A02001", "A03001", "A05001", "A07003", "A10001"] {
            let from_explain = explain(code).expect(&format!("{code} should exist"));
            let from_catalog = catalog
                .iter()
                .find(|e| e.code == *code)
                .expect("in catalog");
            assert_eq!(from_explain.name, from_catalog.name);
            assert_eq!(from_explain.description, from_catalog.description);
        }
    }
}
