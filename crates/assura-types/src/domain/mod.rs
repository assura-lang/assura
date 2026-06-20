//! Domain-specific type checkers.
//!
//! Each checker validates contracts against a specific domain (memory,
//! concurrency, formatting, security, etc.). They are self-contained
//! structs that operate on AST nodes and produce `Vec<TypeError>`.
//!
//! See [`CHECKER-LAYERS.md`](../CHECKER-LAYERS.md) for how `domain/`,
//! `checkers/`, and `checks/` relate.

mod concurrency;
mod core;
mod format;
mod memory;
mod meta;
mod numeric;
mod platform;
mod safety;
mod storage;
#[cfg(test)]
mod tests;

pub(crate) use concurrency::*;
pub(crate) use core::*;
pub(crate) use format::*;
pub(crate) use memory::*;
pub(crate) use meta::*;
pub(crate) use numeric::*;
pub(crate) use platform::*;
pub(crate) use safety::*;
pub(crate) use storage::*;

// Re-export public items
pub use core::{
    GeneratedTest, QuantifierTriggerChecker, TestGenerator, TestKind, TestableContract,
};
