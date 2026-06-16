//! Storage-related checks.
//!
//! Crash recovery, page cache, MVCC, rollback,
//! monotonic state, storage failure.

use assura_parser::ast::{ClauseKind, Decl, Expr};

use crate::TypeError;
use crate::checkers::*;
use crate::domain::*;
use crate::types::*;

pub(crate) fn run_crash_recovery_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = CrashRecoveryChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "wal" || k == "crash_recovery" || k == "write_ahead" {
                    found = true;
                    if let Some(id) = extract_ident(&clause.body) {
                        checker.begin_write(id.to_string());
                    }
                }
                if (k == "write_data" || k == "data_write")
                    && let Some(id) = extract_ident(&clause.body)
                {
                    checker.write_data(id);
                }
                if (k == "write_wal" || k == "wal_write")
                    && let Some(id) = extract_ident(&clause.body)
                {
                    checker.write_wal(id);
                }
                if (k == "fsync" || k == "flush")
                    && let Some(id) = extract_ident(&clause.body)
                {
                    checker.fsync(id);
                }
                if k == "commit"
                    && let Some(id) = extract_ident(&clause.body)
                {
                    checker.commit(id);
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    checker.check_all()
}

/// Scan for page cache annotations.
pub(crate) fn run_page_cache_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker: Option<PageCacheChecker> = None;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "page_cache" || k == "buffer_pool" || k == "cache_policy")
            {
                // Extract capacity from annotation body
                let capacity = match &clause.body {
                    Expr::Call { args, .. } => {
                        args.first()
                            .and_then(extract_int_literal)
                            .unwrap_or(DEFAULT_PAGE_SIZE) as usize
                    }
                    Expr::Literal(assura_parser::ast::Literal::Int(s)) => {
                        s.parse::<usize>().unwrap_or(DEFAULT_PAGE_SIZE as usize)
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        kvs.iter()
                            .find(|(k, _)| *k == "capacity" || *k == "size" || *k == "max_pages")
                            .and_then(|(_, v)| extract_int_literal(v))
                            .unwrap_or(DEFAULT_PAGE_SIZE) as usize
                    }
                };
                if checker.is_none() {
                    checker = Some(PageCacheChecker::new(capacity));
                }
            }
            // Extract page operations from requires/ensures clauses
            if let Some(ch) = checker.as_mut()
                && matches!(
                    clause.kind,
                    ClauseKind::Requires | ClauseKind::Ensures | ClauseKind::Other(_)
                )
            {
                page_cache_scan_expr(&clause.body, ch);
            }
        }
    }
    match checker {
        Some(ch) => ch.check_capacity(),
        None => Vec::new(),
    }
}

/// Scan an expression for page cache operations (load_page, pin, dirty, evict, page_count).
fn page_cache_scan_expr(expr: &Expr, checker: &mut PageCacheChecker) {
    if let Some((name, args)) = extract_call(expr) {
        let page_id = args
            .first()
            .and_then(extract_int_literal)
            .unwrap_or(DEFAULT_PARAM_ZERO) as u64;
        match name {
            "load_page" | "load" | "fetch_page" => checker.load_page(page_id),
            "pin" | "pin_page" => checker.pin(page_id),
            "unpin" | "unpin_page" => checker.unpin(page_id),
            "mark_dirty" | "dirty" => checker.mark_dirty(page_id),
            "flush" | "flush_page" => checker.flush(page_id),
            "evict" | "evict_page" => {
                checker.evict(page_id);
            }
            _ => {}
        }
    }
}

/// Scan for MVCC/snapshot isolation annotations.
pub(crate) fn run_mvcc_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = MvccChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "mvcc" || k == "snapshot_isolation" || k == "serializable")
            {
                found = true;
                // Extract transaction operations from annotation body
                mvcc_scan_expr(&clause.body, &mut checker);
            }
            // Also scan requires/ensures for transaction operations
            if found && matches!(clause.kind, ClauseKind::Requires | ClauseKind::Ensures) {
                mvcc_scan_expr(&clause.body, &mut checker);
            }
        }
    }
    if !found {
        return Vec::new();
    }
    let mut errors = checker.check_write_conflicts();
    // Check snapshot read isolation for referenced keys
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
                    if let Some(err) = checker.check_snapshot_read(name, 1) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    // Check phantom reads for the first transaction
    errors.extend(checker.check_phantom(1));
    errors
}

