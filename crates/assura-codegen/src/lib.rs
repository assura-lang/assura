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

use assura_parser::ast::{
    BinOp, BindDecl, Clause, ClauseKind, ContractDecl, Decl, EnumDef, Expr, ExternDecl, FnDef,
    Literal, ServiceDecl, ServiceItem, TypeBody, TypeDef, UnaryOp,
};
use assura_types::TypedFile;

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

impl From<&assura_config::CodegenConfig> for BackendConfig {
    fn from(cfg: &assura_config::CodegenConfig) -> Self {
        let target = CompileTarget::from_str_loose(&cfg.target).unwrap_or(CompileTarget::Native);
        Self {
            target,
            ..Default::default()
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
            } if kind == "feature_max" => {
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
            Decl::EnumDef(_) | Decl::Prophecy(_) | Decl::Block { .. } => {}
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
            | Decl::Block { .. } => {}
        }
    }
    let total_modules = contract_names.len() + service_names.len();
    let use_multi_file = total_modules >= 2;

    if use_multi_file {
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
                    if kind != "feature_max" {
                        generate_block(kind, name, body, &mut shared);
                    }
                }
                // Prophecy variables are ghost; erased in codegen.
                Decl::Prophecy(_) => {}
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

        GeneratedProject { cargo_toml, files }
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
                Decl::Block {
                    kind, name, body, ..
                } => {
                    if kind != "feature_max" {
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
    }
}

/// Generate a Rust project from a type-checked Assura file.
///
/// Uses default backend configuration (`Rustc`, opt-level 2, no debug info).
/// For custom configuration, use [`codegen_with_config`].
pub fn codegen(typed: &TypedFile) -> GeneratedProject {
    codegen_with_config(typed, &BackendConfig::default())
}

// ---------------------------------------------------------------------------
// Cargo.toml generation
// ---------------------------------------------------------------------------

fn generate_cargo_toml_impl(
    crate_name: &str,
    config: &BackendConfig,
    include_proptest: bool,
    include_thiserror: bool,
) -> String {
    let mut toml = format!(
        r#"[package]
name = "{crate_name}"
version = "0.1.0"
edition = "2024"

# Generated by the Assura compiler.
# Do not edit manually.

[dependencies]
"#
    );

    if include_thiserror {
        toml.push_str("thiserror = \"2\"\n");
    }

    if include_proptest {
        toml.push_str("\n[dev-dependencies]\nproptest = \"1\"\n");
    }

    // Add profile configuration based on BackendConfig
    if config.opt_level != 2 || config.debug_info {
        toml.push_str("\n[profile.release]\n");
        toml.push_str(&format!("opt-level = {}\n", config.opt_level));
        if config.debug_info {
            toml.push_str("debug = true\n");
        }
    }

    // Add backend-specific configuration
    if matches!(config.backend, CodegenBackend::Cranelift) {
        toml.push_str(
            "\n# Using Cranelift backend for fast compilation\n\
             # Install: rustup component add rustc-codegen-cranelift\n",
        );
    }

    // WASM target: add cdylib crate type so cargo produces a .wasm file
    if matches!(config.target, CompileTarget::Wasm) {
        toml.push_str("\n[lib]\ncrate-type = [\"cdylib\"]\n");
        toml.push_str(
            "\n# WASM target: build with `cargo build --target wasm32-wasip1`\n\
             # Install target: `rustup target add wasm32-wasip1`\n",
        );
    }

    toml
}

// ---------------------------------------------------------------------------
// Type mapping: Assura types -> Rust types
// ---------------------------------------------------------------------------

/// Map a single Assura type token to its Rust equivalent.
fn map_type_token(tok: &str) -> &str {
    match tok {
        "Int" => "i64",
        "Nat" => "u64",
        "Float" => "f64",
        "Bool" => "bool",
        "String" => "String",
        "Bytes" => "Vec<u8>",
        "Unit" => "()",
        "Never" => "!",
        "U8" => "u8",
        "U16" => "u16",
        "U32" => "u32",
        "U64" => "u64",
        "I8" => "i8",
        "I16" => "i16",
        "I32" => "i32",
        "I64" => "i64",
        "F32" => "f32",
        "F64" => "f64",
        "List" => "Vec",
        "Map" => "std::collections::BTreeMap",
        "Set" => "std::collections::BTreeSet",
        "Sequence" => "Vec",
        // Option, Result map to themselves
        _ => tok,
    }
}

/// Convert an Assura type token sequence (e.g., `["List", "<", "Int", ">"]`)
/// to a valid Rust type string.
///
/// Handles Assura-specific syntax that is not valid Rust:
/// - Refinement types: `{ x : T | P }` -> base type `T`
/// - Taint annotations: `T @ taint : label` -> just `T`
/// - Union error types: `T | E` -> `Result<T, E>`
/// - Smart joining: `Vec < i64 >` -> `Vec<i64>`
fn map_type_tokens(tokens: &[String]) -> String {
    if tokens.is_empty() {
        return "()".to_string();
    }

    // Phase 1: Strip annotations and clause keywords that leak into type tokens
    // Stops at: "@" (taint), "#" (attribute), "decreases", "where"
    let clean: Vec<&str> = tokens
        .iter()
        .map(|s| s.as_str())
        .take_while(|t| !matches!(*t, "@" | "#" | "decreases" | "where"))
        .collect();
    if clean.is_empty() {
        return "()".to_string();
    }

    // Phase 2: Strip refinement predicates: { x : T | P } -> just the base type
    if clean.first() == Some(&"{") {
        return extract_base_type_from_refined(tokens);
    }

    // Phase 3: Handle union error types: T | E -> Result<T, E>
    // Find a "|" not inside angle brackets
    let mut depth = 0i32;
    let mut pipe_pos = None;
    for (i, tok) in clean.iter().enumerate() {
        match *tok {
            "<" => depth += 1,
            ">" => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            "|" if depth == 0 => {
                pipe_pos = Some(i);
                break;
            }
            _ => {}
        }
    }

    if let Some(pos) = pipe_pos {
        let ok_tokens: Vec<String> = clean[..pos].iter().map(|s| s.to_string()).collect();
        let err_tokens: Vec<String> = clean[pos + 1..].iter().map(|s| s.to_string()).collect();
        let ok_type = map_type_tokens(&ok_tokens);
        let err_type = map_type_tokens(&err_tokens);
        return format!("Result<{ok_type}, {err_type}>");
    }

    // Phase 4: Map each token and join smartly (no extra spaces around < > & etc.)
    // Convert SCREAMING_SNAKE_CASE names inside angle brackets to marker types,
    // since Assura const-generics don't translate directly to Rust generics.
    let mut in_angle = 0i32;
    let mapped: Vec<String> = clean
        .iter()
        .map(|t| {
            match *t {
                "<" => in_angle += 1,
                ">" if in_angle > 0 => {
                    in_angle -= 1;
                }
                _ => {}
            }
            if in_angle > 0 && is_const_name(t) {
                // Const-generic name used as type param: wrap as marker type
                t.to_string()
            } else {
                map_type_token(t).to_string()
            }
        })
        .collect();
    let refs: Vec<&str> = mapped.iter().map(|s| s.as_str()).collect();
    smart_join_type_tokens(&refs)
}

/// Join type tokens without spurious spaces around angle brackets, ampersands, etc.
fn smart_join_type_tokens(tokens: &[&str]) -> String {
    let mut result = String::new();
    for (i, tok) in tokens.iter().enumerate() {
        if i > 0 {
            let prev = tokens[i - 1];
            let no_space = matches!(*tok, ">" | "," | ")" | ".")
                || matches!(prev, "<" | "(" | "&" | ".")
                || (*tok == "mut" && prev == "&");
            if !no_space {
                result.push(' ');
            }
        }
        result.push_str(tok);
    }
    result
}

// ---------------------------------------------------------------------------
// Expression codegen
// ---------------------------------------------------------------------------

/// Heuristic: returns true if the expression is likely a numeric value
/// (variable, constant, literal, or arithmetic). Used to decide whether to
/// emit `i128::from(...)` casts for cross-width comparisons.
fn is_numeric_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(_) | Expr::Literal(Literal::Int(_)) | Expr::Literal(Literal::Float(_)) => true,
        Expr::Field(_, _) => true,
        Expr::BinOp { op, .. } => matches!(
            op,
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod
        ),
        Expr::UnaryOp {
            op: UnaryOp::Neg, ..
        } => true,
        Expr::Paren(e) | Expr::Old(e) | Expr::Cast { expr: e, .. } => is_numeric_expr(e),
        Expr::Call { .. } | Expr::MethodCall { .. } | Expr::Index { .. } => true,
        Expr::Let { body, .. } => is_numeric_expr(body),
        Expr::If { then_branch, .. } => is_numeric_expr(then_branch),
        Expr::Match { arms, .. } => arms.first().is_some_and(|a| is_numeric_expr(&a.body)),
        // These are definitively not numeric expressions
        Expr::Literal(Literal::Str(_) | Literal::Bool(_))
        | Expr::UnaryOp {
            op: UnaryOp::Not, ..
        }
        | Expr::Forall { .. }
        | Expr::Exists { .. }
        | Expr::List(_)
        | Expr::Tuple(_)
        | Expr::Ghost(_)
        | Expr::Apply { .. }
        | Expr::Block(_)
        | Expr::Raw(_) => false,
    }
}

/// Convert an Assura `Expr` to a Rust expression string.
fn expr_to_rust(expr: &Expr) -> String {
    match expr {
        Expr::Literal(lit) => match lit {
            Literal::Int(s) | Literal::Float(s) => s.clone(),
            Literal::Str(s) => format!("\"{s}\""),
            Literal::Bool(b) => b.to_string(),
        },
        Expr::Ident(s) => {
            if s == "result" {
                "__result".to_string()
            } else {
                s.clone()
            }
        }
        Expr::Field(recv, field) => format!("{}.{field}", expr_to_rust(recv)),
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let args_s: Vec<String> = args.iter().map(expr_to_rust).collect();
            format!("{}.{method}({})", expr_to_rust(receiver), args_s.join(", "))
        }
        Expr::Call { func, args } => {
            let args_s: Vec<String> = args.iter().map(expr_to_rust).collect();
            format!("{}({})", expr_to_rust(func), args_s.join(", "))
        }
        Expr::Index { expr: e, index } => {
            format!("{}[{}]", expr_to_rust(e), expr_to_rust(index))
        }
        Expr::BinOp { lhs, op, rhs } => {
            // For ordering comparisons, cast both sides to i128 to avoid
            // type mismatch between different integer widths (e.g., u16 vs u64).
            // This mirrors Assura's abstract numeric semantics.
            // We only do this for ordering (< <= > >=), not equality (== !=),
            // because equality works via PartialEq and wrapping in i128::from
            // would fail on non-numeric types.
            let is_numeric_cmp = matches!(op, BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte)
                && is_numeric_expr(lhs)
                && is_numeric_expr(rhs);

            let op_s = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Mod => "%",
                BinOp::Eq => "==",
                BinOp::Neq => "!=",
                BinOp::Lt => "<",
                BinOp::Lte => "<=",
                BinOp::Gt => ">",
                BinOp::Gte => ">=",
                BinOp::And => "&&",
                BinOp::Or => "||",
                BinOp::Implies => {
                    return format!("(!{} || {})", expr_to_rust(lhs), expr_to_rust(rhs));
                }
                BinOp::In => {
                    // `x in S` means S.contains(&x)
                    return format!("{}.contains(&{})", expr_to_rust(rhs), expr_to_rust(lhs));
                }
                BinOp::NotIn => {
                    return format!("!{}.contains(&{})", expr_to_rust(rhs), expr_to_rust(lhs));
                }
                BinOp::Concat => {
                    return format!("[{}, {}].concat()", expr_to_rust(lhs), expr_to_rust(rhs));
                }
                BinOp::Range => "..",
            };
            if is_numeric_cmp {
                format!(
                    "(i128::from({}) {op_s} i128::from({}))",
                    expr_to_rust(lhs),
                    expr_to_rust(rhs)
                )
            } else {
                format!("({} {op_s} {})", expr_to_rust(lhs), expr_to_rust(rhs))
            }
        }
        Expr::UnaryOp { op, expr: e } => {
            let op_s = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
            };
            format!("({op_s}{})", expr_to_rust(e))
        }
        Expr::Old(e) => {
            // old(expr) references a pre-state snapshot saved at function entry.
            // The variable name is derived from the inner expression.
            format!("__old_{}", old_var_name(e))
        }
        Expr::Forall { var, domain, body } => {
            format!(
                "{}.iter().all(|{var}| {})",
                expr_to_rust(domain),
                expr_to_rust(body)
            )
        }
        Expr::Exists { var, domain, body } => {
            format!(
                "{}.iter().any(|{var}| {})",
                expr_to_rust(domain),
                expr_to_rust(body)
            )
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => match else_branch {
            Some(eb) => format!(
                "if {} {{ {} }} else {{ {} }}",
                expr_to_rust(cond),
                expr_to_rust(then_branch),
                expr_to_rust(eb)
            ),
            None => format!(
                "if {} {{ {} }}",
                expr_to_rust(cond),
                expr_to_rust(then_branch)
            ),
        },
        Expr::Paren(e) => format!("({})", expr_to_rust(e)),
        Expr::List(items) => {
            let elems: Vec<String> = items.iter().map(expr_to_rust).collect();
            format!("vec![{}]", elems.join(", "))
        }
        Expr::Cast { expr: e, ty } => {
            format!("({} as {})", expr_to_rust(e), map_type_token(ty))
        }
        Expr::Block(exprs) => {
            let strs: Vec<String> = exprs.iter().map(expr_to_rust).collect();
            strs.join(" ")
        }
        Expr::Ghost(_inner) => {
            // Ghost blocks are erased at runtime; emit nothing.
            "/* ghost erased */()".to_string()
        }
        Expr::Apply { lemma_name, .. } => {
            // Lemma applications are erased at runtime; emit comment.
            format!("/* lemma {lemma_name} applied */")
        }
        Expr::Match { scrutinee, arms } => {
            let scrut = expr_to_rust(scrutinee);
            let arms_code: Vec<String> = arms
                .iter()
                .map(|arm| {
                    let pat = match &arm.pattern {
                        assura_parser::ast::Pattern::Ident(name) => name.clone(),
                        assura_parser::ast::Pattern::Wildcard => "_".into(),
                        assura_parser::ast::Pattern::Literal(lit) => match lit {
                            Literal::Int(s) | Literal::Float(s) => s.clone(),
                            Literal::Str(s) => format!("\"{s}\""),
                            Literal::Bool(b) => b.to_string(),
                        },
                        assura_parser::ast::Pattern::Constructor { name, fields } => {
                            if fields.is_empty() {
                                name.clone()
                            } else {
                                let fs: Vec<String> = fields.iter().map(pattern_to_rust).collect();
                                format!("{name}({})", fs.join(", "))
                            }
                        }
                        assura_parser::ast::Pattern::Tuple(pats) => {
                            let ps: Vec<String> = pats.iter().map(pattern_to_rust).collect();
                            format!("({})", ps.join(", "))
                        }
                    };
                    let body = expr_to_rust(&arm.body);
                    format!("    {pat} => {body},")
                })
                .collect();
            // Add wildcard fallback if no arm is a catch-all
            let has_wildcard = arms.iter().any(|arm| {
                matches!(
                    &arm.pattern,
                    assura_parser::ast::Pattern::Wildcard | assura_parser::ast::Pattern::Ident(_)
                )
            });
            if !has_wildcard {
                let mut all_arms = arms_code;
                all_arms.push("    _ => unreachable!(\"non-exhaustive match\"),".to_string());
                format!("match {} {{\n{}\n}}", scrut, all_arms.join("\n"))
            } else {
                format!("match {} {{\n{}\n}}", scrut, arms_code.join("\n"))
            }
        }
        Expr::Let { name, value, body } => {
            format!(
                "{{ let {} = {}; {} }}",
                name,
                expr_to_rust(value),
                expr_to_rust(body)
            )
        }
        Expr::Tuple(elems) => {
            let items: Vec<String> = elems.iter().map(expr_to_rust).collect();
            format!("({})", items.join(", "))
        }
        Expr::Raw(tokens) => raw_tokens_to_rust(tokens),
    }
}

/// Convert raw token sequences to Rust, handling quantifier patterns.
///
/// Detects `forall var in domain: body` and `exists var in domain: body`
/// in raw tokens and translates them to `.iter().all(|var| body)` /
/// `.iter().any(|var| body)` respectively. Falls back to joined tokens
/// for non-quantifier sequences.
fn raw_tokens_to_rust(tokens: &[String]) -> String {
    if tokens.is_empty() {
        return String::new();
    }
    // Detect: forall/exists VAR in DOMAIN : BODY
    let first = tokens[0].as_str();
    if matches!(first, "forall" | "exists")
        && tokens.len() >= 5
        && let Some(in_pos) = tokens[1..].iter().position(|t| t == "in")
    {
        let in_pos = in_pos + 1; // offset from tokens[0]
        let var = &tokens[1..in_pos].join("_");
        // Find the colon that separates domain from body
        if let Some(colon_offset) = tokens[in_pos + 1..].iter().position(|t| t == ":") {
            let colon_pos = in_pos + 1 + colon_offset;
            let domain_tokens = &tokens[in_pos + 1..colon_pos];
            let body_tokens = &tokens[colon_pos + 1..];

            let domain = {
                let mapped: Vec<&str> = domain_tokens.iter().map(|t| map_type_token(t)).collect();
                smart_join_type_tokens(&mapped)
            };
            let body = raw_tokens_to_rust(body_tokens);

            let method = if first == "forall" { "all" } else { "any" };
            return format!("{domain}.iter().{method}(|{var}| {body})");
        }
    }

    // Strip typestate annotations: `expr @ State` -> `true /* typestate: expr @ State */`
    if let Some(at_pos) = tokens.iter().position(|t| t == "@") {
        let before = &tokens[..at_pos];
        let after = &tokens[at_pos + 1..];
        let expr_s = raw_tokens_to_rust(before);
        let state_s = after.join(" ");
        return format!("true /* typestate: {expr_s} @ {state_s} */");
    }

    // Check for `result` keyword — replace with `__result`
    let mapped: Vec<String> = tokens
        .iter()
        .map(|t| {
            if t == "result" {
                "__result".to_string()
            } else {
                map_type_token(t).to_string()
            }
        })
        .collect();
    let refs: Vec<&str> = mapped.iter().map(|s| s.as_str()).collect();
    smart_join_type_tokens(&refs)
}

// ---------------------------------------------------------------------------
// old(expr) support
// ---------------------------------------------------------------------------

/// Derive a variable name for an `old(expr)` snapshot from the expression.
/// E.g., `old(x)` -> `__old_x`, `old(buf.len)` -> `__old_buf_len`.
/// Generate a debug_assert! that handles multi-line expressions.
///
/// If the expression contains newlines (e.g. a match block), wraps it in a
/// block `{ ... }` so the assert is valid Rust syntax.
///
/// If the expression contains patterns that would fail on stub types
/// (nested field accesses like `a.b.c`), emit it as a comment instead
/// to keep the generated code compilable while preserving the contract intent.
fn generate_debug_assert(code: &mut String, expr: &str, label: &str) {
    // If expression references deep field chains (e.g., state.head.extra.extra_max),
    // emit as a comment since stub types don't have these fields.
    if has_deep_field_access(expr) {
        code.push_str(&format!("    // {label}: {}\n", expr.replace('"', "\\\"")));
        return;
    }
    if expr.contains('\n') {
        // Multi-line expressions (match, etc.) need a block wrapper
        let msg = expr.replace('\n', " ").replace('"', "\\\"");
        code.push_str(&format!(
            "    debug_assert!({{ {expr} }}, \"{label}: {msg}\");\n"
        ));
    } else {
        code.push_str(&format!(
            "    debug_assert!({expr}, \"{label}: {}\");\n",
            expr.replace('"', "\\\"")
        ));
    }
}

