//! Project configuration and output mode types for the Assura compiler.
//!
//! Provides `ProjectConfig` (parsed from `assura.toml`), `OutputMode`,
//! and `Verbosity` types used across the CLI and library crates.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Which SMT solver backend to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SolverChoice {
    /// Z3 via the Rust crate (requires `z3-verify` feature).
    Z3,
    /// CVC5 via command-line binary (requires `cvc5` on PATH).
    Cvc5,
    /// Portfolio: try Z3 first, fall back to CVC5 on timeout/unknown.
    Portfolio,
}

impl SolverChoice {
    /// Parse from a string (case-insensitive).
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "z3" => Some(Self::Z3),
            "cvc5" => Some(Self::Cvc5),
            "portfolio" => Some(Self::Portfolio),
            _ => None,
        }
    }

    /// Return the solver name as a string slice.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Z3 => "z3",
            Self::Cvc5 => "cvc5",
            Self::Portfolio => "portfolio",
        }
    }
}

impl std::fmt::Display for SolverChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

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
    /// Package dependencies from `[dependencies]` in assura.toml.
    pub dependencies: HashMap<String, DependencySpec>,
}

/// A single dependency specification.
///
/// Supports local path dependencies (Phase 1):
/// ```toml
/// [dependencies]
/// my-lib = { path = "../my-lib" }
/// ```
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    /// Inline version string (reserved for future registry support).
    Version(String),
    /// Detailed dependency with explicit source.
    Detailed(DetailedDependency),
}

/// Detailed dependency specification with source location.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct DetailedDependency {
    /// Local filesystem path to the dependency project root.
    pub path: Option<String>,
    /// Git repository URL (reserved for Phase 2).
    pub git: Option<String>,
    /// Git tag for version pinning (reserved for Phase 2).
    pub tag: Option<String>,
    /// Version requirement (reserved for Phase 3 registry).
    pub version: Option<String>,
}

impl DependencySpec {
    /// Returns the local path if this is a path dependency.
    pub fn local_path(&self) -> Option<&str> {
        match self {
            DependencySpec::Detailed(d) => d.path.as_deref(),
            DependencySpec::Version(_) => None,
        }
    }
}

/// Package metadata from `[package]` in assura.toml.
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

/// Build settings from `[build]` in assura.toml.
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

/// Verification settings from `[verify]` in assura.toml.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct VerifyConfig {
    pub smt_solver: SolverChoice,
    pub layer: u8,
    pub timeout: u64,
    /// Use native SMT string theory (QF_S/QF_SLIA) instead of integer encoding.
    /// Default: false (integer encoding is more predictable for most contracts).
    pub string_theory: bool,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            smt_solver: SolverChoice::Z3,
            layer: 1,
            timeout: 1000,
            string_theory: false,
        }
    }
}

/// Verification profile from `[profile]` in assura.toml.
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
                solver: project.verify.smt_solver,
                string_theory: project.verify.string_theory,
                ..VerifyOptions::default()
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
            allowed_effects: Vec::new(),
            denied_effects: Vec::new(),
            default_effect: "pure".to_string(),
        }
    }
}

/// SMT verification options.
///
/// This is the single source of truth for how [`assura_smt::Verifier`] is
/// configured from the pipeline, CLI, server, and MCP. Prefer threading
/// `VerifyOptions` (or full `CompilerConfig`) rather than re-building
/// `.parallel()` / `.with_decrease_checks()` ad hoc at each call site.
#[derive(Debug, Clone)]
pub struct VerifyOptions {
    /// Verification layer (0 = structural only, 1 = SMT).
    pub layer: u8,
    /// SMT solver timeout in milliseconds.
    pub timeout_ms: u64,
    /// Which SMT solver to use.
    pub solver: SolverChoice,
    /// Use native SMT string theory instead of integer encoding.
    pub string_theory: bool,
    /// Run clause verification in parallel via rayon (CLI / `compile_full` default).
    pub parallel: bool,
    /// Include pending decrease (termination) checks from the type checker.
    pub decrease_checks: bool,
    /// Prefer disk-backed verification caching when a source path is set
    /// (parallel path creates `VerificationCache` under the source parent dir).
    ///
    /// **Default is `false`**: cache keys are easy to get wrong relative to IR
    /// sidecars and encoder changes; `assura check` should be trustworthy by
    /// default. Enable explicitly for watch/incremental workflows or via
    /// `assura.toml` / `CompilerConfig` when you accept the tradeoff.
    /// When `false`, the parallel path still uses an ephemeral on-disk cache
    /// dir under `.` only if parallel is on (API shape); prefer
    /// [`VerifyOptions::for_tests`] which also sets `parallel: false`.
    pub enable_cache: bool,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            layer: 1,
            timeout_ms: 1000,
            solver: SolverChoice::Z3,
            string_theory: false,
            // Match historical CLI / compile_full defaults so all entry points
            // behave the same unless explicitly overridden.
            parallel: true,
            decrease_checks: true,
            enable_cache: false,
        }
    }
}

impl VerifyOptions {
    /// Lightweight options for unit tests and fast smoke checks (serial, no
    /// decrease checks, no cache). Keeps layer/solver/timeout at normal defaults.
    pub fn for_tests() -> Self {
        Self {
            parallel: false,
            decrease_checks: false,
            enable_cache: false,
            ..Self::default()
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

    match toml::from_str::<ProjectConfig>(&content) {
        Ok(config) => Some((config, project_root)),
        Err(e) => {
            eprintln!("warning: failed to parse {}: {e}", config_path.display());
            None
        }
    }
}
#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
