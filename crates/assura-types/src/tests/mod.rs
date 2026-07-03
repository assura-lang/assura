use std::collections::HashMap;
use std::ops::Range;

use super::*;
use crate::clauses::{
    check_clause_expr, collect_input_param_types, extract_output_type_from_body,
    register_input_clause_params,
};
use crate::inference::{infer_expr, types_compatible};

// Re-export AST types used across many test submodules
pub(super) use assura_parser::ast::{
    BinOp, BinOp as AstBinOp, Clause as AstClause, ClauseKind, Decl, Expr, Expr as AstExpr,
    FnDef as AstFnDef, Literal as AstLit, Param as AstParam, SpExpr, Spanned, UnaryOp as AstUnOp,
};
pub(super) use assura_resolve::ResolvedFile;

/// Helper: parse + resolve source text, panicking on errors.
///
/// Implemented in-crate (no `assura-test-support`) so this package can be
/// published without path deps on unpublished workspace crates. Do **not**
/// return `TypedFile` from a support crate that depends on `assura-types`
/// (different type instance). Happy-path: `resolve_ok` + `type_check`.
pub(super) fn resolve_ok(source: &str) -> ResolvedFile {
    let file = assura_parser::parse_unwrap(source);
    assura_resolve::resolve(&file).expect("resolve should succeed")
}

mod basics;
mod domain_checkers;
mod field_call;
mod generics;
mod inference;
mod integration;
mod interactions;
mod linear;
mod patterns_clauses;
mod security;
mod typestate_effects;
mod wiring;