/// Check if an expression string contains patterns that would fail to compile
/// against placeholder stub types:
/// - Any field access (a.b) since stub types have no fields
/// - Method calls on unknown objects
/// - References to `__result.field`
fn has_deep_field_access(expr: &str) -> bool {
    // Detect struct field access like `state.head.extra` that would fail on stub types.
    // Exclude method-call chains like `.iter().all()`, `.len()`, `.clone()` which are
    // standard library methods and work fine.
    let method_names = [
        "iter",
        "all",
        "any",
        "map",
        "filter",
        "len",
        "is_empty",
        "clone",
        "count",
        "sum",
        "collect",
        "flat_map",
        "zip",
        "enumerate",
        "take",
        "skip",
        "find",
        "fold",
        "for_each",
        "min",
        "max",
        "contains",
        "position",
        "into_iter",
        "as_ref",
        "as_mut",
        "unwrap",
        "unwrap_or",
        "expect",
        "ok",
        "err",
        "is_some",
        "is_none",
        "is_ok",
        "is_err",
    ];
    for word in expr.split(|c: char| !c.is_alphanumeric() && c != '.' && c != '_') {
        if word.contains('.') && !word.is_empty() {
            let parts: Vec<&str> = word.split('.').collect();
            if parts.len() >= 2
                && parts[0]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
            {
                // Check if ALL dotted segments after the first are known method names
                let all_methods = parts[1..].iter().all(|p| method_names.contains(p));
                if !all_methods {
                    return true;
                }
            }
        }
    }
    // __result.field references (but not __result.iter(), etc.)
    if expr.contains("__result.") {
        // Check if all occurrences of __result. are followed by method calls
        for chunk in expr.split("__result.") {
            if chunk.is_empty() {
                continue;
            }
            let after: String = chunk
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !after.is_empty() && !method_names.contains(&after.as_str()) {
                return true;
            }
        }
    }
    false
}

/// Like `generate_debug_assert` but with configurable indent level.
fn generate_debug_assert_indented(code: &mut String, expr: &str, label: &str, indent: usize) {
    let pad = "    ".repeat(indent);
    if has_deep_field_access(expr) {
        code.push_str(&format!("{pad}// {label}: {}\n", expr.replace('"', "\\\"")));
        return;
    }
    if expr.contains('\n') {
        let msg = expr.replace('\n', " ").replace('"', "\\\"");
        code.push_str(&format!(
            "{pad}debug_assert!({{ {expr} }}, \"{label}: {msg}\");\n"
        ));
    } else {
        code.push_str(&format!(
            "{pad}debug_assert!({expr}, \"{label}: {}\");\n",
            expr.replace('"', "\\\"")
        ));
    }
}

/// Convert a pattern to Rust pattern syntax.
fn pattern_to_rust(pat: &assura_parser::ast::Pattern) -> String {
    match pat {
        assura_parser::ast::Pattern::Ident(name) => name.clone(),
        assura_parser::ast::Pattern::Wildcard => "_".into(),
        assura_parser::ast::Pattern::Literal(lit) => match lit {
            Literal::Int(s) | Literal::Float(s) => s.clone(),
            Literal::Str(s) => format!("\"{s}\""),
            Literal::Bool(b) => b.to_string(),
        },
        assura_parser::ast::Pattern::Constructor { name, fields } => {
            if fields.is_empty() {
                name.clone()
            } else {
                let fs: Vec<String> = fields.iter().map(pattern_to_rust).collect();
                format!("{name}({})", fs.join(", "))
            }
        }
        assura_parser::ast::Pattern::Tuple(pats) => {
            let ps: Vec<String> = pats.iter().map(pattern_to_rust).collect();
            format!("({})", ps.join(", "))
        }
    }
}

fn old_var_name(expr: &Expr) -> String {
    match expr {
        Expr::Ident(s) => s.clone(),
        Expr::Field(recv, field) => format!("{}_{field}", old_var_name(recv)),
        Expr::Call { func, .. } => old_var_name(func),
        Expr::MethodCall {
            receiver, method, ..
        } => format!("{}_{method}", old_var_name(receiver)),
        Expr::Index { expr: e, .. } => format!("{}_idx", old_var_name(e)),
        Expr::Literal(lit) => match lit {
            Literal::Int(s) | Literal::Float(s) => format!("lit_{s}"),
            Literal::Str(s) => format!("lit_{}", s.trim_matches('"')),
            Literal::Bool(b) => format!("lit_{b}"),
        },
        Expr::BinOp { lhs, op, rhs } => {
            let op_name = match op {
                BinOp::Add => "add",
                BinOp::Sub => "sub",
                BinOp::Mul => "mul",
                BinOp::Div => "div",
                BinOp::Mod => "mod",
                BinOp::And => "and",
                BinOp::Or => "or",
                BinOp::Eq => "eq",
                BinOp::Neq => "neq",
                BinOp::Lt => "lt",
                BinOp::Gt => "gt",
                BinOp::Lte => "lte",
                BinOp::Gte => "gte",
                BinOp::Implies => "implies",
                BinOp::In => "in",
                BinOp::NotIn => "notin",
                BinOp::Concat => "concat",
                BinOp::Range => "range",
            };
            format!("{}_{op_name}_{}", old_var_name(lhs), old_var_name(rhs))
        }
        Expr::UnaryOp { op, expr: e } => {
            let prefix = match op {
                UnaryOp::Neg => "neg",
                UnaryOp::Not => "not",
            };
            format!("{prefix}_{}", old_var_name(e))
        }
        Expr::Old(inner) => old_var_name(inner),
        Expr::Paren(inner) => old_var_name(inner),
        Expr::Cast { expr: e, .. } => old_var_name(e),
        Expr::Ghost(inner) => format!("ghost_{}", old_var_name(inner)),
        Expr::Forall { var, .. } => format!("forall_{var}"),
        Expr::Exists { var, .. } => format!("exists_{var}"),
        Expr::If { cond, .. } => format!("if_{}", old_var_name(cond)),
        Expr::Let { name, .. } => format!("let_{name}"),
        Expr::Match { scrutinee, .. } => format!("match_{}", old_var_name(scrutinee)),
        Expr::Apply { lemma_name, .. } => format!("apply_{lemma_name}"),
        Expr::List(_) => "list".to_string(),
        Expr::Tuple(_) => "tuple".to_string(),
        Expr::Block(exprs) => {
            if let Some(first) = exprs.first() {
                old_var_name(first)
            } else {
                "block".to_string()
            }
        }
        Expr::Raw(tokens) => {
            if let Some(first) = tokens.first() {
                first.clone()
            } else {
                "raw".to_string()
            }
        }
    }
}

/// Walk an expression tree and collect all `old(inner)` sub-expressions.
/// Returns `(var_name, rust_expr)` pairs for generating pre-state snapshots.
fn collect_old_exprs(expr: &Expr) -> Vec<(String, String)> {
    let mut result = Vec::new();
    collect_old_exprs_inner(expr, &mut result);
    result
}

fn collect_old_exprs_inner(expr: &Expr, out: &mut Vec<(String, String)>) {
    match expr {
        Expr::Old(inner) => {
            let var = old_var_name(inner);
            let rust = expr_to_rust(inner);
            // Avoid duplicates
            if !out.iter().any(|(v, _)| v == &var) {
                out.push((var, rust));
            }
            // Also recurse into the inner expression (in case of nested old)
            collect_old_exprs_inner(inner, out);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_old_exprs_inner(lhs, out);
            collect_old_exprs_inner(rhs, out);
        }
        Expr::UnaryOp { expr: e, .. }
        | Expr::Paren(e)
        | Expr::Field(e, _)
        | Expr::Cast { expr: e, .. } => {
            collect_old_exprs_inner(e, out);
        }
        Expr::Call { func, args } => {
            collect_old_exprs_inner(func, out);
            for a in args {
                collect_old_exprs_inner(a, out);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_old_exprs_inner(receiver, out);
            for a in args {
                collect_old_exprs_inner(a, out);
            }
        }
        Expr::Index { expr: e, index } => {
            collect_old_exprs_inner(e, out);
            collect_old_exprs_inner(index, out);
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_old_exprs_inner(domain, out);
            collect_old_exprs_inner(body, out);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_old_exprs_inner(cond, out);
            collect_old_exprs_inner(then_branch, out);
            if let Some(eb) = else_branch {
                collect_old_exprs_inner(eb, out);
            }
        }
        Expr::List(items) | Expr::Block(items) => {
            for item in items {
                collect_old_exprs_inner(item, out);
            }
        }
        Expr::Ghost(inner) => {
            // Ghost blocks are erased but may reference old() in
            // their verification expressions.
            collect_old_exprs_inner(inner, out);
        }
        Expr::Apply { args, .. } => {
            // Apply is erased but may reference old() in arguments.
            for a in args {
                collect_old_exprs_inner(a, out);
            }
        }
        Expr::Match { scrutinee, arms } => {
            collect_old_exprs_inner(scrutinee, out);
            for arm in arms {
                collect_old_exprs_inner(&arm.body, out);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_old_exprs_inner(value, out);
            collect_old_exprs_inner(body, out);
        }
        Expr::Tuple(elems) => {
            for e in elems {
                collect_old_exprs_inner(e, out);
            }
        }
        // Leaf nodes: no old() inside
        Expr::Literal(_) | Expr::Ident(_) | Expr::Raw(_) => {}
    }
}

// ---------------------------------------------------------------------------
// Type definitions
// ---------------------------------------------------------------------------

fn generate_type_def(t: &TypeDef, code: &mut String) {
    let tps = if t.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", t.type_params.join(", "))
    };

    match &t.body {
        TypeBody::Struct(fields) => {
            code.push_str(&format!(
                "#[derive(Debug, Clone, PartialEq)]\npub struct {}{tps} {{\n",
                t.name
            ));
            for f in fields {
                let vis = if f.is_pub { "pub " } else { "" };
                let ty = map_type_tokens(&f.ty);
                code.push_str(&format!("    {vis}{}: {ty},\n", f.name));
            }
            code.push_str("}\n\n");
        }
        TypeBody::Alias(tokens) => {
            let ty = map_type_tokens(tokens);
            code.push_str(&format!("pub type {}{tps} = {ty};\n\n", t.name));
        }
        TypeBody::Refined(tokens) => {
            let base_ty = extract_base_type_from_refined(tokens);
            code.push_str(&format!(
                "/// Refined type: {}\n#[derive(Debug, Clone, PartialEq)]\npub struct {}{tps}(pub {base_ty});\n\n",
                tokens.join(" "),
                t.name
            ));
        }
        TypeBody::Empty => {
            code.push_str(&format!(
                "#[derive(Debug, Clone, PartialEq)]\npub struct {}{tps};\n\n",
                t.name
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Type reference collection (for generating stub types)
// ---------------------------------------------------------------------------

/// Check if a name looks like a constant (SCREAMING_SNAKE_CASE).
/// E.g., `TOTAL_TABLE_SIZE`, `MAX_ALPHABET_SIZE`.
fn is_const_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
        && name.contains('_')
}

/// Info about a generic parameter: either a type param or a const param.
#[derive(Debug, Clone)]
enum GenericParamKind {
    Type,
    Const,
}

/// Scan a type token sequence for `TypeName<Arg1, Arg2, ...>` patterns.
/// Records the generic params (type vs const) for each type name, and
/// collects names that appear as const-generic arguments.
fn detect_generic_arity(
    tokens: &[String],
    param_map: &mut std::collections::HashMap<String, Vec<GenericParamKind>>,
    const_names: &mut std::collections::HashSet<String>,
) {
    let mut i = 0;
    while i < tokens.len() {
        if i + 1 < tokens.len() && tokens[i + 1] == "<" && is_user_type_name(&tokens[i]) {
            let type_name = tokens[i].clone();
            let mut depth = 0;
            let mut params = Vec::new();
            let mut current_is_const = false;
            let mut j = i + 1;
            while j < tokens.len() {
                match tokens[j].as_str() {
                    "<" => {
                        depth += 1;
                        if depth == 1 {
                            current_is_const = false;
                        }
                    }
                    ">" => {
                        depth -= 1;
                        if depth == 0 {
                            // Record the last parameter
                            params.push(if current_is_const {
                                GenericParamKind::Const
                            } else {
                                GenericParamKind::Type
                            });
                            break;
                        }
                    }
                    "," if depth == 1 => {
                        params.push(if current_is_const {
                            GenericParamKind::Const
                        } else {
                            GenericParamKind::Type
                        });
                        current_is_const = false;
                    }
                    tok if depth == 1 && is_const_name(tok) => {
                        const_names.insert(tok.to_string());
                        current_is_const = true;
                    }
                    tok if depth == 1
                        && !tok.is_empty()
                        && tok.chars().all(|c| c.is_ascii_digit()) =>
                    {
                        // Numeric literal as const generic
                        current_is_const = true;
                    }
                    _ => {}
                }
                j += 1;
            }
            let existing_len = param_map.get(&type_name).map_or(0, |v| v.len());
            if params.len() > existing_len {
                param_map.insert(type_name, params);
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
}

/// Check if a token looks like a user-defined type name (starts with uppercase,
/// alphanumeric, not a Rust keyword or built-in).
fn is_user_type_name(tok: &str) -> bool {
    !tok.is_empty()
        && tok.chars().next().is_some_and(|c| c.is_uppercase())
        && tok.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !matches!(
            tok,
            "Int"
                | "Nat"
                | "Float"
                | "Bool"
                | "String"
                | "Bytes"
                | "Unit"
                | "Never"
                | "U8"
                | "U16"
                | "U32"
                | "U64"
                | "I8"
                | "I16"
                | "I32"
                | "I64"
                | "F32"
                | "F64"
                | "List"
                | "Vec"
                | "Map"
                | "Set"
                | "Option"
                | "Result"
                | "Sequence"
                | "Self"
                | "Box"
                | "Fn"
                | "FnOnce"
                | "FnMut"
        )
}

/// Collect user-defined type names from a type token sequence.
fn collect_type_refs_from_tokens(tokens: &[String], out: &mut std::collections::HashSet<String>) {
    for tok in tokens {
        // Skip taint annotations, attributes, and keywords
        if matches!(
            tok.as_str(),
            "@" | "#"
                | "taint"
                | "untrusted"
                | "validated"
                | "secret"
                | "pub"
                | "mut"
                | ":"
                | "|"
                | "&"
                | ">"
                | "<"
                | ","
                | "("
                | ")"
                | "{"
                | "}"
                | "["
                | "]"
                | "decreases"
                | "where"
        ) {
            continue;
        }
        if is_user_type_name(tok) {
            out.insert(tok.clone());
        }
    }
}

/// Collect type names referenced in expressions (e.g., constructor calls).
fn collect_type_refs_from_expr(expr: &Expr, out: &mut std::collections::HashSet<String>) {
    match expr {
        Expr::Ident(name) => {
            if is_user_type_name(name) {
                out.insert(name.clone());
            }
        }
        Expr::Call { func, args } => {
            collect_type_refs_from_expr(func, out);
            for a in args {
                collect_type_refs_from_expr(a, out);
            }
        }
        Expr::MethodCall {
            receiver,
            method: _,
            args,
        } => {
            collect_type_refs_from_expr(receiver, out);
            for a in args {
                collect_type_refs_from_expr(a, out);
            }
        }
        Expr::Field(recv, _) => collect_type_refs_from_expr(recv, out),
        Expr::Index { expr: e, index } => {
            collect_type_refs_from_expr(e, out);
            collect_type_refs_from_expr(index, out);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_type_refs_from_expr(lhs, out);
            collect_type_refs_from_expr(rhs, out);
        }
        Expr::UnaryOp { expr: e, .. }
        | Expr::Old(e)
        | Expr::Paren(e)
        | Expr::Cast { expr: e, .. }
        | Expr::Ghost(e) => {
            collect_type_refs_from_expr(e, out);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_type_refs_from_expr(cond, out);
            collect_type_refs_from_expr(then_branch, out);
            if let Some(eb) = else_branch {
                collect_type_refs_from_expr(eb, out);
            }
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_type_refs_from_expr(domain, out);
            collect_type_refs_from_expr(body, out);
        }
        Expr::Let { value, body, .. } => {
            collect_type_refs_from_expr(value, out);
            collect_type_refs_from_expr(body, out);
        }
        Expr::Match { scrutinee, arms } => {
            collect_type_refs_from_expr(scrutinee, out);
            for arm in arms {
                collect_type_refs_from_expr(&arm.body, out);
            }
        }
        Expr::Apply { args, .. } => {
            for a in args {
                collect_type_refs_from_expr(a, out);
            }
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                collect_type_refs_from_expr(item, out);
            }
        }
        Expr::Raw(tokens) => {
            for tok in tokens {
                if is_user_type_name(tok) {
                    out.insert(tok.clone());
                }
            }
        }
        // Literals never contain type references
        Expr::Literal(_) => {}
    }
}

/// Find the value for a feature_max constant from the AST.
fn find_feature_max_value(source: &assura_parser::ast::SourceFile, name: &str) -> String {
    for decl in &source.decls {
        if let Decl::Block {
            kind,
            name: n,
            value,
            body,
        } = &decl.node
            && kind == "feature_max"
            && n == name
        {
            // First, try the inline value field.
            // The value tokens may include a type annotation: `["Nat", "=", "280"]`
            // from `feature_max X: Nat = 280`. Extract after the `=`.
            if let Some(val_tokens) = value {
                if let Some(eq_pos) = val_tokens.iter().position(|t| t == "=") {
                    let after_eq = &val_tokens[eq_pos + 1..];
                    if !after_eq.is_empty() {
                        return after_eq.join(" ");
                    }
                } else if val_tokens.len() == 1 {
                    // Single-token value without `=` (e.g., just a number)
                    return val_tokens[0].clone();
                }
            }
            // Fallback: try to extract from body clauses
            for clause in body {
                let val = expr_to_rust_static(&clause.body);
                if !val.is_empty() && val != "()" {
                    return val;
                }
            }
        }
    }
    // No value found: emit a compile_error! so generated code fails to
    // build rather than silently using 0, which hides missing definitions.
    format!("compile_error!(\"feature_max `{name}` has no value\")")
}

/// Convert an Expr to a Rust expression for use in const context.
pub fn expr_to_rust_static(expr: &Expr) -> String {
    match expr {
        Expr::Literal(lit) => match lit {
            Literal::Int(s) | Literal::Float(s) => s.clone(),
            Literal::Str(s) => format!("\"{s}\""),
            Literal::Bool(b) => b.to_string(),
        },
        Expr::Ident(s) => s.clone(),
        Expr::BinOp { lhs, op, rhs } => {
            let op_s = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Mod => "%",
                BinOp::And => "&&",
                BinOp::Or => "||",
                BinOp::Eq => "==",
                BinOp::Neq => "!=",
                BinOp::Lt => "<",
                BinOp::Gt => ">",
                BinOp::Lte => "<=",
                BinOp::Gte => ">=",
                BinOp::Implies => "/* implies */",
                BinOp::In => "/* in */",
                BinOp::NotIn => "/* not in */",
                BinOp::Concat => "/* ++ */",
                BinOp::Range => "..",
            };
            format!(
                "({} {op_s} {})",
                expr_to_rust_static(lhs),
                expr_to_rust_static(rhs)
            )
        }
        Expr::Paren(inner) => expr_to_rust_static(inner),
        Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: e,
        } => {
            let inner = expr_to_rust_static(e);
            format!("-{inner}")
        }
        Expr::UnaryOp {
            op: UnaryOp::Not,
            expr: e,
        } => {
            let inner = expr_to_rust_static(e);
            format!("!{inner}")
        }
        Expr::Field(receiver, field) => {
            let recv = expr_to_rust_static(receiver);
            format!("{recv}.{field}")
        }
        Expr::Call { func, args } => {
            let f = expr_to_rust_static(func);
            let a: Vec<String> = args.iter().map(expr_to_rust_static).collect();
            format!("{f}({})", a.join(", "))
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let recv = expr_to_rust_static(receiver);
            let a: Vec<String> = args.iter().map(expr_to_rust_static).collect();
            format!("{recv}.{method}({})", a.join(", "))
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = expr_to_rust_static(cond);
            let t = expr_to_rust_static(then_branch);
            if let Some(e) = else_branch {
                let e = expr_to_rust_static(e);
                format!("if {c} {{ {t} }} else {{ {e} }}")
            } else {
                format!("if {c} {{ {t} }}")
            }
        }
        Expr::Index { expr: e, index } => {
            let base = expr_to_rust_static(e);
            let idx = expr_to_rust_static(index);
            format!("{base}[{idx}]")
        }
        Expr::Old(inner) => {
            // old() is a verification concept; in generated Rust, just emit the inner expr
            expr_to_rust_static(inner)
        }
        Expr::Ghost(inner) => {
            // Ghost expressions are erased at runtime; emit as comment
            let inner_s = expr_to_rust_static(inner);
            format!("/* ghost: {inner_s} */ ()")
        }
        Expr::Cast { expr: e, ty } => {
            let inner = expr_to_rust_static(e);
            format!("({inner} as {ty})")
        }
        Expr::List(items) => {
            let elems: Vec<String> = items.iter().map(expr_to_rust_static).collect();
            format!("vec![{}]", elems.join(", "))
        }
        Expr::Tuple(items) => {
            let elems: Vec<String> = items.iter().map(expr_to_rust_static).collect();
            format!("({})", elems.join(", "))
        }
        Expr::Let { name, value, body } => {
            let v = expr_to_rust_static(value);
            let b = expr_to_rust_static(body);
            format!("{{ let {name} = {v}; {b} }}")
        }
        Expr::Match { scrutinee, arms } => {
            let scrut = expr_to_rust_static(scrutinee);
            let arm_strs: Vec<String> = arms
                .iter()
                .map(|arm| {
                    let pat = match &arm.pattern {
                        assura_parser::ast::Pattern::Ident(s) => s.clone(),
                        assura_parser::ast::Pattern::Wildcard => "_".to_string(),
                        assura_parser::ast::Pattern::Literal(lit) => match lit {
                            Literal::Int(s) | Literal::Float(s) => s.clone(),
                            Literal::Str(s) => format!("\"{s}\""),
                            Literal::Bool(b) => b.to_string(),
                        },
                        assura_parser::ast::Pattern::Constructor { name, fields } => {
                            if fields.is_empty() {
                                name.clone()
                            } else {
                                format!("{name}(..)")
                            }
                        }
                        assura_parser::ast::Pattern::Tuple(pats) => {
                            let ps: Vec<&str> = pats.iter().map(|_| "_").collect();
                            format!("({})", ps.join(", "))
                        }
                    };
                    let body = expr_to_rust_static(&arm.body);
                    format!("{pat} => {body}")
                })
                .collect();
            format!("match {scrut} {{ {} }}", arm_strs.join(", "))
        }
        Expr::Block(exprs) => {
            let strs: Vec<String> = exprs.iter().map(expr_to_rust_static).collect();
            strs.join(" ")
        }
        Expr::Forall { var, domain, body } => {
            // Verification-only; emit as a comment in generated Rust
            let d = expr_to_rust_static(domain);
            let b = expr_to_rust_static(body);
            format!("/* forall {var} in {d}: {b} */ true")
        }
        Expr::Exists { var, domain, body } => {
            let d = expr_to_rust_static(domain);
            let b = expr_to_rust_static(body);
            format!("/* exists {var} in {d}: {b} */ true")
        }
        Expr::Apply { lemma_name, args } => {
            // Verification-only; emit as comment
            let a: Vec<String> = args.iter().map(expr_to_rust_static).collect();
            format!("/* apply {lemma_name}({}) */ ()", a.join(", "))
        }
        Expr::Raw(tokens) => {
            // Try to extract a simple numeric literal from raw tokens
            let clean: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();
            if clean.len() == 1 {
                return clean[0].to_string();
            }
            // For "= value" patterns (common in feature_max)
            if clean.len() >= 2 && clean[0] == "=" {
                return clean[1..].join(" ");
            }
            clean.join(" ")
        }
    }
}

/// Extract the base type from a refined type token sequence like
/// `["n", ":", "Int", "|", "n", ">", "0"]` -> "i64".
fn extract_base_type_from_refined(tokens: &[String]) -> String {
    // Look for a token after ':' that starts with uppercase
    let mut after_colon = false;
    for tok in tokens {
        if tok == ":" {
            after_colon = true;
            continue;
        }
        if after_colon {
            if tok.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                return map_type_token(tok).to_string();
            }
            // Not a type, stop looking
            after_colon = false;
        }
    }
    // Fallback: just use the first type-looking token
    for tok in tokens {
        if tok.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            return map_type_token(tok).to_string();
        }
    }
    "i64".to_string()
}

// ---------------------------------------------------------------------------
// Enum definitions
// ---------------------------------------------------------------------------

fn generate_enum_def(e: &EnumDef, code: &mut String) {
    let tps = if e.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", e.type_params.join(", "))
    };

    code.push_str(&format!(
        "#[derive(Debug, Clone, PartialEq)]\npub enum {}{tps} {{\n",
        e.name
    ));
    for v in &e.variants {
        if v.fields.is_empty() {
            code.push_str(&format!("    {},\n", v.name));
        } else {
            let fields: Vec<String> = v
                .fields
                .iter()
                .map(|f| map_type_token(f).to_string())
                .collect();
            code.push_str(&format!("    {}({}),\n", v.name, fields.join(", ")));
        }
    }
    code.push_str("}\n\n");

    // Generate Display implementation for non-generic enums
    if e.type_params.is_empty() {
        code.push_str(&format!("impl std::fmt::Display for {} {{\n", e.name));
        code.push_str("    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n");
        code.push_str("        match self {\n");
        for v in &e.variants {
            if v.fields.is_empty() {
                code.push_str(&format!(
                    "            {}::{} => write!(f, \"{}\"),\n",
                    e.name, v.name, v.name
                ));
            } else {
                let underscores: Vec<&str> = (0..v.fields.len()).map(|_| "_").collect();
                code.push_str(&format!(
                    "            {}::{}({}) => write!(f, \"{}(...)\"),\n",
                    e.name,
                    v.name,
                    underscores.join(", "),
                    v.name
                ));
            }
        }
        code.push_str("        }\n");
        code.push_str("    }\n");
        code.push_str("}\n\n");
    }

    // Generate exhaustiveness check: a match with no wildcard arm.
    // Rust's compiler will error if a variant is added but not covered,
    // catching missing cases at compile time rather than runtime.
    if !e.variants.is_empty() && e.type_params.is_empty() {
        code.push_str(&format!(
            "/// Compile-time exhaustiveness check for `{}`.\n",
            e.name
        ));
        code.push_str(
            "/// Adding a variant without updating all match sites causes a build error.\n",
        );
        code.push_str(&format!(
            "#[allow(dead_code)]\nfn __exhaustive_check_{}(v: &{}) -> &'static str {{\n",
            e.name.to_lowercase(),
            e.name
        ));
        code.push_str("    match v {\n");
        for v in &e.variants {
            if v.fields.is_empty() {
                code.push_str(&format!(
                    "        {}::{} => \"{}\",\n",
                    e.name, v.name, v.name
                ));
            } else {
                let underscores: Vec<&str> = (0..v.fields.len()).map(|_| "_").collect();
                code.push_str(&format!(
                    "        {}::{}({}) => \"{}\",\n",
                    e.name,
                    v.name,
                    underscores.join(", "),
                    v.name
                ));
            }
        }
        code.push_str("    }\n");
        code.push_str("}\n\n");
    }
}

// ---------------------------------------------------------------------------
// Contract declarations
// ---------------------------------------------------------------------------

/// Generate the body of a contract as standalone module contents (no `pub mod`
/// wrapper). Used in multi-file mode where each contract gets its own `.rs` file.
fn generate_contract_contents(c: &ContractDecl, code: &mut String) {
    // Interface contracts become traits even in multi-file mode
    let is_interface = c
        .clauses
        .iter()
        .any(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "interface"));
    if is_interface {
        generate_interface_trait_from_contract(c, code);
        return;
    }

    let implements: Vec<String> = c
        .clauses
        .iter()
        .filter(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "implements"))
        .filter_map(|cl| match &cl.body {
            Expr::Ident(name) => Some(name.clone()),
            Expr::Raw(tokens) if tokens.len() == 1 => Some(tokens[0].clone()),
            _ => None,
        })
        .collect();

    let tps = if c.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", c.type_params.join(", "))
    };

    let mut input_params: Vec<(String, String)> = Vec::new();
    let mut output_type = "()".to_string();
    let mut requires_exprs: Vec<String> = Vec::new();
    let mut ensures_exprs: Vec<String> = Vec::new();
    let mut effects: Vec<String> = Vec::new();
    let mut modifies: Vec<String> = Vec::new();
    let mut invariants: Vec<String> = Vec::new();

    for clause in &c.clauses {
        match &clause.kind {
            ClauseKind::Input => extract_input_params(&clause.body, &mut input_params),
            ClauseKind::Output => output_type = extract_output_type(&clause.body),
            ClauseKind::Requires => requires_exprs.push(expr_to_rust(&clause.body)),
            ClauseKind::Ensures => ensures_exprs.push(expr_to_rust(&clause.body)),
            ClauseKind::Effects => effects.push(expr_to_rust(&clause.body)),
            ClauseKind::Modifies => modifies.push(expr_to_rust(&clause.body)),
            ClauseKind::Invariant => invariants.push(expr_to_rust(&clause.body)),
            ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Other(_) => {}
        }
    }

    // Generate error enum if errors clause is present
    let error_variants = collect_error_variants(&c.clauses);
    let error_enum_name = if !error_variants.is_empty() {
        let name = format!("{}Error", c.name);
        generate_error_enum(&c.name, &error_variants, code);
        Some(name)
    } else {
        None
    };

    // Determine return type: wrap in Result when errors are declared
    let return_type = if let Some(ref err_name) = error_enum_name {
        format!("Result<{output_type}, {err_name}>")
    } else {
        output_type.clone()
    };

    for req in &requires_exprs {
        code.push_str(&format!("/// Requires: {req}\n"));
    }
    for eff in &effects {
        code.push_str(&format!("/// Effects: {eff}\n"));
    }
    for m in &modifies {
        code.push_str(&format!("/// Modifies: {m}\n"));
    }

    let params_s: String = input_params
        .iter()
        .map(|(name, ty)| format!("{name}: {ty}"))
        .collect::<Vec<_>>()
        .join(", ");

    code.push_str(&format!(
        "pub fn check{tps}({params_s}) -> {return_type} {{\n"
    ));

    for clause in &c.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                code.push_str(&format!("    let __old_{var} = {rust_expr}.clone();\n"));
            }
        }
    }

    for req in &requires_exprs {
        generate_debug_assert(code, req, "requires");
    }

    if ensures_exprs.is_empty() && invariants.is_empty() {
        code.push_str("    todo!(\"implementation provided by AI agent\")\n");
    } else {
        code.push_str(&format!(
            "    let __result: {output_type} = todo!(\"implementation provided by AI agent\");\n"
        ));
        for ens in &ensures_exprs {
            generate_debug_assert(code, ens, "ensures");
        }
        for inv in &invariants {
            generate_debug_assert(code, inv, "invariant");
        }
        if error_enum_name.is_some() {
            code.push_str("    Ok(__result)\n");
        } else {
            code.push_str("    __result\n");
        }
    }
    code.push_str("}\n");

    if !implements.is_empty() {
        code.push_str(&format!("\npub struct {}{tps};\n\n", c.name));
        for iface in &implements {
            code.push_str(&format!("impl{tps} {iface} for {}{tps} {{\n", c.name));
            for clause in &c.clauses {
                if let ClauseKind::Other(k) = &clause.kind
                    && k == "method"
                {
                    let method_name = match &clause.body {
                        Expr::Ident(n) => Some(n.as_str()),
                        Expr::Raw(tokens) if tokens.len() == 1 => Some(tokens[0].as_str()),
                        _ => None,
                    };
                    if let Some(method_name) = method_name {
                        code.push_str(&format!("    fn {method_name}(&self) {{ todo!() }}\n"));
                    }
                }
            }
            code.push_str("}\n");
        }
    }
}

