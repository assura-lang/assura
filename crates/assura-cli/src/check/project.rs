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

    // Module map keys are declared `module a.b` names (or filesystem-derived
    // dotted paths), not on-disk paths. Scan source files once for SHOWCASE.
    let showcase_modules = if showcase_only {
        collect_showcase_module_names(&project_root)
    } else {
        std::collections::HashSet::new()
    };

    // Type-check each resolved file with cross-module type information
    let modules_map = resolved_files.clone();
    for (module_path, resolved) in resolved_files {
        if showcase_only && !showcase_modules.contains(&module_path) {
            continue;
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

/// Walk `.assura` files under `project_root` and return module keys whose
/// first few lines contain `SHOWCASE` (must-pass demos).
///
/// Keys match resolve's discovery: declared `module a.b` path, else the
/// filesystem-derived dotted path.
fn collect_showcase_module_names(project_root: &Path) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    let mut files = Vec::new();
    collect_assura_files_under(project_root, &mut files);
    for file_path in files {
        let Ok(src) = std::fs::read_to_string(&file_path) else {
            continue;
        };
        let head: String = src.lines().take(8).collect::<Vec<_>>().join("\n");
        if !head.contains("SHOWCASE") {
            continue;
        }
        let fs_path = file_path
            .strip_prefix(project_root)
            .unwrap_or(&file_path)
            .with_extension("")
            .to_string_lossy()
            .replace(['/', '\\'], ".");
        let key = match assura_parser::parse(&src) {
            (Some(ast), _) => ast
                .module
                .as_ref()
                .map(|m| m.path.join("."))
                .unwrap_or(fs_path),
            (None, _) => fs_path,
        };
        names.insert(key);
    }
    names
}

fn collect_assura_files_under(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip generated/target noise if present under a project tree.
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && (name == "target" || name == "generated" || name == ".git")
            {
                continue;
            }
            collect_assura_files_under(&path, files);
        } else if path.extension().and_then(|e| e.to_str()) == Some("assura") {
            files.push(path);
        }
    }
}

#[cfg(test)]
mod showcase_path_tests {
    use super::collect_showcase_module_names;
    use std::fs;

    #[test]
    fn collect_showcase_uses_declared_module_name() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("hb.assura"),
            "// SHOWCASE\nmodule tls.heartbeat;\ncontract C { requires { true } ensures { true } }\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("other.assura"),
            "module other;\ncontract D { requires { true } ensures { true } }\n",
        )
        .unwrap();
        let names = collect_showcase_module_names(dir.path());
        assert!(
            names.contains("tls.heartbeat"),
            "expected declared module name, got {names:?}"
        );
        assert!(!names.contains("other"));
    }

    #[test]
    fn collect_showcase_falls_back_to_file_path() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("echo.assura"),
            "// SHOWCASE\ncontract E { requires { true } ensures { true } }\n",
        )
        .unwrap();
        let names = collect_showcase_module_names(dir.path());
        assert!(
            names.contains("echo"),
            "expected filesystem module key, got {names:?}"
        );
    }
}
