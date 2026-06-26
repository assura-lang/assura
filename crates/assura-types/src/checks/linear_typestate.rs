//! Linearity and typestate checks.

use crate::TypeError;
use crate::checkers::*;

pub(crate) fn run_linearity_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    run_linearity_checks_source(source)
}

pub(crate) fn run_typestate_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    run_typestate_checks_source(source)
}

#[cfg(test)]
#[path = "linear_typestate_tests.rs"]
mod tests;
