//! Shared verification **labels** and lemma collection (one compiler brain).
//!
//! Owns human-readable clause descriptors (`Foo::ensures`) and lemma ensures
//! extraction from a [`TypedFile`]. Z3 previously used `solver::clause_desc`
//! while CVC5 used `format!("{name}::{:?}", kind)` (Debug strings like
//! `Ensures`), producing divergent cache keys and CLI output. Both use this
//! module now.
//!
//! Complements [`crate::clause_policy`] / [`crate::prelude_policy`] /
//! [`crate::clause_gate_policy`]; does not unify expression encoding.

use std::collections::HashMap;

use assura_ast::{ClauseKind, Decl, SpExpr};
use assura_types::TypedFile;

/// Stable kind segment for clause descriptors (lowercase / snake, not `Debug`).
pub(crate) fn clause_kind_label(kind: &ClauseKind) -> &str {
    match kind {
        ClauseKind::Requires => "requires",
        ClauseKind::Ensures => "ensures",
        ClauseKind::Invariant => "invariant",
        ClauseKind::Effects => "effects",
        ClauseKind::Modifies => "modifies",
        ClauseKind::Input => "input",
        ClauseKind::Output => "output",
        ClauseKind::Errors => "errors",
        ClauseKind::Rule => "rule",
        ClauseKind::DataFlow => "data_flow",
        ClauseKind::MustNot => "must_not",
        ClauseKind::Decreases => "decreases",
        ClauseKind::Ordering => "ordering",
        ClauseKind::Other(s) => s.as_str(),
    }
}

/// Canonical clause descriptor: `{parent}::{kind_label}` (e.g. `SafeDiv::ensures`).
///
/// Used in results, session-cache keys (via [`crate::clause_gate_policy`]), and logs.
pub(crate) fn clause_desc(parent_name: &str, kind: &ClauseKind) -> String {
    format!("{parent_name}::{}", clause_kind_label(kind))
}

/// Service / standalone invariant descriptor (`{parent}::invariant`).
pub(crate) fn invariant_desc(parent_name: &str) -> String {
    format!("{parent_name}::invariant")
}

/// Feature-clause descriptor (`{parent}: {feature_label}`), colon not double-colon.
pub(crate) fn feature_clause_desc(parent_name: &str, feature_label: &str) -> String {
    format!("{parent_name}: {feature_label}")
}

/// Collect lemma definitions: lemma name → ensures clause bodies.
///
/// Shared by Z3 `verify_file` and CVC5 lemma injection (was duplicated as
/// `collect_lemma_defs` / `collect_lemma_defs_for_cvc5`).
pub(crate) fn collect_lemma_defs(typed: &TypedFile) -> HashMap<String, Vec<&SpExpr>> {
    let mut lemmas = HashMap::new();
    for decl in &typed.resolved.source.decls {
        if let Decl::FnDef(f) = &decl.node
            && f.is_lemma
        {
            let ensures: Vec<&SpExpr> = f
                .clauses
                .iter()
                .filter(|c| c.kind == ClauseKind::Ensures)
                .map(|c| &c.body)
                .collect();
            lemmas.insert(f.name.clone(), ensures);
        }
    }
    lemmas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clause_desc_uses_stable_kind_labels_not_debug() {
        assert_eq!(clause_desc("C", &ClauseKind::Ensures), "C::ensures");
        assert_eq!(clause_desc("C", &ClauseKind::MustNot), "C::must_not");
        assert_eq!(
            clause_desc("C", &ClauseKind::Other("sec.1".into())),
            "C::sec.1"
        );
        // Must not match old CVC5 `{:?}` form
        assert_ne!(clause_desc("C", &ClauseKind::Ensures), "C::Ensures");
    }

    #[test]
    fn invariant_and_feature_desc_shapes() {
        assert_eq!(invariant_desc("Svc"), "Svc::invariant");
        assert_eq!(
            feature_clause_desc("C", "structural_invariant"),
            "C: structural_invariant"
        );
    }
}
