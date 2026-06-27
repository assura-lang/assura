//! Shared helpers for encoding Implementation IR expressions in SMT.

use std::collections::HashMap;

use assura_types::TypeEnv;

use crate::ir::{IrFunction, IrInstr, IrModule};
use crate::ir_type_ctx::IrTypeContext;

/// Type + cross-function IR context for havoc+assume encoding.
#[derive(Clone, Copy, Default)]
pub struct IrEncodeContext<'a> {
    pub type_ctx: IrTypeContext<'a>,
    pub ir_bodies: Option<&'a HashMap<String, IrFunction>>,
    pub ir_blocks: Option<&'a HashMap<usize, Vec<IrInstr>>>,
}

impl<'a> IrEncodeContext<'a> {
    pub fn new(
        type_env: Option<&'a TypeEnv>,
        ir_bodies: Option<&'a HashMap<String, IrFunction>>,
        ir_blocks: Option<&'a HashMap<usize, Vec<IrInstr>>>,
    ) -> Self {
        Self {
            type_ctx: IrTypeContext::from_type_env(type_env),
            ir_bodies,
            ir_blocks,
        }
    }

    /// Lookup a callee IR body by function name for `call` inlining.
    pub fn callee_ir(&self, name: &str) -> Option<&IrFunction> {
        self.ir_bodies?.get(name)
    }
}

/// Whether an IR type name denotes a collection-like value with `.length()`.
pub fn is_collection_ir_type(ty: &str) -> bool {
    matches!(ty, "Bytes" | "String" | "List" | "Map" | "Set")
        || ty.starts_with("List<")
        || ty.starts_with("Map<")
        || ty.starts_with("Set<")
}

/// `call length` / `call len` (and other length method aliases) with one receiver argument.
pub fn is_length_ir_call(func: &str, arity: usize) -> bool {
    arity == 1 && crate::encode_atom_policy::is_length_method_name(func)
}

/// Build a map of block id -> instruction body from all `fn #N` entries in a module.
pub fn block_map_from_module(module: &IrModule) -> HashMap<usize, Vec<IrInstr>> {
    let mut blocks = HashMap::new();
    for func in &module.functions {
        if let Some(rest) = func.id.strip_prefix('#')
            && let Ok(id) = rest.parse::<usize>()
        {
            blocks.insert(id, func.body.clone());
        }
    }
    blocks
}

/// Slot index -> declared IR type for parameter slots.
pub fn slot_type_map(func: &IrFunction) -> HashMap<usize, String> {
    func.params.iter().map(|p| (p.slot, p.ty.clone())).collect()
}

/// Shared fixture: `fn #0` with `if` branching to sibling `fn #1` / `fn #2`.
///
/// Used by Z3, CVC5 shell, and CVC5 native `ir_blocks` inlining parity tests.
#[cfg(test)]
pub(crate) fn branch_if_else_ir_fixture() -> (IrFunction, HashMap<usize, Vec<IrInstr>>) {
    use crate::ir::parse_ir_module;

    const SOURCE: &str = r#"
module branch {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $1 = if $0 then #1 else #2 : Int
    $result = load $1 : Int
  }
  fn #1 : ($0: Int) -> Int ! pure
  {
    $result = load $0 : Int
  }
  fn #2 : ($0: Int) -> Int ! pure
  {
    $result = const 0 : Int
  }
}
"#;
    let module = parse_ir_module(SOURCE).unwrap();
    let func = module.functions[0].clone();
    let blocks = block_map_from_module(&module);
    (func, blocks)
}

/// Fixture: `fn #0` with `if #99 else #100` but no block map (UF fallback path).
#[cfg(test)]
pub(crate) fn branch_if_else_missing_blocks_fixture() -> IrFunction {
    use crate::ir::parse_ir_module;

    const SOURCE: &str = r#"
module branch_missing {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $1 = if $0 then #99 else #100 : Int
    $result = load $1 : Int
  }
}
"#;
    parse_ir_module(SOURCE).unwrap().functions[0].clone()
}

/// Assert missing block ids fall back to opaque `__ir_block_{N}` UFs (#296).
#[cfg(test)]
pub(crate) fn assert_ir_blocks_missing_uf_fallback(output: &str) {
    assert!(
        output.contains("__ir_block_99"),
        "missing then-block should use UF fallback, got:\n{output}"
    );
    assert!(
        output.contains("__ir_block_100"),
        "missing else-block should use UF fallback, got:\n{output}"
    );
}

/// Assert if-branch sibling bodies use block-local result slots (#297).
#[cfg(test)]
pub(crate) fn assert_ir_branch_results_scoped(output: &str) {
    assert!(
        output.contains("__ir_block1_result"),
        "then-branch should use block-local result, got:\n{output}"
    );
    assert!(
        output.contains("__ir_block2_result"),
        "else-branch should use block-local result, got:\n{output}"
    );
    let binds_x_to_main_result =
        output.contains("(= x result)") || output.contains("(= |x| |result|)");
    let binds_zero_to_main_result =
        output.contains("(= 0 result)") || output.contains("(= |0| |result|)");
    assert!(
        !(binds_x_to_main_result && binds_zero_to_main_result),
        "must not bind both x and 0 to main result unconditionally, got:\n{output}"
    );
}

/// Assert sibling `fn #N` bodies were inlined (not opaque `__ir_block_N` UFs).
#[cfg(test)]
pub(crate) fn assert_ir_blocks_inlined(output: &str, axiom_count: usize) {
    assert!(
        axiom_count >= 3,
        "expected multiple IR block axioms, got {axiom_count}"
    );
    assert!(
        output.contains("ite"),
        "expected if-expr lowering, got:\n{output}"
    );
    assert!(
        !output.contains("__ir_block_1") && !output.contains("__ir_block_2"),
        "inlined blocks must not use opaque __ir_block_N UFs, got:\n{output}"
    );
    assert_ir_branch_results_scoped(output);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::parse_ir_module;

    #[test]
    fn branch_if_else_fixture_has_three_blocks() {
        let (_, blocks) = branch_if_else_ir_fixture();
        assert_eq!(blocks.len(), 3);
        assert!(blocks.contains_key(&1));
        assert!(blocks.contains_key(&2));
    }

    #[test]
    fn block_map_collects_sibling_functions() {
        let module = parse_ir_module(
            r#"
module m {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $result = load $0 : Int
  }
  fn #1 : () -> Int ! pure
  {
    $result = const 1 : Int
  }
}
"#,
        )
        .unwrap();
        let blocks = block_map_from_module(&module);
        assert_eq!(blocks.len(), 2);
        assert!(blocks.contains_key(&0));
        assert!(blocks.contains_key(&1));
    }

    #[test]
    fn length_call_classification() {
        assert!(is_length_ir_call("length", 1));
        assert!(is_length_ir_call("len", 1));
        assert!(!is_length_ir_call("length", 2));
    }

    #[test]
    fn callee_ir_lookup_from_bodies_map() {
        let module = parse_ir_module(
            r#"
module helper {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $result = load $0 : Int
  }
}
"#,
        )
        .unwrap();
        let mut bodies = HashMap::new();
        bodies.insert("helper".into(), module.functions[0].clone());
        let ctx = IrEncodeContext::new(None, Some(&bodies), None);
        ctx.callee_ir("helper").unwrap();
        assert!(ctx.callee_ir("missing").is_none());
    }
}
