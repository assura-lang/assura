//! Clause/param extraction and verify extras builders shared by entry paths.

use assura_ast::{Clause, ClauseKind, Expr, Param, TypeExpr};
use assura_types::TypedFile;

/// Extract the return type from `output(result: Nat)` clauses in a contract.
///
/// Contracts declare their output type via `output(result: Nat)` instead of
/// a function return type. The clause body is `Expr::Raw(["result", ":", "Nat"])`.
pub(crate) fn extract_output_return_type(clauses: &[Clause]) -> Vec<String> {
    for clause in clauses {
        if clause.kind == ClauseKind::Output
            && let Expr::Raw(tokens) = &clause.body.node
        {
            if tokens.len() >= 3 && tokens[1] == ":" {
                return tokens[2..].to_vec();
            }
            return tokens.clone();
        }
    }
    Vec::new()
}

/// Extract parameters from `input(raw_data: Bytes)` clauses in a contract.
pub(crate) fn extract_input_params(clauses: &[Clause]) -> Vec<Param> {
    for clause in clauses {
        if clause.kind == ClauseKind::Input
            && let Expr::Raw(tokens) = &clause.body.node
        {
            let mut params = Vec::new();
            let mut i = 0;
            while i < tokens.len() {
                if tokens[i] == "," {
                    i += 1;
                    continue;
                }
                let name = tokens[i].clone();
                i += 1;
                if i < tokens.len() && tokens[i] == ":" {
                    i += 1;
                    let mut ty_tokens = Vec::new();
                    while i < tokens.len() && tokens[i] != "," {
                        ty_tokens.push(tokens[i].clone());
                        i += 1;
                    }
                    params.push(Param {
                        name,
                        ty: simple_type_from_tokens(&ty_tokens),
                    });
                } else {
                    params.push(Param { name, ty: None });
                }
            }
            return params;
        }
    }
    Vec::new()
}

/// Convert raw type tokens to a `TypeExpr` (simplified parser for SMT-internal use).
fn simple_type_from_tokens(tokens: &[String]) -> Option<TypeExpr> {
    if tokens.is_empty() {
        return None;
    }
    if tokens.len() == 1 {
        return Some(TypeExpr::Named(tokens[0].clone()));
    }
    // Multi-token: join as a named type for SMT purposes
    Some(TypeExpr::Named(tokens.join(" ")))
}

/// Convert `Option<TypeExpr>` to `Vec<String>` tokens (bridge for SMT internals).
pub(crate) fn type_expr_to_token_vec(te: Option<&TypeExpr>) -> Vec<String> {
    te.map(|t| t.to_tokens()).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Optional per-file inputs discovered outside the typed AST (e.g. IR sidecars).
#[derive(Debug, Default, Clone, Copy)]
pub struct VerifyFileExtras<'a> {
    pub ir_bodies: Option<&'a std::collections::HashMap<String, crate::ir::IrFunction>>,
    /// Block bodies (`fn #N`) from multi-function IR modules, keyed by contract name.
    pub ir_blocks: Option<
        &'a std::collections::HashMap<
            String,
            std::collections::HashMap<usize, Vec<crate::ir::IrInstr>>,
        >,
    >,
    /// Layer-0 type environment for HIR/type-aware IR encoding.
    pub type_env: Option<&'a assura_types::TypeEnv>,
    /// Whether IR sidecar loading was attempted (source path was provided).
    /// When true, ensures-with-result clauses without an IR body are skipped
    /// with Unknown instead of being sent to the solver (#703).
    pub ir_loading_attempted: bool,
}

/// Build optional IR/type extras for a verify pass.
pub(crate) fn build_verify_extras<'a>(
    typed: &'a TypedFile,
    loaded: Option<&'a crate::ir_loader::LoadedVerifyExtras>,
    ir_loading_attempted: bool,
) -> VerifyFileExtras<'a> {
    VerifyFileExtras {
        ir_bodies: loaded.filter(|l| !l.is_empty()).map(|l| &l.ir_map),
        ir_blocks: loaded.filter(|l| !l.is_empty()).map(|l| &l.block_map),
        type_env: Some(&typed.type_env),
        ir_loading_attempted,
    }
}
