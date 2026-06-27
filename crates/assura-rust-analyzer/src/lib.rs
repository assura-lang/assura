//! Parse inline contract annotations (`/// @requires`, `/// @ensures`, etc.)
//! from Rust source files.
//!
//! This crate extracts Assura contract clauses embedded in Rust doc comments
//! and maps them to the functions, structs, and impl blocks they annotate.

mod merge;
mod parse;
mod python;
mod types;

pub use merge::{ClauseSource, MergedContract, SourcedClause, merge_contracts};
pub use parse::{parse_doc_clauses, parse_rust_file, parse_rust_source, scan_directory};
pub use python::PythonAdapter;
pub use types::{
    AnnotatedItem, AnnotatedItemKind, ContractClause, FieldInfo, InlineClauseKind, InlineContract,
    ParamInfo,
};

/// Errors produced by the `assura-rust-analyzer` crate.
#[derive(Debug, thiserror::Error)]
pub enum RustAnalyzerError {
    /// A Rust/Python source file could not be parsed.
    #[error("parse error: {0}")]
    Parse(String),
    /// An I/O operation (reading a file or scanning a directory) failed.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Catch-all for other error conditions.
    #[error("{0}")]
    Other(String),
}

// ---------------------------------------------------------------------------
// Multi-language annotation framework
// ---------------------------------------------------------------------------

/// Trait for language-specific annotation parsing.
///
/// Each language adapter knows how to extract contract annotations from
/// its source format. The clause kinds (`@requires`, `@ensures`, etc.)
/// are universal; only the comment syntax and type mapping differ.
pub trait LanguageAdapter {
    /// Language identifier (e.g., "rust", "python", "go").
    fn language_id(&self) -> &str;

    /// File extensions handled by this adapter (e.g., `["rs"]`).
    fn file_extensions(&self) -> &[&str];

    /// Extract annotated items from source text.
    fn parse_source(&self, source: &str) -> Result<Vec<AnnotatedItem>, RustAnalyzerError>;

    /// Map a language-specific type name to an Assura type.
    /// Returns `None` if the type has no Assura equivalent.
    fn map_type(&self, language_type: &str) -> Option<String>;
}

/// Rust language adapter (delegates to existing `parse_rust_source`).
pub struct RustAdapter;

impl LanguageAdapter for RustAdapter {
    fn language_id(&self) -> &str {
        "rust"
    }

    fn file_extensions(&self) -> &[&str] {
        &["rs"]
    }

    fn parse_source(&self, source: &str) -> Result<Vec<AnnotatedItem>, RustAnalyzerError> {
        parse_rust_source(source)
    }

    fn map_type(&self, language_type: &str) -> Option<String> {
        match language_type {
            "i8" | "i16" | "i32" | "i64" | "i128" | "isize" => Some("Int".to_string()),
            "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => Some("Nat".to_string()),
            "f32" | "f64" => Some("Float".to_string()),
            "bool" => Some("Bool".to_string()),
            "String" | "&str" => Some("String".to_string()),
            "()" => Some("Unit".to_string()),
            _ => None,
        }
    }
}

/// Get the appropriate language adapter for a file extension.
pub fn adapter_for_extension(ext: &str) -> Option<Box<dyn LanguageAdapter>> {
    match ext {
        "rs" => Some(Box::new(RustAdapter)),
        "py" => Some(Box::new(python::PythonAdapter)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "ra_tests.rs"]
mod tests;
