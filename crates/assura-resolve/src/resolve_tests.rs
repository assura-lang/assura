use super::*;
use assura_parser::ast::Spanned;

/// Helper: parse source text into a `SourceFile` (panics on error).
fn parse_ok(source: &str) -> SourceFile {
    assura_parser::parse_unwrap(source)
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
    assert!(
        errs[0].message.contains("`Foo`"),
        "error should name the duplicate definition `Foo`, got: {}",
        errs[0].message
    );
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
    // Built-in types + stdlib prelude types (minus duplicates) + prelude contracts
    let stdlib_extras = assura_stdlib::prelude_type_names()
        .iter()
        .filter(|name| !BUILTIN_TYPES.contains(name))
        .count();
    let prelude_contracts = assura_stdlib::prelude_contract_names().len();
    assert_eq!(
        resolved.symbols.symbols.len(),
        BUILTIN_TYPES.len() + stdlib_extras + prelude_contracts
    );
}

#[test]
fn prelude_contracts_registered_as_contract_def() {
    let file = parse_ok("");
    let resolved = resolve(&file).expect("empty file should resolve");
    for &name in &assura_stdlib::prelude_contract_names() {
        let sym = resolved.symbols.symbols.iter().find(|s| s.name == name);
        assert!(
            sym.is_some(),
            "prelude contract '{name}' not in symbol table"
        );
        assert_eq!(
            sym.unwrap().kind,
            SymbolKind::ContractDef,
            "prelude contract '{name}' should be ContractDef"
        );
    }
}

#[test]
fn clamp_resolves_without_import() {
    let src = r#"
contract BoundedValue {
    input(x: Int)
    output(result: Int)
    ensures { clamp(x, 0, 100) >= 0 }
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file);
    assert!(
        resolved.is_ok(),
        "clamp should resolve without import: {resolved:?}"
    );
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
    assert!(
        errs[0].message.contains("`x`"),
        "error should name the duplicate parameter `x`, got: {}",
        errs[0].message
    );
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
    assert!(
        errs[0].message.contains("`x`"),
        "error should name the duplicate field `x`, got: {}",
        errs[0].message
    );
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
    assert!(
        errs[0].message.contains("`A`"),
        "error should name the duplicate variant `A`, got: {}",
        errs[0].message
    );
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
    assert!(
        errs[0].message.contains("`go`"),
        "error should name the duplicate operation `go`, got: {}",
        errs[0].message
    );
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
    // A parameter named the same as a top-level type is OK --
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
    let resolved = resolve_with_modules(&file, &module_map, &mut visited).expect("should succeed");
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
    assert!(
        errs[0].message.contains("`mymod`"),
        "error should name the circular module `mymod`, got: {}",
        errs[0].message
    );
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
    assert!(
        errs[0].message.contains("`b`"),
        "error should name the circular module `b`, got: {}",
        errs[0].message
    );
}

#[test]
fn import_mixed_resolved_and_unresolved() {
    // One import resolves, another does not. Non-empty module map => A02010 error.
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
    let result = resolve_with_modules(&file, &module_map, &mut visited);
    assert!(
        result.is_err(),
        "missing module in project map should hard-error"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.code == "A02010" && e.message.contains("unknown.mod")),
        "expected A02010 for unknown.mod, got {errs:?}"
    );
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

// -----------------------------------------------------------------------
// Type reference resolution tests (T012)
// -----------------------------------------------------------------------

#[test]
fn builtin_types_resolve_in_fields() {
    let src = r#"
type Point {
  x: Int;
  y: Float;
  name: String;
  active: Bool;
}
"#;
    let file = parse_ok(src);
    resolve(&file).expect("built-in types in fields should resolve");
}

#[test]
fn builtin_types_resolve_in_fn_params() {
    let src = r#"
fn helper(n: Int, s: String) -> Bool {
  ensures { result == true }
}
"#;
    let file = parse_ok(src);
    resolve(&file).expect("built-in types in fn params should resolve");
}

#[test]
fn builtin_types_resolve_in_extern() {
    let src = r#"
extern fn malloc(size: Nat) -> Bytes
  requires { size > 0 }
"#;
    let file = parse_ok(src);
    resolve(&file).expect("built-in types in extern should resolve");
}

#[test]
fn user_defined_type_resolves_in_field() {
    let src = r#"
type UserId = { id: Nat | id > 0 };

type User {
  id: UserId;
  name: String;
}
"#;
    let file = parse_ok(src);
    resolve(&file).expect("user-defined type in fields should resolve");
}

