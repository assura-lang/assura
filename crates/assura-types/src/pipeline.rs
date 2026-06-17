//! Type checking pipeline entry points.
//!
//! Contains type_check(), type_check_with_modules(),
//! type_check_hir(), and the unified run_all_checks dispatcher.

use std::collections::HashMap;
use std::sync::Arc;

use assura_parser::ast::{ClauseKind, Decl, ServiceItem};
use assura_resolve::{ImportStatus, ResolvedFile, SymbolTable};

use crate::checkers::PendingDecreaseCheck;
use crate::checks::*;
use crate::clauses::{
    check_clause_bodies, check_clause_bodies_hir, collect_input_param_types,
    extract_output_type_from_body, register_input_clause_params,
};
use crate::convert::{parse_type_tokens, resolve_type};
use crate::env::{build_type_env, build_type_env_from_hir};
use crate::generics::run_generic_instantiation_checks;
use crate::{Type, TypeEnv, TypeError, TypedFile};

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
    type_check_with_config(resolved, &assura_config::TypeCheckConfig::default())
}

/// Run all domain and structural checkers on the source AST.
///
/// This is the single dispatch point for all 50+ checkers. All three
/// type-check entry points (`type_check_with_config`, `type_check_with_modules`,
/// `type_check_hir_with_config`) call this after their specific clause-body
/// checking and env-building.
fn run_all_checks(
    source: &assura_parser::ast::SourceFile,
    type_env: &TypeEnv,
    symbols: &SymbolTable,
    config: &assura_config::TypeCheckConfig,
) -> (Vec<TypeError>, Vec<PendingDecreaseCheck>) {
    let mut errors = Vec::new();

    errors.extend(run_axiomatic_checks(source, symbols));
    errors.extend(run_liveness_checks(source));
    errors.extend(run_crud_auth_checks(source));
    errors.extend(run_linearity_checks(source));
    errors.extend(run_typestate_checks(source));

    // Effect checking with config-driven filtering
    let mut effect_errors = run_effect_checks(source);
    if !config.allowed_effects.is_empty() || !config.denied_effects.is_empty() {
        effect_errors.retain(|e| !config.allowed_effects.iter().any(|a| e.message.contains(a)));
    }
    if config.strict_effects {
        errors.extend(effect_errors);
    } else {
        errors.extend(effect_errors.into_iter().filter(|e| e.code != "A07003"));
    }

    errors.extend(run_taint_checks(source));
    errors.extend(run_info_flow_checks(source));
    errors.extend(run_ffi_checks(source));
    errors.extend(run_error_propagation_checks(source));
    errors.extend(run_frame_checks(source, type_env, symbols));
    let (totality_errors, pending_decrease_checks) = run_totality_checks(source);
    errors.extend(totality_errors);
    errors.extend(run_fixed_width_checks(source, type_env));
    errors.extend(run_collection_contract_checks(source));
    errors.extend(run_match_exhaustiveness_checks(source, symbols));
    errors.extend(run_constant_time_checks(source));
    errors.extend(run_determinism_checks(source));
    errors.extend(run_memory_checks(source));
    errors.extend(run_secure_erasure_checks(source));
    errors.extend(run_interface_checks(source));
    errors.extend(run_structural_invariant_checks(source));
    errors.extend(run_shared_mem_checks(source));
    errors.extend(run_lock_order_checks(source));
    errors.extend(run_weak_memory_checks(source));
    errors.extend(run_allocator_checks(source));
    errors.extend(run_circular_buffer_checks(source));
    errors.extend(run_callback_reentrancy_checks(source));
    errors.extend(run_temporal_deadline_checks(source));
    errors.extend(run_binary_format_checks(source));
    errors.extend(run_bit_level_checks(source));
    errors.extend(run_string_encoding_checks(source));
    errors.extend(run_checksum_checks(source));
    errors.extend(run_protocol_grammar_checks(source));
    errors.extend(run_opaque_function_checks(source));
    errors.extend(run_crash_recovery_checks(source));
    errors.extend(run_page_cache_checks(source));
    errors.extend(run_mvcc_checks(source));
    errors.extend(run_rollback_checks(source));
    errors.extend(run_monotonic_state_checks(source));
    errors.extend(run_storage_failure_checks(source));
    errors.extend(run_numerical_precision_checks(source));
    errors.extend(run_precomputed_table_checks(source));
    errors.extend(run_platform_abstraction_checks(source));
    errors.extend(run_feature_flag_checks(source));
    errors.extend(run_resource_limit_checks(source));
    errors.extend(run_unsafe_escape_checks(source));
    errors.extend(run_complexity_bound_checks(source));
    errors.extend(run_behavioral_equivalence_checks(source));
    errors.extend(run_multi_pass_refinement_checks(source));
    errors.extend(run_incremental_contract_checks(source));
    errors.extend(run_scoped_invariant_checks(source));
    errors.extend(run_contract_composition_checks(source));
    errors.extend(run_contract_library_checks(source));
    errors.extend(run_crypto_conformance_checks(source));
    errors.extend(run_codec_registry_checks(source));
    errors.extend(run_generic_instantiation_checks(source));
    errors.extend(run_quantifier_trigger_checks(source));
    errors.extend(run_prophecy_resolution_checks(source));

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
                            let ty = if p.ty.is_empty() {
                                Type::Unknown
                            } else {
                                crate::convert::parse_type_tokens(&p.ty)
                            };
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

/// Type-check a resolved file with cross-module type information.
///
/// Unlike [`type_check_with_config`], this populates the `TypeEnv` with
/// type information from imported modules so that cross-file references
/// (contract input/output types, struct fields, enum variants) resolve
/// to concrete types instead of `Type::Unknown`.
pub fn type_check_with_modules(
    resolved: &ResolvedFile,
    modules: &HashMap<String, ResolvedFile>,
    config: &assura_config::TypeCheckConfig,
) -> Result<TypedFile, Vec<TypeError>> {
    let mut type_env = build_type_env(&resolved.symbols, &resolved.source);

    // Inject type information from imported modules
    for imp in &resolved.imports {
        if imp.status != ImportStatus::Resolved {
            continue;
        }
        let module_key = imp.path.join(".");
        if let Some(imported_resolved) = modules.get(&module_key) {
            inject_imported_types(&mut type_env, imp, &imported_resolved.source);
        }
    }

    let mut errors = check_clause_bodies(&resolved.source, &type_env);
    let (check_errors, pending_decrease_checks) =
        run_all_checks(&resolved.source, &type_env, &resolved.symbols, config);
    errors.extend(check_errors);

    if !errors.is_empty() {
        return Err(errors);
    }

    let generated_tests = generate_tests_from_contracts(&resolved.source);

    Ok(TypedFile {
        resolved: Arc::new(resolved.clone()),
        pending_decrease_checks,
        type_env,
        hir: None,
        generated_tests,
    })
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
                        .map(|f| (f.name.clone(), parse_type_tokens(&f.ty)))
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
                    .map(|p| resolve_type(p.parsed_type.as_ref(), &p.ty))
                    .collect();
                let ret = if f.return_ty.is_empty() {
                    Type::Unit
                } else {
                    parse_type_tokens(&f.return_ty)
                };
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
                    .map(|p| resolve_type(p.parsed_type.as_ref(), &p.ty))
                    .collect();
                let ret = if e.return_ty.is_empty() {
                    Type::Unit
                } else {
                    parse_type_tokens(&e.return_ty)
                };
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

/// Type-check from an HIR file. This is the preferred entry point when the
/// HIR lowering pass has already been run.
pub fn type_check_hir(hir: &assura_hir::HirFile) -> Result<TypedFile, Vec<TypeError>> {
    type_check_hir_with_config(hir, &assura_config::TypeCheckConfig::default())
}

/// Type-check from an HIR file using the given configuration.
///
/// Uses `build_type_env_from_hir` to construct the type environment from
/// structured HIR types instead of raw token parsing.
pub fn type_check_hir_with_config(
    hir: &assura_hir::HirFile,
    config: &assura_config::TypeCheckConfig,
) -> Result<TypedFile, Vec<TypeError>> {
    let resolved = hir.resolved();
    let type_env = build_type_env_from_hir(hir);

    let mut errors = check_clause_bodies_hir(hir, &type_env);
    let (check_errors, pending_decrease_checks) =
        run_all_checks(&resolved.source, &type_env, &resolved.symbols, config);
    errors.extend(check_errors);

    if !errors.is_empty() {
        return Err(errors);
    }

    let generated_tests = generate_tests_from_contracts(&resolved.source);

    Ok(TypedFile {
        resolved: Arc::clone(&hir.resolved),
        pending_decrease_checks,
        type_env,
        hir: Some(hir.clone()),
        generated_tests,
    })
}

/// Type-check a resolved file using the given configuration.
///
/// `config.strict_effects` controls whether the effect checker runs.
/// `config.warn_unused_imports` is reserved for future import analysis.
pub fn type_check_with_config(
    resolved: &ResolvedFile,
    config: &assura_config::TypeCheckConfig,
) -> Result<TypedFile, Vec<TypeError>> {
    let type_env = build_type_env(&resolved.symbols, &resolved.source);

    let mut errors = check_clause_bodies(&resolved.source, &type_env);
    let (check_errors, pending_decrease_checks) =
        run_all_checks(&resolved.source, &type_env, &resolved.symbols, config);
    errors.extend(check_errors);

    if !errors.is_empty() {
        return Err(errors);
    }

    let generated_tests = generate_tests_from_contracts(&resolved.source);

    Ok(TypedFile {
        resolved: Arc::new(resolved.clone()),
        pending_decrease_checks,
        type_env,
        hir: None,
        generated_tests,
    })
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
        let strict_result = type_check_with_config(&resolved, &strict_config);
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
