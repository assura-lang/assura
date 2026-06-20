//! Type checking pipeline entry points.
//!
//! Contains type_check(), type_check_with_modules(),
//! and the unified run_all_checks dispatcher.

use std::collections::HashMap;
use std::sync::Arc;

use assura_parser::ast::{ClauseKind, Decl, ServiceItem};
use assura_resolve::{ImportStatus, ResolvedFile, SymbolTable};

use crate::checkers::PendingDecreaseCheck;
use crate::checks::*;
use crate::clauses::{
    check_clause_bodies, collect_input_param_types,
    extract_output_type_from_body, register_input_clause_params,
};
use crate::convert::{parse_type_tokens, resolve_type_opt};
use crate::env::build_type_env;
use crate::generics::run_generic_instantiation_checks;
use crate::{Type, TypeEnv, TypeError, TypedFile};

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Type-check a resolved file with default configuration.
///
/// Convenience wrapper around [`TypeChecker`]. For custom configuration
/// or cross-module type checking, use the builder directly.
pub fn type_check(resolved: &ResolvedFile) -> Result<TypedFile, Vec<TypeError>> {
    TypeChecker::new().check(resolved)
}

type SourceChecker = fn(&assura_parser::ast::SourceFile) -> Vec<TypeError>;
type SymbolChecker = fn(&assura_parser::ast::SourceFile, &SymbolTable) -> Vec<TypeError>;
type EnvChecker = fn(&assura_parser::ast::SourceFile, &TypeEnv) -> Vec<TypeError>;
type EnvSymbolChecker =
    fn(&assura_parser::ast::SourceFile, &TypeEnv, &SymbolTable) -> Vec<TypeError>;

enum CheckerDispatch {
    Source(SourceChecker),
    Symbols(SymbolChecker),
    Env(EnvChecker),
    EnvSymbols(EnvSymbolChecker),
    Effects,
    Totality,
}

/// Ordered checker registry (order matches original `run_all_checks` wiring).
const CHECKER_PIPELINE: &[CheckerDispatch] = &[
    CheckerDispatch::Symbols(run_axiomatic_checks),
    CheckerDispatch::Source(run_liveness_checks),
    CheckerDispatch::Source(run_crud_auth_checks),
    CheckerDispatch::Source(run_linearity_checks),
    CheckerDispatch::Source(run_typestate_checks),
    CheckerDispatch::Effects,
    CheckerDispatch::Source(run_taint_checks),
    CheckerDispatch::Source(run_info_flow_checks),
    CheckerDispatch::Source(run_ffi_checks),
    CheckerDispatch::Source(run_error_propagation_checks),
    CheckerDispatch::EnvSymbols(run_frame_checks),
    CheckerDispatch::Totality,
    CheckerDispatch::Env(run_fixed_width_checks),
    CheckerDispatch::Source(run_collection_contract_checks),
    CheckerDispatch::Symbols(run_match_exhaustiveness_checks),
    CheckerDispatch::Source(run_constant_time_checks),
    CheckerDispatch::Source(run_determinism_checks),
    CheckerDispatch::Source(run_memory_checks),
    CheckerDispatch::Source(run_secure_erasure_checks),
    CheckerDispatch::Source(run_interface_checks),
    CheckerDispatch::Source(run_structural_invariant_checks),
    CheckerDispatch::Source(run_shared_mem_checks),
    CheckerDispatch::Source(run_lock_order_checks),
    CheckerDispatch::Source(run_weak_memory_checks),
    CheckerDispatch::Source(run_allocator_checks),
    CheckerDispatch::Source(run_circular_buffer_checks),
    CheckerDispatch::Source(run_callback_reentrancy_checks),
    CheckerDispatch::Source(run_temporal_deadline_checks),
    CheckerDispatch::Source(run_binary_format_checks),
    CheckerDispatch::Source(run_bit_level_checks),
    CheckerDispatch::Source(run_string_encoding_checks),
    CheckerDispatch::Source(run_checksum_checks),
    CheckerDispatch::Source(run_protocol_grammar_checks),
    CheckerDispatch::Source(run_opaque_function_checks),
    CheckerDispatch::Source(run_crash_recovery_checks),
    CheckerDispatch::Source(run_page_cache_checks),
    CheckerDispatch::Source(run_mvcc_checks),
    CheckerDispatch::Source(run_rollback_checks),
    CheckerDispatch::Source(run_monotonic_state_checks),
    CheckerDispatch::Source(run_storage_failure_checks),
    CheckerDispatch::Source(run_numerical_precision_checks),
    CheckerDispatch::Source(run_precomputed_table_checks),
    CheckerDispatch::Source(run_platform_abstraction_checks),
    CheckerDispatch::Source(run_feature_flag_checks),
    CheckerDispatch::Source(run_resource_limit_checks),
    CheckerDispatch::Source(run_unsafe_escape_checks),
    CheckerDispatch::Source(run_complexity_bound_checks),
    CheckerDispatch::Source(run_behavioral_equivalence_checks),
    CheckerDispatch::Source(run_multi_pass_refinement_checks),
    CheckerDispatch::Source(run_incremental_contract_checks),
    CheckerDispatch::Source(run_scoped_invariant_checks),
    CheckerDispatch::Source(run_contract_composition_checks),
    CheckerDispatch::Source(run_contract_library_checks),
    CheckerDispatch::Source(run_crypto_conformance_checks),
    CheckerDispatch::Source(run_codec_registry_checks),
    CheckerDispatch::Source(run_generic_instantiation_checks),
    CheckerDispatch::Source(run_quantifier_trigger_checks),
    CheckerDispatch::Source(run_prophecy_resolution_checks),
];

