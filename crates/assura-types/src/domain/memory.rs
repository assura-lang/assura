//! Memory-related domain checkers.
//!
//! AllocatorChecker, CircularBufferChecker, and source-level check
//! wiring moved from `checks/memory.rs`.

use std::collections::HashMap;
use std::ops::Range;

use assura_parser::ast::{BinOp, BlockKind, ClauseKind, Decl, Expr, SpExpr};

use crate::TypeError;
use crate::checkers::*;
use crate::types::*;

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

// ===========================================================================
// Source-level check wiring (moved from checks/memory.rs)
// ===========================================================================

/// Extract a capacity annotation from a type string like "Buffer<1024>".
pub(crate) fn extract_capacity_annotation(ty: &str) -> Option<String> {
    for prefix in &["Buffer", "Region", "FixedBuffer"] {
        if let Some(rest) = ty.strip_prefix(prefix) {
            let rest = rest.trim_start();
            if let Some(inner) = rest.strip_prefix('<') {
                let inner = inner.trim_end();
                if let Some(cap) = inner.strip_suffix('>') {
                    return Some(cap.trim().to_string());
                }
            }
        }
    }
    None
}

/// Namespace struct for memory source-level checks.
pub(crate) struct MemorySourceChecker;

impl MemorySourceChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for decl in &source.decls {
            let (params, clauses) = if let Decl::FnDef(f) = &decl.node {
                if f.is_ghost || f.is_lemma {
                    continue;
                }
                let has_runtime_contract = f
                    .clauses
                    .iter()
                    .any(|c| c.kind == ClauseKind::Requires || c.kind == ClauseKind::Ensures);
                if !has_runtime_contract {
                    continue;
                }
                (f.params.as_slice(), f.clauses.as_slice())
            } else if let Decl::Extern(e) = &decl.node {
                (e.params.as_slice(), e.clauses.as_slice())
            } else {
                continue;
            };

            let mut checker = MemoryChecker::new();
            let mut has_buffers = false;

            for param in params {
                let ty_str = param.ty.as_ref().map(|t| t.to_string()).unwrap_or_default();
                if let Some(cap) = extract_capacity_annotation(&ty_str) {
                    checker.register_buffer(param.name.clone(), cap);
                    has_buffers = true;
                } else if ty_str.contains("Bytes") || ty_str.contains("Sequence") {
                    checker.register_buffer(param.name.clone(), format!("{}.len", param.name));
                    has_buffers = true;
                }
            }

            if !has_buffers {
                continue;
            }

            let requires_exprs: Vec<&SpExpr> = clauses
                .iter()
                .filter(|c| c.kind == ClauseKind::Requires)
                .map(|c| &c.body)
                .collect();

            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && k == "region"
                    && let Expr::Raw(tokens) = &clause.body.node
                    && tokens.len() >= 4
                {
                    let region = MemoryRegion {
                        name: tokens[0].clone(),
                        lower: tokens.get(1).cloned().unwrap_or_default(),
                        upper: tokens.get(2).cloned().unwrap_or_default(),
                        buffer: tokens.get(3).cloned().unwrap_or_default(),
                    };
                    checker.register_region(region);
                }
            }

            for buf_name in checker.buffer_names() {
                let has_any_constraint = requires_exprs
                    .iter()
                    .any(|expr| expr_references_var(expr, &buf_name));
                if has_any_constraint {
                    continue;
                }
                if let Some(mem_err) =
                    checker.check_bounds_in_requires(&buf_name, &requires_exprs, &decl.span)
                {
                    errors.push(TypeError {
                        code: mem_err.code,
                        message: mem_err.message,
                        span: mem_err.span,
                        secondary: None,
                    });
                }
            }

            for mem_err in checker.check_region_buffers(&decl.span) {
                errors.push(TypeError {
                    code: mem_err.code,
                    message: mem_err.message,
                    span: mem_err.span,
                    secondary: None,
                });
            }

            let regions = checker.regions();
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && k == "contains"
                    && let Expr::Raw(tokens) = &clause.body.node
                    && tokens.len() >= 2
                {
                    let sub_name = &tokens[0];
                    let parent_name = &tokens[1];
                    if (regions.iter().any(|r| r.name == *sub_name)
                        || regions.iter().any(|r| r.name == *parent_name))
                        && let Some(mem_err) =
                            checker.check_region_containment(sub_name, parent_name, &decl.span)
                    {
                        errors.push(TypeError {
                            code: mem_err.code,
                            message: mem_err.message,
                            span: mem_err.span,
                            secondary: None,
                        });
                    }
                }
            }
        }
        errors
    }
}

/// Namespace struct for shared memory source-level checks.
pub(crate) struct SharedMemSourceChecker;

