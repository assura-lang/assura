//! `feature_max` constant collection and refinement narrowing (solver-neutral).

use assura_ast::{BlockKind, Decl};

use crate::TypedFile;

/// Collect `feature_max` constants from a `TypedFile`'s declarations.
///
/// Returns a vec of (name, value) pairs. Only declarations with a
/// parseable integer value are included; non-integer or missing values
/// are silently skipped (they remain free solver variables).
pub fn collect_feature_max_constants(typed: &TypedFile) -> Vec<(String, i64)> {
    let mut constants = Vec::new();
    for decl in &typed.resolved.source.decls {
        // Value tokens include type annotation: [":", "Nat", "=", "65536"]
        // Find the token after "=" for the actual integer value.
        if let Decl::Block {
            kind,
            name,
            value: Some(tokens),
            ..
        } = &decl.node
            && *kind == BlockKind::FeatureMax
            && let Some(eq_pos) = tokens.iter().position(|t| t == "=")
            && let Some(val_str) = tokens.get(eq_pos + 1)
            && let Ok(v) = val_str.parse::<i64>()
        {
            constants.push((name.clone(), v));
        }
    }
    constants
}

/// Derive refinement narrowing pairs from `feature_max` constant names.
///
/// Per spec Section 14 (PLAT.2): `feature_max max_page_size = 4096` narrows
/// all variables named `page_size` with `page_size <= 4096`. The rule strips
/// the `max_` prefix (case-insensitive) from the constant name to produce the
/// narrowed variable name.
pub fn derive_narrowings(constants: &[(String, i64)]) -> Vec<(String, i64)> {
    let mut narrowings = Vec::new();
    for (name, value) in constants {
        let narrowed = name
            .strip_prefix("max_")
            .or_else(|| name.strip_prefix("MAX_"));
        if let Some(narrowed) = narrowed.filter(|s| !s.is_empty()) {
            narrowings.push((narrowed.to_string(), *value));
            let lower = narrowed.to_lowercase();
            if lower != narrowed {
                narrowings.push((lower, *value));
            }
        }
    }
    narrowings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_narrowings_basic() {
        let narrowings = derive_narrowings(&[("max_size".into(), 100)]);
        assert_eq!(narrowings.len(), 1);
        assert_eq!(narrowings[0], ("size".into(), 100));
    }

    #[test]
    fn derive_narrowings_no_prefix() {
        assert!(derive_narrowings(&[("size".into(), 50)]).is_empty());
    }

    #[test]
    fn derive_narrowings_uppercase_prefix() {
        let narrowings = derive_narrowings(&[("MAX_BUFFER".into(), 1024)]);
        assert_eq!(narrowings.len(), 2);
        assert_eq!(narrowings[0], ("BUFFER".into(), 1024));
        assert_eq!(narrowings[1], ("buffer".into(), 1024));
    }

    #[test]
    fn derive_narrowings_multiple() {
        let narrowings = derive_narrowings(&[("max_size".into(), 100), ("max_depth".into(), 10)]);
        assert_eq!(narrowings.len(), 2);
    }
}
