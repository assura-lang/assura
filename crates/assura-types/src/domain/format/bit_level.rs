// ===========================================================================
// T071: FMT.2 Bit-level format contracts
// ===========================================================================

use std::ops::Range;

use assura_parser::ast::{ClauseKind, Expr, SpExpr};

use crate::TypeError;
use crate::checkers::*;
use crate::types::*;

/// Validates sub-byte parsing with ghost bit cursor tracking.
///
/// Error codes:
/// - A27001: bit offset exceeds container size
/// - A27002: bit field crosses byte boundary without permission
/// - A27003: total bit width doesn't match declared size
#[derive(Debug, Clone)]
pub(crate) struct BitLevelChecker {
    fields: Vec<BitField>,
    container_bits: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct BitField {
    pub name: String,
    pub bit_offset: usize,
    pub bit_width: usize,
    pub span: Range<usize>,
    pub cross_byte_ok: bool,
}

impl BitLevelChecker {
    pub fn new(container_bits: usize) -> Self {
        Self {
            fields: Vec::new(),
            container_bits,
        }
    }

    pub fn add_field(&mut self, field: BitField) {
        self.fields.push(field);
    }

    pub fn check_bounds(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for f in &self.fields {
            if f.bit_offset + f.bit_width > self.container_bits {
                errors.push(TypeError {
                    code: "A27001".into(),
                    message: format!(
                        "bit field `{}` at bit {} + width {} exceeds container ({} bits)",
                        f.name, f.bit_offset, f.bit_width, self.container_bits
                    ),
                    span: f.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    pub fn check_byte_crossing(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for f in &self.fields {
            if !f.cross_byte_ok {
                let start_byte = f.bit_offset / 8;
                let end_byte = (f.bit_offset + f.bit_width.saturating_sub(1)) / 8;
                if start_byte != end_byte {
                    errors.push(TypeError {
                        code: "A27002".into(),
                        message: format!(
                            "bit field `{}` crosses byte boundary (bytes {start_byte}-{end_byte})",
                            f.name
                        ),
                        span: f.span.clone(),
                        secondary: None,
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_total_width(&self, declared_size: usize) -> Option<TypeError> {
        let total: usize = self.fields.iter().map(|f| f.bit_width).sum();
        if total != declared_size {
            return Some(TypeError {
                code: "A27003".into(),
                message: format!(
                    "total bit width {total} doesn't match declared size {declared_size}"
                ),
                span: 0..1,
                secondary: None,
                suggestion: None,
            });
        }
        None
    }

    pub fn check_all(&self, declared_size: usize) -> Vec<TypeError> {
        let mut errors = self.check_bounds();
        errors.extend(self.check_byte_crossing());
        if let Some(e) = self.check_total_width(declared_size) {
            errors.push(e);
        }
        errors
    }
}

impl BitLevelChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut container_bits: usize = 0;
        let mut checker: Option<BitLevelChecker> = None;
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "bit_layout" || k == "bit_level" {
                        found = true;
                        let bits = match &clause.body.node {
                            Expr::Call { func: _, args } => args
                                .first()
                                .and_then(extract_int_literal)
                                .unwrap_or(DEFAULT_BIT_CONTAINER_BITS)
                                as usize,
                            Expr::Literal(_) => extract_int_literal(&clause.body)
                                .unwrap_or(DEFAULT_BIT_CONTAINER_BITS)
                                as usize,
                            _ => 64,
                        };
                        container_bits = bits;
                        checker = Some(BitLevelChecker::new(bits));
                    }
                    if k == "bit_field" {
                        found = true;
                        let field = parse_bit_field(&clause.body, &decl.span);
                        if let Some(field) = field {
                            if let Some(ref mut ch) = checker {
                                ch.add_field(field);
                            } else {
                                container_bits = DEFAULT_BIT_CONTAINER_BITS as usize;
                                let mut ch = BitLevelChecker::new(container_bits);
                                ch.add_field(field);
                                checker = Some(ch);
                            }
                        }
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        match checker {
            Some(ch) => ch.check_all(container_bits),
            None => Vec::new(),
        }
    }
}

/// Parse a bit field declaration from an expression.
fn parse_bit_field(body: &SpExpr, span: &Range<usize>) -> Option<BitField> {
    match &body.node {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = &func.as_ref().node {
                let bit_offset = args
                    .first()
                    .and_then(extract_int_literal)
                    .unwrap_or(DEFAULT_PARAM_ZERO) as usize;
                let bit_width = args
                    .get(1)
                    .and_then(extract_int_literal)
                    .unwrap_or(DEFAULT_PARAM_ONE) as usize;
                let cross_byte_ok = args
                    .get(2)
                    .and_then(extract_ident)
                    .is_some_and(|v| v == "true");
                Some(BitField {
                    name: name.clone(),
                    bit_offset,
                    bit_width,
                    span: span.clone(),
                    cross_byte_ok,
                })
            } else {
                None
            }
        }
        Expr::Ident(name) => Some(BitField {
            name: name.clone(),
            bit_offset: 0,
            bit_width: 1,
            span: span.clone(),
            cross_byte_ok: false,
        }),
        _ => {
            let kvs = extract_kv_pairs(body);
            let name = kvs
                .iter()
                .find(|(k, _)| *k == "name")
                .and_then(|(_, v)| extract_ident(v))
                .unwrap_or("unnamed")
                .to_string();
            let bit_offset = kvs
                .iter()
                .find(|(k, _)| *k == "offset" || *k == "bit_offset")
                .and_then(|(_, v)| extract_int_literal(v))
                .unwrap_or(DEFAULT_PARAM_ZERO) as usize;
            let bit_width = kvs
                .iter()
                .find(|(k, _)| *k == "width" || *k == "bit_width" || *k == "size")
                .and_then(|(_, v)| extract_int_literal(v))
                .unwrap_or(DEFAULT_PARAM_ONE) as usize;
            let cross_byte_ok = kvs
                .iter()
                .find(|(k, _)| *k == "cross_byte" || *k == "cross_byte_ok")
                .and_then(|(_, v)| extract_ident(v))
                .is_some_and(|v| v == "true");
            Some(BitField {
                name,
                bit_offset,
                bit_width,
                span: span.clone(),
                cross_byte_ok,
            })
        }
    }
}