impl SharedMemSourceChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            let has_shared = clauses.iter().any(|c| {
                matches!(&c.kind, ClauseKind::Other(k) if k == "shared" || k == "concurrent" || k == "access_mode")
            });
            if !has_shared {
                continue;
            }
            let mut checker = SharedMemChecker::new();
            for clause in clauses {
                if let ClauseKind::Other(k) = &clause.kind
                    && (k == "access_mode" || k == "shared")
                    && let Expr::BinOp {
                        lhs,
                        op: BinOp::Implies,
                        rhs,
                    } = &clause.body.node
                    && let (Expr::Ident(obj), Expr::Ident(mode)) =
                        (&lhs.as_ref().node, &rhs.as_ref().node)
                {
                    let access_mode = match mode.as_str() {
                        "exclusive" => AccessMode::Exclusive,
                        "shared_read" => AccessMode::SharedRead,
                        _ => AccessMode::None,
                    };
                    checker.set_mode(obj.clone(), access_mode);
                }
            }
            for clause in clauses {
                if clause.kind == ClauseKind::Modifies {
                    let modified = collect_ident_references(&clause.body);
                    for name in &modified {
                        for err in checker.check_write(name, &decl.span) {
                            errors.push(err.into());
                        }
                    }
                }
                if matches!(clause.kind, ClauseKind::Requires | ClauseKind::Ensures) {
                    let refs = collect_ident_references(&clause.body);
                    for name in &refs {
                        for err in checker.check_read(name, &decl.span) {
                            errors.push(err.into());
                        }
                    }
                }
                if let ClauseKind::Other(k) = &clause.kind
                    && k == "concurrent_access"
                    && let Expr::Raw(tokens) = &clause.body.node
                    && tokens.len() >= 3
                {
                    let object = &tokens[0];
                    let mode_a = match tokens[1].as_str() {
                        "exclusive" => AccessMode::Exclusive,
                        "shared_read" => AccessMode::SharedRead,
                        _ => AccessMode::None,
                    };
                    let mode_b = match tokens[2].as_str() {
                        "exclusive" => AccessMode::Exclusive,
                        "shared_read" => AccessMode::SharedRead,
                        _ => AccessMode::None,
                    };
                    for err in checker.check_data_race(object, mode_a, mode_b, &decl.span) {
                        errors.push(err.into());
                    }
                }
            }
        }
        errors
    }
}

/// Namespace struct for lock ordering source-level checks.
pub(crate) struct LockOrderSourceChecker;

impl LockOrderSourceChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = LockOrderChecker::new();
        let mut errors = Vec::new();

        for decl in &source.decls {
            if let Decl::Block { kind, body, .. } = &decl.node
                && *kind == BlockKind::LockOrder
            {
                for (priority, clause) in body.iter().enumerate() {
                    if let Expr::Ident(lock_name) = &clause.body.node {
                        checker.define_order(lock_name.clone(), priority as u32);
                    }
                }
            }
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            let mut inline_priority = 0u32;
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && k == "lock_order"
                {
                    for name in collect_ident_references(&clause.body) {
                        checker.define_order(name, inline_priority);
                        inline_priority += 1;
                    }
                }
            }
        }

        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(k) = &clause.kind
                    && (k == "acquires" || k == "acquire" || k == "locks")
                {
                    let lock_names = collect_ident_references(&clause.body);
                    for name in &lock_names {
                        for err in checker.check_ordering_defined(name, &decl.span) {
                            errors.push(err.into());
                        }
                        for err in checker.acquire(name, &decl.span) {
                            errors.push(err.into());
                        }
                    }
                }
                if let ClauseKind::Other(k) = &clause.kind
                    && (k == "releases" || k == "unlocks")
                {
                    let lock_names = collect_ident_references(&clause.body);
                    for name in &lock_names {
                        for err in checker.release(name, &decl.span) {
                            errors.push(err.into());
                        }
                    }
                }
            }
        }

        errors
    }
}

/// Namespace struct for weak memory ordering source-level checks.
pub(crate) struct WeakMemorySourceChecker;