/// Scan an expression for MVCC operations (begin_txn, write, commit).
fn mvcc_scan_expr(expr: &Expr, checker: &mut MvccChecker) {
    if let Some((name, args)) = extract_call(expr) {
        match name {
            "begin_txn" | "begin" | "start_transaction" => {
                checker.begin_txn();
            }
            "write" | "write_version" | "put" => {
                let key = args
                    .first()
                    .and_then(extract_ident)
                    .unwrap_or("default")
                    .to_string();
                let txn_id = args
                    .get(1)
                    .and_then(extract_int_literal)
                    .unwrap_or(DEFAULT_PARAM_ONE) as u64;
                checker.write_version(key, txn_id);
            }
            "commit" | "commit_txn" => {
                let txn_id = args
                    .first()
                    .and_then(extract_int_literal)
                    .unwrap_or(DEFAULT_PARAM_ONE) as u64;
                checker.commit_txn(txn_id);
            }
            _ => {}
        }
    }
    // Scan sub-expressions
    match expr {
        Expr::Block(exprs) | Expr::List(exprs) => {
            for e in exprs {
                mvcc_scan_expr(e, checker);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            mvcc_scan_expr(lhs, checker);
            mvcc_scan_expr(rhs, checker);
        }
        _ => {}
    }
}

/// Scan for transactional rollback annotations.
pub(crate) fn run_rollback_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = RollbackChecker::new();
    let mut scan_errors = Vec::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "rollback" || k == "savepoint" || k == "transactional")
            {
                found = true;
                scan_errors.extend(rollback_scan_expr(&clause.body, &mut checker));
            }
            if found && matches!(clause.kind, ClauseKind::Requires | ClauseKind::Ensures) {
                scan_errors.extend(rollback_scan_expr(&clause.body, &mut checker));
            }
        }
    }
    if !found {
        return Vec::new();
    }
    let mut errors = scan_errors;
    errors.extend(checker.check_resource_leak());
    errors.extend(checker.check_savepoint_nesting());
    errors
}

/// Scan an expression for rollback operations (savepoint, acquire, release, rollback).
/// Returns any immediate errors (e.g., rollback to unknown savepoint).
fn rollback_scan_expr(expr: &Expr, checker: &mut RollbackChecker) -> Vec<TypeError> {
    let mut scan_errors = Vec::new();
    if let Some((name, args)) = extract_call(expr) {
        match name {
            "savepoint" | "create_savepoint" => {
                let sp_name = args
                    .first()
                    .and_then(extract_ident)
                    .unwrap_or("default")
                    .to_string();
                checker.create_savepoint(sp_name);
            }
            "acquire" | "acquire_resource" | "lock" => {
                let res_name = args
                    .first()
                    .and_then(extract_ident)
                    .unwrap_or("resource")
                    .to_string();
                checker.acquire_resource(res_name);
            }
            "release" | "release_resource" | "unlock" => {
                let res_name = args.first().and_then(extract_ident).unwrap_or("resource");
                checker.release_resource(res_name);
            }
            "rollback" | "rollback_to" => {
                let sp_name = args.first().and_then(extract_ident).unwrap_or("default");
                if let Some(err) = checker.rollback_to(sp_name) {
                    scan_errors.push(err);
                }
            }
            _ => {}
        }
    }
    // Also check for identifier-based savepoint declarations
    if let Expr::Ident(name) = expr {
        checker.create_savepoint(name.clone());
    }
    // Scan sub-expressions recursively
    match expr {
        Expr::Block(exprs) | Expr::List(exprs) => {
            for e in exprs {
                scan_errors.extend(rollback_scan_expr(e, checker));
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            scan_errors.extend(rollback_scan_expr(lhs, checker));
            scan_errors.extend(rollback_scan_expr(rhs, checker));
        }
        Expr::Call { func, args } => {
            scan_errors.extend(rollback_scan_expr(func, checker));
            for a in args {
                scan_errors.extend(rollback_scan_expr(a, checker));
            }
        }
        _ => {}
    }
    scan_errors
}

/// Scan for monotonic state annotations and check update direction.
pub(crate) fn run_monotonic_state_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = MonotonicStateChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "monotonic" || k == "monotone" || k == "increasing" {
                    found = true;
                    // Extract direction from call syntax: monotonic(name, direction, initial)
                    match &clause.body {
                        Expr::Call { func, args } => {
                            if let Expr::Ident(name) = func.as_ref() {
                                let direction = args
                                    .first()
                                    .and_then(extract_ident)
                                    .map(|d| match d {
                                        "strictly_increasing" => {
                                            MonotonicDirection::StrictlyIncreasing
                                        }
                                        "decreasing" => MonotonicDirection::Decreasing,
                                        _ => MonotonicDirection::Increasing,
                                    })
                                    .unwrap_or(MonotonicDirection::Increasing);
                                let initial = args
                                    .get(1)
                                    .and_then(extract_int_literal)
                                    .unwrap_or(DEFAULT_PARAM_ZERO);
                                checker.declare(
                                    name.clone(),
                                    direction,
                                    initial,
                                    decl.span.clone(),
                                );
                            }
                        }
                        Expr::Ident(name) => {
                            checker.declare(
                                name.clone(),
                                MonotonicDirection::Increasing,
                                0,
                                decl.span.clone(),
                            );
                        }
                        _ => {}
                    }
                }
                // Check updates: update(name, value)
                if (k == "update" || k == "assign" || k == "set")
                    && let Some((name, args)) = extract_call(&clause.body)
                    && let Some(val) = args.first().and_then(extract_int_literal)
                    && let Some(err) = checker.update(name, val)
                {
                    return vec![err];
                }
                // Check resets
                if k == "reset"
                    && let Some(name) = extract_ident(&clause.body)
                    && let Some(err) = checker.check_reset(name)
                {
                    return vec![err];
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Check ensures clauses for monotonicity violations via identifier usage
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
                    if let Some(mut err) = checker.check_access(name) {
                        if let Some(val) = checker.current_value(name) {
                            err.message.push_str(&format!(" (current value: {val})"));
                        }
                        errors.push(err);
                    }
                }
            }
        }
    }
    errors
}

