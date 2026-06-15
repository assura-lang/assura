//! Project configuration and output mode types for the Assura compiler.
//!
//! Provides `ProjectConfig` (parsed from `assura.toml`), `OutputMode`,
//! and `Verbosity` types used across the CLI and library crates.

use std::fs;
use std::path::Path;

/// Parsed `assura.toml` project configuration.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    pub package: PackageConfig,
    pub build: BuildConfig,
    pub verify: VerifyConfig,
    pub profile: ProfileConfig,
    pub effects: EffectsConfig,
    pub codegen: CodegenTomlConfig,
    pub contracts: ContractsConfig,
    pub inline: InlineConfig,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct PackageConfig {
    pub name: String,
    pub version: String,
}

impl Default for PackageConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            version: "0.1.0".to_string(),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct BuildConfig {
    pub target: String,
    pub output: String,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            target: "native".to_string(),
            output: "generated".to_string(),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct VerifyConfig {
    pub smt_solver: String,
    pub layer: u8,
    pub timeout: u64,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            smt_solver: "z3".to_string(),
            layer: 1,
            timeout: 1000,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ProfileConfig {
    #[serde(rename = "type")]
    pub profile_type: String,
}

impl Default for ProfileConfig {
    fn default() -> Self {
        Self {
            profile_type: "minimal".to_string(),
        }
    }
}

/// Effect system configuration from `[effects]` in assura.toml.
///
/// Controls which effects are allowed or denied project-wide.
/// Per spec Section 10.3, this allows projects to restrict the effect
/// vocabulary (e.g., deny `io` in a pure-math library).
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct EffectsConfig {
    /// Effects explicitly allowed. If non-empty, only these effects can appear.
    pub allowed: Vec<String>,
    /// Effects explicitly denied. Contracts using these effects produce errors.
    pub denied: Vec<String>,
    /// Default effect for contracts without an explicit `effects` clause.
    /// Defaults to "pure".
    pub default_effect: String,
}

impl EffectsConfig {
    /// Returns true if a given effect name is permitted by this configuration.
    pub fn is_allowed(&self, effect: &str) -> bool {
        if self.denied.iter().any(|d| d == effect) {
            return false;
        }
        if self.allowed.is_empty() {
            return true;
        }
        self.allowed.iter().any(|a| a == effect)
    }
}

/// Code generation settings from `[codegen]` in assura.toml.
///
/// Separate from `[build]` to allow fine-grained control over generated code.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct CodegenTomlConfig {
    /// Backend: "rustc" (default) or "cranelift".
    pub backend: String,
    /// Whether to emit `debug_assert!` for requires clauses.
    pub emit_debug_asserts: bool,
    /// Whether to generate property-based tests from contracts.
    pub generate_tests: bool,
    /// Whether to run `cargo check` on the generated code.
    pub check_generated: bool,
}

impl Default for CodegenTomlConfig {
    fn default() -> Self {
        Self {
            backend: "rustc".to_string(),
            emit_debug_asserts: true,
            generate_tests: false,
            check_generated: true,
        }
    }
}

/// External `.assura` contract file configuration from `[contracts]`.
///
/// Controls where external contract files are stored and how they bind
/// to Rust functions (per spec #105 dual-source contracts).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ContractsConfig {
    /// Directory containing external `.assura` contract files.
    pub path: String,
    /// Crate name for bind path resolution.
    pub crate_name: String,
    /// Additional search paths for bind target resolution.
    pub search_paths: Vec<String>,
}

impl Default for ContractsConfig {
    fn default() -> Self {
        Self {
            path: "contracts".to_string(),
            crate_name: String::new(),
            search_paths: vec!["src".to_string()],
        }
    }
}

/// Inline contract annotation configuration from `[inline]`.
///
/// Controls how inline doc comment annotations are processed
/// (per spec #101 inline contract annotations).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct InlineConfig {
    /// Whether inline annotations are enabled.
    pub enabled: bool,
    /// Source paths to scan for inline annotations.
    pub source_paths: Vec<String>,
    /// Merge strategy when both external and inline exist: "merge" or "external-only".
    pub merge_strategy: String,
}

impl Default for InlineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            source_paths: vec!["src".to_string()],
            merge_strategy: "merge".to_string(),
        }
    }
}

/// Output mode for the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Human,
    Json,
}

