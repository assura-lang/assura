// Structural checker stubs for MASTER-PLAN Phase 2/3; methods are wired in
// as the corresponding tasks are implemented.

//! Analysis pass checker structs.

use std::collections::HashMap;
use std::ops::Range;

use assura_parser::ast::{
    BinOp, ClauseKind, Decl, Expr, ExprVisitor, Literal, ServiceItem, UnaryOp,
};

use crate::{Type, TypeEnv, TypeError};

/// Unified error type for all checker structs. All 16 previous per-checker
/// error types (TypestateError, EffectError, MemoryError, etc.) are now
/// type aliases for this single struct.
#[derive(Debug, Clone)]
pub(crate) struct CheckerError {
    pub code: assura_diagnostics::ErrorCode,
    pub message: String,
    pub span: Range<usize>,
}

impl CheckerError {
    /// Enrich the error message with a prefix while preserving all other fields.
    pub fn with_context(self, context: &str) -> Self {
        Self {
            message: format!("{context}: {}", self.message),
            ..self
        }
    }
}

impl From<CheckerError> for TypeError {
    fn from(e: CheckerError) -> Self {
        TypeError {
            code: e.code,
            message: e.message,
            span: e.span,
            secondary: None,
        }
    }
}

mod effects;
mod error_propagation;
mod ffi;
mod fixed_width;
mod frame;
mod info_flow;
mod interface;
mod linear;
mod memory;
mod security;
mod taint;
mod totality;
mod typestate;

pub(crate) use effects::*;
pub(crate) use error_propagation::*;
pub(crate) use ffi::*;
pub(crate) use fixed_width::*;
pub(crate) use frame::*;
pub(crate) use info_flow::*;
pub(crate) use interface::*;
pub(crate) use linear::*;
pub(crate) use memory::*;
pub(crate) use security::*;
pub(crate) use taint::*;
pub(crate) use totality::*;
pub(crate) use typestate::*;

// Re-export items that are used by external crates
pub use error_propagation::FrameChecker;
pub use memory::expr_references_var;
pub use taint::TaintLabel;
pub use totality::PendingDecreaseCheck;