/// Scan for storage failure model annotations.
pub(crate) fn run_storage_failure_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = StorageFailureChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "failure_mode" || k == "storage_failure" {
                    found = true;
                    if let Expr::Ident(name) = &clause.body {
                        let mode = match name.as_str() {
                            "partial_write" => FailureMode::PartialWrite,
                            "torn_page" => FailureMode::TornPage,
                            "bit_rot" => FailureMode::BitRot,
                            "disk_full" => FailureMode::DiskFull,
                            "io_timeout" => FailureMode::IoTimeout,
                            _ => continue,
                        };
                        checker.declare_failure_mode(mode);
                    }
                }
                if (k == "handles" || k == "handles_failure")
                    && let Expr::Ident(name) = &clause.body
                {
                    checker.mark_handled(name);
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    let mut errors = checker.check_unhandled();
    errors.extend(checker.check_critical_coverage());
    errors.extend(checker.check_spurious_handlers());
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
        let (sf, errs) = assura_parser::parse(src);
        assert!(errs.is_empty(), "parse errors: {errs:?}");
        sf.unwrap()
    }

    #[test]
    fn crash_recovery_no_annotation_no_errors() {
        let sf = parse_source(r#"contract Simple { requires { true } }"#);
        assert!(run_crash_recovery_checks(&sf).is_empty());
    }

    #[test]
    fn crash_recovery_wal_without_write_wal_emits_a33001() {
        let sf = parse_source(r#"contract W { wal txn1 write_data txn1 }"#);
        let errs = run_crash_recovery_checks(&sf);
        assert!(errs.iter().any(|e| e.code == "A33001"), "got: {errs:?}");
    }

    #[test]
    fn monotonic_no_annotation_no_errors() {
        let sf = parse_source(r#"contract Simple { requires { true } }"#);
        assert!(run_monotonic_state_checks(&sf).is_empty());
    }

    #[test]
    fn monotonic_undeclared_access_emits_a37003() {
        let sf = parse_source(r#"contract C { monotonic seq ensures { other > 0 } }"#);
        let errs = run_monotonic_state_checks(&sf);
        assert!(errs.iter().any(|e| e.code == "A37003"), "got: {errs:?}");
    }

    #[test]
    fn storage_failure_no_annotation_no_errors() {
        let sf = parse_source(r#"contract Simple { requires { true } }"#);
        assert!(run_storage_failure_checks(&sf).is_empty());
    }

    #[test]
    fn storage_failure_unhandled_emits_a38001() {
        let sf = parse_source(r#"contract D { storage_failure partial_write }"#);
        let errs = run_storage_failure_checks(&sf);
        assert!(errs.iter().any(|e| e.code == "A38001"), "got: {errs:?}");
    }
}
