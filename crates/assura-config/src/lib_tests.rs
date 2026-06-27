use crate::*;

#[test]
fn default_config() {
    let config = ProjectConfig::default();
    assert_eq!(config.build.target, "native");
    assert_eq!(config.build.output, "generated");
    assert_eq!(config.verify.smt_solver, SolverChoice::Z3);
    assert_eq!(config.verify.layer, 1);
    assert_eq!(config.verify.timeout, 1000);
    assert_eq!(config.profile.profile_type, "minimal");
}

#[test]
fn compiler_config_defaults() {
    let config = CompilerConfig::default();
    assert_eq!(config.output_mode, OutputMode::Human);
    assert_eq!(config.verbosity, Verbosity::Normal);
    assert_eq!(config.verify.layer, 1);
    assert_eq!(config.verify.timeout_ms, 1000);
    assert_eq!(config.verify.solver, SolverChoice::Z3);
    assert_eq!(config.codegen.output_dir, "generated");
    assert_eq!(config.codegen.target, "native");
    assert!(config.codegen.run_cargo_check);
    assert!(config.type_check.warn_unused_imports);
}

#[test]
fn compiler_config_from_project() {
    let project = ProjectConfig {
        verify: VerifyConfig {
            layer: 0,
            timeout: 5000,
            smt_solver: SolverChoice::Cvc5,
            string_theory: false,
        },
        build: BuildConfig {
            output: "out".to_string(),
            target: "wasm".to_string(),
        },
        ..Default::default()
    };
    let config = CompilerConfig::from_project(&project, OutputMode::Json, Verbosity::Verbose);
    assert_eq!(config.output_mode, OutputMode::Json);
    assert_eq!(config.verbosity, Verbosity::Verbose);
    assert_eq!(config.verify.layer, 0);
    assert_eq!(config.verify.timeout_ms, 5000);
    assert_eq!(config.codegen.output_dir, "out");
    assert_eq!(config.codegen.target, "wasm");
}

#[test]
fn parse_config_all_fields() {
    let toml_str = r#"
[package]
name = "my-project"
version = "1.2.3"

[build]
target = "wasm"
output = "dist"

[verify]
smt-solver = "cvc5"
layer = 2
timeout = 5000

[profile]
type = "strict"
"#;
    let config: ProjectConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.package.name, "my-project");
    assert_eq!(config.package.version, "1.2.3");
    assert_eq!(config.build.target, "wasm");
    assert_eq!(config.build.output, "dist");
    assert_eq!(config.verify.smt_solver, SolverChoice::Cvc5);
    assert_eq!(config.verify.layer, 2);
    assert_eq!(config.verify.timeout, 5000);
    assert_eq!(config.profile.profile_type, "strict");
}

#[test]
fn parse_config_only_package() {
    let toml_str = r#"
[package]
name = "pkg-only"
version = "0.2.0"
"#;
    let config: ProjectConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.package.name, "pkg-only");
    assert_eq!(config.package.version, "0.2.0");
    assert_eq!(config.build.target, "native");
    assert_eq!(config.verify.smt_solver, SolverChoice::Z3);
}

#[test]
fn parse_config_only_verify() {
    let toml_str = r#"
[verify]
smt-solver = "portfolio"
layer = 3
timeout = 10000
"#;
    let config: ProjectConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.verify.smt_solver, SolverChoice::Portfolio);
    assert_eq!(config.verify.layer, 3);
    assert_eq!(config.verify.timeout, 10000);
    assert_eq!(config.package.name, "");
    assert_eq!(config.build.target, "native");
}

#[test]
fn parse_config_only_build() {
    let toml_str = r#"
[build]
target = "wasm"
output = "out/gen"
"#;
    let config: ProjectConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.build.target, "wasm");
    assert_eq!(config.build.output, "out/gen");
    assert_eq!(config.verify.smt_solver, SolverChoice::Z3);
}

