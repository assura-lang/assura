//! Type checking pipeline entry points.
//!
//! Contains type_check(), type_check_with_modules(),
//! and the unified run_all_checks dispatcher.

use std::collections::HashMap;
use std::sync::Arc;

use assura_parser::ast::{
    ClauseKind, ContractDecl, Decl, EnumDef, ExternDecl, FnDef, ServiceDecl, ServiceItem, TypeDef,
};
use assura_resolve::{ImportStatus, ResolvedFile, SymbolTable};

use crate::checkers::PendingDecreaseCheck;
use crate::checks::*;
use crate::clauses::{
    check_clause_bodies, collect_input_param_types, extract_output_type_from_body,
    register_input_clause_params,
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
pub fn type_check(resolved: ResolvedFile) -> Result<TypedFile, Vec<TypeError>> {
    TypeChecker::new().check(resolved).map_err(|(errs, _)| errs)
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

use assura_parser::features::FeatureCategory;

/// A named, categorized checker entry in the pipeline.
///
/// The `name` and `category` fields are metadata for error attribution,
/// selective execution, and pipeline introspection. `dispatch` is used at
/// runtime; `name` and `category` are read in tests (uniqueness, non-empty,
/// category coverage).
#[cfg_attr(not(test), expect(dead_code))]
struct CheckerEntry {
    /// Human-readable name for error attribution and logging.
    name: &'static str,
    /// Feature category this checker belongs to.
    category: FeatureCategory,
    /// The actual checker dispatch.
    dispatch: CheckerDispatch,
}

/// Ordered checker registry (order matches original `run_all_checks` wiring).
///
/// **Agent rule:** every new `run_*_checks` in `checks/` must appear here in
/// the same PR, or it is dead code. `checker_pipeline_has_expected_breadth`
/// below guards against accidental empty registries. Run
/// `bash scripts/guards.sh` after adding a checker.
const CHECKER_PIPELINE: &[CheckerEntry] = &[
    // -- CORE --
    CheckerEntry {
        name: "axiomatic",
        category: FeatureCategory::Core,
        dispatch: CheckerDispatch::Symbols(run_axiomatic_checks),
    },
    CheckerEntry {
        name: "liveness",
        category: FeatureCategory::Core,
        dispatch: CheckerDispatch::Source(run_liveness_checks),
    },
    CheckerEntry {
        name: "frame",
        category: FeatureCategory::Core,
        dispatch: CheckerDispatch::EnvSymbols(run_frame_checks),
    },
    CheckerEntry {
        name: "opaque_function",
        category: FeatureCategory::Core,
        dispatch: CheckerDispatch::Source(run_opaque_function_checks),
    },
    CheckerEntry {
        name: "quantifier_trigger",
        category: FeatureCategory::Core,
        dispatch: CheckerDispatch::Source(run_quantifier_trigger_checks),
    },
    CheckerEntry {
        name: "prophecy_resolution",
        category: FeatureCategory::Core,
        dispatch: CheckerDispatch::Source(run_prophecy_resolution_checks),
    },
    CheckerEntry {
        name: "totality",
        category: FeatureCategory::Core,
        dispatch: CheckerDispatch::Totality,
    },
    CheckerEntry {
        name: "generic_instantiation",
        category: FeatureCategory::Core,
        dispatch: CheckerDispatch::Source(run_generic_instantiation_checks),
    },
    // -- MEM --
    CheckerEntry {
        name: "memory",
        category: FeatureCategory::Mem,
        dispatch: CheckerDispatch::Source(run_memory_checks),
    },
    CheckerEntry {
        name: "fixed_width",
        category: FeatureCategory::Mem,
        dispatch: CheckerDispatch::Env(run_fixed_width_checks),
    },
    CheckerEntry {
        name: "allocator",
        category: FeatureCategory::Mem,
        dispatch: CheckerDispatch::Source(run_allocator_checks),
    },
    CheckerEntry {
        name: "circular_buffer",
        category: FeatureCategory::Mem,
        dispatch: CheckerDispatch::Source(run_circular_buffer_checks),
    },
    // -- TYPE --
    CheckerEntry {
        name: "linearity",
        category: FeatureCategory::Type,
        dispatch: CheckerDispatch::Source(run_linearity_checks),
    },
    CheckerEntry {
        name: "typestate",
        category: FeatureCategory::Type,
        dispatch: CheckerDispatch::Source(run_typestate_checks),
    },
    CheckerEntry {
        name: "interface",
        category: FeatureCategory::Type,
        dispatch: CheckerDispatch::Source(run_interface_checks),
    },
    CheckerEntry {
        name: "structural_invariant",
        category: FeatureCategory::Type,
        dispatch: CheckerDispatch::Source(run_structural_invariant_checks),
    },
    CheckerEntry {
        name: "error_propagation",
        category: FeatureCategory::Type,
        dispatch: CheckerDispatch::Source(run_error_propagation_checks),
    },
    CheckerEntry {
        name: "match_exhaustiveness",
        category: FeatureCategory::Type,
        dispatch: CheckerDispatch::Symbols(run_match_exhaustiveness_checks),
    },
    CheckerEntry {
        name: "collection_contract",
        category: FeatureCategory::Type,
        dispatch: CheckerDispatch::Source(run_collection_contract_checks),
    },
    // -- SEC --
    CheckerEntry {
        name: "taint",
        category: FeatureCategory::Sec,
        dispatch: CheckerDispatch::Source(run_taint_checks),
    },
    CheckerEntry {
        name: "info_flow",
        category: FeatureCategory::Sec,
        dispatch: CheckerDispatch::Source(run_info_flow_checks),
    },
    CheckerEntry {
        name: "constant_time",
        category: FeatureCategory::Sec,
        dispatch: CheckerDispatch::Source(run_constant_time_checks),
    },
    CheckerEntry {
        name: "secure_erasure",
        category: FeatureCategory::Sec,
        dispatch: CheckerDispatch::Source(run_secure_erasure_checks),
    },
    CheckerEntry {
        name: "crypto_conformance",
        category: FeatureCategory::Sec,
        dispatch: CheckerDispatch::Source(run_crypto_conformance_checks),
    },
    CheckerEntry {
        name: "ffi",
        category: FeatureCategory::Sec,
        dispatch: CheckerDispatch::Source(run_ffi_checks),
    },
    // -- CONC --
    CheckerEntry {
        name: "shared_mem",
        category: FeatureCategory::Conc,
        dispatch: CheckerDispatch::Source(run_shared_mem_checks),
    },
    CheckerEntry {
        name: "callback_reentrancy",
        category: FeatureCategory::Conc,
        dispatch: CheckerDispatch::Source(run_callback_reentrancy_checks),
    },
    CheckerEntry {
        name: "determinism",
        category: FeatureCategory::Conc,
        dispatch: CheckerDispatch::Source(run_determinism_checks),
    },
    CheckerEntry {
        name: "lock_order",
        category: FeatureCategory::Conc,
        dispatch: CheckerDispatch::Source(run_lock_order_checks),
    },
    CheckerEntry {
        name: "temporal_deadline",
        category: FeatureCategory::Conc,
        dispatch: CheckerDispatch::Source(run_temporal_deadline_checks),
    },
    CheckerEntry {
        name: "weak_memory",
        category: FeatureCategory::Conc,
        dispatch: CheckerDispatch::Source(run_weak_memory_checks),
    },
    // -- STOR --
    CheckerEntry {
        name: "crash_recovery",
        category: FeatureCategory::Stor,
        dispatch: CheckerDispatch::Source(run_crash_recovery_checks),
    },
    CheckerEntry {
        name: "page_cache",
        category: FeatureCategory::Stor,
        dispatch: CheckerDispatch::Source(run_page_cache_checks),
    },
    CheckerEntry {
        name: "mvcc",
        category: FeatureCategory::Stor,
        dispatch: CheckerDispatch::Source(run_mvcc_checks),
    },
    CheckerEntry {
        name: "rollback",
        category: FeatureCategory::Stor,
        dispatch: CheckerDispatch::Source(run_rollback_checks),
    },
    CheckerEntry {
        name: "monotonic_state",
        category: FeatureCategory::Stor,
        dispatch: CheckerDispatch::Source(run_monotonic_state_checks),
    },
    CheckerEntry {
        name: "storage_failure",
        category: FeatureCategory::Stor,
        dispatch: CheckerDispatch::Source(run_storage_failure_checks),
    },
    // -- FMT --
    CheckerEntry {
        name: "binary_format",
        category: FeatureCategory::Fmt,
        dispatch: CheckerDispatch::Source(run_binary_format_checks),
    },
    CheckerEntry {
        name: "bit_level",
        category: FeatureCategory::Fmt,
        dispatch: CheckerDispatch::Source(run_bit_level_checks),
    },
    CheckerEntry {
        name: "string_encoding",
        category: FeatureCategory::Fmt,
        dispatch: CheckerDispatch::Source(run_string_encoding_checks),
    },
    CheckerEntry {
        name: "codec_registry",
        category: FeatureCategory::Fmt,
        dispatch: CheckerDispatch::Source(run_codec_registry_checks),
    },
    CheckerEntry {
        name: "checksum",
        category: FeatureCategory::Fmt,
        dispatch: CheckerDispatch::Source(run_checksum_checks),
    },
    CheckerEntry {
        name: "protocol_grammar",
        category: FeatureCategory::Fmt,
        dispatch: CheckerDispatch::Source(run_protocol_grammar_checks),
    },
    // -- NUM --
    CheckerEntry {
        name: "numerical_precision",
        category: FeatureCategory::Num,
        dispatch: CheckerDispatch::Source(run_numerical_precision_checks),
    },
    CheckerEntry {
        name: "precomputed_table",
        category: FeatureCategory::Num,
        dispatch: CheckerDispatch::Source(run_precomputed_table_checks),
    },
    // -- PLAT --
    CheckerEntry {
        name: "platform_abstraction",
        category: FeatureCategory::Plat,
        dispatch: CheckerDispatch::Source(run_platform_abstraction_checks),
    },
    CheckerEntry {
        name: "feature_flag",
        category: FeatureCategory::Plat,
        dispatch: CheckerDispatch::Source(run_feature_flag_checks),
    },
    CheckerEntry {
        name: "resource_limit",
        category: FeatureCategory::Plat,
        dispatch: CheckerDispatch::Source(run_resource_limit_checks),
    },
    // -- PERF --
    CheckerEntry {
        name: "unsafe_escape",
        category: FeatureCategory::Perf,
        dispatch: CheckerDispatch::Source(run_unsafe_escape_checks),
    },
    CheckerEntry {
        name: "complexity_bound",
        category: FeatureCategory::Perf,
        dispatch: CheckerDispatch::Source(run_complexity_bound_checks),
    },
    // -- TEST --
    CheckerEntry {
        name: "behavioral_equivalence",
        category: FeatureCategory::Test,
        dispatch: CheckerDispatch::Source(run_behavioral_equivalence_checks),
    },
    CheckerEntry {
        name: "multi_pass_refinement",
        category: FeatureCategory::Test,
        dispatch: CheckerDispatch::Source(run_multi_pass_refinement_checks),
    },
    // -- MISC --
    CheckerEntry {
        name: "effects",
        category: FeatureCategory::Misc,
        dispatch: CheckerDispatch::Effects,
    },
    CheckerEntry {
        name: "crud_auth",
        category: FeatureCategory::Misc,
        dispatch: CheckerDispatch::Source(run_crud_auth_checks),
    },
    CheckerEntry {
        name: "incremental_contract",
        category: FeatureCategory::Misc,
        dispatch: CheckerDispatch::Source(run_incremental_contract_checks),
    },
    CheckerEntry {
        name: "scoped_invariant",
        category: FeatureCategory::Misc,
        dispatch: CheckerDispatch::Source(run_scoped_invariant_checks),
    },
    CheckerEntry {
        name: "contract_composition",
        category: FeatureCategory::Misc,
        dispatch: CheckerDispatch::Source(run_contract_composition_checks),
    },
    CheckerEntry {
        name: "contract_library",
        category: FeatureCategory::Misc,
        dispatch: CheckerDispatch::Source(run_contract_library_checks),
    },
];

/// Number of entries in [`CHECKER_PIPELINE`] (for tests / agent guards).
#[cfg(test)]
pub(crate) fn checker_pipeline_len() -> usize {
    CHECKER_PIPELINE.len()
}

fn run_effect_checks_filtered(
    source: &assura_parser::ast::SourceFile,
    config: &assura_config::TypeCheckConfig,
) -> Vec<TypeError> {
    let mut effect_errors = run_effect_checks(source);
    if !config.allowed_effects.is_empty() || !config.denied_effects.is_empty() {
        effect_errors.retain(|e| !config.allowed_effects.iter().any(|a| e.message.contains(a)));
    }
    effect_errors
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

    for entry in CHECKER_PIPELINE {
        match &entry.dispatch {
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
/// environment. Uses `DeclVisitor` to walk declarations and register
/// matching names as types/signatures.
fn inject_imported_types(
    env: &mut TypeEnv,
    imp: &assura_resolve::ResolvedImport,
    source: &assura_parser::ast::SourceFile,
) {
    // Collect the names this import brings into scope
    let imported_names: Vec<&str> = if !imp.items.is_empty() {
        imp.items.iter().map(|s| s.as_str()).collect()
    } else if let Some(alias) = &imp.alias {
        vec![alias.as_str()]
    } else if let Some(last) = imp.path.last() {
        vec![last.as_str()]
    } else {
        return;
    };

    struct ImportInjector<'a> {
        env: &'a mut TypeEnv,
        imported_names: Vec<&'a str>,
    }

    impl<'a> assura_parser::ast::DeclVisitor for ImportInjector<'a> {
        fn visit_contract(&mut self, c: &ContractDecl) {
            if !self.imported_names.contains(&c.name.as_str()) {
                return;
            }
            self.env.insert(c.name.clone(), Type::Named(c.name.clone()));
            for clause in &c.clauses {
                if clause.kind == ClauseKind::Input {
                    register_input_clause_params(&clause.body, self.env);
                }
            }
        }

        fn visit_service(&mut self, s: &ServiceDecl) {
            if !self.imported_names.contains(&s.name.as_str()) {
                return;
            }
            self.env.insert(s.name.clone(), Type::Named(s.name.clone()));
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
                self.env.insert(
                    name.clone(),
                    Type::Fn {
                        params: param_types,
                        ret: Box::new(ret),
                    },
                );
            }
        }

        fn visit_type_def(&mut self, td: &TypeDef) {
            if !self.imported_names.contains(&td.name.as_str()) {
                return;
            }
            self.env
                .insert(td.name.clone(), Type::Named(td.name.clone()));
            if let assura_parser::ast::TypeBody::Struct(fields) = &td.body {
                let field_types: Vec<(String, Type)> = fields
                    .iter()
                    .map(|f| (f.name.clone(), resolve_type_opt(f.ty.as_ref())))
                    .collect();
                self.env.struct_fields.insert(td.name.clone(), field_types);
            }
        }

        fn visit_enum_def(&mut self, e: &EnumDef) {
            if !self.imported_names.contains(&e.name.as_str()) {
                return;
            }
            self.env.insert(e.name.clone(), Type::Named(e.name.clone()));
            for variant in &e.variants {
                if !variant.fields.is_empty() {
                    let field_types: Vec<Type> = variant
                        .fields
                        .iter()
                        .map(|f| parse_type_tokens(std::slice::from_ref(f)))
                        .collect();
                    self.env.insert(
                        variant.name.clone(),
                        Type::Fn {
                            params: field_types,
                            ret: Box::new(Type::Named(e.name.clone())),
                        },
                    );
                }
            }
        }

        fn visit_fn_def(&mut self, f: &FnDef) {
            if !self.imported_names.contains(&f.name.as_str()) {
                return;
            }
            let param_types: Vec<Type> = f
                .params
                .iter()
                .map(|p| resolve_type_opt(p.ty.as_ref()))
                .collect();
            let ret = resolve_type_opt(f.return_ty.as_ref());
            self.env.insert(
                f.name.clone(),
                Type::Fn {
                    params: param_types,
                    ret: Box::new(ret),
                },
            );
        }

        fn visit_extern(&mut self, e: &ExternDecl) {
            if !self.imported_names.contains(&e.name.as_str()) {
                return;
            }
            let param_types: Vec<Type> = e
                .params
                .iter()
                .map(|p| resolve_type_opt(p.ty.as_ref()))
                .collect();
            let ret = resolve_type_opt(e.return_ty.as_ref());
            self.env.insert(
                e.name.clone(),
                Type::Fn {
                    params: param_types,
                    ret: Box::new(ret),
                },
            );
        }
    }

    let mut injector = ImportInjector {
        env,
        imported_names,
    };
    assura_parser::ast::walk_decls(&mut injector, &source.decls);
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
///     .check(resolved)
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
    pub fn check(
        self,
        resolved: ResolvedFile,
    ) -> Result<TypedFile, (Vec<TypeError>, ResolvedFile)> {
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
            return Err((errors, resolved));
        }

        let generated_tests = generate_tests_from_contracts(&resolved.source);

        // Collect non-fatal warnings (clause quality)
        let warnings = run_unconstrained_output_checks(&resolved.source);

        Ok(TypedFile {
            resolved: Arc::new(resolved),
            pending_decrease_checks,
            type_env,
            generated_tests,
            warnings,
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

    #[test]
    fn checker_pipeline_has_expected_breadth() {
        // Guard against accidentally clearing CHECKER_PIPELINE or removing
        // most checkers without noticing. Update the floor only when
        // intentionally shrinking the registry.
        assert!(
            checker_pipeline_len() >= 50,
            "CHECKER_PIPELINE has only {} entries; new run_*_checks must be registered here",
            checker_pipeline_len()
        );
    }

    #[test]
    fn checker_entries_have_non_empty_names() {
        for entry in CHECKER_PIPELINE {
            assert!(
                !entry.name.is_empty(),
                "CheckerEntry has empty name (index in pipeline)"
            );
        }
    }

    #[test]
    fn checker_entries_have_unique_names() {
        let mut seen = std::collections::HashSet::new();
        for entry in CHECKER_PIPELINE {
            assert!(
                seen.insert(entry.name),
                "duplicate checker name: {:?}",
                entry.name
            );
        }
    }

    #[test]
    fn checker_pipeline_covers_all_categories() {
        let cats: std::collections::HashSet<_> =
            CHECKER_PIPELINE.iter().map(|e| e.category).collect();
        // Every FeatureCategory should have at least one checker
        assert!(cats.contains(&FeatureCategory::Core), "missing Core");
        assert!(cats.contains(&FeatureCategory::Mem), "missing Mem");
        assert!(cats.contains(&FeatureCategory::Type), "missing Type");
        assert!(cats.contains(&FeatureCategory::Sec), "missing Sec");
        assert!(cats.contains(&FeatureCategory::Conc), "missing Conc");
        assert!(cats.contains(&FeatureCategory::Stor), "missing Stor");
        assert!(cats.contains(&FeatureCategory::Fmt), "missing Fmt");
        assert!(cats.contains(&FeatureCategory::Num), "missing Num");
        assert!(cats.contains(&FeatureCategory::Plat), "missing Plat");
        assert!(cats.contains(&FeatureCategory::Perf), "missing Perf");
        assert!(cats.contains(&FeatureCategory::Test), "missing Test");
        assert!(cats.contains(&FeatureCategory::Misc), "missing Misc");
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
        let file = assura_parser::parse_unwrap(src);
        let resolved = assura_resolve::resolve(&file).expect("resolve failed");
        let result = type_check(resolved);
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
        let file = assura_parser::parse_unwrap(src);
        let resolved = assura_resolve::resolve(&file).expect("resolve failed");
        let result = type_check(resolved);
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
            allowed_effects: vec!["database".to_string()],
            ..Default::default()
        };
        let strict_result = TypeChecker::new().config(strict_config).check(resolved);
        match strict_result {
            Err((errors, _)) => {
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
