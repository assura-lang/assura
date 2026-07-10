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
    showcase_only: bool,
    _strict: bool,
) {
    let (project_root, dep_map, dep_warnings) = load_project_deps(project_dir);

    if output_mode == OutputMode::Human {
        eprintln!("Checking project at {}", project_root.display());
        if showcase_only {
            eprintln!("  (showcase-only: files with SHOWCASE header)");
        }
    }
    for w in &dep_warnings {
        if output_mode == OutputMode::Human {
            eprintln!("Warning: {w}");
        }
    }

    let (resolved_files, warnings) =
        match assura_resolve::discover_and_resolve_project_with_deps(&project_root, &dep_map) {
            Ok(pair) => pair,
            Err(errors) => {
                if output_mode == OutputMode::Json {
                    let diags: Vec<_> = errors
                        .iter()
                        .map(|e| {
                            assura_diagnostics::Diagnostic::error("A02000", e.to_string(), 0..0)
                                .with_file(project_root.display().to_string())
                        })
                        .collect();
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&diags).unwrap_or_default()
                    );
                } else {
                    for e in &errors {
                        eprintln!("Error: {e}");
                    }
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
    let mut module_results: Vec<serde_json::Value> = Vec::new();
    let mut all_diags: Vec<assura_diagnostics::Diagnostic> = Vec::new();

    // Type-check each resolved file with cross-module type information
    let modules_map = resolved_files.clone();
    for (module_path, resolved) in resolved_files {
        if showcase_only {
            // Prefer co-located source path; fall back to module path string.
            let path = Path::new(&module_path);
            let src = std::fs::read_to_string(path).unwrap_or_default();
            let head: String = src.lines().take(8).collect::<Vec<_>>().join("\n");
            if !head.contains("SHOWCASE") {
                continue;
            }
        }
        total_modules += 1;
        match assura_types::TypeChecker::new()
            .config(config.type_check.clone())
            .modules(modules_map.clone())
            .check(resolved)
        {
            Ok(typed) => {
                let bindings = typed.type_env.len();
                total_bindings += bindings;
                let symbols = typed.resolved.symbols.symbols.len();
                if output_mode == OutputMode::Human {
                    eprintln!("OK  {module_path}: {symbols} symbol(s), {bindings} binding(s)");
                } else {
                    module_results.push(serde_json::json!({
                        "module": module_path,
                        "status": "ok",
                        "symbols": symbols,
                        "bindings": bindings,
                        "errors": 0,
                    }));
                }
            }
            Err((errors, _returned_resolved)) => {
                total_errors += errors.len();
                if output_mode == OutputMode::Human {
                    eprintln!("ERR {module_path}: {} error(s)", errors.len());
                    for err in &errors {
                        eprintln!("  {}: {}", err.code, err.message);
                    }
                } else {
                    module_results.push(serde_json::json!({
                        "module": module_path,
                        "status": "error",
                        "errors": errors.len(),
                        "messages": errors.iter().map(|e| {
                            serde_json::json!({
                                "code": e.code.as_str(),
                                "message": e.message,
                            })
                        }).collect::<Vec<_>>(),
                    }));
                    for err in &errors {
                        all_diags.push(
                            assura_diagnostics::Diagnostic::error(
                                err.code.as_str(),
                                err.message.clone(),
                                err.span.clone(),
                            )
                            .with_file(module_path.clone()),
                        );
                    }
                }
            }
        }
    }

    if output_mode == OutputMode::Json {
        let report = serde_json::json!({
            "project": project_root.display().to_string(),
            "modules": total_modules,
            "bindings": total_bindings,
            "errors": total_errors,
            "success": total_errors == 0,
            "results": module_results,
            "diagnostics": all_diags,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    } else {
        eprintln!();
        eprintln!(
            "Project: {total_modules} module(s), {total_bindings} binding(s), {total_errors} error(s)"
        );
    }

    if total_errors > 0 {
        process::exit(1);
    }
}
