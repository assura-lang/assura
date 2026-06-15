//! Domain-specific type checkers.
//!
//! Each checker validates contracts against a specific domain (memory,
//! concurrency, formatting, security, etc.). They are self-contained
//! structs that operate on AST nodes and produce `Vec<TypeError>`.

use std::collections::HashMap;
use std::ops::Range;

use crate::{Type, TypeError};

// ===========================================================================
// T056: MEM.3 Allocator contracts
// ===========================================================================

/// Tracks allocation/deallocation pairing and size constraints.
///
/// Error codes:
/// - A22001: allocation not paired with deallocation
/// - A22002: double free (deallocating already freed allocation)
/// - A22003: size mismatch between allocation and deallocation
/// - A22004: arena lifetime violation (use after arena drop)
#[derive(Debug, Clone)]
pub struct AllocatorChecker {
    allocations: HashMap<std::string::String, AllocInfo>,
    freed: HashMap<std::string::String, Range<usize>>,
    arenas: HashMap<std::string::String, ArenaInfo>,
}

#[derive(Debug, Clone)]
pub struct AllocInfo {
    pub size_expr: std::string::String,
    pub span: Range<usize>,
    pub arena: Option<std::string::String>,
}

#[derive(Debug, Clone)]
pub struct ArenaInfo {
    pub dropped: bool,
    pub drop_span: Option<Range<usize>>,
}

impl AllocatorChecker {
    pub fn new() -> Self {
        Self {
            allocations: HashMap::new(),
            freed: HashMap::new(),
            arenas: HashMap::new(),
        }
    }

    pub fn declare_arena(&mut self, name: std::string::String) {
        self.arenas.insert(
            name,
            ArenaInfo {
                dropped: false,
                drop_span: None,
            },
        );
    }

    pub fn drop_arena(&mut self, name: &str, span: Range<usize>) {
        if let Some(info) = self.arenas.get_mut(name) {
            info.dropped = true;
            info.drop_span = Some(span);
        }
    }

    pub fn record_alloc(
        &mut self,
        name: std::string::String,
        size_expr: std::string::String,
        arena: Option<std::string::String>,
        span: Range<usize>,
    ) {
        self.allocations.insert(
            name,
            AllocInfo {
                size_expr,
                span,
                arena,
            },
        );
    }

    pub fn record_free(&mut self, name: &str, span: Range<usize>) -> Option<TypeError> {
        if self.freed.contains_key(name) {
            return Some(TypeError {
                code: "A22002".into(),
                message: format!("double free: `{name}` already deallocated"),
                span: span.clone(),
                secondary: None,
            });
        }
        self.freed.insert(name.to_string(), span);
        None
    }

