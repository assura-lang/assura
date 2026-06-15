// Structural checker stubs for MASTER-PLAN Phase 2/3; methods are wired in
// as the corresponding tasks are implemented.
#![allow(dead_code)]

//! Analysis pass checker structs.

use std::collections::HashMap;
use std::ops::Range;

use assura_parser::ast::{BinOp, ClauseKind, Decl, Expr, Literal, ServiceItem, UnaryOp};

use crate::{Type, TypeEnv, TypeError};

mod linear;
mod typestate;
mod effects;
mod frame;
mod error_propagation;
mod memory;
mod taint;
mod ffi;
mod interface;
mod security;
mod info_flow;
mod totality;
mod fixed_width;

pub(crate) use linear::*;
pub(crate) use typestate::*;
pub(crate) use effects::*;
pub(crate) use frame::*;
pub(crate) use error_propagation::*;
pub(crate) use memory::*;
pub(crate) use taint::*;
pub(crate) use ffi::*;
pub(crate) use interface::*;
pub(crate) use security::*;
pub(crate) use info_flow::*;
pub(crate) use totality::*;
pub(crate) use fixed_width::*;

// Re-export items that are used by external crates
pub use error_propagation::FrameChecker;
pub use memory::expr_references_var;
pub use taint::TaintLabel;
pub use totality::PendingDecreaseCheck;
