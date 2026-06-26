//! Safety-related checks (thin wrappers).
//!
//! Constant-time, crypto conformance, secure erasure, unsafe escape.

use crate::TypeError;
use crate::checkers::*;
use crate::domain::*;

pub(crate) fn run_constant_time_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    ConstantTimeChecker::check_source(source)
}

pub(crate) fn run_crypto_conformance_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    CryptoConformanceChecker::check_source(source)
}

pub(crate) fn run_secure_erasure_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    SecureErasureChecker::check_source(source)
}

pub(crate) fn run_unsafe_escape_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    UnsafeEscapeChecker::check_source(source)
}

#[cfg(test)]
#[path = "safety_tests.rs"]
mod tests;
