//! Doc comment parser and Rust source file parser.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use quote::ToTokens;

use crate::RustAnalyzerError;
use crate::types::{
    AnnotatedItem, AnnotatedItemKind, ContractClause, FieldInfo, InlineClauseKind, InlineContract,
    ParamInfo,
};

/// Parse contract clauses from a sequence of doc comment lines.
///
/// Each line should be the content of a `///` doc comment (without the `///` prefix).
/// The `base_offset` is the byte offset of the first doc comment in the source file.
pub fn parse_doc_clauses(doc_lines: &[(String, usize)]) -> InlineContract {
    let mut contract = InlineContract::default();
    let mut current_kind: Option<InlineClauseKind> = None;
    let mut current_body = String::new();
    let mut current_offset: usize = 0;

    for (line, offset) in doc_lines {
        let trimmed = line.trim();

        // Check if this line starts a new @-clause
        if let Some(rest) = trimmed.strip_prefix('@') {
            // Flush any previous clause
            if let Some(kind) = current_kind.take() {
                let body = current_body.trim().to_string();
                if !body.is_empty() {
                    contract.push(ContractClause {
                        kind,
                        body,
                        offset: current_offset,
                    });
                }
                current_body.clear();
            }

            // Parse the keyword
            let (keyword, body_rest) = match rest.find(|c: char| c.is_whitespace()) {
                Some(pos) => (&rest[..pos], rest[pos..].trim()),
                None => (rest, ""),
            };

            if let Some(kind) = InlineClauseKind::from_keyword(keyword) {
                current_kind = Some(kind);
                current_body = body_rest.to_string();
                current_offset = *offset;
            }
            // If keyword is not recognized, ignore this line
        } else if current_kind.is_some() {
            // Continuation line for multi-line predicate
            if trimmed.is_empty() {
                // Empty line ends multi-line predicate
                if let Some(kind) = current_kind.take() {
                    let body = current_body.trim().to_string();
                    if !body.is_empty() {
                        contract.push(ContractClause {
                            kind,
                            body,
                            offset: current_offset,
                        });
                    }
                    current_body.clear();
                }
            } else {
                // Continuation: append to current body
                if !current_body.is_empty() {
                    current_body.push(' ');
                }
                current_body.push_str(trimmed);
            }
        }
        // Non-@, non-continuation lines are regular doc comments; skip.
    }

    // Flush final clause
    if let Some(kind) = current_kind {
        let body = current_body.trim().to_string();
        if !body.is_empty() {
            contract.push(ContractClause {
                kind,
                body,
                offset: current_offset,
            });
        }
    }

    contract
}

/// Extract contract clauses from proc-macro attributes (`#[requires(...)]`,
/// `#[ensures(...)]`, `#[invariant(...)]`).
///
/// This complements `parse_doc_clauses` which handles `/// @requires` doc comments.
fn extract_attr_clauses(attrs: &[syn::Attribute], source: &str) -> InlineContract {
    let mut contract = InlineContract::default();
    for attr in attrs {
        if let syn::Meta::List(meta_list) = &attr.meta {
            let kind = if meta_list.path.is_ident("requires") {
                Some(InlineClauseKind::Requires)
            } else if meta_list.path.is_ident("ensures") {
                Some(InlineClauseKind::Ensures)
            } else if meta_list.path.is_ident("ensures_ok") {
                Some(InlineClauseKind::EnsuresOk)
            } else if meta_list.path.is_ident("ensures_err") {
                Some(InlineClauseKind::EnsuresErr)
            } else if meta_list.path.is_ident("invariant") {
                Some(InlineClauseKind::Invariant)
            } else {
                None
            };

            if let Some(kind) = kind {
                let body = meta_list.tokens.to_string();
                let offset = span_to_offset(attr.pound_token.span, source);
                if !body.is_empty() {
                    contract.push(ContractClause { kind, body, offset });
                }
            }
        }
    }
    contract
}

/// Merge clauses from `other` into `base`.
fn merge_contracts(base: &mut InlineContract, other: InlineContract) {
    base.requires.extend(other.requires);
    base.ensures.extend(other.ensures);
    base.invariants.extend(other.invariants);
    base.effects.extend(other.effects);
    base.decreases.extend(other.decreases);
    base.ffi_boundary.extend(other.ffi_boundary);
    base.annotations.extend(other.annotations);
}

