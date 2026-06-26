//! Format-related checks -- thin wrappers.
//!
//! Domain logic lives in [`crate::domain::format`].

use crate::TypeError;
use crate::domain::*;

pub(crate) fn run_binary_format_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    BinaryFormatChecker::check_source(source)
}

pub(crate) fn run_bit_level_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    BitLevelChecker::check_source(source)
}

pub(crate) fn run_string_encoding_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    StringEncodingChecker::check_source(source)
}

pub(crate) fn run_checksum_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    ChecksumChecker::check_source(source)
}

pub(crate) fn run_protocol_grammar_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    ProtocolGrammarChecker::check_source(source)
}

pub(crate) fn run_opaque_function_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    OpaqueFunctionChecker::check_source(source)
}

/// Check codec registry declarations.
pub(crate) fn run_codec_registry_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    crate::domain::check_codec_registry(source)
}

#[cfg(test)]
#[path = "format_tests.rs"]
mod tests;
