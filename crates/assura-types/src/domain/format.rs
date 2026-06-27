//! Format-related domain checkers.
//!
//! BinaryFormatChecker, BitLevelChecker, StringEncodingChecker,
//! ChecksumChecker, ProtocolGrammarChecker, and source-level check
//! wiring moved from `checks/format.rs`.

use std::collections::HashMap;
use std::ops::Range;

use assura_parser::ast::{ClauseKind, Decl, Expr, SpExpr};

use crate::TypeError;
use crate::checkers::*;
use crate::domain::OpaqueFunctionChecker;
use crate::types::*;

// ===========================================================================
// T070: FMT.1 Binary format contracts
// ===========================================================================

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

// ===========================================================================
// T071: FMT.2 Bit-level format contracts
// ===========================================================================

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

// ===========================================================================
// T072: FMT.3 String encoding contracts
// ===========================================================================

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

// ===========================================================================
// T074: FMT.5 Checksum integrity
// ===========================================================================

/// Validates checksum verification contracts.
///
/// Error codes:
/// - A29001: data used before checksum verification
/// - A29002: checksum algorithm mismatch
/// - A29003: checksum covers wrong byte range
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ChecksumAlgorithm {
    Crc32,
    Adler32,
    Sha256,
    Sha512,
    Md5,
    Custom(String),
}

#[derive(Debug, Clone)]
pub(crate) struct ChecksumChecker {
    /// Data regions and their checksum status
    regions: HashMap<String, ChecksumRegion>,
}

#[derive(Debug, Clone)]
pub(crate) struct ChecksumRegion {
    pub algorithm: ChecksumAlgorithm,
    pub byte_start: usize,
    pub byte_end: usize,
    pub verified: bool,
}

impl ChecksumChecker {
    pub fn new() -> Self {
        Self {
            regions: HashMap::new(),
        }
    }

    pub fn declare_region(
        &mut self,
        name: String,
        algorithm: ChecksumAlgorithm,
        start: usize,
        end: usize,
    ) {
        self.regions.insert(
            name,
            ChecksumRegion {
                algorithm,
                byte_start: start,
                byte_end: end,
                verified: false,
            },
        );
    }

    pub fn mark_verified(&mut self, name: &str) {
        if let Some(region) = self.regions.get_mut(name) {
            region.verified = true;
        }
    }

    /// Look up a region's declared algorithm and byte range.
    pub fn region_info(&self, name: &str) -> Option<(&ChecksumAlgorithm, usize, usize)> {
        self.regions
            .get(name)
            .map(|r| (&r.algorithm, r.byte_start, r.byte_end))
    }

    pub fn check_use_before_verify(&self, name: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some(region) = self.regions.get(name)
            && !region.verified
        {
            return Some(TypeError {
                code: "A29001".into(),
                message: format!("data region `{name}` used before checksum verification"),
                span: span.clone(),
                secondary: None,
                suggestion: None,
            });
        }
        None
    }

    pub fn check_algorithm_match(
        &self,
        name: &str,
        expected: &ChecksumAlgorithm,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if let Some(region) = self.regions.get(name)
            && &region.algorithm != expected
        {
            return Some(TypeError {
                code: "A29002".into(),
                message: format!(
                    "checksum algorithm mismatch for `{name}`: declared {:?}, used {:?}",
                    region.algorithm, expected
                ),
                span: span.clone(),
                secondary: None,
                suggestion: None,
            });
        }
        None
    }

    pub fn check_range_coverage(
        &self,
        name: &str,
        data_start: usize,
        data_end: usize,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if let Some(region) = self.regions.get(name)
            && (region.byte_start > data_start || region.byte_end < data_end)
        {
            return Some(TypeError {
                code: "A29003".into(),
                message: format!(
                    "checksum for `{name}` covers [{},{}] but data range is [{data_start},{data_end}]",
                    region.byte_start, region.byte_end
                ),
                span: span.clone(),
                secondary: None,
                suggestion: None,
            });
        }
        None
    }
}