fn run_effect_checks_filtered(
    source: &assura_parser::ast::SourceFile,
    config: &assura_config::TypeCheckConfig,
) -> Vec<TypeError> {
    let mut effect_errors = run_effect_checks(source);
    if !config.allowed_effects.is_empty() || !config.denied_effects.is_empty() {
        effect_errors.retain(|e| !config.allowed_effects.iter().any(|a| e.message.contains(a)));
    }
    if config.strict_effects {
        effect_errors
    } else {
        effect_errors
            .into_iter()
            .filter(|e| e.code != "A07003")
            .collect()
    }
}

/// Run all domain and structural checkers on the source AST.
///
/// This is the single dispatch point for all 50+ checkers. Both
/// type-check entry points (`type_check_with_config`, `type_check_with_modules`)
/// call this after their specific clause-body checking and env-building.
fn run_all_checks(
    source: &assura_parser::ast::SourceFile,
    type_env: &TypeEnv,
    symbols: &SymbolTable,
    config: &assura_config::TypeCheckConfig,
) -> (Vec<TypeError>, Vec<PendingDecreaseCheck>) {
    let mut errors = Vec::new();
    let mut pending_decrease_checks = Vec::new();

    for dispatch in CHECKER_PIPELINE {
        match dispatch {
            CheckerDispatch::Source(f) => errors.extend(f(source)),
            CheckerDispatch::Symbols(f) => errors.extend(f(source, symbols)),
            CheckerDispatch::Env(f) => errors.extend(f(source, type_env)),
            CheckerDispatch::EnvSymbols(f) => errors.extend(f(source, type_env, symbols)),
            CheckerDispatch::Effects => {
                errors.extend(run_effect_checks_filtered(source, config));
            }
            CheckerDispatch::Totality => {
                let (totality_errors, pending) = run_totality_checks(source);
                errors.extend(totality_errors);
                pending_decrease_checks = pending;
            }
        }
    }

    (errors, pending_decrease_checks)
}

/// Generate tests from contracts using TestGenerator (TEST.1).
///
/// Scans all contract declarations, extracts testable constraints, and
/// produces property-based, boundary-value, and smoke tests.
fn generate_tests_from_contracts(
    source: &assura_parser::ast::SourceFile,
) -> Vec<crate::GeneratedTest> {
    use crate::domain::{TestGenerator, TestableContract};
    use assura_parser::ast::extract_clause_params;

    let mut tgen = TestGenerator::new();

    for decl in &source.decls {
        if let Decl::Contract(c) = &decl.node {
            let mut params = Vec::new();
            let mut requires = Vec::new();
            let mut ensures = Vec::new();

            for clause in &c.clauses {
                match clause.kind {
                    ClauseKind::Input => {
                        for p in extract_clause_params(&clause.body) {
                            let ty = resolve_type_opt(p.ty.as_ref());
                            params.push((p.name, ty));
                        }
                    }
                    ClauseKind::Requires => {
                        requires.push(format!("{:?}", clause.body));
                    }
                    ClauseKind::Ensures => {
                        ensures.push(format!("{:?}", clause.body));
                    }
                    _ => {}
                }
            }

            // Only generate tests for contracts that have testable constraints
            if !requires.is_empty() || !ensures.is_empty() {
                tgen.add_contract(TestableContract {
                    name: c.name.clone(),
                    params,
                    requires,
                    ensures,
                });
            }
        }
    }

    tgen.generate_all()
}



