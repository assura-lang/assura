//! Filesystem-based module resolution and project-level resolution.

use std::collections::{HashMap, HashSet};

use crate::errors::ResolvedFile;
use crate::imports::ModuleMap;
use crate::resolve_with_modules;

/// Find the project root by walking up from `start` until `assura.toml`
/// is found.  Returns the directory containing `assura.toml`, or `None`
/// if no config file exists (single-file mode).
pub fn find_project_root(start: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut dir = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };

    loop {
        if dir.join("assura.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Resolve a dotted module path (`a.b.c`) to a file path relative to
/// the project root.  The convention is `a/b/c.assura`.
pub(crate) fn resolve_module_path(
    project_root: &std::path::Path,
    module_path: &[String],
) -> Option<std::path::PathBuf> {
    if module_path.is_empty() {
        return None;
    }
    let mut file_path = project_root.to_path_buf();
    for segment in module_path {
        file_path.push(segment);
    }
    file_path.set_extension("assura");
    if file_path.exists() {
        Some(file_path)
    } else {
        None
    }
}

/// Errors produced during module graph construction.
#[derive(Debug, Clone)]
pub(crate) struct ModuleError {
    pub module_path: String,
    pub message: String,
}

/// A compiled module graph: all reachable modules parsed and resolved.
#[derive(Debug)]
pub(crate) struct ModuleGraph {
    /// All successfully resolved modules, keyed by dotted path.
    pub modules: ModuleMap,
    /// Errors encountered while loading modules.
    pub errors: Vec<ModuleError>,
    /// Topological order of module paths (leaves first, root last).
    pub order: Vec<String>,
}

/// Build a complete module graph starting from a root file.
///
/// 1. Parse the root file.
/// 2. For each `import` in the root, resolve the module path to a file,
///    parse it, and add it to the module map.
/// 3. Recursively resolve imports in each discovered module.
/// 4. Detect circular imports via the visited set.
/// 5. Return all modules in topological order (dependencies before
///    dependents).
pub(crate) fn build_module_graph(
    root_file: &std::path::Path,
    project_root: &std::path::Path,
) -> ModuleGraph {
    let mut modules = ModuleMap::new();
    let mut errors = Vec::new();
    let mut order = Vec::new();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();

    // Derive a module name from the root file path relative to the project root
    let root_module = file_to_module_path(root_file, project_root);

    // Parse the root file
    let root_source = match std::fs::read_to_string(root_file) {
        Ok(s) => s,
        Err(e) => {
            errors.push(ModuleError {
                module_path: root_module,
                message: format!("cannot read file: {e}"),
            });
            return ModuleGraph {
                modules,
                errors,
                order,
            };
        }
    };
    let (root_ast, parse_errs) = assura_parser::parse(&root_source);
    if !parse_errs.is_empty() {
        errors.push(ModuleError {
            module_path: root_module.clone(),
            message: format!("{} parse error(s)", parse_errs.len()),
        });
    }

    if let Some(ast) = root_ast {
        modules.insert(root_module.clone(), ast);
    } else {
        errors.push(ModuleError {
            module_path: root_module,
            message: "failed to parse root file".to_string(),
        });
        return ModuleGraph {
            modules,
            errors,
            order,
        };
    }

    // Recursively load all imports
    resolve_imports_recursive(
        &root_module,
        project_root,
        &mut modules,
        &mut visiting,
        &mut visited,
        &mut order,
        &mut errors,
    );

    // The root itself is last in topological order
    if !order.contains(&root_module) {
        order.push(root_module);
    }

    ModuleGraph {
        modules,
        errors,
        order,
    }
}

fn resolve_imports_recursive(
    module_path: &str,
    project_root: &std::path::Path,
    modules: &mut ModuleMap,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
    order: &mut Vec<String>,
    errors: &mut Vec<ModuleError>,
) {
    if visited.contains(module_path) {
        return;
    }
    if !visiting.insert(module_path.to_string()) {
        // Circular import
        errors.push(ModuleError {
            module_path: module_path.to_string(),
            message: "circular import detected".to_string(),
        });
        return;
    }

    // Get the imports for this module
    let imports: Vec<Vec<String>> = modules
        .get(module_path)
        .map(|source| source.imports.iter().map(|i| i.path.clone()).collect())
        .unwrap_or_default();

    for imp_path in &imports {
        let path_str = imp_path.join(".");
        if modules.contains_key(&path_str) {
            // Already loaded, just recurse for transitive imports
            resolve_imports_recursive(
                &path_str,
                project_root,
                modules,
                visiting,
                visited,
                order,
                errors,
            );
            continue;
        }

        // Resolve to filesystem
        match resolve_module_path(project_root, imp_path) {
            Some(file_path) => {
                match std::fs::read_to_string(&file_path) {
                    Ok(source) => {
                        let (ast, parse_errs) = assura_parser::parse(&source);
                        if !parse_errs.is_empty() {
                            errors.push(ModuleError {
                                module_path: path_str.clone(),
                                message: format!(
                                    "{}: {} parse error(s)",
                                    file_path.display(),
                                    parse_errs.len()
                                ),
                            });
                        }
                        if let Some(ast) = ast {
                            modules.insert(path_str.clone(), ast);
                        }
                        // Recursively resolve this module's imports
                        resolve_imports_recursive(
                            &path_str,
                            project_root,
                            modules,
                            visiting,
                            visited,
                            order,
                            errors,
                        );
                    }
                    Err(e) => {
                        errors.push(ModuleError {
                            module_path: path_str.clone(),
                            message: format!("{}: {e}", file_path.display()),
                        });
                    }
                }
            }
            None => {
                // Module not found on filesystem. Not necessarily an error:
                // could be a standard library module.
                errors.push(ModuleError {
                    module_path: path_str.clone(),
                    message: format!("module not found: {}", imp_path.join("/")),
                });
            }
        }
    }

    visiting.remove(module_path);
    visited.insert(module_path.to_string());
    let mp = module_path.to_string();
    if !order.contains(&mp) {
        order.push(mp);
    }
}

pub(crate) fn file_to_module_path(
    file: &std::path::Path,
    project_root: &std::path::Path,
) -> String {
    file.strip_prefix(project_root)
        .unwrap_or(file)
        .with_extension("")
        .to_string_lossy()
        .replace(['/', '\\'], ".")
}

/// Resolve all modules in a graph, producing `ResolvedFile` for each.
///
/// Processes modules in topological order so that a module's dependencies
/// are always resolved before the module itself.
pub(crate) fn resolve_module_graph(
    graph: &ModuleGraph,
) -> (HashMap<String, ResolvedFile>, Vec<ModuleError>) {
    let mut resolved = HashMap::new();
    let mut errors = Vec::new();

    for module_path in &graph.order {
        if let Some(source) = graph.modules.get(module_path) {
            let module_map: ModuleMap = graph
                .modules
                .iter()
                .filter(|(k, _)| *k != module_path)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            let mut visited = HashSet::new();
            visited.insert(module_path.clone());

            match resolve_with_modules(source, &module_map, &mut visited) {
                Ok(result) => {
                    resolved.insert(module_path.clone(), result);
                }
                Err(errs) => {
                    errors.push(ModuleError {
                        module_path: module_path.clone(),
                        message: format!("{} resolution error(s)", errs.len()),
                    });
                }
            }
        }
    }

    (resolved, errors)
}

/// Result of project-level resolution: resolved files + warnings.
pub type ProjectResult = Result<(HashMap<String, ResolvedFile>, Vec<String>), Vec<String>>;

/// High-level project resolution: given a root file inside a project,
/// discover the project root, build the module graph from imports,
/// and resolve all modules.
///
/// Returns `(resolved_files, warnings)` where `resolved_files` maps
/// dotted module paths to their `ResolvedFile`s.
pub fn resolve_project(root_file: &std::path::Path) -> ProjectResult {
    let project_root = find_project_root(root_file).unwrap_or_else(|| {
        root_file
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf()
    });

    let graph = build_module_graph(root_file, &project_root);

    let mut all_errors: Vec<String> = graph
        .errors
        .iter()
        .map(|e| format!("{}: {}", e.module_path, e.message))
        .collect();

    let (resolved, resolve_errors) = resolve_module_graph(&graph);
    all_errors.extend(
        resolve_errors
            .iter()
            .map(|e| format!("{}: {}", e.module_path, e.message)),
    );

    if resolved.is_empty() && !all_errors.is_empty() {
        Err(all_errors)
    } else {
        Ok((resolved, all_errors))
    }
}

/// Discover all `.assura` files under the project root, build the
/// module graph, and resolve all of them. This is the entry point
/// for `assura check /path/to/project/`.
pub fn discover_and_resolve_project(project_root: &std::path::Path) -> ProjectResult {
    // Find all .assura files
    let mut assura_files = Vec::new();
    collect_assura_files(project_root, &mut assura_files);

    if assura_files.is_empty() {
        return Err(vec![format!(
            "no .assura files found under {}",
            project_root.display()
        )]);
    }

    // Build a combined module map from all files
    let mut all_modules = ModuleMap::new();
    let mut errors = Vec::new();

    for file_path in &assura_files {
        let fs_path = file_to_module_path(file_path, project_root);
        match std::fs::read_to_string(file_path) {
            Ok(source) => {
                let (ast, parse_errs) = assura_parser::parse(&source);
                if !parse_errs.is_empty() {
                    errors.push(format!(
                        "{}: {} parse error(s)",
                        file_path.display(),
                        parse_errs.len()
                    ));
                }
                if let Some(ast) = ast {
                    // Use declared module name if present, otherwise
                    // fall back to the filesystem-derived path.
                    let key = ast
                        .module
                        .as_ref()
                        .map(|m| m.path.join("."))
                        .unwrap_or(fs_path);
                    all_modules.insert(key, ast);
                }
            }
            Err(e) => {
                errors.push(format!("{}: {e}", file_path.display()));
            }
        }
    }

    // Resolve each module with access to the full module map
    let mut resolved = HashMap::new();
    for (module_path, source) in &all_modules {
        let other_modules: ModuleMap = all_modules
            .iter()
            .filter(|(k, _)| *k != module_path)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut visited = HashSet::new();
        visited.insert(module_path.clone());
        match resolve_with_modules(source, &other_modules, &mut visited) {
            Ok(result) => {
                resolved.insert(module_path.clone(), result);
            }
            Err(errs) => {
                errors.push(format!(
                    "{}: {} resolution error(s)",
                    module_path,
                    errs.len()
                ));
            }
        }
    }

    if resolved.is_empty() && !errors.is_empty() {
        Err(errors)
    } else {
        Ok((resolved, errors))
    }
}

fn collect_assura_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_assura_files(&path, files);
            } else if path.extension().is_some_and(|e| e == "assura") {
                files.push(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ---- file_to_module_path ----

    #[test]
    fn file_to_module_simple() {
        let result = file_to_module_path(Path::new("/proj/math.assura"), Path::new("/proj"));
        assert_eq!(result, "math");
    }

    #[test]
    fn file_to_module_nested() {
        let result = file_to_module_path(Path::new("/proj/std/math.assura"), Path::new("/proj"));
        assert_eq!(result, "std.math");
    }

    #[test]
    fn file_to_module_deep_nesting() {
        let result = file_to_module_path(Path::new("/proj/a/b/c.assura"), Path::new("/proj"));
        assert_eq!(result, "a.b.c");
    }

    #[test]
    fn file_to_module_same_as_root() {
        // File is at the project root itself
        let result = file_to_module_path(Path::new("/proj/main.assura"), Path::new("/proj"));
        assert_eq!(result, "main");
    }

    #[test]
    fn file_to_module_outside_root() {
        // File outside the project root falls back to full path
        let result = file_to_module_path(Path::new("/other/foo.assura"), Path::new("/proj"));
        // strip_prefix fails, so we get the full path minus extension
        assert!(result.contains("foo"));
    }

    // ---- find_project_root ----

    #[test]
    fn find_project_root_no_config() {
        let tmp = std::env::temp_dir().join("assura_test_no_config");
        let _ = std::fs::create_dir_all(&tmp);
        assert!(find_project_root(&tmp).is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn find_project_root_with_config() {
        let tmp = std::env::temp_dir().join("assura_test_with_config");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("assura.toml"), "").unwrap();
        let result = find_project_root(&tmp);
        assert_eq!(result.unwrap(), tmp);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn find_project_root_from_subdirectory() {
        let tmp = std::env::temp_dir().join("assura_test_subdir");
        let sub = tmp.join("src");
        let _ = std::fs::create_dir_all(&sub);
        std::fs::write(tmp.join("assura.toml"), "").unwrap();
        let result = find_project_root(&sub);
        assert_eq!(result.unwrap(), tmp);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn find_project_root_from_file_path() {
        let tmp = std::env::temp_dir().join("assura_test_file_path");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("assura.toml"), "").unwrap();
        let file = tmp.join("main.assura");
        std::fs::write(&file, "").unwrap();
        let result = find_project_root(&file);
        assert_eq!(result.unwrap(), tmp);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ---- resolve_module_path ----

    #[test]
    fn resolve_module_path_empty() {
        let tmp = std::env::temp_dir();
        assert!(resolve_module_path(&tmp, &[]).is_none());
    }

    #[test]
    fn resolve_module_path_exists() {
        let tmp = std::env::temp_dir().join("assura_test_resolve_mod");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("math.assura"), "").unwrap();
        let result = resolve_module_path(&tmp, &["math".into()]);
        result.unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_module_path_not_found() {
        let tmp = std::env::temp_dir().join("assura_test_resolve_miss");
        let _ = std::fs::create_dir_all(&tmp);
        let result = resolve_module_path(&tmp, &["nonexistent".into()]);
        assert!(result.is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ---- collect_assura_files ----

    #[test]
    fn collect_assura_files_empty_dir() {
        let tmp = std::env::temp_dir().join("assura_test_collect_empty");
        let _ = std::fs::create_dir_all(&tmp);
        let mut files = Vec::new();
        collect_assura_files(&tmp, &mut files);
        assert!(files.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn collect_assura_files_finds_files() {
        let tmp = std::env::temp_dir().join("assura_test_collect_files");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("a.assura"), "").unwrap();
        std::fs::write(tmp.join("b.rs"), "").unwrap(); // Not .assura
        let mut files = Vec::new();
        collect_assura_files(&tmp, &mut files);
        assert_eq!(files.len(), 1);
        assert!(files[0].to_string_lossy().contains("a.assura"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn collect_assura_files_recursive() {
        let tmp = std::env::temp_dir().join("assura_test_collect_recursive");
        let sub = tmp.join("sub");
        let _ = std::fs::create_dir_all(&sub);
        std::fs::write(tmp.join("a.assura"), "").unwrap();
        std::fs::write(sub.join("b.assura"), "").unwrap();
        let mut files = Vec::new();
        collect_assura_files(&tmp, &mut files);
        assert_eq!(files.len(), 2);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ---- ModuleError ----

    #[test]
    fn module_error_debug() {
        let e = ModuleError {
            module_path: "std.math".into(),
            message: "not found".into(),
        };
        let debug = format!("{e:?}");
        assert!(debug.contains("std.math"));
        assert!(debug.contains("not found"));
    }
}
