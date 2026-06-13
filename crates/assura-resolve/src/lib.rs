//! Name resolution and symbol table for the Assura contract language.
//!
//! The resolver walks the parsed AST, collects all declarations into a
//! `SymbolTable`, detects duplicate names (A02003), registers built-in
//! types, and resolves import declarations. Full name resolution
//! (undefined A02001, ambiguous A02002) is deferred to later tasks.

use std::collections::{HashMap, HashSet};

use assura_parser::ast::{Decl, ImportDecl, ServiceItem, SourceFile, Span, TypeBody};

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
    Parameter,
    TypeParam,
    Field,
    EnumVariant,
}

// ---------------------------------------------------------------------------
// Import resolution
// ---------------------------------------------------------------------------

/// The status of a resolved import declaration.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportStatus {
    /// The import was resolved to a known module in the module map.
    Resolved,
    /// The module was not found in the module map (external/unknown module).
    /// This is not a hard error; the module may be a standard library or
    /// external dependency that is not yet available.
    Unresolved,
    /// A circular import was detected (A02005).
    Circular,
}

/// A single resolved import, recording the original declaration and its
/// resolution status.
#[derive(Debug, Clone)]
pub struct ResolvedImport {
    /// The dotted module path, e.g. `["std", "math"]`.
    pub path: Vec<String>,
    /// If the import used `as alias`, this is the alias name.
    pub alias: Option<String>,
    /// Selectively imported items, e.g. `{ List, Map }`.
    pub items: Vec<String>,
    /// How this import was resolved.
    pub status: ImportStatus,
}

