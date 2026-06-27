//! Ghost and lemma function effect checking.

use std::ops::Range;

use assura_parser::ast::{ClauseKind, Expr, SpExpr};

use crate::TypeError;

// ---------------------------------------------------------------------------
// Ghost function effect checking (T043 CORE.1)
// ---------------------------------------------------------------------------

/// Extract the declared effect set from an `effects` clause on a function.
///
/// If no `effects` clause exists, returns `None` (meaning no explicit
/// declaration, which is NOT the same as pure).
fn extract_fn_effects(f: &assura_parser::ast::FnDef) -> Option<Vec<String>> {
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
fn extract_effect_names(expr: &SpExpr, names: &mut Vec<String>) {
    match &expr.node {
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
                suggestion: None,
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
                suggestion: None,
            });
        }
    }
    // If no effects clause is present, ghost fns are implicitly pure: OK.
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::{Clause, FnDef, Spanned};

    /// Build a minimal FnDef with the given name, ghost/lemma flags, and clauses.
    fn make_fn(name: &str, is_ghost: bool, is_lemma: bool, clauses: Vec<Clause>) -> FnDef {
        FnDef {
            name: name.into(),
            is_ghost,
            is_lemma,
            params: vec![],
            return_ty: None,
            clauses,
        }
    }

    /// Build an effects clause with the given effect names as Ident expressions.
    fn effects_clause_idents(names: &[&str]) -> Clause {
        let items: Vec<SpExpr> = names
            .iter()
            .map(|n| Spanned::no_span(Expr::Ident((*n).into())))
            .collect();
        Clause {
            kind: ClauseKind::Effects,
            body: Spanned::no_span(Expr::Block(items)),
            effect_variables: vec![],
        }
    }

    /// Build an effects clause with raw token strings.
    fn effects_clause_raw(tokens: &[&str]) -> Clause {
        Clause {
            kind: ClauseKind::Effects,
            body: Spanned::no_span(Expr::Raw(tokens.iter().map(|t| (*t).into()).collect())),
            effect_variables: vec![],
        }
    }

    // ---- Ghost function effect checks ----

    #[test]
    fn ghost_fn_no_effects_clause_ok() {
        let f = make_fn("my_ghost", true, false, vec![]);
        let mut errors = Vec::new();
        check_ghost_fn_effects(&f, &(0..10), &mut errors);
        assert!(
            errors.is_empty(),
            "ghost fn with no effects clause should be OK"
        );
    }

    #[test]
    fn ghost_fn_pure_effects_ok() {
        let f = make_fn(
            "my_ghost",
            true,
            false,
            vec![effects_clause_idents(&["pure"])],
        );
        let mut errors = Vec::new();
        check_ghost_fn_effects(&f, &(0..10), &mut errors);
        assert!(errors.is_empty(), "ghost fn with pure effects should be OK");
    }

    #[test]
    fn ghost_fn_io_effects_a54001() {
        let f = make_fn(
            "my_ghost",
            true,
            false,
            vec![effects_clause_idents(&["io"])],
        );
        let mut errors = Vec::new();
        check_ghost_fn_effects(&f, &(0..10), &mut errors);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A54001");
        assert!(errors[0].message.contains("my_ghost"));
        assert!(errors[0].message.contains("io"));
    }

    #[test]
    fn ghost_fn_multiple_non_pure_effects() {
        let f = make_fn(
            "spec_fn",
            true,
            false,
            vec![effects_clause_idents(&["io", "database"])],
        );
        let mut errors = Vec::new();
        check_ghost_fn_effects(&f, &(0..10), &mut errors);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("io"));
        assert!(errors[0].message.contains("database"));
    }

    #[test]
    fn ghost_fn_mixed_pure_and_non_pure() {
        let f = make_fn(
            "spec_fn",
            true,
            false,
            vec![effects_clause_idents(&["pure", "net"])],
        );
        let mut errors = Vec::new();
        check_ghost_fn_effects(&f, &(0..10), &mut errors);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("net"));
        // The effect list portion should not include "pure" as a listed effect;
        // the message template says "must be pure" but the effect_list only has "net"
        let effect_part = errors[0]
            .message
            .split("non-pure effects: ")
            .nth(1)
            .unwrap();
        let effect_list = effect_part.split(';').next().unwrap();
        assert!(
            !effect_list.contains("pure"),
            "effect list should not include 'pure': {effect_list}"
        );
    }

    #[test]
    fn ghost_fn_raw_token_effects() {
        let f = make_fn(
            "ghost_raw",
            true,
            false,
            vec![effects_clause_raw(&["fs", ",", "logging"])],
        );
        let mut errors = Vec::new();
        check_ghost_fn_effects(&f, &(0..10), &mut errors);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("fs"));
        assert!(errors[0].message.contains("logging"));
    }

    // ---- Lemma function effect checks ----

    #[test]
    fn lemma_fn_no_effects_clause_ok() {
        let f = make_fn("my_lemma", false, true, vec![]);
        let mut errors = Vec::new();
        check_lemma_fn_effects(&f, &(0..10), &mut errors);
        assert!(
            errors.is_empty(),
            "lemma fn with no effects clause should be OK"
        );
    }

    #[test]
    fn lemma_fn_pure_effects_ok() {
        let f = make_fn(
            "my_lemma",
            false,
            true,
            vec![effects_clause_idents(&["pure"])],
        );
        let mut errors = Vec::new();
        check_lemma_fn_effects(&f, &(0..10), &mut errors);
        assert!(errors.is_empty(), "lemma fn with pure effects should be OK");
    }

    #[test]
    fn lemma_fn_database_effect_a55001() {
        let f = make_fn(
            "my_lemma",
            false,
            true,
            vec![effects_clause_idents(&["database"])],
        );
        let mut errors = Vec::new();
        check_lemma_fn_effects(&f, &(0..10), &mut errors);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A55001");
        assert!(errors[0].message.contains("my_lemma"));
        assert!(errors[0].message.contains("database"));
    }

    #[test]
    fn lemma_fn_span_propagated() {
        let f = make_fn(
            "my_lemma",
            false,
            true,
            vec![effects_clause_idents(&["io"])],
        );
        let mut errors = Vec::new();
        check_lemma_fn_effects(&f, &(42..99), &mut errors);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].span, 42..99);
    }

    // ---- extract_effect_names (tested indirectly) ----

    #[test]
    fn raw_tokens_whitespace_and_commas_filtered() {
        // Commas and empty/whitespace-only tokens should be filtered out
        let f = make_fn(
            "g",
            true,
            false,
            vec![effects_clause_raw(&["  ", ",", "io", " ", ",", ""])],
        );
        let mut errors = Vec::new();
        check_ghost_fn_effects(&f, &(0..1), &mut errors);
        assert_eq!(errors.len(), 1);
        // Only "io" should appear (whitespace/commas filtered)
        assert!(errors[0].message.contains("io"));
    }

    #[test]
    fn non_effects_clause_ignored() {
        // A requires clause is not an effects clause; should be ignored
        let requires = Clause {
            kind: ClauseKind::Requires,
            body: Spanned::no_span(Expr::Ident("io".into())),
            effect_variables: vec![],
        };
        let f = make_fn("g", true, false, vec![requires]);
        let mut errors = Vec::new();
        check_ghost_fn_effects(&f, &(0..1), &mut errors);
        assert!(errors.is_empty(), "non-effects clauses should be ignored");
    }
}