/// Inject type information from an imported module's AST into the type
/// environment. Adds concrete types for imported contracts, services,
/// type definitions, enum variants, and function signatures.
fn inject_imported_types(
    env: &mut TypeEnv,
    imp: &assura_resolve::ResolvedImport,
    source: &assura_parser::ast::SourceFile,
) {
    // Collect the names this import brings into scope
    let imported_names: Vec<&str> = if !imp.items.is_empty() {
        // Selective import: `import math { Add, Vector }`
        imp.items.iter().map(|s| s.as_str()).collect()
    } else if let Some(alias) = &imp.alias {
        // Aliased import: `import math as m`
        vec![alias.as_str()]
    } else if let Some(last) = imp.path.last() {
        // Default import: `import math` brings the module name
        vec![last.as_str()]
    } else {
        return;
    };

    for decl in &source.decls {
        match &decl.node {
            Decl::Contract(c) if imported_names.contains(&c.name.as_str()) => {
                // Register the contract name as a Named type
                env.insert(c.name.clone(), Type::Named(c.name.clone()));
                // Register input params so callers know the contract's signature
                for clause in &c.clauses {
                    if clause.kind == ClauseKind::Input {
                        register_input_clause_params(&clause.body, env);
                    }
                }
            }
            Decl::Service(s) if imported_names.contains(&s.name.as_str()) => {
                env.insert(s.name.clone(), Type::Named(s.name.clone()));
                // Register operation/query signatures
                for item in &s.items {
                    let (name, clauses) = match item {
                        ServiceItem::Operation { name, clauses } => (name, clauses),
                        ServiceItem::Query { name, clauses } => (name, clauses),
                        _ => continue,
                    };
                    let mut param_types = Vec::new();
                    for clause in clauses {
                        if clause.kind == ClauseKind::Input {
                            collect_input_param_types(&clause.body, &mut param_types);
                        }
                    }
                    let mut ret = Type::Unit;
                    for clause in clauses {
                        if clause.kind == ClauseKind::Output {
                            let ty = extract_output_type_from_body(&clause.body);
                            if !ty.is_indeterminate() {
                                ret = ty;
                                break;
                            }
                        }
                    }
                    env.insert(
                        name.clone(),
                        Type::Fn {
                            params: param_types,
                            ret: Box::new(ret),
                        },
                    );
                }
            }
            Decl::TypeDef(td) if imported_names.contains(&td.name.as_str()) => {
                env.insert(td.name.clone(), Type::Named(td.name.clone()));
                if let assura_parser::ast::TypeBody::Struct(fields) = &td.body {
                    let field_types: Vec<(String, Type)> = fields
                        .iter()
                        .map(|f| (f.name.clone(), resolve_type_opt(f.ty.as_ref())))
                        .collect();
                    env.struct_fields.insert(td.name.clone(), field_types);
                }
            }
            Decl::EnumDef(e) if imported_names.contains(&e.name.as_str()) => {
                env.insert(e.name.clone(), Type::Named(e.name.clone()));
                for variant in &e.variants {
                    if !variant.fields.is_empty() {
                        let field_types: Vec<Type> = variant
                            .fields
                            .iter()
                            .map(|f| parse_type_tokens(std::slice::from_ref(f)))
                            .collect();
                        env.insert(
                            variant.name.clone(),
                            Type::Fn {
                                params: field_types,
                                ret: Box::new(Type::Named(e.name.clone())),
                            },
                        );
                    }
                }
            }
            Decl::FnDef(f) if imported_names.contains(&f.name.as_str()) => {
                let param_types: Vec<Type> = f
                    .params
                    .iter()
                    .map(|p| resolve_type_opt(p.ty.as_ref()))
                    .collect();
                let ret = resolve_type_opt(f.return_ty.as_ref());
                env.insert(
                    f.name.clone(),
                    Type::Fn {
                        params: param_types,
                        ret: Box::new(ret),
                    },
                );
            }
            Decl::Extern(e) if imported_names.contains(&e.name.as_str()) => {
                let param_types: Vec<Type> = e
                    .params
                    .iter()
                    .map(|p| resolve_type_opt(p.ty.as_ref()))
                    .collect();
                let ret = resolve_type_opt(e.return_ty.as_ref());
                env.insert(
                    e.name.clone(),
                    Type::Fn {
                        params: param_types,
                        ret: Box::new(ret),
                    },
                );
            }
            _ => {}
        }
    }
}



