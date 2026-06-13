//! Name resolution and symbol table for the Assura contract language.
//!
//! The resolver walks the parsed AST, collects all declarations into a
//! `SymbolTable`, detects duplicate names (A02003), and registers built-in
//! types. Full name resolution (undefined A02001, ambiguous A02002) is
//! deferred to later tasks.

use std::collections::HashMap;

use assura_parser::ast::{Decl, ServiceItem, SourceFile, Span};

// ---------------------------------------------------------------------------
// Symbol kinds
// ---------------------------------------------------------------------------

/// A resolved symbol in the symbol table.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub name: String,
    pub span: Span,
    pub scope_id: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SymbolKind {
    TypeDef,
    ContractDef,
    ServiceDef,
    FnDef,
    EnumDef,
    ExternFn,
    BuiltinType,
    Operation,
    Query,
}

// ---------------------------------------------------------------------------
// Scopes
// ---------------------------------------------------------------------------

/// A lexical scope that maps names to symbol indices.
#[derive(Debug, Clone)]
pub struct Scope {
    pub name: String,
    pub parent: Option<usize>,
    /// Maps symbol name -> index in `SymbolTable::symbols`.
    pub symbols: HashMap<String, usize>,
}

// ---------------------------------------------------------------------------
// Symbol table
// ---------------------------------------------------------------------------

/// The central symbol table built by the resolver.
#[derive(Debug, Clone)]
pub struct SymbolTable {
    pub symbols: Vec<Symbol>,
    pub scopes: Vec<Scope>,
}

impl SymbolTable {
    fn new() -> Self {
        Self {
            symbols: Vec::new(),
            scopes: Vec::new(),
        }
    }

    /// Create a new scope, returning its index.
    fn push_scope(&mut self, name: &str, parent: Option<usize>) -> usize {
        let id = self.scopes.len();
        self.scopes.push(Scope {
            name: name.to_string(),
            parent,
            symbols: HashMap::new(),
        });
        id
    }

    /// Insert a symbol into a scope. Returns `Err` with the existing
    /// symbol's span if a duplicate is detected.
    fn insert(
        &mut self,
        scope_id: usize,
        name: &str,
        kind: SymbolKind,
        span: Span,
    ) -> Result<usize, Span> {
        if let Some(&existing_idx) = self.scopes[scope_id].symbols.get(name) {
            return Err(self.symbols[existing_idx].span.clone());
        }
        let idx = self.symbols.len();
        self.symbols.push(Symbol {
            kind,
            name: name.to_string(),
            span,
            scope_id,
        });
        self.scopes[scope_id].symbols.insert(name.to_string(), idx);
        Ok(idx)
    }

