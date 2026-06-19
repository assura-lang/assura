//! Shared helpers for encoding Implementation IR expressions in SMT.

use std::collections::HashMap;

use crate::ir::{IrFunction, IrInstr, IrModule};

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
}
