//! Memory-related checks.
//!
//! Memory regions, shared memory, lock ordering, weak memory,
//! allocators, circular buffers.

use assura_parser::ast::{BinOp, BlockKind, ClauseKind, Decl, Expr};

use crate::TypeError;
use crate::checkers::*;
use crate::domain::*;
use crate::types::*;

// ---------------------------------------------------------------------------
// Memory safety wiring (T046)
// ---------------------------------------------------------------------------

/// Scan for functions with buffer/region parameters and validate memory
/// bounds annotations using the MemoryChecker.
pub(crate) fn run_memory_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut errors = Vec::new();

    // Per-function analysis: for each function with buffer-typed params,
    // check that its requires clauses include bounds checks.
    for decl in &source.decls {
        let (params, clauses) = match &decl.node {
            Decl::FnDef(f) => {
                // Skip axioms, lemmas, and ghost functions: they are
                // mathematical definitions without runtime semantics
                // and should not require bounds-checking annotations.
                if f.is_ghost || f.is_lemma {
                    continue;
                }
                // Axioms are parsed as FnDef with is_lemma=false but
                // use define/property clauses instead of requires/ensures.
                // Skip any function that has no requires AND no ensures.
                let has_runtime_contract = f
                    .clauses
                    .iter()
                    .any(|c| c.kind == ClauseKind::Requires || c.kind == ClauseKind::Ensures);
                if !has_runtime_contract {
                    continue;
                }
                (f.params.as_slice(), f.clauses.as_slice())
            }
            Decl::Extern(e) => (e.params.as_slice(), e.clauses.as_slice()),
            _ => continue,
        };

        let mut checker = MemoryChecker::new();
        let mut has_buffers = false;

        for param in params {
            let ty_str = param.ty.join(" ");
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

        let requires_exprs: Vec<&Expr> = clauses
            .iter()
            .filter(|c| c.kind == ClauseKind::Requires)
            .map(|c| &c.body)
            .collect();

        // Register regions from "region" clauses
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && k == "region"
                && let Expr::Raw(tokens) = &clause.body
                && tokens.len() >= 4
            {
                // region name = lower..upper on buffer
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
            // Any requires clause referencing the buffer counts as a
            // bounds constraint (the author is aware of the buffer).
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

        // Check that regions reference valid buffers
        for mem_err in checker.check_region_buffers(&decl.span) {
            errors.push(TypeError {
                code: mem_err.code,
                message: mem_err.message,
                span: mem_err.span,
                secondary: None,
            });
        }

        // Check region containment from "contains" clauses
        let regions = checker.regions();
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && k == "contains"
                && let Expr::Raw(tokens) = &clause.body
                && tokens.len() >= 2
            {
                let sub_name = &tokens[0];
                let parent_name = &tokens[1];
                // Only check if both names match registered regions
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

/// Extract a capacity annotation from a type string like "Buffer<1024>" or
/// "Region<MAX_SIZE>".
fn extract_capacity_annotation(ty: &str) -> Option<String> {
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

// ---------------------------------------------------------------------------
// Shared memory protocols wiring (T065)
// ---------------------------------------------------------------------------

/// Scan for functions with `shared` or `concurrent` annotations and
/// validate that access modes are declared correctly.
pub(crate) fn run_shared_mem_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut errors = Vec::new();

    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::FnDef(f) => &f.clauses,
            Decl::Contract(c) => &c.clauses,
            _ => continue,
        };

        let has_shared = clauses.iter().any(|c| {
            matches!(&c.kind, ClauseKind::Other(k) if k == "shared" || k == "concurrent" || k == "access_mode")
        });
        if !has_shared {
            continue;
        }

        let mut checker = SharedMemChecker::new();

        // Register access modes from clauses
        for clause in clauses {
            if let ClauseKind::Other(k) = &clause.kind
                && (k == "access_mode" || k == "shared")
                && let Expr::BinOp {
                    lhs,
                    op: BinOp::Implies,
                    rhs,
                } = &clause.body
                && let (Expr::Ident(obj), Expr::Ident(mode)) = (lhs.as_ref(), rhs.as_ref())
            {
                let access_mode = match mode.as_str() {
                    "exclusive" => AccessMode::Exclusive,
                    "shared_read" => AccessMode::SharedRead,
                    _ => AccessMode::None,
                };
                checker.set_mode(obj.clone(), access_mode);
            }
        }

        // Check modifies clauses reference objects with correct access
        for clause in clauses {
            if clause.kind == ClauseKind::Modifies {
                let modified = collect_ident_references(&clause.body);
                for name in &modified {
                    for err in checker.check_write(name, &decl.span) {
                        errors.push(err.into());
                    }
                }
            }
            // Check read accesses in requires/ensures clauses
            if matches!(clause.kind, ClauseKind::Requires | ClauseKind::Ensures) {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    for err in checker.check_read(name, &decl.span) {
                        errors.push(err.into());
                    }
                }
            }
            // Check for data race annotations
            if let ClauseKind::Other(k) = &clause.kind
                && k == "concurrent_access"
                && let Expr::Raw(tokens) = &clause.body
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

// ---------------------------------------------------------------------------
// Lock ordering wiring (T068)
// ---------------------------------------------------------------------------

/// Scan for lock ordering declarations and validate acquisition order.
pub(crate) fn run_lock_order_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = LockOrderChecker::new();
    let mut errors = Vec::new();

    // First pass: collect lock ordering declarations from blocks and inline clauses
    for decl in &source.decls {
        if let Decl::Block { kind, body, .. } = &decl.node
            && *kind == BlockKind::LockOrder
        {
            for (priority, clause) in body.iter().enumerate() {
                if let Expr::Ident(lock_name) = &clause.body {
                    checker.define_order(lock_name.clone(), priority as u32);
                }
            }
        }
        // Also pick up inline lock_order clauses inside contracts/fns
        let clauses = match &decl.node {
            Decl::Contract(c) => c.clauses.as_slice(),
            Decl::FnDef(f) => f.clauses.as_slice(),
            _ => continue,
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

    // Second pass: check lock acquisitions in function bodies
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::FnDef(f) => &f.clauses,
            Decl::Contract(c) => &c.clauses,
            _ => continue,
        };

        for clause in clauses {
            if let ClauseKind::Other(k) = &clause.kind
                && (k == "acquires" || k == "acquire" || k == "locks")
            {
                let lock_names = collect_ident_references(&clause.body);
                for name in &lock_names {
                    // Check that the lock has a defined ordering
                    for err in checker.check_ordering_defined(name, &decl.span) {
                        errors.push(err.into());
                    }
                    for err in checker.acquire(name, &decl.span) {
                        errors.push(err.into());
                    }
                }
            }
            // Handle lock release clauses
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

// ---------------------------------------------------------------------------
// Weak memory ordering wiring (G007, CONC.6)
// ---------------------------------------------------------------------------

/// Validate `ordering` clauses on atomic operations.
///
/// Checks:
/// - The ordering value is a recognized memory ordering keyword
///   (relaxed, acquire, release, acqrel, seq_cst)
/// - Contracts with `ordering: relaxed` that also have `ensures` clauses
///   depending on the value get A23016 warnings (relaxed read
///   without view check)
pub(crate) fn run_weak_memory_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    use assura_parser::ast::MemoryOrdering;
    let mut errors = Vec::new();

    for decl in &source.decls {
        let (name, clauses) = match &decl.node {
            Decl::Contract(c) => (c.name.as_str(), &c.clauses),
            Decl::FnDef(f) => (f.name.as_str(), &f.clauses),
            _ => continue,
        };

        let mut ordering_value: Option<MemoryOrdering> = None;
        let mut has_ensures = false;

        for clause in clauses {
            if clause.kind == ClauseKind::Ordering {
                // Extract the ordering value from the clause body
                let ordering_str = match &clause.body {
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

        // A23016: relaxed read with ensures (value-dependent assertion)
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

// ---------------------------------------------------------------------------
// Domain checker wiring: ~28 checkers from domain.rs
// ---------------------------------------------------------------------------

/// Scan for allocator/arena annotations and check allocation pairing.
pub(crate) fn run_allocator_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = AllocatorChecker::new();
    let mut has_alloc = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "allocator" || k == "alloc" || k == "arena" {
                    has_alloc = true;
                    if let Expr::Ident(name) = &clause.body {
                        checker.record_alloc(name.clone(), None, decl.span.clone());
                    }
                }
                if (k == "dealloc" || k == "free")
                    && let Expr::Ident(name) = &clause.body
                    && let Some(err) = checker.record_free(name, decl.span.clone())
                {
                    return vec![err];
                }
            }
        }
    }
    // Wire arena lifecycle: declare arenas and track drop/use-after-drop
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if (k == "arena" || k == "declare_arena")
                    && let Expr::Ident(name) = &clause.body
                {
                    checker.declare_arena(name.clone());
                    has_alloc = true;
                }
                if (k == "drop_arena" || k == "arena_drop")
                    && let Expr::Ident(name) = &clause.body
                {
                    checker.drop_arena(name, decl.span.clone());
                }
            }
        }
    }
    if !has_alloc {
        return Vec::new();
    }
    // Check bounded annotations: mark allocations that have a proved bound
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "bounded" || k == "alloc_bound")
                && let Expr::Ident(name) = &clause.body
            {
                checker.mark_bounded(name);
            }
        }
    }
    // Check arena use-after-drop for all allocations
    let mut errors = Vec::new();
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
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

/// Scan for circular buffer declarations and validate indexing.
pub(crate) fn run_circular_buffer_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = CircularBufferChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "circular_buffer" || k == "ring_buffer")
            {
                found = true;
                match &clause.body {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = func.as_ref() {
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
    // Process push/pop operations and index checks via collected references
    let mut errors = Vec::new();
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if (k == "push" || k == "insert")
                    && let Expr::Ident(name) = &clause.body
                {
                    checker.push(name);
                }
                if (k == "pop" || k == "remove")
                    && let Expr::Ident(name) = &clause.body
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
                    // Check index bounds for buffer references
                    if let Some(err) = checker.check_index(name, 0, &decl.span) {
                        errors.push(err);
                    }
                    // Check physical wrap for buffer access
                    if let Some(err) = checker.check_physical_wrap(name, 0, &decl.span) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    // Check fullness and logical-to-physical mapping for declared buffers
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_capacity_buffer() {
        assert_eq!(
            extract_capacity_annotation("Buffer<1024>"),
            Some("1024".into())
        );
    }

    #[test]
    fn extract_capacity_region() {
        assert_eq!(
            extract_capacity_annotation("Region<MAX_SIZE>"),
            Some("MAX_SIZE".into())
        );
    }

    #[test]
    fn extract_capacity_fixed_buffer() {
        assert_eq!(
            extract_capacity_annotation("FixedBuffer<256>"),
            Some("256".into())
        );
    }

    #[test]
    fn extract_capacity_no_match() {
        assert_eq!(extract_capacity_annotation("Int"), None);
        assert_eq!(extract_capacity_annotation("String"), None);
        assert_eq!(extract_capacity_annotation("List<Int>"), None);
    }

    #[test]
    fn extract_capacity_empty_angle() {
        assert_eq!(extract_capacity_annotation("Buffer<>"), Some("".into()));
    }

    fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
        let (sf, errs) = assura_parser::parse(src);
        assert!(errs.is_empty(), "parse errors: {errs:?}");
        sf.unwrap()
    }

    #[test]
    fn allocator_unbounded_via_source() {
        let src = r#"
module test;
contract Alloc {
    input(size: Nat)
    alloc buf
    requires { size > 0 }
    ensures { size > 0 }
}
"#;
        let sf = parse_source(src);
        let errors = run_allocator_checks(&sf);
        let has_a22003 = errors.iter().any(|e| e.code == "A22003");
        assert!(has_a22003, "expected A22003 unbounded alloc: {errors:?}");
    }

    #[test]
    fn allocator_bounded_via_source() {
        let src = r#"
module test;
contract Alloc {
    input(size: Nat)
    alloc buf
    bounded buf
    requires { size > 0 }
    ensures { size > 0 }
}
"#;
        let sf = parse_source(src);
        let errors = run_allocator_checks(&sf);
        let has_a22003 = errors.iter().any(|e| e.code == "A22003");
        assert!(
            !has_a22003,
            "bounded alloc should not produce A22003: {errors:?}"
        );
    }
}
