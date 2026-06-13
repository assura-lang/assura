//! Type checking for the Assura contract language.
//!
//! Builds a `TypeEnv` (type environment) from a `ResolvedFile` by mapping
//! each symbol in the symbol table to its `Type`. For T013 this creates the
//! scaffolding: type environment construction and the `type_check` entry
//! point. Actual expression-level type checking (T014-T018) builds on this.

use std::collections::HashMap;
use std::ops::Range;

use assura_resolve::{ResolvedFile, SymbolKind, SymbolTable};

// ---------------------------------------------------------------------------
// Type representation
// ---------------------------------------------------------------------------

/// Represents all Assura types.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    // --- Base types ---
    Int,
    Nat,
    Float,
    Bool,
    String,
    Bytes,
    Unit,
    Never,

    // --- Fixed-width integers ---
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,

    // --- Generic container types ---
    List(Box<Type>),
    Map(Box<Type>, Box<Type>),
    Set(Box<Type>),
    Option(Box<Type>),
    Result(Box<Type>, Box<Type>),

    // --- Sequence (used in demos) ---
    Sequence(Box<Type>),

    // --- User-defined named type ---
    Named(std::string::String),

    // --- Generic type parameter ---
    TypeParam(std::string::String),

    // --- Function type ---
    Fn {
        params: Vec<Type>,
        ret: Box<Type>,
    },

    // --- Refined type: base type with predicate ---
    Refined {
        base: Box<Type>,
        predicate: std::string::String,
    },

    // --- Unknown / error recovery placeholder ---
    Unknown,
}

// ---------------------------------------------------------------------------
// Type environment
// ---------------------------------------------------------------------------

/// Maps names to their types. This is the typing context built during
/// type checking.
#[derive(Debug, Clone, Default)]
pub struct TypeEnv {
    /// Maps symbol name -> Type.
    pub bindings: HashMap<std::string::String, Type>,
}

impl TypeEnv {
    /// Create an empty type environment.
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
        }
    }

    /// Insert a binding. Returns the previous type if the name was already
    /// bound.
    pub fn insert(&mut self, name: std::string::String, ty: Type) -> Option<Type> {
        self.bindings.insert(name, ty)
    }

    /// Look up a name in the environment.
    pub fn lookup(&self, name: &str) -> Option<&Type> {
        self.bindings.get(name)
    }

    /// Number of bindings.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Returns true if the environment has no bindings.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Type errors
// ---------------------------------------------------------------------------

/// A structured type error with error code, span, and message.
#[derive(Debug, Clone)]
pub struct TypeError {
    /// Error code from the spec (A03xxx series).
    pub code: std::string::String,
    /// Human-readable error message.
    pub message: std::string::String,
    /// Source location where the error was detected.
    pub span: Range<usize>,
    /// Optional secondary span with label (e.g., "expected type declared here").
    pub secondary: Option<(Range<usize>, std::string::String)>,
}

// ---------------------------------------------------------------------------
// Typed file
// ---------------------------------------------------------------------------

/// The result of successful type checking: the resolved file plus the
/// type environment constructed from its symbols.
#[derive(Debug, Clone)]
pub struct TypedFile {
    pub resolved: ResolvedFile,
    pub type_env: TypeEnv,
}

// ---------------------------------------------------------------------------
// Built-in type mapping
// ---------------------------------------------------------------------------

