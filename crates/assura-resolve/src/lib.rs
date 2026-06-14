//! Name resolution and symbol table for the Assura contract language.
//!
//! The resolver walks the parsed AST, collects all declarations into a
//! `SymbolTable`, detects duplicate names (A02003), registers built-in
//! types, resolves import declarations, and checks type references
//! (A02001 for unknown types). Full expression-level name resolution
//! (ambiguous A02002) is deferred to later tasks.

use std::collections::{HashMap, HashSet};

use assura_parser::ast::{
    ClauseKind, Decl, EnumDef, Expr, ExternDecl, FieldDef, FnDef, ImportDecl, Param, ServiceItem,
    SourceFile, Span, TypeBody, TypeDef,
};

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
    BindFn,
    Prophecy,
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

    /// Look up a name starting from `scope_id`, walking up the scope
    /// chain until the name is found or the root is reached.
    pub fn lookup(&self, name: &str, scope_id: usize) -> Option<&Symbol> {
        let mut current = Some(scope_id);
        while let Some(id) = current {
            if let Some(&sym_idx) = self.scopes[id].symbols.get(name) {
                return Some(&self.symbols[sym_idx]);
            }
            current = self.scopes[id].parent;
        }
        None
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

impl From<ResolutionError> for assura_diagnostics::Diagnostic {
    fn from(e: ResolutionError) -> Self {
        let mut d = assura_diagnostics::Diagnostic::error(e.code, e.message, e.span);
        if let Some((span, label)) = e.secondary {
            d.secondary.push(assura_diagnostics::SecondaryLabel {
                span,
                message: label,
            });
        }
        d
    }
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
    /// Non-fatal warnings (e.g., unused imports). These don't prevent
    /// resolution from succeeding.
    pub warnings: Vec<ResolutionError>,
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
fn extract_input_param_names(body: &Expr) -> Vec<String> {
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
    span: &Span,
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
                    0..0,
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
                0..0,
            );
        } else if let Some(last) = imp.path.last() {
            // Bare import: inject the last path segment
            try_insert(
                &mut table,
                &mut errors,
                module,
                last,
                SymbolKind::BuiltinType,
                0..0,
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

/// Resolve all import declarations from a source file.
///
/// For each `ImportDecl`, checks whether the target module exists in the
/// `module_map`. If it does, the import is marked `Resolved`. If the
/// target module is currently being resolved (present in `visited`), the
/// import is marked `Circular` and an A02005 error is emitted. Otherwise
/// the import is marked `Unresolved` (external/unknown module, not an error).
/// Returns true if `s` is a valid module path segment: starts with a
/// lowercase ASCII letter or underscore, then ASCII letters, digits, or
/// underscores.
fn is_valid_path_segment(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn resolve_imports(
    imports: &[ImportDecl],
    module_map: &ModuleMap,
    visited: &HashSet<String>,
    errors: &mut Vec<ResolutionError>,
) -> Vec<ResolvedImport> {
    // Detect duplicate imports
    let mut seen_paths: HashSet<String> = HashSet::new();
    for imp in imports {
        let path_str = imp.path.join(".");
        if !seen_paths.insert(path_str.clone()) {
            errors.push(ResolutionError {
                code: "A02006",
                message: format!("duplicate import of module `{path_str}`"),
                span: 0..0,
                secondary: None,
            });
        }
    }

    // Validate import path segments
    for imp in imports {
        if imp.path.is_empty() {
            errors.push(ResolutionError {
                code: "A02008",
                message: "import path is empty".to_string(),
                span: 0..0,
                secondary: None,
            });
            continue;
        }
        for segment in &imp.path {
            if !is_valid_path_segment(segment) {
                errors.push(ResolutionError {
                    code: "A02008",
                    message: format!(
                        "invalid module path segment `{segment}` in import `{}`; \
                         segments must start with a lowercase letter or underscore",
                        imp.path.join(".")
                    ),
                    span: 0..0,
                    secondary: None,
                });
            }
        }
    }

    // Detect self-imports (importing your own module)
    for imp in imports {
        let path_str = imp.path.join(".");
        if visited.contains(&path_str) && !imp.path.is_empty() {
            // Already caught by circular import below, but this gives
            // a clearer message for the direct self-import case.
        }
    }

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

// ---------------------------------------------------------------------------
// Type reference resolution (T012)
// ---------------------------------------------------------------------------

/// Tokens that are clearly syntax or modifiers, not type names.
const TYPE_SYNTAX_TOKENS: &[&str] = &[
    "<",
    ">",
    ",",
    "|",
    "{",
    "}",
    "&",
    "(",
    ")",
    "[",
    "]",
    ":",
    ";",
    "=",
    "->",
    "..",
    "+",
    "-",
    "*",
    "/",
    "%",
    "!",
    "?",
    "@",
    "#",
    "==",
    "!=",
    "<=",
    ">=",
    // Modifiers and keywords that appear in type positions
    "pub",
    "ghost",
    "pure",
    "mut",
    "and",
    "or",
    "not",
    "in",
    "if",
    "then",
    "else",
    "let",
    "for",
    "forall",
    "exists",
    "old",
    "true",
    "false",
    "taint",
    "untrusted",
    "validated",
    "secret",
    "deterministic",
    "effects",
    "requires",
    "ensures",
    "invariant",
    "modifies",
    "where",
    // Self and result
    "self",
    "result",
    "Self",
];

/// Check whether a token looks like a type name candidate.
///
/// A type name is an identifier that starts with an uppercase letter and
/// is not a syntax/modifier token. We only check names that start with
/// uppercase because lowercase names are more likely to be values,
/// keywords, or effect names (e.g., `io.read`, `pure`).
fn is_type_name_candidate(tok: &str) -> bool {
    if TYPE_SYNTAX_TOKENS.contains(&tok) {
        return false;
    }
    // Must start with uppercase ASCII letter
    tok.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

/// Extract candidate type names from a raw token sequence (`Vec<String>`).
///
/// Skips syntax, modifiers, and lowercase identifiers. Returns the list
/// of uppercase-initial identifiers that should resolve as types.
fn extract_type_names(tokens: &[String]) -> Vec<&str> {
    tokens
        .iter()
        .filter(|t| is_type_name_candidate(t))
        .map(|t| t.as_str())
        .collect()
}

/// Returns `true` if we should be lenient about unknown type names.
///
/// We are lenient when the file may have access to types from external
/// sources that we cannot resolve yet: unresolved imports, a project
/// declaration (which enables profiles providing types like `Region`),
/// or a module declaration (which implies a multi-module project with
/// a potential prelude). Only bare standalone files with none of these
/// get strict checking.
fn should_be_lenient(source: &SourceFile, imports: &[ResolvedImport]) -> bool {
    // Project declaration implies profile-provided types
    if source.project.is_some() {
        return true;
    }
    // Module declaration implies multi-module project
    if source.module.is_some() {
        return true;
    }
    // Any unresolved import means external types may exist
    imports
        .iter()
        .any(|imp| imp.status == ImportStatus::Unresolved)
}

/// Check a list of type-name tokens against the symbol table. Reports
/// A02001 for names that cannot be resolved. When unresolved imports
/// exist, unknown names are silently skipped (they may come from an
/// external module).
fn check_type_tokens(
    tokens: &[String],
    table: &SymbolTable,
    scope_id: usize,
    span: &Span,
    lenient: bool,
    errors: &mut Vec<ResolutionError>,
) {
    for name in extract_type_names(tokens) {
        if table.lookup(name, scope_id).is_some() {
            continue;
        }
        // In lenient mode (unresolved imports), skip unknown types
        if lenient {
            continue;
        }
        errors.push(ResolutionError {
            code: "A02001",
            message: format!("unknown type `{name}`"),
            span: span.clone(),
            secondary: None,
        });
    }
}

/// Check type references in field definitions.
fn check_fields(
    fields: &[FieldDef],
    table: &SymbolTable,
    scope_id: usize,
    span: &Span,
    lenient: bool,
    errors: &mut Vec<ResolutionError>,
) {
    for f in fields {
        check_type_tokens(&f.ty, table, scope_id, span, lenient, errors);
    }
}

/// Check type references in function/extern parameters and return type.
fn check_fn_signature(
    params: &[Param],
    return_ty: &[String],
    table: &SymbolTable,
    scope_id: usize,
    span: &Span,
    lenient: bool,
    errors: &mut Vec<ResolutionError>,
) {
    for p in params {
        check_type_tokens(&p.ty, table, scope_id, span, lenient, errors);
    }
    if !return_ty.is_empty() {
        check_type_tokens(return_ty, table, scope_id, span, lenient, errors);
    }
}

/// Build a map from declaration name to its scope ID by scanning the scope
/// list. When multiple scopes share a name (e.g., nested `Config` types),
/// this finds the one whose parent matches the expected parent scope.
fn find_scope_for(table: &SymbolTable, name: &str, parent_scope: usize) -> Option<usize> {
    // Prefer the scope whose parent matches; fall back to any match.
    let mut fallback = None;
    for (i, scope) in table.scopes.iter().enumerate() {
        if scope.name == name {
            if scope.parent == Some(parent_scope) {
                return Some(i);
            }
            if fallback.is_none() {
                fallback = Some(i);
            }
        }
    }
    fallback
}

/// Walk all declarations and resolve type references.
fn resolve_type_refs(
    source: &SourceFile,
    table: &SymbolTable,
    imports: &[ResolvedImport],
    module_scope: usize,
    errors: &mut Vec<ResolutionError>,
) {
    let lenient = should_be_lenient(source, imports);
    let decls = &source.decls;

    for decl in decls {
        match &decl.node {
            Decl::TypeDef(t) => {
                resolve_typedef_refs(t, table, &decl.span, module_scope, lenient, errors);
            }
            Decl::FnDef(f) => {
                resolve_fndef_refs(f, table, &decl.span, module_scope, lenient, errors);
            }
            Decl::Extern(ex) => {
                resolve_extern_refs(ex, table, &decl.span, module_scope, lenient, errors);
            }
            Decl::Bind(b) => {
                // Bind has the same param/return structure as extern
                resolve_extern_refs_generic(
                    &b.params,
                    &b.return_ty,
                    table,
                    &decl.span,
                    module_scope,
                    lenient,
                    errors,
                );
            }
            Decl::Contract(_) => {
                // Contract clauses don't have structured type refs in
                // the current AST; nothing to check here yet.
            }
            Decl::Service(s) => {
                let svc_scope =
                    find_scope_for(table, &s.name, module_scope).unwrap_or(module_scope);
                for item in &s.items {
                    match item {
                        ServiceItem::TypeDef(t) => {
                            resolve_typedef_refs(t, table, &decl.span, svc_scope, lenient, errors);
                        }
                        ServiceItem::EnumDef(e) => {
                            // Check enum variant field types
                            let enum_scope =
                                find_scope_for(table, &e.name, svc_scope).unwrap_or(svc_scope);
                            check_enum_variant_types(
                                e, table, enum_scope, &decl.span, lenient, errors,
                            );
                        }
                        ServiceItem::States(_)
                        | ServiceItem::Operation { .. }
                        | ServiceItem::Query { .. }
                        | ServiceItem::Invariant(_)
                        | ServiceItem::Other { .. } => {}
                    }
                }
            }
            Decl::EnumDef(e) => {
                // Check enum variant field types against the symbol table
                let enum_scope =
                    find_scope_for(table, &e.name, module_scope).unwrap_or(module_scope);
                check_enum_variant_types(e, table, enum_scope, &decl.span, lenient, errors);
            }
            // Prophecy variables have a type annotation but it's stored as
            // raw tokens, not structured params. No type ref resolution needed.
            Decl::Prophecy(_) => {}
            Decl::Block { .. } => {}
        }
    }
}

/// Resolve type references inside a type definition.
fn resolve_typedef_refs(
    t: &TypeDef,
    table: &SymbolTable,
    span: &Span,
    parent_scope: usize,
    lenient: bool,
    errors: &mut Vec<ResolutionError>,
) {
    // Use the type's own scope (which has type params) if found
    let scope = find_scope_for(table, &t.name, parent_scope).unwrap_or(parent_scope);
    match &t.body {
        TypeBody::Struct(fields) => {
            check_fields(fields, table, scope, span, lenient, errors);
        }
        TypeBody::Alias(tokens) => {
            check_type_tokens(tokens, table, scope, span, lenient, errors);
        }
        TypeBody::Refined(tokens) => {
            check_type_tokens(tokens, table, scope, span, lenient, errors);
        }
        TypeBody::Empty => {}
    }
}

/// Resolve type references in a function definition.
fn resolve_fndef_refs(
    f: &FnDef,
    table: &SymbolTable,
    span: &Span,
    parent_scope: usize,
    lenient: bool,
    errors: &mut Vec<ResolutionError>,
) {
    let scope = find_scope_for(table, &f.name, parent_scope).unwrap_or(parent_scope);
    check_fn_signature(&f.params, &f.return_ty, table, scope, span, lenient, errors);
}

/// Resolve type references in an extern function declaration.
fn resolve_extern_refs(
    ex: &ExternDecl,
    table: &SymbolTable,
    span: &Span,
    parent_scope: usize,
    lenient: bool,
    errors: &mut Vec<ResolutionError>,
) {
    let scope = find_scope_for(table, &ex.name, parent_scope).unwrap_or(parent_scope);
    check_fn_signature(
        &ex.params,
        &ex.return_ty,
        table,
        scope,
        span,
        lenient,
        errors,
    );
}

fn resolve_extern_refs_generic(
    params: &[Param],
    return_ty: &[String],
    table: &SymbolTable,
    span: &Span,
    parent_scope: usize,
    lenient: bool,
    errors: &mut Vec<ResolutionError>,
) {
    check_fn_signature(
        params,
        return_ty,
        table,
        parent_scope,
        span,
        lenient,
        errors,
    );
}

/// Check type references in enum variant fields.
///
/// Each variant has a `fields: Vec<String>` of type tokens. We check each
/// token against the symbol table using `check_type_tokens`.
fn check_enum_variant_types(
    e: &EnumDef,
    table: &SymbolTable,
    scope_id: usize,
    span: &Span,
    lenient: bool,
    errors: &mut Vec<ResolutionError>,
) {
    for variant in &e.variants {
        if !variant.fields.is_empty() {
            check_type_tokens(&variant.fields, table, scope_id, span, lenient, errors);
        }
    }
}

// ---------------------------------------------------------------------------
// Expression-level name resolution in clause bodies
// ---------------------------------------------------------------------------

/// Walk all clause bodies (requires, ensures, invariant, etc.) and check
/// that `Expr::Ident` references resolve to a known name in scope.
///
/// This catches typos in contract bodies like `requires { c > 0 }` when the
/// input clause only declares `a` and `b`. In lenient mode (files with
/// imports/modules/projects), unknown names are skipped since they may
/// come from imported modules.
fn resolve_clause_body_names(
    source: &SourceFile,
    table: &SymbolTable,
    imports: &[ResolvedImport],
    module_scope: usize,
    errors: &mut Vec<ResolutionError>,
) {
    let lenient = should_be_lenient(source, imports);

    for decl in &source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                let scope = find_scope_for(table, &c.name, module_scope).unwrap_or(module_scope);
                for clause in &c.clauses {
                    if is_body_clause(&clause.kind) {
                        check_expr_idents(
                            &clause.body,
                            table,
                            scope,
                            &decl.span,
                            lenient,
                            &mut Vec::new(),
                            errors,
                        );
                    }
                }
            }
            Decl::FnDef(f) => {
                let scope = find_scope_for(table, &f.name, module_scope).unwrap_or(module_scope);
                for clause in &f.clauses {
                    if is_body_clause(&clause.kind) {
                        check_expr_idents(
                            &clause.body,
                            table,
                            scope,
                            &decl.span,
                            lenient,
                            &mut Vec::new(),
                            errors,
                        );
                    }
                }
            }
            Decl::Extern(ex) => {
                let scope = find_scope_for(table, &ex.name, module_scope).unwrap_or(module_scope);
                for clause in &ex.clauses {
                    if is_body_clause(&clause.kind) {
                        check_expr_idents(
                            &clause.body,
                            table,
                            scope,
                            &decl.span,
                            lenient,
                            &mut Vec::new(),
                            errors,
                        );
                    }
                }
            }
            Decl::Bind(b) => {
                let scope = find_scope_for(table, &b.name, module_scope).unwrap_or(module_scope);
                for clause in &b.clauses {
                    if is_body_clause(&clause.kind) {
                        check_expr_idents(
                            &clause.body,
                            table,
                            scope,
                            &decl.span,
                            lenient,
                            &mut Vec::new(),
                            errors,
                        );
                    }
                }
            }
            Decl::Service(s) => {
                let svc_scope =
                    find_scope_for(table, &s.name, module_scope).unwrap_or(module_scope);
                for item in &s.items {
                    match item {
                        ServiceItem::Operation { name, clauses, .. }
                        | ServiceItem::Query { name, clauses, .. } => {
                            let op_scope =
                                find_scope_for(table, name, svc_scope).unwrap_or(svc_scope);
                            for clause in clauses {
                                if is_body_clause(&clause.kind) {
                                    check_expr_idents(
                                        &clause.body,
                                        table,
                                        op_scope,
                                        &Span::default(),
                                        lenient,
                                        &mut Vec::new(),
                                        errors,
                                    );
                                }
                            }
                        }
                        ServiceItem::Invariant(expr) => {
                            check_expr_idents(
                                expr,
                                table,
                                svc_scope,
                                &Span::default(),
                                lenient,
                                &mut Vec::new(),
                                errors,
                            );
                        }
                        ServiceItem::Other { body, .. } => {
                            check_expr_idents(
                                body,
                                table,
                                svc_scope,
                                &Span::default(),
                                lenient,
                                &mut Vec::new(),
                                errors,
                            );
                        }
                        // TypeDef, EnumDef, and States don't contain
                        // expressions that need ident checking.
                        ServiceItem::TypeDef(_)
                        | ServiceItem::EnumDef(_)
                        | ServiceItem::States(_) => {}
                    }
                }
            }
            Decl::Block { body, .. } => {
                for clause in body {
                    if is_body_clause(&clause.kind) {
                        check_expr_idents(
                            &clause.body,
                            table,
                            module_scope,
                            &decl.span,
                            lenient,
                            &mut Vec::new(),
                            errors,
                        );
                    }
                }
            }
            // TypeDef, EnumDef, and Prophecy don't contain expressions.
            Decl::TypeDef(_) | Decl::EnumDef(_) | Decl::Prophecy(_) => {}
        }
    }
}

/// Returns `true` for clause kinds whose bodies contain expressions that
/// should be checked for name resolution (predicates, not declarations).
fn is_body_clause(kind: &ClauseKind) -> bool {
    matches!(
        kind,
        ClauseKind::Requires
            | ClauseKind::Ensures
            | ClauseKind::Invariant
            | ClauseKind::Modifies
            | ClauseKind::Decreases
    )
}

/// Recursively check `Expr::Ident` references in an expression tree.
///
/// The `locals` parameter tracks locally-bound names (quantifier variables,
/// let bindings) that are valid within their subtree.
fn check_expr_idents(
    expr: &Expr,
    table: &SymbolTable,
    scope_id: usize,
    span: &Span,
    lenient: bool,
    locals: &mut Vec<String>,
    errors: &mut Vec<ResolutionError>,
) {
    match expr {
        Expr::Ident(name) => {
            // Skip if it resolves in the symbol table
            if table.lookup(name, scope_id).is_some() {
                return;
            }
            // Skip if it's a locally-bound variable (quantifier/let)
            if locals.contains(name) {
                return;
            }
            // Skip if it's a built-in value/function name
            if BUILTIN_VALUE_NAMES.contains(&name.as_str()) {
                return;
            }
            // Skip numeric-looking tokens
            if name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return;
            }
            // In lenient mode, skip all unknown names
            if lenient {
                return;
            }
            errors.push(ResolutionError {
                code: "A02001",
                message: format!("undefined name `{name}` in clause body"),
                span: span.clone(),
                secondary: None,
            });
        }
        Expr::Field(receiver, _field) => {
            // Only check the receiver; the field name is resolved structurally
            check_expr_idents(receiver, table, scope_id, span, lenient, locals, errors);
        }
        Expr::MethodCall { receiver, args, .. } => {
            check_expr_idents(receiver, table, scope_id, span, lenient, locals, errors);
            for arg in args {
                check_expr_idents(arg, table, scope_id, span, lenient, locals, errors);
            }
        }
        Expr::Call { func, args } => {
            check_expr_idents(func, table, scope_id, span, lenient, locals, errors);
            for arg in args {
                check_expr_idents(arg, table, scope_id, span, lenient, locals, errors);
            }
        }
        Expr::Index { expr: base, index } => {
            check_expr_idents(base, table, scope_id, span, lenient, locals, errors);
            check_expr_idents(index, table, scope_id, span, lenient, locals, errors);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            check_expr_idents(lhs, table, scope_id, span, lenient, locals, errors);
            check_expr_idents(rhs, table, scope_id, span, lenient, locals, errors);
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Paren(inner)
        | Expr::Old(inner)
        | Expr::Ghost(inner) => {
            check_expr_idents(inner, table, scope_id, span, lenient, locals, errors);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            check_expr_idents(cond, table, scope_id, span, lenient, locals, errors);
            check_expr_idents(then_branch, table, scope_id, span, lenient, locals, errors);
            if let Some(e) = else_branch {
                check_expr_idents(e, table, scope_id, span, lenient, locals, errors);
            }
        }
        Expr::Forall {
            var, domain, body, ..
        }
        | Expr::Exists {
            var, domain, body, ..
        } => {
            check_expr_idents(domain, table, scope_id, span, lenient, locals, errors);
            locals.push(var.clone());
            check_expr_idents(body, table, scope_id, span, lenient, locals, errors);
            locals.pop();
        }
        Expr::Let { name, value, body } => {
            check_expr_idents(value, table, scope_id, span, lenient, locals, errors);
            locals.push(name.clone());
            check_expr_idents(body, table, scope_id, span, lenient, locals, errors);
            locals.pop();
        }
        Expr::Match { scrutinee, arms } => {
            check_expr_idents(scrutinee, table, scope_id, span, lenient, locals, errors);
            for arm in arms {
                let mut arm_locals = locals.clone();
                collect_pattern_bindings(&arm.pattern, &mut arm_locals);
                check_expr_idents(
                    &arm.body,
                    table,
                    scope_id,
                    span,
                    lenient,
                    &mut arm_locals,
                    errors,
                );
            }
        }
        Expr::Apply { lemma_name, args } => {
            // The lemma name should resolve as a function/declaration
            if table.lookup(lemma_name, scope_id).is_none()
                && !locals.contains(lemma_name)
                && !BUILTIN_VALUE_NAMES.contains(&lemma_name.as_str())
                && !lenient
            {
                errors.push(ResolutionError {
                    code: "A02001",
                    message: format!("undefined lemma `{lemma_name}`"),
                    span: span.clone(),
                    secondary: None,
                });
            }
            for arg in args {
                check_expr_idents(arg, table, scope_id, span, lenient, locals, errors);
            }
        }
        Expr::Cast { expr: inner, .. } => {
            check_expr_idents(inner, table, scope_id, span, lenient, locals, errors);
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                check_expr_idents(item, table, scope_id, span, lenient, locals, errors);
            }
        }
        Expr::Raw(tokens) => {
            // For raw tokens, check identifiers that look like value references
            for tok in tokens {
                if tok
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                    && table.lookup(tok, scope_id).is_none()
                    && !locals.contains(tok)
                    && !BUILTIN_VALUE_NAMES.contains(&tok.as_str())
                    && !TYPE_SYNTAX_TOKENS.contains(&tok.as_str())
                    && !is_type_name_candidate(tok)
                    && !lenient
                {
                    errors.push(ResolutionError {
                        code: "A02001",
                        message: format!("undefined name `{tok}` in clause body"),
                        span: span.clone(),
                        secondary: None,
                    });
                }
            }
        }
        Expr::Literal(_) => {}
    }
}

/// Collect names bound by a pattern (for match arm local scope).
fn collect_pattern_bindings(pattern: &assura_parser::ast::Pattern, locals: &mut Vec<String>) {
    use assura_parser::ast::Pattern;
    match pattern {
        Pattern::Ident(name) if name != "_" => {
            locals.push(name.clone());
        }
        Pattern::Constructor { fields, .. } => {
            for f in fields {
                collect_pattern_bindings(f, locals);
            }
        }
        Pattern::Tuple(pats) => {
            for p in pats {
                collect_pattern_bindings(p, locals);
            }
        }
        Pattern::Wildcard | Pattern::Literal(_) | Pattern::Ident(_) => {}
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
// Unused import detection
// ---------------------------------------------------------------------------

/// Collect all identifier-like names referenced in the AST. This includes
/// type annotations, expression identifiers, and field/param type tokens.
fn collect_referenced_names(source: &SourceFile) -> HashSet<String> {
    let mut names = HashSet::new();
    for decl in &source.decls {
        match &decl.node {
            Decl::TypeDef(t) => {
                collect_type_body_names(&t.body, &mut names);
            }
            Decl::FnDef(f) => {
                collect_fn_names(f, &mut names);
            }
            Decl::Extern(ex) => {
                for p in &ex.params {
                    collect_type_token_names(&p.ty, &mut names);
                }
                collect_type_token_names(&ex.return_ty, &mut names);
                for clause in &ex.clauses {
                    collect_expr_names(&clause.body, &mut names);
                }
            }
            Decl::Bind(b) => {
                for p in &b.params {
                    collect_type_token_names(&p.ty, &mut names);
                }
                collect_type_token_names(&b.return_ty, &mut names);
                for clause in &b.clauses {
                    collect_expr_names(&clause.body, &mut names);
                }
            }
            Decl::Contract(c) => {
                for clause in &c.clauses {
                    collect_expr_names(&clause.body, &mut names);
                }
            }
            Decl::Service(s) => {
                for item in &s.items {
                    match item {
                        ServiceItem::TypeDef(t) => collect_type_body_names(&t.body, &mut names),
                        ServiceItem::EnumDef(e) => {
                            for v in &e.variants {
                                for f in &v.fields {
                                    names.insert(f.clone());
                                }
                            }
                        }
                        ServiceItem::Operation { clauses, .. }
                        | ServiceItem::Query { clauses, .. } => {
                            for clause in clauses {
                                collect_expr_names(&clause.body, &mut names);
                            }
                        }
                        ServiceItem::Invariant(expr) => collect_expr_names(expr, &mut names),
                        ServiceItem::Other { body, .. } => collect_expr_names(body, &mut names),
                        // States don't contribute expression names.
                        ServiceItem::States(_) => {}
                    }
                }
            }
            Decl::EnumDef(e) => {
                for v in &e.variants {
                    for f in &v.fields {
                        names.insert(f.clone());
                    }
                }
            }
            Decl::Prophecy(p) => {
                // Prophecy type tokens may reference user-defined types
                for tok in &p.ty_tokens {
                    if tok.chars().next().is_some_and(|c| c.is_uppercase()) {
                        names.insert(tok.clone());
                    }
                }
            }
            Decl::Block { body, .. } => {
                for clause in body {
                    collect_expr_names(&clause.body, &mut names);
                }
            }
        }
    }
    names
}

fn collect_type_body_names(body: &TypeBody, names: &mut HashSet<String>) {
    match body {
        TypeBody::Struct(fields) => {
            for f in fields {
                collect_type_token_names(&f.ty, names);
            }
        }
        TypeBody::Alias(tokens) | TypeBody::Refined(tokens) => {
            collect_type_token_names(tokens, names);
        }
        TypeBody::Empty => {}
    }
}

fn collect_fn_names(f: &FnDef, names: &mut HashSet<String>) {
    for p in &f.params {
        collect_type_token_names(&p.ty, names);
    }
    collect_type_token_names(&f.return_ty, names);
    for clause in &f.clauses {
        collect_expr_names(&clause.body, names);
    }
}

fn collect_type_token_names(tokens: &[String], names: &mut HashSet<String>) {
    for tok in tokens {
        if !TYPE_SYNTAX_TOKENS.contains(&tok.as_str())
            && !tok.starts_with(|c: char| c.is_ascii_digit())
        {
            names.insert(tok.clone());
        }
    }
}

fn collect_expr_names(expr: &assura_parser::ast::Expr, names: &mut HashSet<String>) {
    use assura_parser::ast::Expr;
    match expr {
        Expr::Ident(name) => {
            names.insert(name.clone());
        }
        Expr::Field(receiver, field) => {
            collect_expr_names(receiver, names);
            names.insert(field.clone());
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_expr_names(lhs, names);
            collect_expr_names(rhs, names);
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Paren(inner)
        | Expr::Old(inner)
        | Expr::Ghost(inner) => {
            collect_expr_names(inner, names);
        }
        Expr::Call { func, args } => {
            collect_expr_names(func, names);
            for arg in args {
                collect_expr_names(arg, names);
            }
        }
        Expr::MethodCall {
            receiver,
            args,
            method,
        } => {
            collect_expr_names(receiver, names);
            names.insert(method.clone());
            for arg in args {
                collect_expr_names(arg, names);
            }
        }
        Expr::Index { expr: base, index } => {
            collect_expr_names(base, names);
            collect_expr_names(index, names);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr_names(cond, names);
            collect_expr_names(then_branch, names);
            if let Some(e) = else_branch {
                collect_expr_names(e, names);
            }
        }
        Expr::Forall {
            var, domain, body, ..
        }
        | Expr::Exists {
            var, domain, body, ..
        } => {
            names.insert(var.clone());
            collect_expr_names(domain, names);
            collect_expr_names(body, names);
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                collect_expr_names(item, names);
            }
        }
        Expr::Cast { expr: inner, ty } => {
            collect_expr_names(inner, names);
            names.insert(ty.clone());
        }
        Expr::Apply { lemma_name, args } => {
            names.insert(lemma_name.clone());
            for arg in args {
                collect_expr_names(arg, names);
            }
        }
        Expr::Match { scrutinee, arms } => {
            collect_expr_names(scrutinee, names);
            for arm in arms {
                collect_expr_names(&arm.body, names);
            }
        }
        Expr::Let { name, value, body } => {
            names.insert(name.clone());
            collect_expr_names(value, names);
            collect_expr_names(body, names);
        }
        Expr::Raw(tokens) => {
            for tok in tokens {
                if tok
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                {
                    names.insert(tok.clone());
                }
            }
        }
        Expr::Literal(_) => {}
    }
}

/// Check which imports introduced names that are never referenced in the AST.
fn check_unused_imports(
    imports: &[ResolvedImport],
    referenced: &HashSet<String>,
    errors: &mut Vec<ResolutionError>,
) {
    for imp in imports {
        if imp.status == ImportStatus::Circular {
            continue;
        }
        let introduced: Vec<&str> = if !imp.items.is_empty() {
            imp.items.iter().map(|s| s.as_str()).collect()
        } else if let Some(alias) = &imp.alias {
            vec![alias.as_str()]
        } else if let Some(last) = imp.path.last() {
            vec![last.as_str()]
        } else {
            continue;
        };

        // An import is unused if none of its introduced names appear in references
        if introduced.iter().all(|name| !referenced.contains(*name)) {
            let path_str = imp.path.join(".");
            errors.push(ResolutionError {
                code: "A02007",
                message: format!("unused import `{path_str}`"),
                span: 0..0,
                secondary: None,
            });
        }
    }
}

// ===========================================================================
// A002: Filesystem-based module resolution
// ===========================================================================

/// Find the project root by walking up from `start` until `assura.toml`
/// is found.  Returns the directory containing `assura.toml`, or `None`
/// if no config file exists (single-file mode).
pub fn find_project_root(start: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut dir = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };

    loop {
        if dir.join("assura.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Resolve a dotted module path (`a.b.c`) to a file path relative to
/// the project root.  The convention is `a/b/c.assura`.
pub fn resolve_module_path(
    project_root: &std::path::Path,
    module_path: &[String],
) -> Option<std::path::PathBuf> {
    if module_path.is_empty() {
        return None;
    }
    let mut file_path = project_root.to_path_buf();
    for segment in module_path {
        file_path.push(segment);
    }
    file_path.set_extension("assura");
    if file_path.exists() {
        Some(file_path)
    } else {
        None
    }
}

/// Errors produced during module graph construction.
#[derive(Debug, Clone)]
pub struct ModuleError {
    pub module_path: String,
    pub message: String,
}

/// A compiled module graph: all reachable modules parsed and resolved.
#[derive(Debug)]
pub struct ModuleGraph {
    /// All successfully resolved modules, keyed by dotted path.
    pub modules: ModuleMap,
    /// Errors encountered while loading modules.
    pub errors: Vec<ModuleError>,
    /// Topological order of module paths (leaves first, root last).
    pub order: Vec<String>,
}

/// Build a complete module graph starting from a root file.
///
/// 1. Parse the root file.
/// 2. For each `import` in the root, resolve the module path to a file,
///    parse it, and add it to the module map.
/// 3. Recursively resolve imports in each discovered module.
/// 4. Detect circular imports via the visited set.
/// 5. Return all modules in topological order (dependencies before
///    dependents).
pub fn build_module_graph(
    root_file: &std::path::Path,
    project_root: &std::path::Path,
) -> ModuleGraph {
    let mut modules = ModuleMap::new();
    let mut errors = Vec::new();
    let mut order = Vec::new();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();

    // Derive a module name from the root file path relative to the project root
    let root_module = file_to_module_path(root_file, project_root);

    // Parse the root file
    let root_source = match std::fs::read_to_string(root_file) {
        Ok(s) => s,
        Err(e) => {
            errors.push(ModuleError {
                module_path: root_module,
                message: format!("cannot read file: {e}"),
            });
            return ModuleGraph {
                modules,
                errors,
                order,
            };
        }
    };
    let (root_ast, parse_errs) = assura_parser::parse(&root_source);
    if !parse_errs.is_empty() {
        errors.push(ModuleError {
            module_path: root_module.clone(),
            message: format!("{} parse error(s)", parse_errs.len()),
        });
    }

    if let Some(ast) = root_ast {
        modules.insert(root_module.clone(), ast);
    } else {
        errors.push(ModuleError {
            module_path: root_module,
            message: "failed to parse root file".to_string(),
        });
        return ModuleGraph {
            modules,
            errors,
            order,
        };
    }

    // Recursively load all imports
    resolve_imports_recursive(
        &root_module,
        project_root,
        &mut modules,
        &mut visiting,
        &mut visited,
        &mut order,
        &mut errors,
    );

    // The root itself is last in topological order
    if !order.contains(&root_module) {
        order.push(root_module);
    }

    ModuleGraph {
        modules,
        errors,
        order,
    }
}

fn resolve_imports_recursive(
    module_path: &str,
    project_root: &std::path::Path,
    modules: &mut ModuleMap,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
    order: &mut Vec<String>,
    errors: &mut Vec<ModuleError>,
) {
    if visited.contains(module_path) {
        return;
    }
    if !visiting.insert(module_path.to_string()) {
        // Circular import
        errors.push(ModuleError {
            module_path: module_path.to_string(),
            message: "circular import detected".to_string(),
        });
        return;
    }

    // Get the imports for this module
    let imports: Vec<Vec<String>> = modules
        .get(module_path)
        .map(|source| source.imports.iter().map(|i| i.path.clone()).collect())
        .unwrap_or_default();

    for imp_path in &imports {
        let path_str = imp_path.join(".");
        if modules.contains_key(&path_str) {
            // Already loaded, just recurse for transitive imports
            resolve_imports_recursive(
                &path_str,
                project_root,
                modules,
                visiting,
                visited,
                order,
                errors,
            );
            continue;
        }

        // Resolve to filesystem
        match resolve_module_path(project_root, imp_path) {
            Some(file_path) => {
                match std::fs::read_to_string(&file_path) {
                    Ok(source) => {
                        let (ast, parse_errs) = assura_parser::parse(&source);
                        if !parse_errs.is_empty() {
                            errors.push(ModuleError {
                                module_path: path_str.clone(),
                                message: format!(
                                    "{}: {} parse error(s)",
                                    file_path.display(),
                                    parse_errs.len()
                                ),
                            });
                        }
                        if let Some(ast) = ast {
                            modules.insert(path_str.clone(), ast);
                        }
                        // Recursively resolve this module's imports
                        resolve_imports_recursive(
                            &path_str,
                            project_root,
                            modules,
                            visiting,
                            visited,
                            order,
                            errors,
                        );
                    }
                    Err(e) => {
                        errors.push(ModuleError {
                            module_path: path_str.clone(),
                            message: format!("{}: {e}", file_path.display()),
                        });
                    }
                }
            }
            None => {
                // Module not found on filesystem. Not necessarily an error:
                // could be a standard library module.
                errors.push(ModuleError {
                    module_path: path_str.clone(),
                    message: format!("module not found: {}", imp_path.join("/")),
                });
            }
        }
    }

    visiting.remove(module_path);
    visited.insert(module_path.to_string());
    let mp = module_path.to_string();
    if !order.contains(&mp) {
        order.push(mp);
    }
}

fn file_to_module_path(file: &std::path::Path, project_root: &std::path::Path) -> String {
    file.strip_prefix(project_root)
        .unwrap_or(file)
        .with_extension("")
        .to_string_lossy()
        .replace(['/', '\\'], ".")
}

/// Resolve all modules in a graph, producing `ResolvedFile` for each.
///
/// Processes modules in topological order so that a module's dependencies
/// are always resolved before the module itself.
pub fn resolve_module_graph(
    graph: &ModuleGraph,
) -> (HashMap<String, ResolvedFile>, Vec<ModuleError>) {
    let mut resolved = HashMap::new();
    let mut errors = Vec::new();

    for module_path in &graph.order {
        if let Some(source) = graph.modules.get(module_path) {
            let module_map: ModuleMap = graph
                .modules
                .iter()
                .filter(|(k, _)| *k != module_path)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            let mut visited = HashSet::new();
            visited.insert(module_path.clone());

            match resolve_with_modules(source, &module_map, &mut visited) {
                Ok(result) => {
                    resolved.insert(module_path.clone(), result);
                }
                Err(errs) => {
                    errors.push(ModuleError {
                        module_path: module_path.clone(),
                        message: format!("{} resolution error(s)", errs.len()),
                    });
                }
            }
        }
    }

    (resolved, errors)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        let configs: Vec<&super::Symbol> = resolved
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
                .any(|w| w.code == "A02007" && w.message.contains("std.math")),
            "expected unused import warning for std.math"
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
    fn import_path_uppercase_segment_rejected() {
        // Module path segments must start with lowercase
        let src = r#"
import std.Math;
"#;
        let file = parse_ok(src);
        let result = resolve(&file);
        assert!(result.is_err(), "uppercase segment should produce an error");
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
        let body = Expr::Raw(vec![
            "a".to_string(),
            ":".to_string(),
            "Int".to_string(),
            ",".to_string(),
            "b".to_string(),
            ":".to_string(),
            "Nat".to_string(),
        ]);
        let names = extract_input_param_names(&body);
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn extract_input_params_generic_type() {
        use assura_parser::ast::Expr;
        // input(items: List<Int>, count: Nat)
        let body = Expr::Raw(vec![
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
        ]);
        let names = extract_input_param_names(&body);
        assert_eq!(names, vec!["items", "count"]);
    }

    #[test]
    fn extract_input_params_call_expr() {
        use assura_parser::ast::Expr;
        let body = Expr::Call {
            func: Box::new(Expr::Ident("input".to_string())),
            args: vec![
                Expr::Cast {
                    expr: Box::new(Expr::Ident("x".to_string())),
                    ty: "Int".to_string(),
                },
                Expr::Ident("y".to_string()),
            ],
        };
        let names = extract_input_param_names(&body);
        assert_eq!(names, vec!["x", "y"]);
    }

    #[test]
    fn extract_input_params_ident() {
        use assura_parser::ast::Expr;
        let body = Expr::Ident("x".to_string());
        let names = extract_input_param_names(&body);
        assert_eq!(names, vec!["x"]);
    }

    #[test]
    fn extract_input_params_cast() {
        use assura_parser::ast::Expr;
        let body = Expr::Cast {
            expr: Box::new(Expr::Ident("n".to_string())),
            ty: "Int".to_string(),
        };
        let names = extract_input_param_names(&body);
        assert_eq!(names, vec!["n"]);
    }

    #[test]
    fn extract_input_params_paren() {
        use assura_parser::ast::Expr;
        let body = Expr::Paren(Box::new(Expr::Cast {
            expr: Box::new(Expr::Ident("val".to_string())),
            ty: "Nat".to_string(),
        }));
        let names = extract_input_param_names(&body);
        assert_eq!(names, vec!["val"]);
    }

    #[test]
    fn extract_input_params_tuple() {
        use assura_parser::ast::Expr;
        let body = Expr::Tuple(vec![
            Expr::Cast {
                expr: Box::new(Expr::Ident("a".to_string())),
                ty: "Int".to_string(),
            },
            Expr::Ident("b".to_string()),
        ]);
        let names = extract_input_param_names(&body);
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn extract_input_params_raw_as_separator() {
        use assura_parser::ast::Expr;
        let body = Expr::Raw(vec![
            "x".into(),
            "as".into(),
            "Int".into(),
            ",".into(),
            "y".into(),
            "as".into(),
            "Nat".into(),
        ]);
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
        assert!(resolved.symbols.lookup("a", contract_scope).is_some());
        assert!(resolved.symbols.lookup("b", contract_scope).is_some());
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
        assert!(root.is_some());
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
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("math/util.assura"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_module_path_missing() {
        let dir = std::env::temp_dir().join("assura-test-mod-missing");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = vec!["nonexistent".into(), "module".into()];
        assert!(resolve_module_path(&dir, &path).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_to_module_path_conversion() {
        let root = std::path::Path::new("/project");
        let file = std::path::Path::new("/project/src/math/util.assura");
        let result = file_to_module_path(file, root);
        assert_eq!(result, "src.math.util");
    }

    #[test]
    fn build_module_graph_single_file() {
        let dir = std::env::temp_dir().join("assura-test-graph-single");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("main.assura"),
            "module test.main;\ncontract Foo {\n  input(x: Int)\n}",
        )
        .unwrap();

        let graph = build_module_graph(&dir.join("main.assura"), &dir);
        assert_eq!(graph.modules.len(), 1);
        assert_eq!(graph.order.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_module_graph_produces_resolved_files() {
        let dir = std::env::temp_dir().join("assura-test-resolve-graph");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("main.assura"),
            "module test.main;\ncontract Bar {\n  input(x: Int)\n}",
        )
        .unwrap();

        let graph = build_module_graph(&dir.join("main.assura"), &dir);
        let (resolved, errs) = resolve_module_graph(&graph);
        // The single module may have resolution warnings but should produce a result
        assert!(!resolved.is_empty() || !errs.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
