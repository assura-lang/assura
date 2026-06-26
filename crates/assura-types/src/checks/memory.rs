//! Memory-related checks -- thin wrappers.
//!
//! Domain logic lives in [`crate::domain::memory`].

use crate::TypeError;
use crate::domain::*;

pub(crate) fn run_memory_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    MemorySourceChecker::check_source(source)
}

pub(crate) fn run_shared_mem_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    SharedMemSourceChecker::check_source(source)
}

pub(crate) fn run_lock_order_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    LockOrderSourceChecker::check_source(source)
}

pub(crate) fn run_weak_memory_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    WeakMemorySourceChecker::check_source(source)
}

pub(crate) fn run_allocator_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    AllocatorChecker::check_source(source)
}

pub(crate) fn run_circular_buffer_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    CircularBufferChecker::check_source(source)
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