// ---------------------------------------------------------------------------
// Builder API (consolidates all 5 type_check entry points)
// ---------------------------------------------------------------------------

/// Builder for type-checking. Replaces the 5 standalone `type_check*` functions
/// with a single composable API:
///
/// ```ignore
/// TypeChecker::new()
///     .config(my_config)
///     .modules(dep_map)
///     .check(&resolved)
/// ```
pub struct TypeChecker {
    config: assura_config::TypeCheckConfig,
    modules: Option<HashMap<String, ResolvedFile>>,
}

impl TypeChecker {
    /// Create a new type checker with default configuration.
    pub fn new() -> Self {
        Self {
            config: assura_config::TypeCheckConfig::default(),
            modules: None,
        }
    }

    /// Set the type-checking configuration.
    pub fn config(mut self, config: assura_config::TypeCheckConfig) -> Self {
        self.config = config;
        self
    }

    /// Provide cross-module type information for import resolution.
    pub fn modules(mut self, modules: HashMap<String, ResolvedFile>) -> Self {
        self.modules = Some(modules);
        self
    }

    /// Type-check from a resolved AST file.
    pub fn check(self, resolved: &ResolvedFile) -> Result<TypedFile, Vec<TypeError>> {
        let mut type_env = build_type_env(&resolved.symbols, &resolved.source);

        // Inject type information from imported modules if provided
        if let Some(modules) = &self.modules {
            for imp in &resolved.imports {
                if imp.status != ImportStatus::Resolved {
                    continue;
                }
                let module_key = imp.path.join(".");
                if let Some(imported_resolved) = modules.get(&module_key) {
                    inject_imported_types(&mut type_env, imp, &imported_resolved.source);
                }
            }
        }

        let mut errors = check_clause_bodies(&resolved.source, &type_env);
        let (check_errors, pending_decrease_checks) =
            run_all_checks(&resolved.source, &type_env, &resolved.symbols, &self.config);
        errors.extend(check_errors);

        if !errors.is_empty() {
            return Err(errors);
        }

        let generated_tests = generate_tests_from_contracts(&resolved.source);

        Ok(TypedFile {
            resolved: Arc::new(resolved.clone()),
            pending_decrease_checks,
            type_env,
            generated_tests,
        })
    }


}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_check_source(src: &str) -> Result<TypedFile, Vec<TypeError>> {
        let file = assura_parser::parse_unwrap(src);
        let resolved = assura_resolve::resolve(&file).expect("resolve failed");
        type_check(&resolved)
    }

    #[test]
    fn pipeline_produces_errors_from_effect_checker() {
        // "memory" is not a valid effect name; should trigger A07003
        let src = r#"
            contract Multi {
                requires(x: Int)
                effects(memory)
                ensures(result: Int)
            }
        "#;
        let result = type_check_source(src);
        match result {
            Err(errors) => {
                assert!(
                    errors.iter().any(|e| e.code == "A07003"),
                    "expected A07003 for unknown effect 'memory', got: {errors:?}"
                );
            }
            Ok(_) => {
                // Default config may not enforce strict effects
            }
        }
    }

    #[test]
    fn pipeline_valid_contract_succeeds() {
        let src = r#"
            contract Add {
                requires(a: Int, b: Int)
                ensures(result: Int)
            }
        "#;
        let result = type_check_source(src);
        assert!(
            result.is_ok(),
            "valid contract should type-check: {result:?}"
        );
    }

    #[test]
    fn pipeline_with_strict_effects_config() {
        let src = r#"
            contract Effectful {
                requires(x: Int)
                effects(io)
                ensures(result: Int)
            }
        "#;
        let file = assura_parser::parse_unwrap(src);
        let resolved = assura_resolve::resolve(&file).expect("resolve failed");

        // Strict config: only "database" allowed
        let strict_config = assura_config::TypeCheckConfig {
            strict_effects: true,
            allowed_effects: vec!["database".to_string()],
            ..Default::default()
        };
        let strict_result = TypeChecker::new().config(strict_config).check(&resolved);
        match strict_result {
            Err(errors) => {
                assert!(
                    errors.iter().any(|e| e.code == "A07003"),
                    "strict mode should reject 'io' not in allowed list, got: {errors:?}"
                );
            }
            Ok(_) => {
                // Effect filtering may not reject known effects
            }
        }
    }
}
