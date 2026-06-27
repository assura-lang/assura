//! Wiring functions that run domain and structural checkers on AST source.
//!
//! Each submodule groups related `run_*_checks` functions by category.
//! See [`CHECKER-LAYERS.md`](../CHECKER-LAYERS.md) for how `checks/`,
//! `checkers/`, and `domain/` relate.
//!
//! Prefer [`assura_parser::ast::Decl::clauses`], [`Decl::name`], and
//! [`Decl::params`] (or small helpers below) over open-coding
//! `match &decl.node { Decl::Contract(c) => &c.clauses, ... }` in every
//! checker. That pattern misses new `Decl` variants and duplicates boilerplate.

mod clause_quality;
mod concurrency;
mod core;
mod effects;
mod ffi_error;
mod format;
mod frame_totality;
mod info_flow;
mod linear_typestate;
mod memory;
mod meta;
mod numeric;
mod platform;
mod safety;
mod storage;

pub(crate) use clause_quality::*;
pub(crate) use concurrency::*;
pub(crate) use core::*;
pub(crate) use effects::*;
pub(crate) use ffi_error::*;
pub(crate) use format::*;
pub(crate) use frame_totality::*;
pub(crate) use info_flow::*;
pub(crate) use linear_typestate::*;
pub(crate) use memory::*;
pub(crate) use meta::*;
pub use numeric::collect_table_smt_obligations;
pub(crate) use numeric::*;
pub(crate) use platform::*;
pub(crate) use safety::*;
pub(crate) use storage::*;

use assura_parser::ast::{Clause, Decl, Param};

/// Name + clauses for functions and contracts (common determinism / purity path).
#[inline]
pub(crate) fn fn_or_contract_name_clauses(decl: &Decl) -> Option<(&str, &[Clause])> {
    match decl {
        Decl::FnDef(f) => Some((f.name.as_str(), f.clauses.as_slice())),
        Decl::Contract(c) => Some((c.name.as_str(), c.clauses.as_slice())),
        _ => None,
    }
}

/// Clauses + params for fn / contract / extern (constant-time, effects-adjacent).
#[inline]
pub(crate) fn runtime_decl_clauses_params(decl: &Decl) -> Option<(&[Clause], &[Param])> {
    match decl {
        Decl::FnDef(f) => Some((f.clauses.as_slice(), f.params.as_slice())),
        Decl::Contract(c) => Some((c.clauses.as_slice(), &[])),
        Decl::Extern(e) => Some((e.clauses.as_slice(), e.params.as_slice())),
        _ => None,
    }
}

/// Clauses for contract / fn / block (most domain checkers).
///
/// Blocks store clause-like items in `body`; contracts and functions use
/// their normal clause lists. Extern/bind/etc. return `None`.
#[inline]
pub(crate) fn clauses_contract_fn_block(decl: &Decl) -> Option<&[Clause]> {
    match decl {
        Decl::Contract(c) => Some(c.clauses.as_slice()),
        Decl::FnDef(f) => Some(f.clauses.as_slice()),
        Decl::Block { body, .. } => Some(body.as_slice()),
        _ => None,
    }
}

/// Clauses for contract / fn only (no blocks).
#[inline]
pub(crate) fn clauses_contract_fn(decl: &Decl) -> Option<&[Clause]> {
    match decl {
        Decl::Contract(c) => Some(c.clauses.as_slice()),
        Decl::FnDef(f) => Some(f.clauses.as_slice()),
        _ => None,
    }
}

/// Clauses for contract / fn / extern.
#[inline]
pub(crate) fn clauses_contract_fn_extern(decl: &Decl) -> Option<&[Clause]> {
    match decl {
        Decl::Contract(c) => Some(c.clauses.as_slice()),
        Decl::FnDef(f) => Some(f.clauses.as_slice()),
        Decl::Extern(e) => Some(e.clauses.as_slice()),
        _ => None,
    }
}