fn generate_contract(c: &ContractDecl, code: &mut String) {
    // Check if this contract is an interface declaration
    let is_interface = c
        .clauses
        .iter()
        .any(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "interface"));
    if is_interface {
        generate_interface_trait_from_contract(c, code);
        return;
    }

    // Check if this contract implements an interface
    let implements: Vec<String> = c
        .clauses
        .iter()
        .filter(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "implements"))
        .filter_map(|cl| match &cl.body {
            Expr::Ident(name) => Some(name.clone()),
            Expr::Raw(tokens) if tokens.len() == 1 => Some(tokens[0].clone()),
            _ => None,
        })
        .collect();

    let tps = if c.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", c.type_params.join(", "))
    };

    code.push_str(&format!(
        "/// Contract: {}\npub mod contract_{} {{\n",
        c.name,
        c.name.to_lowercase()
    ));

    // Extract input params and output type from clauses
    let mut input_params: Vec<(String, String)> = Vec::new();
    let mut output_type = "()".to_string();
    let mut requires_exprs: Vec<String> = Vec::new();
    let mut ensures_exprs: Vec<String> = Vec::new();

    let mut effects: Vec<String> = Vec::new();
    let mut modifies: Vec<String> = Vec::new();
    let mut invariants: Vec<String> = Vec::new();

    for clause in &c.clauses {
        match &clause.kind {
            ClauseKind::Input => {
                extract_input_params(&clause.body, &mut input_params);
            }
            ClauseKind::Output => {
                output_type = extract_output_type(&clause.body);
            }
            ClauseKind::Requires => {
                requires_exprs.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Ensures => {
                ensures_exprs.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Effects => {
                effects.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Modifies => {
                modifies.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Invariant => {
                invariants.push(expr_to_rust(&clause.body));
            }
            // Other clause kinds don't produce direct codegen output.
            ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Other(_) => {}
        }
    }

    // Generate error enum if errors clause is present
    let error_variants = collect_error_variants(&c.clauses);
    let error_enum_name = if !error_variants.is_empty() {
        let name = format!("{}Error", c.name);
        code.push_str("    ");
        // Generate the enum inside the module (indented)
        let mut enum_code = String::new();
        generate_error_enum(&c.name, &error_variants, &mut enum_code);
        // Indent each line for the module context
        for line in enum_code.lines() {
            code.push_str(&format!("    {line}\n"));
        }
        code.push('\n');
        Some(name)
    } else {
        None
    };

    // Determine return type
    let return_type = if let Some(ref err_name) = error_enum_name {
        format!("Result<{output_type}, {err_name}>")
    } else {
        output_type.clone()
    };

    // Generate doc comments for requires, effects, and modifies
    for req in &requires_exprs {
        code.push_str(&format!("    /// Requires: {req}\n"));
    }
    for eff in &effects {
        code.push_str(&format!("    /// Effects: {eff}\n"));
    }
    for m in &modifies {
        code.push_str(&format!("    /// Modifies: {m}\n"));
    }

    // Generate the contract function signature
    let params_s: String = input_params
        .iter()
        .map(|(name, ty)| format!("{name}: {ty}"))
        .collect::<Vec<_>>()
        .join(", ");

    code.push_str(&format!(
        "    pub fn check{tps}({params_s}) -> {return_type} {{\n"
    ));

    // Collect old() expressions from ensures clauses and save pre-state values
    for clause in &c.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                code.push_str(&format!("        let __old_{var} = {rust_expr}.clone();\n"));
            }
        }
    }

    // Generate requires assertions
    for req in &requires_exprs {
        generate_debug_assert_indented(code, req, "requires", 2);
    }

    if ensures_exprs.is_empty() && invariants.is_empty() {
        code.push_str("        todo!(\"implementation provided by AI agent\")\n");
    } else {
        code.push_str(&format!(
            "        let __result: {output_type} = todo!(\"implementation provided by AI agent\");\n"
        ));
        for ens in &ensures_exprs {
            generate_debug_assert_indented(code, ens, "ensures", 2);
        }
        for inv in &invariants {
            generate_debug_assert_indented(code, inv, "invariant", 2);
        }
        if error_enum_name.is_some() {
            code.push_str("        Ok(__result)\n");
        } else {
            code.push_str("        __result\n");
        }
    }
    code.push_str("    }\n");

    // Generate struct + impl Trait if the contract implements an interface
    if !implements.is_empty() {
        // Generate a struct for this contract
        code.push_str(&format!("\n    pub struct {}{tps};\n\n", c.name));
        // Generate impl blocks for each implemented trait
        for iface in &implements {
            code.push_str(&format!("    impl{tps} {iface} for {}{tps} {{\n", c.name));
            // Extract method clauses and generate stubs
            for clause in &c.clauses {
                if let ClauseKind::Other(k) = &clause.kind
                    && k == "method"
                {
                    let method_name = match &clause.body {
                        Expr::Ident(n) => Some(n.as_str()),
                        Expr::Raw(tokens) if tokens.len() == 1 => Some(tokens[0].as_str()),
                        _ => None,
                    };
                    if let Some(method_name) = method_name {
                        code.push_str(&format!("        fn {method_name}(&self) {{ todo!() }}\n"));
                    }
                }
            }
            code.push_str("    }\n");
        }
    }

    code.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// S009: Proptest generation from contracts
// ---------------------------------------------------------------------------

/// Map a Rust type to a proptest strategy expression.
fn proptest_strategy_for_type(rust_type: &str) -> String {
    match rust_type {
        "i64" => "proptest::prelude::any::<i64>()".to_string(),
        "u64" => "proptest::prelude::any::<u64>()".to_string(),
        "i32" => "proptest::prelude::any::<i32>()".to_string(),
        "u32" => "proptest::prelude::any::<u32>()".to_string(),
        "i16" => "proptest::prelude::any::<i16>()".to_string(),
        "u16" => "proptest::prelude::any::<u16>()".to_string(),
        "i8" => "proptest::prelude::any::<i8>()".to_string(),
        "u8" => "proptest::prelude::any::<u8>()".to_string(),
        "f64" => "proptest::prelude::any::<f64>()".to_string(),
        "f32" => "proptest::prelude::any::<f32>()".to_string(),
        "bool" => "proptest::prelude::any::<bool>()".to_string(),
        "usize" => "proptest::prelude::any::<usize>()".to_string(),
        "isize" => "proptest::prelude::any::<isize>()".to_string(),
        _ => format!("proptest::prelude::any::<{rust_type}>()"),
    }
}

/// Try to refine a proptest strategy based on a requires constraint.
///
/// Recognizes patterns like:
///   - `x != 0` -> range that excludes zero
///   - `x > 0` / `x >= 1` -> positive range
///   - `x < N` / `x <= N` -> bounded range
///
/// Returns `Some((param_name, refined_strategy))` if the constraint can be
/// encoded as a generator, or `None` if it should remain a filter/assumption.
fn try_refine_strategy(requires_expr: &Expr) -> Option<(String, String)> {
    if let Expr::BinOp { lhs, op, rhs } = requires_expr {
        let param = match lhs.as_ref() {
            Expr::Ident(name) => name.clone(),
            _ => return None,
        };

        match (op, rhs.as_ref()) {
            // x != 0 -> filter: use 1..=MAX for unsigned, two ranges for signed
            (BinOp::Neq, Expr::Literal(Literal::Int(val))) if val == "0" => {
                Some((param, "1i64..=i64::MAX".to_string()))
            }
            // x > 0 -> 1..=MAX
            (BinOp::Gt, Expr::Literal(Literal::Int(val))) if val == "0" => {
                Some((param, "1i64..=i64::MAX".to_string()))
            }
            // x >= 0 -> 0..=MAX
            (BinOp::Gte, Expr::Literal(Literal::Int(val))) if val == "0" => {
                Some((param, "0i64..=i64::MAX".to_string()))
            }
            // x >= 1 -> 1..=MAX
            (BinOp::Gte, Expr::Literal(Literal::Int(val))) if val == "1" => {
                Some((param, "1i64..=i64::MAX".to_string()))
            }
            // x < N -> MIN..N
            (BinOp::Lt, Expr::Literal(Literal::Int(val))) => {
                Some((param, format!("i64::MIN..{val}i64")))
            }
            // x <= N -> MIN..=N
            (BinOp::Lte, Expr::Literal(Literal::Int(val))) => {
                Some((param, format!("i64::MIN..={val}i64")))
            }
            _ => None,
        }
    } else {
        None
    }
}

/// Check if a contract has testable content (inputs + ensures/requires).
fn contract_is_testable(c: &ContractDecl) -> bool {
    let has_input = c
        .clauses
        .iter()
        .any(|cl| matches!(cl.kind, ClauseKind::Input));
    let has_ensures = c
        .clauses
        .iter()
        .any(|cl| matches!(cl.kind, ClauseKind::Ensures));
    has_input && has_ensures
}

/// Generate proptest property-based tests for a contract.
///
/// For each contract with input params and ensures clauses, generates a
/// `proptest!` block that:
/// - Uses the contract's input types as proptest strategies
/// - Refines strategies based on requires constraints where possible
/// - Falls back to `prop_assume!` for complex requires constraints
/// - Asserts ensures clauses with `prop_assert!`
fn generate_proptest_for_contract(c: &ContractDecl, code: &mut String) {
    if !contract_is_testable(c) {
        return;
    }

    let mut input_params: Vec<(String, String)> = Vec::new();
    let mut requires_exprs: Vec<String> = Vec::new();
    let mut requires_ast: Vec<&Expr> = Vec::new();
    let mut ensures_exprs: Vec<String> = Vec::new();

    for clause in &c.clauses {
        match &clause.kind {
            ClauseKind::Input => extract_input_params(&clause.body, &mut input_params),
            ClauseKind::Requires => {
                requires_exprs.push(expr_to_rust(&clause.body));
                requires_ast.push(&clause.body);
            }
            ClauseKind::Ensures => {
                ensures_exprs.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Output
            | ClauseKind::Effects
            | ClauseKind::Modifies
            | ClauseKind::Invariant
            | ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Other(_) => {}
        }
    }

    if input_params.is_empty() || ensures_exprs.is_empty() {
        return;
    }

    // Build refined strategies from requires constraints
    let mut refined: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut unrefined_requires: Vec<String> = Vec::new();
    for (i, ast) in requires_ast.iter().enumerate() {
        if let Some((param, strategy)) = try_refine_strategy(ast) {
            refined.insert(param, strategy);
        } else {
            unrefined_requires.push(requires_exprs[i].clone());
        }
    }

    let fn_name = c.name.to_lowercase();
    let contract_mod = format!("contract_{fn_name}");

    code.push_str("#[cfg(test)]\n");
    code.push_str(&format!("mod proptest_{fn_name} {{\n"));
    code.push_str("    use proptest::prelude::*;\n\n");
    code.push_str("    proptest! {\n");
    code.push_str("        #[test]\n");

    // Build parameter list with strategies
    let param_strs: Vec<String> = input_params
        .iter()
        .map(|(name, ty)| {
            if let Some(strategy) = refined.get(name) {
                format!("{name} in {strategy}")
            } else {
                let strategy = proptest_strategy_for_type(ty);
                format!("{name} in {strategy}")
            }
        })
        .collect();
    code.push_str(&format!(
        "        fn test_{fn_name}({}) {{\n",
        param_strs.join(", ")
    ));

    // Emit prop_assume! for requires that could not be encoded as strategies
    for req in &unrefined_requires {
        code.push_str(&format!("            prop_assume!({req});\n"));
    }

    // Call the contract check function
    let call_args: Vec<&str> = input_params.iter().map(|(n, _)| n.as_str()).collect();
    code.push_str(&format!(
        "            let result = super::{contract_mod}::check({});\n",
        call_args.join(", ")
    ));

    // Emit prop_assert! for each ensures clause
    for ens in &ensures_exprs {
        code.push_str(&format!("            prop_assert!({ens});\n"));
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code.push_str("}\n\n");
}

/// Generate proptest for a contract in multi-file mode (the test module
/// is inside the contract's own .rs file, so the call is `super::check()`).
fn generate_proptest_for_contract_contents(c: &ContractDecl, code: &mut String) {
    if !contract_is_testable(c) {
        return;
    }

    let mut input_params: Vec<(String, String)> = Vec::new();
    let mut requires_exprs: Vec<String> = Vec::new();
    let mut requires_ast: Vec<&Expr> = Vec::new();
    let mut ensures_exprs: Vec<String> = Vec::new();

    for clause in &c.clauses {
        match &clause.kind {
            ClauseKind::Input => extract_input_params(&clause.body, &mut input_params),
            ClauseKind::Requires => {
                requires_exprs.push(expr_to_rust(&clause.body));
                requires_ast.push(&clause.body);
            }
            ClauseKind::Ensures => {
                ensures_exprs.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Output
            | ClauseKind::Effects
            | ClauseKind::Modifies
            | ClauseKind::Invariant
            | ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Other(_) => {}
        }
    }

    if input_params.is_empty() || ensures_exprs.is_empty() {
        return;
    }

    let mut refined: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut unrefined_requires: Vec<String> = Vec::new();
    for (i, ast) in requires_ast.iter().enumerate() {
        if let Some((param, strategy)) = try_refine_strategy(ast) {
            refined.insert(param, strategy);
        } else {
            unrefined_requires.push(requires_exprs[i].clone());
        }
    }

    let fn_name = c.name.to_lowercase();

    code.push_str("#[cfg(test)]\n");
    code.push_str(&format!("mod proptest_{fn_name} {{\n"));
    code.push_str("    use proptest::prelude::*;\n\n");
    code.push_str("    proptest! {\n");
    code.push_str("        #[test]\n");

    let param_strs: Vec<String> = input_params
        .iter()
        .map(|(name, ty)| {
            if let Some(strategy) = refined.get(name) {
                format!("{name} in {strategy}")
            } else {
                let strategy = proptest_strategy_for_type(ty);
                format!("{name} in {strategy}")
            }
        })
        .collect();
    code.push_str(&format!(
        "        fn test_{fn_name}({}) {{\n",
        param_strs.join(", ")
    ));

    for req in &unrefined_requires {
        code.push_str(&format!("            prop_assume!({req});\n"));
    }

    let call_args: Vec<&str> = input_params.iter().map(|(n, _)| n.as_str()).collect();
    code.push_str(&format!(
        "            let result = super::check({});\n",
        call_args.join(", ")
    ));

    for ens in &ensures_exprs {
        code.push_str(&format!("            prop_assert!({ens});\n"));
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code.push_str("}\n\n");
}

/// Check if any contract in the source is testable (needs proptest).
/// Check if any declaration has an `errors` clause that will generate error types.
fn source_has_error_types(source: &assura_parser::ast::SourceFile) -> bool {
    source.decls.iter().any(|decl| match &decl.node {
        Decl::Contract(c) => c.clauses.iter().any(|cl| cl.kind == ClauseKind::Errors),
        Decl::FnDef(f) => f.clauses.iter().any(|cl| cl.kind == ClauseKind::Errors),
        _ => false,
    })
}

fn source_has_testable_contracts(source: &assura_parser::ast::SourceFile) -> bool {
    source.decls.iter().any(|decl| {
        if let Decl::Contract(c) = &decl.node {
            contract_is_testable(c)
        } else {
            false
        }
    })
}

/// Generate a Rust trait from a contract that has an `interface` clause.
fn generate_interface_trait_from_contract(c: &ContractDecl, code: &mut String) {
    let tps = if c.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", c.type_params.join(", "))
    };

    // Collect extends (supertraits)
    let extends: Vec<String> = c
        .clauses
        .iter()
        .filter(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "extends"))
        .filter_map(|cl| {
            if let Expr::Ident(name) = &cl.body {
                Some(name.clone())
            } else {
                None
            }
        })
        .collect();

    if extends.is_empty() {
        code.push_str(&format!(
            "/// Interface contract: {}\npub trait {}{tps} {{\n",
            c.name, c.name
        ));
    } else {
        let bounds = extends.join(" + ");
        code.push_str(&format!(
            "/// Interface contract: {}\npub trait {}{tps}: {bounds} {{\n",
            c.name, c.name
        ));
    }

    // Generate trait methods from `method` clauses
    for clause in &c.clauses {
        if let ClauseKind::Other(k) = &clause.kind
            && k == "method"
        {
            generate_trait_method(&clause.body, code);
        }
    }

    // Generate invariant as a provided method
    for clause in &c.clauses {
        if matches!(clause.kind, ClauseKind::Invariant | ClauseKind::Ensures) {
            let expr = expr_to_rust(&clause.body);
            code.push_str(&format!(
                "    /// Interface invariant\n    fn check_invariant(&self) {{ debug_assert!({expr}); }}\n\n"
            ));
        }
    }

    code.push_str("}\n\n");
}

/// Extract `(name, rust_type)` pairs from an input clause body.
///
/// Uses the shared `extract_clause_params` from assura-parser, then maps
/// Assura type tokens to Rust types via `map_type_token`/`map_type_tokens`.
fn extract_input_params(body: &Expr, params: &mut Vec<(String, String)>) {
    use assura_parser::ast::extract_clause_params;
    for param in extract_clause_params(body) {
        let rust_ty = if param.ty.is_empty() {
            "i64".to_string()
        } else {
            // Filter out "linear" modifier from type tokens
            let filtered: Vec<String> = param
                .ty
                .into_iter()
                .filter(|t| t.as_str() != "linear")
                .collect();
            if filtered.is_empty() {
                "i64".to_string()
            } else if filtered.len() == 1 {
                map_type_token(&filtered[0]).to_string()
            } else {
                map_type_tokens(&filtered)
            }
        };
        params.push((param.name, rust_ty));
    }
}

/// Extract the Rust return type from an output clause body.
fn extract_output_type(body: &Expr) -> String {
    match body {
        Expr::Call { args, .. } => {
            // output(result: Int) => parse the cast or ident in args
            for arg in args {
                match arg {
                    Expr::Cast { ty, .. } => return map_type_token(ty).to_string(),
                    Expr::Ident(name) => return map_type_token(name).to_string(),
                    Expr::Paren(inner) => return extract_output_type(inner),
                    other => {
                        let ty = extract_output_type(other);
                        if ty != "()" {
                            return ty;
                        }
                    }
                }
            }
            "()".to_string()
        }
        Expr::Cast { ty, .. } => map_type_token(ty).to_string(),
        Expr::Ident(name) => map_type_token(name).to_string(),
        Expr::Paren(inner) => extract_output_type(inner),
        Expr::Tuple(items) | Expr::Block(items) => {
            // First typed element wins (e.g., (result: Int) parsed as tuple)
            for item in items {
                let ty = extract_output_type(item);
                if ty != "()" {
                    return ty;
                }
            }
            "()".to_string()
        }
        Expr::Raw(tokens) => {
            // Look for the type after ":" or "as"
            for (i, tok) in tokens.iter().enumerate() {
                if (tok == ":" || tok == "as") && i + 1 < tokens.len() {
                    let type_tokens = &tokens[i + 1..];
                    return map_type_tokens(type_tokens);
                }
            }
            if tokens.len() == 1 {
                return map_type_token(&tokens[0]).to_string();
            }
            "()".to_string()
        }
        // Expressions that can carry type info through structure
        Expr::If { then_branch, .. } => extract_output_type(then_branch),
        Expr::Let { body, .. } => extract_output_type(body),
        Expr::Match { arms, .. } => {
            if let Some(arm) = arms.first() {
                extract_output_type(&arm.body)
            } else {
                "()".to_string()
            }
        }
        Expr::Old(inner) | Expr::Ghost(inner) | Expr::UnaryOp { expr: inner, .. } => {
            extract_output_type(inner)
        }
        // These expression forms do not carry type annotations;
        // the output clause type cannot be determined from them.
        Expr::Literal(_)
        | Expr::Field(_, _)
        | Expr::MethodCall { .. }
        | Expr::Index { .. }
        | Expr::BinOp { .. }
        | Expr::Forall { .. }
        | Expr::Exists { .. }
        | Expr::List(_)
        | Expr::Apply { .. } => "()".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Error type generation (P004)
// ---------------------------------------------------------------------------

/// Extract error variant names from an `errors` clause body.
///
/// The errors clause body may be:
/// - `Expr::Raw(["DivByZero", ",", "Overflow"])` -> vec!["DivByZero", "Overflow"]
/// - `Expr::Ident("DivByZero")` -> vec!["DivByZero"]
/// - `Expr::Tuple([Ident("A"), Ident("B")])` -> vec!["A", "B"]
fn extract_error_variants(body: &Expr) -> Vec<String> {
    match body {
        Expr::Ident(name) => vec![name.clone()],
        Expr::Tuple(items) | Expr::List(items) | Expr::Block(items) => items
            .iter()
            .flat_map(extract_error_variants)
            .collect(),
        Expr::Raw(tokens) => tokens
            .iter()
            .filter(|t| {
                let s = t.as_str();
                s != "," && s != "(" && s != ")" && s != "{" && s != "}"
            })
            .cloned()
            .collect(),
        Expr::Paren(inner) | Expr::Ghost(inner) | Expr::Old(inner) => {
            extract_error_variants(inner)
        }
        Expr::Call { args, .. } => args.iter().flat_map(extract_error_variants).collect(),
        // These expression forms cannot meaningfully contain error variant names
        Expr::Literal(_)
        | Expr::Field(_, _)
        | Expr::MethodCall { .. }
        | Expr::Index { .. }
        | Expr::BinOp { .. }
        | Expr::UnaryOp { .. }
        | Expr::Cast { .. }
        | Expr::Forall { .. }
        | Expr::Exists { .. }
        | Expr::If { .. }
        | Expr::Let { .. }
        | Expr::Match { .. }
        | Expr::Apply { .. } => vec![],
    }
}

/// Collect all error variants from a set of clauses.
fn collect_error_variants(clauses: &[Clause]) -> Vec<String> {
    let mut errors = Vec::new();
    for clause in clauses {
        if clause.kind == ClauseKind::Errors {
            errors.extend(extract_error_variants(&clause.body));
        }
    }
    errors
}

/// Generate a `#[derive(Debug, thiserror::Error)]` enum for contract errors.
fn generate_error_enum(contract_name: &str, variants: &[String], code: &mut String) {
    let enum_name = format!("{contract_name}Error");
    code.push_str("#[derive(Debug, thiserror::Error)]\n");
    code.push_str(&format!("pub enum {enum_name} {{\n"));
    for variant in variants {
        code.push_str(&format!("    #[error(\"{variant}\")]\n    {variant},\n"));
    }
    code.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Bind declarations (checked wrappers for existing Rust functions)
// ---------------------------------------------------------------------------

/// Generate a checked wrapper for a `bind` declaration.
///
/// A `bind` maps an existing Rust function path to an Assura contract name.
/// The generated code calls the real function and wraps it with
/// `debug_assert!` checks for `requires` and `ensures` clauses.
fn generate_bind(b: &BindDecl, code: &mut String) {
    let params_s: String = b
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, map_type_tokens(&p.ty)))
        .collect::<Vec<_>>()
        .join(", ");

    let ret = if b.return_ty.is_empty() {
        "()".to_string()
    } else {
        map_type_tokens(&b.return_ty)
    };

    let args_s: String = b
        .params
        .iter()
        .map(|p| p.name.clone())
        .collect::<Vec<_>>()
        .join(", ");

    let rust_path = &b.target_path;

    code.push_str(&format!(
        "/// Bind: {} -> {rust_path}\npub fn {}({params_s}) -> {ret} {{\n",
        b.name, b.name
    ));

    // Collect old() expressions from ensures clauses and save pre-state values
    let mut ensures_exprs: Vec<String> = Vec::new();
    for clause in &b.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                code.push_str(&format!("    let __old_{var} = {rust_expr}.clone();\n"));
            }
            ensures_exprs.push(expr_to_rust(&clause.body));
        }
    }

    // Generate requires assertions at function entry
    for clause in &b.clauses {
        if clause.kind == ClauseKind::Requires {
            let expr = expr_to_rust(&clause.body);
            generate_debug_assert(code, &expr, "requires");
        }
    }

    // Call the actual Rust function
    code.push_str(&format!(
        "    let __result: {ret} = {rust_path}({args_s});\n"
    ));

    // Generate ensures assertions on the result
    for ens in &ensures_exprs {
        generate_debug_assert(code, ens, "ensures");
    }

    code.push_str("    __result\n");
    code.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Extern declarations
// ---------------------------------------------------------------------------

fn generate_extern(ex: &ExternDecl, code: &mut String) {
    let params_s: String = ex
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, map_type_tokens(&p.ty)))
        .collect::<Vec<_>>()
        .join(", ");

    let ret = if ex.return_ty.is_empty() {
        "()".to_string()
    } else {
        map_type_tokens(&ex.return_ty)
    };

    // Generate as a regular function with contract assertions
    code.push_str(&format!(
        "/// Extern: {}\npub fn {}({params_s}) -> {ret} {{\n",
        ex.name, ex.name
    ));

    // Collect old() expressions from ensures clauses and save pre-state values
    let mut ensures_exprs: Vec<String> = Vec::new();
    for clause in &ex.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                code.push_str(&format!("    let __old_{var} = {rust_expr}.clone();\n"));
            }
            ensures_exprs.push(expr_to_rust(&clause.body));
        }
    }

    // Generate requires assertions at function entry
    for clause in &ex.clauses {
        if clause.kind == ClauseKind::Requires {
            let expr = expr_to_rust(&clause.body);
            generate_debug_assert(code, &expr, "requires");
        }
    }

    if ensures_exprs.is_empty() {
        code.push_str("    todo!(\"extern function: implementation required\")\n");
    } else {
        code.push_str(&format!(
            "    let __result: {ret} = todo!(\"extern function: implementation required\");\n"
        ));
        for ens in &ensures_exprs {
            generate_debug_assert(code, ens, "ensures");
        }
        code.push_str("    __result\n");
    }
    code.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Function definitions
