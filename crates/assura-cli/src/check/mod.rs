//! `assura check` / `check-rust` / project / watch command implementations.
//!
//! Split by concern so agents open the right module:
//! - [`run`] — single-file `assura check` pipeline entry
//! - [`report`] — verify + human/JSON diagnostics, SMT Unknown policy
//! - [`watch`] — watch mode
//! - [`project`] — multi-file project check
//! - [`check_rust`] — `assura check-rust` inline annotations
//! - [`types`] — `CheckOptions` / `VerifyContext`

mod check_rust;
mod project;
mod report;
mod run;
mod types;
mod watch;

#[cfg(test)]
mod tests;

pub(crate) use check_rust::*;
pub(crate) use project::*;
pub(crate) use report::*;
pub(crate) use run::*;
pub(crate) use types::*;
pub(crate) use watch::*;
