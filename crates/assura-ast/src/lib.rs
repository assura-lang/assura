//! Assura AST types. These are the canonical compiler IR types used by
//! codegen, smt, types, etc. Parser produces them; downstream crates
//! should depend on assura-ast (parser reexports for convenience).

pub mod ast;

pub use ast::*;
