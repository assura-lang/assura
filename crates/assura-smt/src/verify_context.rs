//! Shared verification context for per-contract solver dispatch.

use std::collections::HashMap;

use assura_parser::ast::{Clause, ClauseKind, Expr, Param};
use assura_types::TypeEnv;

use crate::cache::SessionCache;
use crate::cvc5_verify_shared::Cvc5ContractPrepared;
use crate::havoc_assume::HavocAssumeInput;
use crate::ir::{IrFunction, IrInstr};
use crate::ir_encode::IrEncodeContext;
use crate::ir_type_ctx::IrTypeContext;
use crate::VerifyFileExtras;

/// Per-contract loaded IR context for havoc+assume encoding.
#[must_use]
pub struct LoadedIrContext<'a> {
    body: Option<&'a IrFunction>,
    blocks: Option<&'a HashMap<usize, Vec<IrInstr>>>,
    bodies: Option<&'a HashMap<String, IrFunction>>,
    type_env: Option<&'a TypeEnv>,
}

impl<'a> LoadedIrContext<'a> {
    /// Build per-contract IR context from file-level extras and optional type env fallback.
    pub fn for_contract(
        contract_name: &str,
        extras: Option<&VerifyFileExtras<'a>>,
        type_env_fallback: Option<&'a TypeEnv>,
    ) -> Option<Self> {
        let bodies = extras.and_then(|e| e.ir_bodies);
        let block_maps = extras.and_then(|e| e.ir_blocks);
        let type_env = extras.and_then(|e| e.type_env).or(type_env_fallback);
        let body = bodies.and_then(|m| m.get(contract_name));
        let blocks = block_maps.and_then(|m| m.get(contract_name));
        if body.is_none() && blocks.is_none() && bodies.is_none() && type_env.is_none() {
            return None;
        }
        Some(Self {
            body,
            blocks,
            bodies,
            type_env,
        })
    }

    /// Test helper: IR body only, no file-level extras.
    #[cfg(test)]
    pub fn with_body(body: &'a IrFunction) -> Self {
        Self {
            body: Some(body),
            blocks: None,
            bodies: None,
            type_env: None,
        }
    }

    pub fn body(&self) -> Option<&'a IrFunction> {
        self.body
    }

    pub fn blocks(&self) -> Option<&'a HashMap<usize, Vec<IrInstr>>> {
        self.blocks
    }

    pub fn bodies(&self) -> Option<&'a HashMap<String, IrFunction>> {
        self.bodies
    }

    pub fn type_env(&self) -> Option<&'a TypeEnv> {
        self.type_env
    }

    pub fn type_ctx(&self) -> IrTypeContext<'a> {
        IrTypeContext::from_type_env(self.type_env)
    }

    pub fn enc_ctx(&self) -> IrEncodeContext<'a> {
        IrEncodeContext::new(self.type_env, self.bodies, self.blocks)
    }
}

/// Per-contract verification context shared by Z3 and CVC5 dispatch paths.
pub struct ContractVerifyContext<'a> {
    pub contract_name: &'a str,
    pub clauses: &'a [Clause],
    pub params: &'a [Param],
    pub return_ty: &'a [String],
    pub constants: &'a [(String, i64)],
    pub ir: Option<LoadedIrContext<'a>>,
}

impl<'a> ContractVerifyContext<'a> {
    /// Backward-compat accessor for per-contract IR body.
    pub fn ir_body(&self) -> Option<&'a IrFunction> {
        self.ir.as_ref().and_then(LoadedIrContext::body)
    }

    /// Backward-compat accessor for per-contract `fn #N` block map.
    pub fn ir_blocks(&self) -> Option<&'a HashMap<usize, Vec<IrInstr>>> {
        self.ir.as_ref().and_then(LoadedIrContext::blocks)
    }

    /// Backward-compat accessor for file-level IR bodies (call inlining).
    pub fn ir_bodies(&self) -> Option<&'a HashMap<String, IrFunction>> {
        self.ir.as_ref().and_then(LoadedIrContext::bodies)
    }

    /// Backward-compat accessor for layer-0 type environment.
    pub fn type_env(&self) -> Option<&'a TypeEnv> {
        self.ir.as_ref().and_then(LoadedIrContext::type_env)
    }
}

/// CVC5 contract verification session (contract + prepared state + cache).
pub(crate) struct Cvc5ContractVerifySession<'a> {
    pub contract: &'a ContractVerifyContext<'a>,
    pub prepared: Cvc5ContractPrepared<'a>,
    pub lemma_defs: Option<&'a HashMap<String, Vec<&'a Expr>>>,
    pub cache: &'a mut SessionCache,
}

impl<'a> Cvc5ContractVerifySession<'a> {
    pub fn new(
        contract: &'a ContractVerifyContext<'a>,
        prepared: Cvc5ContractPrepared<'a>,
        lemma_defs: Option<&'a HashMap<String, Vec<&'a Expr>>>,
        cache: &'a mut SessionCache,
    ) -> Self {
        Self {
            contract,
            prepared,
            lemma_defs,
            cache,
        }
    }

    pub fn havoc_assume_input(&self) -> HavocAssumeInput<'_> {
        let ir = self.contract.ir.as_ref();
        HavocAssumeInput {
            requires: &self.prepared.requires_clauses,
            ensures: &self.prepared.ensures_clauses,
            return_ty: self.contract.return_ty,
            param_names: &self.prepared.param_names,
            ir: ir.and_then(LoadedIrContext::body),
            enc_ctx: ir.map(LoadedIrContext::enc_ctx).unwrap_or_default(),
        }
    }
}

/// Per-clause verification input for CVC5 native and shell-out backends.
pub(crate) struct Cvc5ClauseVerifyInput<'a> {
    pub desc: &'a str,
    pub body: &'a Expr,
    pub kind: ClauseKind,
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_types::TypeEnv;

    #[test]
    fn for_contract_type_env_fallback_without_extras() {
        let env = TypeEnv::new();
        let ctx = LoadedIrContext::for_contract("T", None, Some(&env)).expect("type env");
        assert!(ctx.body().is_none());
        assert!(ctx.blocks().is_none());
        assert!(ctx.type_env().is_some());
    }

    #[test]
    fn for_contract_returns_none_when_empty() {
        assert!(LoadedIrContext::for_contract("T", None, None).is_none());
    }
}