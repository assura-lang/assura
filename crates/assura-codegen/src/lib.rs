//! Rust code generation from type-checked Assura contracts.
//!
//! Takes a `TypedFile` from `assura-types` and generates a Rust project
//! consisting of a `Cargo.toml` and one or more `.rs` source files.
//! Generated Rust is formatted via `prettyplease`.
//!
//! This is T019: the initial scaffolding. Type mapping (T020), contract
//! codegen (T021), project generation (T022), and struct/enum codegen
//! (T023) extend this foundation.

/// Assura-to-Rust type mapping (Int -> i64, Nat -> u64, etc.).
pub mod type_map;

mod block;
mod contract;
mod decl;
mod expr;
/// Feature-specific code generation for all 50 verification features.
pub mod features;
mod service;
mod types_gen;

pub use types_gen::expr_to_rust_static;

use block::*;
use contract::*;
use decl::*;
use expr::*;
use service::*;
use types_gen::*;

use assura_ast::{
    BinOp, BindDecl, BlockKind, Clause, ClauseKind, CodecRegistryDecl, ContractDecl, Decl,
    DeclVisitor, EnumDef, Expr, ExternDecl, FnDef, Literal, MagicPattern, ServiceDecl, ServiceItem,
    SpExpr, Spanned, TypeBody, TypeDef, UnaryOp,
};
use assura_types::TypedFile;

/// Convert an `Option<TypeExpr>` to a `Vec<String>` of type tokens.
///
/// Bridge helper used during codegen to pass structured types to functions
/// that still work on raw token slices (e.g., `map_type_tokens`, `collect_type_refs_from_tokens`).
fn type_expr_to_token_vec(te: Option<&assura_ast::TypeExpr>) -> Vec<String> {
    te.map(|t| t.to_tokens()).unwrap_or_default()
}

#[cfg(test)]
#[path = "codegen_tests.rs"]
mod tests;

// ---------------------------------------------------------------------------
// Public output types
// ---------------------------------------------------------------------------

/// The result of code generation: a complete Rust project.
#[derive(Debug, Clone)]
pub struct GeneratedProject {
    /// Content of the generated `Cargo.toml`.
    pub cargo_toml: String,
    /// Generated source files as `(relative_path, rust_source)` pairs.
    /// Typically `[("src/lib.rs", "...")]`.
    pub files: Vec<(String, String)>,
}

// ---------------------------------------------------------------------------
// T119: Backend selection (Rustc vs Cranelift for fast dev builds)
// ---------------------------------------------------------------------------

/// Code generation backend.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum CodegenBackend {
    /// Standard rustc backend (optimized, production).
    #[default]
    Rustc,
    /// Cranelift backend (fast compilation, dev builds).
    Cranelift,
}

/// Compilation target for the generated Rust project.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum CompileTarget {
    /// Native host target (default).
    #[default]
    Native,
    /// WebAssembly via wasm32-wasip1 (formerly wasm32-wasi).
    Wasm,
}

impl CompileTarget {
    /// Parse a target string from CLI/config (e.g. "native", "wasm32-wasi").
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "native" => Some(Self::Native),
            "wasm" | "wasm32-wasi" | "wasm32-wasip1" => Some(Self::Wasm),
            _ => None,
        }
    }

    /// The Rust target triple for cargo commands.
    pub fn rust_target(&self) -> Option<&'static str> {
        match self {
            Self::Native => None, // use host default
            Self::Wasm => Some("wasm32-wasip1"),
        }
    }
}

/// Configuration for code generation.
#[derive(Debug, Clone)]
pub struct BackendConfig {
    pub backend: CodegenBackend,
    pub opt_level: u8,
    pub debug_info: bool,
    pub target: CompileTarget,
    /// Optional IR-generated Rust function bodies, keyed by contract/function name.
    ///
    /// When present, codegen replaces `todo!()` placeholders with real
    /// implementations from IR sidecar files.
    pub ir_bodies: std::collections::HashMap<String, String>,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            backend: CodegenBackend::Rustc,
            opt_level: 2,
            debug_info: false,
            target: CompileTarget::Native,
            ir_bodies: std::collections::HashMap::new(),
        }
    }
}

