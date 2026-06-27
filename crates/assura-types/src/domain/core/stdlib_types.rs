//! T107: Core standard library types.

use std::collections::HashMap;

use crate::Type;

/// Core standard library type definitions (Pos, NonNeg, Email, Uuid, etc.)
#[derive(Debug, Clone)]
pub(crate) struct StdlibTypes {
    types: HashMap<String, StdlibTypeDef>,
}

#[derive(Debug, Clone)]
pub(crate) struct StdlibTypeDef {
    pub name: String,
    pub base_type: Type,
}

impl StdlibTypes {
    pub fn new() -> Self {
        let mut types = HashMap::new();
        // Numeric refinement types
        types.insert(
            "Pos".into(),
            StdlibTypeDef {
                name: "Pos".into(),
                base_type: Type::Int,
            },
        );
        types.insert(
            "NonNeg".into(),
            StdlibTypeDef {
                name: "NonNeg".into(),
                base_type: Type::Int,
            },
        );
        types.insert(
            "Nat".into(),
            StdlibTypeDef {
                name: "Nat".into(),
                base_type: Type::Nat,
            },
        );
        types.insert(
            "Port".into(),
            StdlibTypeDef {
                name: "Port".into(),
                base_type: Type::Int,
            },
        );
        types.insert(
            "Percentage".into(),
            StdlibTypeDef {
                name: "Percentage".into(),
                base_type: Type::Float,
            },
        );
        // String refinement types
        types.insert(
            "Email".into(),
            StdlibTypeDef {
                name: "Email".into(),
                base_type: Type::String,
            },
        );
        types.insert(
            "Uuid".into(),
            StdlibTypeDef {
                name: "Uuid".into(),
                base_type: Type::String,
            },
        );
        types.insert(
            "Url".into(),
            StdlibTypeDef {
                name: "Url".into(),
                base_type: Type::String,
            },
        );
        types.insert(
            "FilePath".into(),
            StdlibTypeDef {
                name: "FilePath".into(),
                base_type: Type::String,
            },
        );
        types.insert(
            "IpAddr".into(),
            StdlibTypeDef {
                name: "IpAddr".into(),
                base_type: Type::String,
            },
        );
        types.insert(
            "Hostname".into(),
            StdlibTypeDef {
                name: "Hostname".into(),
                base_type: Type::String,
            },
        );
        // Byte/buffer types
        types.insert(
            "NonEmptyBytes".into(),
            StdlibTypeDef {
                name: "NonEmptyBytes".into(),
                base_type: Type::Bytes,
            },
        );
        types.insert(
            "BoundedBytes".into(),
            StdlibTypeDef {
                name: "BoundedBytes".into(),
                base_type: Type::Bytes,
            },
        );
        // Timestamp / duration
        types.insert(
            "Timestamp".into(),
            StdlibTypeDef {
                name: "Timestamp".into(),
                base_type: Type::Int,
            },
        );
        types.insert(
            "Duration".into(),
            StdlibTypeDef {
                name: "Duration".into(),
                base_type: Type::Int,
            },
        );
        Self { types }
    }

    pub fn all_types(&self) -> Vec<&StdlibTypeDef> {
        self.types.values().collect()
    }
}

impl Default for StdlibTypes {
    fn default() -> Self {
        Self::new()
    }
}
