//! Shared verification context for per-contract solver dispatch.

use std::collections::HashMap;

use assura_parser::ast::{Clause, ClauseKind, Expr, Param};

use crate::cache::SessionCache;
use crate::cvc5_verify_shared::Cvc5ContractPrepared;
use crate::havoc_assume::HavocAssumeInput;
use crate::ir_encode::IrEncodeContext;

/// Per-contract verification context shared by Z3 and CVC5 dispatch paths.
pub struct ContractVerifyContext<'a> {
    pub contract_name: &'a str,
    pub clauses: &'a [Clause],
    pub params: &'a [Param],
    pub return_ty: &'a [String],
    pub constants: &'a [(String, i64)],
    pub ir_body: Option<&'a crate::ir::IrFunction>,
    pub ir_blocks: Option<&'a HashMap<usize, Vec<crate::ir::IrInstr>>>,
    pub ir_bodies: Option<&'a HashMap<String, crate::ir::IrFunction>>,
    pub type_env: Option<&'a assura_types::TypeEnv>,
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
        HavocAssumeInput {
            requires: &self.prepared.requires_clauses,
            ensures: &self.prepared.ensures_clauses,
            return_ty: self.contract.return_ty,
            param_names: &self.prepared.param_names,
            ir: self.contract.ir_body,
            enc_ctx: IrEncodeContext::new(
                self.contract.type_env,
                self.contract.ir_bodies,
                self.contract.ir_blocks,
            ),
        }
    }
}

/// Per-clause verification input for CVC5 native and shell-out backends.
pub(crate) struct Cvc5ClauseVerifyInput<'a> {
    pub desc: &'a str,
    pub body: &'a Expr,
    pub kind: ClauseKind,
}
