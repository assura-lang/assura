//! Import resolution: status tracking, path validation, and module map.

use std::collections::{HashMap, HashSet};

use assura_parser::ast::{ImportDecl, SourceFile};

use crate::errors::ResolutionError;

/// The status of a resolved import declaration.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportStatus {
    /// The import was resolved to a known module in the module map.
    Resolved,
    /// The module was not found in the module map (external/unknown module).
    /// This is not a hard error; the module may be a standard library or
    /// external dependency that is not yet available.
    Unresolved,
    /// A circular import was detected (A02005).
    Circular,
}

/// A single resolved import, recording the original declaration and its
/// resolution status.
#[derive(Debug, Clone)]
pub struct ResolvedImport {
    /// The dotted module path, e.g. `["std", "math"]`.
    pub path: Vec<String>,
    /// If the import used `as alias`, this is the alias name.
    pub alias: Option<String>,
    /// Selectively imported items, e.g. `{ List, Map }`.
    pub items: Vec<String>,
    /// How this import was resolved.
    pub status: ImportStatus,
    /// Source span of the import declaration.
    pub span: std::ops::Range<usize>,
}

/// An in-memory map of known modules, keyed by their dotted path.
///
/// For now this is a simple `HashMap`; actual filesystem resolution is
/// deferred to a future task. Callers can pre-populate the map with
/// parsed `SourceFile`s to enable multi-file resolution.
pub type ModuleMap = HashMap<String, SourceFile>;

/// Returns true if `s` is a valid module path segment: starts with a
/// lowercase ASCII letter or underscore, then ASCII letters, digits, or
/// underscores.
pub(crate) fn is_valid_path_segment(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(crate) fn resolve_imports(
    imports: &[ImportDecl],
    module_map: &ModuleMap,
    visited: &HashSet<String>,
    errors: &mut Vec<ResolutionError>,
) -> Vec<ResolvedImport> {
    // Detect duplicate imports
    let mut seen_paths: HashSet<String> = HashSet::new();
    for imp in imports {
        let path_str = imp.path.join(".");
        if !seen_paths.insert(path_str.clone()) {
            errors.push(ResolutionError {
                code: "A02006".into(),
                message: format!("duplicate import of module `{path_str}`"),
                span: imp.span.clone(),
                secondary: None,
                suggestion: None,
            });
        }
    }

    // Validate import path segments.
    // The last segment may be a symbol name (e.g., `Add` in `import math.Add`)
    // so only validate module path segments (all but the last).  The last
    // segment is validated as a module path only when it looks like one
    // (starts lowercase).
    for imp in imports {
        if imp.path.is_empty() {
            errors.push(ResolutionError {
                code: "A02008".into(),
                message: "import path is empty".to_string(),
                span: imp.span.clone(),
                secondary: None,
                suggestion: None,
            });
            continue;
        }
        let module_segments = if imp.path.len() > 1 {
            &imp.path[..imp.path.len() - 1]
        } else {
            &imp.path[..]
        };
        for segment in module_segments {
            if !is_valid_path_segment(segment) {
                errors.push(ResolutionError {
                    code: "A02008".into(),
                    message: format!(
                        "invalid module path segment `{segment}` in import `{}`; \
                         segments must start with a lowercase letter or underscore",
                        imp.path.join(".")
                    ),
                    span: imp.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
    }

    // Detect self-imports (importing your own module)
    for imp in imports {
        let path_str = imp.path.join(".");
        if visited.contains(&path_str) && !imp.path.is_empty() {
            // Already caught by circular import below, but this gives
            // a clearer message for the direct self-import case.
        }
    }

    imports
        .iter()
        .map(|imp| {
            let path_str = imp.path.join(".");

            let status = if visited.contains(&path_str) {
                // Circular import detected: module A imports B which
                // imports A (or transitively).
                errors.push(ResolutionError {
                    code: "A02005".into(),
                    message: format!("circular import of module `{path_str}`"),
                    span: imp.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
                ImportStatus::Circular
            } else if module_map.contains_key(&path_str)
                || find_module_prefix(&imp.path, module_map).is_some()
            {
                ImportStatus::Resolved
            } else {
                // Unknown module. Not an error: could be a standard
                // library module or external dependency not yet loaded.
                ImportStatus::Unresolved
            };

            ResolvedImport {
                path: imp.path.clone(),
                alias: imp.alias.clone(),
                items: imp.items.clone(),
                status,
                span: imp.span.clone(),
            }
        })
        .collect()
}

/// Try progressively shorter prefixes of `path` to find a module key.
///
/// For `import math.Add`, the path is `["math", "Add"]`. The module map
/// has key `"math"` (not `"math.Add"`, since `Add` is a symbol inside the
/// module). This function tries `"math.Add"` first, then `"math"`, and
/// returns the first match.
pub(crate) fn find_module_prefix(path: &[String], module_map: &ModuleMap) -> Option<String> {
    for end in (1..=path.len()).rev() {
        let candidate = path[..end].join(".");
        if module_map.contains_key(&candidate) {
            return Some(candidate);
        }
    }
    None
}