impl BackendConfig {
    /// Create a default config with IR bodies from loaded sidecars.
    pub fn with_ir_bodies(ir_bodies: std::collections::HashMap<String, String>) -> Self {
        Self {
            ir_bodies,
            ..Self::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Phase helpers (extracted from codegen_with_config)
// ---------------------------------------------------------------------------

/// Visitor that collects user-defined type names, feature_max constants,
/// and referenced types from all declarations.
struct TypeCollectVisitor<'a> {
    defined_types: &'a mut std::collections::HashSet<String>,
    feature_max_consts: &'a mut Vec<(String, String)>,
    referenced_types: &'a mut std::collections::HashSet<String>,
}

impl DeclVisitor for TypeCollectVisitor<'_> {
    fn visit_type_def(&mut self, t: &TypeDef) {
        self.defined_types.insert(t.name.clone());
        if let TypeBody::Struct(fields) = &t.body {
            for f in fields {
                collect_type_refs_from_tokens(
                    &type_expr_to_token_vec(f.ty.as_ref()),
                    self.referenced_types,
                );
            }
        }
    }
    fn visit_enum_def(&mut self, e: &EnumDef) {
        self.defined_types.insert(e.name.clone());
    }
    fn visit_block(
        &mut self,
        kind: &BlockKind,
        name: &str,
        value: &Option<Vec<String>>,
        _body: &[Clause],
    ) {
        if *kind == BlockKind::FeatureMax {
            // Extract type from inline value tokens (e.g., ["Nat", "=", "280"] -> "Nat")
            let ty = value
                .as_ref()
                .and_then(|v| {
                    v.iter()
                        .take_while(|t| t.as_str() != "=")
                        .find(|t| t.chars().next().is_some_and(|c| c.is_uppercase()))
                })
                .map(|t| map_type_token(t).to_string())
                .unwrap_or_else(|| "u64".to_string());
            self.feature_max_consts.push((name.to_string(), ty));
        }
    }
    fn visit_fn_def(&mut self, f: &FnDef) {
        collect_type_refs_from_tokens(
            &type_expr_to_token_vec(f.return_ty.as_ref()),
            self.referenced_types,
        );
        for p in &f.params {
            collect_type_refs_from_tokens(
                &type_expr_to_token_vec(p.ty.as_ref()),
                self.referenced_types,
            );
        }
    }
    fn visit_extern(&mut self, ex: &ExternDecl) {
        collect_type_refs_from_tokens(
            &type_expr_to_token_vec(ex.return_ty.as_ref()),
            self.referenced_types,
        );
        for p in &ex.params {
            collect_type_refs_from_tokens(
                &type_expr_to_token_vec(p.ty.as_ref()),
                self.referenced_types,
            );
        }
    }
    fn visit_contract(&mut self, c: &ContractDecl) {
        for clause in &c.clauses {
            collect_type_refs_from_expr(&clause.body, self.referenced_types);
        }
    }
    fn visit_service(&mut self, s: &ServiceDecl) {
        for item in &s.items {
            match item {
                ServiceItem::TypeDef(t) => {
                    self.defined_types.insert(t.name.clone());
                }
                ServiceItem::EnumDef(e) => {
                    self.defined_types.insert(e.name.clone());
                }
                ServiceItem::Operation { clauses, .. } | ServiceItem::Query { clauses, .. } => {
                    for clause in clauses {
                        collect_type_refs_from_expr(&clause.body, self.referenced_types);
                    }
                }
                ServiceItem::States(_) | ServiceItem::Invariant(_) | ServiceItem::Other { .. } => {}
            }
        }
    }
    fn visit_bind(&mut self, b: &BindDecl) {
        collect_type_refs_from_tokens(
            &type_expr_to_token_vec(b.return_ty.as_ref()),
            self.referenced_types,
        );
        for p in &b.params {
            collect_type_refs_from_tokens(
                &type_expr_to_token_vec(p.ty.as_ref()),
                self.referenced_types,
            );
        }
    }
    // Prophecy / CodecRegistry: default no-op
}

/// Phase 1+2: Walk decls to collect defined type names, feature_max constants,
/// and referenced types.
///
/// Returns `(defined_types, feature_max_consts, referenced_types)`.
fn collect_type_names(
    decls: &[Spanned<Decl>],
) -> (
    std::collections::HashSet<String>,
    Vec<(String, String)>,
    std::collections::HashSet<String>,
) {
    let mut defined_types = std::collections::HashSet::new();
    let mut feature_max_consts: Vec<(String, String)> = Vec::new();
    let mut referenced_types = std::collections::HashSet::new();

    let mut visitor = TypeCollectVisitor {
        defined_types: &mut defined_types,
        feature_max_consts: &mut feature_max_consts,
        referenced_types: &mut referenced_types,
    };
    assura_ast::walk_decls(&mut visitor, decls);

    // Add built-in type names that should never generate stubs
    for builtin in &[
        "Int", "Nat", "Float", "Bool", "String", "Bytes", "Unit", "Never", "U8", "U16", "U32",
        "U64", "I8", "I16", "I32", "I64", "F32", "F64", "List", "Vec", "Map", "Set", "Option",
        "Result", "Sequence", "i64", "u64", "f64", "bool", "u8", "u16", "u32", "i8", "i16", "i32",
        "f32", "f64",
    ] {
        defined_types.insert(builtin.to_string());
    }

    (defined_types, feature_max_consts, referenced_types)
}

/// Visitor that collects type token lists from FnDef/Extern/Bind params and
/// return types, and optionally from TypeDef struct fields.
struct TypeTokenCollectVisitor<'a> {
    token_lists: &'a mut Vec<Vec<String>>,
    include_typedef_fields: bool,
}

