//! Re-export of the canonical AST from assura-ast (the compiler IR crate).
//! This keeps `assura_parser::ast::*` working for backward compat while
//! allowing codegen/smt to depend only on assura-ast.

pub use assura_ast::*;
