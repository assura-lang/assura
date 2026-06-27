use std::path::Path;

use assura_config::{CompilerConfig, ProjectConfig};

/// Load `assura.toml` from the project root, if it exists.
pub(crate) fn load_project_config(
    start_path: &Path,
) -> Option<(ProjectConfig, std::path::PathBuf)> {
    assura_config::load_project_config(start_path, assura_resolve::find_project_root)
}

/// Type alias: CLI code uses this name to destructure `CompilationOutput`.
pub(crate) type CompilationResult = assura_pipeline::CompilationOutput;

/// Format a counterexample as a clean single-line summary for diagnostics.
///
/// If a structured `CounterexampleModel` is available, produces a summary
/// like `"counterexample: a = -2, b = 1"`. Otherwise, parses the raw Z3
/// model string and formats it the same way.
pub(crate) fn format_counterexample_summary(
    counter_model: &Option<assura_smt::CounterexampleModel>,
    raw_model: &str,
) -> String {
    // Use the display module's formatting to get clean lines
    let lines = assura_smt::display::format_counterexample_lines(counter_model, raw_model);
    // Each line starts with "| "; strip that and join into a single line
    let pairs: Vec<&str> = lines
        .iter()
        .map(|l| l.strip_prefix("| ").unwrap_or(l.as_str()))
        .collect();
    if pairs.is_empty() {
        return "counterexample found".to_string();
    }
    format!("counterexample: {}", pairs.join("; "))
}

/// Discover the project root, load config, and build the dependency map.
///
/// Returns `(project_root, dep_map, dep_warnings)`. If no `assura.toml`
/// exists, `dep_map` is empty.
pub(crate) fn load_project_deps(
    project_dir: &Path,
) -> (
    std::path::PathBuf,
    assura_resolve::DependencyMap,
    Vec<String>,
) {
    let project_root = if project_dir.join("assura.toml").exists() {
        project_dir.to_path_buf()
    } else {
        assura_resolve::find_project_root(project_dir).unwrap_or_else(|| project_dir.to_path_buf())
    };

    let config = load_project_config(&project_root);
    let (dep_map, dep_warnings) = if let Some((ref cfg, ref root)) = config {
        assura_resolve::resolve_dependency_map(root, cfg)
    } else {
        (assura_resolve::DependencyMap::new(), vec![])
    };

    (project_root, dep_map, dep_warnings)
}

/// Run lex -> parse -> resolve -> typecheck on source text, collecting all diagnostics.
pub(crate) fn compile(source: &str, filename: &str) -> CompilationResult {
    assura_pipeline::compile(source, filename, &CompilerConfig::default())
}

/// Run the full pipeline with explicit configuration.
pub(crate) fn compile_with_config(
    source: &str,
    filename: &str,
    config: &CompilerConfig,
) -> CompilationResult {
    assura_pipeline::compile(source, filename, config)
}