impl DeclVisitor for TypeTokenCollectVisitor<'_> {
    fn visit_fn_def(&mut self, f: &FnDef) {
        self.token_lists
            .push(type_expr_to_token_vec(f.return_ty.as_ref()));
        for p in &f.params {
            self.token_lists.push(type_expr_to_token_vec(p.ty.as_ref()));
        }
    }
    fn visit_extern(&mut self, ex: &ExternDecl) {
        self.token_lists
            .push(type_expr_to_token_vec(ex.return_ty.as_ref()));
        for p in &ex.params {
            self.token_lists.push(type_expr_to_token_vec(p.ty.as_ref()));
        }
    }
    fn visit_bind(&mut self, b: &BindDecl) {
        self.token_lists
            .push(type_expr_to_token_vec(b.return_ty.as_ref()));
        for p in &b.params {
            self.token_lists.push(type_expr_to_token_vec(p.ty.as_ref()));
        }
    }
    fn visit_type_def(&mut self, t: &TypeDef) {
        if self.include_typedef_fields
            && let TypeBody::Struct(fields) = &t.body
        {
            for f in fields {
                self.token_lists.push(type_expr_to_token_vec(f.ty.as_ref()));
            }
        }
    }
    // Contract, Service, EnumDef, Prophecy, CodecRegistry, Block: default no-op
}

/// Collect all type token lists from FnDef/Extern/Bind params and return types.
///
/// Phases 3, 4, and 4b all need this same walk. The `include_typedef_fields`
/// flag controls whether TypeDef struct fields are included (Phase 4 needs them;
/// Phases 3 and 4b do not).
fn collect_all_type_token_lists(
    decls: &[Spanned<Decl>],
    include_typedef_fields: bool,
) -> Vec<Vec<String>> {
    let mut token_lists: Vec<Vec<String>> = Vec::new();
    let mut visitor = TypeTokenCollectVisitor {
        token_lists: &mut token_lists,
        include_typedef_fields,
    };
    assura_ast::walk_decls(&mut visitor, decls);
    token_lists
}