// ---------------------------------------------------------------------------
// Compiler pipeline configuration
// ---------------------------------------------------------------------------

/// Full compiler configuration, assembled from CLI args + assura.toml.
///
/// This is the single struct threaded through the compilation pipeline.
/// Each pass extracts the subset it needs via accessor methods.
#[derive(Debug, Clone)]
pub struct CompilerConfig {
    /// Output mode (human-readable or JSON).
    pub output_mode: OutputMode,
    /// Verbosity level.
    pub verbosity: Verbosity,
    /// Type-checking options.
    pub type_check: TypeCheckConfig,
    /// SMT verification options.
    pub verify: VerifyOptions,
    /// Code generation options.
    pub codegen: CodegenConfig,
}

impl Default for CompilerConfig {
    fn default() -> Self {
        Self {
            output_mode: OutputMode::Human,
            verbosity: Verbosity::Normal,
            type_check: TypeCheckConfig::default(),
            verify: VerifyOptions::default(),
            codegen: CodegenConfig::default(),
        }
    }
}

impl CompilerConfig {
    /// Create a CompilerConfig from CLI args and project config.
    pub fn from_project(
        project: &ProjectConfig,
        output_mode: OutputMode,
        verbosity: Verbosity,
    ) -> Self {
        Self {
            output_mode,
            verbosity,
            type_check: TypeCheckConfig {
                strict_effects: true,
                warn_unused_imports: true,
                allowed_effects: project.effects.allowed.clone(),
                denied_effects: project.effects.denied.clone(),
                default_effect: if project.effects.default_effect.is_empty() {
                    "pure".to_string()
                } else {
                    project.effects.default_effect.clone()
                },
            },
            verify: VerifyOptions {
                layer: project.verify.layer,
                timeout_ms: project.verify.timeout,
                ..Default::default()
            },
            codegen: CodegenConfig {
                output_dir: project.build.output.clone(),
                target: project.build.target.clone(),
                run_cargo_check: project.codegen.check_generated,
                emit_debug_asserts: project.codegen.emit_debug_asserts,
                generate_tests: project.codegen.generate_tests,
                backend: project.codegen.backend.clone(),
            },
        }
    }
}

/// Type-checking pass configuration.
#[derive(Debug, Clone)]
pub struct TypeCheckConfig {
    /// Whether to emit warnings for unused imports.
    pub warn_unused_imports: bool,
    /// Whether to perform strict mode checking (reject unknown effects).
    pub strict_effects: bool,
    /// Effects allowed by the project config. Empty means all are allowed.
    pub allowed_effects: Vec<String>,
    /// Effects denied by the project config.
    pub denied_effects: Vec<String>,
    /// Default effect for contracts without an explicit `effects` clause.
    pub default_effect: String,
}

impl Default for TypeCheckConfig {
    fn default() -> Self {
        Self {
            warn_unused_imports: true,
            strict_effects: true,
            allowed_effects: Vec::new(),
            denied_effects: Vec::new(),
            default_effect: "pure".to_string(),
        }
    }
}

/// SMT verification options.
#[derive(Debug, Clone)]
pub struct VerifyOptions {
    /// Verification layer (0 = structural only, 1 = SMT).
    pub layer: u8,
    /// SMT solver timeout in milliseconds.
    pub timeout_ms: u64,
    /// Solver choice name (e.g., "z3", "cvc5", "portfolio").
    pub solver: String,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            layer: 1,
            timeout_ms: 1000,
            solver: "z3".to_string(),
        }
    }
}

/// Code generation options.
#[derive(Debug, Clone)]
pub struct CodegenConfig {
    /// Output directory for generated Rust code.
    pub output_dir: String,
    /// Compilation target ("native" or "wasm").
    pub target: String,
    /// Whether to run `cargo check` on generated code.
    pub run_cargo_check: bool,
    /// Whether to emit `debug_assert!` for requires clauses.
    pub emit_debug_asserts: bool,
    /// Whether to generate property-based tests from contracts.
    pub generate_tests: bool,
    /// Backend name ("rustc" or "cranelift").
    pub backend: String,
}

impl Default for CodegenConfig {
    fn default() -> Self {
        Self {
            output_dir: "generated".to_string(),
            target: "native".to_string(),
            run_cargo_check: true,
            emit_debug_asserts: true,
            generate_tests: false,
            backend: "rustc".to_string(),
        }
    }
}

