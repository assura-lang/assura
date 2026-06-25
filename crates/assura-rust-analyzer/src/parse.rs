//! Doc comment parser and Rust source file parser.

use std::path::Path;

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
fn span_to_offset(span: proc_macro2::Span, _source: &str) -> usize {
    // proc_macro2 spans in non-proc-macro context (parsed via syn::parse_str
    // or syn::parse_file) provide byte offsets via start().
    span.start().column
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

/// Parse a Rust source string and extract all annotated items.
pub fn parse_rust_source(source: &str) -> Result<Vec<AnnotatedItem>, RustAnalyzerError> {
    let file = syn::parse_file(source).map_err(|e| RustAnalyzerError::Parse(format!("{e}")))?;

    let mut items = Vec::new();

    for item in &file.items {
        match item {
            syn::Item::Fn(func) => {
                let doc_lines = extract_doc_lines(&func.attrs, source);
                let contract = parse_doc_clauses(&doc_lines);
                if !contract.is_empty() {
                    let offset = func_span_offset(&func.sig, source);
                    items.push(AnnotatedItem {
                        contract,
                        kind: AnnotatedItemKind::Function {
                            name: func.sig.ident.to_string(),
                            params: extract_params(&func.sig),
                            return_type: extract_return_type(&func.sig),
                            is_unsafe: func.sig.unsafety.is_some(),
                            is_async: func.sig.asyncness.is_some(),
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
                    let offset = st.ident.span().start().column;
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
                    .map(|(_, path, _)| path.to_token_stream().to_string());

                if !impl_contract.is_empty() {
                    let offset = imp.impl_token.span.start().column;
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
                        let contract = parse_doc_clauses(&doc_lines);
                        if !contract.is_empty() {
                            let offset = func_span_offset_method(&method.sig, source);
                            items.push(AnnotatedItem {
                                contract,
                                kind: AnnotatedItemKind::Function {
                                    name: method.sig.ident.to_string(),
                                    params: extract_params(&method.sig),
                                    return_type: extract_return_type(&method.sig),
                                    is_unsafe: method.sig.unsafety.is_some(),
                                    is_async: method.sig.asyncness.is_some(),
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

/// Convert (line, column) (0-based line from proc_macro2) to byte offset.
fn line_col_to_offset(source: &str, line: usize, column: usize) -> usize {
    let mut offset = 0;
    for (i, src_line) in source.lines().enumerate() {
        if i == line {
            return offset + column.min(src_line.len());
        }
        offset += src_line.len() + 1; // +1 for newline
    }
    source.len()
}

/// Parse a Rust source file from disk and extract all annotated items.
pub fn parse_rust_file(path: &Path) -> Result<Vec<AnnotatedItem>, RustAnalyzerError> {
    let source = std::fs::read_to_string(path)?;
    parse_rust_source(&source)
}

/// Scan a directory recursively for `.rs` files and extract all annotated items.
pub fn scan_directory(
    dir: &Path,
) -> Result<Vec<(std::path::PathBuf, Vec<AnnotatedItem>)>, RustAnalyzerError> {
    let mut results = Vec::new();
    scan_dir_recursive(dir, &mut results)?;
    Ok(results)
}

fn scan_dir_recursive(
    dir: &Path,
    results: &mut Vec<(std::path::PathBuf, Vec<AnnotatedItem>)>,
) -> Result<(), RustAnalyzerError> {
    let entries = std::fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            // Skip target, hidden dirs, and generated dirs
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" || name == "generated" {
                continue;
            }
            scan_dir_recursive(&path, results)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            match parse_rust_file(&path) {
                Ok(items) if !items.is_empty() => {
                    results.push((path, items));
                }
                Ok(_) => {}  // No annotations found
                Err(_) => {} // Skip files that fail to parse
            }
        }
    }
    Ok(())
}
