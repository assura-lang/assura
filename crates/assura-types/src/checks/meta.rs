//! Meta-level checks: thin wiring that delegates to domain checkers.
//!
//! Match exhaustiveness, interface, structural invariant,
//! complexity bounds, behavioral equivalence, refinement,
//! incremental contracts, scoped invariants, composition, libraries.

use crate::TypeError;
use crate::checkers::{InterfaceChecker, StructuralInvariantChecker};
use crate::domain::*;

pub(crate) fn run_match_exhaustiveness_checks(
    source: &assura_parser::ast::SourceFile,
    symbols: &assura_resolve::SymbolTable,
) -> Vec<TypeError> {
    run_match_exhaustiveness_source(source, symbols)
}

pub(crate) fn run_interface_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    InterfaceChecker::check_source(source)
}

pub(crate) fn run_structural_invariant_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    StructuralInvariantChecker::check_source(source)
}

pub(crate) fn run_complexity_bound_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    ComplexityBoundChecker::check_source(source)
}

pub(crate) fn run_behavioral_equivalence_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    BehavioralEquivalenceChecker::check_source(source)
}

pub(crate) fn run_multi_pass_refinement_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    MultiPassRefinementChecker::check_source(source)
}

pub(crate) fn run_incremental_contract_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    IncrementalContractChecker::check_source(source)
}

pub(crate) fn run_scoped_invariant_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    ScopedInvariantChecker::check_source(source)
}

pub(crate) fn run_contract_composition_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    ContractCompositionChecker::check_source(source)
}

pub(crate) fn run_contract_library_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    ContractLibraryChecker::check_source(source)
}

#[cfg(test)]
#[path = "meta_tests.rs"]
mod tests;
