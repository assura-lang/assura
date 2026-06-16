//! Format-related domain checkers.
//!
//! BinaryFormatChecker, BitLevelChecker, StringEncodingChecker,
//! ChecksumChecker, ProtocolGrammarChecker.

use std::collections::HashMap;
use std::ops::Range;

use crate::TypeError;

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

    pub fn check_use_as_string(&self, name: &str, span: &Range<usize>) -> Option<TypeError> {
        match self.variables.get(name) {
            Some(StringEncoding::RawBytes) => Some(TypeError {
                code: "A28001".into(),
                message: format!("`{name}` is raw bytes, not a validated string"),
                span: span.clone(),
                secondary: None,
            }),
            None => Some(TypeError {
                code: "A28001".into(),
                message: format!("`{name}` has unknown encoding, cannot use as string"),
                span: span.clone(),
                secondary: None,
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

    pub fn check_use_before_verify(&self, name: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some(region) = self.regions.get(name)
            && !region.verified
        {
            return Some(TypeError {
                code: "A29001".into(),
                message: format!("data region `{name}` used before checksum verification"),
                span: span.clone(),
                secondary: None,
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
                    });
                }
            }
        }
        errors
    }
}
