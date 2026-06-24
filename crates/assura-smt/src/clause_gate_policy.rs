//! Per-clause **gate** policy before/after solver work (one compiler brain).
//!
//! Owns unmodelable short-circuit results, session-cache key shape, and cache
//! entry ↔ [`VerificationResult`] mapping. Z3 and CVC5 still implement their
//! own unmodelable *walks* (`expr_has_unmodelable_features` vs CVC5 mirror)
//! and solver calls; this module unifies orchestration outcomes so backends
//! do not invent divergent reason strings or cache key formats.
//!
//! Complements [`crate::clause_policy`] (contract-level partition/polarity) and
//! [`crate::prelude_policy`] (prelude order/constraints).

use assura_ast::{ClauseKind, SpExpr};

use crate::VerificationResult;
use crate::cache::SessionCache;

/// Per-clause step order inside the verifiable loop (after contract prelude).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClauseGateStep {
    /// Known-limitation / unmodelable feature gate (may skip solver entirely).
    UnmodelablePrecheck,
    /// Session cache lookup (may skip solver entirely).
    SessionCacheLookup,
    /// Backend: incremental push, frame axioms, encode body, polarity assert, check-sat.
    SolverWork,
    /// Session cache insert from solver outcome.
    SessionCacheStore,
}

/// Canonical per-clause gate order (Z3 incremental and CVC5 per-clause/contract paths).
pub(crate) const CLAUSE_GATE_ORDER: &[ClauseGateStep] = &[
    ClauseGateStep::UnmodelablePrecheck,
    ClauseGateStep::SessionCacheLookup,
    ClauseGateStep::SolverWork,
    ClauseGateStep::SessionCacheStore,
];

#[inline]
pub(crate) fn clause_gate_order() -> &'static [ClauseGateStep] {
    CLAUSE_GATE_ORDER
}

/// Session-cache key for one verifiable clause (shared Z3 / CVC5 shape).
///
/// Includes `kind` (stable label, not `Debug`) so ensures/invariant/must_not with
/// identical bodies do not collide and keys match [`crate::verify_labels`] descriptors.
pub(crate) fn clause_session_cache_key(desc: &str, kind: &ClauseKind, body: &SpExpr) -> String {
    let kind_label = crate::verify_labels::clause_kind_label(kind);
    format!("{desc}::{kind_label}:{body:?}")
}

/// Reason detail when a clause body uses features not fully encoded in SMT.
pub(crate) fn unmodelable_clause_detail(reasons: &[String]) -> String {
    if reasons.is_empty() {
        "clause uses features".to_string()
    } else {
        format!("clause uses features ({})", reasons.join(", "))
    }
}

/// Build the standard unmodelable/unknown-not-encoded result for a clause gate miss.
pub(crate) fn unmodelable_clause_result(
    desc: impl Into<String>,
    reasons: &[String],
) -> VerificationResult {
    VerificationResult::unknown_not_encoded(desc, unmodelable_clause_detail(reasons))
}

/// If `has_unmodelable` is true, return the gate result; otherwise `None` (continue to solver).
pub(crate) fn unmodelable_precheck_if(
    desc: &str,
    has_unmodelable: bool,
    reasons: &[String],
) -> Option<VerificationResult> {
    if !has_unmodelable {
        return None;
    }
    Some(unmodelable_clause_result(desc, reasons))
}

/// Coarse result tag stored in [`SessionCache`] (solver-neutral).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClauseCacheTag {
    Verified,
    Counterexample,
    Timeout,
    Unknown,
}

impl ClauseCacheTag {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Counterexample => "counterexample",
            Self::Timeout => "timeout",
            Self::Unknown => "unknown",
        }
    }

    pub(crate) fn from_result(result: &VerificationResult) -> Self {
        match result {
            VerificationResult::Verified { .. } => Self::Verified,
            VerificationResult::Counterexample { .. } => Self::Counterexample,
            VerificationResult::Timeout { .. } => Self::Timeout,
            VerificationResult::Unknown { .. } => Self::Unknown,
        }
    }

    pub(crate) fn parse(s: &str) -> Self {
        match s {
            "verified" => Self::Verified,
            "timeout" => Self::Timeout,
            "counterexample" => Self::Counterexample,
            _ => Self::Unknown,
        }
    }
}

/// Reconstruct a [`VerificationResult`] from a session-cache entry (lookup path).
///
/// Cached non-verified outcomes are surfaced as [`VerificationResult::Unknown`] with
/// a `cached: …` reason (preserves prior CVC5 behavior) except timeout which restores
/// [`VerificationResult::Timeout`] when the stored tag is exactly `timeout` (Z3 path).
pub(crate) fn result_from_session_cache_tag(desc: &str, tag: ClauseCacheTag) -> VerificationResult {
    match tag {
        ClauseCacheTag::Verified => VerificationResult::verified(desc.to_string()),
        ClauseCacheTag::Timeout => VerificationResult::Timeout {
            clause_desc: desc.to_string(),
        },
        other => VerificationResult::Unknown {
            clause_desc: desc.to_string(),
            reason: format!("cached: {}", other.as_str()),
        },
    }
}

