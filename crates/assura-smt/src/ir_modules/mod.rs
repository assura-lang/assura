//! Implementation IR modules: encoding, execution, codegen, and templates.

pub mod ir_codegen;
pub mod ir_encode;
pub mod ir_exec;
pub mod ir_generate;
pub mod ir_loader;
pub mod ir_lower;
pub mod ir_templates;
pub mod ir_type_ctx;

#[cfg(test)]
pub mod ir_parity;