/// Verbosity level for the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
    Quiet,
    Normal,
    Verbose,
}

/// Load `assura.toml` from the project root, if it exists.
///
/// Walks up from `start_path` to find the project root (directory
/// containing `assura.toml`), then parses the config file.
/// Returns `None` if no `assura.toml` is found (single-file mode).
///
/// The `find_root` callback locates the project root directory.
pub fn load_project_config(
    start_path: &Path,
    find_root: fn(&Path) -> Option<std::path::PathBuf>,
) -> Option<(ProjectConfig, std::path::PathBuf)> {
    let project_root = find_root(start_path)?;
    let config_path = project_root.join("assura.toml");
    let content = fs::read_to_string(&config_path).ok()?;

    // Support both [package] and legacy [project] section names.
    let parse_content = if content.contains("[project]") && !content.contains("[package]") {
        content.replace("[project]", "[package]")
    } else {
        content
    };

    match toml::from_str::<ProjectConfig>(&parse_content) {
        Ok(config) => Some((config, project_root)),
        Err(e) => {
            eprintln!("warning: failed to parse {}: {e}", config_path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = ProjectConfig::default();
        assert_eq!(config.build.target, "native");
        assert_eq!(config.build.output, "generated");
        assert_eq!(config.verify.smt_solver, "z3");
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
        assert_eq!(config.verify.solver, "z3");
        assert_eq!(config.codegen.output_dir, "generated");
        assert_eq!(config.codegen.target, "native");
        assert!(config.codegen.run_cargo_check);
        assert!(config.type_check.warn_unused_imports);
        assert!(config.type_check.strict_effects);
    }

    #[test]
    fn compiler_config_from_project() {
        let project = ProjectConfig {
            verify: VerifyConfig {
                layer: 0,
                timeout: 5000,
                smt_solver: "cvc5".to_string(),
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
        assert_eq!(config.verify.smt_solver, "cvc5");
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
        assert_eq!(config.verify.smt_solver, "z3");
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
        assert_eq!(config.verify.smt_solver, "portfolio");
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
        assert_eq!(config.verify.smt_solver, "z3");
    }

    #[test]
    fn parse_empty_string_returns_default() {
        let config: ProjectConfig = toml::from_str("").unwrap();
        assert_eq!(config.package.name, "");
        assert_eq!(config.package.version, "0.1.0");
        assert_eq!(config.build.target, "native");
        assert_eq!(config.build.output, "generated");
        assert_eq!(config.verify.smt_solver, "z3");
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
        assert_eq!(config.verify.smt_solver, "z3");
        assert_eq!(config.verify.layer, 1);
        assert_eq!(config.verify.timeout, 1000);
        assert_eq!(config.profile.profile_type, "minimal");
    }

    #[test]
    fn parse_legacy_project_section() {
        let legacy_toml = r#"
[project]
name = "legacy-app"
version = "0.5.0"
"#;
        let parse_content =
            if legacy_toml.contains("[project]") && !legacy_toml.contains("[package]") {
                legacy_toml.replace("[project]", "[package]")
            } else {
                legacy_toml.to_string()
            };
        let config: ProjectConfig = toml::from_str(&parse_content).unwrap();
        assert_eq!(config.package.name, "legacy-app");
        assert_eq!(config.package.version, "0.5.0");
    }

    #[test]
    fn verify_smt_solver_accepts_z3() {
        let toml_str = "[verify]\nsmt-solver = \"z3\"\n";
        let config: ProjectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.verify.smt_solver, "z3");
    }

    #[test]
    fn verify_smt_solver_accepts_cvc5() {
        let toml_str = "[verify]\nsmt-solver = \"cvc5\"\n";
        let config: ProjectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.verify.smt_solver, "cvc5");
    }

    #[test]
    fn verify_smt_solver_accepts_portfolio() {
        let toml_str = "[verify]\nsmt-solver = \"portfolio\"\n";
        let config: ProjectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.verify.smt_solver, "portfolio");
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
        assert!(config.strict_effects);
    }

    #[test]
    fn verify_options_defaults() {
        let config = VerifyOptions::default();
        assert_eq!(config.layer, 1);
        assert_eq!(config.timeout_ms, 1000);
        assert_eq!(config.solver, "z3");
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
        assert_eq!(config.verify.smt_solver, "portfolio");
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
}