impl Default for ChecksumChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T075: FMT.6 Protocol grammar
// ===========================================================================

/// Validates protocol state machine and RFC conformance.
///
/// Error codes:
/// - A30001: invalid state transition
/// - A30002: message sent in wrong protocol state
/// - A30003: required message field missing
#[derive(Debug, Clone)]
pub(crate) struct ProtocolGrammarChecker {
    states: Vec<String>,
    current_state: String,
    transitions: Vec<ProtocolTransition>,
    required_fields: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProtocolTransition {
    pub from: String,
    pub to: String,
    pub message: String,
}

impl ProtocolGrammarChecker {
    pub fn new(initial_state: String) -> Self {
        Self {
            states: vec![initial_state.clone()],
            current_state: initial_state,
            transitions: Vec::new(),
            required_fields: HashMap::new(),
        }
    }

    pub fn add_state(&mut self, state: String) {
        if !self.states.contains(&state) {
            self.states.push(state);
        }
    }

    pub fn add_transition(&mut self, from: String, to: String, message: String) {
        self.transitions
            .push(ProtocolTransition { from, to, message });
    }

    pub fn add_required_fields(&mut self, message: String, fields: Vec<String>) {
        self.required_fields.insert(message, fields);
    }

    pub fn check_send(&self, message: &str, span: &Range<usize>) -> Option<TypeError> {
        let valid = self
            .transitions
            .iter()
            .any(|t| t.from == self.current_state && t.message == message);
        if !valid {
            return Some(TypeError {
                code: "A30002".into(),
                message: format!("cannot send `{message}` in state `{}`", self.current_state),
                span: span.clone(),
                secondary: None,
                suggestion: None,
            });
        }
        None
    }

    pub fn transition(&mut self, message: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some(t) = self
            .transitions
            .iter()
            .find(|t| t.from == self.current_state && t.message == message)
        {
            self.current_state = t.to.clone();
            None
        } else {
            Some(TypeError {
                code: "A30001".into(),
                message: format!(
                    "invalid transition: no `{message}` transition from state `{}`",
                    self.current_state
                ),
                span: span.clone(),
                secondary: None,
                suggestion: None,
            })
        }
    }

