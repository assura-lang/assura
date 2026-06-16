//! Memory-related domain checkers.
//!
//! AllocatorChecker, CircularBufferChecker.

use std::collections::HashMap;
use std::ops::Range;

use crate::TypeError;

// ===========================================================================
// T056: MEM.3 Allocator contracts
// ===========================================================================

/// Tracks allocation/deallocation pairing and size constraints.
///
/// Error codes:
/// - A22001: allocation not paired with deallocation
/// - A22002: double free (deallocating already freed allocation)
/// - A22003: unbounded allocation detected (no allocation bound proved)
/// - A22004: arena lifetime violation (use after arena drop)
#[derive(Debug, Clone)]
pub(crate) struct AllocatorChecker {
    allocations: HashMap<String, AllocInfo>,
    freed: HashMap<String, Range<usize>>,
    arenas: HashMap<String, ArenaInfo>,
}

#[derive(Debug, Clone)]
pub(crate) struct AllocInfo {
    pub span: Range<usize>,
    pub arena: Option<String>,
    pub bounded: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ArenaInfo {
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

    pub fn declare_arena(&mut self, name: String) {
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

    pub fn record_alloc(&mut self, name: String, arena: Option<String>, span: Range<usize>) {
        self.allocations.insert(
            name,
            AllocInfo {
                span,
                arena,
                bounded: false,
            },
        );
    }

    /// Mark an allocation as having a proved bound.
    pub fn mark_bounded(&mut self, name: &str) {
        if let Some(info) = self.allocations.get_mut(name) {
            info.bounded = true;
        }
    }

    /// Return errors for allocations that have no proved bound.
    pub fn check_unbounded(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, info) in &self.allocations {
            if !info.bounded {
                errors.push(TypeError {
                    code: "A22003".into(),
                    message: format!("unbounded allocation: `{name}` has no allocation bound"),
                    span: info.span.clone(),
                    secondary: None,
                });
            }
        }
        errors.sort_by_key(|e| e.span.start);
        errors
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
pub(crate) struct CircularBufferChecker {
    pub(crate) buffers: HashMap<String, CircBufInfo>,
}

#[derive(Debug, Clone)]
pub(crate) struct CircBufInfo {
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
}

impl CircularBufferChecker {
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
        }
    }

    pub fn declare(&mut self, name: String, capacity: usize) {
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
