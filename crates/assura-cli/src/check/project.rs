//! Project-mode check: multi-file resolve + type-check.

use super::super::*;

// ---------------------------------------------------------------------------
// Project-mode check: resolve and type-check all .assura files in a project
// ---------------------------------------------------------------------------

pub(crate) fn run_check_project(
    project_dir: &Path,
    output_mode: OutputMode,
    _verbosity: Verbosity,
    config: &CompilerConfig,
) {
    let project_root = if project_dir.join("assura.toml").exists() {
        project_dir.to_path_buf()
    } else {
        assura_resolve::find_project_root(project_dir).unwrap_or_else(|| project_dir.to_path_buf())
    };

    if output_mode == OutputMode::Human {
        eprintln!("Checking project at {}", project_root.display());
    }

    let (resolved_files, warnings) =
        match assura_resolve::discover_and_resolve_project(&project_root) {
            Ok(pair) => pair,
            Err(errors) => {
                for e in &errors {
                    eprintln!("Error: {e}");
                }
                process::exit(1);
            }
        };

    for w in &warnings {
        if output_mode == OutputMode::Human {
            eprintln!("Warning: {w}");
        }
    }

    let mut total_errors = 0usize;
    let mut total_modules = 0usize;
    let mut total_bindings = 0usize;

    // Type-check each resolved file with cross-module type information
    for (module_path, resolved) in &resolved_files {
        total_modules += 1;
        match assura_types::TypeChecker::new()
            .config(config.type_check.clone())
            .modules(resolved_files.clone())
            .check(resolved)
        {
            Ok(typed) => {
                let bindings = typed.type_env.len();
                total_bindings += bindings;
                if output_mode == OutputMode::Human {
                    eprintln!(
                        "OK  {module_path}: {} symbol(s), {bindings} binding(s)",
                        resolved.symbols.symbols.len()
                    );
                }
            }
            Err(errors) => {
                total_errors += errors.len();
                if output_mode == OutputMode::Human {
                    eprintln!("ERR {module_path}: {} error(s)", errors.len());
                    for err in &errors {
                        eprintln!("  {}: {}", err.code, err.message);
                    }
                }
            }
        }
    }

    if output_mode == OutputMode::Human {
        eprintln!();
        eprintln!(
            "Project: {total_modules} module(s), {total_bindings} binding(s), {total_errors} error(s)"
        );
    }

    if total_errors > 0 {
        process::exit(1);
    }
}