/// An in-memory map of known modules, keyed by their dotted path.
///
/// For now this is a simple `HashMap`; actual filesystem resolution is
/// deferred to a future task. Callers can pre-populate the map with
/// parsed `SourceFile`s to enable multi-file resolution.
pub type ModuleMap = HashMap<String, SourceFile>;

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
/// symbol table and resolved imports.
#[derive(Debug, Clone)]
pub struct ResolvedFile {
    pub source: SourceFile,
    pub symbols: SymbolTable,
    /// All import declarations with their resolution status.
    pub imports: Vec<ResolvedImport>,
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
/// Walks the AST, collects declarations into a `SymbolTable`, resolves
/// imports, and detects duplicate definitions (A02003). Returns a
/// `ResolvedFile` on success or a list of `ResolutionError`s.
///
/// Imports to unknown modules are recorded as `Unresolved` without
/// producing hard errors (the modules may be external dependencies).
pub fn resolve(source: &SourceFile) -> Result<ResolvedFile, Vec<ResolutionError>> {
    resolve_with_modules(source, &ModuleMap::new(), &mut HashSet::new())
}

/// Resolve names with an explicit module map and visited-module set.
///
/// The `module_map` provides known modules for import resolution.
/// The `visited` set tracks module paths currently being resolved,
/// enabling detection of circular imports (A02005).
pub fn resolve_with_modules(
    source: &SourceFile,
    module_map: &ModuleMap,
    visited: &mut HashSet<String>,
) -> Result<ResolvedFile, Vec<ResolutionError>> {
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

    // Mark this module as being resolved (for circular import detection).
    visited.insert(module_name.clone());

    // --- Resolve imports ---
    let resolved_imports = resolve_imports(&source.imports, module_map, visited, &mut errors);

    // --- Walk top-level declarations ---
    for decl in &source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                let inserted = try_insert(
                    &mut table,
                    &mut errors,
                    module,
                    &c.name,
                    SymbolKind::ContractDef,
                    decl.span.clone(),
                );
                if inserted {
                    let contract_scope = table.push_scope(&c.name, Some(module));
                    for tp in &c.type_params {
                        try_insert(
                            &mut table,
                            &mut errors,
                            contract_scope,
                            tp,
                            SymbolKind::TypeParam,
                            decl.span.clone(),
                        );
                    }
                }
            }
            Decl::TypeDef(t) => {
                let inserted = try_insert(
                    &mut table,
                    &mut errors,
                    module,
                    &t.name,
                    SymbolKind::TypeDef,
                    decl.span.clone(),
                );
                if inserted {
                    let type_scope = table.push_scope(&t.name, Some(module));
                    for tp in &t.type_params {
                        try_insert(
                            &mut table,
                            &mut errors,
                            type_scope,
                            tp,
                            SymbolKind::TypeParam,
                            decl.span.clone(),
                        );
                    }
                    if let TypeBody::Struct(fields) = &t.body {
                        for f in fields {
                            try_insert(
                                &mut table,
                                &mut errors,
                                type_scope,
                                &f.name,
                                SymbolKind::Field,
                                decl.span.clone(),
                            );
                        }
                    }
                }
            }
            Decl::EnumDef(e) => {
                let inserted = try_insert(
                    &mut table,
                    &mut errors,
                    module,
                    &e.name,
                    SymbolKind::EnumDef,
                    decl.span.clone(),
                );
                if inserted {
                    let enum_scope = table.push_scope(&e.name, Some(module));
                    for tp in &e.type_params {
                        try_insert(
                            &mut table,
                            &mut errors,
                            enum_scope,
                            tp,
                            SymbolKind::TypeParam,
                            decl.span.clone(),
                        );
                    }
                    for v in &e.variants {
                        try_insert(
                            &mut table,
                            &mut errors,
                            enum_scope,
                            &v.name,
                            SymbolKind::EnumVariant,
                            decl.span.clone(),
                        );
                    }
                }
            }
            Decl::Extern(ex) => {
                let inserted = try_insert(
                    &mut table,
                    &mut errors,
                    module,
                    &ex.name,
                    SymbolKind::ExternFn,
                    decl.span.clone(),
                );
                if inserted {
                    let fn_scope = table.push_scope(&ex.name, Some(module));
                    for p in &ex.params {
                        try_insert(
                            &mut table,
                            &mut errors,
                            fn_scope,
                            &p.name,
                            SymbolKind::Parameter,
                            decl.span.clone(),
                        );
                    }
                }
            }
            Decl::FnDef(f) => {
                let inserted = try_insert(
                    &mut table,
                    &mut errors,
                    module,
                    &f.name,
                    SymbolKind::FnDef,
                    decl.span.clone(),
                );
                if inserted {
                    let fn_scope = table.push_scope(&f.name, Some(module));
                    for p in &f.params {
                        try_insert(
                            &mut table,
                            &mut errors,
                            fn_scope,
                            &p.name,
                            SymbolKind::Parameter,
                            decl.span.clone(),
                        );
                    }
                }
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
                                let ins = try_insert(
                                    &mut table,
                                    &mut errors,
                                    svc_scope,
                                    &t.name,
                                    SymbolKind::TypeDef,
                                    decl.span.clone(),
                                );
                                if ins {
                                    let td_scope = table.push_scope(&t.name, Some(svc_scope));
                                    for tp in &t.type_params {
                                        try_insert(
                                            &mut table,
                                            &mut errors,
                                            td_scope,
                                            tp,
                                            SymbolKind::TypeParam,
                                            decl.span.clone(),
                                        );
                                    }
                                    if let TypeBody::Struct(fields) = &t.body {
                                        for f in fields {
                                            try_insert(
                                                &mut table,
                                                &mut errors,
                                                td_scope,
                                                &f.name,
                                                SymbolKind::Field,
                                                decl.span.clone(),
                                            );
                                        }
                                    }
                                }
                            }
                            ServiceItem::EnumDef(e) => {
                                let ins = try_insert(
                                    &mut table,
                                    &mut errors,
                                    svc_scope,
                                    &e.name,
                                    SymbolKind::EnumDef,
                                    decl.span.clone(),
                                );
                                if ins {
                                    let ed_scope = table.push_scope(&e.name, Some(svc_scope));
                                    for tp in &e.type_params {
                                        try_insert(
                                            &mut table,
                                            &mut errors,
                                            ed_scope,
                                            tp,
                                            SymbolKind::TypeParam,
                                            decl.span.clone(),
                                        );
                                    }
                                    for v in &e.variants {
                                        try_insert(
                                            &mut table,
                                            &mut errors,
                                            ed_scope,
                                            &v.name,
                                            SymbolKind::EnumVariant,
                                            decl.span.clone(),
                                        );
                                    }
                                }
                            }
                            ServiceItem::Operation { name, .. } => {
                                let ins = try_insert(
                                    &mut table,
                                    &mut errors,
                                    svc_scope,
                                    name,
                                    SymbolKind::Operation,
                                    decl.span.clone(),
                                );
                                if ins {
                                    // Scope for future clause-level resolution.
                                    table.push_scope(name, Some(svc_scope));
                                }
                            }
                            ServiceItem::Query { name, .. } => {
                                let ins = try_insert(
                                    &mut table,
                                    &mut errors,
                                    svc_scope,
                                    name,
                                    SymbolKind::Query,
                                    decl.span.clone(),
                                );
                                if ins {
                                    // Scope for future clause-level resolution.
                                    table.push_scope(name, Some(svc_scope));
                                }
                            }
                            // States / Invariant / Other don't introduce named symbols.
                            _ => {}
                        }
                    }
                }
            }
            Decl::Block { name, .. } => {
                // Generic blocks (feature, incremental, liveness, etc.)
                // register their name if non-empty and create a child scope.
                if !name.is_empty() {
                    let inserted = try_insert(
                        &mut table,
                        &mut errors,
                        module,
                        name,
                        SymbolKind::ContractDef,
                        decl.span.clone(),
                    );
                    if inserted {
                        table.push_scope(name, Some(module));
                    }
                }
            }
        }
    }

    // Remove this module from the visited set now that resolution is done.
    visited.remove(&module_name);

    if errors.is_empty() {
        Ok(ResolvedFile {
            source: source.clone(),
            symbols: table,
            imports: resolved_imports,
        })
    } else {
        Err(errors)
    }
}

