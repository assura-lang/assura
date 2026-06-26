//! Concurrency-related checks.
//!
//! Determinism, callback re-entrancy, temporal deadlines.

use crate::TypeError;
use crate::checkers::*;
use crate::domain::*;

pub(crate) fn run_determinism_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    DeterminismChecker::check_source(source)
}

pub(crate) fn run_callback_reentrancy_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    CallbackReentrancyChecker::check_source(source)
}

pub(crate) fn run_temporal_deadline_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    TemporalDeadlineChecker::check_source(source)
}

#[cfg(test)]
#[path = "concurrency_tests.rs"]
mod tests;
