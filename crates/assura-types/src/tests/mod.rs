use super::*;
use crate::clauses::{check_clause_expr, extract_output_type_from_body};
use crate::inference::{infer_expr, types_compatible};

// Re-export AST type aliases used across many test submodules
pub(super) use assura_parser::ast::{
    BinOp as AstBinOp, Clause as AstClause, Expr as AstExpr, FnDef as AstFnDef, Literal as AstLit,
    Param as AstParam, UnaryOp as AstUnOp,
};

/// Helper: parse + resolve source text, panicking on errors.
pub(super) fn resolve_ok(source: &str) -> ResolvedFile {
    let file = assura_parser::parse_unwrap(source);
    assura_resolve::resolve(&file).expect("resolve should succeed")
}

mod basics;
mod inference;
mod field_call;
mod generics;
mod patterns_clauses;
mod linear;
mod typestate_effects;
mod interactions;
mod security;
mod domain_checkers;
mod wiring;
mod integration;
