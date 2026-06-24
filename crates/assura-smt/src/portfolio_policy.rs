//! Shared multi-solver **portfolio** merge policy (one compiler brain).
//!
//! Owns how Z3 and CVC5 results are combined per clause (definitive vs
//! inconclusive priority, Z3 preference on ties). The parallel/threaded
//! runner stays in [`crate::entry`]; this module is pure result selection
//! so shell/native/Z3/CVC5 cannot diverge on which outcome wins.
//!
//! Complements [`crate::solver_outcome_policy`] (single-solver interpretation).

use crate::VerificationResult;

/// Priority for portfolio merge: higher wins. Ties prefer the first argument (Z3).
#[inline]
pub(crate) fn portfolio_result_priority(r: &VerificationResult) -> u8 {
    match r {
        VerificationResult::Verified { .. } => 3,
        VerificationResult::Counterexample { .. } => 2,
        VerificationResult::Unknown { .. } => 1,
        VerificationResult::Timeout { .. } => 0,
    }
}

/// Pick the better of two results for the same clause position.
///
/// Priority: Verified > Counterexample > Unknown > Timeout.
/// On equal priority, prefer `primary` (Z3: richer counter-models / unsat cores).
pub(crate) fn pick_better_portfolio_result(
    primary: VerificationResult,
    secondary: VerificationResult,
) -> VerificationResult {
    let p_pri = portfolio_result_priority(&primary);
    let s_pri = portfolio_result_priority(&secondary);
    if p_pri >= s_pri { primary } else { secondary }
}

/// Merge Z3 and CVC5 result vectors by position (zip, then append extras).
///
/// Extra CVC5-only tail clauses are kept (CVC5 reported more clauses than Z3).
pub(crate) fn merge_portfolio_results(
    primary: Vec<VerificationResult>,
    secondary: Vec<VerificationResult>,
) -> Vec<VerificationResult> {
    let mut merged = Vec::with_capacity(primary.len().max(secondary.len()));
    let mut sec_iter = secondary.into_iter();
    for p in primary {
        if let Some(s) = sec_iter.next() {
            merged.push(pick_better_portfolio_result(p, s));
        } else {
            merged.push(p);
        }
    }
    merged.extend(sec_iter);
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verified(d: &str) -> VerificationResult {
        VerificationResult::verified(d)
    }
    fn counterexample(d: &str) -> VerificationResult {
        VerificationResult::Counterexample {
            clause_desc: d.into(),
            model: "m".into(),
            counter_model: None,
        }
    }
    fn timeout(d: &str) -> VerificationResult {
        VerificationResult::Timeout {
            clause_desc: d.into(),
        }
    }
    fn unknown(d: &str) -> VerificationResult {
        VerificationResult::Unknown {
            clause_desc: d.into(),
            reason: "r".into(),
        }
    }

    #[test]
    fn priority_order() {
        assert!(
            portfolio_result_priority(&verified("a"))
                > portfolio_result_priority(&counterexample("a"))
        );
        assert!(
            portfolio_result_priority(&counterexample("a"))
                > portfolio_result_priority(&unknown("a"))
        );
        assert!(
            portfolio_result_priority(&unknown("a")) > portfolio_result_priority(&timeout("a"))
        );
    }

    #[test]
    fn prefer_verified_over_timeout() {
        let r = pick_better_portfolio_result(timeout("c"), verified("c"));
        assert!(matches!(r, VerificationResult::Verified { .. }));
    }

    #[test]
    fn tie_prefers_primary_z3() {
        let r = pick_better_portfolio_result(counterexample("c"), counterexample("c"));
        match r {
            VerificationResult::Counterexample { model, .. } => assert_eq!(model, "m"),
            other => panic!("expected CE, got {other:?}"),
        }
    }

    #[test]
    fn merge_zips_and_appends_secondary_tail() {
        let primary = vec![timeout("a"), verified("b")];
        let secondary = vec![verified("a"), timeout("b"), counterexample("c")];
        let m = merge_portfolio_results(primary, secondary);
        assert_eq!(m.len(), 3);
        assert!(matches!(&m[0], VerificationResult::Verified { .. }));
        assert!(matches!(&m[1], VerificationResult::Verified { .. }));
        assert!(matches!(&m[2], VerificationResult::Counterexample { .. }));
    }
}
