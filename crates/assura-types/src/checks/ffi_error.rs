//! FFI and error propagation checks.

use crate::TypeError;
use crate::checkers::*;

pub(crate) fn run_ffi_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    FfiBoundaryChecker::check_source(source)
}

pub(crate) fn run_error_propagation_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    ErrorPropagationChecker::check_source(source)
}

#[cfg(test)]
#[path = "ffi_error_tests.rs"]
mod tests;
