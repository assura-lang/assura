//! Type-aware IR encoding hints from the Layer-0 type environment.

use std::collections::HashMap;

use assura_types::{Type, TypeEnv};

/// Base type name from an IR type string (`List<Int>` → `List`).
pub fn base_type_name(ir_ty: &str) -> &str {
    ir_ty.split('<').next().unwrap_or(ir_ty).trim()
}

/// Struct field layouts and bindings for IR Call/Field/Construct encoding.
#[derive(Clone, Copy, Default)]
pub struct IrTypeContext<'a> {
    struct_fields: Option<&'a HashMap<String, Vec<(String, Type)>>>,
}

impl<'a> IrTypeContext<'a> {
    pub fn from_type_env(env: Option<&'a TypeEnv>) -> Self {
        Self {
            struct_fields: env.map(|e| &e.struct_fields),
        }
    }

    /// Field name at tuple index for a struct-like IR type.
    pub fn field_name_at(&self, ir_type: &str, index: usize) -> Option<&str> {
        let type_name = base_type_name(ir_type);
        let fields = self.struct_fields?.get(type_name)?;
        fields.get(index).map(|(name, _)| name.as_str())
    }

    /// Ordered field names for a named struct/typedef.
    pub fn field_names_for(&self, type_name: &str) -> Option<Vec<&str>> {
        let fields = self.struct_fields?.get(type_name)?;
        Some(fields.iter().map(|(n, _)| n.as_str()).collect())
    }

    pub fn has_struct_layout(&self, type_name: &str) -> bool {
        self.struct_fields
            .and_then(|m| m.get(type_name))
            .is_some_and(|f| !f.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_type_strips_generics() {
        assert_eq!(base_type_name("List<Int>"), "List");
        assert_eq!(base_type_name("Point"), "Point");
    }

    #[test]
    fn field_name_resolves_from_type_env() {
        let mut env = TypeEnv::new();
        env.struct_fields.insert(
            "Point".into(),
            vec![("x".into(), Type::Int), ("y".into(), Type::Int)],
        );
        let ctx = IrTypeContext::from_type_env(Some(&env));
        assert_eq!(ctx.field_name_at("Point", 0), Some("x"));
        assert_eq!(ctx.field_name_at("Point", 1), Some("y"));
        assert_eq!(ctx.field_name_at("Point", 2), None);
    }
}
