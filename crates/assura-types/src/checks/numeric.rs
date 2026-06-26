//! Numeric, fixed-width, and collection checks -- thin wrappers.
//!
//! Domain logic lives in [`crate::domain::numeric`].

use crate::domain::*;
use crate::{TypeEnv, TypeError};

/// T055: Detect potential integer overflow in fixed-width arithmetic.
pub(crate) fn run_fixed_width_checks(
    source: &assura_parser::ast::SourceFile,
    type_env: &TypeEnv,
) -> Vec<TypeError> {
    FixedWidthSourceChecker::check_source(source, type_env)
}

/// Validate that contracts referencing standard collection operations
/// declare postconditions consistent with the operation's semantics.
pub(crate) fn run_collection_contract_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    CollectionContracts::check_source(source)
}

pub(crate) fn run_numerical_precision_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    NumericalPrecisionChecker::check_source(source)
}

/// Scan for precomputed table annotations.
pub(crate) fn run_precomputed_table_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    PrecomputedTableChecker::check_source(source)
}

/// Collect SMT verification obligations for precomputed tables.
pub fn collect_table_smt_obligations(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TableSmtObligation> {
    PrecomputedTableChecker::collect_smt_obligations(source)
}

#[cfg(test)]
#[path = "numeric_tests.rs"]
mod tests;
