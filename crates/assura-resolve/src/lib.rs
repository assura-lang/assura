//! Name resolution and symbol table for the Assura contract language.
//!
//! The resolver walks the parsed AST, collects all declarations into a
//! `SymbolTable`, detects duplicate names (A02003), registers built-in
//! types, resolves import declarations, and checks type references
//! (A02001 for unknown types). Full expression-level name resolution
//! (ambiguous A02002) is deferred to later tasks.

mod clause_names;
mod errors;
mod imports;
mod project;
mod symbols;
mod type_refs;
mod unused;

use std::collections::HashSet;

use assura_parser::ast::{ClauseKind, Decl, ServiceItem, SourceFile, SpExpr, TypeBody};

// Re-export public API
pub use errors::{ResolutionError, ResolvedFile};
pub use imports::{ImportStatus, ModuleMap, ResolvedImport};
pub use project::{
    DependencyMap, ProjectResult, discover_and_resolve_project,
    discover_and_resolve_project_with_deps, find_project_root, resolve_dependency_map,
    resolve_project, resolve_project_with_deps,
};
pub use symbols::{Scope, Symbol, SymbolKind, SymbolTable};

// Crate-internal imports
use clause_names::resolve_clause_body_names;
use imports::resolve_imports;
use symbols::try_insert;
use type_refs::resolve_type_refs;
use unused::{check_unused_imports, collect_referenced_names};

// ---------------------------------------------------------------------------
// Built-in types
// ---------------------------------------------------------------------------

const BUILTIN_TYPES: &[&str] = &[
    "Int", "Nat", "Float", "Bool", "String", "Bytes", "Unit", "Never", "List", "Map", "Set",
    "Option", "Result", // Sized integer types used in demos
    "U8", "U16", "U32", "U64", "I8", "I16", "I32", "I64", "F32", "F64", "Sequence",
];

// ---------------------------------------------------------------------------
// Built-in value/function names (always in scope for clause bodies)
// ---------------------------------------------------------------------------

/// Names that are always available inside contract/function clause bodies.
/// These include keywords-as-values and common built-in functions.
const BUILTIN_VALUE_NAMES: &[&str] = &[
    "result",
    "self",
    "true",
    "false",
    // Common built-in functions / measures (spec Section 9)
    "len",
    "size",
    "abs",
    "min",
    "max",
    "clamp",
    "signum",
    "gcd",
    "lcm",
    "divmod",
    "pow",
    "contains",
    "keys",
    "values",
    "get",
    "put",
    "set",
    "push",
    "pop",
    "head",
    "tail",
    "first",
    "last",
    "map",
    "filter",
    "fold",
    "sum",
    "count",
    "any",
    "all",
    "concat",
    "split",
    "trim",
    "substring",
    "index_of",
    "capacity",
    "length",
    "is_empty",
    // Quantifier / logic
    "forall",
    "exists",
    "old",
    "ghost",
    // Effects (commonly appear as clause body identifiers)
    "pure",
    "io",
    "mem",
    "db",
    "net",
    "audit",
    "crypto",
    "read",
    "write",
    "alloc",
    "free",
    "log",
    // Other keywords that may appear as values
    "deterministic",
    "taint",
    "untrusted",
    "validated",
    "secret",
    "incremental",
    "monotonic",
];

// ---------------------------------------------------------------------------
// Input clause parameter extraction
// ---------------------------------------------------------------------------

