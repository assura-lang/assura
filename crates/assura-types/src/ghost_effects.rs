//! Ghost and lemma function effect checking.

use std::ops::Range;

use assura_parser::ast::{ClauseKind, Expr};

use crate::TypeError;

// ---------------------------------------------------------------------------
// Ghost function effect checking (T043 CORE.1)
// ---------------------------------------------------------------------------

/// Extract the declared effect set from an `effects` clause on a function.
///
/// If no `effects` clause exists, returns `None` (meaning no explicit
/// declaration, which is NOT the same as pure).
fn extract_fn_effects(f: &assura_parser::ast::FnDef) -> Option<Vec<std::string::String>> {
    for clause in &f.clauses {
        if clause.kind == ClauseKind::Effects {
            // Extract effect names from the clause body
            let mut names = Vec::new();
            extract_effect_names(&clause.body, &mut names);
            return Some(names);
        }
    }
    None
}

/// Recursively extract effect name strings from an expression.
fn extract_effect_names(expr: &Expr, names: &mut Vec<std::string::String>) {
    match expr {
        Expr::Ident(s) => names.push(s.clone()),
        Expr::Raw(tokens) => {
            for tok in tokens {
                let trimmed = tok.trim().to_string();
                if !trimmed.is_empty() && trimmed != "," {
                    names.push(trimmed);
                }
            }
        }
        Expr::Block(items) => {
            for item in items {
                extract_effect_names(item, names);
            }
        }
        _ => {}
    }
}

/// Check that a lemma function has pure effects.
///
/// Lemma functions are proof functions that generate no runtime code.
/// They cannot perform side effects. If an `effects` clause is present
/// and declares non-pure effects, emit A55001.
pub(crate) fn check_lemma_fn_effects(
    f: &assura_parser::ast::FnDef,
    span: &Range<usize>,
    errors: &mut Vec<TypeError>,
) {
    if let Some(effects) = extract_fn_effects(f) {
        let has_non_pure = effects.iter().any(|e| e != "pure");
        if has_non_pure {
            let effect_list = effects
                .iter()
                .filter(|e| *e != "pure")
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            errors.push(TypeError {
                code: "A55001".into(),
                message: format!(
                    "lemma function `{}` has non-pure effects: {effect_list}; \
                     lemma functions must be pure (no side effects)",
                    f.name,
                ),
                span: span.clone(),
                secondary: None,
            });
        }
    }
    // If no effects clause is present, lemma fns are implicitly pure: OK.
}

/// Check that a ghost function has pure effects.
///
/// Ghost functions exist only for verification; they cannot perform side
/// effects. If an `effects` clause is present and declares non-pure effects,
/// emit A54001.
pub(crate) fn check_ghost_fn_effects(
    f: &assura_parser::ast::FnDef,
    span: &Range<usize>,
    errors: &mut Vec<TypeError>,
) {
    if let Some(effects) = extract_fn_effects(f) {
        // "pure" or an empty list means no effects: that's fine for ghost fns.
        let has_non_pure = effects.iter().any(|e| e != "pure");
        if has_non_pure {
            let effect_list = effects
                .iter()
                .filter(|e| *e != "pure")
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            errors.push(TypeError {
                code: "A54001".into(),
                message: format!(
                    "ghost function `{}` has non-pure effects: {effect_list}; \
                     ghost functions must be pure (no side effects)",
                    f.name,
                ),
                span: span.clone(),
                secondary: None,
            });
        }
    }
    // If no effects clause is present, ghost fns are implicitly pure: OK.
}