// ---------------------------------------------------------------------------
// Rust source file parser
// ---------------------------------------------------------------------------

/// Extract doc comment lines from syn attributes.
///
/// Returns pairs of (line_content, byte_offset).
fn extract_doc_lines(attrs: &[syn::Attribute], source: &str) -> Vec<(String, usize)> {
    let mut lines = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let syn::Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(lit_str),
                ..
            }) = &nv.value
        {
            let content = lit_str.value();
            // Compute byte offset from span
            let span = attr.pound_token.span;
            let offset = span_to_offset(span, source);
            lines.push((content, offset));
        }
    }
    lines
}

/// Convert a proc_macro2::Span to a byte offset in source.
///
/// In non-proc-macro context, span locations may not be available.
/// We fall back to 0 if we cannot determine the offset.
fn span_to_offset(span: proc_macro2::Span, source: &str) -> usize {
    let start = span.start();
    line_col_to_offset(source, start.line, start.column)
}

/// Extract a string representation of a syn type.
fn type_to_string(ty: &syn::Type) -> String {
    ty.to_token_stream().to_string()
}

/// Extract function parameters as ParamInfo.
fn extract_params(sig: &syn::Signature) -> Vec<ParamInfo> {
    sig.inputs
        .iter()
        .map(|arg| match arg {
            syn::FnArg::Typed(pat_type) => {
                let name = pat_to_string(&pat_type.pat);
                let ty = type_to_string(&pat_type.ty);
                ParamInfo { name, ty }
            }
            syn::FnArg::Receiver(_) => ParamInfo {
                name: "self".to_string(),
                ty: "Self".to_string(),
            },
        })
        .collect()
}

/// Extract a string from a pattern.
fn pat_to_string(pat: &syn::Pat) -> String {
    pat.to_token_stream().to_string()
}

/// Extract return type as a string, None for `()` / no return.
fn extract_return_type(sig: &syn::Signature) -> Option<String> {
    match &sig.output {
        syn::ReturnType::Default => None,
        syn::ReturnType::Type(_, ty) => Some(type_to_string(ty)),
    }
}

/// Compute line number (1-based) from byte offset in source.
fn offset_to_line(source: &str, offset: usize) -> usize {
    let clamped = offset.min(source.len());
    source[..clamped].chars().filter(|&c| c == '\n').count() + 1
}

/// Options for scanning Rust sources for contract items.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScanOptions {
    /// When true, include functions that have no contract annotations.
    /// Used by `check-rust --suggest` so unannotated items are candidates.
    pub include_unannotated: bool,
}

/// Parse a Rust source string and extract annotated items.
pub fn parse_rust_source(source: &str) -> Result<Vec<AnnotatedItem>, RustAnalyzerError> {
    parse_rust_source_with_options(source, ScanOptions::default())
}

