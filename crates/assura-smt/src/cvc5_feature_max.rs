//! `feature_max` constant collection and refinement narrowing (CVC5).

use assura_parser::ast::{BlockKind, Decl};

/// Collect `feature_max` constants from a `TypedFile`'s declarations.
pub(crate) fn collect_feature_max_constants_cvc5(typed: &crate::TypedFile) -> Vec<(String, i64)> {
    let mut constants = Vec::new();
    for decl in &typed.resolved.source.decls {
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

/// Derive refinement narrowings from `feature_max` constants.
///
/// For a constant named `max_X` or `MAX_X`, derives a narrowing
/// `(X, value)` that asserts `X <= value` in the solver.
pub(crate) fn derive_narrowings_cvc5(constants: &[(String, i64)]) -> Vec<(String, i64)> {
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
        let narrowings = derive_narrowings_cvc5(&[("max_size".into(), 100)]);
        assert_eq!(narrowings.len(), 1);
        assert_eq!(narrowings[0], ("size".into(), 100));
    }

    #[test]
    fn derive_narrowings_no_prefix() {
        assert!(derive_narrowings_cvc5(&[("size".into(), 50)]).is_empty());
    }
}