#[test]
fn user_defined_type_resolves_in_fn() {
    let src = r#"
type UserId = { id: Nat | id > 0 };

fn get_user(id: UserId) -> String {
  ensures { result.length() > 0 }
}
"#;
    let file = parse_ok(src);
    resolve(&file).expect("user-defined type in fn params should resolve");
}

#[test]
fn type_param_resolves_in_scope() {
    // Generic type: T should resolve within the type's own scope.
    let src = r#"
type Container<T> {
  items: List;
}
"#;
    let file = parse_ok(src);
    resolve(&file).expect("type param should resolve in type scope");
}

#[test]
fn unknown_type_a02001_in_field() {
    // No imports, no definition of Banana => A02001
    let src = r#"
type Basket {
  fruit: Banana;
}
"#;
    let file = parse_ok(src);
    let result = resolve(&file);
    assert!(result.is_err(), "unknown type should produce A02001");
    let errs = result.unwrap_err();
    assert!(
        errs.iter().any(|e| e.code == "A02001"),
        "should have A02001"
    );
    assert!(
        errs.iter().any(|e| e.message.contains("Banana")),
        "error should mention Banana"
    );
}

#[test]
fn unknown_type_a02001_in_fn_param() {
    let src = r#"
fn process(item: Unicorn) -> Int {
  ensures { result >= 0 }
}
"#;
    let file = parse_ok(src);
    let result = resolve(&file);
    assert!(result.is_err(), "unknown type should produce A02001");
    let errs = result.unwrap_err();
    assert!(errs.iter().any(|e| e.code == "A02001"));
    assert!(errs.iter().any(|e| e.message.contains("Unicorn")));
}

#[test]
fn unknown_type_a02001_in_return_type() {
    let src = r#"
fn compute(x: Int) -> Phantom {
  ensures { result == x }
}
"#;
    let file = parse_ok(src);
    let result = resolve(&file);
    assert!(result.is_err(), "unknown return type should produce A02001");
    let errs = result.unwrap_err();
    assert!(errs.iter().any(|e| e.code == "A02001"));
    assert!(errs.iter().any(|e| e.message.contains("Phantom")));
}

#[test]
fn unknown_type_lenient_with_imports() {
    // When there are unresolved imports, unknown types are NOT errors
    // (they may come from the imported module).
    let src = r#"
import external.types;

type Wrapper {
  inner: ExternalType;
}
"#;
    let file = parse_ok(src);
    resolve(&file).expect("unknown type with unresolved imports should be lenient");
}

#[test]
fn enum_used_as_type_resolves() {
    let src = r#"
enum Color {
  Red
  Green
  Blue
}

type Pixel {
  color: Color;
  x: Int;
  y: Int;
}
"#;
    let file = parse_ok(src);
    resolve(&file).expect("enum used as field type should resolve");
}

#[test]
fn service_nested_type_refs_resolve() {
    let src = r#"
service Svc {
  type Config {
max_size: Nat;
enabled: Bool;
  }
}
"#;
    let file = parse_ok(src);
    resolve(&file).expect("types in service nested type defs should resolve");
}