    pub fn check_required_fields(
        &self,
        message: &str,
        provided: &[&str],
        span: &Range<usize>,
    ) -> Vec<TypeError> {
        let mut errors = Vec::new();
        if let Some(required) = self.required_fields.get(message) {
            for field in required {
                if !provided.contains(&field.as_str()) {
                    errors.push(TypeError {
                        code: "A30003".into(),
                        message: format!("required field `{field}` missing in message `{message}`"),
                        span: span.clone(),
                        secondary: None,
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }
}

// ===========================================================================
// Source-level check wiring (moved from checks/format.rs)
// ===========================================================================

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

impl ChecksumChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = ChecksumChecker::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "checksum" || k == "crc" || k == "hash" {
                        found = true;
                        parse_checksum_decl(&mut checker, &clause.body, &decl.span);
                    }
                    if (k == "verify_checksum" || k == "verified")
                        && let Expr::Ident(name) = &clause.body.node
                    {
                        checker.mark_verified(name);
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
                if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                    let refs = collect_ident_references(&clause.body);
                    for name in &refs {
                        if let Some(err) = checker.check_use_before_verify(name, &decl.span) {
                            errors.push(err);
                        }
                        if let Some((algo, region_start, region_end)) = checker.region_info(name) {
                            let algo = algo.clone();
                            if let Some(err) =
                                checker.check_algorithm_match(name, &algo, &decl.span)
                            {
                                errors.push(err);
                            }
                            if let Some(err) = checker.check_range_coverage(
                                name,
                                region_start,
                                region_end,
                                &decl.span,
                            ) {
                                errors.push(err);
                            }
                        }
                    }
                }
            }
        }
        errors
    }
}

/// Parse a checksum declaration clause body.
fn parse_checksum_decl(checker: &mut ChecksumChecker, body: &SpExpr, _span: &Range<usize>) {
    match &body.node {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = &func.as_ref().node {
                let algo = args
                    .first()
                    .and_then(extract_ident)
                    .map(parse_checksum_algorithm)
                    .unwrap_or(ChecksumAlgorithm::Crc32);
                let start = args
                    .get(1)
                    .and_then(extract_int_literal)
                    .unwrap_or(DEFAULT_PARAM_ZERO) as usize;
                let end = args
                    .get(2)
                    .and_then(extract_int_literal)
                    .unwrap_or(DEFAULT_REGION_SIZE) as usize;
                checker.declare_region(name.clone(), algo, start, end);
            }
        }
        Expr::Ident(name) => {
            checker.declare_region(name.clone(), ChecksumAlgorithm::Crc32, 0, 0);
        }
        _ => {
            let kvs = extract_kv_pairs(body);
            let name = kvs
                .iter()
                .find(|(k, _)| *k == "name" || *k == "region")
                .and_then(|(_, v)| extract_ident(v))
                .unwrap_or("unnamed")
                .to_string();
            let algo = kvs
                .iter()
                .find(|(k, _)| *k == "algorithm" || *k == "algo")
                .and_then(|(_, v)| extract_ident(v))
                .map(parse_checksum_algorithm)
                .unwrap_or(ChecksumAlgorithm::Crc32);
            let start = kvs
                .iter()
                .find(|(k, _)| *k == "start")
                .and_then(|(_, v)| extract_int_literal(v))
                .unwrap_or(DEFAULT_PARAM_ZERO) as usize;
            let end = kvs
                .iter()
                .find(|(k, _)| *k == "end")
                .and_then(|(_, v)| extract_int_literal(v))
                .unwrap_or(DEFAULT_REGION_SIZE) as usize;
            checker.declare_region(name, algo, start, end);
        }
    }
}

/// Parse a checksum algorithm name to the enum.
fn parse_checksum_algorithm(name: &str) -> ChecksumAlgorithm {
    match name {
        "crc32" | "CRC32" | "crc" => ChecksumAlgorithm::Crc32,
        "adler32" | "ADLER32" | "adler" => ChecksumAlgorithm::Adler32,
        "sha256" | "SHA256" | "sha-256" => ChecksumAlgorithm::Sha256,
        "sha512" | "SHA512" | "sha-512" => ChecksumAlgorithm::Sha512,
        "md5" | "MD5" => ChecksumAlgorithm::Md5,
        _ => ChecksumAlgorithm::Custom(name.to_string()),
    }
}

impl ProtocolGrammarChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker: Option<ProtocolGrammarChecker> = None;
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "protocol" || k == "state_machine" || k == "rfc" {
                        found = true;
                        let initial = extract_ident(&clause.body).unwrap_or("init").to_string();
                        if checker.is_none() {
                            checker = Some(ProtocolGrammarChecker::new(initial));
                        }
                    }
                    if (k == "state" || k == "protocol_state")
                        && let Some(name) = extract_ident(&clause.body)
                        && let Some(ref mut ch) = checker
                    {
                        ch.add_state(name.to_string());
                    }
                    if k == "transition"
                        && let Some((from, args)) = extract_call(&clause.body)
                        && args.len() >= 2
                        && let Some(ref mut ch) = checker
                    {
                        let msg = extract_ident(&args[0]).unwrap_or("unknown").to_string();
                        let to = extract_ident(&args[1]).unwrap_or("unknown").to_string();
                        ch.add_transition(from.to_string(), to, msg);
                    }
                    if (k == "required_fields" || k == "required")
                        && let Some((msg, args)) = extract_call(&clause.body)
                        && let Some(ref mut ch) = checker
                    {
                        let field_names: Vec<String> = args
                            .iter()
                            .filter_map(|a| extract_ident(a).map(String::from))
                            .collect();
                        ch.add_required_fields(msg.to_string(), field_names);
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let checker = match checker {
            Some(c) => c,
            None => return Vec::new(),
        };
        let mut checker = checker;
        let mut errors = Vec::new();
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "send" || k == "message")
                    && let Some(msg) = extract_ident(&clause.body)
                {
                    if let Some(err) = checker.check_send(msg, &decl.span) {
                        errors.push(err);
                    }
                    if let Some(err) = checker.transition(msg, &decl.span) {
                        errors.push(err);
                    }
                    let field_errs = checker.check_required_fields(msg, &[], &decl.span);
                    errors.extend(field_errs);
                }
            }
        }
        errors
    }
}