/// Phase 3: Detect feature_max names used as type arguments inside `<>`.
fn detect_feature_max_as_type(
    token_lists: &[Vec<String>],
    feature_max_consts: &[(String, String)],
) -> std::collections::HashSet<String> {
    let fm_set: std::collections::HashSet<&str> =
        feature_max_consts.iter().map(|(n, _)| n.as_str()).collect();
    let mut result = std::collections::HashSet::new();
    for tokens in token_lists {
        let mut in_angle = 0i32;
        for tok in tokens {
            match tok.as_str() {
                "<" => in_angle += 1,
                ">" if in_angle > 0 => in_angle -= 1,
                name if in_angle > 0 && fm_set.contains(name) => {
                    result.insert(name.to_string());
                }
                _ => {}
            }
        }
    }
    result
}

/// Phase 4: Detect generic arity for type references and collect const-generic names.
///
/// Returns `(type_generic_params, const_generic_names)`.
fn detect_generic_arities(
    token_lists: &[Vec<String>],
) -> (
    std::collections::HashMap<String, Vec<GenericParamKind>>,
    std::collections::HashSet<String>,
) {
    let mut type_generic_params: std::collections::HashMap<String, Vec<GenericParamKind>> =
        std::collections::HashMap::new();
    let mut const_generic_names = std::collections::HashSet::new();
    for tokens in token_lists {
        detect_generic_arity(tokens, &mut type_generic_params, &mut const_generic_names);
    }
    (type_generic_params, const_generic_names)
}

/// Phase 4b: Collect SCREAMING_SNAKE_CASE names used as type arguments inside `<>`,
/// combining const_generic_names from Phase 4 with feature_max names found in type positions.
fn detect_const_type_stubs(
    token_lists: &[Vec<String>],
    const_generic_names: &std::collections::HashSet<String>,
    feature_max_consts: &[(String, String)],
    defined_types: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut all_const_as_types = const_generic_names.clone();
    let feature_max_set: std::collections::HashSet<String> =
        feature_max_consts.iter().map(|(n, _)| n.clone()).collect();
    for tokens in token_lists {
        let mut in_angle = 0i32;
        for tok in tokens {
            match tok.as_str() {
                "<" => in_angle += 1,
                ">" if in_angle > 0 => in_angle -= 1,
                name if in_angle > 0 && feature_max_set.contains(name) => {
                    all_const_as_types.insert(name.to_string());
                }
                _ => {}
            }
        }
    }
    let mut const_as_types: Vec<String> = all_const_as_types
        .iter()
        .filter(|n| !defined_types.contains(*n))
        .cloned()
        .collect();
    const_as_types.sort();
    const_as_types
}

/// Phase 5: Generate stub structs for undefined types.
fn generate_undefined_type_stubs(
    referenced_types: &std::collections::HashSet<String>,
    defined_types: &std::collections::HashSet<String>,
    feature_max_consts: &[(String, String)],
    const_as_types: &[String],
    type_generic_params: &std::collections::HashMap<String, Vec<GenericParamKind>>,
    code: &mut String,
) {
    let feature_max_set: std::collections::HashSet<String> =
        feature_max_consts.iter().map(|(n, _)| n.clone()).collect();
    let mut undefined: Vec<String> = referenced_types
        .difference(defined_types)
        .filter(|t| {
            // Skip things that aren't type names (operators, keywords, etc.)
            !t.is_empty()
                && t.chars().next().is_some_and(|c| c.is_uppercase())
                && t.chars().all(|c| c.is_alphanumeric() || c == '_')
                // Skip feature_max constants (already generated as const + type stub)
                && !feature_max_consts.iter().any(|(n, _)| n == *t)
                // Skip SCREAMING_SNAKE_CASE names already handled as const-as-type stubs
                && !const_as_types.iter().any(|s| s == *t)
                // Also skip feature_max names that are in const_as_types
                && !feature_max_set.contains(*t)
        })
        .cloned()
        .collect();
    undefined.sort();
    if !undefined.is_empty() {
        code.push_str("// Placeholder types for types used but not defined in this file.\n");
        code.push_str("// Replace with real definitions when implementations are provided.\n");
        for name in &undefined {
            let arity = type_generic_params.get(name).map_or(0, |v| v.len());
            if arity > 0 {
                let params: Vec<String> = (0..arity).map(|i| format!("T{i}")).collect();
                let phantoms: Vec<String> = params
                    .iter()
                    .map(|p| format!("std::marker::PhantomData<{p}>"))
                    .collect();
                code.push_str(&format!(
                    "#[derive(Debug, Clone, PartialEq)]\npub struct {name}<{}>({});\n",
                    params.join(", "),
                    phantoms.join(", ")
                ));
            } else {
                code.push_str(&format!(
                    "#[derive(Debug, Clone, PartialEq)]\npub struct {name};\n"
                ));
            }
        }
        code.push('\n');
    }
}

