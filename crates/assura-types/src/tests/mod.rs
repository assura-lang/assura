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
/// Delegates to [`assura_test_support::resolve_ok`] so tests share the same
/// parse/resolve entry as other crates. Do **not** add a `typecheck_ok` shim
/// that returns [`TypedFile`] from `assura_test_support`: that crate depends
/// on `assura-types` via the pipeline, so the returned `TypedFile` is a
/// *different type instance* than this crate under test (same footgun as
/// `codegen_ok` inside `assura-codegen` tests). For happy-path type checks
/// here, use `resolve_ok` + `type_check(resolved).expect(...)`. For
/// error-code-only negative tests, call `assura_test_support::expect_type_errors`
/// / `compile_result` directly (inspect codes via support helpers, not
/// `TypedFile`).
pub(super) fn resolve_ok(source: &str) -> ResolvedFile {
    assura_test_support::resolve_ok(source)
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
