// ===========================================================================
// T070: FMT.1 Binary format contracts
// ===========================================================================

use std::ops::Range;

use assura_parser::ast::{ClauseKind, Expr, SpExpr};

use crate::TypeError;
use crate::checkers::*;
use crate::types::*;

/// Validates byte-aligned binary format contracts.
///
/// Error codes:
/// - A26001: field offset exceeds buffer length
/// - A26002: field size mismatch
/// - A26003: endianness not specified for multi-byte field
/// - A26004: overlapping fields
#[derive(Debug, Clone)]
pub(crate) struct BinaryFormatChecker {
    fields: Vec<BinaryField>,
}

#[derive(Debug, Clone)]
pub(crate) struct BinaryField {
    pub name: String,
    pub offset: usize,
    pub size: usize,
    pub endianness: Option<Endianness>,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Endianness {
    Big,
    Little,
    Native,
}

impl BinaryFormatChecker {
    pub fn new() -> Self {
        Self { fields: Vec::new() }
    }

    pub fn add_field(&mut self, field: BinaryField) {
        self.fields.push(field);
    }

    pub fn check_bounds(&self, buffer_len: usize) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for f in &self.fields {
            if f.offset + f.size > buffer_len {
                errors.push(TypeError {
                    code: "A26001".into(),
                    message: format!(
                        "field `{}` at offset {} + size {} exceeds buffer length {buffer_len}",
                        f.name, f.offset, f.size
                    ),
                    span: f.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    pub fn check_endianness(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for f in &self.fields {
            if f.size > 1 && f.endianness.is_none() {
                errors.push(TypeError {
                    code: "A26003".into(),
                    message: format!(
                        "multi-byte field `{}` (size {}) has no endianness annotation",
                        f.name, f.size
                    ),
                    span: f.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    pub fn check_overlaps(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for i in 0..self.fields.len() {
            for j in (i + 1)..self.fields.len() {
                let a = &self.fields[i];
                let b = &self.fields[j];
                let a_end = a.offset + a.size;
                let b_end = b.offset + b.size;
                if a.offset < b_end && b.offset < a_end {
                    errors.push(TypeError {
                        code: "A26004".into(),
                        message: format!(
                            "fields `{}` [{},{}] and `{}` [{},{}] overlap",
                            a.name, a.offset, a_end, b.name, b.offset, b_end
                        ),
                        span: a.span.clone(),
                        secondary: None,
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_all(&self, buffer_len: usize) -> Vec<TypeError> {
        let mut errors = self.check_bounds(buffer_len);
        errors.extend(self.check_endianness());
        errors.extend(self.check_overlaps());
        errors
    }
}

impl Default for BinaryFormatChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl BinaryFormatChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = BinaryFormatChecker::new();
        let mut found = false;
        let mut buffer_len: usize = 0;
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "binary_format" || k == "byte_layout" {
                        found = true;
                        if let Some((_, args)) = extract_call(&clause.body) {
                            if let Some(len) = args.first().and_then(extract_int_literal) {
                                buffer_len = len as usize;
                            }
                        } else if let Some(len) = extract_int_literal(&clause.body) {
                            buffer_len = len as usize;
                        }
                    }
                    if k == "field" {
                        found = true;
                        let field = parse_binary_field(&clause.body, &decl.span);
                        checker.add_field(field);
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        checker.check_all(buffer_len)
    }
}

/// Parse a binary field declaration from an expression.
fn parse_binary_field(body: &SpExpr, span: &Range<usize>) -> BinaryField {
    match &body.node {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = &func.as_ref().node {
                let offset = args
                    .first()
                    .and_then(extract_int_literal)
                    .unwrap_or(DEFAULT_PARAM_ZERO) as usize;
                let size = args
                    .get(1)
                    .and_then(extract_int_literal)
                    .unwrap_or(DEFAULT_PARAM_ONE) as usize;
                let endianness = args.get(2).and_then(extract_ident).map(|e| match e {
                    "big" | "be" => Endianness::Big,
                    "little" | "le" => Endianness::Little,
                    _ => Endianness::Native,
                });
                return BinaryField {
                    name: name.clone(),
                    offset,
                    size,
                    endianness,
                    span: span.clone(),
                };
            }
            BinaryField {
                name: "unnamed".into(),
                offset: 0,
                size: 1,
                endianness: None,
                span: span.clone(),
            }
        }
        Expr::Ident(name) => BinaryField {
            name: name.clone(),
            offset: 0,
            size: 1,
            endianness: None,
            span: span.clone(),
        },
        _ => {
            let kvs = extract_kv_pairs(body);
            let name = kvs
                .iter()
                .find(|(k, _)| *k == "name")
                .and_then(|(_, v)| extract_ident(v))
                .unwrap_or("unnamed")
                .to_string();
            let offset = kvs
                .iter()
                .find(|(k, _)| *k == "offset")
                .and_then(|(_, v)| extract_int_literal(v))
                .unwrap_or(DEFAULT_PARAM_ZERO) as usize;
            let size = kvs
                .iter()
                .find(|(k, _)| *k == "size")
                .and_then(|(_, v)| extract_int_literal(v))
                .unwrap_or(DEFAULT_PARAM_ONE) as usize;
            let endianness = kvs
                .iter()
                .find(|(k, _)| *k == "endian" || *k == "endianness")
                .and_then(|(_, v)| extract_ident(v))
                .map(|e| match e {
                    "big" | "be" => Endianness::Big,
                    "little" | "le" => Endianness::Little,
                    _ => Endianness::Native,
                });
            BinaryField {
                name,
                offset,
                size,
                endianness,
                span: span.clone(),
            }
        }
    }
}
