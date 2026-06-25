//! Python language adapter for contract annotation extraction.

use crate::LanguageAdapter;
use crate::RustAnalyzerError;
use crate::parse::parse_doc_clauses;
use crate::types::{AnnotatedItem, AnnotatedItemKind, ParamInfo};

/// Python language adapter.
///
/// Extracts `# @requires`, `# @ensures`, `# @invariant`, `# @effects`,
/// and `# @decreases` annotations from Python comments and docstrings.
pub struct PythonAdapter;

impl LanguageAdapter for PythonAdapter {
    fn language_id(&self) -> &str {
        "python"
    }

    fn file_extensions(&self) -> &[&str] {
        &["py"]
    }

    fn parse_source(&self, source: &str) -> Result<Vec<AnnotatedItem>, RustAnalyzerError> {
        parse_python_source(source)
    }

    fn map_type(&self, language_type: &str) -> Option<String> {
        match language_type {
            "int" => Some("Int".to_string()),
            "float" => Some("Float".to_string()),
            "bool" => Some("Bool".to_string()),
            "str" => Some("String".to_string()),
            "bytes" => Some("Bytes".to_string()),
            "None" => Some("Unit".to_string()),
            "list" | "List" => Some("List".to_string()),
            "dict" | "Dict" => Some("Map".to_string()),
            "set" | "Set" => Some("Set".to_string()),
            "Optional" => Some("Option".to_string()),
            _ => None,
        }
    }
}

/// Parse Python source for contract annotations in comments and docstrings.
///
/// Supports two annotation styles:
/// 1. Hash comments: `# @requires x > 0`
/// 2. Docstring annotations: `"""@requires x > 0"""`
fn parse_python_source(source: &str) -> Result<Vec<AnnotatedItem>, RustAnalyzerError> {
    let lines: Vec<&str> = source.lines().collect();
    let mut items = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        // Collect annotation lines (# @keyword ... or docstring @keyword ...)
        let mut doc_lines: Vec<(String, usize)> = Vec::new();
        let _annotation_start = i;

        // Collect consecutive comment lines with @-clauses
        while i < lines.len() {
            let trimmed = lines[i].trim();
            if let Some(rest) = trimmed.strip_prefix('#') {
                let content = rest.trim_start();
                if content.starts_with('@') {
                    let byte_offset = source[..source_line_offset(source, i)].len();
                    doc_lines.push((content.to_string(), byte_offset));
                } else if !doc_lines.is_empty() {
                    // Continuation of non-@ comment after @-clauses
                    break;
                }
                i += 1;
            } else {
                break;
            }
        }

        if doc_lines.is_empty() {
            // Check for docstring annotations on the next line (inside a function)
            i += 1;
            continue;
        }

        let contract = parse_doc_clauses(&doc_lines);
        if contract.is_empty() {
            continue;
        }

        // Look for the function/class definition following the annotations
        while i < lines.len() && lines[i].trim().is_empty() {
            i += 1;
        }

        if i < lines.len() {
            let trimmed = lines[i].trim();

            if let Some(func_info) = parse_python_function_def(trimmed) {
                let byte_offset = source_line_offset(source, i);
                items.push(AnnotatedItem {
                    contract,
                    kind: AnnotatedItemKind::Function {
                        name: func_info.0,
                        params: func_info.1,
                        return_type: func_info.2,
                        is_unsafe: false,
                        is_async: func_info.3,
                    },
                    line: i + 1, // 1-based
                    offset: byte_offset,
                });
            } else if let Some(class_name) = parse_python_class_def(trimmed) {
                let byte_offset = source_line_offset(source, i);
                // Check for docstring invariants inside the class
                let mut class_doc_lines = Vec::new();
                let mut j = i + 1;
                while j < lines.len() {
                    let inner = lines[j].trim();
                    if inner.starts_with("\"\"\"") || inner.starts_with("'''") {
                        // Parse docstring for @-clauses
                        let in_docstring = inner.starts_with("\"\"\"");
                        let quote = if in_docstring { "\"\"\"" } else { "'''" };
                        // Check single-line docstring
                        if inner.len() > 6 && inner.ends_with(quote) {
                            let content = &inner[3..inner.len() - 3];
                            if content.trim().starts_with('@') {
                                let offset = source_line_offset(source, j);
                                class_doc_lines.push((format!(" {}", content.trim()), offset));
                            }
                        }
                        break;
                    } else if inner.is_empty() || inner.starts_with('#') {
                        j += 1;
                        continue;
                    } else {
                        break;
                    }
                }

                // Merge class-level annotations from before and after
                let mut full_contract = contract;
                if !class_doc_lines.is_empty() {
                    let docstring_contract = parse_doc_clauses(&class_doc_lines);
                    for c in docstring_contract.invariants {
                        full_contract.invariants.push(c);
                    }
                }

                items.push(AnnotatedItem {
                    contract: full_contract,
                    kind: AnnotatedItemKind::Struct {
                        name: class_name,
                        fields: Vec::new(), // Python class fields need runtime analysis
                    },
                    line: i + 1,
                    offset: byte_offset,
                });
            }
        }

        i += 1;
    }

    Ok(items)
}

/// Get byte offset of a line in source text.
fn source_line_offset(source: &str, line_index: usize) -> usize {
    let mut offset = 0;
    for (i, line) in source.lines().enumerate() {
        if i == line_index {
            return offset;
        }
        offset += line.len() + 1; // +1 for newline
    }
    source.len()
}

/// Parse a Python function definition line.
/// Returns (name, params, return_type, is_async).
fn parse_python_function_def(line: &str) -> Option<(String, Vec<ParamInfo>, Option<String>, bool)> {
    let is_async = line.starts_with("async ");
    let rest = if is_async {
        line.strip_prefix("async ")?.trim()
    } else {
        line
    };

    let rest = rest.strip_prefix("def ")?;
    let paren_start = rest.find('(')?;
    let name = rest[..paren_start].trim().to_string();

    // Extract parameters (simplified: just names and optional type annotations)
    let paren_end = rest.rfind(')')?;
    let params_str = &rest[paren_start + 1..paren_end];
    let params: Vec<ParamInfo> = params_str
        .split(',')
        .filter_map(|p| {
            let p = p.trim();
            if p.is_empty() || p == "self" || p == "cls" {
                return None;
            }
            if let Some((name, ty)) = p.split_once(':') {
                Some(ParamInfo {
                    name: name.trim().to_string(),
                    ty: ty.trim().to_string(),
                })
            } else {
                Some(ParamInfo {
                    name: p.to_string(),
                    ty: "Any".to_string(),
                })
            }
        })
        .collect();

    // Extract return type
    let after_paren = &rest[paren_end + 1..];
    let return_type = after_paren
        .strip_prefix("->")
        .or_else(|| after_paren.strip_prefix(" ->"))
        .map(|s| s.trim().trim_end_matches(':').trim().to_string())
        .filter(|s| !s.is_empty());

    Some((name, params, return_type, is_async))
}

/// Parse a Python class definition line. Returns the class name.
fn parse_python_class_def(line: &str) -> Option<String> {
    let rest = line.strip_prefix("class ")?;
    let end = rest.find(['(', ':']).unwrap_or(rest.len());
    let name = rest[..end].trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}
