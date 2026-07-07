//! Shared parameter/context structs for `assura check`.

use super::super::*;

/// Configuration for the `assura check` command.
pub(crate) struct CheckOptions<'a> {
    pub(crate) filename: &'a str,
    pub(crate) output_mode: OutputMode,
    pub(crate) verbosity: Verbosity,
    pub(crate) layer: u8,
    pub(crate) solver: Option<assura_smt::SolverChoice>,
    pub(crate) watch: bool,
    pub(crate) stats: bool,
    pub(crate) dump_smt: Option<&'a str>,
    pub(crate) show_cores: bool,
    /// Unknown (incl. known limitations) and Timeout fail the check.
    pub(crate) strict: bool,
    /// Directory mode: only files whose header marks SHOWCASE.
    pub(crate) showcase_only: bool,
}

/// Context for verification + diagnostic reporting.
pub(crate) struct VerifyContext<'a> {
    pub(crate) filename: &'a str,
    pub(crate) source: &'a str,
    pub(crate) typed: &'a Option<assura_types::TypedFile>,
    pub(crate) file: &'a Option<assura_parser::ast::SourceFile>,
    pub(crate) diagnostics: &'a mut Vec<assura_diagnostics::Diagnostic>,
    pub(crate) has_errors: &'a mut bool,
    pub(crate) output_mode: OutputMode,
    pub(crate) verbosity: Verbosity,
    /// Full verify options (timeout, string_theory, parallel, etc.) with
    /// layer/solver already resolved from CLI + assura.toml.
    pub(crate) verify_options: assura_config::VerifyOptions,
    pub(crate) show_cores: bool,
    pub(crate) strict: bool,
}