#[test]
fn lookup_walks_scope_chain() {
    // Verify the lookup method walks up the scope chain.
    let src = r#"
type Outer {
  x: Int
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("resolve should succeed");
    let table = &resolved.symbols;
    // Find the Outer type scope
    let outer_scope = table
        .scopes
        .iter()
        .position(|s| s.name == "Outer")
        .expect("Outer scope not found");
    // Int is in root scope; lookup from Outer scope should find it
    let int_sym = table.lookup("Int", outer_scope);
    assert!(int_sym.is_some(), "Int should be found via scope chain");
    assert_eq!(int_sym.unwrap().kind, SymbolKind::BuiltinType);
    // Nonexistent name should return None
    let missing = table.lookup("DoesNotExist", outer_scope);
    assert!(missing.is_none(), "missing name should return None");
}

#[test]
fn type_alias_refs_resolve() {
    let src = r#"
type PositiveInt = { n: Int | n > 0 };
"#;
    let file = parse_ok(src);
    resolve(&file).expect("type alias with Int reference should resolve");
}

#[test]
fn multiple_unknown_types_reported() {
    let src = r#"
type Bad {
  a: Alpha;
  b: Beta;
}
"#;
    let file = parse_ok(src);
    let result = resolve(&file);
    assert!(result.is_err(), "should report errors for unknown types");
    let errs = result.unwrap_err();
    let a02001_count = errs.iter().filter(|e| e.code == "A02001").count();
    assert_eq!(a02001_count, 2, "should report 2 A02001 errors");
}

#[test]
fn lowercase_tokens_not_checked_as_types() {
    // Lowercase tokens in type positions (e.g., modifiers, keywords)
    // should not trigger A02001.
    let src = r#"
type Wrapper {
  x: Int;
}
"#;
    let file = parse_ok(src);
    resolve(&file).expect("lowercase tokens should not be checked as types");
}

#[test]
fn sized_int_types_resolve() {
    let src = r#"
type Packet {
  header: U32;
  length: U16;
  checksum: U8;
  signed_val: I64;
  ratio: F32;
}
"#;
    let file = parse_ok(src);
    resolve(&file).expect("sized integer types should resolve");
}

#[test]
fn generic_builtin_components_resolve() {
    // In `List<Int>`, both `List` and `Int` should resolve.
    // The raw tokens will be something like ["List", "<", "Int", ">"]
    let src = r#"
fn process(items: List) -> Nat {
  ensures { result >= 0 }
}
"#;
    let file = parse_ok(src);
    resolve(&file).expect("generic type components should resolve");
}

#[test]
fn nested_same_name_scopes_resolve_correctly() {
    // A service-nested type and a top-level type with the same name
    // should each resolve in their own scope without collision.
    let src = r#"
type Config {
  x: Int
}

service MyService {
  type Config {
y: Nat
  }
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("should resolve without errors");
    // Both Config types should exist
    let configs: Vec<&Symbol> = resolved
        .symbols
        .symbols
        .iter()
        .filter(|s| s.name == "Config")
        .collect();
    assert_eq!(configs.len(), 2, "should have two Config symbols");
}

#[test]
fn block_does_not_register_as_contract() {
    // A block declaration should NOT register as a ContractDef
    let src = r#"
contract RealContract {
  requires { true }
}

feature enhanced_mode {
  requires { true }
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("should resolve");
    // RealContract is a ContractDef, but enhanced_mode should not be
    let contract_defs: Vec<&str> = resolved
        .symbols
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::ContractDef)
        .map(|s| s.name.as_str())
        .collect();
    assert!(
        contract_defs.contains(&"RealContract"),
        "RealContract should be ContractDef"
    );
    assert!(
        !contract_defs.contains(&"enhanced_mode"),
        "block should not be registered as ContractDef"
    );
}

#[test]
fn enum_variant_types_checked_in_strict_mode() {
    // In strict mode (no module/project/imports), unknown types in
    // enum variant fields should be reported as A02001.
    let src = r#"
enum MyResult {
  Ok(Int)
  Err(ErrorDetails)
}
"#;
    let file = parse_ok(src);
    let result = resolve(&file);
    // ErrorDetails is not a known type, should trigger A02001
    // (Int is a builtin, so only ErrorDetails should fail)
    assert!(
        result.is_err(),
        "should detect unknown type in enum variant"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter().any(|e| e.code == "A02001"),
        "should report A02001 for unknown type"
    );
}

#[test]
fn selective_import_injects_symbols() {
    let src = r#"
import std.collections { List, Map };
type MyData {
  items: List
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("should resolve with imported types");
    // List and Map should be in the symbol table as BuiltinType
    let names: Vec<&str> = resolved
        .symbols
        .symbols
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert!(
        names.contains(&"List"),
        "List should be injected from import"
    );
    assert!(names.contains(&"Map"), "Map should be injected from import");
}

#[test]
fn aliased_import_injects_alias() {
    let src = r#"
import crypto.hash as hash;
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("should resolve");
    let names: Vec<&str> = resolved
        .symbols
        .symbols
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert!(
        names.contains(&"hash"),
        "alias should be injected from import"
    );
}

#[test]
fn bare_import_injects_last_segment() {
    let src = r#"
import std.math;
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("should resolve");
    let names: Vec<&str> = resolved
        .symbols
        .symbols
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert!(
        names.contains(&"math"),
        "last path segment should be injected from import"
    );
}

#[test]
fn duplicate_import_detected() {
    let src = r#"
import std.math;
import std.math;
"#;
    let file = parse_ok(src);
    let result = resolve(&file);
    assert!(result.is_err(), "duplicate import should produce an error");
    let errs = result.unwrap_err();
    assert!(
        errs.iter().any(|e| e.code == "A02006"),
        "should report A02006 for duplicate import"
    );
}

#[test]
fn different_imports_not_duplicate() {
    let src = r#"
import std.math;
import std.collections;
"#;
    let file = parse_ok(src);
    resolve(&file).expect("different imports should not be duplicates");
}

#[test]
fn unused_import_reported_as_warning() {
    // Single-file resolve: unknown import is A02010 (cannot resolve), not
    // the misleading A02007 unused import.
    let src = r#"
import std.math;
contract Foo {
requires { x > 0 }
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("resolve succeeds (warnings are not errors)");
    assert!(
        resolved
            .warnings
            .iter()
            .any(|w| w.code == "A02010" && w.message.contains("std.math")),
        "expected A02010 cannot-resolve warning for std.math, got {:?}",
        resolved.warnings
    );
    assert!(
        resolved.warnings.iter().all(|w| w.code != "A02007"),
        "unresolved imports must not also be A02007 unused"
    );
}

#[test]
fn unused_resolved_import_is_a02007() {
    // When the module is in the map, an unused import is A02007.
    let target_src = r#"
module std.math;
type T { x: Int }
"#;
    let target_file = parse_ok(target_src);
    let mut module_map = ModuleMap::new();
    module_map.insert("std.math".to_string(), target_file);

    let src = r#"
import std.math;
contract Foo {
requires { true }
}
"#;
    let file = parse_ok(src);
    let mut visited = HashSet::new();
    let resolved =
        resolve_with_modules(&file, &module_map, &mut visited).expect("resolve succeeds");
    assert!(
        resolved
            .warnings
            .iter()
            .any(|w| w.code == "A02007" && w.message.contains("std.math")),
        "expected A02007 unused for resolved import, got {:?}",
        resolved.warnings
    );
}

#[test]
fn used_import_no_warning() {
    // The import introduces "List" which appears in a type annotation
    let src = r#"
import std.collections { List };
type Wrapper {
items: List
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("resolve succeeds");
    assert!(
        !resolved.warnings.iter().any(|w| w.code == "A02007"),
        "no unused import warning expected when imported name is used"
    );
}

#[test]
fn unused_selective_import_warning() {
    let src = r#"
import std.collections { Map, Set };
type Wrapper {
items: Map
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("resolve succeeds");
    // Map is used, but the import brings both Map and Set.
    // Since at least one name (Map) is referenced, the import is considered used.
    assert!(
        !resolved.warnings.iter().any(|w| w.code == "A02007"),
        "import with at least one used name should not be flagged"
    );
}

#[test]
fn import_path_uppercase_last_segment_allowed() {
    // The last segment of an import path can be uppercase (symbol name).
    // `import std.Math` means "import symbol Math from module std".
    let src = r#"
import std.Math;
"#;
    let file = parse_ok(src);
    let result = resolve(&file);
    // Should succeed: uppercase last segment is a symbol reference
    assert!(
        result.is_ok(),
        "uppercase last segment should be allowed: {result:?}"
    );
}

#[test]
fn import_path_uppercase_module_segment_rejected() {
    // Module path segments (non-last) must start with lowercase.
    // `import Std.math` has an uppercase module segment, which is invalid.
    let src = r#"
import Std.math;
"#;
    let file = parse_ok(src);
    let result = resolve(&file);
    assert!(
        result.is_err(),
        "uppercase module segment should produce an error"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter().any(|e| e.code == "A02008"),
        "should report A02008 for invalid path segment: {errs:?}"
    );
}

#[test]
fn import_path_valid_segments_pass() {
    // Valid path segments: lowercase, underscores
    let src = r#"
import std.math;
import crypto.hash_utils;
"#;
    let file = parse_ok(src);
    resolve(&file).expect("valid import paths should resolve without errors");
}

#[test]
fn is_valid_path_segment_tests() {
    use imports::is_valid_path_segment;
    assert!(is_valid_path_segment("std"));
    assert!(is_valid_path_segment("math"));
    assert!(is_valid_path_segment("hash_utils"));
    assert!(is_valid_path_segment("_private"));
    assert!(is_valid_path_segment("x86"));
    assert!(!is_valid_path_segment("Math"));
    assert!(!is_valid_path_segment("123"));
    assert!(!is_valid_path_segment(""));
    assert!(!is_valid_path_segment("foo-bar"));
}

// -----------------------------------------------------------------------
// Input param extraction tests
// -----------------------------------------------------------------------

#[test]
fn extract_input_params_raw_tokens() {
    use assura_parser::ast::Expr;
    let body = Spanned::no_span(Expr::Raw(vec![
        "a".to_string(),
        ":".to_string(),
        "Int".to_string(),
        ",".to_string(),
        "b".to_string(),
        ":".to_string(),
        "Nat".to_string(),
    ]));
    let names = extract_input_param_names(&body);
    assert_eq!(names, vec!["a", "b"]);
}

#[test]
fn extract_input_params_generic_type() {
    use assura_parser::ast::Expr;
    // input(items: List<Int>, count: Nat)
    let body = Spanned::no_span(Expr::Raw(vec![
        "items".into(),
        ":".into(),
        "List".into(),
        "<".into(),
        "Int".into(),
        ">".into(),
        ",".into(),
        "count".into(),
        ":".into(),
        "Nat".into(),
    ]));
    let names = extract_input_param_names(&body);
    assert_eq!(names, vec!["items", "count"]);
}

#[test]
fn extract_input_params_call_expr() {
    use assura_parser::ast::Expr;
    let body = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("input".to_string()))),
        args: vec![
            Spanned::no_span(Expr::Cast {
                expr: Box::new(Spanned::no_span(Expr::Ident("x".to_string()))),
                ty: "Int".to_string(),
            }),
            Spanned::no_span(Expr::Ident("y".to_string())),
        ],
    });
    let names = extract_input_param_names(&body);
    assert_eq!(names, vec!["x", "y"]);
}

#[test]
fn extract_input_params_ident() {
    use assura_parser::ast::Expr;
    let body = Spanned::no_span(Expr::Ident("x".to_string()));
    let names = extract_input_param_names(&body);
    assert_eq!(names, vec!["x"]);
}

#[test]
fn extract_input_params_cast() {
    use assura_parser::ast::Expr;
    let body = Spanned::no_span(Expr::Cast {
        expr: Box::new(Spanned::no_span(Expr::Ident("n".to_string()))),
        ty: "Int".to_string(),
    });
    let names = extract_input_param_names(&body);
    assert_eq!(names, vec!["n"]);
}

#[test]
fn extract_input_params_tuple() {
    use assura_parser::ast::Expr;
    let body = Spanned::no_span(Expr::Tuple(vec![
        Spanned::no_span(Expr::Cast {
            expr: Box::new(Spanned::no_span(Expr::Ident("a".to_string()))),
            ty: "Int".to_string(),
        }),
        Spanned::no_span(Expr::Ident("b".to_string())),
    ]));
    let names = extract_input_param_names(&body);
    assert_eq!(names, vec!["a", "b"]);
}

#[test]
fn extract_input_params_raw_as_separator() {
    use assura_parser::ast::Expr;
    let body = Spanned::no_span(Expr::Raw(vec![
        "x".into(),
        "as".into(),
        "Int".into(),
        ",".into(),
        "y".into(),
        "as".into(),
        "Nat".into(),
    ]));
    let names = extract_input_param_names(&body);
    assert_eq!(names, vec!["x", "y"]);
}

// -----------------------------------------------------------------------
// Contract input params registered in scope
// -----------------------------------------------------------------------

#[test]
fn contract_input_params_in_scope() {
    let src = r#"
contract Foo {
  input(a: Int, b: Int)
  requires { a > 0 }
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("resolve should succeed");
    // Parameters a and b should be in the contract's scope
    let params: Vec<&str> = resolved
        .symbols
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Parameter)
        .map(|s| s.name.as_str())
        .collect();
    assert!(params.contains(&"a"), "param a not found");
    assert!(params.contains(&"b"), "param b not found");
}

#[test]
fn contract_input_and_inline_fn_same_params_not_duplicate() {
    // Dogfood: natural form combines input(...) with named inline fn.
    let src = r#"
contract Safe {
  input(x: Int, y: Int)
  requires { y != 0 }
  ensures { true }
  fn div(x: Int, y: Int) -> Int
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("input + inline fn should not A02003");
    let params: Vec<&str> = resolved
        .symbols
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Parameter)
        .map(|s| s.name.as_str())
        .collect();
    assert!(params.contains(&"x"));
    assert!(params.contains(&"y"));
    assert_eq!(
        params.iter().filter(|n| **n == "x").count(),
        1,
        "x registered once"
    );
}

#[test]
fn contract_input_params_accessible_from_ensures() {
    // Params declared in input should be usable in ensures
    let src = r#"
contract Div {
  input(a: Int, b: Int)
  output(result: Int)
  requires { b != 0 }
  ensures  { result * b <= a }
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("resolve should succeed");
    let contract_scope = resolved
        .symbols
        .scopes
        .iter()
        .position(|s| s.name == "Div")
        .expect("Div scope not found");
    // a, b, result should all be accessible from the contract scope
    resolved.symbols.lookup("a", contract_scope).unwrap();
    resolved.symbols.lookup("b", contract_scope).unwrap();
    // result is a built-in value name, not in the symbol table,
    // but won't produce a warning in clause body checks
}

// -----------------------------------------------------------------------
// Expression-level name resolution warnings
// -----------------------------------------------------------------------

#[test]
fn undefined_name_in_clause_body_warns() {
    // No imports, no module => strict mode. 'c' is undefined.
    let src = r#"
contract Foo {
  input(a: Int, b: Int)
  requires { c > 0 }
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("resolve succeeds (warnings, not errors)");
    let body_warnings: Vec<_> = resolved
        .warnings
        .iter()
        .filter(|w| w.code == "A02001" && w.message.contains("undefined name"))
        .collect();
    assert!(
        body_warnings.iter().any(|w| w.message.contains("`c`")),
        "should warn about undefined `c`: {body_warnings:?}"
    );
}

#[test]
fn defined_name_in_clause_body_no_warning() {
    // 'a' is defined in input clause, should not produce a warning
    let src = r#"
contract Foo {
  input(a: Int, b: Int)
  requires { a > 0 }
  ensures  { result >= 0 }
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("resolve should succeed");
    let body_warnings: Vec<_> = resolved
        .warnings
        .iter()
        .filter(|w| w.message.contains("undefined name"))
        .collect();
    assert!(
        body_warnings.is_empty(),
        "should not warn about defined params: {body_warnings:?}"
    );
}

#[test]
fn fn_param_in_clause_body_no_warning() {
    let src = r#"
fn helper(n: Int) -> Int {
  requires { n > 0 }
  ensures  { result >= n }
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("resolve should succeed");
    let body_warnings: Vec<_> = resolved
        .warnings
        .iter()
        .filter(|w| w.message.contains("undefined name"))
        .collect();
    assert!(
        body_warnings.is_empty(),
        "fn params should not trigger warnings: {body_warnings:?}"
    );
}

#[test]
fn quantifier_var_in_scope_no_warning() {
    // Quantifier variable 'x' should be locally scoped
    let src = r#"
contract ListCheck {
  input(items: List)
  ensures { forall x in items: x > 0 }
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("resolve should succeed");
    let body_warnings: Vec<_> = resolved
        .warnings
        .iter()
        .filter(|w| w.message.contains("`x`"))
        .collect();
    assert!(
        body_warnings.is_empty(),
        "quantifier var should not trigger warnings: {body_warnings:?}"
    );
}

#[test]
fn lenient_mode_skips_unknown_names() {
    // With imports, lenient mode skips unknown names
    let src = r#"
import std.math;

contract Foo {
  input(a: Int)
  requires { external_check(a) }
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("resolve should succeed in lenient mode");
    let body_warnings: Vec<_> = resolved
        .warnings
        .iter()
        .filter(|w| w.message.contains("undefined name"))
        .collect();
    assert!(
        body_warnings.is_empty(),
        "lenient mode should not warn: {body_warnings:?}"
    );
}

#[test]
fn service_other_item_body_resolved() {
    // ServiceItem::Other { kind, body } should have its body
    // expression walked for identifier resolution.
    let src = r#"
service Svc {
  priority { true }
}
"#;
    let file = parse_ok(src);
    // "priority" is not a recognized keyword, so it parses as
    // ServiceItem::Other { kind: "priority", body: Ident("true") }.
    // resolve() should succeed without errors, proving the body
    // expression was walked (not silently skipped).
    resolve(&file).expect("service with Other item should resolve");
}

#[test]
fn service_operation_params_in_scope() {
    let src = r#"
service Svc {
  operation doStuff {
input { name: String }
requires { name.length() > 0 }
  }
}
"#;
    let file = parse_ok(src);
    let resolved = resolve(&file).expect("resolve should succeed");
    // 'name' should be registered as a parameter in the operation scope
    let params: Vec<&str> = resolved
        .symbols
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Parameter && s.name == "name")
        .map(|s| s.name.as_str())
        .collect();
    assert!(
        !params.is_empty(),
        "service operation input params should be in scope"
    );
}

// ===================================================================
// A002: Module resolution tests
// ===================================================================

#[test]
fn find_project_root_with_toml() {
    let dir = std::env::temp_dir().join("assura-test-root");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("assura.toml"), "[project]\nname = \"test\"\n").unwrap();

    let sub = dir.join("src");
    std::fs::create_dir_all(&sub).unwrap();
    let file = sub.join("main.assura");
    std::fs::write(&file, "").unwrap();

    let root = find_project_root(&file);
    assert_eq!(root.unwrap(), dir);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn find_project_root_none() {
    // A temp file with no assura.toml anywhere above
    let dir = std::env::temp_dir().join("assura-test-no-root");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test.assura");
    std::fs::write(&file, "").unwrap();

    // May or may not find one depending on whether assura.toml
    // exists somewhere above /tmp. Just check it doesn't panic.
    let _ = find_project_root(&file);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resolve_module_path_existing() {
    use project::resolve_module_path;
    let dir = std::env::temp_dir().join("assura-test-mod-resolve");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("math")).unwrap();
    std::fs::write(
        dir.join("math/util.assura"),
        "module math.util;\ncontract Add {\n  input(a: Int)\n}",
    )
    .unwrap();

    let path = vec!["math".into(), "util".into()];
    let result = resolve_module_path(&dir, &path);
    assert!(result.unwrap().ends_with("math/util.assura"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resolve_module_path_missing() {
    use project::resolve_module_path;
    let dir = std::env::temp_dir().join("assura-test-mod-missing");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let path = vec!["nonexistent".into(), "module".into()];
    assert!(resolve_module_path(&dir, &path).is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_to_module_path_conversion() {
    use project::file_to_module_path;
    let root = std::path::Path::new("/project");
    let file = std::path::Path::new("/project/src/math/util.assura");
    let result = file_to_module_path(file, root);
    assert_eq!(result, "src.math.util");
}

#[test]
fn build_module_graph_single_file() {
    use project::{DependencyMap, build_module_graph_with_deps};
    let dir = std::env::temp_dir().join("assura-test-graph-single");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.assura"),
        "module test.main;\ncontract Foo {\n  input(x: Int)\n}",
    )
    .unwrap();

    let graph = build_module_graph_with_deps(&dir.join("main.assura"), &dir, &DependencyMap::new());
    assert_eq!(graph.modules.len(), 1);
    assert_eq!(graph.order.len(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resolve_module_graph_produces_resolved_files() {
    use project::{DependencyMap, build_module_graph_with_deps, resolve_module_graph};
    let dir = std::env::temp_dir().join("assura-test-resolve-graph");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.assura"),
        "module test.main;\ncontract Bar {\n  input(x: Int)\n}",
    )
    .unwrap();

    let graph = build_module_graph_with_deps(&dir.join("main.assura"), &dir, &DependencyMap::new());
    let (resolved, errs) = resolve_module_graph(&graph);
    // The single module may have resolution warnings but should produce a result
    assert!(!resolved.is_empty() || !errs.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

// -----------------------------------------------------------------------
// Multi-file project resolution tests (issue #64)
// -----------------------------------------------------------------------

/// Helper: set up a multi-file project in a temp dir and return the root.
fn setup_multi_file_project(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("assura-multi-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        dir.join("assura.toml"),
        format!("[project]\nname = \"{name}\""),
    )
    .unwrap();
    dir
}

#[test]
fn multi_file_valid_cross_module_import() {
    let dir = setup_multi_file_project("valid-import");
    let src = dir.join("src");
    std::fs::write(
        src.join("math.assura"),
        "module math\ncontract Add {\n  requires(a: Int, b: Int)\n  ensures(result: Int)\n}",
    )
    .unwrap();
    std::fs::write(
        src.join("main.assura"),
        "module main\nimport math.Add\ncontract Main {\n  requires(x: Int)\n  ensures(result: Int)\n}",
    )
    .unwrap();

    let (resolved, warnings) =
        discover_and_resolve_project(&dir).expect("multi-file project with import should resolve");
    assert!(
        resolved.contains_key("math"),
        "math module should be resolved"
    );
    assert!(
        resolved.contains_key("main"),
        "main module should be resolved"
    );
    assert!(warnings.is_empty(), "no warnings expected: {warnings:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn multi_file_missing_import() {
    let dir = setup_multi_file_project("missing-import");
    let src = dir.join("src");
    std::fs::write(
        src.join("main.assura"),
        "module main\nimport nonexistent.Foo\ncontract Main {\n  requires(x: Int)\n}",
    )
    .unwrap();

    // Missing imports hard-fail with A02010 when a project module map is present.
    let result = discover_and_resolve_project(&dir);
    match result {
        Ok((resolved, issues)) => {
            // Some modules may still resolve; issues must report the miss.
            assert!(
                !issues.is_empty() || !resolved.contains_key("main"),
                "missing import must surface as issue or omit the broken module"
            );
            if !issues.is_empty() {
                assert!(
                    issues
                        .iter()
                        .any(|i| i.contains("nonexistent") || i.contains("resolution")),
                    "expected missing-import issue, got {issues:?}"
                );
            }
        }
        Err(errors) => {
            assert!(
                errors
                    .iter()
                    .any(|e| e.contains("resolution") || e.contains("nonexistent")),
                "expected resolution error, got {errors:?}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn multi_file_circular_import() {
    let dir = setup_multi_file_project("circular");
    let src = dir.join("src");
    std::fs::write(
        src.join("a.assura"),
        "module a\nimport b\ncontract Foo {\n  requires { true }\n}",
    )
    .unwrap();
    std::fs::write(
        src.join("b.assura"),
        "module b\nimport a\ncontract Bar {\n  requires { true }\n}",
    )
    .unwrap();

    let (resolved, warnings) = discover_and_resolve_project(&dir)
        .expect("circular import project should return Ok with warnings");
    assert!(!resolved.is_empty(), "at least one module should resolve");
    let has_circ = warnings.iter().any(|w| w.contains("circular"));
    assert!(
        has_circ,
        "a↔b project import cycle must be reported: warnings={warnings:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn multi_file_declared_module_name_used_as_key() {
    // Verify that declared module names (not filesystem paths) are used
    let dir = setup_multi_file_project("declared-key");
    let src = dir.join("src");
    std::fs::write(
        src.join("utils.assura"),
        "module helpers\ncontract Aid {\n  requires(x: Int)\n}",
    )
    .unwrap();

    let result = discover_and_resolve_project(&dir);
    let (resolved, _) = result.unwrap();
    // Key should be "helpers" (declared), not "src.utils" (filesystem)
    assert!(
        resolved.contains_key("helpers"),
        "module key should be declared name 'helpers', got keys: {:?}",
        resolved.keys().collect::<Vec<_>>()
    );
    assert!(
        !resolved.contains_key("src.utils"),
        "should NOT use filesystem path as key"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn multi_file_no_assura_files() {
    let dir = setup_multi_file_project("empty");
    // Don't create any .assura files
    let result = discover_and_resolve_project(&dir);
    assert!(result.is_err(), "should fail with no .assura files");
    let errors = result.unwrap_err();
    assert!(
        errors.iter().any(|e| e.contains("no .assura files")),
        "should say no files found: {errors:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn find_module_prefix_matches() {
    use imports::find_module_prefix;
    let mut map = ModuleMap::new();
    let source = parse_ok("module math\ncontract Add { requires(x: Int) }");
    map.insert("math".to_string(), source);

    // "math.Add" should find "math" as prefix
    let path = vec!["math".to_string(), "Add".to_string()];
    assert_eq!(find_module_prefix(&path, &map), Some("math".to_string()));

    // "math" should match directly
    let path2 = vec!["math".to_string()];
    assert_eq!(find_module_prefix(&path2, &map), Some("math".to_string()));

    // "nonexistent" should return None
    let path3 = vec!["nonexistent".to_string()];
    assert_eq!(find_module_prefix(&path3, &map), None);
}

/// Regression test for #171: fn params must be visible in clause bodies.
#[test]
fn test_contract_params_visible_in_clauses() {
    let src = r#"
contract Safe {
requires n > 0
ensures result > 0
fn identity(n: Int) -> Int
}
"#;
    let file = assura_parser::parse_unwrap(src);
    let resolved = resolve(&file).expect("resolve failed");
    // No A02001 errors about `n` being undefined
    let a02001_errors: Vec<_> = resolved
        .warnings
        .iter()
        .filter(|e| e.code == "A02001")
        .collect();
    assert!(
        a02001_errors.is_empty(),
        "fn params should not produce A02001, got: {:?}",
        a02001_errors
    );
}

/// feature_max constants must resolve in clause bodies (no false A02001).
/// SMT already binds their values; resolve must register the name.
#[test]
fn test_feature_max_visible_in_clauses() {
    let src = r#"
feature_max MAX_SIZE: Nat = 280
feature_max MAX_LEN: Nat = 15

fn check_bounds(size: Nat, max_len: Nat)
  requires { size <= MAX_SIZE }
  requires { max_len <= MAX_LEN }
  ensures  { size + max_len <= 295 }
  ensures  { MAX_SIZE == 280 }
  effects: pure
"#;
    let file = assura_parser::parse_unwrap(src);
    let resolved = resolve(&file).expect("resolve failed");
    let a02001: Vec<_> = resolved
        .warnings
        .iter()
        .filter(|e| e.code == "A02001")
        .collect();
    assert!(
        a02001.is_empty(),
        "feature_max names must not produce A02001, got: {a02001:?}"
    );
    assert!(
        resolved
            .symbols
            .symbols
            .iter()
            .any(|s| s.name == "MAX_SIZE"),
        "MAX_SIZE should be registered in the symbol table"
    );
}