#[test]
fn parse_empty_string_returns_default() {
    let config: ProjectConfig = toml::from_str("").unwrap();
    assert_eq!(config.package.name, "");
    assert_eq!(config.package.version, "0.1.0");
    assert_eq!(config.build.target, "native");
    assert_eq!(config.build.output, "generated");
    assert_eq!(config.verify.smt_solver, SolverChoice::Z3);
    assert_eq!(config.verify.layer, 1);
    assert_eq!(config.verify.timeout, 1000);
    assert_eq!(config.profile.profile_type, "minimal");
}

#[test]
fn parse_malformed_toml_errors() {
    let bad = "this is not [valid toml {{{";
    let result = toml::from_str::<ProjectConfig>(bad);
    assert!(result.is_err());
}

#[test]
fn default_values_match_expected() {
    let config = ProjectConfig::default();
    assert_eq!(config.package.name, "");
    assert_eq!(config.package.version, "0.1.0");
    assert_eq!(config.build.target, "native");
    assert_eq!(config.build.output, "generated");
    assert_eq!(config.verify.smt_solver, SolverChoice::Z3);
    assert_eq!(config.verify.layer, 1);
    assert_eq!(config.verify.timeout, 1000);
    assert_eq!(config.profile.profile_type, "minimal");
}

#[test]
fn verify_smt_solver_accepts_z3() {
    let toml_str = "[verify]\nsmt-solver = \"z3\"\n";
    let config: ProjectConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.verify.smt_solver, SolverChoice::Z3);
}

#[test]
fn verify_smt_solver_accepts_cvc5() {
    let toml_str = "[verify]\nsmt-solver = \"cvc5\"\n";
    let config: ProjectConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.verify.smt_solver, SolverChoice::Cvc5);
}

#[test]
fn verify_smt_solver_accepts_portfolio() {
    let toml_str = "[verify]\nsmt-solver = \"portfolio\"\n";
    let config: ProjectConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.verify.smt_solver, SolverChoice::Portfolio);
}

#[test]
fn output_mode_equality() {
    assert_eq!(OutputMode::Human, OutputMode::Human);
    assert_eq!(OutputMode::Json, OutputMode::Json);
    assert_ne!(OutputMode::Human, OutputMode::Json);
}

#[test]
fn codegen_config_defaults() {
    let config = CodegenConfig::default();
    assert_eq!(config.output_dir, "generated");
    assert_eq!(config.target, "native");
    assert!(config.run_cargo_check);
}

#[test]
fn type_check_config_defaults() {
    let config = TypeCheckConfig::default();
    assert!(config.warn_unused_imports);
}

#[test]
fn verify_options_defaults() {
    let config = VerifyOptions::default();
    assert_eq!(config.layer, 1);
    assert_eq!(config.timeout_ms, 1000);
    assert_eq!(config.solver, SolverChoice::Z3);
    assert!(!config.string_theory);
    assert!(config.parallel);
    assert!(config.decrease_checks);
    assert!(
        !config.enable_cache,
        "disk verify cache off by default (IR/encoder footgun)"
    );
}

#[test]
fn verify_options_for_tests_disables_heavy_flags() {
    let opts = VerifyOptions::for_tests();
    assert!(!opts.parallel);
    assert!(!opts.decrease_checks);
    assert!(!opts.enable_cache);
    assert_eq!(opts.layer, 1);
    assert_eq!(opts.solver, SolverChoice::Z3);
}

#[test]
fn string_theory_config_default_false() {
    let config = VerifyConfig::default();
    assert!(!config.string_theory, "string_theory must default to false");
    let opts = VerifyOptions::default();
    assert!(
        !opts.string_theory,
        "VerifyOptions string_theory must default to false"
    );
}

#[test]
fn parse_string_theory_from_toml() {
    let toml_str = "[verify]\nstring-theory = true\n";
    let config: ProjectConfig = toml::from_str(toml_str).unwrap();
    assert!(config.verify.string_theory);
}

#[test]
fn parse_string_theory_false_from_toml() {
    let toml_str = "[verify]\nstring-theory = false\n";
    let config: ProjectConfig = toml::from_str(toml_str).unwrap();
    assert!(!config.verify.string_theory);
}