/// Visitor that collects contract and service names for module structure decisions.
struct CollectModuleNames {
    contract_names: Vec<String>,
    service_names: Vec<String>,
}

impl DeclVisitor for CollectModuleNames {
    fn visit_contract(&mut self, c: &ContractDecl) {
        self.contract_names.push(c.name.clone());
    }
    fn visit_service(&mut self, s: &ServiceDecl) {
        self.service_names.push(s.name.clone());
    }
}

/// Visitor that generates Rust code for each declaration.
///
/// In multi-file mode (`include_contracts_services = false`), contracts and
/// services are skipped (they get their own files). In single-file mode
/// (`include_contracts_services = true`), they are generated inline.
struct CodeGenVisitor<'a> {
    code: &'a mut String,
    include_contracts_services: bool,
    ir_bodies: Option<&'a std::collections::HashMap<String, String>>,
}

impl DeclVisitor for CodeGenVisitor<'_> {
    fn visit_type_def(&mut self, t: &TypeDef) {
        generate_type_def(t, self.code);
    }
    fn visit_enum_def(&mut self, e: &EnumDef) {
        generate_enum_def(e, self.code);
    }
    fn visit_extern(&mut self, ex: &ExternDecl) {
        generate_extern(ex, self.code);
    }
    fn visit_bind(&mut self, b: &BindDecl) {
        generate_bind(b, self.code);
    }
    fn visit_fn_def(&mut self, f: &FnDef) {
        if !f.is_ghost && !f.is_lemma {
            generate_fn_def(f, self.code, self.ir_bodies);
        }
    }
    fn visit_block(
        &mut self,
        kind: &BlockKind,
        name: &str,
        _value: &Option<Vec<String>>,
        body: &[Clause],
    ) {
        if *kind != BlockKind::FeatureMax {
            generate_block(kind, name, body, self.code);
        }
    }
    fn visit_codec_registry(&mut self, cr: &CodecRegistryDecl) {
        generate_codec_registry(cr, self.code);
    }
    fn visit_contract(&mut self, c: &ContractDecl) {
        if self.include_contracts_services {
            generate_contract(c, self.code, self.ir_bodies);
        }
    }
    fn visit_service(&mut self, s: &ServiceDecl) {
        if self.include_contracts_services {
            generate_service(s, self.code, self.ir_bodies);
        }
    }
    // Prophecy: default no-op (ghost, erased in codegen)
}

/// Visitor that emits `pub mod` declarations for contracts and services
/// in multi-file mode.
struct ModDeclVisitor<'a> {
    code: &'a mut String,
}