/// Extract parameter names from an `input` clause body.
///
/// Extract parameter names from a clause body using the shared parser utility.
fn extract_input_param_names(body: &SpExpr) -> Vec<String> {
    assura_parser::ast::extract_clause_params(body)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

/// Register input and output clause parameters from a clause list into a scope.
fn register_clause_params(
    clauses: &[assura_parser::ast::Clause],
    table: &mut SymbolTable,
    errors: &mut Vec<ResolutionError>,
    scope_id: usize,
    span: &assura_parser::ast::Span,
) {
    for clause in clauses {
        if clause.kind == ClauseKind::Input || clause.kind == ClauseKind::Output {
            for param_name in extract_input_param_names(&clause.body) {
                try_insert(
                    table,
                    errors,
                    scope_id,
                    &param_name,
                    SymbolKind::Parameter,
                    span.clone(),
                );
            }
        }
    }
}

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

    // --- Stdlib prelude types (Pos, NonNeg, Email, etc.) ---
    for &name in &assura_stdlib::prelude_type_names() {
        // Skip types already registered as built-ins above.
        if table.scopes[root].symbols.contains_key(name) {
            continue;
        }
        table
            .insert(root, name, SymbolKind::BuiltinType, 0..0)
            .expect("stdlib prelude types should not collide with built-ins");
    }

    // --- Stdlib prelude contracts (abs, min, max, clamp, ...) ---
    for &name in &assura_stdlib::prelude_contract_names() {
        if table.scopes[root].symbols.contains_key(name) {
            continue;
        }
        table
            .insert(root, name, SymbolKind::ContractDef, 0..0)
            .expect("stdlib prelude contracts should not collide");
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

    // --- Inject imported symbols into module scope ---
    // Selective imports (`import X { A, B }`) inject each named item.
    // Aliased imports (`import X as Y`) inject the alias as a module reference.
    // Unselective imports (`import X`) inject the last path segment.
    for imp in &resolved_imports {
        if imp.status == ImportStatus::Circular {
            continue;
        }
        if !imp.items.is_empty() {
            // Selective: inject each named item as a BuiltinType (external type)
            for item in &imp.items {
                try_insert(
                    &mut table,
                    &mut errors,
                    module,
                    item,
                    SymbolKind::BuiltinType,
                    imp.span.clone(),
                );
            }
        } else if let Some(alias) = &imp.alias {
            // Aliased: inject the alias as a module-level symbol
            try_insert(
                &mut table,
                &mut errors,
                module,
                alias,
                SymbolKind::BuiltinType,
                imp.span.clone(),
            );
        } else if let Some(last) = imp.path.last() {
            // Bare import: inject the last path segment
            try_insert(
                &mut table,
                &mut errors,
                module,
                last,
                SymbolKind::BuiltinType,
                imp.span.clone(),
            );
        }
    }

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
                    // Register input and output clause parameters in the contract scope
                    for clause in &c.clauses {
                        if clause.kind == ClauseKind::Input || clause.kind == ClauseKind::Output {
                            for param_name in extract_input_param_names(&clause.body) {
                                try_insert(
                                    &mut table,
                                    &mut errors,
                                    contract_scope,
                                    &param_name,
                                    SymbolKind::Parameter,
                                    decl.span.clone(),
                                );
                            }
                        }
                    }
                    // Register parameters from inline fn definitions
                    for p in &c.fn_params {
                        try_insert(
                            &mut table,
                            &mut errors,
                            contract_scope,
                            &p.name,
                            SymbolKind::Parameter,
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
            Decl::Bind(b) => {
                let inserted = try_insert(
                    &mut table,
                    &mut errors,
                    module,
                    &b.name,
                    SymbolKind::BindFn,
                    decl.span.clone(),
                );
                if inserted {
                    let fn_scope = table.push_scope(&b.name, Some(module));
                    for p in &b.params {
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
                            ServiceItem::Operation { name, clauses, .. } => {
                                let ins = try_insert(
                                    &mut table,
                                    &mut errors,
                                    svc_scope,
                                    name,
                                    SymbolKind::Operation,
                                    decl.span.clone(),
                                );
                                if ins {
                                    let op_scope = table.push_scope(name, Some(svc_scope));
                                    register_clause_params(
                                        clauses,
                                        &mut table,
                                        &mut errors,
                                        op_scope,
                                        &decl.span,
                                    );
                                }
                            }
                            ServiceItem::Query { name, clauses, .. } => {
                                let ins = try_insert(
                                    &mut table,
                                    &mut errors,
                                    svc_scope,
                                    name,
                                    SymbolKind::Query,
                                    decl.span.clone(),
                                );
                                if ins {
                                    let q_scope = table.push_scope(name, Some(svc_scope));
                                    register_clause_params(
                                        clauses,
                                        &mut table,
                                        &mut errors,
                                        q_scope,
                                        &decl.span,
                                    );
                                }
                            }
                            // States, Invariant, and Other don't introduce
                            // named symbols at the service scope level.
                            // (TypeDef and EnumDef are handled above.)
                            ServiceItem::States(_)
                            | ServiceItem::Invariant(_)
                            | ServiceItem::Other { .. } => {}
                        }
                    }
                }
            }
            Decl::Prophecy(p) => {
                // Ghost prophecy variables are registered as ghost symbols.
                // They don't create a child scope.
                try_insert(
                    &mut table,
                    &mut errors,
                    module,
                    &p.name,
                    SymbolKind::Prophecy,
                    decl.span.clone(),
                );
            }
            Decl::CodecRegistry(cr) => {
                try_insert(
                    &mut table,
                    &mut errors,
                    module,
                    &cr.name,
                    SymbolKind::CodecRegistry,
                    decl.span.clone(),
                );
            }
            Decl::Block { name, .. } => {
                // Generic blocks (feature, incremental, liveness, etc.)
                // create a child scope for their body but don't register
                // as a named symbol (they are structural, not definitions).
                if !name.is_empty() {
                    table.push_scope(name, Some(module));
                }
            }
        }
    }

    // --- Resolve type references (T012) ---
    // Walk declarations and check that every type name used in field types,
    // parameter types, return types, and type aliases resolves to a known
    // type (built-in, user-defined, or type parameter). Only report A02001
    // for names that are *definitely* unknown: skip names that could
    // plausibly come from external sources (imports, project profiles, etc.).
    resolve_type_refs(source, &table, &resolved_imports, module, &mut errors);

    // --- Check for unused imports (A02007) ---
    // These are warnings, not errors: they don't prevent resolution.
    let referenced_names = collect_referenced_names(source);
    let mut warnings = Vec::new();
    check_unused_imports(&resolved_imports, &referenced_names, &mut warnings);

    // --- Expression-level name resolution in clause bodies ---
    // These produce warnings, not hard errors, since we may not know about
    // all names in scope (external modules, built-in functions, etc.).
    resolve_clause_body_names(source, &table, &resolved_imports, module, &mut warnings);

    // Remove this module from the visited set now that resolution is done.
    visited.remove(&module_name);

    if errors.is_empty() {
        Ok(ResolvedFile {
            source: source.clone(),
            symbols: table,
            imports: resolved_imports,
            warnings,
        })
    } else {
        Err(errors)
    }
}

#[cfg(test)]
#[path = "resolve_tests.rs"]
mod tests;