impl OpaqueFunctionChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = OpaqueFunctionChecker::new();
        let mut found = false;
        for decl in &source.decls {
            if let Decl::FnDef(f) = &decl.node {
                for clause in &f.clauses {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && k == "opaque"
                    {
                        found = true;
                        let has_contract = f
                            .clauses
                            .iter()
                            .any(|c| matches!(c.kind, ClauseKind::Requires | ClauseKind::Ensures));
                        checker.declare_opaque(f.name.clone(), has_contract, decl.span.clone());
                    }
                }
            } else if let Decl::Contract(c) = &decl.node {
                for clause in &c.clauses {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && k == "opaque"
                    {
                        found = true;
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
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "proof" || k == "proof_context" {
                        checker.enter_proof();
                    }
                    if k == "end_proof" {
                        checker.exit_proof();
                    }
                    if k == "reveal"
                        && let Expr::Ident(fn_name) = &clause.body.node
                        && let Some(err) = checker.reveal(fn_name, &decl.span)
                    {
                        errors.push(err);
                    }
                }
                if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                    let refs = collect_ident_references(&clause.body);
                    for name in &refs {
                        if let Some(err) = checker.check_call(name, &decl.span) {
                            errors.push(err);
                        }
                        if checker.is_opaque(name)
                            && let Some(mut err) = checker.check_body_access(name, &decl.span)
                        {
                            err.secondary = checker.opaque_span(name).map(|s| {
                                (s.clone(), format!("opaque function `{name}` declared here"))
                            });
                            errors.push(err);
                        }
                    }
                }
            }
        }
        errors
    }
}

/// Check codec registry declarations (G008: FMT.4).
pub fn check_codec_registry(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    use assura_parser::ast::MagicPattern;
    let mut errors = Vec::new();

    for decl in &source.decls {
        let Decl::CodecRegistry(cr) = &decl.node else {
            continue;
        };

        // A52001: Check for overlapping magic byte prefixes
        let byte_patterns: Vec<(usize, &[u8])> = cr
            .codecs
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match &c.magic {
                MagicPattern::Bytes { bytes, .. } if !bytes.is_empty() => {
                    Some((i, bytes.as_slice()))
                }
                _ => None,
            })
            .collect();

        for (i, (idx_a, bytes_a)) in byte_patterns.iter().enumerate() {
            for (idx_b, bytes_b) in byte_patterns.iter().skip(i + 1) {
                let min_len = bytes_a.len().min(bytes_b.len());
                if bytes_a[..min_len] == bytes_b[..min_len] {
                    errors.push(TypeError {
                        code: "A52001".into(),
                        message: format!(
                            "overlapping magic byte patterns in codec registry `{}`: \
                             codec `{}` and codec `{}` share a common prefix",
                            cr.name, cr.codecs[*idx_a].name, cr.codecs[*idx_b].name,
                        ),
                        span: decl.span.clone(),
                        secondary: None,
                        suggestion: None,
                    });
                }
            }
        }

        // A52002: Check for empty decoder names
        for codec in &cr.codecs {
            if codec.decoder.is_empty() {
                errors.push(TypeError {
                    code: "A52002".into(),
                    message: format!(
                        "codec `{}` in registry `{}` has no decoder function",
                        codec.name, cr.name,
                    ),
                    span: decl.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
    }

    errors
}
