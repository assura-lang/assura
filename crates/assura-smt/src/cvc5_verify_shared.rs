//! Shared verification helpers for CVC5 native and shell-out paths.

use assura_parser::ast::{Clause, ClauseKind, Decl, Expr};

use crate::VerificationResult;
use crate::cache::SessionCache;
use crate::cvc5_common::{collect_unmodelable_reasons_cvc5, expr_has_unmodelable_features_cvc5};
use crate::cvc5_feature_max::derive_narrowings_cvc5;

/// Collect lemma definitions from a typed file's declarations.
///
/// Maps each lemma name to its ensures clause bodies. This mirrors
/// `z3_backend::collect_lemma_defs` but is available without the
/// `z3-verify` feature.
pub(crate) fn collect_lemma_defs_for_cvc5(
    typed: &assura_types::TypedFile,
) -> std::collections::HashMap<String, Vec<&Expr>> {
    let mut lemmas = std::collections::HashMap::new();
    for decl in &typed.resolved.source.decls {
        if let Decl::FnDef(f) = &decl.node
            && f.is_lemma
        {
            let ensures: Vec<&Expr> = f
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

/// Shared contract setup for native and shell-out CVC5 verify paths.
pub(crate) fn cvc5_contract_shared_setup<'a>(
    clauses: &'a [Clause],
    constants: &[(String, i64)],
) -> (
    Vec<(String, i64)>,
    Vec<&'a Expr>,
    assura_types::FrameChecker,
) {
    let narrowings = derive_narrowings_cvc5(constants);
    let requires_exprs: Vec<&Expr> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let modifies_bodies: Vec<&Expr> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Modifies)
        .map(|c| &c.body)
        .collect();
    let frame_checker = if modifies_bodies.is_empty() {
        assura_types::FrameChecker::empty()
    } else {
        assura_types::FrameChecker::new(&modifies_bodies)
    };
    (narrowings, requires_exprs, frame_checker)
}

pub(crate) fn cvc5_lookup_cached_clause(
    cache: &mut SessionCache,
    cache_key: &str,
    desc: &str,
) -> Option<VerificationResult> {
    cache
        .lookup(cache_key)
        .map(|entry| match entry.result.as_str() {
            "verified" => VerificationResult::verified(desc.to_string()),
            other => VerificationResult::Unknown {
                clause_desc: desc.to_string(),
                reason: format!("cached: {other}"),
            },
        })
}

pub(crate) fn cvc5_unmodelable_precheck(desc: &str, body: &Expr) -> Option<VerificationResult> {
    if !expr_has_unmodelable_features_cvc5(body) {
        return None;
    }
    let reasons = collect_unmodelable_reasons_cvc5(body);
    Some(VerificationResult::Unknown {
        clause_desc: desc.to_string(),
        reason: format!(
            "clause uses features not yet encoded in SMT ({})",
            reasons.join(", ")
        ),
    })
}

pub(crate) fn store_cvc5_clause_cache(
    cache: &mut SessionCache,
    cache_key: String,
    result: &VerificationResult,
) {
    let result_str = match result {
        VerificationResult::Verified { .. } => "verified",
        VerificationResult::Counterexample { .. } => "counterexample",
        VerificationResult::Timeout { .. } => "timeout",
        VerificationResult::Unknown { .. } => "unknown",
    };
    cache.insert(cache_key, result_str.to_string(), 0);
}

#[cfg_attr(feature = "cvc5-verify", expect(dead_code))]
pub(crate) fn cvc5_clause_result_from_unsat(desc: &str, kind: ClauseKind) -> VerificationResult {
    if matches!(kind, ClauseKind::Invariant) {
        VerificationResult::Counterexample {
            clause_desc: desc.to_string(),
            model: "invariant is unsatisfiable".to_string(),
            counter_model: None,
        }
    } else {
        VerificationResult::verified(desc.to_string())
    }
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn cvc5_encode_failure(desc: &str) -> VerificationResult {
    VerificationResult::Unknown {
        clause_desc: desc.to_string(),
        reason: "could not encode clause to CVC5 terms".into(),
    }
}
