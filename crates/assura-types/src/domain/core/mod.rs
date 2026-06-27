//! Core domain checkers.
//!
//! AxiomaticDefChecker, OpaqueFunctionChecker, TestGenerator,
//! StdlibTypes, CollectionContracts, CrudAuthContracts.

mod axiomatic_def;
mod collection_contracts;
mod crud_auth;
mod opaque_function;
mod prophecy_resolution;
mod quantifier_trigger;
mod stdlib_types;
mod test_generator;

pub(crate) use axiomatic_def::*;
pub(crate) use collection_contracts::*;
pub(crate) use crud_auth::*;
pub(crate) use opaque_function::*;
pub(crate) use prophecy_resolution::*;
pub(crate) use stdlib_types::*;

pub use quantifier_trigger::QuantifierTriggerChecker;
pub use test_generator::{GeneratedTest, TestGenerator, TestKind, TestableContract};