/// Map a built-in type name to its `Type` representation.
fn builtin_type(name: &str) -> Option<Type> {
    match name {
        "Int" => Some(Type::Int),
        "Nat" => Some(Type::Nat),
        "Float" => Some(Type::Float),
        "Bool" => Some(Type::Bool),
        "String" => Some(Type::String),
        "Bytes" => Some(Type::Bytes),
        "Unit" => Some(Type::Unit),
        "Never" => Some(Type::Never),
        "U8" => Some(Type::U8),
        "U16" => Some(Type::U16),
        "U32" => Some(Type::U32),
        "U64" => Some(Type::U64),
        "I8" => Some(Type::I8),
        "I16" => Some(Type::I16),
        "I32" => Some(Type::I32),
        "I64" => Some(Type::I64),
        "F32" => Some(Type::F32),
        "F64" => Some(Type::F64),
        // Generic container types get `Unknown` as their type argument
        // until T014+ refines this with actual type argument resolution.
        "List" => Some(Type::List(Box::new(Type::Unknown))),
        "Map" => Some(Type::Map(Box::new(Type::Unknown), Box::new(Type::Unknown))),
        "Set" => Some(Type::Set(Box::new(Type::Unknown))),
        "Option" => Some(Type::Option(Box::new(Type::Unknown))),
        "Result" => Some(Type::Result(
            Box::new(Type::Unknown),
            Box::new(Type::Unknown),
        )),
        "Sequence" => Some(Type::Sequence(Box::new(Type::Unknown))),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Type environment construction
// ---------------------------------------------------------------------------

/// Build a `TypeEnv` from a resolved symbol table.
///
/// Walks every symbol and assigns it a `Type` based on its kind:
/// - `BuiltinType`: mapped via `builtin_type()`
/// - `TypeDef`, `ContractDef`, `ServiceDef`, `EnumDef`: `Type::Named(name)`
/// - `FnDef`, `ExternFn`: `Type::Fn { params: [], ret: Unknown }`
///   (parameter types are not yet resolved from raw token sequences)
/// - `TypeParam`: `Type::TypeParam(name)`
/// - `Parameter`, `Field`: `Type::Unknown` (refined in T014+)
/// - `Operation`, `Query`: `Type::Fn { params: [], ret: Unknown }`
/// - `EnumVariant`: `Type::Named(name)` (constructor)
fn build_type_env(symbols: &SymbolTable) -> TypeEnv {
    let mut env = TypeEnv::new();

    for sym in &symbols.symbols {
        let ty = match sym.kind {
            SymbolKind::BuiltinType => builtin_type(&sym.name).unwrap_or(Type::Unknown),
            SymbolKind::TypeDef
            | SymbolKind::ContractDef
            | SymbolKind::ServiceDef
            | SymbolKind::EnumDef => Type::Named(sym.name.clone()),

            SymbolKind::FnDef | SymbolKind::ExternFn => Type::Fn {
                params: Vec::new(),
                ret: Box::new(Type::Unknown),
            },

            SymbolKind::Operation | SymbolKind::Query => Type::Fn {
                params: Vec::new(),
                ret: Box::new(Type::Unknown),
            },

            SymbolKind::TypeParam => Type::TypeParam(sym.name.clone()),

            SymbolKind::Parameter | SymbolKind::Field => Type::Unknown,

            SymbolKind::EnumVariant => Type::Named(sym.name.clone()),
        };

        env.insert(sym.name.clone(), ty);
    }

    env
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Type-check a resolved file.
///
/// Builds a type environment from the symbol table. For T013 this always
/// succeeds (no expression-level checking yet). Returns a `TypedFile`
/// containing the resolved file and its type environment, or a list of
/// `TypeError`s.
pub fn type_check(resolved: &ResolvedFile) -> Result<TypedFile, Vec<TypeError>> {
    let type_env = build_type_env(&resolved.symbols);

    // T013: no expression-level type checking yet; always succeeds.
    // T014+ will add actual checking and may produce TypeErrors.

    Ok(TypedFile {
        resolved: resolved.clone(),
        type_env,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse + resolve source text, panicking on errors.
    fn resolve_ok(source: &str) -> ResolvedFile {
        let (file, errs) = assura_parser::parse(source);
        assert!(errs.is_empty(), "unexpected parse errors: {errs:?}");
        let file = file.expect("parse returned None");
        assura_resolve::resolve(&file).expect("resolve should succeed")
    }

    #[test]
    fn empty_file_type_checks() {
        let resolved = resolve_ok("");
        let typed = type_check(&resolved).expect("type_check should succeed");
        // Should have at least the built-in types in the environment.
        assert!(!typed.type_env.is_empty());
    }

    #[test]
    fn builtin_types_in_env() {
        let resolved = resolve_ok("");
        let typed = type_check(&resolved).expect("type_check should succeed");
        let env = &typed.type_env;

        assert_eq!(env.lookup("Int"), Some(&Type::Int));
        assert_eq!(env.lookup("Nat"), Some(&Type::Nat));
        assert_eq!(env.lookup("Float"), Some(&Type::Float));
        assert_eq!(env.lookup("Bool"), Some(&Type::Bool));
        assert_eq!(env.lookup("String"), Some(&Type::String));
        assert_eq!(env.lookup("Bytes"), Some(&Type::Bytes));
        assert_eq!(env.lookup("Unit"), Some(&Type::Unit));
        assert_eq!(env.lookup("Never"), Some(&Type::Never));
        assert_eq!(env.lookup("U8"), Some(&Type::U8));
        assert_eq!(env.lookup("U16"), Some(&Type::U16));
        assert_eq!(env.lookup("U32"), Some(&Type::U32));
        assert_eq!(env.lookup("U64"), Some(&Type::U64));
        assert_eq!(env.lookup("I8"), Some(&Type::I8));
        assert_eq!(env.lookup("I16"), Some(&Type::I16));
        assert_eq!(env.lookup("I32"), Some(&Type::I32));
        assert_eq!(env.lookup("I64"), Some(&Type::I64));
        assert_eq!(env.lookup("F32"), Some(&Type::F32));
        assert_eq!(env.lookup("F64"), Some(&Type::F64));
    }

    #[test]
    fn user_defined_types_in_env() {
        let src = r#"
type Foo {
  x: Int
  y: Bool
}

enum Color {
  Red
  Green
  Blue
}
"#;
        let resolved = resolve_ok(src);
        let typed = type_check(&resolved).expect("type_check should succeed");
        let env = &typed.type_env;

        assert_eq!(env.lookup("Foo"), Some(&Type::Named("Foo".into())));
        assert_eq!(env.lookup("Color"), Some(&Type::Named("Color".into())));
        // Enum variants are Named
        assert_eq!(env.lookup("Red"), Some(&Type::Named("Red".into())));
    }

    #[test]
    fn contract_in_env() {
        let src = r#"
contract SafeBuffer {
  requires { true }
}
"#;
        let resolved = resolve_ok(src);
        let typed = type_check(&resolved).expect("type_check should succeed");
        assert_eq!(
            typed.type_env.lookup("SafeBuffer"),
            Some(&Type::Named("SafeBuffer".into()))
        );
    }

    #[test]
    fn fn_def_in_env() {
        let src = r#"
fn helper(n: Int) -> Int {
  ensures { result >= 0 }
}
"#;
        let resolved = resolve_ok(src);
        let typed = type_check(&resolved).expect("type_check should succeed");
        assert_eq!(
            typed.type_env.lookup("helper"),
            Some(&Type::Fn {
                params: Vec::new(),
                ret: Box::new(Type::Unknown),
            })
        );
        // Parameter gets Unknown for now
        assert_eq!(typed.type_env.lookup("n"), Some(&Type::Unknown));
    }

    #[test]
    fn type_param_in_env() {
        let src = r#"
contract Container<T> {
  requires { true }
}
"#;
        let resolved = resolve_ok(src);
        let typed = type_check(&resolved).expect("type_check should succeed");
        assert_eq!(
            typed.type_env.lookup("T"),
            Some(&Type::TypeParam("T".into()))
        );
    }

    #[test]
    fn typed_file_preserves_resolved() {
        let src = r#"
type Point {
  x: Int
  y: Int
}
"#;
        let resolved = resolve_ok(src);
        let typed = type_check(&resolved).expect("type_check should succeed");
        // The resolved file should be preserved intact
        assert_eq!(typed.resolved.source.decls.len(), 1);
    }

    #[test]
    fn type_env_len() {
        let resolved = resolve_ok("");
        let typed = type_check(&resolved).expect("type_check should succeed");
        // At minimum, all 22 built-in types should be in the env
        assert!(typed.type_env.len() >= 22);
    }
}
