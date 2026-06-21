use assura_parser::ast::SpExpr;

use super::*;

/// Stub verification when Z3 is not available.
pub(crate) fn verify_stub(typed: &TypedFile) -> Vec<VerificationResult> {
    let mut results = Vec::new();
    for decl in &typed.resolved.source.decls {
        if let Decl::Contract(c) = &decl.node {
            for clause in &c.clauses {
                if matches!(clause.kind, ClauseKind::Ensures | ClauseKind::Invariant) {
                    results.push(VerificationResult::Unknown {
                        clause_desc: format!("{}::{:?}", c.name, clause.kind),
                        reason: "Z3 not available (compiled without z3-verify feature)".into(),
                    });
                }
            }
        }
    }
    results
}

/// Stub refinement subtype check when Z3 is not available.
pub(crate) fn refinement_stub(_ante: &SpExpr, _cons: &SpExpr) -> VerificationResult {
    VerificationResult::Unknown {
        clause_desc: "refinement_subtype".into(),
        reason: "Z3 not available (compiled without z3-verify feature)".into(),
    }
}

/// Stub refinement subtype check with context when Z3 is not available.
pub(crate) fn refinement_ctx_stub(
    _context: &[SpExpr],
    _ante: &SpExpr,
    _cons: &SpExpr,
) -> VerificationResult {
    VerificationResult::Unknown {
        clause_desc: "refinement_subtype_with_context".into(),
        reason: "Z3 not available (compiled without z3-verify feature)".into(),
    }
}
