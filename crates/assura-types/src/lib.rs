//! Type checking for the Assura contract language.
//!
//! Builds a `TypeEnv` (type environment) from a `ResolvedFile` by mapping
//! each symbol in the symbol table to its `Type`. For T013 this creates the
//! scaffolding: type environment construction and the `type_check` entry
//! point. Actual expression-level type checking (T014-T018) builds on this.

/// Structural checkers: linearity, typestate, effects, info-flow, generics, etc.
///
/// See `CHECKER-LAYERS.md` for how `checkers/`, `checks/`, and `domain/` differ.
pub mod checkers;
/// Wiring functions that run domain and structural checkers.
mod checks;
/// Clause-body type checking (requires/ensures/invariant expressions).
pub mod clauses;
/// Type conversion functions (AST TypeExpr, HIR HirType, raw tokens -> Type).
pub(crate) mod convert;
/// Domain-specific checkers (memory, concurrency, security, formatting, etc.).
pub mod domain;
/// Type environment construction.
pub(crate) mod env;
/// Generic type instantiation and arity checking.
pub(crate) mod generics;
/// Ghost and lemma function effect checking.
pub(crate) mod ghost_effects;
/// Expression type inference (`infer_expr`).
pub mod inference;
/// Type checking pipeline entry points.
mod pipeline;
/// Core type definitions (Type, TypeEnv, TypeError, TypedFile).
mod types;

// ---- Re-exports: maintain the exact public API ----

// From types module
pub use types::{Type, TypeEnv, TypeError, TypedFile};

// From checkers module
pub use checkers::{FrameChecker, PendingDecreaseCheck, TaintLabel};

// From domain module
pub use domain::{GeneratedTest, TestGenerator, TestKind, TestableContract};

// From convert module (pub(crate) items accessed within crate)
pub(crate) use convert::parse_type_tokens;
pub(crate) use convert::type_from_hir_type;

// From inference module
pub(crate) use inference::*;

// From generics module (only used in tests)
#[cfg(test)]
pub(crate) use generics::{check_generic_instantiation, instantiate_builtin_generic, substitute};

// From ghost_effects module
pub(crate) use ghost_effects::{check_ghost_fn_effects, check_lemma_fn_effects};

// From pipeline module
pub use pipeline::{
    type_check, type_check_hir, type_check_hir_with_config, type_check_with_config,
    type_check_with_modules,
};

// Test-only re-exports: make checker types, domain types, and internal
// functions visible to the tests/ module via `use super::*;`.
#[cfg(test)]
pub(crate) use checkers::*;
#[cfg(test)]
pub(crate) use checks::*;
#[cfg(test)]
pub(crate) use domain::*;

#[cfg(test)]
mod tests;