#[test]
fn compiler_config_threads_string_theory() {
    let project = ProjectConfig {
        verify: VerifyConfig {
            string_theory: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let config = CompilerConfig::from_project(&project, OutputMode::Human, Verbosity::Normal);
    assert!(config.verify.string_theory);
}

// -----------------------------------------------------------------------
// Effects config tests
// -----------------------------------------------------------------------

#[test]
fn effects_config_defaults() {
    let config = EffectsConfig::default();
    assert!(config.allowed.is_empty());
    assert!(config.denied.is_empty());
    assert_eq!(config.default_effect, "");
}

#[test]
fn effects_config_is_allowed_no_restrictions() {
    let config = EffectsConfig::default();
    assert!(config.is_allowed("io"));
    assert!(config.is_allowed("database"));
    assert!(config.is_allowed("anything"));
}

#[test]
fn effects_config_is_allowed_allowlist() {
    let config = EffectsConfig {
        allowed: vec!["pure".to_string(), "io".to_string()],
        denied: Vec::new(),
        default_effect: "pure".to_string(),
    };
    assert!(config.is_allowed("pure"));
    assert!(config.is_allowed("io"));
    assert!(!config.is_allowed("database"));
    assert!(!config.is_allowed("net"));
}

#[test]
fn effects_config_is_allowed_denylist() {
    let config = EffectsConfig {
        allowed: Vec::new(),
        denied: vec!["io".to_string(), "net".to_string()],
        default_effect: "pure".to_string(),
    };
    assert!(!config.is_allowed("io"));
    assert!(!config.is_allowed("net"));
    assert!(config.is_allowed("pure"));
    assert!(config.is_allowed("database"));
}

#[test]
fn effects_config_deny_overrides_allow() {
    let config = EffectsConfig {
        allowed: vec!["io".to_string(), "net".to_string()],
        denied: vec!["io".to_string()],
        default_effect: "pure".to_string(),
    };
    assert!(!config.is_allowed("io"), "denied should override allowed");
    assert!(config.is_allowed("net"));
}

#[test]
fn parse_effects_config_from_toml() {
    let toml_str = r#"
[effects]
allowed = ["pure", "io", "logging"]
denied = ["net"]
default-effect = "pure"
"#;
    let config: ProjectConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.effects.allowed, vec!["pure", "io", "logging"]);
    assert_eq!(config.effects.denied, vec!["net"]);
    assert_eq!(config.effects.default_effect, "pure");
}

// -----------------------------------------------------------------------
// Codegen toml config tests
// -----------------------------------------------------------------------

#[test]
fn codegen_toml_config_defaults() {
    let config = CodegenTomlConfig::default();
    assert_eq!(config.backend, "rustc");
    assert!(config.emit_debug_asserts);
    assert!(!config.generate_tests);
    assert!(config.check_generated);
}

#[test]
fn parse_codegen_section_from_toml() {
    let toml_str = r#"
[codegen]
backend = "cranelift"
emit-debug-asserts = false
generate-tests = true
check-generated = false
"#;
    let config: ProjectConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.codegen.backend, "cranelift");
    assert!(!config.codegen.emit_debug_asserts);
    assert!(config.codegen.generate_tests);
    assert!(!config.codegen.check_generated);
}

#[test]
fn parse_full_config_with_all_sections() {
    let toml_str = r#"
[package]
name = "full-project"
version = "2.0.0"

[build]
target = "wasm"
output = "out"

[verify]
smt-solver = "portfolio"
layer = 3
timeout = 30000

[profile]
type = "strict"

[effects]
allowed = ["pure", "io"]
denied = []
default-effect = "pure"

[codegen]
backend = "rustc"
emit-debug-asserts = true
generate-tests = true
check-generated = false

[contracts]
path = "specs"
crate-name = "my_crate"
search-paths = ["src", "lib"]

[inline]
enabled = false
source-paths = ["src", "tests"]
merge-strategy = "external-only"
"#;
    let config: ProjectConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.package.name, "full-project");
    assert_eq!(config.package.version, "2.0.0");
    assert_eq!(config.build.target, "wasm");
    assert_eq!(config.verify.smt_solver, SolverChoice::Portfolio);
    assert_eq!(config.verify.layer, 3);
    assert_eq!(config.profile.profile_type, "strict");
    assert_eq!(config.effects.allowed, vec!["pure", "io"]);
    assert_eq!(config.effects.default_effect, "pure");
    assert_eq!(config.codegen.backend, "rustc");
    assert!(config.codegen.generate_tests);
    assert_eq!(config.contracts.path, "specs");
    assert_eq!(config.contracts.crate_name, "my_crate");
    assert_eq!(config.contracts.search_paths, vec!["src", "lib"]);
    assert!(!config.inline.enabled);
    assert_eq!(config.inline.source_paths, vec!["src", "tests"]);
    assert_eq!(config.inline.merge_strategy, "external-only");
}