    /// Total number of symbols (including built-ins).
    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    /// Returns `true` if the table contains no symbols.
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Resolution errors
// ---------------------------------------------------------------------------

/// An error produced during name resolution.
#[derive(Debug, Clone)]
pub struct ResolutionError {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
    /// Optional secondary span (e.g., previous definition site).
    pub secondary: Option<(Span, String)>,
}

// ---------------------------------------------------------------------------
// Resolved file
// ---------------------------------------------------------------------------

/// The result of successful name resolution: the original AST plus the
/// symbol table.
#[derive(Debug, Clone)]
pub struct ResolvedFile {
    pub source: SourceFile,
    pub symbols: SymbolTable,
}

// ---------------------------------------------------------------------------
// Built-in types
// ---------------------------------------------------------------------------

const BUILTIN_TYPES: &[&str] = &[
    "Int", "Nat", "Float", "Bool", "String", "Bytes", "Unit", "Never", "List", "Map", "Set",
    "Option", "Result", // Sized integer types used in demos
    "U8", "U16", "U32", "U64", "I8", "I16", "I32", "I64", "F32", "F64", "Sequence",
];

// ---------------------------------------------------------------------------
// Resolver
// ---------------------------------------------------------------------------

/// Resolve all names in a `SourceFile`.
///
/// Walks the AST, collects declarations into a `SymbolTable`, and detects
/// duplicate definitions (A02003). Returns a `ResolvedFile` on success or
/// a list of `ResolutionError`s.
pub fn resolve(source: &SourceFile) -> Result<ResolvedFile, Vec<ResolutionError>> {
    let mut table = SymbolTable::new();
    let mut errors: Vec<ResolutionError> = Vec::new();

    // --- Root scope with built-in types ---
    let root = table.push_scope("<root>", None);
    for &name in BUILTIN_TYPES {
        // Built-ins use a sentinel span (0..0).
        table
            .insert(root, name, SymbolKind::BuiltinType, 0..0)
            .expect("built-in types should not collide");
    }

    // --- Module scope (child of root) ---
    let module_name = source
        .module
        .as_ref()
        .map(|m| m.path.join("."))
        .unwrap_or_else(|| "<anonymous>".to_string());
    let module = table.push_scope(&module_name, Some(root));

    // --- Walk top-level declarations ---
    for decl in &source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                try_insert(
                    &mut table,
                    &mut errors,
                    module,
                    &c.name,
                    SymbolKind::ContractDef,
                    decl.span.clone(),
                );
            }
            Decl::TypeDef(t) => {
                try_insert(
                    &mut table,
                    &mut errors,
                    module,
                    &t.name,
                    SymbolKind::TypeDef,
                    decl.span.clone(),
                );
            }
            Decl::EnumDef(e) => {
                try_insert(
                    &mut table,
                    &mut errors,
                    module,
                    &e.name,
                    SymbolKind::EnumDef,
                    decl.span.clone(),
                );
            }
            Decl::Extern(ex) => {
                try_insert(
                    &mut table,
                    &mut errors,
                    module,
                    &ex.name,
                    SymbolKind::ExternFn,
                    decl.span.clone(),
                );
            }
            Decl::FnDef(f) => {
                try_insert(
                    &mut table,
                    &mut errors,
                    module,
                    &f.name,
                    SymbolKind::FnDef,
                    decl.span.clone(),
                );
            }
            Decl::Service(s) => {
                let svc_sym_span = decl.span.clone();
                let inserted = try_insert(
                    &mut table,
                    &mut errors,
                    module,
                    &s.name,
                    SymbolKind::ServiceDef,
                    svc_sym_span,
                );
                // Create a child scope for the service's items.
                if inserted {
                    let svc_scope = table.push_scope(&s.name, Some(module));
                    for item in &s.items {
                        match item {
                            ServiceItem::TypeDef(t) => {
                                try_insert(
                                    &mut table,
                                    &mut errors,
                                    svc_scope,
                                    &t.name,
                                    SymbolKind::TypeDef,
                                    decl.span.clone(),
                                );
                            }
                            ServiceItem::EnumDef(e) => {
                                try_insert(
                                    &mut table,
                                    &mut errors,
                                    svc_scope,
                                    &e.name,
                                    SymbolKind::EnumDef,
                                    decl.span.clone(),
                                );
                            }
                            ServiceItem::Operation { name, .. } => {
                                try_insert(
                                    &mut table,
                                    &mut errors,
                                    svc_scope,
                                    name,
                                    SymbolKind::Operation,
                                    decl.span.clone(),
                                );
                            }
                            ServiceItem::Query { name, .. } => {
                                try_insert(
                                    &mut table,
                                    &mut errors,
                                    svc_scope,
                                    name,
                                    SymbolKind::Query,
                                    decl.span.clone(),
                                );
                            }
                            // States / Invariant / Other don't introduce named symbols.
                            _ => {}
                        }
                    }
                }
            }
            Decl::Block { name, .. } => {
                // Generic blocks (feature, incremental, liveness, etc.)
                // register their name if non-empty.
                if !name.is_empty() {
                    try_insert(
                        &mut table,
                        &mut errors,
                        module,
                        name,
                        SymbolKind::ContractDef,
                        decl.span.clone(),
                    );
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(ResolvedFile {
            source: source.clone(),
            symbols: table,
        })
    } else {
        Err(errors)
    }
}

/// Try to insert a symbol; on duplicate, push an A02003 error.
/// Returns `true` if the symbol was inserted successfully.
fn try_insert(
    table: &mut SymbolTable,
    errors: &mut Vec<ResolutionError>,
    scope_id: usize,
    name: &str,
    kind: SymbolKind,
    span: Span,
) -> bool {
    match table.insert(scope_id, name, kind, span.clone()) {
        Ok(_) => true,
        Err(prev_span) => {
            errors.push(ResolutionError {
                code: "A02003",
                message: format!("duplicate definition of `{name}`"),
                span,
                secondary: Some((prev_span, format!("`{name}` previously defined here"))),
            });
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse source text into a `SourceFile` (panics on error).
    fn parse_ok(source: &str) -> SourceFile {
        let (file, errs) = assura_parser::parse(source);
        assert!(errs.is_empty(), "unexpected parse errors: {errs:?}");
        file.expect("parse returned None")
    }

    #[test]
    fn builtins_registered() {
        let file = parse_ok("");
        let resolved = resolve(&file).expect("resolve should succeed on empty file");
        // All built-in types should be in the table.
        assert!(resolved.symbols.len() >= BUILTIN_TYPES.len());
        for &name in BUILTIN_TYPES {
            let found = resolved
                .symbols
                .symbols
                .iter()
                .any(|s| s.name == name && s.kind == SymbolKind::BuiltinType);
            assert!(found, "built-in type `{name}` not found");
        }
    }

    #[test]
    fn collects_top_level_decls() {
        let src = r#"
contract Foo {
  requires { true }
}

type Bar {
  x: Int
}

enum Baz {
  A
  B
}

fn helper(n: Int) -> Int {
  ensures { result >= 0 }
}
"#;
        let file = parse_ok(src);
        let resolved = resolve(&file).expect("resolve should succeed");
        let names: Vec<&str> = resolved
            .symbols
            .symbols
            .iter()
            .filter(|s| s.kind != SymbolKind::BuiltinType)
            .map(|s| s.name.as_str())
            .collect();
        assert!(names.contains(&"Foo"), "missing Foo");
        assert!(names.contains(&"Bar"), "missing Bar");
        assert!(names.contains(&"Baz"), "missing Baz");
        assert!(names.contains(&"helper"), "missing helper");
    }

    #[test]
    fn duplicate_detection() {
        let src = r#"
contract Foo {
  requires { true }
}

contract Foo {
  ensures { true }
}
"#;
        let file = parse_ok(src);
        let result = resolve(&file);
        assert!(result.is_err(), "should detect duplicate");
        let errs = result.unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A02003");
        assert!(errs[0].message.contains("Foo"));
    }

    #[test]
    fn service_creates_child_scope() {
        let src = r#"
service ImageDecoder {
  type Config {
    max_size: Nat
  }

  operation decode {
    input { data: Bytes }
    output { image: Bytes }
  }

  query status {
    output { state: String }
  }
}
"#;
        let file = parse_ok(src);
        let resolved = resolve(&file).expect("resolve should succeed");
        // Should have: root scope, module scope, ImageDecoder scope
        assert!(resolved.symbols.scopes.len() >= 3);
        // Service itself is a symbol
        let svc = resolved
            .symbols
            .symbols
            .iter()
            .find(|s| s.name == "ImageDecoder");
        assert!(svc.is_some(), "ImageDecoder not found");
        // Items inside the service are also symbols
        let config = resolved.symbols.symbols.iter().find(|s| s.name == "Config");
        assert!(config.is_some(), "Config not found in service scope");
        let decode = resolved.symbols.symbols.iter().find(|s| s.name == "decode");
        assert!(decode.is_some(), "decode not found in service scope");
        let status = resolved.symbols.symbols.iter().find(|s| s.name == "status");
        assert!(status.is_some(), "status not found in service scope");
    }

    #[test]
    fn empty_file_ok() {
        let file = parse_ok("");
        let resolved = resolve(&file).expect("empty file should resolve");
        // Only builtins
        assert_eq!(resolved.symbols.symbols.len(), BUILTIN_TYPES.len());
    }
}