/// Parse a Rust source string, optionally including unannotated functions.
pub fn parse_rust_source_with_options(
    source: &str,
    opts: ScanOptions,
) -> Result<Vec<AnnotatedItem>, RustAnalyzerError> {
    let file = syn::parse_file(source).map_err(|e| RustAnalyzerError::Parse(format!("{e}")))?;

    let mut items = Vec::new();

    for item in &file.items {
        match item {
            syn::Item::Fn(func) => {
                let doc_lines = extract_doc_lines(&func.attrs, source);
                let mut contract = parse_doc_clauses(&doc_lines);
                let attr_contract = extract_attr_clauses(&func.attrs, source);
                merge_contracts(&mut contract, attr_contract);
                if !contract.is_empty() || opts.include_unannotated {
                    let offset = func_span_offset(&func.sig, source);
                    items.push(AnnotatedItem {
                        contract,
                        kind: AnnotatedItemKind::Function {
                            name: func.sig.ident.to_string(),
                            params: extract_params(&func.sig),
                            return_type: extract_return_type(&func.sig),
                            is_unsafe: matches!(func.sig.safety, syn::Safety::Unsafe(_)),
                            is_async: func.sig.asyncness.is_some(),
                            is_public: matches!(func.vis, syn::Visibility::Public(_)),
                        },
                        line: offset_to_line(source, offset),
                        offset,
                    });
                }
            }
            syn::Item::Struct(st) => {
                let doc_lines = extract_doc_lines(&st.attrs, source);
                let contract = parse_doc_clauses(&doc_lines);
                if !contract.is_empty() {
                    let start = st.ident.span().start();
                    let offset = line_col_to_offset(source, start.line, start.column);
                    let fields = match &st.fields {
                        syn::Fields::Named(named) => named
                            .named
                            .iter()
                            .filter_map(|f| {
                                f.ident.as_ref().map(|id| FieldInfo {
                                    name: id.to_string(),
                                    ty: type_to_string(&f.ty),
                                })
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                    items.push(AnnotatedItem {
                        contract,
                        kind: AnnotatedItemKind::Struct {
                            name: st.ident.to_string(),
                            fields,
                        },
                        line: offset_to_line(source, offset),
                        offset,
                    });
                }
            }
            syn::Item::Impl(imp) => {
                // Check impl-level doc comments for invariants
                let impl_doc_lines = extract_doc_lines(&imp.attrs, source);
                let impl_contract = parse_doc_clauses(&impl_doc_lines);

                let self_type = type_to_string(&imp.self_ty);
                let trait_name = imp
                    .trait_
                    .as_ref()
                    .map(|(path, _)| path.to_token_stream().to_string());

                if !impl_contract.is_empty() {
                    let start = imp.impl_token.span.start();
                    let offset = line_col_to_offset(source, start.line, start.column);
                    items.push(AnnotatedItem {
                        contract: impl_contract,
                        kind: AnnotatedItemKind::ImplBlock {
                            self_type: self_type.clone(),
                            trait_name: trait_name.clone(),
                        },
                        line: offset_to_line(source, offset),
                        offset,
                    });
                }

                // Check methods within the impl block
                for impl_item in &imp.items {
                    if let syn::ImplItem::Fn(method) = impl_item {
                        let doc_lines = extract_doc_lines(&method.attrs, source);
                        let mut contract = parse_doc_clauses(&doc_lines);
                        let attr_contract = extract_attr_clauses(&method.attrs, source);
                        merge_contracts(&mut contract, attr_contract);
                        if !contract.is_empty() || opts.include_unannotated {
                            let offset = func_span_offset_method(&method.sig, source);
                            items.push(AnnotatedItem {
                                contract,
                                kind: AnnotatedItemKind::Function {
                                    name: method.sig.ident.to_string(),
                                    params: extract_params(&method.sig),
                                    return_type: extract_return_type(&method.sig),
                                    is_unsafe: matches!(method.sig.safety, syn::Safety::Unsafe(_)),
                                    is_async: method.sig.asyncness.is_some(),
                                    is_public: matches!(method.vis, syn::Visibility::Public(_)),
                                },
                                line: offset_to_line(source, offset),
                                offset,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(items)
}

/// Get byte offset of a function signature's identifier in source.
fn func_span_offset(sig: &syn::Signature, source: &str) -> usize {
    // Use the fn keyword span line/col to compute byte offset
    let start = sig.fn_token.span.start();
    line_col_to_offset(source, start.line, start.column)
}

/// Get byte offset of a method signature's identifier in source.
fn func_span_offset_method(sig: &syn::Signature, source: &str) -> usize {
    let start = sig.fn_token.span.start();
    line_col_to_offset(source, start.line, start.column)
}

/// Convert (line, column) to byte offset.
///
/// `proc_macro2::LineColumn::line` is 1-based; `column` is 0-based.
/// Walks raw terminators (`\n` or `\r\n`) so CRLF sources stay aligned.
fn line_col_to_offset(source: &str, line: usize, column: usize) -> usize {
    let mut offset = 0;
    let mut current = 1;
    while current < line {
        let rest = &source[offset..];
        if let Some(idx) = rest.find('\n') {
            offset += idx + 1;
            current += 1;
        } else if let Some(idx) = rest.find('\r') {
            offset += idx + 1;
            current += 1;
        } else {
            return source.len();
        }
    }
    let rest = &source[offset..];
    let line_len = rest.find(['\n', '\r']).unwrap_or(rest.len());
    offset + column.min(line_len)
}

/// Parse a Rust source file from disk and extract all annotated items.
pub fn parse_rust_file(path: &Path) -> Result<Vec<AnnotatedItem>, RustAnalyzerError> {
    parse_rust_file_with_options(path, ScanOptions::default())
}

/// Parse a Rust source file, optionally including unannotated functions.
pub fn parse_rust_file_with_options(
    path: &Path,
    opts: ScanOptions,
) -> Result<Vec<AnnotatedItem>, RustAnalyzerError> {
    let source = read_source_limited(path, MAX_SOURCE_BYTES)?;
    parse_rust_source_with_options(&source, opts)
}

/// Scan a directory recursively for `.rs` files and extract all annotated items.
pub fn scan_directory(
    dir: &Path,
) -> Result<Vec<(std::path::PathBuf, Vec<AnnotatedItem>)>, RustAnalyzerError> {
    scan_directory_with_options(dir, ScanOptions::default())
}

/// Scan a directory, optionally including unannotated functions.
pub fn scan_directory_with_options(
    dir: &Path,
    opts: ScanOptions,
) -> Result<Vec<(std::path::PathBuf, Vec<AnnotatedItem>)>, RustAnalyzerError> {
    let mut results = Vec::new();
    let mut visited = HashSet::new();
    scan_dir_recursive(dir, opts, &mut results, &mut visited)?;
    Ok(results)
}

fn scan_dir_recursive(
    dir: &Path,
    opts: ScanOptions,
    results: &mut Vec<(PathBuf, Vec<AnnotatedItem>)>,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), RustAnalyzerError> {
    if let Ok(canon) = dir.canonicalize() {
        if !visited.insert(canon) {
            return Ok(());
        }
    }
    let entries = std::fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if is_scan_symlink(&path) {
            continue;
        }
        if path.is_dir() {
            // Skip target, hidden dirs, and generated dirs
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" || name == "generated" {
                continue;
            }
            scan_dir_recursive(&path, opts, results, visited)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            match parse_rust_file_with_options(&path, opts) {
                Ok(items) if !items.is_empty() => {
                    results.push((path, items));
                }
                Ok(_) => {} // No matching items found
                Err(e) => {
                    return Err(RustAnalyzerError::Parse(format!("{}: {e}", path.display())));
                }
            }
        }
    }
    Ok(())
}

/// Same 16 MiB cap as CLI `read_source_arg` / MCP `MAX_SOURCE_BYTES`.
const MAX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;

fn read_source_limited(path: &Path, max: u64) -> Result<String, std::io::Error> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    file.take(max.saturating_add(1)).read_to_end(&mut buf)?;
    if (buf.len() as u64) > max {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("source exceeds maximum size of {max} bytes"),
        ));
    }
    String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn scan_entry_is_link(file_type_is_symlink: bool, windows_reparse_point: bool) -> bool {
    file_type_is_symlink || windows_reparse_point
}

fn is_scan_symlink(path: &Path) -> bool {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let is_reparse = {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
            meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        }
        #[cfg(not(windows))]
        {
            false
        }
    };
    scan_entry_is_link(meta.file_type().is_symlink(), is_reparse)
}

#[cfg(test)]
mod limited_read_tests {
    use super::*;

    #[test]
    fn read_source_limited_rejects_over_max() {
        let dir = std::env::temp_dir().join(format!(
            "assura_ra_cap_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("big.rs");
        std::fs::write(&path, "fn foo() {}\n// extra").expect("write probe");
        let result = read_source_limited(&path, 8);
        let _ = std::fs::remove_dir_all(&dir);
        let err = result.expect_err("over-max file must be rejected");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            err.to_string().contains("maximum size"),
            "size error must mention the cap: {err}"
        );
    }

    #[test]
    fn scan_entry_is_link_skips_symlinks_and_reparse_points() {
        assert!(
            scan_entry_is_link(true, false),
            "symlink entries must be skipped"
        );
        assert!(
            scan_entry_is_link(false, true),
            "Windows reparse/junction entries must be skipped"
        );
        assert!(!scan_entry_is_link(false, false));
    }
}