// ---------------------------------------------------------------------------

fn generate_fn_def(f: &FnDef, code: &mut String) {
    let params_s: String = f
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, map_type_tokens(&p.ty)))
        .collect::<Vec<_>>()
        .join(", ");

    let ret_ty = if f.return_ty.is_empty() {
        "()".to_string()
    } else {
        map_type_tokens(&f.return_ty)
    };

    // Generate error enum if errors clause is present
    let error_variants = collect_error_variants(&f.clauses);
    let error_enum_name = if !error_variants.is_empty() {
        let name = format!("{}Error", f.name);
        generate_error_enum(&f.name, &error_variants, code);
        Some(name)
    } else {
        None
    };

    let return_type = if let Some(ref err_name) = error_enum_name {
        format!("Result<{ret_ty}, {err_name}>")
    } else {
        ret_ty.clone()
    };

    let ret_sig = if f.return_ty.is_empty() && error_enum_name.is_none() {
        String::new()
    } else {
        format!(" -> {return_type}")
    };

    code.push_str(&format!("pub fn {}({params_s}){ret_sig} {{\n", f.name));

    // Collect old() expressions from ensures clauses and save pre-state values
    let mut ensures_exprs: Vec<String> = Vec::new();
    for clause in &f.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                code.push_str(&format!("    let __old_{var} = {rust_expr}.clone();\n"));
            }
            ensures_exprs.push(expr_to_rust(&clause.body));
        }
    }

    // Generate requires assertions at function entry
    for clause in &f.clauses {
        if clause.kind == ClauseKind::Requires {
            let expr = expr_to_rust(&clause.body);
            generate_debug_assert(code, &expr, "requires");
        }
    }

    if ensures_exprs.is_empty() {
        code.push_str("    todo!(\"implementation provided by AI agent\")\n");
    } else {
        code.push_str(&format!(
            "    let __result: {ret_ty} = todo!(\"implementation provided by AI agent\");\n"
        ));
        for ens in &ensures_exprs {
            generate_debug_assert(code, ens, "ensures");
        }
        if error_enum_name.is_some() {
            code.push_str("    Ok(__result)\n");
        } else {
            code.push_str("    __result\n");
        }
    }
    code.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Service declarations
// ---------------------------------------------------------------------------

/// Generate a service operation or query method with proper parameter extraction.
///
/// Operations take `&mut self`, queries take `&self`. Both extract input params
/// and output types from their clauses for proper function signatures.
/// Extract a state name from a `self.state == StateName` pattern.
fn extract_state_comparison(body: &Expr) -> Option<String> {
    if let Expr::BinOp { lhs, op, rhs } = body
        && matches!(op, BinOp::Eq)
    {
        // Check lhs is self.state
        let is_self_state = matches!(
            lhs.as_ref(),
            Expr::Field(recv, field) if matches!(recv.as_ref(), Expr::Ident(s) if s == "self") && field == "state"
        );
        if is_self_state && let Expr::Ident(state_name) = rhs.as_ref() {
            return Some(state_name.clone());
        }
    }
    None
}