impl WeakMemorySourceChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        use assura_parser::ast::MemoryOrdering;
        let mut errors = Vec::new();

        for decl in &source.decls {
            let Some((name, clauses)) = crate::fn_or_contract_name_clauses(&decl.node) else {
                continue;
            };

            let mut ordering_value: Option<MemoryOrdering> = None;
            let mut has_ensures = false;

            for clause in clauses {
                if clause.kind == ClauseKind::Ordering {
                    let ordering_str = match &clause.body.node {
                        Expr::Ident(s) => Some(s.as_str()),
                        Expr::Raw(tokens) => tokens
                            .iter()
                            .find(|t| MemoryOrdering::parse(t).is_some())
                            .map(|t| t.as_str()),
                        _ => None,
                    };
                    if let Some(s) = ordering_str {
                        if let Some(ord) = MemoryOrdering::parse(s) {
                            ordering_value = Some(ord);
                        } else {
                            errors.push(TypeError {
                                code: "A23019".into(),
                                message: format!(
                                    "unknown memory ordering `{s}` in `{name}`; \
                                     expected one of: relaxed, acquire, release, acqrel, seq_cst"
                                ),
                                span: decl.span.clone(),
                                secondary: None,
                            });
                        }
                    }
                }
                if clause.kind == ClauseKind::Ensures {
                    has_ensures = true;
                }
            }

            if ordering_value == Some(MemoryOrdering::Relaxed) && has_ensures {
                errors.push(TypeError {
                    code: "A23016".into(),
                    message: format!(
                        "relaxed ordering in `{name}` with ensures clause: \
                         value read with Relaxed may be stale; \
                         use Acquire for value-dependent assertions"
                    ),
                    span: decl.span.clone(),
                    secondary: None,
                });
            }
        }

        errors
    }
}

impl AllocatorChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = AllocatorChecker::new();
        let mut has_alloc = false;
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "allocator" || k == "alloc" || k == "arena" {
                        has_alloc = true;
                        if let Expr::Ident(name) = &clause.body.node {
                            checker.record_alloc(name.clone(), None, decl.span.clone());
                        }
                    }
                    if (k == "dealloc" || k == "free")
                        && let Expr::Ident(name) = &clause.body.node
                        && let Some(err) = checker.record_free(name, decl.span.clone())
                    {
                        return vec![err];
                    }
                }
            }
        }
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if (k == "arena" || k == "declare_arena")
                        && let Expr::Ident(name) = &clause.body.node
                    {
                        checker.declare_arena(name.clone());
                        has_alloc = true;
                    }
                    if (k == "drop_arena" || k == "arena_drop")
                        && let Expr::Ident(name) = &clause.body.node
                    {
                        checker.drop_arena(name, decl.span.clone());
                    }
                }
            }
        }
        if !has_alloc {
            return Vec::new();
        }
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "bounded" || k == "alloc_bound")
                    && let Expr::Ident(name) = &clause.body.node
                {
                    checker.mark_bounded(name);
                }
            }
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
                        if let Some(err) = checker.check_arena_use(name, &decl.span) {
                            errors.push(err);
                        }
                    }
                }
            }
        }
        errors.extend(checker.check_unpaired());
        errors.extend(checker.check_unbounded());
        errors
    }
}

impl CircularBufferChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = CircularBufferChecker::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "circular_buffer" || k == "ring_buffer")
                {
                    found = true;
                    match &clause.body.node {
                        Expr::Call { func, args } => {
                            if let Expr::Ident(name) = &func.as_ref().node {
                                let cap = args
                                    .first()
                                    .and_then(extract_int_literal)
                                    .unwrap_or(DEFAULT_BUFFER_CAPACITY)
                                    as usize;
                                checker.declare(name.clone(), cap);
                            }
                        }
                        Expr::Ident(name) => {
                            checker.declare(name.clone(), DEFAULT_BUFFER_CAPACITY as usize);
                        }
                        _ => {
                            let kvs = extract_kv_pairs(&clause.body);
                            let name = kvs
                                .iter()
                                .find(|(k, _)| *k == "name" || *k == "buffer")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("unnamed")
                                .to_string();
                            let cap = kvs
                                .iter()
                                .find(|(k, _)| *k == "capacity" || *k == "size")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_BUFFER_CAPACITY)
                                as usize;
                            checker.declare(name, cap);
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
                if let ClauseKind::Other(ref k) = clause.kind {
                    if (k == "push" || k == "insert")
                        && let Expr::Ident(name) = &clause.body.node
                    {
                        checker.push(name);
                    }
                    if (k == "pop" || k == "remove")
                        && let Expr::Ident(name) = &clause.body.node
                    {
                        checker.pop(name);
                    }
                }
                if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                    let refs = collect_ident_references(&clause.body);
                    for name in &refs {
                        if let Some(err) = checker.check_read(name, &decl.span) {
                            errors.push(err);
                        }
                        if let Some(err) = checker.check_index(name, 0, &decl.span) {
                            errors.push(err);
                        }
                        if let Some(err) = checker.check_physical_wrap(name, 0, &decl.span) {
                            errors.push(err);
                        }
                    }
                }
            }
        }
        for (name, buf) in &checker.buffers {
            if buf.is_full() {
                errors.push(TypeError {
                    code: "A23002".into(),
                    message: format!("circular buffer `{name}` is full"),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }
}
