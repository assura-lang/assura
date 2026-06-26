//! Information flow, taint, and dependent type checks (thin wrappers).

use crate::TypeError;
use crate::checkers::*;

pub(crate) fn run_taint_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    TaintChecker::check_file(source)
}

pub(crate) fn run_info_flow_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    InfoFlowChecker::check_source(source)
}

#[cfg(test)]
#[path = "info_flow_tests.rs"]
mod tests;