/// Resolve all import declarations from a source file.
///
/// For each `ImportDecl`, checks whether the target module exists in the
/// `module_map`. If it does, the import is marked `Resolved`. If the
/// target module is currently being resolved (present in `visited`), the
/// import is marked `Circular` and an A02005 error is emitted. Otherwise
/// the import is marked `Unresolved` (external/unknown module, not an error).
fn resolve_imports(
    imports: &[ImportDecl],
    module_map: &ModuleMap,
    visited: &HashSet<String>,
    errors: &mut Vec<ResolutionError>,
) -> Vec<ResolvedImport> {
    imports
        .iter()
        .map(|imp| {
            let path_str = imp.path.join(".");

            let status = if visited.contains(&path_str) {
                // Circular import detected: module A imports B which
                // imports A (or transitively).
                errors.push(ResolutionError {
                    code: "A02005",
                    message: format!("circular import of module `{path_str}`"),
                    // Imports don't carry spans in the current AST, so
                    // use a sentinel span.
                    span: 0..0,
                    secondary: None,
                });
                ImportStatus::Circular
            } else if module_map.contains_key(&path_str) {
                ImportStatus::Resolved
            } else {
                // Unknown module. Not an error: could be a standard
                // library module or external dependency not yet loaded.
                ImportStatus::Unresolved
            };

            ResolvedImport {
                path: imp.path.clone(),
                alias: imp.alias.clone(),
                items: imp.items.clone(),
                status,
            }
        })
        .collect()
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

    #[test]
    fn contract_scope_with_type_params() {
        let src = r#"
contract SafeBuffer<T> {
  requires { true }
}
"#;
        let file = parse_ok(src);
        let resolved = resolve(&file).expect("resolve should succeed");
        // Contract scope is a child of module scope
        let contract_scope = resolved
            .symbols
            .scopes
            .iter()
            .find(|s| s.name == "SafeBuffer");
        assert!(contract_scope.is_some(), "SafeBuffer scope not found");
        // Type param T should be a symbol
        let tp = resolved
            .symbols
            .symbols
            .iter()
            .find(|s| s.name == "T" && s.kind == SymbolKind::TypeParam);
        assert!(tp.is_some(), "type param T not found");
    }

    #[test]
    fn fn_scope_with_params() {
        let src = r#"
fn helper(n: Int, m: Int) -> Int {
  ensures { result >= 0 }
}
"#;
        let file = parse_ok(src);
        let resolved = resolve(&file).expect("resolve should succeed");
        // Function scope exists
        let fn_scope = resolved.symbols.scopes.iter().find(|s| s.name == "helper");
        assert!(fn_scope.is_some(), "helper scope not found");
        // Parameters are symbols
        let params: Vec<&str> = resolved
            .symbols
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Parameter)
            .map(|s| s.name.as_str())
            .collect();
        assert!(params.contains(&"n"), "param n not found");
        assert!(params.contains(&"m"), "param m not found");
    }

    #[test]
    fn extern_scope_with_params() {
        let src = r#"
extern fn malloc(size: Nat) -> Bytes
  requires { size > 0 }
"#;
        let file = parse_ok(src);
        let resolved = resolve(&file).expect("resolve should succeed");
        let p = resolved
            .symbols
            .symbols
            .iter()
            .find(|s| s.name == "size" && s.kind == SymbolKind::Parameter);
        assert!(p.is_some(), "extern param size not found");
    }

    #[test]
    fn duplicate_fn_params() {
        let src = r#"
fn bad(x: Int, x: Int) -> Int {
  ensures { result >= 0 }
}
"#;
        let file = parse_ok(src);
        let result = resolve(&file);
        assert!(result.is_err(), "should detect duplicate param");
        let errs = result.unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A02003");
        assert!(errs[0].message.contains("x"));
    }

    #[test]
    fn type_scope_with_fields() {
        let src = r#"
type Point {
  x: Int;
  y: Int;
}
"#;
        let file = parse_ok(src);
        let resolved = resolve(&file).expect("resolve should succeed");
        let fields: Vec<&str> = resolved
            .symbols
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Field)
            .map(|s| s.name.as_str())
            .collect();
        assert!(fields.contains(&"x"), "field x not found");
        assert!(fields.contains(&"y"), "field y not found");
    }

    #[test]
    fn duplicate_struct_fields() {
        let src = r#"
type BadStruct {
  x: Int;
  x: Float;
}
"#;
        let file = parse_ok(src);
        let result = resolve(&file);
        assert!(result.is_err(), "should detect duplicate field");
        let errs = result.unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A02003");
        assert!(errs[0].message.contains("x"));
    }

    #[test]
    fn enum_scope_with_variants() {
        let src = r#"
enum Color {
  Red
  Green
  Blue
}
"#;
        let file = parse_ok(src);
        let resolved = resolve(&file).expect("resolve should succeed");
        let variants: Vec<&str> = resolved
            .symbols
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::EnumVariant)
            .map(|s| s.name.as_str())
            .collect();
        assert!(variants.contains(&"Red"), "variant Red not found");
        assert!(variants.contains(&"Green"), "variant Green not found");
        assert!(variants.contains(&"Blue"), "variant Blue not found");
    }

    #[test]
    fn duplicate_enum_variants() {
        let src = r#"
enum Bad {
  A
  A
}
"#;
        let file = parse_ok(src);
        let result = resolve(&file);
        assert!(result.is_err(), "should detect duplicate variant");
        let errs = result.unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A02003");
        assert!(errs[0].message.contains("A"));
    }

    #[test]
    fn service_nested_type_fields() {
        let src = r#"
service Svc {
  type Config {
    max_size: Nat;
    retries: Nat;
  }

  operation start {
    input { data: Bytes }
  }

  query health {
    output { state: String }
  }
}
"#;
        let file = parse_ok(src);
        let resolved = resolve(&file).expect("resolve should succeed");
        // Config fields are symbols
        let fields: Vec<&str> = resolved
            .symbols
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Field)
            .map(|s| s.name.as_str())
            .collect();
        assert!(fields.contains(&"max_size"), "field max_size not found");
        assert!(fields.contains(&"retries"), "field retries not found");
        // Operation and query have scopes
        let op_scope = resolved.symbols.scopes.iter().find(|s| s.name == "start");
        assert!(op_scope.is_some(), "start operation scope not found");
        let q_scope = resolved.symbols.scopes.iter().find(|s| s.name == "health");
        assert!(q_scope.is_some(), "health query scope not found");
    }

    #[test]
    fn duplicate_service_operations() {
        let src = r#"
service BadSvc {
  operation go {
    input { data: Bytes }
  }

  operation go {
    input { other: Bytes }
  }
}
"#;
        let file = parse_ok(src);
        let result = resolve(&file);
        assert!(result.is_err(), "should detect duplicate operation");
        let errs = result.unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A02003");
        assert!(errs[0].message.contains("go"));
    }

    #[test]
    fn scope_hierarchy_depth() {
        // Verify that a service with a type def creates
        // root > module > service > type scopes (4 levels).
        let src = r#"
service Deep {
  type Inner {
    field: Int
  }
}
"#;
        let file = parse_ok(src);
        let resolved = resolve(&file).expect("resolve should succeed");
        // Walk from Inner scope up to root
        let inner_scope = resolved
            .symbols
            .scopes
            .iter()
            .position(|s| s.name == "Inner")
            .expect("Inner scope not found");
        let inner = &resolved.symbols.scopes[inner_scope];
        let svc_id = inner.parent.expect("Inner should have parent");
        let svc = &resolved.symbols.scopes[svc_id];
        assert_eq!(svc.name, "Deep");
        let mod_id = svc.parent.expect("Deep should have parent");
        let module = &resolved.symbols.scopes[mod_id];
        let root_id = module.parent.expect("module should have parent");
        let root = &resolved.symbols.scopes[root_id];
        assert_eq!(root.name, "<root>");
        assert!(root.parent.is_none(), "root should have no parent");
    }

    #[test]
    fn name_shadowing_allowed_across_scopes() {
        // A parameter named the same as a top-level type is OK —
        // shadowing across scope levels is not a duplicate error.
        let src = r#"
type Foo {
  x: Int
}

fn helper(Foo: Int) -> Int {
  ensures { result >= 0 }
}
"#;
        let file = parse_ok(src);
        let resolved = resolve(&file).expect("shadowing should be allowed");
        // Both exist: one as TypeDef, one as Parameter
        let type_sym = resolved
            .symbols
            .symbols
            .iter()
            .find(|s| s.name == "Foo" && s.kind == SymbolKind::TypeDef);
        let param_sym = resolved
            .symbols
            .symbols
            .iter()
            .find(|s| s.name == "Foo" && s.kind == SymbolKind::Parameter);
        assert!(type_sym.is_some(), "type Foo not found");
        assert!(param_sym.is_some(), "param Foo not found");
    }

    // -----------------------------------------------------------------------
    // Import resolution tests
    // -----------------------------------------------------------------------

    #[test]
    fn import_basic_recorded() {
        let src = r#"
import std.math;
"#;
        let file = parse_ok(src);
        let resolved = resolve(&file).expect("resolve should succeed");
        assert_eq!(resolved.imports.len(), 1);
        assert_eq!(resolved.imports[0].path, vec!["std", "math"]);
        assert!(resolved.imports[0].alias.is_none());
        assert!(resolved.imports[0].items.is_empty());
        // Without a module map entry, status is Unresolved.
        assert_eq!(resolved.imports[0].status, ImportStatus::Unresolved);
    }

    #[test]
    fn import_aliased_recorded() {
        let src = r#"
import crypto.hash as hash;
"#;
        let file = parse_ok(src);
        let resolved = resolve(&file).expect("resolve should succeed");
        assert_eq!(resolved.imports.len(), 1);
        assert_eq!(resolved.imports[0].path, vec!["crypto", "hash"]);
        assert_eq!(resolved.imports[0].alias.as_deref(), Some("hash"));
        assert!(resolved.imports[0].items.is_empty());
        assert_eq!(resolved.imports[0].status, ImportStatus::Unresolved);
    }

    #[test]
    fn import_selective_recorded() {
        let src = r#"
import std.collections { List, Map };
"#;
        let file = parse_ok(src);
        let resolved = resolve(&file).expect("resolve should succeed");
        assert_eq!(resolved.imports.len(), 1);
        assert_eq!(resolved.imports[0].path, vec!["std", "collections"]);
        assert!(resolved.imports[0].alias.is_none());
        assert_eq!(resolved.imports[0].items, vec!["List", "Map"]);
        assert_eq!(resolved.imports[0].status, ImportStatus::Unresolved);
    }

    #[test]
    fn import_multiple_recorded() {
        let src = r#"
import std.math;
import std.collections { List, Map };
import crypto.hash as hash;
"#;
        let file = parse_ok(src);
        let resolved = resolve(&file).expect("resolve should succeed");
        assert_eq!(resolved.imports.len(), 3);
        assert_eq!(resolved.imports[0].path, vec!["std", "math"]);
        assert_eq!(resolved.imports[1].path, vec!["std", "collections"]);
        assert_eq!(resolved.imports[2].path, vec!["crypto", "hash"]);
    }

    #[test]
    fn import_unresolved_no_hard_error() {
        // External/unknown modules should NOT cause resolution failure.
        let src = r#"
import assura.mem;
import assura.sec;

contract Foo {
  requires { true }
}
"#;
        let file = parse_ok(src);
        let resolved = resolve(&file).expect("unresolved imports should not fail");
        assert_eq!(resolved.imports.len(), 2);
        assert_eq!(resolved.imports[0].status, ImportStatus::Unresolved);
        assert_eq!(resolved.imports[1].status, ImportStatus::Unresolved);
        // Declarations are still resolved normally.
        let foo = resolved.symbols.symbols.iter().find(|s| s.name == "Foo");
        assert!(foo.is_some(), "Foo should still be resolved");
    }

    #[test]
    fn import_resolved_with_module_map() {
        // Pre-populate the module map so the import resolves.
        let target_src = r#"
module std.math;

fn abs(x: Int) -> Int {
  ensures { result >= 0 }
}
"#;
        let target_file = parse_ok(target_src);
        let mut module_map = ModuleMap::new();
        module_map.insert("std.math".to_string(), target_file);

        let src = r#"
import std.math;
"#;
        let file = parse_ok(src);
        let mut visited = HashSet::new();
        let resolved =
            resolve_with_modules(&file, &module_map, &mut visited).expect("should succeed");
        assert_eq!(resolved.imports.len(), 1);
        assert_eq!(resolved.imports[0].status, ImportStatus::Resolved);
    }

    #[test]
    fn import_circular_detected() {
        // Simulate circular import: module A is being resolved and it
        // imports module A (itself).
        let src = r#"
module mymod;

import mymod;
"#;
        let file = parse_ok(src);
        let mut visited = HashSet::new();
        // Pre-seed visited with "mymod" to simulate a cycle.
        visited.insert("mymod".to_string());
        let result = resolve_with_modules(&file, &ModuleMap::new(), &mut visited);
        assert!(result.is_err(), "circular import should produce an error");
        let errs = result.unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A02005");
        assert!(errs[0].message.contains("mymod"));
    }

    #[test]
    fn import_circular_indirect() {
        // Simulate indirect circular import: module A imports B, and B
        // is already being resolved (present in visited).
        let src = r#"
module a;

import b;
"#;
        let file = parse_ok(src);
        let mut visited = HashSet::new();
        // "b" is already being resolved somewhere up the call chain.
        visited.insert("b".to_string());
        let result = resolve_with_modules(&file, &ModuleMap::new(), &mut visited);
        assert!(result.is_err(), "circular import should produce an error");
        let errs = result.unwrap_err();
        assert_eq!(errs[0].code, "A02005");
        assert!(errs[0].message.contains("b"));
    }

    #[test]
    fn import_mixed_resolved_and_unresolved() {
        // One import resolves, another does not.
        let target_src = r#"
module known.mod;

type Foo { x: Int }
"#;
        let target_file = parse_ok(target_src);
        let mut module_map = ModuleMap::new();
        module_map.insert("known.mod".to_string(), target_file);

        let src = r#"
import known.mod { Foo };
import unknown.mod;
"#;
        let file = parse_ok(src);
        let mut visited = HashSet::new();
        let resolved =
            resolve_with_modules(&file, &module_map, &mut visited).expect("should succeed");
        assert_eq!(resolved.imports.len(), 2);
        assert_eq!(resolved.imports[0].status, ImportStatus::Resolved);
        assert_eq!(resolved.imports[1].status, ImportStatus::Unresolved);
    }

    #[test]
    fn no_imports_empty_list() {
        let src = r#"
contract Foo {
  requires { true }
}
"#;
        let file = parse_ok(src);
        let resolved = resolve(&file).expect("resolve should succeed");
        assert!(resolved.imports.is_empty());
    }

    #[test]
    fn visited_set_cleaned_up_after_resolve() {
        // After resolve_with_modules returns, the current module should
        // be removed from the visited set so sibling modules are not
        // falsely flagged as circular.
        let src = r#"
module a;
"#;
        let file = parse_ok(src);
        let mut visited = HashSet::new();
        resolve_with_modules(&file, &ModuleMap::new(), &mut visited).expect("should succeed");
        assert!(
            !visited.contains("a"),
            "module 'a' should be removed from visited after resolution"
        );
    }
}
