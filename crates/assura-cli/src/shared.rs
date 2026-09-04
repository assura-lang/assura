use std::io::{self, Read};
use std::path::Path;

use assura_config::{CompilerConfig, ProjectConfig};

/// Load `assura.toml` from the project root, if it exists.
pub(crate) fn load_project_config(
    start_path: &Path,
) -> Option<(ProjectConfig, std::path::PathBuf)> {
    assura_config::load_project_config(start_path, assura_resolve::find_project_root)
}

/// Maximum Assura/Rust source size accepted by CLI file and stdin readers.
pub(crate) const MAX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;

fn source_too_large(max: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("source exceeds maximum size of {max} bytes"),
    )
}

/// Read a UTF-8 source file, failing if it is larger than `max` bytes.
///
/// The error is a plain `InvalidData` message (JSON-friendly; no file body).
pub(crate) fn read_source_limited(path: impl AsRef<Path>, max: u64) -> io::Result<String> {
    let file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    if len > max {
        return Err(source_too_large(max));
    }
    let mut buf = String::new();
    file.take(max.saturating_add(1)).read_to_string(&mut buf)?;
    if buf.len() as u64 > max {
        return Err(source_too_large(max));
    }
    Ok(buf)
}

/// Read source from a path, or from stdin when `path` is `"-"`.
///
/// Display name for diagnostics is `"<stdin>"` when reading from stdin.
/// Both paths reject input larger than [`MAX_SOURCE_BYTES`].
pub(crate) fn read_source_arg(path: &str) -> io::Result<(String, String)> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .take(MAX_SOURCE_BYTES.saturating_add(1))
            .read_to_string(&mut buf)?;
        if buf.len() as u64 > MAX_SOURCE_BYTES {
            return Err(source_too_large(MAX_SOURCE_BYTES));
        }
        Ok((buf, "<stdin>".to_string()))
    } else {
        let source = read_source_limited(path, MAX_SOURCE_BYTES)?;
        Ok((source, path.to_string()))
    }
}

/// True when the path argument means standard input (`-`).
pub(crate) fn is_stdin_arg(path: &str) -> bool {
    path == "-"
}

/// Validate `--format` for subcommands that only support human/json.
///
/// When `as_json` is true (global `--json` or explicit `--format json`),
/// invalid values emit a JSON error object on stdout so agents stay pure.
pub(crate) fn validate_human_json_format(format: &str, cmd: &str, as_json: bool) {
    match format {
        "human" | "json" => {}
        other => {
            if as_json {
                let report = serde_json::json!({
                    "ok": false,
                    "command": cmd,
                    "error": "invalid_format",
                    "format": other,
                    "message": format!(
                        "invalid --format '{other}' for {cmd} (expected human or json)"
                    ),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                eprintln!("Error: invalid --format '{other}' for {cmd} (expected human or json)");
            }
            std::process::exit(2);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn max_source_bytes_is_16_mib() {
        assert_eq!(MAX_SOURCE_BYTES, 16 * 1024 * 1024);
    }

    #[test]
    fn read_source_limited_ok_under_cap() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "hello").unwrap();
        f.flush().unwrap();
        let s = read_source_limited(f.path(), 16).unwrap();
        assert_eq!(s, "hello");
    }

    #[test]
    fn read_source_limited_rejects_over_cap() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "hello world").unwrap();
        f.flush().unwrap();
        let err = read_source_limited(f.path(), 4).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let msg = err.to_string();
        assert!(
            msg.contains("maximum size"),
            "expected size-cap message, got {msg}"
        );
        assert!(
            !msg.contains("hello world"),
            "error must not include file body: {msg}"
        );
    }

    #[test]
    fn read_source_arg_file_uses_limited_reader() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "contract Foo {{}}\n").unwrap();
        f.flush().unwrap();
        let path = f.path().to_string_lossy().to_string();
        let (source, display) = read_source_arg(&path).unwrap();
        assert_eq!(source, "contract Foo {}\n");
        assert_eq!(display, path);
    }
}
