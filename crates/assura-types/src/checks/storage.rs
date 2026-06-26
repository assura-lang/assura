//! Storage-related checks.
//!
//! Crash recovery, page cache, MVCC, rollback,
//! monotonic state, storage failure.

use crate::TypeError;
use crate::domain::*;

pub(crate) fn run_crash_recovery_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    CrashRecoveryChecker::check_source(source)
}

pub(crate) fn run_page_cache_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    PageCacheChecker::check_source(source)
}

pub(crate) fn run_mvcc_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    MvccChecker::check_source(source)
}

pub(crate) fn run_rollback_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    RollbackChecker::check_source(source)
}

pub(crate) fn run_monotonic_state_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    MonotonicStateChecker::check_source(source)
}

pub(crate) fn run_storage_failure_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    StorageFailureChecker::check_source(source)
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