fn generate_service_method(
    code: &mut String,
    name: &str,
    clauses: &[Clause],
    is_mutation: bool,
    has_invariants: bool,
) {
    // Extract input/output from clauses
    let mut input_params: Vec<(String, String)> = Vec::new();
    let mut output_type = "()".to_string();
    let mut requires_exprs: Vec<String> = Vec::new();
    let mut ensures_exprs: Vec<String> = Vec::new();
    let mut modifies: Vec<String> = Vec::new();
    let mut invariants: Vec<String> = Vec::new();
    let mut pre_state: Option<String> = None;
    let mut post_state: Option<String> = None;

    for clause in clauses {
        match &clause.kind {
            ClauseKind::Input => {
                extract_input_params(&clause.body, &mut input_params);
            }
            ClauseKind::Output => {
                output_type = extract_output_type(&clause.body);
            }
            ClauseKind::Requires => {
                // Check for state guard pattern: requires { self.state == X }
                if let Some(state) = extract_state_comparison(&clause.body) {
                    pre_state = Some(state);
                } else {
                    requires_exprs.push(expr_to_rust(&clause.body));
                }
            }
            ClauseKind::Ensures => {
                // Check for state transition pattern: ensures { self.state == X }
                if let Some(state) = extract_state_comparison(&clause.body) {
                    post_state = Some(state);
                } else {
                    ensures_exprs.push(expr_to_rust(&clause.body));
                }
            }
            ClauseKind::Modifies => {
                modifies.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Invariant => {
                invariants.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Effects
            | ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Other(_) => {}
        }
    }

    let kind_label = if is_mutation { "Operation" } else { "Query" };
    code.push_str(&format!("        /// {kind_label}: {name}\n"));

    // Doc comments for requires/ensures/effects/modifies
    for clause in clauses {
        match clause.kind {
            ClauseKind::Requires => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("        /// Requires: {expr}\n"));
            }
            ClauseKind::Ensures => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("        /// Ensures: {expr}\n"));
            }
            ClauseKind::Effects => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("        /// Effects: {expr}\n"));
            }
            ClauseKind::Modifies => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("        /// Modifies: {expr}\n"));
            }
            // Input/Output are handled in the signature generation.
            // Other clause kinds don't produce doc comments.
            ClauseKind::Input
            | ClauseKind::Output
            | ClauseKind::Invariant
            | ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Other(_) => {}
        }
    }

    // Build function signature
    let self_param = if is_mutation { "&mut self" } else { "&self" };
    let extra_params = if input_params.is_empty() {
        String::new()
    } else {
        let ps: Vec<String> = input_params
            .iter()
            .map(|(n, t)| format!("{n}: {t}"))
            .collect();
        format!(", {}", ps.join(", "))
    };
    let ret_sig = if output_type == "()" {
        String::new()
    } else {
        format!(" -> {output_type}")
    };

    code.push_str(&format!(
        "        pub fn {name}({self_param}{extra_params}){ret_sig} {{\n"
    ));

    // Invariant check at entry
    if has_invariants {
        code.push_str("            self.check_invariant();\n");
    }

    // State pre-condition guard
    if let Some(ref state) = pre_state {
        code.push_str(&format!(
            "            debug_assert_eq!(self.state, State::{state}, \"requires state {state}\");\n"
        ));
    }

    // Requires assertions
    for req in &requires_exprs {
        generate_debug_assert_indented(code, req, "requires", 3);
    }

    if output_type == "()" {
        // State transition
        if let Some(ref state) = post_state {
            code.push_str(&format!("            self.state = State::{state};\n"));
        }
        code.push_str(&format!(
            "            todo!(\"{} implementation\")\n",
            kind_label.to_lowercase()
        ));
        // Operation-level invariant assertions
        for inv in &invariants {
            generate_debug_assert_indented(code, inv, "invariant", 3);
        }
        // Invariant check at exit (for void operations)
        if has_invariants {
            code.push_str("            self.check_invariant();\n");
        }
    } else {
        code.push_str(&format!(
            "            let __result: {output_type} = todo!(\"{} implementation\");\n",
            kind_label.to_lowercase()
        ));
        // Ensures assertions
        for ens in &ensures_exprs {
            generate_debug_assert_indented(code, ens, "ensures", 3);
        }
        // Operation-level invariant assertions
        for inv in &invariants {
            generate_debug_assert_indented(code, inv, "invariant", 3);
        }
        // State transition
        if let Some(ref state) = post_state {
            code.push_str(&format!("            self.state = State::{state};\n"));
        }
        // Invariant check at exit
        if has_invariants {
            code.push_str("            self.check_invariant();\n");
        }
        code.push_str("            __result\n");
    }

    code.push_str("        }\n\n");
}

/// Generate a service method for typestate-encoded services.
///
/// State transitions consume `self` and return `ServiceName<NewState>`.
/// Pre-state guards are enforced by the type system (the method only
/// exists on `impl ServiceName<PreState>`), so no runtime assertions.
fn generate_typestate_method(
    code: &mut String,
    service_name: &str,
    name: &str,
    clauses: &[Clause],
    is_mutation: bool,
    _has_invariants: bool,
) {
    let mut input_params: Vec<(String, String)> = Vec::new();
    let mut output_type = "()".to_string();
    let mut requires_exprs: Vec<String> = Vec::new();
    let mut ensures_exprs: Vec<String> = Vec::new();
    let mut invariants: Vec<String> = Vec::new();
    let mut post_state: Option<String> = None;

    for clause in clauses {
        match &clause.kind {
            ClauseKind::Input => {
                extract_input_params(&clause.body, &mut input_params);
            }
            ClauseKind::Output => {
                output_type = extract_output_type(&clause.body);
            }
            ClauseKind::Requires => {
                // State guards are encoded in the type, skip them
                if extract_state_comparison(&clause.body).is_none() {
                    requires_exprs.push(expr_to_rust(&clause.body));
                }
            }
            ClauseKind::Ensures => {
                if let Some(state) = extract_state_comparison(&clause.body) {
                    post_state = Some(state);
                } else {
                    ensures_exprs.push(expr_to_rust(&clause.body));
                }
            }
            ClauseKind::Invariant => {
                invariants.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Modifies
            | ClauseKind::Effects
            | ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Other(_) => {}
        }
    }

    let kind_label = if is_mutation { "Operation" } else { "Query" };
    code.push_str(&format!("/// {kind_label}: {name}\n"));

    // Doc comments
    for clause in clauses {
        match clause.kind {
            ClauseKind::Requires => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("/// Requires: {expr}\n"));
            }
            ClauseKind::Ensures => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("/// Ensures: {expr}\n"));
            }
            ClauseKind::Effects => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("/// Effects: {expr}\n"));
            }
            ClauseKind::Modifies => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("/// Modifies: {expr}\n"));
            }
            ClauseKind::Input
            | ClauseKind::Output
            | ClauseKind::Invariant
            | ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Other(_) => {}
        }
    }

    // Determine self parameter and return type based on state transition
    let has_transition = post_state.is_some();
    let self_param = if has_transition {
        "self" // consume self for state transitions
    } else if is_mutation {
        "&mut self"
    } else {
        "&self"
    };

    let extra_params = if input_params.is_empty() {
        String::new()
    } else {
        let ps: Vec<String> = input_params
            .iter()
            .map(|(n, t)| format!("{n}: {t}"))
            .collect();
        format!(", {}", ps.join(", "))
    };

    let ret_sig = if let Some(ref new_state) = post_state {
        format!(" -> {service_name}<{new_state}>")
    } else if output_type == "()" {
        String::new()
    } else {
        format!(" -> {output_type}")
    };

    code.push_str(&format!(
        "pub fn {name}({self_param}{extra_params}){ret_sig} {{\n"
    ));

    // Requires assertions (non-state-guard ones)
    for req in &requires_exprs {
        generate_debug_assert_indented(code, req, "requires", 1);
    }

    // For state transitions, todo!() coerces to the return type
    // For non-transitions, standard pattern
    // Invariant assertions (emitted before the body in all cases)
    for inv in &invariants {
        generate_debug_assert_indented(code, inv, "invariant", 1);
    }

    if post_state.is_some() || output_type == "()" {
        code.push_str(&format!(
            "    todo!(\"{} implementation\")\n",
            kind_label.to_lowercase()
        ));
    } else {
        code.push_str(&format!(
            "    let __result: {output_type} = todo!(\"{} implementation\");\n",
            kind_label.to_lowercase()
        ));
        for ens in &ensures_exprs {
            generate_debug_assert_indented(code, ens, "ensures", 1);
        }
        code.push_str("    __result\n");
    }

    code.push_str("}\n");
}