    pub fn check_arena_use(&self, alloc_name: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some(info) = self.allocations.get(alloc_name)
            && let Some(arena_name) = &info.arena
            && let Some(arena) = self.arenas.get(arena_name)
            && arena.dropped
        {
            return Some(TypeError {
                code: "A22004".into(),
                message: format!("use of `{alloc_name}` after arena `{arena_name}` dropped"),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_unpaired(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, info) in &self.allocations {
            if !self.freed.contains_key(name) && info.arena.is_none() {
                errors.push(TypeError {
                    code: "A22001".into(),
                    message: format!("allocation `{name}` not paired with deallocation"),
                    span: info.span.clone(),
                    secondary: None,
                });
            }
        }
        errors.sort_by_key(|e| e.span.start);
        errors
    }
}

impl Default for AllocatorChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T057: MEM.4 Circular buffer contracts
// ===========================================================================

/// Checks circular buffer indexing invariants.
///
/// Error codes:
/// - A23001: logical index exceeds buffer capacity
/// - A23002: physical index computation may wrap incorrectly
/// - A23003: buffer empty on read
#[derive(Debug, Clone)]
pub struct CircularBufferChecker {
    pub(crate) buffers: HashMap<std::string::String, CircBufInfo>,
}

#[derive(Debug, Clone)]
pub struct CircBufInfo {
    pub capacity: usize,
    pub head: usize,
    pub tail: usize,
    pub count: usize,
}

impl CircBufInfo {
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    pub fn is_full(&self) -> bool {
        self.count >= self.capacity
    }
    pub fn logical_to_physical(&self, logical: usize) -> usize {
        (self.head + logical) % self.capacity
    }
}

impl CircularBufferChecker {
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
        }
    }

    pub fn declare(&mut self, name: std::string::String, capacity: usize) {
        self.buffers.insert(
            name,
            CircBufInfo {
                capacity,
                head: 0,
                tail: 0,
                count: 0,
            },
        );
    }

    pub fn check_read(&self, name: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some(buf) = self.buffers.get(name)
            && buf.is_empty()
        {
            return Some(TypeError {
                code: "A23003".into(),
                message: format!("read from empty circular buffer `{name}`"),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_index(
        &self,
        name: &str,
        logical_idx: usize,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if let Some(buf) = self.buffers.get(name)
            && logical_idx >= buf.capacity
        {
            return Some(TypeError {
                code: "A23001".into(),
                message: format!(
                    "logical index {logical_idx} exceeds capacity {} of `{name}`",
                    buf.capacity
                ),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_physical_wrap(
        &self,
        name: &str,
        offset: usize,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if let Some(buf) = self.buffers.get(name) {
            if buf.capacity == 0 {
                return Some(TypeError {
                    code: "A23002".into(),
                    message: format!(
                        "circular buffer `{name}` has zero capacity, modular wrap undefined"
                    ),
                    span: span.clone(),
                    secondary: None,
                });
            }
            let _physical = (buf.head + offset) % buf.capacity;
        }
        None
    }

    pub fn push(&mut self, name: &str) {
        if let Some(buf) = self.buffers.get_mut(name)
            && buf.count < buf.capacity
        {
            buf.tail = (buf.tail + 1) % buf.capacity;
            buf.count += 1;
        }
    }

    pub fn pop(&mut self, name: &str) {
        if let Some(buf) = self.buffers.get_mut(name)
            && buf.count > 0
        {
            buf.head = (buf.head + 1) % buf.capacity;
            buf.count -= 1;
        }
    }
}

impl Default for CircularBufferChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T066: CONC.2 Callback re-entrancy prevention
// ===========================================================================

/// Prevents re-entrant calls through callback chains.
///
/// Error codes:
/// - A24001: re-entrant callback invocation detected
/// - A24002: callback registered in non-reentrant context
/// - A24003: unbounded callback depth
#[derive(Debug, Clone)]
pub struct CallbackReentrancyChecker {
    /// Functions currently on the call stack
    call_stack: Vec<std::string::String>,
    /// Functions marked as non-reentrant
    non_reentrant: HashMap<std::string::String, Range<usize>>,
    /// Maximum allowed callback depth
    max_depth: usize,
}

impl CallbackReentrancyChecker {
    pub fn new() -> Self {
        Self {
            call_stack: Vec::new(),
            non_reentrant: HashMap::new(),
            max_depth: 16,
        }
    }

    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    pub fn mark_non_reentrant(&mut self, fn_name: std::string::String, span: Range<usize>) {
        self.non_reentrant.insert(fn_name, span);
    }

    pub fn enter_call(&mut self, fn_name: &str, span: &Range<usize>) -> Vec<TypeError> {
        let mut errors = Vec::new();

        // Check re-entrancy
        if self.call_stack.contains(&fn_name.to_string())
            && self.non_reentrant.contains_key(fn_name)
        {
            errors.push(TypeError {
                code: "A24001".into(),
                message: format!("re-entrant call to non-reentrant function `{fn_name}`"),
                span: span.clone(),
                secondary: None,
            });
        }

        // Check depth
        if self.call_stack.len() >= self.max_depth {
            errors.push(TypeError {
                code: "A24003".into(),
                message: format!(
                    "callback depth {} exceeds maximum {}",
                    self.call_stack.len() + 1,
                    self.max_depth
                ),
                span: span.clone(),
                secondary: None,
            });
        }

        self.call_stack.push(fn_name.to_string());
        errors
    }

    pub fn exit_call(&mut self) {
        self.call_stack.pop();
    }

    pub fn check_register_callback(
        &self,
        target_fn: &str,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if self.non_reentrant.contains_key(target_fn)
            && self.call_stack.contains(&target_fn.to_string())
        {
            return Some(TypeError {
                code: "A24002".into(),
                message: format!(
                    "registering callback to non-reentrant `{target_fn}` while inside it"
                ),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn current_depth(&self) -> usize {
        self.call_stack.len()
    }
}

impl Default for CallbackReentrancyChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T069: CONC.5 Temporal deadlines
// ===========================================================================

/// Enforces bounded response time contracts.
///
/// Error codes:
/// - A25001: operation exceeds declared deadline
/// - A25002: nested deadline violation (inner > outer)
/// - A25003: unbounded operation in deadline context
#[derive(Debug, Clone)]
pub struct TemporalDeadlineChecker {
    /// Active deadline scopes (name -> deadline_ms)
    deadlines: Vec<(std::string::String, u64)>,
    /// Operations with known worst-case times
    operation_bounds: HashMap<std::string::String, u64>,
}

impl TemporalDeadlineChecker {
    pub fn new() -> Self {
        Self {
            deadlines: Vec::new(),
            operation_bounds: HashMap::new(),
        }
    }

    pub fn register_bound(&mut self, op: std::string::String, worst_case_ms: u64) {
        self.operation_bounds.insert(op, worst_case_ms);
    }

    pub fn enter_deadline(
        &mut self,
        name: std::string::String,
        deadline_ms: u64,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        // Check nested deadline doesn't exceed outer
        if let Some((outer_name, outer_ms)) = self.deadlines.last()
            && deadline_ms > *outer_ms
        {
            return Some(TypeError {
                code: "A25002".into(),
                message: format!(
                    "inner deadline `{name}` ({deadline_ms}ms) exceeds outer `{outer_name}` ({outer_ms}ms)"
                ),
                span: span.clone(),
                secondary: None,
            });
        }
        self.deadlines.push((name, deadline_ms));
        None
    }

    pub fn exit_deadline(&mut self) {
        self.deadlines.pop();
    }

    pub fn check_operation(&self, op: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some((deadline_name, deadline_ms)) = self.deadlines.last() {
            if let Some(worst_case) = self.operation_bounds.get(op) {
                if worst_case > deadline_ms {
                    return Some(TypeError {
                        code: "A25001".into(),
                        message: format!(
                            "operation `{op}` worst-case {worst_case}ms exceeds deadline `{deadline_name}` ({deadline_ms}ms)"
                        ),
                        span: span.clone(),
                        secondary: None,
                    });
                }
            } else {
                return Some(TypeError {
                    code: "A25003".into(),
                    message: format!(
                        "unbounded operation `{op}` in deadline context `{deadline_name}`"
                    ),
                    span: span.clone(),
                    secondary: None,
                });
            }
        }
        None
    }

    pub fn current_deadline(&self) -> Option<(&str, u64)> {
        self.deadlines.last().map(|(n, d)| (n.as_str(), *d))
    }
}

impl Default for TemporalDeadlineChecker {
    fn default() -> Self {
        Self::new()
    }
}

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
pub struct BinaryFormatChecker {
    fields: Vec<BinaryField>,
}

#[derive(Debug, Clone)]
pub struct BinaryField {
    pub name: std::string::String,
    pub offset: usize,
    pub size: usize,
    pub endianness: Option<Endianness>,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Endianness {
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
pub struct BitLevelChecker {
    fields: Vec<BitField>,
    container_bits: usize,
}

#[derive(Debug, Clone)]
pub struct BitField {
    pub name: std::string::String,
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
pub enum StringEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    Ascii,
    Latin1,
    RawBytes,
}

#[derive(Debug, Clone)]
pub struct StringEncodingChecker {
    variables: HashMap<std::string::String, StringEncoding>,
}

impl StringEncodingChecker {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
        }
    }

    pub fn declare(&mut self, name: std::string::String, encoding: StringEncoding) {
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
pub enum ChecksumAlgorithm {
    Crc32,
    Adler32,
    Sha256,
    Sha512,
    Md5,
    Custom(std::string::String),
}

#[derive(Debug, Clone)]
pub struct ChecksumChecker {
    /// Data regions and their checksum status
    regions: HashMap<std::string::String, ChecksumRegion>,
}

#[derive(Debug, Clone)]
pub struct ChecksumRegion {
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
        name: std::string::String,
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
pub struct ProtocolGrammarChecker {
    states: Vec<std::string::String>,
    current_state: std::string::String,
    transitions: Vec<ProtocolTransition>,
    required_fields: HashMap<std::string::String, Vec<std::string::String>>,
}

#[derive(Debug, Clone)]
pub struct ProtocolTransition {
    pub from: std::string::String,
    pub to: std::string::String,
    pub message: std::string::String,
}

impl ProtocolGrammarChecker {
    pub fn new(initial_state: std::string::String) -> Self {
        Self {
            states: vec![initial_state.clone()],
            current_state: initial_state,
            transitions: Vec::new(),
            required_fields: HashMap::new(),
        }
    }

    pub fn add_state(&mut self, state: std::string::String) {
        if !self.states.contains(&state) {
            self.states.push(state);
        }
    }

    pub fn add_transition(
        &mut self,
        from: std::string::String,
        to: std::string::String,
        message: std::string::String,
    ) {
        self.transitions
            .push(ProtocolTransition { from, to, message });
    }

    pub fn add_required_fields(
        &mut self,
        message: std::string::String,
        fields: Vec<std::string::String>,
    ) {
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

    pub fn current_state(&self) -> &str {
        &self.current_state
    }
}

// ===========================================================================
// T077: CORE.4 Axiomatic definitions
// ===========================================================================

/// Validates axiomatic (abstract mathematical) definitions.
///
/// Error codes:
/// - A31001: axiom references undefined symbol
/// - A31002: axiom set is inconsistent (circular or contradictory)
/// - A31003: axiom not used in any proof
#[derive(Debug, Clone)]
pub struct AxiomaticDefChecker {
    axioms: HashMap<std::string::String, AxiomDef>,
    used_axioms: Vec<std::string::String>,
}

#[derive(Debug, Clone)]
pub struct AxiomDef {
    pub name: std::string::String,
    pub params: Vec<std::string::String>,
    pub body: std::string::String,
    pub span: Range<usize>,
    pub references: Vec<std::string::String>,
}

impl AxiomaticDefChecker {
    pub fn new() -> Self {
        Self {
            axioms: HashMap::new(),
            used_axioms: Vec::new(),
        }
    }

    pub fn declare_axiom(&mut self, axiom: AxiomDef) {
        self.axioms.insert(axiom.name.clone(), axiom);
    }

    pub fn mark_used(&mut self, name: &str) {
        if !self.used_axioms.contains(&name.to_string()) {
            self.used_axioms.push(name.to_string());
        }
    }

    pub fn check_references(&self, known_symbols: &[&str]) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for axiom in self.axioms.values() {
            for reference in &axiom.references {
                let is_axiom = self.axioms.contains_key(reference);
                let is_known = known_symbols.contains(&reference.as_str());
                if !is_axiom && !is_known {
                    errors.push(TypeError {
                        code: "A31001".into(),
                        message: format!(
                            "axiom `{}` references undefined symbol `{reference}`",
                            axiom.name
                        ),
                        span: axiom.span.clone(),
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_unused(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, axiom) in &self.axioms {
            if !self.used_axioms.contains(name) {
                errors.push(TypeError {
                    code: "A31003".into(),
                    message: format!("axiom `{name}` is never used in any proof"),
                    span: axiom.span.clone(),
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_circular(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, axiom) in &self.axioms {
            if self.has_cycle(name, &mut vec![name.clone()]) {
                errors.push(TypeError {
                    code: "A31002".into(),
                    message: format!("axiom `{name}` has circular dependency"),
                    span: axiom.span.clone(),
                    secondary: None,
                });
            }
        }
        errors
    }

    fn has_cycle(&self, current: &str, visited: &mut Vec<std::string::String>) -> bool {
        if let Some(axiom) = self.axioms.get(current) {
            for reference in &axiom.references {
                if visited.contains(reference) {
                    return true;
                }
                if self.axioms.contains_key(reference) {
                    visited.push(reference.clone());
                    if self.has_cycle(reference, visited) {
                        return true;
                    }
                    visited.pop();
                }
            }
        }
        false
    }
}

impl Default for AxiomaticDefChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T079: CORE.6 Opaque functions
// ===========================================================================

/// Manages opaque function declarations that hide implementation from verifier.
///
/// Error codes:
/// - A32001: opaque function called without contract
/// - A32002: opaque function body accessed during verification
/// - A32003: reveal used outside proof context
#[derive(Debug, Clone)]
pub struct OpaqueFunctionChecker {
    opaque_fns: HashMap<std::string::String, OpaqueFnInfo>,
    revealed: Vec<std::string::String>,
    in_proof_context: bool,
}

#[derive(Debug, Clone)]
pub struct OpaqueFnInfo {
    pub has_contract: bool,
    pub span: Range<usize>,
}

impl OpaqueFunctionChecker {
    pub fn new() -> Self {
        Self {
            opaque_fns: HashMap::new(),
            revealed: Vec::new(),
            in_proof_context: false,
        }
    }

    pub fn declare_opaque(
        &mut self,
        name: std::string::String,
        has_contract: bool,
        span: Range<usize>,
    ) {
        self.opaque_fns
            .insert(name, OpaqueFnInfo { has_contract, span });
    }

    pub fn enter_proof(&mut self) {
        self.in_proof_context = true;
    }

    pub fn exit_proof(&mut self) {
        self.in_proof_context = false;
    }

    pub fn check_call(&self, fn_name: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some(info) = self.opaque_fns.get(fn_name)
            && !info.has_contract
        {
            return Some(TypeError {
                code: "A32001".into(),
                message: format!("opaque function `{fn_name}` called without contract"),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_body_access(&self, fn_name: &str, span: &Range<usize>) -> Option<TypeError> {
        if self.opaque_fns.contains_key(fn_name) && !self.revealed.contains(&fn_name.to_string()) {
            return Some(TypeError {
                code: "A32002".into(),
                message: format!("body of opaque function `{fn_name}` accessed without reveal"),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn reveal(&mut self, fn_name: &str, span: &Range<usize>) -> Option<TypeError> {
        if !self.in_proof_context {
            return Some(TypeError {
                code: "A32003".into(),
                message: format!("`reveal {fn_name}` used outside proof context"),
                span: span.clone(),
                secondary: None,
            });
        }
        self.revealed.push(fn_name.to_string());
        None
    }

    pub fn is_opaque(&self, fn_name: &str) -> bool {
        self.opaque_fns.contains_key(fn_name)
    }
}

impl Default for OpaqueFunctionChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T083: TEST.1 Test generation from contracts
// ===========================================================================

/// Generates property-based and boundary-value tests from contract specs.
///
/// Produces Rust test code (proptest/quickcheck) from requires/ensures clauses.
#[derive(Debug, Clone)]
pub struct TestGenerator {
    contracts: Vec<TestableContract>,
}

#[derive(Debug, Clone)]
pub struct TestableContract {
    pub name: std::string::String,
    pub params: Vec<(std::string::String, Type)>,
    pub requires: Vec<std::string::String>,
    pub ensures: Vec<std::string::String>,
}

#[derive(Debug, Clone)]
pub struct GeneratedTest {
    pub name: std::string::String,
    pub body: std::string::String,
    pub kind: TestKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TestKind {
    Property,
    Boundary,
    Smoke,
}

impl TestGenerator {
    pub fn new() -> Self {
        Self {
            contracts: Vec::new(),
        }
    }

    pub fn add_contract(&mut self, contract: TestableContract) {
        self.contracts.push(contract);
    }

    pub fn generate_property_test(&self, contract: &TestableContract) -> GeneratedTest {
        let param_list: Vec<std::string::String> = contract
            .params
            .iter()
            .map(|(n, t)| format!("{n}: {}", Self::type_to_proptest_strategy(t)))
            .collect();
        let preconditions = if contract.requires.is_empty() {
            String::new()
        } else {
            format!(
                "prop_assume!({});\n        ",
                contract.requires.join(" && ")
            )
        };
        let postconditions = contract.ensures.join(" && ");
        let body = format!(
            "proptest! {{\n    #[test]\n    fn prop_{}({}) {{\n        {preconditions}prop_assert!({postconditions});\n    }}\n}}",
            contract.name,
            param_list.join(", ")
        );
        GeneratedTest {
            name: format!("prop_{}", contract.name),
            body,
            kind: TestKind::Property,
        }
    }

    pub fn generate_boundary_tests(&self, contract: &TestableContract) -> Vec<GeneratedTest> {
        let mut tests = Vec::new();
        for (name, ty) in &contract.params {
            let boundaries = Self::boundary_values(ty);
            for (i, val) in boundaries.iter().enumerate() {
                tests.push(GeneratedTest {
                    name: format!("boundary_{}_{}_{}", contract.name, name, i),
                    body: format!("#[test]\nfn boundary_{}_{}_{i}() {{\n    let {name} = {val};\n    // boundary test for {name}\n}}", contract.name, name),
                    kind: TestKind::Boundary,
                });
            }
        }
        tests
    }

    pub fn generate_smoke_test(&self, contract: &TestableContract) -> GeneratedTest {
        let body = format!(
            "#[test]\nfn smoke_{}() {{\n    // smoke test: basic valid inputs\n}}",
            contract.name
        );
        GeneratedTest {
            name: format!("smoke_{}", contract.name),
            body,
            kind: TestKind::Smoke,
        }
    }

    pub fn generate_all(&self) -> Vec<GeneratedTest> {
        let mut tests = Vec::new();
        for contract in &self.contracts {
            tests.push(self.generate_property_test(contract));
            tests.extend(self.generate_boundary_tests(contract));
            tests.push(self.generate_smoke_test(contract));
        }
        tests
    }

    fn type_to_proptest_strategy(ty: &Type) -> &'static str {
        match ty {
            Type::Int | Type::I64 => "i64::ANY",
            Type::Nat | Type::U64 => "u64::ANY",
            Type::U8 => "u8::ANY",
            Type::U16 => "u16::ANY",
            Type::U32 => "u32::ANY",
            Type::I8 => "i8::ANY",
            Type::I16 => "i16::ANY",
            Type::I32 => "i32::ANY",
            Type::Float | Type::F64 => "f64::ANY",
            Type::F32 => "f32::ANY",
            Type::Bool => "bool::ANY",
            Type::String => "\".*\"",
            _ => "any::<()>()",
        }
    }

    fn boundary_values(ty: &Type) -> Vec<std::string::String> {
        match ty {
            Type::Int | Type::I64 => vec![
                "0".into(),
                "1".into(),
                "-1".into(),
                "i64::MAX".into(),
                "i64::MIN".into(),
            ],
            Type::Nat | Type::U64 => vec!["0".into(), "1".into(), "u64::MAX".into()],
            Type::U8 => vec!["0u8".into(), "1u8".into(), "255u8".into()],
            Type::U16 => vec!["0u16".into(), "1u16".into(), "65535u16".into()],
            Type::U32 => vec!["0u32".into(), "1u32".into(), "u32::MAX".into()],
            Type::I8 => vec![
                "0i8".into(),
                "1i8".into(),
                "-1i8".into(),
                "127i8".into(),
                "-128i8".into(),
            ],
            Type::I16 => vec![
                "0i16".into(),
                "1i16".into(),
                "-1i16".into(),
                "i16::MAX".into(),
                "i16::MIN".into(),
            ],
            Type::I32 => vec![
                "0i32".into(),
                "1i32".into(),
                "-1i32".into(),
                "i32::MAX".into(),
                "i32::MIN".into(),
            ],
            Type::Bool => vec!["true".into(), "false".into()],
            Type::Float | Type::F64 => vec![
                "0.0".into(),
                "1.0".into(),
                "-1.0".into(),
                "f64::INFINITY".into(),
                "f64::NAN".into(),
            ],
            _ => vec![],
        }
    }
}

impl Default for TestGenerator {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------

// ===========================================================================
// T086: STOR.1 Crash recovery contracts
// ===========================================================================

/// Tracks write-ahead log (WAL) discipline and crash-safe commit sequences.
#[derive(Debug, Clone)]
pub struct CrashRecoveryChecker {
    wal_entries: Vec<WalEntry>,
    committed: Vec<std::string::String>,
    fsynced: Vec<std::string::String>,
}

#[derive(Debug, Clone)]
pub struct WalEntry {
    pub id: std::string::String,
    pub data_written: bool,
    pub wal_written: bool,
    pub fsynced: bool,
}

impl CrashRecoveryChecker {
    pub fn new() -> Self {
        Self {
            wal_entries: Vec::new(),
            committed: Vec::new(),
            fsynced: Vec::new(),
        }
    }

    pub fn begin_write(&mut self, id: std::string::String) {
        self.wal_entries.push(WalEntry {
            id,
            data_written: false,
            wal_written: false,
            fsynced: false,
        });
    }

    pub fn write_wal(&mut self, id: &str) {
        if let Some(e) = self.wal_entries.iter_mut().find(|e| e.id == id) {
            e.wal_written = true;
        }
    }

    pub fn write_data(&mut self, id: &str) {
        if let Some(e) = self.wal_entries.iter_mut().find(|e| e.id == id) {
            e.data_written = true;
        }
    }

    pub fn fsync(&mut self, id: &str) {
        if let Some(e) = self.wal_entries.iter_mut().find(|e| e.id == id) {
            e.fsynced = true;
        }
        self.fsynced.push(id.to_string());
    }

    pub fn commit(&mut self, id: &str) {
        self.committed.push(id.to_string());
    }

    pub fn check_write_ahead(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for e in &self.wal_entries {
            if e.data_written && !e.wal_written {
                errors.push(TypeError {
                    code: "A33001".into(),
                    message: format!("data write for `{}` without preceding WAL entry", e.id),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_commit_durability(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for id in &self.committed {
            if !self.fsynced.contains(id) {
                errors.push(TypeError {
                    code: "A33002".into(),
                    message: format!("commit for `{id}` without fsync"),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_ordering(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for e in &self.wal_entries {
            if e.fsynced && !e.data_written {
                errors.push(TypeError {
                    code: "A33003".into(),
                    message: format!("fsync for `{}` before data write", e.id),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_all(&self) -> Vec<TypeError> {
        let mut errs = self.check_write_ahead();
        errs.extend(self.check_commit_durability());
        errs.extend(self.check_ordering());
        errs
    }
}

impl Default for CrashRecoveryChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T087: STOR.2 Page cache contracts
// ===========================================================================

#[derive(Debug, Clone)]
pub struct PageCacheChecker {
    pages: std::collections::HashMap<u64, PageInfo>,
    capacity: usize,
}

#[derive(Debug, Clone)]
pub struct PageInfo {
    pub page_id: u64,
    pub dirty: bool,
    pub pinned: bool,
    pub pin_count: u32,
}

impl PageCacheChecker {
    pub fn new(capacity: usize) -> Self {
        Self {
            pages: std::collections::HashMap::new(),
            capacity,
        }
    }

    pub fn load_page(&mut self, page_id: u64) {
        self.pages.insert(
            page_id,
            PageInfo {
                page_id,
                dirty: false,
                pinned: false,
                pin_count: 0,
            },
        );
    }

    pub fn pin(&mut self, page_id: u64) {
        if let Some(p) = self.pages.get_mut(&page_id) {
            p.pinned = true;
            p.pin_count += 1;
        }
    }

    pub fn unpin(&mut self, page_id: u64) {
        if let Some(p) = self.pages.get_mut(&page_id) {
            if p.pin_count > 0 {
                p.pin_count -= 1;
            }
            if p.pin_count == 0 {
                p.pinned = false;
            }
        }
    }

    pub fn mark_dirty(&mut self, page_id: u64) {
        if let Some(p) = self.pages.get_mut(&page_id) {
            p.dirty = true;
        }
    }

    pub fn flush(&mut self, page_id: u64) {
        if let Some(p) = self.pages.get_mut(&page_id) {
            p.dirty = false;
        }
    }

    pub fn evict(&mut self, page_id: u64) -> Option<TypeError> {
        if let Some(p) = self.pages.get(&page_id) {
            if p.pinned {
                return Some(TypeError {
                    code: "A34001".into(),
                    message: format!("cannot evict pinned page {page_id}"),
                    span: 0..1,
                    secondary: None,
                });
            }
            if p.dirty {
                return Some(TypeError {
                    code: "A34002".into(),
                    message: format!("evicting dirty page {page_id} without flush"),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        self.pages.remove(&page_id);
        None
    }

    pub fn check_capacity(&self) -> Vec<TypeError> {
        if self.pages.len() > self.capacity {
            vec![TypeError {
                code: "A34003".into(),
                message: format!(
                    "page cache size {} exceeds capacity {}",
                    self.pages.len(),
                    self.capacity
                ),
                span: 0..1,
                secondary: None,
            }]
        } else {
            vec![]
        }
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

impl Default for PageCacheChecker {
    fn default() -> Self {
        Self::new(1024)
    }
}

// ===========================================================================
// T088: STOR.3 MVCC / snapshot isolation
// ===========================================================================

#[derive(Debug, Clone)]
pub struct MvccChecker {
    versions: std::collections::HashMap<std::string::String, Vec<MvccVersion>>,
    active_snapshots: Vec<u64>,
    next_txn_id: u64,
}

#[derive(Debug, Clone)]
pub struct MvccVersion {
    pub txn_id: u64,
    pub committed: bool,
}

impl MvccChecker {
    pub fn new() -> Self {
        Self {
            versions: std::collections::HashMap::new(),
            active_snapshots: Vec::new(),
            next_txn_id: 1,
        }
    }

    pub fn begin_txn(&mut self) -> u64 {
        let id = self.next_txn_id;
        self.next_txn_id += 1;
        self.active_snapshots.push(id);
        id
    }

    pub fn write_version(&mut self, key: std::string::String, txn_id: u64) {
        self.versions.entry(key).or_default().push(MvccVersion {
            txn_id,
            committed: false,
        });
    }

    pub fn commit_txn(&mut self, txn_id: u64) {
        self.active_snapshots.retain(|&id| id != txn_id);
        for versions in self.versions.values_mut() {
            for v in versions.iter_mut() {
                if v.txn_id == txn_id {
                    v.committed = true;
                }
            }
        }
    }

    pub fn check_write_conflicts(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (key, versions) in &self.versions {
            let uncommitted: Vec<_> = versions.iter().filter(|v| !v.committed).collect();
            if uncommitted.len() > 1 {
                errors.push(TypeError {
                    code: "A35001".into(),
                    message: format!(
                        "write-write conflict on key `{key}`: {} uncommitted versions",
                        uncommitted.len()
                    ),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_snapshot_read(&self, key: &str, reader_txn: u64) -> Option<TypeError> {
        if let Some(versions) = self.versions.get(key) {
            for v in versions {
                if v.txn_id != reader_txn
                    && !v.committed
                    && self.active_snapshots.contains(&v.txn_id)
                {
                    return Some(TypeError {
                        code: "A35002".into(),
                        message: format!(
                            "snapshot isolation violation: txn {reader_txn} reads uncommitted from txn {} on `{key}`",
                            v.txn_id
                        ),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        None
    }

    pub fn check_phantom(&self, txn_id: u64) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (key, versions) in &self.versions {
            for v in versions {
                if v.txn_id > txn_id && v.committed {
                    errors.push(TypeError { code: "A35003".into(), message: format!("phantom read: txn {txn_id} sees committed version from later txn {} on `{key}`", v.txn_id), span: 0..1, secondary: None });
                }
            }
        }
        errors
    }
}

impl Default for MvccChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T089: STOR.4 Transactional rollback
// ===========================================================================

#[derive(Debug, Clone)]
pub struct RollbackChecker {
    savepoints: Vec<std::string::String>,
    resources_acquired: Vec<std::string::String>,
    rolled_back: bool,
}

impl RollbackChecker {
    pub fn new() -> Self {
        Self {
            savepoints: Vec::new(),
            resources_acquired: Vec::new(),
            rolled_back: false,
        }
    }

    pub fn create_savepoint(&mut self, name: std::string::String) {
        self.savepoints.push(name);
    }

    pub fn acquire_resource(&mut self, name: std::string::String) {
        self.resources_acquired.push(name);
    }

    pub fn release_resource(&mut self, name: &str) {
        self.resources_acquired.retain(|r| r != name);
    }

    pub fn rollback_to(&mut self, savepoint: &str) -> Option<TypeError> {
        if !self.savepoints.contains(&savepoint.to_string()) {
            return Some(TypeError {
                code: "A36001".into(),
                message: format!("rollback to unknown savepoint `{savepoint}`"),
                span: 0..1,
                secondary: None,
            });
        }
        self.rolled_back = true;
        if let Some(pos) = self.savepoints.iter().position(|s| s == savepoint) {
            self.savepoints.truncate(pos + 1);
        }
        None
    }

    pub fn check_resource_leak(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        if self.rolled_back {
            for r in &self.resources_acquired {
                errors.push(TypeError {
                    code: "A36002".into(),
                    message: format!("resource `{r}` not released after rollback"),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_savepoint_nesting(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for sp in &self.savepoints {
            if !seen.insert(sp.clone()) {
                errors.push(TypeError {
                    code: "A36003".into(),
                    message: format!("duplicate savepoint name `{sp}`"),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }
}

impl Default for RollbackChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T090: STOR.5 Monotonic state
// ===========================================================================

#[derive(Debug, Clone)]
pub struct MonotonicStateChecker {
    monotonic_vars: std::collections::HashMap<std::string::String, MonotonicInfo>,
}

#[derive(Debug, Clone)]
pub struct MonotonicInfo {
    pub current_value: i64,
    pub direction: MonotonicDirection,
    pub span: std::ops::Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MonotonicDirection {
    Increasing,
    StrictlyIncreasing,
    Decreasing,
}

impl MonotonicStateChecker {
    pub fn new() -> Self {
        Self {
            monotonic_vars: std::collections::HashMap::new(),
        }
    }

    pub fn declare(
        &mut self,
        name: std::string::String,
        direction: MonotonicDirection,
        initial: i64,
        span: std::ops::Range<usize>,
    ) {
        self.monotonic_vars.insert(
            name,
            MonotonicInfo {
                current_value: initial,
                direction,
                span,
            },
        );
    }

    pub fn update(&mut self, name: &str, new_value: i64) -> Option<TypeError> {
        if let Some(info) = self.monotonic_vars.get_mut(name) {
            let violation = match info.direction {
                MonotonicDirection::Increasing => new_value < info.current_value,
                MonotonicDirection::StrictlyIncreasing => new_value <= info.current_value,
                MonotonicDirection::Decreasing => new_value > info.current_value,
            };
            if violation {
                return Some(TypeError {
                    code: "A37001".into(),
                    message: format!(
                        "monotonicity violation: `{name}` changed from {} to {new_value}",
                        info.current_value
                    ),
                    span: info.span.clone(),
                    secondary: None,
                });
            }
            info.current_value = new_value;
        }
        None
    }

    pub fn check_reset(&self, name: &str) -> Option<TypeError> {
        if self.monotonic_vars.contains_key(name) {
            Some(TypeError {
                code: "A37002".into(),
                message: format!("illegal reset of monotonic variable `{name}`"),
                span: 0..1,
                secondary: None,
            })
        } else {
            None
        }
    }

    pub fn check_access(&self, name: &str) -> Option<TypeError> {
        if !self.monotonic_vars.contains_key(name) {
            Some(TypeError {
                code: "A37003".into(),
                message: format!("access to undeclared monotonic variable `{name}`"),
                span: 0..1,
                secondary: None,
            })
        } else {
            None
        }
    }

    pub fn current_value(&self, name: &str) -> Option<i64> {
        self.monotonic_vars.get(name).map(|i| i.current_value)
    }
}

impl Default for MonotonicStateChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T091: STOR.6 Storage failure model
// ===========================================================================

#[derive(Debug, Clone)]
pub struct StorageFailureChecker {
    failure_modes: Vec<FailureMode>,
    handled_modes: Vec<std::string::String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FailureMode {
    PartialWrite,
    TornPage,
    BitRot,
    DiskFull,
    IoTimeout,
}

impl FailureMode {
    pub fn name(&self) -> &str {
        match self {
            Self::PartialWrite => "partial_write",
            Self::TornPage => "torn_page",
            Self::BitRot => "bit_rot",
            Self::DiskFull => "disk_full",
            Self::IoTimeout => "io_timeout",
        }
    }
}

impl StorageFailureChecker {
    pub fn new() -> Self {
        Self {
            failure_modes: Vec::new(),
            handled_modes: Vec::new(),
        }
    }

    pub fn declare_failure_mode(&mut self, mode: FailureMode) {
        self.failure_modes.push(mode);
    }

    pub fn mark_handled(&mut self, mode_name: &str) {
        if !self.handled_modes.contains(&mode_name.to_string()) {
            self.handled_modes.push(mode_name.to_string());
        }
    }

    pub fn check_unhandled(&self) -> Vec<TypeError> {
        self.failure_modes
            .iter()
            .filter(|m| !self.handled_modes.contains(&m.name().to_string()))
            .map(|m| TypeError {
                code: "A38001".into(),
                message: format!("storage failure mode `{}` has no handler", m.name()),
                span: 0..1,
                secondary: None,
            })
            .collect()
    }

    pub fn check_spurious_handlers(&self) -> Vec<TypeError> {
        let declared: Vec<_> = self
            .failure_modes
            .iter()
            .map(|m| m.name().to_string())
            .collect();
        self.handled_modes
            .iter()
            .filter(|h| !declared.contains(h))
            .map(|h| TypeError {
                code: "A38002".into(),
                message: format!("handler for undeclared failure mode `{h}`"),
                span: 0..1,
                secondary: None,
            })
            .collect()
    }

    pub fn check_critical_coverage(&self) -> Vec<TypeError> {
        let critical = [FailureMode::PartialWrite, FailureMode::TornPage];
        critical
            .iter()
            .filter(|m| {
                self.failure_modes.contains(m)
                    && !self.handled_modes.contains(&m.name().to_string())
            })
            .map(|m| TypeError {
                code: "A38003".into(),
                message: format!("critical failure mode `{}` must have a handler", m.name()),
                span: 0..1,
                secondary: None,
            })
            .collect()
    }

    pub fn failure_count(&self) -> usize {
        self.failure_modes.len()
    }
}

impl Default for StorageFailureChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T095: NUM.1 Numerical precision
// ===========================================================================

#[derive(Debug, Clone)]
pub struct NumericalPrecisionChecker {
    variables: std::collections::HashMap<std::string::String, PrecisionInfo>,
}

#[derive(Debug, Clone)]
pub struct PrecisionInfo {
    pub bits: u32,
    pub min_ulp: f64,
    pub span: std::ops::Range<usize>,
}

impl NumericalPrecisionChecker {
    pub fn new() -> Self {
        Self {
            variables: std::collections::HashMap::new(),
        }
    }

    pub fn declare(
        &mut self,
        name: std::string::String,
        bits: u32,
        min_ulp: f64,
        span: std::ops::Range<usize>,
    ) {
        self.variables.insert(
            name,
            PrecisionInfo {
                bits,
                min_ulp,
                span,
            },
        );
    }

    pub fn check_precision_loss(&self, name: &str, result_bits: u32) -> Option<TypeError> {
        if let Some(info) = self.variables.get(name)
            && result_bits < info.bits
        {
            return Some(TypeError {
                code: "A42001".into(),
                message: format!(
                    "precision loss: `{name}` requires {}-bit but operation produces {result_bits}-bit",
                    info.bits
                ),
                span: info.span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_ulp_bound(&self, name: &str, actual_ulp: f64) -> Option<TypeError> {
        if let Some(info) = self.variables.get(name)
            && actual_ulp > info.min_ulp
        {
            return Some(TypeError {
                code: "A42002".into(),
                message: format!(
                    "ULP violation: `{name}` requires ULP <= {} but got {actual_ulp}",
                    info.min_ulp
                ),
                span: info.span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_cancellation(&self, name: &str, operand_ratio: f64) -> Option<TypeError> {
        if operand_ratio > 0.999
            && let Some(info) = self.variables.get(name)
        {
            return Some(TypeError {
                code: "A42003".into(),
                message: format!(
                    "potential catastrophic cancellation in `{name}` (operand ratio: {operand_ratio})"
                ),
                span: info.span.clone(),
                secondary: None,
            });
        }
        None
    }
}

impl Default for NumericalPrecisionChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T096: NUM.2 Precomputed table verification
// ===========================================================================

#[derive(Debug, Clone)]
pub struct PrecomputedTableChecker {
    tables: Vec<TableDecl>,
}

#[derive(Debug, Clone)]
pub struct TableDecl {
    pub name: std::string::String,
    pub size: usize,
    pub verified_entries: usize,
    pub generator_fn: std::string::String,
    pub span: std::ops::Range<usize>,
}

impl PrecomputedTableChecker {
    pub fn new() -> Self {
        Self { tables: Vec::new() }
    }

    pub fn declare_table(
        &mut self,
        name: std::string::String,
        size: usize,
        generator_fn: std::string::String,
        span: std::ops::Range<usize>,
    ) {
        self.tables.push(TableDecl {
            name,
            size,
            verified_entries: 0,
            generator_fn,
            span,
        });
    }

    pub fn mark_entries_verified(&mut self, name: &str, count: usize) {
        if let Some(t) = self.tables.iter_mut().find(|t| t.name == name) {
            t.verified_entries = count;
        }
    }

    pub fn check_coverage(&self) -> Vec<TypeError> {
        self.tables
            .iter()
            .filter(|t| t.verified_entries < t.size)
            .map(|t| TypeError {
                code: "A43001".into(),
                message: format!(
                    "table `{}` has only {}/{} entries verified",
                    t.name, t.verified_entries, t.size
                ),
                span: t.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_generator(&self) -> Vec<TypeError> {
        self.tables
            .iter()
            .filter(|t| t.generator_fn.is_empty())
            .map(|t| TypeError {
                code: "A43002".into(),
                message: format!("table `{}` has no generator function", t.name),
                span: t.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_non_empty(&self) -> Vec<TypeError> {
        self.tables
            .iter()
            .filter(|t| t.size == 0)
            .map(|t| TypeError {
                code: "A43003".into(),
                message: format!("table `{}` has zero size", t.name),
                span: t.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn table_count(&self) -> usize {
        self.tables.len()
    }
}

impl Default for PrecomputedTableChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T097: PLAT.1 Platform abstraction
// ===========================================================================

#[derive(Debug, Clone)]
pub struct PlatformAbstractionChecker {
    platforms: Vec<std::string::String>,
    abstractions: std::collections::HashMap<std::string::String, Vec<std::string::String>>,
}

impl PlatformAbstractionChecker {
    pub fn new() -> Self {
        Self {
            platforms: Vec::new(),
            abstractions: std::collections::HashMap::new(),
        }
    }

    pub fn add_platform(&mut self, name: std::string::String) {
        if !self.platforms.contains(&name) {
            self.platforms.push(name);
        }
    }

    pub fn declare_abstraction(
        &mut self,
        name: std::string::String,
        supported: Vec<std::string::String>,
    ) {
        self.abstractions.insert(name, supported);
    }

    pub fn check_coverage(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, supported) in &self.abstractions {
            for platform in &self.platforms {
                if !supported.contains(platform) {
                    errors.push(TypeError {
                        code: "A44001".into(),
                        message: format!(
                            "abstraction `{name}` missing impl for platform `{platform}`"
                        ),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_direct_platform_use(&self, used_platform: &str) -> Option<TypeError> {
        if self.platforms.contains(&used_platform.to_string()) {
            Some(TypeError {
                code: "A44002".into(),
                message: format!("direct platform reference `{used_platform}` without abstraction"),
                span: 0..1,
                secondary: None,
            })
        } else {
            None
        }
    }

    pub fn check_unknown_platforms(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, supported) in &self.abstractions {
            for p in supported {
                if !self.platforms.contains(p) {
                    errors.push(TypeError {
                        code: "A44003".into(),
                        message: format!("abstraction `{name}` references unknown platform `{p}`"),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }
}

impl Default for PlatformAbstractionChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T098: PLAT.2 Feature flags
// ===========================================================================

#[derive(Debug, Clone)]
pub struct FeatureFlagChecker {
    flags: std::collections::HashMap<std::string::String, FeatureFlagInfo>,
}

#[derive(Debug, Clone)]
pub struct FeatureFlagInfo {
    pub name: std::string::String,
    pub default_enabled: bool,
    pub used: bool,
    pub conflicts_with: Vec<std::string::String>,
}

impl FeatureFlagChecker {
    pub fn new() -> Self {
        Self {
            flags: std::collections::HashMap::new(),
        }
    }

    pub fn declare(
        &mut self,
        name: std::string::String,
        default_enabled: bool,
        conflicts_with: Vec<std::string::String>,
    ) {
        self.flags.insert(
            name.clone(),
            FeatureFlagInfo {
                name,
                default_enabled,
                used: false,
                conflicts_with,
            },
        );
    }

    pub fn mark_used(&mut self, name: &str) {
        if let Some(f) = self.flags.get_mut(name) {
            f.used = true;
        }
    }

    pub fn check_unused(&self) -> Vec<TypeError> {
        self.flags
            .iter()
            .filter(|(_, i)| !i.used)
            .map(|(n, _)| TypeError {
                code: "A45001".into(),
                message: format!("feature flag `{n}` is declared but never used"),
                span: 0..1,
                secondary: None,
            })
            .collect()
    }

    pub fn check_conflicts(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, info) in &self.flags {
            if info.default_enabled {
                for conflict in &info.conflicts_with {
                    if let Some(other) = self.flags.get(conflict)
                        && other.default_enabled
                    {
                        errors.push(TypeError {
                            code: "A45002".into(),
                            message: format!(
                                "conflicting flags: `{name}` and `{conflict}` both enabled"
                            ),
                            span: 0..1,
                            secondary: None,
                        });
                    }
                }
            }
        }
        errors
    }

    pub fn check_undeclared(&self, flag_name: &str) -> Option<TypeError> {
        if !self.flags.contains_key(flag_name) {
            Some(TypeError {
                code: "A45003".into(),
                message: format!("reference to undeclared feature flag `{flag_name}`"),
                span: 0..1,
                secondary: None,
            })
        } else {
            None
        }
    }
}

impl Default for FeatureFlagChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T099: PLAT.3 Resource limits
// ===========================================================================

#[derive(Debug, Clone)]
pub struct ResourceLimitChecker {
    limits: std::collections::HashMap<std::string::String, ResourceLimit>,
    usage: std::collections::HashMap<std::string::String, u64>,
}

#[derive(Debug, Clone)]
pub struct ResourceLimit {
    pub name: std::string::String,
    pub max_value: u64,
    pub unit: std::string::String,
}

impl ResourceLimitChecker {
    pub fn new() -> Self {
        Self {
            limits: std::collections::HashMap::new(),
            usage: std::collections::HashMap::new(),
        }
    }

    pub fn declare_limit(
        &mut self,
        name: std::string::String,
        max_value: u64,
        unit: std::string::String,
    ) {
        self.limits.insert(
            name.clone(),
            ResourceLimit {
                name: name.clone(),
                max_value,
                unit,
            },
        );
        self.usage.insert(name, 0);
    }

    pub fn record_usage(&mut self, name: &str, amount: u64) {
        if let Some(u) = self.usage.get_mut(name) {
            *u += amount;
        }
    }

    pub fn release_usage(&mut self, name: &str, amount: u64) {
        if let Some(u) = self.usage.get_mut(name) {
            *u = u.saturating_sub(amount);
        }
    }

    pub fn check_limits(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, limit) in &self.limits {
            if let Some(&current) = self.usage.get(name)
                && current > limit.max_value
            {
                errors.push(TypeError {
                    code: "A46001".into(),
                    message: format!(
                        "resource `{name}` usage {current} exceeds limit {} {}",
                        limit.max_value, limit.unit
                    ),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_unbounded(&self, name: &str) -> Option<TypeError> {
        if !self.limits.contains_key(name) {
            Some(TypeError {
                code: "A46002".into(),
                message: format!("resource `{name}` used without declared limit"),
                span: 0..1,
                secondary: None,
            })
        } else {
            None
        }
    }

    pub fn check_near_limit(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, limit) in &self.limits {
            if let Some(&current) = self.usage.get(name)
                && limit.max_value > 0
                && current > limit.max_value * 9 / 10
            {
                errors.push(TypeError {
                    code: "A46003".into(),
                    message: format!(
                        "resource `{name}` at {}% of limit",
                        current * 100 / limit.max_value
                    ),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn current_usage(&self, name: &str) -> Option<u64> {
        self.usage.get(name).copied()
    }
}

impl Default for ResourceLimitChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T100: PERF.1 Unsafe escape with proof
// ===========================================================================

#[derive(Debug, Clone)]
pub struct UnsafeEscapeChecker {
    unsafe_blocks: Vec<UnsafeBlock>,
}

#[derive(Debug, Clone)]
pub struct UnsafeBlock {
    pub name: std::string::String,
    pub has_safety_proof: bool,
    pub proof_obligations: Vec<std::string::String>,
    pub obligations_discharged: Vec<std::string::String>,
    pub span: std::ops::Range<usize>,
}

impl UnsafeEscapeChecker {
    pub fn new() -> Self {
        Self {
            unsafe_blocks: Vec::new(),
        }
    }

    pub fn declare_unsafe(
        &mut self,
        name: std::string::String,
        obligations: Vec<std::string::String>,
        span: std::ops::Range<usize>,
    ) {
        self.unsafe_blocks.push(UnsafeBlock {
            name,
            has_safety_proof: false,
            proof_obligations: obligations,
            obligations_discharged: Vec::new(),
            span,
        });
    }

    pub fn attach_proof(&mut self, name: &str) {
        if let Some(b) = self.unsafe_blocks.iter_mut().find(|b| b.name == name) {
            b.has_safety_proof = true;
        }
    }

    pub fn discharge_obligation(&mut self, block_name: &str, obligation: std::string::String) {
        if let Some(b) = self.unsafe_blocks.iter_mut().find(|b| b.name == block_name) {
            b.obligations_discharged.push(obligation);
        }
    }

    pub fn check_unproven(&self) -> Vec<TypeError> {
        self.unsafe_blocks
            .iter()
            .filter(|b| !b.has_safety_proof)
            .map(|b| TypeError {
                code: "A47001".into(),
                message: format!("unsafe block `{}` has no safety proof", b.name),
                span: b.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_obligations(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for b in &self.unsafe_blocks {
            for obl in &b.proof_obligations {
                if !b.obligations_discharged.contains(obl) {
                    errors.push(TypeError {
                        code: "A47002".into(),
                        message: format!(
                            "obligation `{obl}` in unsafe block `{}` not discharged",
                            b.name
                        ),
                        span: b.span.clone(),
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_empty_obligations(&self) -> Vec<TypeError> {
        self.unsafe_blocks
            .iter()
            .filter(|b| b.proof_obligations.is_empty())
            .map(|b| TypeError {
                code: "A47003".into(),
                message: format!("unsafe block `{}` declares no proof obligations", b.name),
                span: b.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn unsafe_count(&self) -> usize {
        self.unsafe_blocks.len()
    }
}

impl Default for UnsafeEscapeChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T101: PERF.2 Complexity bounds (AARA)
// ===========================================================================

#[derive(Debug, Clone)]
pub struct ComplexityBoundChecker {
    bounds: std::collections::HashMap<std::string::String, ComplexityBound>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ComplexityClass {
    Constant,
    Logarithmic,
    Linear,
    NLogN,
    Quadratic,
    Cubic,
    Exponential,
}

#[derive(Debug, Clone)]
pub struct ComplexityBound {
    pub fn_name: std::string::String,
    pub declared: ComplexityClass,
    pub measured: Option<ComplexityClass>,
    pub span: std::ops::Range<usize>,
}

impl ComplexityBoundChecker {
    pub fn new() -> Self {
        Self {
            bounds: std::collections::HashMap::new(),
        }
    }

    pub fn declare_bound(
        &mut self,
        fn_name: std::string::String,
        declared: ComplexityClass,
        span: std::ops::Range<usize>,
    ) {
        self.bounds.insert(
            fn_name.clone(),
            ComplexityBound {
                fn_name,
                declared,
                measured: None,
                span,
            },
        );
    }

    pub fn record_measured(&mut self, fn_name: &str, measured: ComplexityClass) {
        if let Some(b) = self.bounds.get_mut(fn_name) {
            b.measured = Some(measured);
        }
    }

    fn class_rank(c: &ComplexityClass) -> u8 {
        match c {
            ComplexityClass::Constant => 0,
            ComplexityClass::Logarithmic => 1,
            ComplexityClass::Linear => 2,
            ComplexityClass::NLogN => 3,
            ComplexityClass::Quadratic => 4,
            ComplexityClass::Cubic => 5,
            ComplexityClass::Exponential => 6,
        }
    }

    pub fn check_bounds(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, bound) in &self.bounds {
            if let Some(ref measured) = bound.measured
                && Self::class_rank(measured) > Self::class_rank(&bound.declared)
            {
                errors.push(TypeError {
                    code: "A48001".into(),
                    message: format!(
                        "function `{name}` declared as {:?} but measured as {measured:?}",
                        bound.declared
                    ),
                    span: bound.span.clone(),
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_unverified(&self) -> Vec<TypeError> {
        self.bounds
            .iter()
            .filter(|(_, b)| b.measured.is_none())
            .map(|(n, b)| TypeError {
                code: "A48002".into(),
                message: format!("complexity bound for `{n}` is not verified"),
                span: b.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_expensive(&self) -> Vec<TypeError> {
        self.bounds
            .iter()
            .filter(|(_, b)| b.declared == ComplexityClass::Exponential)
            .map(|(n, b)| TypeError {
                code: "A48003".into(),
                message: format!("function `{n}` has exponential complexity bound"),
                span: b.span.clone(),
                secondary: None,
            })
            .collect()
    }
}

impl Default for ComplexityBoundChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T102: TEST.2 Behavioral equivalence
// ===========================================================================

#[derive(Debug, Clone)]
pub struct BehavioralEquivalenceChecker {
    equivalences: Vec<EquivalenceDecl>,
}

#[derive(Debug, Clone)]
pub struct EquivalenceDecl {
    pub name: std::string::String,
    pub impl_a: std::string::String,
    pub impl_b: std::string::String,
    pub contract: std::string::String,
    pub verified: bool,
    pub span: std::ops::Range<usize>,
}

impl BehavioralEquivalenceChecker {
    pub fn new() -> Self {
        Self {
            equivalences: Vec::new(),
        }
    }

    pub fn declare(
        &mut self,
        name: std::string::String,
        impl_a: std::string::String,
        impl_b: std::string::String,
        contract: std::string::String,
        span: std::ops::Range<usize>,
    ) {
        self.equivalences.push(EquivalenceDecl {
            name,
            impl_a,
            impl_b,
            contract,
            verified: false,
            span,
        });
    }

    pub fn mark_verified(&mut self, name: &str) {
        if let Some(e) = self.equivalences.iter_mut().find(|e| e.name == name) {
            e.verified = true;
        }
    }

    pub fn check_unverified(&self) -> Vec<TypeError> {
        self.equivalences
            .iter()
            .filter(|e| !e.verified)
            .map(|e| TypeError {
                code: "A49001".into(),
                message: format!(
                    "behavioral equivalence `{}` between `{}` and `{}` not verified",
                    e.name, e.impl_a, e.impl_b
                ),
                span: e.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_self_equivalence(&self) -> Vec<TypeError> {
        self.equivalences
            .iter()
            .filter(|e| e.impl_a == e.impl_b)
            .map(|e| TypeError {
                code: "A49002".into(),
                message: format!(
                    "trivial self-equivalence in `{}`: both sides are `{}`",
                    e.name, e.impl_a
                ),
                span: e.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_contract_ref(&self) -> Vec<TypeError> {
        self.equivalences
            .iter()
            .filter(|e| e.contract.is_empty())
            .map(|e| TypeError {
                code: "A49003".into(),
                message: format!("equivalence `{}` has no contract reference", e.name),
                span: e.span.clone(),
                secondary: None,
            })
            .collect()
    }
}

impl Default for BehavioralEquivalenceChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T103: TEST.3 Multi-pass refinement
// ===========================================================================

#[derive(Debug, Clone)]
pub struct MultiPassRefinementChecker {
    passes: Vec<RefinementPass>,
}

#[derive(Debug, Clone)]
pub struct RefinementPass {
    pub name: std::string::String,
    pub from_level: std::string::String,
    pub to_level: std::string::String,
    pub obligations_total: usize,
    pub obligations_discharged: usize,
    pub span: std::ops::Range<usize>,
}

impl MultiPassRefinementChecker {
    pub fn new() -> Self {
        Self { passes: Vec::new() }
    }

    pub fn add_pass(
        &mut self,
        name: std::string::String,
        from_level: std::string::String,
        to_level: std::string::String,
        obligations: usize,
        span: std::ops::Range<usize>,
    ) {
        self.passes.push(RefinementPass {
            name,
            from_level,
            to_level,
            obligations_total: obligations,
            obligations_discharged: 0,
            span,
        });
    }

    pub fn discharge(&mut self, pass_name: &str, count: usize) {
        if let Some(p) = self.passes.iter_mut().find(|p| p.name == pass_name) {
            p.obligations_discharged += count;
        }
    }

    pub fn check_complete(&self) -> Vec<TypeError> {
        self.passes
            .iter()
            .filter(|p| p.obligations_discharged < p.obligations_total)
            .map(|p| TypeError {
                code: "A50001".into(),
                message: format!(
                    "refinement `{}` ({} -> {}): {}/{} obligations discharged",
                    p.name, p.from_level, p.to_level, p.obligations_discharged, p.obligations_total
                ),
                span: p.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_chain(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for i in 1..self.passes.len() {
            if self.passes[i].from_level != self.passes[i - 1].to_level {
                errors.push(TypeError {
                    code: "A50002".into(),
                    message: format!(
                        "refinement chain gap: `{}` starts at `{}` but `{}` ends at `{}`",
                        self.passes[i].name,
                        self.passes[i].from_level,
                        self.passes[i - 1].name,
                        self.passes[i - 1].to_level
                    ),
                    span: self.passes[i].span.clone(),
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_non_trivial(&self) -> Vec<TypeError> {
        self.passes
            .iter()
            .filter(|p| p.obligations_total == 0)
            .map(|p| TypeError {
                code: "A50003".into(),
                message: format!("refinement pass `{}` has zero obligations", p.name),
                span: p.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn pass_count(&self) -> usize {
        self.passes.len()
    }
}

impl Default for MultiPassRefinementChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T104: MISC.1 Incremental contracts
// ===========================================================================

#[derive(Debug, Clone)]
pub struct IncrementalContractChecker {
    contracts: std::collections::HashMap<std::string::String, ContractHistoryEntry>,
}

#[derive(Debug, Clone)]
pub struct ContractHistoryEntry {
    pub name: std::string::String,
    pub versions: Vec<ContractVersionEntry>,
}

#[derive(Debug, Clone)]
pub struct ContractVersionEntry {
    pub version: u32,
    pub requires_count: usize,
    pub ensures_count: usize,
}

impl IncrementalContractChecker {
    pub fn new() -> Self {
        Self {
            contracts: std::collections::HashMap::new(),
        }
    }

    pub fn add_version(
        &mut self,
        name: std::string::String,
        version: u32,
        requires_count: usize,
        ensures_count: usize,
    ) {
        let history = self
            .contracts
            .entry(name.clone())
            .or_insert_with(|| ContractHistoryEntry {
                name,
                versions: Vec::new(),
            });
        history.versions.push(ContractVersionEntry {
            version,
            requires_count,
            ensures_count,
        });
    }

    pub fn check_precondition_weakening(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, history) in &self.contracts {
            for i in 1..history.versions.len() {
                if history.versions[i].requires_count > history.versions[i - 1].requires_count {
                    errors.push(TypeError {
                        code: "A51001".into(),
                        message: format!(
                            "contract `{name}` v{} strengthens preconditions",
                            history.versions[i].version
                        ),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_postcondition_strengthening(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, history) in &self.contracts {
            for i in 1..history.versions.len() {
                if history.versions[i].ensures_count < history.versions[i - 1].ensures_count {
                    errors.push(TypeError {
                        code: "A51002".into(),
                        message: format!(
                            "contract `{name}` v{} weakens postconditions",
                            history.versions[i].version
                        ),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_version_continuity(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, history) in &self.contracts {
            for i in 1..history.versions.len() {
                if history.versions[i].version != history.versions[i - 1].version + 1 {
                    errors.push(TypeError {
                        code: "A51003".into(),
                        message: format!(
                            "contract `{name}` has version gap: v{} to v{}",
                            history.versions[i - 1].version,
                            history.versions[i].version
                        ),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }
}

impl Default for IncrementalContractChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T105: MISC.2 Scoped invariant suspension
// ===========================================================================

#[derive(Debug, Clone)]
pub struct ScopedInvariantChecker {
    invariants: std::collections::HashMap<std::string::String, InvariantState>,
    suspension_depth: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InvariantState {
    Active,
    Suspended,
    Restored,
}

impl ScopedInvariantChecker {
    pub fn new() -> Self {
        Self {
            invariants: std::collections::HashMap::new(),
            suspension_depth: 0,
        }
    }

    pub fn declare_invariant(&mut self, name: std::string::String) {
        self.invariants.insert(name, InvariantState::Active);
    }

    pub fn suspend(&mut self, name: &str) -> Option<TypeError> {
        if let Some(state) = self.invariants.get_mut(name) {
            if *state == InvariantState::Suspended {
                return Some(TypeError {
                    code: "A52001".into(),
                    message: format!("invariant `{name}` is already suspended"),
                    span: 0..1,
                    secondary: None,
                });
            }
            *state = InvariantState::Suspended;
            self.suspension_depth += 1;
            None
        } else {
            Some(TypeError {
                code: "A52002".into(),
                message: format!("cannot suspend undeclared invariant `{name}`"),
                span: 0..1,
                secondary: None,
            })
        }
    }

    pub fn restore(&mut self, name: &str) -> Option<TypeError> {
        if let Some(state) = self.invariants.get_mut(name) {
            if *state != InvariantState::Suspended {
                return Some(TypeError {
                    code: "A52003".into(),
                    message: format!("invariant `{name}` is not currently suspended"),
                    span: 0..1,
                    secondary: None,
                });
            }
            *state = InvariantState::Restored;
            if self.suspension_depth > 0 {
                self.suspension_depth -= 1;
            }
            None
        } else {
            None
        }
    }

    pub fn check_all_restored(&self) -> Vec<TypeError> {
        self.invariants
            .iter()
            .filter(|(_, s)| **s == InvariantState::Suspended)
            .map(|(n, _)| TypeError {
                code: "A52001".into(),
                message: format!("invariant `{n}` still suspended at scope exit"),
                span: 0..1,
                secondary: None,
            })
            .collect()
    }

    pub fn is_suspended(&self, name: &str) -> bool {
        self.invariants.get(name) == Some(&InvariantState::Suspended)
    }

    pub fn suspension_depth(&self) -> u32 {
        self.suspension_depth
    }
}

impl Default for ScopedInvariantChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T107: Core standard library types
// ===========================================================================

/// Core standard library type definitions (Pos, NonNeg, Email, Uuid, etc.)
#[derive(Debug, Clone)]
pub struct StdlibTypes {
    types: std::collections::HashMap<std::string::String, StdlibTypeDef>,
}

#[derive(Debug, Clone)]
pub struct StdlibTypeDef {
    pub name: std::string::String,
    pub base_type: Type,
    pub refinement: std::string::String,
    pub description: std::string::String,
}

impl StdlibTypes {
    pub fn new() -> Self {
        let mut types = std::collections::HashMap::new();
        types.insert(
            "Pos".into(),
            StdlibTypeDef {
                name: "Pos".into(),
                base_type: Type::Int,
                refinement: "v > 0".into(),
                description: "Positive integer".into(),
            },
        );
        types.insert(
            "NonNeg".into(),
            StdlibTypeDef {
                name: "NonNeg".into(),
                base_type: Type::Int,
                refinement: "v >= 0".into(),
                description: "Non-negative integer".into(),
            },
        );
        types.insert(
            "Email".into(),
            StdlibTypeDef {
                name: "Email".into(),
                base_type: Type::String,
                refinement: "contains(v, @)".into(),
                description: "Email address".into(),
            },
        );
        types.insert(
            "Uuid".into(),
            StdlibTypeDef {
                name: "Uuid".into(),
                base_type: Type::String,
                refinement: "len(v) == 36".into(),
                description: "UUID string".into(),
            },
        );
        types.insert(
            "Port".into(),
            StdlibTypeDef {
                name: "Port".into(),
                base_type: Type::Int,
                refinement: "v >= 0 && v <= 65535".into(),
                description: "Network port".into(),
            },
        );
        types.insert(
            "Percentage".into(),
            StdlibTypeDef {
                name: "Percentage".into(),
                base_type: Type::Float,
                refinement: "v >= 0.0 && v <= 100.0".into(),
                description: "Percentage value".into(),
            },
        );
        Self { types }
    }

    pub fn lookup(&self, name: &str) -> Option<&StdlibTypeDef> {
        self.types.get(name)
    }

    pub fn all_types(&self) -> Vec<&StdlibTypeDef> {
        self.types.values().collect()
    }

    pub fn type_count(&self) -> usize {
        self.types.len()
    }

    pub fn is_stdlib_type(&self, name: &str) -> bool {
        self.types.contains_key(name)
    }
}

impl Default for StdlibTypes {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T108: Collection contracts (ListOps, sort, filter)
// ===========================================================================

/// Standard collection operation contracts.
#[derive(Debug, Clone)]
pub struct CollectionContracts {
    contracts: Vec<CollectionContract>,
}

#[derive(Debug, Clone)]
pub struct CollectionContract {
    pub name: std::string::String,
    pub collection_type: std::string::String,
    pub preconditions: Vec<std::string::String>,
    pub postconditions: Vec<std::string::String>,
    pub preserves_length: bool,
    pub preserves_elements: bool,
}

impl CollectionContracts {
    pub fn new() -> Self {
        let contracts = vec![
            CollectionContract {
                name: "sort".into(),
                collection_type: "List<T>".into(),
                preconditions: vec![],
                postconditions: vec![
                    "is_sorted(result)".into(),
                    "len(result) == len(input)".into(),
                ],
                preserves_length: true,
                preserves_elements: true,
            },
            CollectionContract {
                name: "filter".into(),
                collection_type: "List<T>".into(),
                preconditions: vec![],
                postconditions: vec![
                    "len(result) <= len(input)".into(),
                    "forall x in result: pred(x)".into(),
                ],
                preserves_length: false,
                preserves_elements: true,
            },
            CollectionContract {
                name: "map".into(),
                collection_type: "List<T>".into(),
                preconditions: vec![],
                postconditions: vec!["len(result) == len(input)".into()],
                preserves_length: true,
                preserves_elements: false,
            },
            CollectionContract {
                name: "reverse".into(),
                collection_type: "List<T>".into(),
                preconditions: vec![],
                postconditions: vec![
                    "len(result) == len(input)".into(),
                    "result[0] == input[len(input)-1]".into(),
                ],
                preserves_length: true,
                preserves_elements: true,
            },
            CollectionContract {
                name: "deduplicate".into(),
                collection_type: "List<T>".into(),
                preconditions: vec![],
                postconditions: vec![
                    "len(result) <= len(input)".into(),
                    "all_unique(result)".into(),
                ],
                preserves_length: false,
                preserves_elements: true,
            },
        ];
        Self { contracts }
    }

    pub fn lookup(&self, name: &str) -> Option<&CollectionContract> {
        self.contracts.iter().find(|c| c.name == name)
    }

    pub fn all_contracts(&self) -> &[CollectionContract] {
        &self.contracts
    }

    pub fn contract_count(&self) -> usize {
        self.contracts.len()
    }
}

impl Default for CollectionContracts {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T109: CRUD patterns and auth contracts
// ===========================================================================

/// Standard CRUD and authentication contract patterns.
#[derive(Debug, Clone)]
pub struct CrudAuthContracts {
    crud_ops: Vec<CrudOperation>,
    auth_policies: Vec<AuthPolicy>,
}

#[derive(Debug, Clone)]
pub struct CrudOperation {
    pub name: std::string::String,
    pub op_type: CrudType,
    pub requires_auth: bool,
    pub preconditions: Vec<std::string::String>,
    pub postconditions: Vec<std::string::String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CrudType {
    Create,
    Read,
    Update,
    Delete,
}

#[derive(Debug, Clone)]
pub struct AuthPolicy {
    pub name: std::string::String,
    pub required_role: std::string::String,
    pub allow_self: bool,
}

impl CrudAuthContracts {
    pub fn new() -> Self {
        Self {
            crud_ops: Vec::new(),
            auth_policies: Vec::new(),
        }
    }

    pub fn add_crud(&mut self, name: std::string::String, op_type: CrudType, requires_auth: bool) {
        self.crud_ops.push(CrudOperation {
            name,
            op_type,
            requires_auth,
            preconditions: Vec::new(),
            postconditions: Vec::new(),
        });
    }

    pub fn add_auth_policy(
        &mut self,
        name: std::string::String,
        required_role: std::string::String,
        allow_self: bool,
    ) {
        self.auth_policies.push(AuthPolicy {
            name,
            required_role,
            allow_self,
        });
    }

    pub fn check_auth_coverage(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for op in &self.crud_ops {
            if op.requires_auth {
                let has_policy = self.auth_policies.iter().any(|p| p.name == op.name);
                if !has_policy {
                    errors.push(TypeError {
                        code: "A53001".into(),
                        message: format!(
                            "CRUD operation `{}` requires auth but has no policy",
                            op.name
                        ),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_delete_protection(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for op in &self.crud_ops {
            if op.op_type == CrudType::Delete && !op.requires_auth {
                errors.push(TypeError {
                    code: "A53002".into(),
                    message: format!(
                        "delete operation `{}` should require authentication",
                        op.name
                    ),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn crud_count(&self) -> usize {
        self.crud_ops.len()
    }
    pub fn policy_count(&self) -> usize {
        self.auth_policies.len()
    }
}

impl Default for CrudAuthContracts {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T110: Contract composition with extends
// ===========================================================================

/// Tracks contract inheritance/composition via extends.
#[derive(Debug, Clone)]
pub struct ContractCompositionChecker {
    contracts: std::collections::HashMap<std::string::String, ComposableContract>,
}

#[derive(Debug, Clone)]
pub struct ComposableContract {
    pub name: std::string::String,
    pub extends: Vec<std::string::String>,
    pub own_clauses: usize,
}

impl ContractCompositionChecker {
    pub fn new() -> Self {
        Self {
            contracts: std::collections::HashMap::new(),
        }
    }

    pub fn declare(
        &mut self,
        name: std::string::String,
        extends: Vec<std::string::String>,
        own_clauses: usize,
    ) {
        self.contracts.insert(
            name.clone(),
            ComposableContract {
                name,
                extends,
                own_clauses,
            },
        );
    }

    /// Check that all extended contracts exist.
    pub fn check_extends(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, contract) in &self.contracts {
            for parent in &contract.extends {
                if !self.contracts.contains_key(parent) {
                    errors.push(TypeError {
                        code: "A54001".into(),
                        message: format!("contract `{name}` extends unknown contract `{parent}`"),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    /// Check for circular extends.
    pub fn check_circular(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for name in self.contracts.keys() {
            let mut visited = vec![name.clone()];
            if self.has_extends_cycle(name, &mut visited) {
                errors.push(TypeError {
                    code: "A54002".into(),
                    message: format!("circular extends chain involving `{name}`"),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    fn has_extends_cycle(&self, current: &str, visited: &mut Vec<std::string::String>) -> bool {
        if let Some(contract) = self.contracts.get(current) {
            for parent in &contract.extends {
                if visited.contains(parent) {
                    return true;
                }
                visited.push(parent.clone());
                if self.has_extends_cycle(parent, visited) {
                    return true;
                }
                visited.pop();
            }
        }
        false
    }

    /// Check for diamond inheritance (same contract extended via two paths).
    pub fn check_diamond(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, contract) in &self.contracts {
            let mut all_ancestors = Vec::new();
            for parent in &contract.extends {
                let ancestors = self.collect_ancestors(parent);
                for a in &ancestors {
                    if all_ancestors.contains(a) {
                        errors.push(TypeError {
                            code: "A54003".into(),
                            message: format!(
                                "diamond inheritance in `{name}`: `{a}` reached via multiple paths"
                            ),
                            span: 0..1,
                            secondary: None,
                        });
                    }
                }
                all_ancestors.extend(ancestors);
            }
        }
        errors
    }

    fn collect_ancestors(&self, name: &str) -> Vec<std::string::String> {
        let mut result = vec![name.to_string()];
        if let Some(c) = self.contracts.get(name) {
            for parent in &c.extends {
                result.extend(self.collect_ancestors(parent));
            }
        }
        result
    }

    pub fn contract_count(&self) -> usize {
        self.contracts.len()
    }
}

impl Default for ContractCompositionChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T111: Contract libraries as publishable packages
// ===========================================================================

/// Tracks contract library packaging metadata.
#[derive(Debug, Clone)]
pub struct ContractLibraryChecker {
    libraries: Vec<ContractLibrary>,
}

#[derive(Debug, Clone)]
pub struct ContractLibrary {
    pub name: std::string::String,
    pub version: std::string::String,
    pub exported_contracts: Vec<std::string::String>,
    pub dependencies: Vec<LibraryDep>,
}

#[derive(Debug, Clone)]
pub struct LibraryDep {
    pub name: std::string::String,
    pub version_req: std::string::String,
}

impl ContractLibraryChecker {
    pub fn new() -> Self {
        Self {
            libraries: Vec::new(),
        }
    }

    pub fn declare_library(&mut self, name: std::string::String, version: std::string::String) {
        self.libraries.push(ContractLibrary {
            name,
            version,
            exported_contracts: Vec::new(),
            dependencies: Vec::new(),
        });
    }

    pub fn add_export(&mut self, lib_name: &str, contract: std::string::String) {
        if let Some(lib) = self.libraries.iter_mut().find(|l| l.name == lib_name) {
            lib.exported_contracts.push(contract);
        }
    }

    pub fn add_dependency(&mut self, lib_name: &str, dep: LibraryDep) {
        if let Some(lib) = self.libraries.iter_mut().find(|l| l.name == lib_name) {
            lib.dependencies.push(dep);
        }
    }

    /// Check for libraries with no exports.
    pub fn check_empty_exports(&self) -> Vec<TypeError> {
        self.libraries
            .iter()
            .filter(|l| l.exported_contracts.is_empty())
            .map(|l| TypeError {
                code: "A55001".into(),
                message: format!("library `{}` has no exported contracts", l.name),
                span: 0..1,
                secondary: None,
            })
            .collect()
    }

    /// Check for circular dependencies.
    pub fn check_circular_deps(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for lib in &self.libraries {
            for dep in &lib.dependencies {
                if dep.name == lib.name {
                    errors.push(TypeError {
                        code: "A55002".into(),
                        message: format!("library `{}` depends on itself", lib.name),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    /// Check for duplicate library names.
    pub fn check_duplicates(&self) -> Vec<TypeError> {
        let mut seen = std::collections::HashSet::new();
        let mut errors = Vec::new();
        for lib in &self.libraries {
            if !seen.insert(lib.name.clone()) {
                errors.push(TypeError {
                    code: "A55003".into(),
                    message: format!("duplicate library name `{}`", lib.name),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn library_count(&self) -> usize {
        self.libraries.len()
    }
}

impl Default for ContractLibraryChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // AllocatorChecker
    // -----------------------------------------------------------------------

    #[test]
    fn alloc_unpaired_detected() {
        let mut ac = AllocatorChecker::new();
        ac.record_alloc("buf".into(), "1024".into(), None, 0..10);
        let errors = ac.check_unpaired();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A22001");
    }

    #[test]
    fn alloc_paired_ok() {
        let mut ac = AllocatorChecker::new();
        ac.record_alloc("buf".into(), "1024".into(), None, 0..10);
        assert!(ac.record_free("buf", 10..20).is_none());
        assert!(ac.check_unpaired().is_empty());
    }

    #[test]
    fn alloc_double_free() {
        let mut ac = AllocatorChecker::new();
        ac.record_alloc("buf".into(), "1024".into(), None, 0..10);
        assert!(ac.record_free("buf", 10..20).is_none());
        let err = ac.record_free("buf", 20..30).unwrap();
        assert_eq!(err.code, "A22002");
    }

    #[test]
    fn alloc_arena_use_after_drop() {
        let mut ac = AllocatorChecker::new();
        ac.declare_arena("pool".into());
        ac.record_alloc("buf".into(), "64".into(), Some("pool".into()), 0..10);
        ac.drop_arena("pool", 10..20);
        let err = ac.check_arena_use("buf", &(20..30)).unwrap();
        assert_eq!(err.code, "A22004");
    }

    #[test]
    fn alloc_arena_no_error_before_drop() {
        let mut ac = AllocatorChecker::new();
        ac.declare_arena("pool".into());
        ac.record_alloc("buf".into(), "64".into(), Some("pool".into()), 0..10);
        assert!(ac.check_arena_use("buf", &(5..15)).is_none());
    }

    #[test]
    fn alloc_arena_skips_unpaired() {
        let mut ac = AllocatorChecker::new();
        ac.declare_arena("pool".into());
        ac.record_alloc("buf".into(), "64".into(), Some("pool".into()), 0..10);
        // Arena allocs don't need explicit free
        assert!(ac.check_unpaired().is_empty());
    }

    // -----------------------------------------------------------------------
    // CircularBufferChecker
    // -----------------------------------------------------------------------

    #[test]
    fn circ_buf_basic() {
        let mut cb = CircularBufferChecker::new();
        cb.declare("ring".into(), 16);
        let err = cb.check_index("ring", 15, &(10..20));
        assert!(err.is_none());
    }

    #[test]
    fn circ_buf_index_exceeds_capacity() {
        let mut cb = CircularBufferChecker::new();
        cb.declare("ring".into(), 16);
        let err = cb.check_index("ring", 20, &(10..20));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A23001");
    }

    #[test]
    fn circ_buf_empty_read() {
        let mut cb = CircularBufferChecker::new();
        cb.declare("ring".into(), 16);
        let err = cb.check_read("ring", &(10..20));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A23003");
    }

    // -----------------------------------------------------------------------
    // PlatformAbstractionChecker
    // -----------------------------------------------------------------------

    #[test]
    fn platform_missing_coverage() {
        let mut pac = PlatformAbstractionChecker::new();
        pac.add_platform("linux".into());
        pac.add_platform("windows".into());
        pac.declare_abstraction("fs_ops".into(), vec!["linux".into()]);
        let errors = pac.check_coverage();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A44001");
        assert!(errors[0].message.contains("windows"));
    }

    #[test]
    fn platform_full_coverage_ok() {
        let mut pac = PlatformAbstractionChecker::new();
        pac.add_platform("linux".into());
        pac.declare_abstraction("fs_ops".into(), vec!["linux".into()]);
        assert!(pac.check_coverage().is_empty());
    }

    #[test]
    fn platform_direct_use_warned() {
        let mut pac = PlatformAbstractionChecker::new();
        pac.add_platform("linux".into());
        let err = pac.check_direct_platform_use("linux").unwrap();
        assert_eq!(err.code, "A44002");
    }

    #[test]
    fn platform_unknown_reference() {
        let mut pac = PlatformAbstractionChecker::new();
        pac.add_platform("linux".into());
        pac.declare_abstraction("net".into(), vec!["linux".into(), "freebsd".into()]);
        let errors = pac.check_unknown_platforms();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A44003");
        assert!(errors[0].message.contains("freebsd"));
    }

    // -----------------------------------------------------------------------
    // FeatureFlagChecker
    // -----------------------------------------------------------------------

    #[test]
    fn feature_flag_unused() {
        let mut ffc = FeatureFlagChecker::new();
        ffc.declare("experimental".into(), false, vec![]);
        let errors = ffc.check_unused();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A45001");
    }

    #[test]
    fn feature_flag_used_ok() {
        let mut ffc = FeatureFlagChecker::new();
        ffc.declare("experimental".into(), false, vec![]);
        ffc.mark_used("experimental");
        assert!(ffc.check_unused().is_empty());
    }

    #[test]
    fn feature_flag_conflicts() {
        let mut ffc = FeatureFlagChecker::new();
        ffc.declare("debug".into(), true, vec!["release".into()]);
        ffc.declare("release".into(), true, vec!["debug".into()]);
        let errors = ffc.check_conflicts();
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, "A45002");
    }

    #[test]
    fn feature_flag_no_conflict_when_disabled() {
        let mut ffc = FeatureFlagChecker::new();
        ffc.declare("debug".into(), true, vec!["release".into()]);
        ffc.declare("release".into(), false, vec!["debug".into()]);
        assert!(ffc.check_conflicts().is_empty());
    }

    #[test]
    fn feature_flag_undeclared() {
        let ffc = FeatureFlagChecker::new();
        let err = ffc.check_undeclared("unknown").unwrap();
        assert_eq!(err.code, "A45003");
    }

    // -----------------------------------------------------------------------
    // ResourceLimitChecker
    // -----------------------------------------------------------------------

    #[test]
    fn resource_limit_exceeded() {
        let mut rlc = ResourceLimitChecker::new();
        rlc.declare_limit("memory".into(), 1024, "bytes".into());
        rlc.record_usage("memory", 2000);
        let errors = rlc.check_limits();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A46001");
    }

    #[test]
    fn resource_limit_ok() {
        let mut rlc = ResourceLimitChecker::new();
        rlc.declare_limit("memory".into(), 1024, "bytes".into());
        rlc.record_usage("memory", 500);
        assert!(rlc.check_limits().is_empty());
    }

    #[test]
    fn resource_near_limit_warned() {
        let mut rlc = ResourceLimitChecker::new();
        rlc.declare_limit("cpu".into(), 100, "percent".into());
        rlc.record_usage("cpu", 95);
        let warnings = rlc.check_near_limit();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "A46003");
    }

    #[test]
    fn resource_unbounded_usage() {
        let rlc = ResourceLimitChecker::new();
        let err = rlc.check_unbounded("unknown").unwrap();
        assert_eq!(err.code, "A46002");
    }

    #[test]
    fn resource_release_reduces_usage() {
        let mut rlc = ResourceLimitChecker::new();
        rlc.declare_limit("mem".into(), 100, "MB".into());
        rlc.record_usage("mem", 80);
        rlc.release_usage("mem", 30);
        assert_eq!(rlc.current_usage("mem"), Some(50));
    }

    // -----------------------------------------------------------------------
    // UnsafeEscapeChecker
    // -----------------------------------------------------------------------

    #[test]
    fn unsafe_no_proof_detected() {
        let mut uec = UnsafeEscapeChecker::new();
        uec.declare_unsafe("raw_ptr".into(), vec!["ptr_valid".into()], 0..10);
        let errors = uec.check_unproven();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A47001");
    }

    #[test]
    fn unsafe_with_proof_ok() {
        let mut uec = UnsafeEscapeChecker::new();
        uec.declare_unsafe("raw_ptr".into(), vec!["ptr_valid".into()], 0..10);
        uec.attach_proof("raw_ptr");
        uec.discharge_obligation("raw_ptr", "ptr_valid".into());
        assert!(uec.check_unproven().is_empty());
        assert!(uec.check_obligations().is_empty());
    }

    #[test]
    fn unsafe_partial_discharge() {
        let mut uec = UnsafeEscapeChecker::new();
        uec.declare_unsafe(
            "raw_ptr".into(),
            vec!["ptr_valid".into(), "no_alias".into()],
            0..10,
        );
        uec.attach_proof("raw_ptr");
        uec.discharge_obligation("raw_ptr", "ptr_valid".into());
        let errors = uec.check_obligations();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A47002");
    }

    // -----------------------------------------------------------------------
    // ContractLibraryChecker
    // -----------------------------------------------------------------------

    #[test]
    fn library_empty_exports() {
        let mut clc = ContractLibraryChecker::new();
        clc.declare_library("math".into(), "2.0.0".into());
        let errors = clc.check_empty_exports();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A55001");
    }

    #[test]
    fn library_with_export_ok() {
        let mut clc = ContractLibraryChecker::new();
        clc.declare_library("math".into(), "1.0.0".into());
        clc.add_export("math", "Arithmetic".into());
        assert!(clc.check_empty_exports().is_empty());
    }

    #[test]
    fn library_self_dependency() {
        let mut clc = ContractLibraryChecker::new();
        clc.declare_library("core".into(), "1.0.0".into());
        clc.add_dependency(
            "core",
            LibraryDep {
                name: "core".into(),
                version_req: "1.0.0".into(),
            },
        );
        let errors = clc.check_circular_deps();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A55002");
    }

    #[test]
    fn library_duplicates() {
        let mut clc = ContractLibraryChecker::new();
        clc.declare_library("math".into(), "1.0.0".into());
        clc.declare_library("math".into(), "2.0.0".into());
        let errors = clc.check_duplicates();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A55003");
    }

    // -----------------------------------------------------------------------
    // ContractCompositionChecker
    // -----------------------------------------------------------------------

    #[test]
    fn composition_extends_unknown() {
        let mut ccc = ContractCompositionChecker::new();
        ccc.declare("MySorter".into(), vec!["Sortable".into()], 2);
        let errors = ccc.check_extends();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A54001");
    }

    #[test]
    fn composition_extends_ok() {
        let mut ccc = ContractCompositionChecker::new();
        ccc.declare("Sortable".into(), vec![], 3);
        ccc.declare("MySorter".into(), vec!["Sortable".into()], 2);
        assert!(ccc.check_extends().is_empty());
    }

    #[test]
    fn composition_circular() {
        let mut ccc = ContractCompositionChecker::new();
        ccc.declare("A".into(), vec!["B".into()], 1);
        ccc.declare("B".into(), vec!["A".into()], 1);
        let errors = ccc.check_circular();
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, "A54002");
    }

    // -----------------------------------------------------------------------
    // StorageFailureChecker
    // -----------------------------------------------------------------------

    #[test]
    fn storage_unhandled_failure() {
        let mut sfc = StorageFailureChecker::new();
        sfc.declare_failure_mode(FailureMode::PartialWrite);
        let errors = sfc.check_unhandled();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A38001");
    }

    #[test]
    fn storage_handled_ok() {
        let mut sfc = StorageFailureChecker::new();
        sfc.declare_failure_mode(FailureMode::DiskFull);
        sfc.mark_handled("disk_full");
        assert!(sfc.check_unhandled().is_empty());
    }

    #[test]
    fn storage_spurious_handler() {
        let mut sfc = StorageFailureChecker::new();
        sfc.mark_handled("nonexistent");
        let errors = sfc.check_spurious_handlers();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A38002");
    }
}