impl DeclVisitor for ModDeclVisitor<'_> {
    fn visit_contract(&mut self, c: &ContractDecl) {
        let mod_name = c.name.to_lowercase();
        self.code
            .push_str(&format!("pub mod contract_{mod_name};\n"));
    }
    fn visit_service(&mut self, s: &ServiceDecl) {
        let mod_name = s.name.to_lowercase();
        self.code.push_str(&format!("pub mod {mod_name};\n"));
    }
    // All others: default no-op
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Generate a Rust project from a type-checked Assura file.
///
/// Walks the AST, maps Assura declarations to Rust source code, and
/// formats the result with `prettyplease`. Returns a `GeneratedProject`
/// with a `Cargo.toml` and generated `.rs` files.
pub fn codegen_with_config(typed: &TypedFile, config: &BackendConfig) -> GeneratedProject {
    let source = &typed.resolved.source;

    let project_name = source
        .project
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "generated".to_string());

    let crate_name: String = project_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();

    let has_proptest = source_has_testable_contracts(source);
    let has_errors = source_has_error_types(source);
    let cargo_toml = generate_cargo_toml_impl(&crate_name, config, has_proptest, has_errors);

    let mut code = String::new();

    // Phase 1+2: Collect defined types, feature_max consts, and referenced types.
    let (defined_types, feature_max_consts, referenced_types) = collect_type_names(&source.decls);

    // Collect type token lists once for Phases 3 and 4b (FnDef/Extern/Bind only).
    let fn_token_lists = collect_all_type_token_lists(&source.decls, false);
    // Phase 4 also needs TypeDef struct fields.
    let all_token_lists = collect_all_type_token_lists(&source.decls, true);

    // Phase 3: Detect feature_max names used as type arguments inside `<>`.
    let feature_max_used_as_type = detect_feature_max_as_type(&fn_token_lists, &feature_max_consts);
    for (name, ty) in &feature_max_consts {
        if feature_max_used_as_type.contains(name) {
            // Will be emitted as a struct stub in Phase 4b.
            // Also emit a const with _VALUE suffix for any value-position uses.
            let value = find_feature_max_value(source, name);
            code.push_str(&format!("pub const {name}_VALUE: {ty} = {value};\n"));
        } else {
            let value = find_feature_max_value(source, name);
            code.push_str(&format!("pub const {name}: {ty} = {value};\n"));
        }
    }
    if !feature_max_consts.is_empty() {
        code.push('\n');
    }

    // Phase 4: Detect generic arity for type references and collect const-generic names.
    let (type_generic_params, const_generic_names) = detect_generic_arities(&all_token_lists);

    // Phase 4b: Generate marker type stubs for SCREAMING_SNAKE_CASE names
    // used as generic arguments.
    let const_as_types = detect_const_type_stubs(
        &fn_token_lists,
        &const_generic_names,
        &feature_max_consts,
        &defined_types,
    );
    for name in &const_as_types {
        // Emit as a marker type (unit struct) rather than a const,
        // since the generic parameter positions use type params.
        code.push_str(&format!(
            "#[derive(Debug, Clone, PartialEq)]\npub struct {name}; // size param from another module\n"
        ));
    }
    if !const_as_types.is_empty() {
        code.push('\n');
    }

    // Phase 5: Generate stub structs for undefined types.
    generate_undefined_type_stubs(
        &referenced_types,
        &defined_types,
        &feature_max_consts,
        &const_as_types,
        &type_generic_params,
        &mut code,
    );

    // Count contracts and services to decide on module structure.
    let mut collector = CollectModuleNames {
        contract_names: Vec::new(),
        service_names: Vec::new(),
    };
    assura_ast::walk_decls(&mut collector, &source.decls);
    let contract_names = collector.contract_names;
    let service_names = collector.service_names;
    let total_modules = contract_names.len() + service_names.len();
    let use_multi_file = total_modules >= 2;

    let mut project = if use_multi_file {
        // ------------------------------------------------------------------
        // Multi-file mode: separate .rs files for each contract/service,
        // shared types/functions/externs in lib.rs.
        // ------------------------------------------------------------------
        let mut files: Vec<(String, String)> = Vec::new();

        // Build shared code: types, enums, externs, functions, blocks
        let mut shared = String::new();
        shared.push_str("#![allow(dead_code, unused_variables, unused_parens, non_camel_case_types, unreachable_code)]\n\n");
        // Emit the pre-built preamble (feature_max, const-as-type stubs,
        // placeholder structs)
        shared.push_str(&code);

        // Generate shared code (types, enums, externs, functions, blocks).
        // Contracts and services go into their own files in multi-file mode.
        let ir_ref = if config.ir_bodies.is_empty() {
            None
        } else {
            Some(&config.ir_bodies)
        };
        let mut codegen_visitor = CodeGenVisitor {
            code: &mut shared,
            include_contracts_services: false,
            ir_bodies: ir_ref,
        };
        assura_ast::walk_decls(&mut codegen_visitor, &source.decls);

        // Add pub mod declarations for each contract/service module
        let mut mod_visitor = ModDeclVisitor { code: &mut shared };
        assura_ast::walk_decls(&mut mod_visitor, &source.decls);

        let formatted_shared = format_rust(&shared);
        let lib_rs = format!(
            "// Generated by the Assura compiler.\n// Do not edit manually.\n\n{formatted_shared}"
        );
        files.push(("src/lib.rs".to_string(), lib_rs));

        // Generate per-contract files
        for decl in &source.decls {
            if let Decl::Contract(c) = &decl.node {
                let mod_name = c.name.to_lowercase();
                let mut mod_code = String::new();
                mod_code.push_str("#![allow(dead_code, unused_variables, unused_parens, non_camel_case_types, unreachable_code)]\n\n");
                mod_code.push_str("use super::*;\n\n");
                // Generate the contract body without wrapping it in
                // `pub mod contract_xxx { ... }` since it IS the module file.
                generate_contract_contents(c, &mut mod_code, ir_ref);
                // S009: proptest for multi-file contracts
                generate_proptest_for_contract_contents(c, &mut mod_code);
                let formatted = format_rust(&mod_code);
                let content = format!(
                    "// Generated by the Assura compiler.\n// Do not edit manually.\n\n{formatted}"
                );
                files.push((format!("src/contract_{mod_name}.rs"), content));
            }
        }

        // Generate per-service files
        for decl in &source.decls {
            if let Decl::Service(s) = &decl.node {
                let mod_name = s.name.to_lowercase();
                let mut mod_code = String::new();
                mod_code.push_str("#![allow(dead_code, unused_variables, unused_parens, non_camel_case_types, unreachable_code)]\n\n");
                mod_code.push_str("use super::*;\n\n");
                generate_service_contents(s, &mut mod_code, ir_ref);
                let formatted = format_rust(&mod_code);
                let content = format!(
                    "// Generated by the Assura compiler.\n// Do not edit manually.\n\n{formatted}"
                );
                files.push((format!("src/{mod_name}.rs"), content));
            }
        }

        GeneratedProject {
            cargo_toml: cargo_toml.clone(),
            files,
        }
    } else {
        // ------------------------------------------------------------------
        // Single-file mode: everything in lib.rs (current behavior).
        // ------------------------------------------------------------------
        let mut all_code = String::new();
        all_code.push_str("#![allow(dead_code, unused_variables, unused_parens, non_camel_case_types, unreachable_code)]\n\n");
        all_code.push_str(&code);

        // Generate code for all declarations (single-file: contracts and services inline).
        let ir_ref = if config.ir_bodies.is_empty() {
            None
        } else {
            Some(&config.ir_bodies)
        };
        let mut codegen_visitor = CodeGenVisitor {
            code: &mut all_code,
            include_contracts_services: true,
            ir_bodies: ir_ref,
        };
        assura_ast::walk_decls(&mut codegen_visitor, &source.decls);

        // S009: Generate proptest tests for testable contracts
        // Skip proptest for Cranelift (dev builds prioritize fast compilation)
        if !matches!(config.backend, CodegenBackend::Cranelift) {
            for decl in &source.decls {
                if let Decl::Contract(c) = &decl.node {
                    generate_proptest_for_contract(c, &mut all_code);
                }
            }
        }

        let formatted_body = format_rust(&all_code);
        let formatted = format!(
            "// Generated by the Assura compiler.\n// Do not edit manually.\n\n{formatted_body}"
        );

        GeneratedProject {
            cargo_toml,
            files: vec![("src/lib.rs".to_string(), formatted)],
        }
    };

    // Add .cargo/config.toml for Cranelift backend
    if matches!(config.backend, CodegenBackend::Cranelift) {
        project.files.push((
            ".cargo/config.toml".to_string(),
            "[unstable]\ncodegen-backend = true\n\n[profile.dev]\ncodegen-backend = \"cranelift\"\n"
                .to_string(),
        ));

        // Transform generated Rust code for C ABI compatibility:
        // - Add #[repr(C)] to structs/enums (deterministic layout for JIT)
        // - Add #[no_mangle] extern "C" to public functions (callable via JIT)
        for (path, content) in &mut project.files {
            if path.ends_with(".rs") {
                *content = cranelift_transform(content);
            }
        }
    }

    project
}