/// Collect state names from a ServiceDecl.
fn collect_service_states(s: &ServiceDecl) -> Vec<String> {
    s.items
        .iter()
        .find_map(|i| match i {
            ServiceItem::States(states) => Some(states.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Extract pre_state from a method's clauses (the state guard in requires).
fn method_pre_state(clauses: &[Clause]) -> Option<String> {
    clauses.iter().find_map(|c| {
        if matches!(c.kind, ClauseKind::Requires) {
            extract_state_comparison(&c.body)
        } else {
            None
        }
    })
}

/// Generate typestate-encoded service body (marker structs, generic struct,
/// state-specific impl blocks). Used when the service declares states.
fn generate_typestate_service_body(s: &ServiceDecl, code: &mut String) {
    let states = collect_service_states(s);
    let has_invariants = s
        .items
        .iter()
        .any(|i| matches!(i, ServiceItem::Invariant(_)));

    // Generate nested type/enum definitions
    for item in &s.items {
        match item {
            ServiceItem::TypeDef(t) => generate_type_def(t, code),
            ServiceItem::EnumDef(e) => generate_enum_def(e, code),
            ServiceItem::States(_)
            | ServiceItem::Operation { .. }
            | ServiceItem::Query { .. }
            | ServiceItem::Invariant(_)
            | ServiceItem::Other { .. } => {}
        }
    }

    // State marker structs
    for state in &states {
        code.push_str(&format!("/// State marker: {state}\npub struct {state};\n"));
    }
    code.push('\n');

    // Generic service struct with PhantomData
    code.push_str(&format!(
        "#[derive(Debug)]\npub struct {}<State> {{\n    _state: std::marker::PhantomData<State>,\n}}\n\n",
        s.name
    ));

    // Group methods by pre_state
    struct MethodRef<'a> {
        name: &'a str,
        clauses: &'a [Clause],
        is_mutation: bool,
    }

    let mut state_methods: Vec<(Option<String>, Vec<MethodRef<'_>>)> = Vec::new();
    let mut invariant_exprs: Vec<&Expr> = Vec::new();
    let mut other_items: Vec<(&str, &Expr)> = Vec::new();

    // Build ordered grouping: preserve state order from declaration
    let mut state_order: Vec<Option<String>> = Vec::new();
    // First entry: initial state (for new())
    if let Some(first) = states.first() {
        state_order.push(Some(first.clone()));
    }
    // Remaining states
    for state in states.iter().skip(1) {
        state_order.push(Some(state.clone()));
    }
    // Generic (None) for state-independent methods
    state_order.push(None);

    for key in &state_order {
        state_methods.push((key.clone(), Vec::new()));
    }

    for item in &s.items {
        match item {
            ServiceItem::Operation { name, clauses } => {
                let pre = method_pre_state(clauses);
                let method = MethodRef {
                    name,
                    clauses,
                    is_mutation: true,
                };
                if let Some(group) = state_methods.iter_mut().find(|(k, _)| *k == pre) {
                    group.1.push(method);
                } else {
                    // State not in declared list; add to generic
                    if let Some(group) = state_methods.iter_mut().find(|(k, _)| k.is_none()) {
                        group.1.push(method);
                    }
                }
            }
            ServiceItem::Query { name, clauses } => {
                let pre = method_pre_state(clauses);
                let method = MethodRef {
                    name,
                    clauses,
                    is_mutation: false,
                };
                if let Some(group) = state_methods.iter_mut().find(|(k, _)| *k == pre) {
                    group.1.push(method);
                } else {
                    if let Some(group) = state_methods.iter_mut().find(|(k, _)| k.is_none()) {
                        group.1.push(method);
                    }
                }
            }
            ServiceItem::Invariant(expr) => invariant_exprs.push(expr),
            ServiceItem::Other { kind, body } => other_items.push((kind, body)),
            ServiceItem::TypeDef(_) | ServiceItem::EnumDef(_) | ServiceItem::States(_) => {}
        }
    }

    let initial_state = states
        .first()
        .cloned()
        .unwrap_or_else(|| "Default".to_string());

    // Generate impl blocks per state
    for (state_key, methods) in &state_methods {
        match state_key {
            Some(state_name) => {
                let is_initial = *state_name == initial_state;
                if methods.is_empty() && !is_initial {
                    continue;
                }
                code.push_str(&format!("impl {}<{state_name}> {{\n", s.name));
                if is_initial {
                    code.push_str(
                        "pub fn new() -> Self { Self { _state: std::marker::PhantomData } }\n",
                    );
                }
                for method in methods {
                    generate_typestate_method(
                        code,
                        &s.name,
                        method.name,
                        method.clauses,
                        method.is_mutation,
                        has_invariants,
                    );
                }
                code.push_str("}\n\n");
            }
            None => {
                // Generic impl block for state-independent methods + invariants
                if methods.is_empty() && invariant_exprs.is_empty() && other_items.is_empty() {
                    continue;
                }
                code.push_str(&format!("impl<S> {}<S> {{\n", s.name));
                for method in methods {
                    generate_typestate_method(
                        code,
                        &s.name,
                        method.name,
                        method.clauses,
                        method.is_mutation,
                        has_invariants,
                    );
                }
                for expr in &invariant_exprs {
                    let rust_expr = expr_to_rust(expr);
                    code.push_str(&format!(
                        "/// Service invariant\npub fn check_invariant(&self) {{ debug_assert!({rust_expr}); }}\n"
                    ));
                }
                for (kind, body) in &other_items {
                    let rust_expr = expr_to_rust(body);
                    code.push_str(&format!("// {kind}: {rust_expr}\n"));
                }
                code.push_str("}\n\n");
            }
        }
    }
}

/// Generate service body as standalone module contents (no `pub mod` wrapper).
/// Used in multi-file mode where each service gets its own `.rs` file.
fn generate_service_contents(s: &ServiceDecl, code: &mut String) {
    let has_states = s.items.iter().any(|i| matches!(i, ServiceItem::States(_)));

    if has_states {
        generate_typestate_service_body(s, code);
        return;
    }

    // Stateless service: simple struct + impl block
    for item in &s.items {
        match item {
            ServiceItem::TypeDef(t) => generate_type_def(t, code),
            ServiceItem::EnumDef(e) => generate_enum_def(e, code),
            ServiceItem::States(_)
            | ServiceItem::Operation { .. }
            | ServiceItem::Query { .. }
            | ServiceItem::Invariant(_)
            | ServiceItem::Other { .. } => {}
        }
    }

    code.push_str(&format!("#[derive(Debug)]\npub struct {} {{\n", s.name));
    code.push_str("}\n\n");

    code.push_str(&format!("impl {} {{\n", s.name));
    code.push_str("    pub fn new() -> Self {\n        Self { }\n    }\n\n");

    let has_invariants = s
        .items
        .iter()
        .any(|i| matches!(i, ServiceItem::Invariant(_)));

    for item in &s.items {
        match item {
            ServiceItem::Operation { name, clauses } => {
                generate_service_method(code, name, clauses, true, has_invariants);
            }
            ServiceItem::Query { name, clauses } => {
                generate_service_method(code, name, clauses, false, has_invariants);
            }
            ServiceItem::Invariant(expr) => {
                let rust_expr = expr_to_rust(expr);
                code.push_str(&format!(
                    "    /// Service invariant\n    pub fn check_invariant(&self) {{ debug_assert!({rust_expr}); }}\n\n"
                ));
            }
            ServiceItem::Other { kind, body } => {
                let rust_expr = expr_to_rust(body);
                code.push_str(&format!("    // {kind}: {rust_expr}\n\n"));
            }
            ServiceItem::TypeDef(_) | ServiceItem::EnumDef(_) | ServiceItem::States(_) => {}
        }
    }

    code.push_str("}\n"); // close impl
}

fn generate_service(s: &ServiceDecl, code: &mut String) {
    code.push_str(&format!(
        "/// Service: {}\npub mod {} {{\n",
        s.name,
        s.name.to_lowercase()
    ));

    // Generate the service body (typestate or classic), then indent it
    let mut inner = String::new();
    generate_service_contents(s, &mut inner);
    for line in inner.lines() {
        if line.is_empty() {
            code.push('\n');
        } else {
            code.push_str(&format!("    {line}\n"));
        }
    }

    code.push_str("}\n\n"); // close mod
}

// ---------------------------------------------------------------------------
// Interface contracts -> Rust traits (T062)
// ---------------------------------------------------------------------------

/// Generate a Rust trait from an Assura interface block.
///
/// Interface blocks contain `method` clauses that declare required
/// methods, and `extends` clauses that declare supertrait bounds.
/// Generates a Rust trait with the declared methods.
fn generate_interface_trait(name: &str, body: &[Clause], code: &mut String) {
    // Collect extends (supertraits)
    let extends: Vec<String> = body
        .iter()
        .filter(|c| matches!(&c.kind, ClauseKind::Other(k) if k == "extends"))
        .filter_map(|c| {
            if let Expr::Ident(n) = &c.body {
                Some(n.clone())
            } else {
                None
            }
        })
        .collect();

    // Build trait header with supertraits
    if extends.is_empty() {
        code.push_str(&format!(
            "/// Interface contract: {name}\npub trait {name} {{\n"
        ));
    } else {
        let bounds = extends.join(" + ");
        code.push_str(&format!(
            "/// Interface contract: {name}\npub trait {name}: {bounds} {{\n"
        ));
    }

    // Collect method declarations
    for clause in body {
        match &clause.kind {
            ClauseKind::Other(k) if k == "method" => {
                generate_trait_method(&clause.body, code);
            }
            ClauseKind::Invariant | ClauseKind::Ensures => {
                // Interface invariants become provided methods with assertions
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!(
                    "    /// Interface invariant\n    fn check_invariant(&self) {{ debug_assert!({expr}); }}\n\n"
                ));
            }
            // Interface blocks only use method and invariant clauses.
            // Other clause kinds are ignored in trait generation.
            ClauseKind::Requires
            | ClauseKind::Effects
            | ClauseKind::Modifies
            | ClauseKind::Input
            | ClauseKind::Output
            | ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Other(_) => {}
        }
    }

    code.push_str("}\n\n");
}

/// Generate a single trait method declaration from an interface method clause.
fn generate_trait_method(body: &Expr, code: &mut String) {
    match body {
        Expr::Ident(name) => {
            // Simple method with no params: fn name(&self);
            code.push_str(&format!("    fn {name}(&self);\n\n"));
        }
        Expr::Call { func, args } => {
            // Method with params: fn name(&self, param: Type, ...) -> RetType
            let func_name = if let Expr::Ident(n) = func.as_ref() {
                n.clone()
            } else {
                "unknown".to_string()
            };
            let params: String = args
                .iter()
                .enumerate()
                .map(|(i, arg)| {
                    if let Expr::Ident(ty) = arg {
                        format!("arg{i}: {}", map_type_token(ty))
                    } else {
                        format!("arg{i}: i64")
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            if params.is_empty() {
                code.push_str(&format!("    fn {func_name}(&self);\n\n"));
            } else {
                code.push_str(&format!("    fn {func_name}(&self, {params});\n\n"));
            }
        }
        Expr::Raw(tokens) => {
            // Parse method from raw tokens: "name(Type, Type) -> RetType"
            if let Some((name, rest)) = tokens.first().map(|n| (n.clone(), &tokens[1..])) {
                let mut params = Vec::new();
                let mut return_type = String::new();
                let mut i = 0;
                let mut in_params = false;

                while i < rest.len() {
                    let tok = &rest[i];
                    if tok == "(" {
                        in_params = true;
                        i += 1;
                        continue;
                    }
                    if tok == ")" {
                        in_params = false;
                        i += 1;
                        continue;
                    }
                    if tok == "->" {
                        i += 1;
                        if i < rest.len() {
                            return_type = map_type_token(&rest[i]).to_string();
                        }
                        break;
                    }
                    if tok == "," {
                        i += 1;
                        continue;
                    }
                    if in_params {
                        // Check for "name: Type" pattern
                        if i + 2 < rest.len() && rest[i + 1] == ":" {
                            let pname = tok.clone();
                            let ptype = map_type_token(&rest[i + 2]).to_string();
                            params.push(format!("{pname}: {ptype}"));
                            i += 3;
                            continue;
                        }
                        // Just a type name
                        let ptype = map_type_token(tok).to_string();
                        params.push(format!("arg{}: {ptype}", params.len()));
                    }
                    i += 1;
                }

                let params_s = if params.is_empty() {
                    String::new()
                } else {
                    format!(", {}", params.join(", "))
                };

                if return_type.is_empty() {
                    code.push_str(&format!("    fn {name}(&self{params_s});\n\n"));
                } else {
                    code.push_str(&format!(
                        "    fn {name}(&self{params_s}) -> {return_type};\n\n"
                    ));
                }
            }
        }
        // These expression forms are not valid trait method declarations;
        // emit a compile_error! so the generated Rust surfaces the issue.
        Expr::Literal(_)
        | Expr::Field(_, _)
        | Expr::MethodCall { .. }
        | Expr::Index { .. }
        | Expr::BinOp { .. }
        | Expr::UnaryOp { .. }
        | Expr::Old(_)
        | Expr::Forall { .. }
        | Expr::Exists { .. }
        | Expr::If { .. }
        | Expr::Paren(_)
        | Expr::List(_)
        | Expr::Cast { .. }
        | Expr::Block(_)
        | Expr::Ghost(_)
        | Expr::Apply { .. }
        | Expr::Let { .. }
        | Expr::Match { .. }
        | Expr::Tuple(_) => {
            code.push_str(&format!(
                "    compile_error!(\"unsupported expression in trait method: {:?}\");\n\n",
                std::mem::discriminant(body)
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Generic blocks (feature, incremental, etc.)
// ---------------------------------------------------------------------------

fn generate_block(kind: &str, name: &str, body: &[Clause], code: &mut String) {
    // Interface blocks generate Rust traits
    if kind == "interface" {
        generate_interface_trait(name, body, code);
        return;
    }

    // Table blocks: generate a doc comment describing the table.
    // The actual compile-time verification happens in the SMT layer,
    // not in generated Rust code.
    if kind == "table" {
        code.push_str(&format!(
            "// {kind} {name}: compile-time verified by SMT\n\n"
        ));
        return;
    }

    // Other blocks: generate as documented constants/assertions
    code.push_str(&format!("/// {kind}: {name}\n"));
    code.push_str(&format!("pub mod block_{} {{\n", name.to_lowercase()));

    for clause in body {
        let expr = expr_to_rust(&clause.body);
        match clause.kind {
            ClauseKind::Ensures | ClauseKind::Invariant => {
                code.push_str(&format!(
                    "    /// Invariant: {expr}\n    pub fn check_{name}() {{ debug_assert!({expr}); }}\n",
                    name = name.to_lowercase()
                ));
            }
            ClauseKind::Requires => {
                code.push_str(&format!(
                    "    /// Precondition: {expr}\n    pub const PRECONDITION: &str = \"{}\";\n",
                    expr.replace('"', "\\\"")
                ));
            }
            ClauseKind::Effects => {
                code.push_str(&format!("    /// Effects: {expr}\n"));
            }
            ClauseKind::Modifies => {
                code.push_str(&format!("    /// Modifies: {expr}\n"));
            }
            ClauseKind::Input => {
                code.push_str(&format!("    /// Input: {expr}\n"));
            }
            ClauseKind::Output => {
                code.push_str(&format!("    /// Output: {expr}\n"));
            }
            ClauseKind::Errors => {
                code.push_str(&format!("    /// Errors: {expr}\n"));
            }
            ClauseKind::Rule => {
                code.push_str(&format!(
                    "    /// Rule: {expr}\n    pub fn check_rule_{name}() {{ debug_assert!({expr}); }}\n",
                    name = name.to_lowercase()
                ));
            }
            ClauseKind::DataFlow => {
                code.push_str(&format!("    /// DataFlow: {expr}\n"));
            }
            ClauseKind::MustNot => {
                code.push_str(&format!(
                    "    /// MustNot: {expr}\n    pub fn check_must_not_{name}() {{ debug_assert!(!({expr})); }}\n",
                    name = name.to_lowercase()
                ));
            }
            ClauseKind::Decreases => {
                code.push_str(&format!("    /// Decreases: {expr}\n"));
            }
            ClauseKind::Other(ref kind_name) => {
                code.push_str(&format!("    /// {kind_name}: {expr}\n"));
            }
        }
    }

    code.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Rust formatting via prettyplease
// ---------------------------------------------------------------------------

/// Format a Rust source string via prettyplease.
///
/// If parsing fails (the generated code is not valid Rust syntax),
/// returns the input unchanged with a comment noting the failure.
fn format_rust(code: &str) -> String {
    match syn::parse_file(code) {
        Ok(syntax_tree) => prettyplease::unparse(&syntax_tree),
        Err(e) => {
            eprintln!("warning: generated Rust has syntax errors, skipping formatting: {e}");
            format!("// WARNING: prettyplease formatting skipped (parse error: {e})\n\n{code}")
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse + resolve + type-check source text, then codegen.
    fn codegen_ok(source: &str) -> GeneratedProject {
        let file = assura_parser::parse_unwrap(source);
        let resolved = assura_resolve::resolve(&file).expect("resolve failed");
        let typed = assura_types::type_check(&resolved).expect("type check failed");
        codegen(&typed)
    }

    #[test]
    fn empty_file_generates_project() {
        let project = codegen_ok("");
        assert!(!project.cargo_toml.is_empty());
        assert_eq!(project.files.len(), 1);
        assert_eq!(project.files[0].0, "src/lib.rs");
        assert!(
            project.files[0]
                .1
                .contains("Generated by the Assura compiler")
        );
    }

    #[test]
    fn struct_codegen() {
        let project = codegen_ok(
            r#"
type Point {
    x: Int
    y: Int
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(lib.contains("struct Point"), "should contain struct Point");
        assert!(lib.contains("i64"), "should map Int to i64");
    }

    #[test]
    fn enum_codegen() {
        let project = codegen_ok(
            r#"
enum Color {
    Red,
    Green,
    Blue
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(lib.contains("enum Color"), "should contain enum Color");
        assert!(lib.contains("Red"), "should contain Red variant");
        assert!(lib.contains("Green"), "should contain Green variant");
        assert!(lib.contains("Blue"), "should contain Blue variant");
    }

    #[test]
    fn enum_generates_display_impl() {
        let project = codegen_ok(
            r#"
enum Status {
    Active,
    Inactive,
    Pending
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("impl std::fmt::Display for Status"),
            "should generate Display impl: {lib}"
        );
        assert!(
            lib.contains("Status::Active => write!(f, \"Active\")"),
            "should have Active display arm: {lib}"
        );
    }

    #[test]
    fn contract_generates_module() {
        let project = codegen_ok(
            r#"
contract SafeDivision {
    requires { true }
    ensures  { true }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("contract_safedivision"),
            "should contain contract module"
        );
    }

    #[test]
    fn fn_def_codegen() {
        // Note: the current parser stores fn clauses in return_ty as raw
        // tokens rather than structured Clause objects. So fn codegen
        // generates the return type string including clause keywords.
        // The codegen still generates a valid function stub.
        let project = codegen_ok(
            r#"
fn helper(n: Int) -> Int {
    requires { n >= 0 }
    ensures  { result >= 0 }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(lib.contains("fn helper"), "should contain fn helper");
    }

    #[test]
    fn contract_generates_debug_assert() {
        let project = codegen_ok(
            r#"
contract Positive {
    requires { true }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("debug_assert!"),
            "contract requires should generate debug_assert"
        );
    }

    #[test]
    fn project_name_used_in_cargo_toml() {
        let project = codegen_ok(
            r#"
project my_cool_project {
    profile: [core]
}

contract Foo {
    requires { true }
}
"#,
        );
        assert!(
            project.cargo_toml.contains("my_cool_project"),
            "Cargo.toml should use the project name"
        );
    }

    #[test]
    fn service_generates_module() {
        let project = codegen_ok(
            r#"
service MyService {
    states: Init -> Running -> Stopped

    operation Start {
        requires: true
    }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("mod myservice"),
            "should contain service module"
        );
        // S008: typestate encoding generates marker structs instead of State enum
        assert!(
            lib.contains("pub struct Init"),
            "should contain state marker struct Init: {lib}"
        );
        assert!(
            lib.contains("PhantomData"),
            "should contain PhantomData for typestate: {lib}"
        );
    }

    #[test]
    fn service_state_transition_codegen() {
        let project = codegen_ok(
            r#"
service Connection {
    states: Disconnected -> Connected -> Closed

    operation Connect {
        requires { self.state == Disconnected }
        ensures { self.state == Connected }
    }
}
"#,
        );
        let lib = &project.files[0].1;
        // S008: typestate encoding puts Connect in impl Connection<Disconnected>
        assert!(
            lib.contains("impl Connection<Disconnected>"),
            "should contain state-specific impl block: {lib}"
        );
        // Return type encodes the transition to Connected
        assert!(
            lib.contains("Connection<Connected>"),
            "should contain transition return type: {lib}"
        );
        // Method consumes self (not &mut self)
        assert!(
            lib.contains("fn Connect(self)"),
            "state-transitioning method should consume self: {lib}"
        );
    }

    #[test]
    fn cargo_toml_well_formed() {
        let project = codegen_ok("");
        assert!(project.cargo_toml.contains("[package]"));
        assert!(project.cargo_toml.contains("edition = \"2024\""));
        assert!(project.cargo_toml.contains("[dependencies]"));
    }

    // -----------------------------------------------------------------------
    // T020: Type mapping tests
    // -----------------------------------------------------------------------

    #[test]
    fn type_mapping_int_to_i64() {
        assert_eq!(map_type_token("Int"), "i64");
    }

    #[test]
    fn type_mapping_nat_to_u64() {
        assert_eq!(map_type_token("Nat"), "u64");
    }

    #[test]
    fn type_mapping_float_to_f64() {
        assert_eq!(map_type_token("Float"), "f64");
    }

    #[test]
    fn type_mapping_bool() {
        assert_eq!(map_type_token("Bool"), "bool");
    }

    #[test]
    fn type_mapping_string() {
        assert_eq!(map_type_token("String"), "String");
    }

    #[test]
    fn type_mapping_bytes_to_vec_u8() {
        assert_eq!(map_type_token("Bytes"), "Vec<u8>");
    }

    #[test]
    fn type_mapping_unit() {
        assert_eq!(map_type_token("Unit"), "()");
    }

    #[test]
    fn type_mapping_never() {
        assert_eq!(map_type_token("Never"), "!");
    }

    #[test]
    fn type_mapping_list_to_vec() {
        assert_eq!(map_type_token("List"), "Vec");
    }

    #[test]
    fn type_mapping_map_to_btreemap() {
        assert_eq!(map_type_token("Map"), "std::collections::BTreeMap");
    }

    #[test]
    fn type_mapping_set_to_btreeset() {
        assert_eq!(map_type_token("Set"), "std::collections::BTreeSet");
    }

    #[test]
    fn type_mapping_option_passthrough() {
        assert_eq!(map_type_token("Option"), "Option");
    }

    #[test]
    fn type_mapping_result_passthrough() {
        assert_eq!(map_type_token("Result"), "Result");
    }

    #[test]
    fn type_mapping_fixed_width() {
        assert_eq!(map_type_token("U8"), "u8");
        assert_eq!(map_type_token("U16"), "u16");
        assert_eq!(map_type_token("U32"), "u32");
        assert_eq!(map_type_token("U64"), "u64");
        assert_eq!(map_type_token("I8"), "i8");
        assert_eq!(map_type_token("I16"), "i16");
        assert_eq!(map_type_token("I32"), "i32");
        assert_eq!(map_type_token("I64"), "i64");
        assert_eq!(map_type_token("F32"), "f32");
        assert_eq!(map_type_token("F64"), "f64");
    }

    #[test]
    fn refined_type_generates_newtype() {
        let project = codegen_ok(
            r#"
type Pos = { n: Int | n > 0 }
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("struct Pos"),
            "refined type should generate a newtype struct"
        );
        assert!(lib.contains("i64"), "refined type base should be i64");
    }

    #[test]
    fn type_alias_codegen() {
        let project = codegen_ok(
            r#"
type UserId = Int
"#,
        );
        let lib = &project.files[0].1;
        assert!(lib.contains("UserId"), "should contain type alias name");
    }

    // -----------------------------------------------------------------------
    // T021: Contract codegen tests
    // -----------------------------------------------------------------------

    #[test]
    fn contract_ensures_generates_debug_assert() {
        let project = codegen_ok(
            r#"
contract NonNeg {
    requires { true }
    ensures  { true }
}
"#,
        );
        let lib = &project.files[0].1;
        // Both requires and ensures should produce debug_assert!
        let assert_count = lib.matches("debug_assert!").count();
        assert!(
            assert_count >= 2,
            "should have debug_assert for both requires and ensures, got {assert_count}"
        );
    }

    #[test]
    fn contract_has_result_variable() {
        let project = codegen_ok(
            r#"
contract Foo {
    requires { true }
    ensures  { true }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("__result"),
            "contract should declare __result variable"
        );
    }

    #[test]
    fn fn_def_ensures_generates_debug_assert() {
        // Note: clauses must be outside the body block for the parser
        // to parse them as structured Clause objects.
        let project = codegen_ok(
            "fn abs(n: Int) -> Int\n    requires { true }\n    ensures  { result >= 0 }\n",
        );
        let lib = &project.files[0].1;
        // requires and ensures should both be debug_assert!
        let assert_count = lib.matches("debug_assert!").count();
        assert!(
            assert_count >= 2,
            "fn should have debug_assert for both requires and ensures, got {assert_count}"
        );
        assert!(
            lib.contains("__result"),
            "fn should declare __result variable"
        );
    }

    #[test]
    fn fn_result_maps_to_dunder_result() {
        let project = codegen_ok("fn double(n: Int) -> Int\n    ensures { result == n + n }\n");
        let lib = &project.files[0].1;
        assert!(
            lib.contains("__result"),
            "result keyword in ensures should map to __result"
        );
    }

    // -----------------------------------------------------------------------
    // T022: Cargo project generation tests
    // -----------------------------------------------------------------------

    #[test]
    fn cargo_toml_has_package_name() {
        let project = codegen_ok(
            r#"
project test_project {
    profile: [core]
}
"#,
        );
        assert!(project.cargo_toml.contains("test_project"));
        assert!(project.cargo_toml.contains("[package]"));
    }

    #[test]
    fn default_project_name_is_generated() {
        let project = codegen_ok("");
        assert!(
            project.cargo_toml.contains("\"generated\""),
            "default project name should be 'generated'"
        );
    }

    #[test]
    fn generated_file_is_src_lib_rs() {
        let project = codegen_ok(
            r#"
type X {
    val: Int
}
"#,
        );
        assert_eq!(project.files.len(), 1);
        assert_eq!(project.files[0].0, "src/lib.rs");
    }

    #[test]
    fn generated_rust_has_header_comment() {
        let project = codegen_ok("");
        let lib = &project.files[0].1;
        assert!(lib.contains("Generated by the Assura compiler"));
        assert!(lib.contains("Do not edit manually"));
    }

    // -----------------------------------------------------------------------
    // T023: Struct and enum codegen tests
    // -----------------------------------------------------------------------

    #[test]
    fn struct_has_derive_debug_clone_partialeq() {
        let project = codegen_ok(
            r#"
type Pair {
    a: Int
    b: Int
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(lib.contains("Debug"), "struct should derive Debug");
        assert!(lib.contains("Clone"), "struct should derive Clone");
        assert!(lib.contains("PartialEq"), "struct should derive PartialEq");
    }

    #[test]
    fn struct_field_types_are_mapped() {
        let project = codegen_ok(
            r#"
type Config {
    name: String
    count: Nat
    enabled: Bool
    ratio: Float
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(lib.contains("String"), "String should map to String");
        assert!(lib.contains("u64"), "Nat should map to u64");
        assert!(lib.contains("bool"), "Bool should map to bool");
        assert!(lib.contains("f64"), "Float should map to f64");
    }

    #[test]
    fn struct_pub_field_visibility() {
        let project = codegen_ok(
            r#"
type Visible {
    pub x: Int
    y: Int
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("pub x"),
            "pub field should have pub visibility in generated code"
        );
    }

    #[test]
    fn enum_has_derive_debug_clone_partialeq() {
        let project = codegen_ok(
            r#"
enum Direction {
    North,
    South,
    East,
    West
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(lib.contains("Debug"), "enum should derive Debug");
        assert!(lib.contains("Clone"), "enum should derive Clone");
        assert!(lib.contains("PartialEq"), "enum should derive PartialEq");
    }

    #[test]
    fn enum_variant_with_data() {
        let project = codegen_ok(
            r#"
enum Value {
    Num(Int),
    Text(String),
    Nothing
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(lib.contains("Num"), "should contain Num variant");
        assert!(lib.contains("i64"), "Int should map to i64 in variant");
        assert!(lib.contains("Text"), "should contain Text variant");
        assert!(
            lib.contains("Nothing"),
            "should contain unit variant Nothing"
        );
    }

    #[test]
    fn empty_struct_codegen() {
        let project = codegen_ok(
            r#"
type Marker {
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(lib.contains("Marker"), "should contain empty struct");
    }

    // -----------------------------------------------------------------------
    // T028: End-to-end SafeDivision contract test
    // -----------------------------------------------------------------------

    #[test]
    fn e2e_safe_division_check_passes() {
        // Parse the e2e test file through the full pipeline
        let source = std::fs::read_to_string("../../tests/e2e/safe_division.assura")
            .expect("failed to read safe_division.assura");
        let file = assura_parser::parse_unwrap(&source);
        let resolved = assura_resolve::resolve(&file).expect("resolution should succeed");
        let typed = assura_types::type_check(&resolved).expect("type check should succeed");

        // Codegen should succeed
        let project = codegen(&typed);
        assert!(
            !project.cargo_toml.is_empty(),
            "Cargo.toml should not be empty"
        );
        assert_eq!(
            project.files.len(),
            1,
            "should produce exactly one source file"
        );
        assert_eq!(project.files[0].0, "src/lib.rs");
    }

    #[test]
    fn e2e_safe_division_generates_debug_assert_for_requires() {
        let source = std::fs::read_to_string("../../tests/e2e/safe_division.assura")
            .expect("failed to read safe_division.assura");
        let project = codegen_ok(&source);
        let lib = &project.files[0].1;

        // The requires clause `b != 0` must produce a debug_assert
        assert!(
            lib.contains("debug_assert!"),
            "generated code must contain debug_assert!"
        );
        assert!(
            lib.contains("b != 0"),
            "generated code must contain the requires predicate 'b != 0'"
        );
    }

    #[test]
    fn e2e_safe_division_generates_ensures_assertion() {
        let source = std::fs::read_to_string("../../tests/e2e/safe_division.assura")
            .expect("failed to read safe_division.assura");
        let project = codegen_ok(&source);
        let lib = &project.files[0].1;

        // The ensures clause should produce a debug_assert with the postcondition
        assert!(
            lib.contains("debug_assert!"),
            "generated code must contain debug_assert from requires/ensures"
        );
        // At least two debug_assert! calls: for requires and ensures
        let assert_count = lib.matches("debug_assert!").count();
        assert!(
            assert_count >= 2,
            "should have debug_assert for both requires and ensures, got {assert_count}"
        );
    }

    #[test]
    fn e2e_safe_division_has_correct_signature() {
        let source = std::fs::read_to_string("../../tests/e2e/safe_division.assura")
            .expect("failed to read safe_division.assura");
        let project = codegen_ok(&source);
        let lib = &project.files[0].1;

        // Should have input params mapped to i64
        assert!(lib.contains("a: i64"), "input param 'a' should map to i64");
        assert!(lib.contains("b: i64"), "input param 'b' should map to i64");
        // Should have the contract module
        assert!(
            lib.contains("contract_safedivision"),
            "should contain the SafeDivision contract module"
        );
    }

    #[test]
    fn e2e_safe_division_generated_rust_is_valid() {
        let source = std::fs::read_to_string("../../tests/e2e/safe_division.assura")
            .expect("failed to read safe_division.assura");
        let project = codegen_ok(&source);
        let lib = &project.files[0].1;

        // Verify the generated Rust parses as valid syntax via syn
        syn::parse_file(lib).expect("generated Rust should be valid syntax");
    }

    // -----------------------------------------------------------------------
    // T043 CORE.1: Ghost code erasure tests
    // -----------------------------------------------------------------------

    #[test]
    fn ghost_fn_produces_no_output() {
        // A ghost function should be completely erased in generated code.
        let project =
            codegen_ok("ghost fn spec_helper(x: Int) -> Bool\n    ensures { result == true }\n");
        let lib = &project.files[0].1;
        assert!(
            !lib.contains("fn spec_helper"),
            "ghost fn should not appear in generated Rust code"
        );
    }

    #[test]
    fn non_ghost_fn_still_generated() {
        // A normal (non-ghost) function should still be generated.
        let project = codegen_ok("fn normal_helper(x: Int) -> Int\n    ensures { result >= 0 }\n");
        let lib = &project.files[0].1;
        assert!(
            lib.contains("fn normal_helper"),
            "non-ghost fn should appear in generated Rust code"
        );
    }

    #[test]
    fn ghost_block_erased_in_expr() {
        // A ghost block expression should produce erased output.
        let expr = Expr::Ghost(Box::new(Expr::Literal(Literal::Bool(true))));
        let rust = expr_to_rust(&expr);
        assert!(
            rust.contains("ghost erased"),
            "ghost block should generate erased marker, got: {rust}"
        );
    }

    // -----------------------------------------------------------------------
    // T044 CORE.2: Lemma erasure tests
    // -----------------------------------------------------------------------

    #[test]
    fn lemma_fn_produces_no_output() {
        // A lemma function should be completely erased in generated code.
        let project =
            codegen_ok("lemma add_comm(a: Int, b: Int)\n    ensures { a + b == b + a }\n");
        let lib = &project.files[0].1;
        assert!(
            !lib.contains("fn add_comm"),
            "lemma fn should not appear in generated Rust code"
        );
    }

    #[test]
    fn apply_expr_erased_in_codegen() {
        // apply lemma_name(args) should produce a comment, not code.
        let expr = Expr::Apply {
            lemma_name: "my_lemma".into(),
            args: vec![Expr::Literal(Literal::Int("42".into()))],
        };
        let rust = expr_to_rust(&expr);
        assert!(
            rust.contains("lemma my_lemma applied"),
            "apply should generate erased comment, got: {rust}"
        );
    }

    #[test]
    fn match_expr_codegen() {
        // match expression should generate Rust match syntax
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("status".into())),
            arms: vec![
                assura_parser::ast::MatchArm {
                    pattern: assura_parser::ast::Pattern::Ident("Active".into()),
                    body: Expr::Literal(Literal::Int("1".into())),
                },
                assura_parser::ast::MatchArm {
                    pattern: assura_parser::ast::Pattern::Wildcard,
                    body: Expr::Literal(Literal::Int("0".into())),
                },
            ],
        };
        let rust = expr_to_rust(&expr);
        assert!(
            rust.contains("match status"),
            "should have match keyword: {rust}"
        );
        assert!(
            rust.contains("Active => 1"),
            "should have Active arm: {rust}"
        );
        assert!(rust.contains("_ => 0"), "should have wildcard arm: {rust}");
    }

    #[test]
    fn match_without_wildcard_gets_fallback() {
        // match with only Constructor patterns (no wildcard) should get _ => unreachable!()
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("color".into())),
            arms: vec![
                assura_parser::ast::MatchArm {
                    pattern: assura_parser::ast::Pattern::Constructor {
                        name: "Red".into(),
                        fields: vec![],
                    },
                    body: Expr::Literal(Literal::Int("1".into())),
                },
                assura_parser::ast::MatchArm {
                    pattern: assura_parser::ast::Pattern::Constructor {
                        name: "Blue".into(),
                        fields: vec![],
                    },
                    body: Expr::Literal(Literal::Int("2".into())),
                },
            ],
        };
        let rust = expr_to_rust(&expr);
        assert!(
            rust.contains("_ => unreachable!"),
            "match without wildcard should get fallback: {rust}"
        );
    }

    #[test]
    fn non_lemma_fn_still_generated() {
        // A normal (non-lemma) function should still be generated.
        let project = codegen_ok("fn helper(n: Int) -> Int\n    ensures { result >= 0 }\n");
        let lib = &project.files[0].1;
        assert!(
            lib.contains("fn helper"),
            "non-lemma fn should appear in generated Rust code"
        );
    }

    // =======================================================================
    // T119: Cranelift backend configuration
    // =======================================================================

    #[test]
    fn backend_default_is_rustc() {
        let config = super::BackendConfig::default();
        assert_eq!(config.backend, super::CodegenBackend::Rustc);
    }

    #[test]
    fn backend_cranelift_fast_dev() {
        let config = super::BackendConfig {
            backend: super::CodegenBackend::Cranelift,
            opt_level: 0,
            debug_info: true,
            target: super::CompileTarget::Native,
        };
        assert_eq!(config.backend, super::CodegenBackend::Cranelift);
        assert_eq!(config.opt_level, 0);
    }

    // =======================================================================
    // Generated Rust compilation tests
    // =======================================================================

    /// Verify generated Rust for a contract parses as valid Rust syntax.
    fn assert_generated_rust_valid(source: &str) {
        let project = codegen_ok(source);
        let lib = &project.files[0].1;
        syn::parse_file(lib).unwrap_or_else(|e| {
            panic!("generated Rust is not valid syntax:\n{lib}\n\nerror: {e}");
        });
    }

    #[test]
    fn generated_rust_contract_is_valid() {
        assert_generated_rust_valid(
            r#"
contract SafeDivision {
  input(a: Int, b: Int)
  output(result: Int)
  requires { b != 0 }
  ensures { result * b == a }
  effects { pure }
}
"#,
        );
    }

    #[test]
    fn generated_rust_fn_is_valid() {
        assert_generated_rust_valid(
            r#"
fn clamp(x: Int, lo: Int, hi: Int) -> Int
  requires { lo <= hi }
  ensures { result >= lo && result <= hi }
{
  if x < lo then lo else if x > hi then hi else x
}
"#,
        );
    }

    #[test]
    fn generated_rust_service_is_valid() {
        let source =
            std::fs::read_to_string("../assura-cli/../../tests/fixtures/service_full.assura")
                .unwrap_or_else(|_| {
                    std::fs::read_to_string("tests/fixtures/service_full.assura")
                        .expect("cannot find service_full fixture")
                });
        let project = codegen_ok(&source);
        let lib = &project.files[0].1;
        syn::parse_file(lib).unwrap_or_else(|e| {
            panic!("service generated Rust is not valid:\n{lib}\n\nerror: {e}");
        });
    }

    #[test]
    fn generated_rust_demo_libwebp_is_valid() {
        let source = std::fs::read_to_string("../assura-cli/../../demos/libwebp-huffman.assura")
            .unwrap_or_else(|_| {
                // Fallback path when running from workspace root
                std::fs::read_to_string("demos/libwebp-huffman.assura")
                    .expect("cannot find libwebp demo")
            });
        let project = codegen_ok(&source);
        let lib = &project.files[0].1;
        syn::parse_file(lib).unwrap_or_else(|e| {
            panic!("libwebp generated Rust is not valid:\n{lib}\n\nerror: {e}");
        });
    }

    #[test]
    fn codegen_with_config_produces_profile() {
        let config = super::BackendConfig {
            backend: super::CodegenBackend::Rustc,
            opt_level: 3,
            debug_info: true,
            target: super::CompileTarget::Native,
        };
        let project = {
            let file = assura_parser::parse_unwrap("");
            let resolved = assura_resolve::resolve(&file).expect("resolve failed");
            let typed = assura_types::type_check(&resolved).expect("type check failed");
            super::codegen_with_config(&typed, &config)
        };
        assert!(project.cargo_toml.contains("opt-level = 3"));
        assert!(project.cargo_toml.contains("debug = true"));
    }

    #[test]
    fn pattern_to_rust_constructor_with_fields() {
        use assura_parser::ast::Pattern;
        let pat = Pattern::Constructor {
            name: "Some".into(),
            fields: vec![Pattern::Ident("x".into())],
        };
        assert_eq!(super::pattern_to_rust(&pat), "Some(x)");
    }

    #[test]
    fn pattern_to_rust_constructor_no_fields() {
        use assura_parser::ast::Pattern;
        let pat = Pattern::Constructor {
            name: "None".into(),
            fields: vec![],
        };
        assert_eq!(super::pattern_to_rust(&pat), "None");
    }

    #[test]
    fn pattern_to_rust_nested_constructor() {
        use assura_parser::ast::Pattern;
        let pat = Pattern::Constructor {
            name: "Ok".into(),
            fields: vec![Pattern::Constructor {
                name: "Some".into(),
                fields: vec![Pattern::Ident("v".into())],
            }],
        };
        assert_eq!(super::pattern_to_rust(&pat), "Ok(Some(v))");
    }

    #[test]
    fn pattern_to_rust_tuple_nested() {
        use assura_parser::ast::Pattern;
        let pat = Pattern::Tuple(vec![
            Pattern::Ident("a".into()),
            Pattern::Tuple(vec![Pattern::Ident("b".into()), Pattern::Wildcard]),
        ]);
        assert_eq!(super::pattern_to_rust(&pat), "(a, (b, _))");
    }

    // =======================================================================
    // Interface trait generation tests (T062)
    // =======================================================================

    #[test]
    fn interface_block_generates_trait() {
        let mut code = String::new();
        let body = vec![
            Clause {
                kind: ClauseKind::Other("method".into()),
                body: Expr::Ident("process".into()),
            },
            Clause {
                kind: ClauseKind::Other("method".into()),
                body: Expr::Ident("validate".into()),
            },
        ];
        super::generate_interface_trait("Processor", &body, &mut code);
        assert!(
            code.contains("pub trait Processor"),
            "should generate trait: {code}"
        );
        assert!(
            code.contains("fn process(&self)"),
            "should have process method: {code}"
        );
        assert!(
            code.contains("fn validate(&self)"),
            "should have validate method: {code}"
        );
    }

    #[test]
    fn interface_with_extends_generates_supertrait() {
        let mut code = String::new();
        let body = vec![
            Clause {
                kind: ClauseKind::Other("extends".into()),
                body: Expr::Ident("Base".into()),
            },
            Clause {
                kind: ClauseKind::Other("method".into()),
                body: Expr::Ident("extra".into()),
            },
        ];
        super::generate_interface_trait("Extended", &body, &mut code);
        assert!(
            code.contains("pub trait Extended: Base"),
            "should have supertrait bound: {code}"
        );
    }

    #[test]
    fn interface_with_invariant_generates_provided_method() {
        let mut code = String::new();
        let body = vec![Clause {
            kind: ClauseKind::Invariant,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
        }];
        super::generate_interface_trait("Positive", &body, &mut code);
        assert!(
            code.contains("fn check_invariant"),
            "should generate invariant check: {code}"
        );
        assert!(
            code.contains("debug_assert!"),
            "should contain debug_assert: {code}"
        );
    }

    #[test]
    fn interface_contract_generates_trait() {
        // Contract with an `interface` clause should produce a trait, not a module
        let source = r#"
contract Sortable {
  interface: true
  method: compare
  method: swap
}
"#;
        let project = codegen_ok(source);
        let lib = &project.files[0].1;
        assert!(
            lib.contains("trait Sortable"),
            "interface contract should generate trait: {lib}"
        );
        assert!(
            !lib.contains("mod contract_sortable"),
            "interface contract should NOT generate module: {lib}"
        );
    }

    #[test]
    fn implements_contract_generates_impl_block() {
        // Contract with `implements` clause should generate struct + impl.
        // With 2 contracts this triggers multi-file mode.
        let source = r#"
contract Sortable {
  interface: true
  method: compare
}

contract MySorter {
  implements: Sortable
  method: compare
  requires { x > 0 }
}
"#;
        let project = codegen_ok(source);

        // Multi-file mode: lib.rs has mod declarations, contract files have contents
        let find_file = |name: &str| -> &str {
            &project
                .files
                .iter()
                .find(|(p, _)| p == name)
                .unwrap_or_else(|| {
                    panic!(
                        "missing file {name}: files={:?}",
                        project.files.iter().map(|(p, _)| p).collect::<Vec<_>>()
                    )
                })
                .1
        };

        let lib = find_file("src/lib.rs");
        assert!(
            lib.contains("pub mod contract_sortable;"),
            "lib.rs should declare sortable module: {lib}"
        );
        assert!(
            lib.contains("pub mod contract_mysorter;"),
            "lib.rs should declare mysorter module: {lib}"
        );

        let sortable = find_file("src/contract_sortable.rs");
        assert!(
            sortable.contains("trait Sortable"),
            "sortable module should contain trait: {sortable}"
        );

        let mysorter = find_file("src/contract_mysorter.rs");
        assert!(
            mysorter.contains("struct MySorter"),
            "mysorter module should contain struct: {mysorter}"
        );
        assert!(
            mysorter.contains("impl Sortable for MySorter"),
            "mysorter module should contain impl block: {mysorter}"
        );
    }

    #[test]
    fn forall_ensures_generates_iter_all() {
        let project = codegen_ok(
            r#"
contract AllPositive {
    input(values: List<Int>)
    requires { forall v in values: v > 0 }
    ensures  { forall v in result: v > 0 }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains(".iter().all("),
            "forall in ensures should generate .iter().all(): {lib}"
        );
        assert!(
            lib.contains("__result.iter().all("),
            "result in forall ensures should map to __result: {lib}"
        );
    }

    #[test]
    fn exists_ensures_generates_iter_any() {
        let project = codegen_ok(
            r#"
contract HasPositive {
    input(values: List<Int>)
    ensures  { exists v in result: v > 0 }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains(".iter().any("),
            "exists in ensures should generate .iter().any(): {lib}"
        );
    }

    #[test]
    fn raw_forall_tokens_converted() {
        // Test the raw_tokens_to_rust helper directly
        let tokens: Vec<String> = vec!["forall", "v", "in", "items", ":", "v", ">", "0"]
            .into_iter()
            .map(String::from)
            .collect();
        let result = raw_tokens_to_rust(&tokens);
        assert!(
            result.contains(".iter().all("),
            "raw forall tokens should produce .iter().all(): {result}"
        );
        assert!(result.contains("|v|"), "should bind variable v: {result}");
    }

    #[test]
    fn raw_exists_tokens_converted() {
        let tokens: Vec<String> = vec!["exists", "x", "in", "data", ":", "x", "==", "target"]
            .into_iter()
            .map(String::from)
            .collect();
        let result = raw_tokens_to_rust(&tokens);
        assert!(
            result.contains(".iter().any("),
            "raw exists tokens should produce .iter().any(): {result}"
        );
    }

    #[test]
    fn raw_result_keyword_replaced() {
        let tokens: Vec<String> = vec!["result", ">=", "0"]
            .into_iter()
            .map(String::from)
            .collect();
        let result = raw_tokens_to_rust(&tokens);
        assert!(
            result.contains("__result"),
            "result keyword in raw tokens should become __result: {result}"
        );
    }

    #[test]
    fn enum_generates_exhaustiveness_check() {
        let project = codegen_ok(
            r#"
enum Color {
    Red,
    Green,
    Blue,
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("__exhaustive_check_color"),
            "enum should generate exhaustiveness check function: {lib}"
        );
        assert!(
            lib.contains("Color::Red =>"),
            "exhaustiveness check should cover Red variant: {lib}"
        );
        assert!(
            lib.contains("Color::Green =>"),
            "exhaustiveness check should cover Green variant: {lib}"
        );
        assert!(
            lib.contains("Color::Blue =>"),
            "exhaustiveness check should cover Blue variant: {lib}"
        );
        // Should NOT have a wildcard arm
        assert!(
            !lib.contains("_ =>"),
            "exhaustiveness check must not have a wildcard arm: {lib}"
        );
    }

    #[test]
    fn enum_with_data_generates_exhaustiveness_check() {
        let project = codegen_ok(
            r#"
enum Value {
    Num(Int),
    Text(String),
    Empty,
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("__exhaustive_check_value"),
            "data enum should generate exhaustiveness check: {lib}"
        );
        assert!(
            lib.contains("Value::Num(_)"),
            "data variant should use wildcard field: {lib}"
        );
        assert!(
            lib.contains("Value::Empty =>"),
            "unit variant should be covered: {lib}"
        );
    }

    #[test]
    fn extract_input_params_single_cast() {
        // Top-level Cast: input(a as Int) at top level
        let mut params = Vec::new();
        let body = Expr::Cast {
            expr: Box::new(Expr::Ident("a".into())),
            ty: "Int".into(),
        };
        extract_input_params(&body, &mut params);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].0, "a");
    }

    #[test]
    fn extract_input_params_single_ident() {
        let mut params = Vec::new();
        let body = Expr::Ident("x".into());
        extract_input_params(&body, &mut params);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], ("x".to_string(), "i64".to_string()));
    }

    #[test]
    fn extract_input_params_raw_as() {
        let mut params = Vec::new();
        let tokens = vec![
            "a".into(),
            "as".into(),
            "Int".into(),
            ",".into(),
            "b".into(),
            "as".into(),
            "String".into(),
        ];
        let body = Expr::Raw(tokens);
        extract_input_params(&body, &mut params);
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].0, "a");
        assert_eq!(params[1].0, "b");
    }

    #[test]
    fn extract_input_params_raw_bare_idents() {
        let mut params = Vec::new();
        let tokens = vec!["buf".into(), ",".into(), "n".into()];
        let body = Expr::Raw(tokens);
        extract_input_params(&body, &mut params);
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].0, "buf");
        assert_eq!(params[1].0, "n");
    }

    #[test]
    fn extract_output_type_paren() {
        let body = Expr::Paren(Box::new(Expr::Ident("Int".into())));
        let ty = extract_output_type(&body);
        assert_eq!(ty, "i64");
    }

    #[test]
    fn extract_output_type_raw_as() {
        let tokens = vec!["result".into(), "as".into(), "Bool".into()];
        let body = Expr::Raw(tokens);
        let ty = extract_output_type(&body);
        assert_eq!(ty, "bool");
    }

    #[test]
    fn contract_effects_generates_doc_comment() {
        let project = codegen_ok(
            r#"
contract Alloc {
    effects  { io }
    requires { true }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("/// Effects:"),
            "effects clause should produce doc comment"
        );
    }

    #[test]
    fn contract_modifies_generates_doc_comment() {
        let project = codegen_ok(
            r#"
contract Mutator {
    modifies { buffer }
    requires { true }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("/// Modifies:"),
            "modifies clause should produce doc comment"
        );
    }

    #[test]
    fn contract_requires_generates_doc_comment() {
        let project = codegen_ok(
            r#"
contract Bounded {
    requires { true }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("/// Requires:"),
            "requires clause should produce doc comment: {lib}"
        );
    }

    #[test]
    fn service_operation_requires_generates_doc_comment() {
        let project = codegen_ok(
            r#"
service Validator {
    states: Idle -> Busy

    operation Validate {
        requires { true }
    }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("/// Requires:"),
            "requires clause in service operation should produce doc comment: {lib}"
        );
    }

    #[test]
    fn service_operation_modifies_generates_doc_comment() {
        let project = codegen_ok(
            r#"
service Storage {
    states: Empty -> Full

    operation Store {
        modifies { buffer }
        requires { true }
    }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("/// Modifies:"),
            "modifies clause in service operation should produce doc comment: {lib}"
        );
    }

    #[test]
    fn service_operation_invariant_generates_debug_assert() {
        let project = codegen_ok(
            r#"
service Counter {
    states: Ready -> Done

    operation Increment {
        invariant { true }
        requires  { true }
    }
}
"#,
        );
        let lib = &project.files[0].1;
        // requires produces one debug_assert, invariant produces another
        let assert_count = lib.matches("debug_assert!").count();
        assert!(
            assert_count >= 2,
            "should have debug_assert for both requires and invariant, got {assert_count}: {lib}"
        );
    }

    #[test]
    fn contract_invariant_generates_debug_assert() {
        let project = codegen_ok(
            r#"
contract Stable {
    invariant { true }
    requires  { true }
}
"#,
        );
        let lib = &project.files[0].1;
        // requires produces one debug_assert, invariant produces another
        let assert_count = lib.matches("debug_assert!").count();
        assert!(
            assert_count >= 2,
            "should have debug_assert for both requires and invariant, got {assert_count}"
        );
    }

    // S008: Typestate codegen tests

    #[test]
    fn s008_typestate_marker_structs_generated() {
        let project = codegen_ok(
            r#"
service Door {
    states: Locked -> Unlocked -> Open
    operation Unlock {
        requires { self.state == Locked }
        ensures { self.state == Unlocked }
    }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("pub struct Locked"),
            "should generate Locked marker struct: {lib}"
        );
        assert!(
            lib.contains("pub struct Unlocked"),
            "should generate Unlocked marker struct: {lib}"
        );
        assert!(
            lib.contains("pub struct Open"),
            "should generate Open marker struct: {lib}"
        );
        // Should NOT generate enum State
        assert!(
            !lib.contains("enum State"),
            "typestate should not generate enum State: {lib}"
        );
    }

    #[test]
    fn s008_typestate_generic_struct_with_phantom() {
        let project = codegen_ok(
            r#"
service Door {
    states: Locked -> Unlocked
    operation Unlock {
        requires { self.state == Locked }
        ensures { self.state == Unlocked }
    }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("struct Door<State>"),
            "service struct should be generic: {lib}"
        );
        assert!(
            lib.contains("PhantomData<State>"),
            "struct should contain PhantomData: {lib}"
        );
    }

    #[test]
    fn s008_typestate_constructor_on_initial_state() {
        let project = codegen_ok(
            r#"
service Workflow {
    states: Pending -> Active -> Complete
    operation Activate {
        requires { self.state == Pending }
        ensures { self.state == Active }
    }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("impl Workflow<Pending>"),
            "new() should be on initial state impl block: {lib}"
        );
        assert!(
            lib.contains("fn new()"),
            "should generate constructor: {lib}"
        );
    }

    #[test]
    fn s008_typestate_transition_consumes_self() {
        let project = codegen_ok(
            r#"
service Conn {
    states: Idle -> Active -> Closed
    operation Open {
        requires { self.state == Idle }
        ensures { self.state == Active }
    }
    operation Close {
        requires { self.state == Active }
        ensures { self.state == Closed }
    }
}
"#,
        );
        let lib = &project.files[0].1;
        // Open should be on impl Conn<Idle> and return Conn<Active>
        assert!(
            lib.contains("impl Conn<Idle>"),
            "Open should be in Idle impl: {lib}"
        );
        assert!(
            lib.contains("fn Open(self) -> Conn<Active>"),
            "Open should consume self and return Conn<Active>: {lib}"
        );
        // Close should be on impl Conn<Active> and return Conn<Closed>
        assert!(
            lib.contains("impl Conn<Active>"),
            "Close should be in Active impl: {lib}"
        );
        assert!(
            lib.contains("fn Close(self) -> Conn<Closed>"),
            "Close should consume self and return Conn<Closed>: {lib}"
        );
    }

    #[test]
    fn s008_typestate_stateless_service_unchanged() {
        let project = codegen_ok(
            r#"
service Simple {
    operation DoWork {
        requires { true }
    }
}
"#,
        );
        let lib = &project.files[0].1;
        // Stateless service should NOT have PhantomData or marker structs
        assert!(
            !lib.contains("PhantomData"),
            "stateless service should not use PhantomData: {lib}"
        );
        assert!(
            lib.contains("struct Simple"),
            "stateless service should have simple struct: {lib}"
        );
        assert!(
            lib.contains("fn new()"),
            "stateless service should have constructor: {lib}"
        );
    }

    #[test]
    fn s008_typestate_query_uses_ref_self() {
        let project = codegen_ok(
            r#"
service Store {
    states: Ready -> Done
    query GetCount {
        output(count: Nat)
    }
}
"#,
        );
        let lib = &project.files[0].1;
        // Query without state guard goes in generic impl
        assert!(
            lib.contains("impl<S> Store<S>"),
            "state-independent query should be in generic impl: {lib}"
        );
        assert!(lib.contains("&self"), "query should use &self: {lib}");
    }

    // S009: Proptest generation tests

    #[test]
    fn s009_proptest_generated_for_contract_with_requires_ensures() {
        let project = codegen_ok(
            r#"
contract SafeDivision {
    input(a: Int, b: Int)
    requires { b != 0 }
    ensures { result * b + (a % b) == a }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("proptest!"),
            "should generate proptest macro: {lib}"
        );
        assert!(
            lib.contains("prop_assert!"),
            "ensures should become prop_assert: {lib}"
        );
        assert!(
            lib.contains("#[cfg(test)]"),
            "proptest module should be gated: {lib}"
        );
    }

    #[test]
    fn s009_proptest_refines_strategy_for_neq_zero() {
        let project = codegen_ok(
            r#"
contract Div {
    input(a: Int, b: Int)
    requires { b != 0 }
    ensures { true }
}
"#,
        );
        let lib = &project.files[0].1;
        // b != 0 should be refined to a range strategy, not prop_assume
        // prettyplease may break the range across lines, so check components
        assert!(
            lib.contains("1i64") && lib.contains("i64::MAX"),
            "b != 0 should refine to positive range: {lib}"
        );
        // Should NOT have prop_assume for b since it was refined
        assert!(
            !lib.contains("prop_assume!"),
            "refined requires should not produce prop_assume: {lib}"
        );
    }

    #[test]
    fn s009_proptest_unrefined_requires_becomes_prop_assume() {
        let project = codegen_ok(
            r#"
contract Complex {
    input(x: Int)
    requires { x * x > 10 }
    ensures { true }
}
"#,
        );
        let lib = &project.files[0].1;
        // Complex requires can't be refined, should become prop_assume
        assert!(
            lib.contains("prop_assume!"),
            "complex requires should become prop_assume: {lib}"
        );
    }

    #[test]
    fn s009_proptest_not_generated_without_ensures() {
        let project = codegen_ok(
            r#"
contract NoEnsures {
    input(x: Int)
    requires { x > 0 }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            !lib.contains("proptest!"),
            "contract without ensures should not generate proptest: {lib}"
        );
    }

    #[test]
    fn s009_proptest_not_generated_without_input() {
        let project = codegen_ok(
            r#"
contract NoInput {
    ensures { true }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            !lib.contains("proptest!"),
            "contract without input should not generate proptest: {lib}"
        );
    }

    #[test]
    fn s009_cargo_toml_has_proptest_dev_dependency() {
        let project = codegen_ok(
            r#"
contract Testable {
    input(x: Int)
    ensures { true }
}
"#,
        );
        assert!(
            project.cargo_toml.contains("proptest"),
            "Cargo.toml should include proptest dev-dependency: {}",
            project.cargo_toml
        );
        assert!(
            project.cargo_toml.contains("[dev-dependencies]"),
            "should have dev-dependencies section: {}",
            project.cargo_toml
        );
    }

    #[test]
    fn s009_cargo_toml_no_proptest_when_not_needed() {
        let project = codegen_ok(
            r#"
contract NotTestable {
    requires { true }
}
"#,
        );
        assert!(
            !project.cargo_toml.contains("proptest"),
            "Cargo.toml should not include proptest when no testable contracts: {}",
            project.cargo_toml
        );
    }

    // R002: Multi-file codegen tests

    #[test]
    fn single_contract_stays_in_lib_rs() {
        // A file with only one contract should produce a single lib.rs
        let project = codegen_ok(
            r#"
contract OnlyOne {
    input(x: Int)
    requires { x > 0 }
}
"#,
        );
        assert_eq!(project.files.len(), 1, "single contract = single file");
        assert_eq!(project.files[0].0, "src/lib.rs");
        assert!(
            project.files[0].1.contains("pub mod contract_onlyone"),
            "single contract should have inline module in lib.rs"
        );
    }

    #[test]
    fn multi_contract_generates_separate_files() {
        let project = codegen_ok(
            r#"
type Point {
    x: Int
    y: Int
}

contract Alpha {
    input(a: Int)
    requires { a > 0 }
}

contract Beta {
    input(b: Int)
    ensures { result > 0 }
}
"#,
        );
        // Should produce: lib.rs, contract_alpha.rs, contract_beta.rs
        assert_eq!(
            project.files.len(),
            3,
            "two contracts = 3 files: {:?}",
            project.files.iter().map(|(p, _)| p).collect::<Vec<_>>()
        );

        let find_file = |name: &str| -> &str {
            &project
                .files
                .iter()
                .find(|(p, _)| p == name)
                .unwrap_or_else(|| {
                    panic!(
                        "missing {name}: {:?}",
                        project.files.iter().map(|(p, _)| p).collect::<Vec<_>>()
                    )
                })
                .1
        };

        let lib = find_file("src/lib.rs");
        // Shared types stay in lib.rs
        assert!(lib.contains("struct Point"), "shared type in lib.rs");
        // Module declarations for contracts
        assert!(
            lib.contains("pub mod contract_alpha;"),
            "lib.rs declares alpha module"
        );
        assert!(
            lib.contains("pub mod contract_beta;"),
            "lib.rs declares beta module"
        );
        // Contracts themselves should NOT be in lib.rs
        assert!(
            !lib.contains("pub fn check(a: i64)"),
            "contract body should not be inline in lib.rs"
        );

        let alpha = find_file("src/contract_alpha.rs");
        assert!(alpha.contains("use super::*"), "module imports parent");
        assert!(
            alpha.contains("pub fn check(a: i64)"),
            "alpha has check fn: {alpha}"
        );

        let beta = find_file("src/contract_beta.rs");
        assert!(
            beta.contains("pub fn check(b: i64)"),
            "beta has check fn: {beta}"
        );
    }

    #[test]
    fn multi_file_with_service() {
        let project = codegen_ok(
            r#"
contract Guard {
    input(x: Int)
    requires { x >= 0 }
}

service Counter {
    states: Idle -> Running -> Done
    operation Start {
        requires: true
        effects: io
    }
}
"#,
        );
        // contract + service = 2 modules -> multi-file
        assert_eq!(
            project.files.len(),
            3,
            "contract + service = 3 files: {:?}",
            project.files.iter().map(|(p, _)| p).collect::<Vec<_>>()
        );

        let find_file = |name: &str| -> &str {
            &project
                .files
                .iter()
                .find(|(p, _)| p == name)
                .unwrap_or_else(|| {
                    panic!(
                        "missing {name}: {:?}",
                        project.files.iter().map(|(p, _)| p).collect::<Vec<_>>()
                    )
                })
                .1
        };

        let lib = find_file("src/lib.rs");
        assert!(lib.contains("pub mod contract_guard;"));
        assert!(lib.contains("pub mod counter;"));

        let guard = find_file("src/contract_guard.rs");
        assert!(guard.contains("pub fn check"));

        let counter = find_file("src/counter.rs");
        assert!(counter.contains("struct Counter"));
        // S008: typestate encoding generates marker structs, not enum State
        assert!(
            counter.contains("pub struct Idle"),
            "should contain state marker struct: {counter}"
        );
    }

    #[test]
    fn multi_file_shared_types_not_duplicated() {
        // Types, enums, and externs stay in lib.rs, not in contract files
        let project = codegen_ok(
            r#"
type MyData {
    value: Int
}

enum Status {
    Active,
    Inactive
}

contract First {
    input(x: Int)
    requires { x > 0 }
}

contract Second {
    input(y: Int)
    ensures { result > 0 }
}
"#,
        );

        let find_file = |name: &str| -> &str {
            &project
                .files
                .iter()
                .find(|(p, _)| p == name)
                .unwrap_or_else(|| {
                    panic!(
                        "missing {name}: {:?}",
                        project.files.iter().map(|(p, _)| p).collect::<Vec<_>>()
                    )
                })
                .1
        };

        let lib = find_file("src/lib.rs");
        assert!(
            lib.contains("struct MyData"),
            "shared type in lib.rs: {lib}"
        );
        assert!(lib.contains("enum Status"), "shared enum in lib.rs: {lib}");

        let first = find_file("src/contract_first.rs");
        assert!(
            !first.contains("struct MyData"),
            "type should not be in contract file"
        );
        assert!(
            !first.contains("enum Status"),
            "enum should not be in contract file"
        );
    }

    // --- P004: Error handling codegen tests ---

    #[test]
    fn contract_with_errors_generates_error_enum() {
        let project = codegen_ok(
            r#"
contract SafeDivision {
    input  { a: Int, b: Int }
    output { result: Int }
    errors { DivByZero, Overflow }
    requires { b != 0 }
    ensures  { result * b == a }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("SafeDivisionError"),
            "should generate error enum: {lib}"
        );
        assert!(
            lib.contains("DivByZero"),
            "should contain DivByZero variant: {lib}"
        );
        assert!(
            lib.contains("Overflow"),
            "should contain Overflow variant: {lib}"
        );
        assert!(
            lib.contains("thiserror::Error"),
            "should derive thiserror::Error: {lib}"
        );
    }

    #[test]
    fn contract_with_errors_returns_result() {
        let project = codegen_ok(
            r#"
contract Validator {
    input  { data: Int }
    output { result: Bool }
    errors { InvalidInput }
    ensures { result == true }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(lib.contains("Result<"), "should return Result type: {lib}");
        assert!(
            lib.contains("ValidatorError"),
            "should reference error type: {lib}"
        );
        assert!(
            lib.contains("Ok(__result)"),
            "should wrap result in Ok: {lib}"
        );
    }

    #[test]
    fn contract_without_errors_no_result_type() {
        let project = codegen_ok(
            r#"
contract Simple {
    input  { x: Int }
    output { result: Int }
    requires { x > 0 }
    ensures  { result > 0 }
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            !lib.contains("Result<"),
            "should not use Result without errors: {lib}"
        );
        assert!(
            !lib.contains("thiserror"),
            "should not include thiserror: {lib}"
        );
    }

    #[test]
    fn extract_error_variants_from_raw() {
        let body = Expr::Raw(vec!["DivByZero".into(), ",".into(), "Overflow".into()]);
        let variants = extract_error_variants(&body);
        assert_eq!(variants, vec!["DivByZero", "Overflow"]);
    }

    #[test]
    fn extract_error_variants_from_ident() {
        let body = Expr::Ident("SingleError".into());
        let variants = extract_error_variants(&body);
        assert_eq!(variants, vec!["SingleError"]);
    }

    #[test]
    fn generate_error_enum_output() {
        let mut code = String::new();
        generate_error_enum("Parser", &["BadInput".into(), "TooLong".into()], &mut code);
        assert!(code.contains("pub enum ParserError"));
        assert!(code.contains("BadInput"));
        assert!(code.contains("TooLong"));
        assert!(code.contains("thiserror::Error"));
    }

    #[test]
    fn errors_clause_adds_thiserror_dep() {
        let project = codegen_ok(
            r#"
contract WithErrors {
    input  { x: Int }
    errors { SomeError }
    requires { x > 0 }
}
"#,
        );
        assert!(
            project.cargo_toml.contains("thiserror"),
            "Cargo.toml should include thiserror: {}",
            project.cargo_toml
        );
    }

    #[test]
    fn no_errors_no_thiserror_dep() {
        let project = codegen_ok(
            r#"
contract NoErrors {
    input  { x: Int }
    requires { x > 0 }
}
"#,
        );
        assert!(
            !project.cargo_toml.contains("thiserror"),
            "Cargo.toml should not include thiserror: {}",
            project.cargo_toml
        );
    }

    // --- WASM target tests ---

    fn codegen_wasm(source: &str) -> GeneratedProject {
        let file = assura_parser::parse_unwrap(source);
        let resolved = assura_resolve::resolve(&file).expect("resolve failed");
        let typed = assura_types::type_check(&resolved).expect("type check failed");
        let config = BackendConfig {
            target: CompileTarget::Wasm,
            ..BackendConfig::default()
        };
        codegen_with_config(&typed, &config)
    }

    #[test]
    fn wasm_target_cargo_toml_has_comment() {
        let project = codegen_wasm("");
        assert!(
            project.cargo_toml.contains("wasm32-wasip1"),
            "WASM Cargo.toml should mention wasm32-wasip1: {}",
            project.cargo_toml
        );
    }

    #[test]
    fn native_target_no_wasm_comment() {
        let project = codegen_ok("");
        assert!(
            !project.cargo_toml.contains("wasm32-wasip1"),
            "Native Cargo.toml should not mention wasm32-wasip1"
        );
    }

    #[test]
    fn compile_target_from_str() {
        assert_eq!(
            CompileTarget::from_str_loose("native"),
            Some(CompileTarget::Native)
        );
        assert_eq!(
            CompileTarget::from_str_loose("wasm"),
            Some(CompileTarget::Wasm)
        );
        assert_eq!(
            CompileTarget::from_str_loose("wasm32-wasi"),
            Some(CompileTarget::Wasm)
        );
        assert_eq!(
            CompileTarget::from_str_loose("wasm32-wasip1"),
            Some(CompileTarget::Wasm)
        );
        assert_eq!(CompileTarget::from_str_loose("unknown"), None);
    }

    #[test]
    fn compile_target_rust_target() {
        assert_eq!(CompileTarget::Native.rust_target(), None);
        assert_eq!(CompileTarget::Wasm.rust_target(), Some("wasm32-wasip1"));
    }

    // --- Bind codegen tests ---

    #[test]
    fn bind_generates_checked_wrapper() {
        let project = codegen_ok(
            r#"
bind "std::cmp::max" as safe_max {
    input(a: Int, b: Int)
    output(result: Int)
    requires a >= 0
    ensures result >= a
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("fn safe_max"),
            "should generate safe_max fn: {lib}"
        );
        assert!(
            lib.contains("std::cmp::max("),
            "should call the bound Rust function: {lib}"
        );
        assert!(
            lib.contains("debug_assert!"),
            "should have contract assertions: {lib}"
        );
    }

    #[test]
    fn bind_with_requires_and_ensures() {
        let project = codegen_ok(
            r#"
bind "my_crate::divide" as safe_divide {
    input(a: Int, b: Int)
    output(result: Int)
    requires b != 0
    ensures result * b == a
}
"#,
        );
        let lib = &project.files[0].1;
        assert!(
            lib.contains("fn safe_divide"),
            "should generate safe_divide fn: {lib}"
        );
        assert!(
            lib.contains("my_crate::divide("),
            "should call the bound Rust function: {lib}"
        );
    }

    #[test]
    fn collect_type_refs_from_nested_exprs() {
        use assura_parser::ast::*;
        let mut out = std::collections::HashSet::new();

        // Type ref inside a Match arm body
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("x".into())),
            arms: vec![MatchArm {
                pattern: Pattern::Wildcard,
                body: Expr::Call {
                    func: Box::new(Expr::Ident("MyType".into())),
                    args: vec![],
                },
            }],
        };
        collect_type_refs_from_expr(&expr, &mut out);
        assert!(
            out.contains("MyType"),
            "should find type ref in Match arm body, got: {out:?}"
        );

        // Type ref inside a Let body
        out.clear();
        let expr = Expr::Let {
            name: "x".into(),
            value: Box::new(Expr::Literal(Literal::Int("1".into()))),
            body: Box::new(Expr::Ident("SomeType".into())),
        };
        collect_type_refs_from_expr(&expr, &mut out);
        assert!(
            out.contains("SomeType"),
            "should find type ref in Let body, got: {out:?}"
        );

        // Type ref inside a Tuple
        out.clear();
        let expr = Expr::Tuple(vec![
            Expr::Ident("x".into()),
            Expr::Ident("CustomStruct".into()),
        ]);
        collect_type_refs_from_expr(&expr, &mut out);
        assert!(
            out.contains("CustomStruct"),
            "should find type ref in Tuple, got: {out:?}"
        );
    }

    #[test]
    fn feature_max_missing_value_emits_compile_error() {
        // A feature_max referenced but with no extractable value should
        // produce compile_error! in the generated code, not a silent "0".
        let source = r#"
feature_max UNKNOWN_CONST

contract UseConst {
    input(x: Int)
    requires { x <= UNKNOWN_CONST }
}
"#;
        let code = codegen_ok(source);
        let lib = &code.files[0].1;
        assert!(
            lib.contains("compile_error!"),
            "missing feature_max value should produce compile_error!, got:\n{lib}"
        );
    }

    // -------------------------------------------------------------------
    // Issue #54: catch-all wildcard elimination tests
    // -------------------------------------------------------------------

    #[test]
    fn extract_output_type_recurses_into_call_args() {
        // Previously a Tuple arg inside Call was silently skipped via _ => {}
        let body = Expr::Call {
            func: Box::new(Expr::Ident("output".into())),
            args: vec![Expr::Tuple(vec![Expr::Cast {
                expr: Box::new(Expr::Ident("x".into())),
                ty: "Int".into(),
            }])],
        };
        let ty = extract_output_type(&body);
        assert_eq!(ty, "i64", "should recurse into Tuple inside Call args");
    }

    #[test]
    fn extract_error_variants_from_block() {
        // Previously Block fell through to _ => vec![]
        let body = Expr::Block(vec![
            Expr::Ident("ErrA".into()),
            Expr::Ident("ErrB".into()),
        ]);
        let variants = extract_error_variants(&body);
        assert_eq!(variants, vec!["ErrA", "ErrB"]);
    }

    #[test]
    fn extract_error_variants_from_list() {
        let body = Expr::List(vec![
            Expr::Ident("X".into()),
            Expr::Ident("Y".into()),
        ]);
        let variants = extract_error_variants(&body);
        assert_eq!(variants, vec!["X", "Y"]);
    }

    #[test]
    fn extract_error_variants_from_paren() {
        let body = Expr::Paren(Box::new(Expr::Ident("Wrapped".into())));
        let variants = extract_error_variants(&body);
        assert_eq!(variants, vec!["Wrapped"]);
    }

    #[test]
    fn extract_error_variants_non_ident_returns_empty() {
        // BinOp cannot contain error variant names
        let body = Expr::BinOp {
            lhs: Box::new(Expr::Ident("a".into())),
            op: assura_parser::ast::BinOp::Add,
            rhs: Box::new(Expr::Ident("b".into())),
        };
        let variants = extract_error_variants(&body);
        assert!(variants.is_empty());
    }

    #[test]
    fn generate_trait_method_unsupported_emits_compile_error() {
        // Previously unsupported Expr variants got a silent comment
        let body = Expr::Literal(assura_parser::ast::Literal::Int("42".into()));
        let mut code = String::new();
        generate_trait_method(&body, &mut code);
        assert!(
            code.contains("compile_error!"),
            "unsupported trait method body should emit compile_error!, got:\n{code}"
        );
    }

    #[test]
    fn generate_block_effects_clause_explicit() {
        // Previously Effects fell through to _ => debug comment
        let clauses = vec![Clause {
            kind: ClauseKind::Effects,
            body: Expr::Raw(vec!["io".into()]),
        }];
        let mut code = String::new();
        generate_block("feature", "test", &clauses, &mut code);
        assert!(
            code.contains("/// Effects:"),
            "Effects clause should produce doc comment, got:\n{code}"
        );
    }
}
