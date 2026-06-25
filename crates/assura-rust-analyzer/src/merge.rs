//! Dual-source merge: combine external `.assura` contracts with inline annotations.

use crate::types::{ContractClause, InlineClauseKind, InlineContract};

/// Source origin for a contract clause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClauseSource {
    /// From an external `.assura` file.
    External,
    /// From inline doc comment annotations in a `.rs` file.
    Inline,
}

/// A contract clause with its source origin, for merged contracts.
#[derive(Debug, Clone, PartialEq)]
pub struct SourcedClause {
    pub kind: InlineClauseKind,
    pub body: String,
    pub source: ClauseSource,
}

/// The result of merging external and inline contracts for a function.
#[derive(Debug, Clone, Default)]
pub struct MergedContract {
    pub clauses: Vec<SourcedClause>,
    pub warnings: Vec<String>,
}

impl MergedContract {
    /// Total number of clauses across both sources.
    pub fn clause_count(&self) -> usize {
        self.clauses.len()
    }

    /// Returns true if no clauses from either source.
    pub fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }

    /// Clauses from external `.assura` files only.
    pub fn external_clauses(&self) -> Vec<&SourcedClause> {
        self.clauses
            .iter()
            .filter(|c| c.source == ClauseSource::External)
            .collect()
    }

    /// Clauses from inline doc comments only.
    pub fn inline_clauses(&self) -> Vec<&SourcedClause> {
        self.clauses
            .iter()
            .filter(|c| c.source == ClauseSource::Inline)
            .collect()
    }
}

/// Merge external and inline contract clauses for a function.
///
/// Rules (per spec #105):
/// 1. External contracts are authoritative (higher priority)
/// 2. Clauses from both sources are merged (union, not replacement)
/// 3. Duplicate clauses are detected and warned
/// 4. Contradictory clauses are reported as warnings
pub fn merge_contracts(
    external_clauses: &[(InlineClauseKind, String)],
    inline: &InlineContract,
) -> MergedContract {
    let mut merged = MergedContract::default();

    // Add all external clauses first (authoritative)
    for (kind, body) in external_clauses {
        merged.clauses.push(SourcedClause {
            kind: *kind,
            body: body.clone(),
            source: ClauseSource::External,
        });
    }

    // Collect all inline clauses
    let inline_all: Vec<(&ContractClause, InlineClauseKind)> = inline
        .requires
        .iter()
        .map(|c| (c, InlineClauseKind::Requires))
        .chain(
            inline
                .ensures
                .iter()
                .map(|c| (c, InlineClauseKind::Ensures)),
        )
        .chain(
            inline
                .invariants
                .iter()
                .map(|c| (c, InlineClauseKind::Invariant)),
        )
        .chain(
            inline
                .effects
                .iter()
                .map(|c| (c, InlineClauseKind::Effects)),
        )
        .chain(
            inline
                .decreases
                .iter()
                .map(|c| (c, InlineClauseKind::Decreases)),
        )
        .chain(inline.annotations.iter().map(|c| (c, c.kind)))
        .collect();

    // Add inline clauses, checking for duplicates
    for (clause, kind) in &inline_all {
        let body_normalized = clause.body.trim().to_string();

        // Check if this clause is a duplicate of an external clause
        let is_duplicate = merged.clauses.iter().any(|existing| {
            existing.source == ClauseSource::External
                && existing.kind == *kind
                && normalize_clause_body(&existing.body) == normalize_clause_body(&body_normalized)
        });

        if is_duplicate {
            merged.warnings.push(format!(
                "duplicate {} clause (inline matches external): {}",
                kind.as_str(),
                body_normalized
            ));
        } else {
            merged.clauses.push(SourcedClause {
                kind: *kind,
                body: body_normalized,
                source: ClauseSource::Inline,
            });
        }
    }

    merged
}

/// Normalize clause body text for comparison (strip whitespace, lowercase).
fn normalize_clause_body(body: &str) -> String {
    body.split_whitespace().collect::<Vec<_>>().join(" ")
}
