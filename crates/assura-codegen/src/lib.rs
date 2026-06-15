//! Rust code generation from type-checked Assura contracts.
//!
//! Takes a `TypedFile` from `assura-types` and generates a Rust project
//! consisting of a `Cargo.toml` and one or more `.rs` source files.
//! Generated Rust is formatted via `prettyplease`.
//!
//! This is T019: the initial scaffolding. Type mapping (T020), contract
//! codegen (T021), project generation (T022), and struct/enum codegen
//! (T023) extend this foundation.

pub mod type_map;

mod block;
mod contract;
mod decl;
mod expr;
mod service;
mod types_gen;

pub use types_gen::expr_to_rust_static;

use block::*;
use contract::*;
use decl::*;
use expr::*;
use service::*;
use types_gen::*;

use assura_parser::ast::{
    BinOp, BindDecl, BlockKind, Clause, ClauseKind, CodecRegistryDecl, ContractDecl, Decl, EnumDef,
    Expr, ExternDecl, FnDef, Literal, MagicPattern, ServiceDecl, ServiceItem, TypeBody, TypeDef,
    UnaryOp,
};
use assura_types::TypedFile;

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
#[derive(Debug, Clone, PartialEq)]
pub enum CodegenBackend {
    /// Standard rustc backend (optimized, production).
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
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            backend: CodegenBackend::Rustc,
            opt_level: 2,
            debug_info: false,
            target: CompileTarget::Native,
        }
    }
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

    // Phase 1: Collect all defined type names and feature_max constants
    let mut defined_types = std::collections::HashSet::new();
    let mut feature_max_consts: Vec<(String, String)> = Vec::new();
    for decl in &source.decls {
        match &decl.node {
            Decl::TypeDef(t) => {
                defined_types.insert(t.name.clone());
            }
            Decl::EnumDef(e) => {
                defined_types.insert(e.name.clone());
            }
            Decl::Block {
                kind, name, value, ..
            } if *kind == BlockKind::FeatureMax => {
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
                feature_max_consts.push((name.clone(), ty));
            }
            // Contract, Service, FnDef, Extern, Prophecy, and non-feature_max
            // blocks don't define type names or feature_max constants.
            Decl::Contract(_)
            | Decl::Service(_)
            | Decl::FnDef(_)
            | Decl::Extern(_)
            | Decl::Bind(_)
            | Decl::Prophecy(_)
            | Decl::CodecRegistry(_)
            | Decl::Block { .. } => {}
        }
    }
    // Add built-in type names that should never generate stubs
    for builtin in &[
        "Int", "Nat", "Float", "Bool", "String", "Bytes", "Unit", "Never", "U8", "U16", "U32",
        "U64", "I8", "I16", "I32", "I64", "F32", "F64", "List", "Vec", "Map", "Set", "Option",
        "Result", "Sequence", "i64", "u64", "f64", "bool", "u8", "u16", "u32", "i8", "i16", "i32",
        "f32", "f64",
    ] {
        defined_types.insert(builtin.to_string());
    }

    // Phase 2: Collect all referenced type names from function params/return types
    let mut referenced_types = std::collections::HashSet::new();
    for decl in &source.decls {
        match &decl.node {
            Decl::FnDef(f) => {
                collect_type_refs_from_tokens(&f.return_ty, &mut referenced_types);
                for p in &f.params {
                    collect_type_refs_from_tokens(&p.ty, &mut referenced_types);
                }
            }
            Decl::Extern(ex) => {
                collect_type_refs_from_tokens(&ex.return_ty, &mut referenced_types);
                for p in &ex.params {
                    collect_type_refs_from_tokens(&p.ty, &mut referenced_types);
                }
            }
            Decl::TypeDef(t) => {
                if let TypeBody::Struct(fields) = &t.body {
                    for f in fields {
                        collect_type_refs_from_tokens(&f.ty, &mut referenced_types);
                    }
                }
            }
            Decl::Contract(c) => {
                for clause in &c.clauses {
                    collect_type_refs_from_expr(&clause.body, &mut referenced_types);
                }
            }
            Decl::Service(s) => {
                for item in &s.items {
                    match item {
                        ServiceItem::TypeDef(t) => {
                            defined_types.insert(t.name.clone());
                        }
                        ServiceItem::EnumDef(e) => {
                            defined_types.insert(e.name.clone());
                        }
                        ServiceItem::Operation { clauses, .. }
                        | ServiceItem::Query { clauses, .. } => {
                            for clause in clauses {
                                collect_type_refs_from_expr(&clause.body, &mut referenced_types);
                            }
                        }
                        // States, Invariant, and Other don't contribute
                        // type references for stub generation.
                        ServiceItem::States(_)
                        | ServiceItem::Invariant(_)
                        | ServiceItem::Other { .. } => {}
                    }
                }
            }
            Decl::Bind(b) => {
                collect_type_refs_from_tokens(&b.return_ty, &mut referenced_types);
                for p in &b.params {
                    collect_type_refs_from_tokens(&p.ty, &mut referenced_types);
                }
            }
            // EnumDef, Prophecy, and Block don't contribute type references
            // that need stub generation.
            Decl::EnumDef(_) | Decl::Prophecy(_) | Decl::CodecRegistry(_) | Decl::Block { .. } => {}
        }
    }

    // Phase 3: Generate feature_max constants BEFORE any code that uses them.
    // Feature_max names that are also used as type arguments inside `<>` will
    // be emitted as struct stubs instead of consts in Phase 4b.
    let feature_max_used_as_type = {
        let fm_set: std::collections::HashSet<&str> =
            feature_max_consts.iter().map(|(n, _)| n.as_str()).collect();
        let mut result = std::collections::HashSet::new();
        for decl in &source.decls {
            let mut token_lists: Vec<&[String]> = Vec::new();
            match &decl.node {
                Decl::FnDef(f) => {
                    token_lists.push(f.return_ty.as_slice());
                    for p in &f.params {
                        token_lists.push(p.ty.as_slice());
                    }
                }
                Decl::Extern(ex) => {
                    token_lists.push(ex.return_ty.as_slice());
                    for p in &ex.params {
                        token_lists.push(p.ty.as_slice());
                    }
                }
                Decl::Bind(b) => {
                    token_lists.push(b.return_ty.as_slice());
                    for p in &b.params {
                        token_lists.push(p.ty.as_slice());
                    }
                }
                // Contract, Service, TypeDef, EnumDef, Prophecy, and Block
                // don't have typed param/return tokens.
                Decl::Contract(_)
                | Decl::Service(_)
                | Decl::TypeDef(_)
                | Decl::EnumDef(_)
                | Decl::Prophecy(_)
                | Decl::CodecRegistry(_)
                | Decl::Block { .. } => {}
            }
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
        }
        result
    };
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
    // Scan type token sequences for patterns like `Region<TOTAL_TABLE_SIZE>` to
    // know how many generic params each stub needs.
    let mut type_generic_params: std::collections::HashMap<String, Vec<GenericParamKind>> =
        std::collections::HashMap::new();
    let mut const_generic_names = std::collections::HashSet::new();
    for decl in &source.decls {
        let mut token_lists: Vec<&[String]> = Vec::new();
        match &decl.node {
            Decl::FnDef(f) => {
                token_lists.push(f.return_ty.as_slice());
                for p in &f.params {
                    token_lists.push(p.ty.as_slice());
                }
            }
            Decl::Extern(ex) => {
                token_lists.push(ex.return_ty.as_slice());
                for p in &ex.params {
                    token_lists.push(p.ty.as_slice());
                }
            }
            Decl::TypeDef(t) => {
                if let TypeBody::Struct(fields) = &t.body {
                    for f in fields {
                        token_lists.push(f.ty.as_slice());
                    }
                }
            }
            Decl::Bind(b) => {
                token_lists.push(b.return_ty.as_slice());
                for p in &b.params {
                    token_lists.push(p.ty.as_slice());
                }
            }
            // Contract, Service, EnumDef, Prophecy, and Block don't have
            // typed token sequences relevant for generic arity detection.
            Decl::Contract(_)
            | Decl::Service(_)
            | Decl::EnumDef(_)
            | Decl::Prophecy(_)
            | Decl::CodecRegistry(_)
            | Decl::Block { .. } => {}
        }
        for tokens in token_lists {
            detect_generic_arity(tokens, &mut type_generic_params, &mut const_generic_names);
        }
    }

    // Phase 4b: Generate type aliases for SCREAMING_SNAKE_CASE names used
    // as generic arguments. These are typically const-generic parameters in
    // Assura but are emitted as type params in the generated Rust.
    // Collect all SCREAMING_SNAKE_CASE names used as generic args:
    // both those from const_generic_names AND feature_max consts that are
    // used inside <> in type positions.
    let mut all_const_as_types = const_generic_names.clone();
    // Feature_max consts that appear in type token sequences inside <>
    // also need marker types for generic positions.
    let feature_max_set: std::collections::HashSet<String> =
        feature_max_consts.iter().map(|(n, _)| n.clone()).collect();
    for decl in &source.decls {
        let mut token_lists: Vec<&[String]> = Vec::new();
        match &decl.node {
            Decl::FnDef(f) => {
                token_lists.push(f.return_ty.as_slice());
                for p in &f.params {
                    token_lists.push(p.ty.as_slice());
                }
            }
            Decl::Extern(ex) => {
                token_lists.push(ex.return_ty.as_slice());
                for p in &ex.params {
                    token_lists.push(p.ty.as_slice());
                }
            }
            Decl::Bind(b) => {
                token_lists.push(b.return_ty.as_slice());
                for p in &b.params {
                    token_lists.push(p.ty.as_slice());
                }
            }
            Decl::Contract(_)
            | Decl::Service(_)
            | Decl::TypeDef(_)
            | Decl::EnumDef(_)
            | Decl::Prophecy(_)
            | Decl::CodecRegistry(_)
            | Decl::Block { .. } => {}
        }
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
    }
    let mut const_as_types: Vec<String> = all_const_as_types
        .iter()
        .filter(|n| !defined_types.contains(*n))
        .cloned()
        .collect();
    const_as_types.sort();
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

    // Phase 5: Generate stub structs for undefined types
    let mut undefined: Vec<String> = referenced_types
        .difference(&defined_types)
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

    // Count contracts and services to decide on module structure.
    let mut contract_names: Vec<String> = Vec::new();
    let mut service_names: Vec<String> = Vec::new();
    for decl in &source.decls {
        match &decl.node {
            Decl::Contract(c) => contract_names.push(c.name.clone()),
            Decl::Service(s) => service_names.push(s.name.clone()),
            Decl::TypeDef(_)
            | Decl::EnumDef(_)
            | Decl::FnDef(_)
            | Decl::Extern(_)
            | Decl::Bind(_)
            | Decl::Prophecy(_)
            | Decl::CodecRegistry(_)
            | Decl::Block { .. } => {}
        }
    }
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
        shared.push_str("#![allow(dead_code, unused_variables, unreachable_code)]\n\n");
        // Emit the pre-built preamble (feature_max, const-as-type stubs,
        // placeholder structs)
        shared.push_str(&code);

        for decl in &source.decls {
            match &decl.node {
                Decl::TypeDef(t) => generate_type_def(t, &mut shared),
                Decl::EnumDef(e) => generate_enum_def(e, &mut shared),
                Decl::Extern(ex) => generate_extern(ex, &mut shared),
                Decl::Bind(b) => generate_bind(b, &mut shared),
                Decl::FnDef(f) => {
                    if !f.is_ghost && !f.is_lemma {
                        generate_fn_def(f, &mut shared);
                    }
                }
                Decl::Block {
                    kind, name, body, ..
                } => {
                    if *kind != BlockKind::FeatureMax {
                        generate_block(kind, name, body, &mut shared);
                    }
                }
                // Prophecy variables are ghost; erased in codegen.
                Decl::Prophecy(_) => {}
                // CodecRegistry: generate dispatch function into shared lib.rs
                Decl::CodecRegistry(cr) => generate_codec_registry(cr, &mut shared),
                // Contracts and services go into their own files.
                Decl::Contract(_) | Decl::Service(_) => {}
            }
        }

        // Add pub mod declarations for each contract/service module
        for decl in &source.decls {
            match &decl.node {
                Decl::Contract(c) => {
                    let mod_name = c.name.to_lowercase();
                    shared.push_str(&format!("pub mod contract_{mod_name};\n"));
                }
                Decl::Service(s) => {
                    let mod_name = s.name.to_lowercase();
                    shared.push_str(&format!("pub mod {mod_name};\n"));
                }
                Decl::TypeDef(_)
                | Decl::EnumDef(_)
                | Decl::FnDef(_)
                | Decl::Extern(_)
                | Decl::Bind(_)
                | Decl::Prophecy(_)
                | Decl::CodecRegistry(_)
                | Decl::Block { .. } => {}
            }
        }

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
                mod_code.push_str("#![allow(dead_code, unused_variables, unreachable_code)]\n\n");
                mod_code.push_str("use super::*;\n\n");
                // Generate the contract body without wrapping it in
                // `pub mod contract_xxx { ... }` since it IS the module file.
                generate_contract_contents(c, &mut mod_code);
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
                mod_code.push_str("#![allow(dead_code, unused_variables, unreachable_code)]\n\n");
                mod_code.push_str("use super::*;\n\n");
                generate_service_contents(s, &mut mod_code);
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
        all_code.push_str("#![allow(dead_code, unused_variables, unreachable_code)]\n\n");
        all_code.push_str(&code);

        for decl in &source.decls {
            match &decl.node {
                Decl::TypeDef(t) => generate_type_def(t, &mut all_code),
                Decl::EnumDef(e) => generate_enum_def(e, &mut all_code),
                Decl::Contract(c) => generate_contract(c, &mut all_code),
                Decl::Extern(ex) => generate_extern(ex, &mut all_code),
                Decl::Bind(b) => generate_bind(b, &mut all_code),
                Decl::FnDef(f) => {
                    if !f.is_ghost && !f.is_lemma {
                        generate_fn_def(f, &mut all_code);
                    }
                }
                Decl::Service(s) => generate_service(s, &mut all_code),
                // Prophecy variables are ghost; erased in codegen.
                Decl::Prophecy(_) => {}
                // CodecRegistry: generate dispatch function
                Decl::CodecRegistry(cr) => generate_codec_registry(cr, &mut all_code),
                Decl::Block {
                    kind, name, body, ..
                } => {
                    if *kind != BlockKind::FeatureMax {
                        generate_block(kind, name, body, &mut all_code);
                    }
                }
            }
        }

        // S009: Generate proptest tests for testable contracts
        for decl in &source.decls {
            if let Decl::Contract(c) = &decl.node {
                generate_proptest_for_contract(c, &mut all_code);
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
    }

    project
}

/// Generate a Rust project from a type-checked Assura file.
///
/// Uses default backend configuration (`Rustc`, opt-level 2, no debug info).
/// For custom configuration, use [`codegen_with_config`].
pub fn codegen(typed: &TypedFile) -> GeneratedProject {
    codegen_with_config(typed, &BackendConfig::default())
}

