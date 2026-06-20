//! Wiring functions that run domain and structural checkers on AST source.
//!
//! Each submodule groups related `run_*_checks` functions by category.
//! See [`CHECKER-LAYERS.md`](../CHECKER-LAYERS.md) for how `checks/`,
//! `checkers/`, and `domain/` relate.

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
pub(crate) use numeric::*;
pub(crate) use platform::*;
pub(crate) use safety::*;
pub(crate) use storage::*;