/// Lookup session cache; `None` means miss (run solver).
pub(crate) fn lookup_clause_session_cache(
    cache: &mut SessionCache,
    cache_key: &str,
    desc: &str,
) -> Option<VerificationResult> {
    cache.lookup(cache_key).map(|entry| {
        let tag = ClauseCacheTag::parse(entry.result.as_str());
        // Z3 historically rehydrated exact timeout/verified; unknown strings became Unknown reason=other.
        // Align on: verified/timeout as first-class; anything else as cached/unknown.
        match entry.result.as_str() {
            "verified" => VerificationResult::verified(desc.to_string()),
            "timeout" => VerificationResult::Timeout {
                clause_desc: desc.to_string(),
            },
            other if other == "counterexample" || other == "unknown" => {
                result_from_session_cache_tag(desc, tag)
            }
            other => VerificationResult::Unknown {
                clause_desc: desc.to_string(),
                reason: other.to_string(),
            },
        }
    })
}

/// Store solver outcome in session cache (coarse tag only).
pub(crate) fn store_clause_session_cache(
    cache: &mut SessionCache,
    cache_key: String,
    result: &VerificationResult,
) {
    let tag = ClauseCacheTag::from_result(result);
    cache.insert(cache_key, tag.as_str().to_string(), 0);
}

/// Encode failure (backend could not build terms / SMT-LIB) — not a known limitation marker.
pub(crate) fn clause_encode_failure(desc: &str, backend: &str) -> VerificationResult {
    VerificationResult::Unknown {
        clause_desc: desc.to_string(),
        reason: format!("could not encode clause to {backend}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::{is_known_smt_limitation, not_encoded_reason};
    use assura_ast::{Expr, Literal, Spanned};

    fn sp_bool() -> SpExpr {
        Spanned::no_span(Expr::Literal(Literal::Bool(true)))
    }

    #[test]
    fn gate_order_starts_with_unmodelable_then_cache() {
        let order = clause_gate_order();
        assert_eq!(order[0], ClauseGateStep::UnmodelablePrecheck);
        assert_eq!(order[1], ClauseGateStep::SessionCacheLookup);
        assert_eq!(order[2], ClauseGateStep::SolverWork);
        assert_eq!(order[3], ClauseGateStep::SessionCacheStore);
    }

    #[test]
    fn cache_key_includes_kind_and_desc() {
        let body = sp_bool();
        let k1 = clause_session_cache_key("C: ensures", &ClauseKind::Ensures, &body);
        let k2 = clause_session_cache_key("C: ensures", &ClauseKind::Invariant, &body);
        // Kind segment uses verify_labels stable labels (not Debug `Ensures`/`Invariant`).
        assert!(k1.contains("ensures"));
        assert!(k2.contains("invariant"));
        assert!(!k1.contains("Ensures"));
        assert_ne!(k1, k2);
        assert!(k1.starts_with("C: ensures::"));
    }

    #[test]
    fn unmodelable_result_is_known_limitation() {
        let r = unmodelable_clause_result("C: ensures", &["ghost".into()]);
        match &r {
            VerificationResult::Unknown { reason, .. } => {
                assert!(is_known_smt_limitation(reason));
                assert!(reason.contains("ghost"));
            }
            _ => panic!("expected Unknown"),
        }
        assert!(unmodelable_precheck_if("d", false, &[]).is_none());
        assert!(unmodelable_precheck_if("d", true, &[]).is_some());
    }

    #[test]
    fn cache_tag_round_trip() {
        let v = VerificationResult::verified("x");
        assert_eq!(ClauseCacheTag::from_result(&v).as_str(), "verified");
        assert_eq!(ClauseCacheTag::parse("timeout"), ClauseCacheTag::Timeout);
        let t = result_from_session_cache_tag("d", ClauseCacheTag::Timeout);
        assert!(matches!(t, VerificationResult::Timeout { .. }));
    }

    #[test]
    fn session_cache_store_and_lookup_verified() {
        let mut cache = SessionCache::new();
        let body = sp_bool();
        let key = clause_session_cache_key("Job: ensures", &ClauseKind::Ensures, &body);
        let ok = VerificationResult::verified("Job: ensures");
        store_clause_session_cache(&mut cache, key.clone(), &ok);
        let hit = lookup_clause_session_cache(&mut cache, &key, "Job: ensures");
        assert!(matches!(hit, Some(VerificationResult::Verified { .. })));
    }

    #[test]
    fn encode_failure_is_not_limitation_marker() {
        let r = clause_encode_failure("C: ensures", "CVC5 terms");
        match r {
            VerificationResult::Unknown { reason, .. } => {
                assert!(!is_known_smt_limitation(&reason));
                assert!(reason.contains("CVC5 terms"));
            }
            _ => panic!("expected Unknown"),
        }
        // not_encoded_reason still canonical for limitation-style details
        assert!(is_known_smt_limitation(&not_encoded_reason("x")));
    }
}
