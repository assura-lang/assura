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

/// `call length` / `call len` with one receiver argument.
pub fn is_length_ir_call(func: &str, arity: usize) -> bool {
    arity == 1 && matches!(func, "length" | "len")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::parse_ir_module;

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
        assert!(ctx.callee_ir("helper").is_some());
        assert!(ctx.callee_ir("missing").is_none());
    }
}
