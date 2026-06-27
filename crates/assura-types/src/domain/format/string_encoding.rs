// ===========================================================================
// T072: FMT.3 String encoding contracts
// ===========================================================================

use std::collections::HashMap;
use std::ops::Range;

use assura_parser::ast::{ClauseKind, Expr, SpExpr};

use crate::TypeError;
use crate::checkers::*;

/// Validates UTF-8/UTF-16/ASCII string encoding safety.
///
/// Error codes:
/// - A28001: unvalidated bytes used as string
/// - A28002: encoding mismatch (e.g., UTF-16 data treated as UTF-8)
/// - A28003: truncation within multi-byte sequence
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StringEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    Ascii,
    Latin1,
    RawBytes,
}

#[derive(Debug, Clone)]
pub(crate) struct StringEncodingChecker {
    variables: HashMap<String, StringEncoding>,
}

impl StringEncodingChecker {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
        }
    }

    pub fn declare(&mut self, name: String, encoding: StringEncoding) {
        self.variables.insert(name, encoding);
    }

    /// Look up the declared encoding for a variable.
    pub fn encoding_of(&self, name: &str) -> Option<&StringEncoding> {
        self.variables.get(name)
    }

    pub fn check_use_as_string(&self, name: &str, span: &Range<usize>) -> Option<TypeError> {
        match self.variables.get(name) {
            Some(StringEncoding::RawBytes) => Some(TypeError {
                code: "A28001".into(),
                message: format!("`{name}` is raw bytes, not a validated string"),
                span: span.clone(),
                secondary: None,
                suggestion: None,
            }),
            None => Some(TypeError {
                code: "A28001".into(),
                message: format!("`{name}` has unknown encoding, cannot use as string"),
                span: span.clone(),
                secondary: None,
                suggestion: None,
            }),
            _ => None,
        }
    }

    pub fn check_encoding_compat(
        &self,
        src: &str,
        dst_encoding: &StringEncoding,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if let Some(src_enc) = self.variables.get(src)
            && src_enc != dst_encoding
            && *src_enc != StringEncoding::Ascii
        {
            return Some(TypeError {
                code: "A28002".into(),
                message: format!("`{src}` is {src_enc:?} but used as {dst_encoding:?}"),
                span: span.clone(),
                secondary: None,
                suggestion: None,
            });
        }
        None
    }

    pub fn check_truncation(
        &self,
        name: &str,
        byte_len: usize,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if let Some(enc) = self.variables.get(name) {
            let unit_size = match enc {
                StringEncoding::Utf16Le | StringEncoding::Utf16Be => 2,
                _ => 1,
            };
            if unit_size > 1 && !byte_len.is_multiple_of(unit_size) {
                return Some(TypeError {
                    code: "A28003".into(),
                    message: format!(
                        "truncation of `{name}` at byte {byte_len} may split a {enc:?} code unit"
                    ),
                    span: span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        None
    }
}

impl Default for StringEncodingChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl StringEncodingChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = StringEncodingChecker::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "encoding" || k == "string_encoding" || k == "charset")
                {
                    found = true;
                    match &clause.body.node {
                        Expr::Call { func, args } => {
                            if let Expr::Ident(name) = &func.as_ref().node {
                                let enc = args
                                    .first()
                                    .and_then(extract_ident)
                                    .map(parse_encoding)
                                    .unwrap_or(StringEncoding::RawBytes);
                                checker.declare(name.clone(), enc);
                            }
                        }
                        Expr::Ident(name) => {
                            checker.declare(name.clone(), StringEncoding::RawBytes);
                        }
                        _ => {
                            let kvs = extract_kv_pairs(&clause.body);
                            let name = kvs
                                .iter()
                                .find(|(k, _)| *k == "name" || *k == "var")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("unnamed")
                                .to_string();
                            let enc = kvs
                                .iter()
                                .find(|(k, _)| *k == "encoding" || *k == "enc")
                                .and_then(|(_, v)| extract_ident(v))
                                .map(parse_encoding)
                                .unwrap_or(StringEncoding::RawBytes);
                            checker.declare(name, enc);
                        }
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let mut errors = Vec::new();
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if clause.kind == ClauseKind::Ensures {
                    let refs = collect_ident_references(&clause.body);
                    for name in &refs {
                        if let Some(err) = checker.check_use_as_string(name, &decl.span) {
                            errors.push(err);
                        }
                        let target_enc = checker
                            .encoding_of(name)
                            .cloned()
                            .unwrap_or(StringEncoding::Utf8);
                        if let Some(err) =
                            checker.check_encoding_compat(name, &target_enc, &decl.span)
                        {
                            errors.push(err);
                        }
                        let byte_len = extract_byte_len_from_context(&clause.body, name)
                            .unwrap_or_else(|| match checker.encoding_of(name) {
                                Some(StringEncoding::Utf16Le | StringEncoding::Utf16Be) => 2,
                                _ => 1,
                            });
                        if let Some(err) = checker.check_truncation(name, byte_len, &decl.span) {
                            errors.push(err);
                        }
                    }
                }
            }
        }
        errors
    }
}

/// Extract byte length from an expression context for a given variable name.
fn extract_byte_len_from_context(expr: &SpExpr, var_name: &str) -> Option<usize> {
    match &expr.node {
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            if let Expr::Ident(recv) = &receiver.node
                && recv == var_name
                && (method == "truncate" || method == "slice")
            {
                let len_arg = if method == "slice" {
                    args.get(1)
                } else {
                    args.first()
                };
                return len_arg.and_then(extract_int_literal).map(|v| v as usize);
            }
            let from_recv = extract_byte_len_from_context(receiver, var_name);
            if from_recv.is_some() {
                return from_recv;
            }
            for a in args {
                let from_a = extract_byte_len_from_context(a, var_name);
                if from_a.is_some() {
                    return from_a;
                }
            }
            None
        }
        Expr::Call { func, args } => {
            let from_f = extract_byte_len_from_context(func, var_name);
            if from_f.is_some() {
                return from_f;
            }
            for a in args {
                let from_a = extract_byte_len_from_context(a, var_name);
                if from_a.is_some() {
                    return from_a;
                }
            }
            None
        }
        Expr::BinOp { lhs, rhs, .. } => extract_byte_len_from_context(lhs, var_name)
            .or_else(|| extract_byte_len_from_context(rhs, var_name)),
        Expr::Block(exprs) | Expr::List(exprs) => exprs
            .iter()
            .find_map(|e| extract_byte_len_from_context(e, var_name)),
        _ => None,
    }
}

/// Parse a string encoding name to the enum.
fn parse_encoding(name: &str) -> StringEncoding {
    match name {
        "utf8" | "UTF8" | "utf-8" | "UTF-8" => StringEncoding::Utf8,
        "utf16le" | "UTF16LE" | "utf-16le" => StringEncoding::Utf16Le,
        "utf16be" | "UTF16BE" | "utf-16be" => StringEncoding::Utf16Be,
        "ascii" | "ASCII" => StringEncoding::Ascii,
        "latin1" | "LATIN1" | "iso-8859-1" => StringEncoding::Latin1,
        _ => StringEncoding::RawBytes,
    }
}
