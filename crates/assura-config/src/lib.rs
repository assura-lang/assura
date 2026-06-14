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
            type_check: TypeCheckConfig::default(),
            verify: VerifyOptions {
                layer: project.verify.layer,
                timeout_ms: project.verify.timeout,
                ..Default::default()
            },
            codegen: CodegenConfig {
                output_dir: project.build.output.clone(),
                target: project.build.target.clone(),
                ..Default::default()
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
}

impl Default for TypeCheckConfig {
    fn default() -> Self {
        Self {
            warn_unused_imports: true,
            strict_effects: true,
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
}

impl Default for CodegenConfig {
    fn default() -> Self {
        Self {
            output_dir: "generated".to_string(),
            target: "native".to_string(),
            run_cargo_check: true,
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

/// Parse verbosity from CLI arguments.
pub fn parse_verbosity(args: &[String]) -> Verbosity {
    if args.contains(&"--verbose".to_string()) || args.contains(&"-v".to_string()) {
        Verbosity::Verbose
    } else if args.contains(&"--quiet".to_string()) || args.contains(&"-q".to_string()) {
        Verbosity::Quiet
    } else {
        Verbosity::Normal
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
    fn parse_verbosity_flags() {
        let args: Vec<String> = vec!["--verbose".into()];
        assert_eq!(parse_verbosity(&args), Verbosity::Verbose);

        let args: Vec<String> = vec!["-q".into()];
        assert_eq!(parse_verbosity(&args), Verbosity::Quiet);

        let args: Vec<String> = vec!["check".into()];
        assert_eq!(parse_verbosity(&args), Verbosity::Normal);
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
    fn parse_verbosity_short_verbose_flag() {
        let args: Vec<String> = vec!["-v".into()];
        assert_eq!(parse_verbosity(&args), Verbosity::Verbose);
    }

    #[test]
    fn parse_verbosity_quiet_long_flag() {
        let args: Vec<String> = vec!["--quiet".into()];
        assert_eq!(parse_verbosity(&args), Verbosity::Quiet);
    }

    #[test]
    fn parse_verbosity_empty_args() {
        let args: Vec<String> = vec![];
        assert_eq!(parse_verbosity(&args), Verbosity::Normal);
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
}
