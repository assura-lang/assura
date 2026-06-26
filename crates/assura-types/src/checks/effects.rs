//! Effect checking (thin wrappers).

use crate::TypeError;
use crate::checkers::*;

pub(crate) fn run_effect_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    EffectChecker::check_source(source)
}

/// Re-export wrapper: build a map from decl names to their declared effect sets.
/// Used by `tests/wiring.rs` via `super::build_effect_map`.
#[cfg(test)]
pub(crate) fn build_effect_map(
    source: &assura_parser::ast::SourceFile,
    checker: &EffectChecker,
) -> std::collections::HashMap<String, EffectSet> {
    EffectChecker::build_effect_map_from(source, checker)
}

#[cfg(test)]
#[path = "effects_tests.rs"]
mod tests;