// -----------------------------------------------------------------------
// Contracts config tests
// -----------------------------------------------------------------------

#[test]
fn contracts_config_defaults() {
    let config = ContractsConfig::default();
    assert_eq!(config.path, "contracts");
    assert_eq!(config.crate_name, "");
    assert_eq!(config.search_paths, vec!["src"]);
}

#[test]
fn parse_contracts_section_from_toml() {
    let toml_str = r#"
[contracts]
path = "specs"
crate-name = "my_lib"
search-paths = ["src", "lib", "generated"]
"#;
    let config: ProjectConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.contracts.path, "specs");
    assert_eq!(config.contracts.crate_name, "my_lib");
    assert_eq!(
        config.contracts.search_paths,
        vec!["src", "lib", "generated"]
    );
}

#[test]
fn contracts_config_partial_override() {
    let toml_str = r#"
[contracts]
path = "my-contracts"
"#;
    let config: ProjectConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.contracts.path, "my-contracts");
    assert_eq!(config.contracts.crate_name, ""); // default
    assert_eq!(config.contracts.search_paths, vec!["src"]); // default
}

// -----------------------------------------------------------------------
// Inline config tests
// -----------------------------------------------------------------------

#[test]
fn inline_config_defaults() {
    let config = InlineConfig::default();
    assert!(config.enabled);
    assert_eq!(config.source_paths, vec!["src"]);
    assert_eq!(config.merge_strategy, "merge");
}

#[test]
fn parse_inline_section_from_toml() {
    let toml_str = r#"
[inline]
enabled = false
source-paths = ["src", "tests", "examples"]
merge-strategy = "external-only"
"#;
    let config: ProjectConfig = toml::from_str(toml_str).unwrap();
    assert!(!config.inline.enabled);
    assert_eq!(config.inline.source_paths, vec!["src", "tests", "examples"]);
    assert_eq!(config.inline.merge_strategy, "external-only");
}

#[test]
fn inline_config_partial_override() {
    let toml_str = r#"
[inline]
enabled = false
"#;
    let config: ProjectConfig = toml::from_str(toml_str).unwrap();
    assert!(!config.inline.enabled);
    assert_eq!(config.inline.source_paths, vec!["src"]); // default
    assert_eq!(config.inline.merge_strategy, "merge"); // default
}

#[test]
fn compiler_config_from_project_with_effects() {
    let project = ProjectConfig {
        effects: EffectsConfig {
            allowed: vec!["pure".to_string(), "io".to_string()],
            denied: vec!["net".to_string()],
            default_effect: "pure".to_string(),
        },
        codegen: CodegenTomlConfig {
            backend: "cranelift".to_string(),
            emit_debug_asserts: false,
            generate_tests: true,
            check_generated: false,
        },
        ..Default::default()
    };
    let config = CompilerConfig::from_project(&project, OutputMode::Human, Verbosity::Normal);
    assert_eq!(config.type_check.allowed_effects, vec!["pure", "io"]);
    assert_eq!(config.type_check.denied_effects, vec!["net"]);
    assert_eq!(config.type_check.default_effect, "pure");
    assert_eq!(config.codegen.backend, "cranelift");
    assert!(!config.codegen.emit_debug_asserts);
    assert!(config.codegen.generate_tests);
    assert!(!config.codegen.run_cargo_check);
}
