//! Platform-related checks.
//!
//! Platform abstraction, feature flags, resource limits.

use crate::TypeError;
use crate::domain::*;

pub(crate) fn run_platform_abstraction_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    PlatformAbstractionChecker::check_source(source)
}

pub(crate) fn run_feature_flag_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    FeatureFlagChecker::check_source(source)
}

pub(crate) fn run_resource_limit_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    ResourceLimitChecker::check_source(source)
}

#[cfg(test)]
#[path = "platform_tests.rs"]
mod tests;