/// Transform generated Rust code for Cranelift JIT compatibility.
///
/// Cranelift is used for fast dev-cycle builds. To make generated functions
/// callable from JIT-compiled code, we:
/// - Add `#[repr(C)]` before `pub struct` and `pub enum` (C ABI layout)
/// - Replace `pub fn` with `#[no_mangle] pub extern "C" fn` (C ABI calling convention)
///
/// This does NOT affect `fn` (private functions) or impl blocks.
fn cranelift_transform(code: &str) -> String {
    let mut out = String::with_capacity(code.len() + 512);
    for line in code.lines() {
        let trimmed = line.trim_start();
        if (trimmed.starts_with("pub struct ") || trimmed.starts_with("pub enum "))
            && !trimmed.contains("pub struct PhantomData")
        {
            // Insert #[repr(C)] with matching indentation
            let indent = &line[..line.len() - trimmed.len()];
            out.push_str(indent);
            out.push_str("#[repr(C)]\n");
            out.push_str(line);
            out.push('\n');
        } else if trimmed.starts_with("pub fn ") && !trimmed.contains("fmt(") {
            // Add #[no_mangle] and extern "C" for top-level pub fn
            let indent = &line[..line.len() - trimmed.len()];
            out.push_str(indent);
            out.push_str("#[no_mangle]\n");
            let transformed = line.replacen("pub fn ", "pub extern \"C\" fn ", 1);
            out.push_str(&transformed);
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod cranelift_tests {
    use super::cranelift_transform;

    #[test]
    fn transforms_pub_struct() {
        let input = "pub struct Foo {\n    x: i64,\n}\n";
        let out = cranelift_transform(input);
        assert!(out.contains("#[repr(C)]\npub struct Foo"));
    }

    #[test]
    fn transforms_pub_enum() {
        let input = "pub enum Color {\n    Red,\n    Blue,\n}\n";
        let out = cranelift_transform(input);
        assert!(out.contains("#[repr(C)]\npub enum Color"));
    }

    #[test]
    fn transforms_pub_fn() {
        let input = "pub fn add(a: i64, b: i64) -> i64 {\n    a + b\n}\n";
        let out = cranelift_transform(input);
        assert!(out.contains("#[no_mangle]"));
        assert!(out.contains("pub extern \"C\" fn add"));
    }

    #[test]
    fn skips_private_fn() {
        let input = "fn helper(x: i64) -> i64 { x }\n";
        let out = cranelift_transform(input);
        assert!(!out.contains("#[no_mangle]"));
        assert!(!out.contains("extern \"C\""));
    }

    #[test]
    fn skips_fmt_method() {
        let input =
            "    pub fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n";
        let out = cranelift_transform(input);
        assert!(!out.contains("#[no_mangle]"));
        assert!(!out.contains("extern \"C\""));
    }

    #[test]
    fn empty_input() {
        let out = cranelift_transform("");
        // Empty string produces a single trailing newline from the lines iterator
        assert!(!out.contains("#[repr(C)]"));
        assert!(!out.contains("#[no_mangle]"));
    }

    #[test]
    fn no_pub_items() {
        let input = "fn private() {}\nstruct Hidden;\n";
        let out = cranelift_transform(input);
        assert!(!out.contains("#[repr(C)]"));
        assert!(!out.contains("#[no_mangle]"));
    }

    #[test]
    fn preserves_indentation() {
        let input = "    pub struct Inner {\n        val: i32,\n    }\n";
        let out = cranelift_transform(input);
        assert!(out.contains("    #[repr(C)]\n    pub struct Inner"));
    }
}

/// Generate a Rust project from a type-checked Assura file.
///
/// Uses default backend configuration (`Rustc`, opt-level 2, no debug info).
/// For custom configuration, use [`codegen_with_config`].
pub fn codegen(typed: &TypedFile) -> GeneratedProject {
    codegen_with_config(typed, &BackendConfig::default())
}
