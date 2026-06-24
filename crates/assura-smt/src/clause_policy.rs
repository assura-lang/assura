//! Shared clause/contract verification **policy** (one compiler brain).
//!
//! Owns which clauses are verifiable, feature-clause dispatch, frame checker
//! setup, and per-kind check polarity (validity vs satisfiability). Solver
//! backends only implement term construction and `check-sat`.
//!
//! This is **not** full expression-encode unification (Z3 `Encoder` vs CVC5
//! `encode_expr_cvc5` remain separate). It unifies the *orchestration* that
//! was previously triplicated in Z3 `verify_clauses_with_types` and CVC5
//! `prepare_cvc5_contract_verification`.

use assura_ast::{Clause, ClauseKind, Param, SpExpr};

use crate::VerificationResult;
use crate::feature_max::derive_narrowings;

/// How to assert a clause body before `check-sat` (solver-neutral).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClauseCheckPolarity {
    /// Assert `NOT body`; UNSAT means body is valid (ensures, rule, decreases).
    ValidityNegateBody,
    /// Assert `body`; SAT means satisfiable (invariant).
    SatisfiabilityAssertBody,
    /// Assert `body`; UNSAT means body is impossible (must_not).
    ValidityAssertBody,
    /// Assert `NOT (measure >= 0)`; UNSAT means measure is always non-negative.
    DecreasesNonNeg,
}

/// Solver-neutral prepared contract clause state (Z3 and CVC5).
#[derive(Debug)]
pub(crate) struct ContractClausePrep<'a> {
    pub narrowings: Vec<(String, i64)>,
    pub requires_exprs: Vec<&'a SpExpr>,
    pub frame_checker: assura_types::FrameChecker,
    pub verifiable: Vec<&'a Clause>,
    pub requires_clauses: Vec<&'a Clause>,
    pub ensures_clauses: Vec<&'a Clause>,
    pub param_names: Vec<String>,
}

/// Clause kinds that get per-clause SMT validity/sat checks (not feature Other).
#[inline]
pub(crate) fn is_verifiable_clause_kind(kind: &ClauseKind) -> bool {
    matches!(
        kind,
        ClauseKind::Ensures
            | ClauseKind::Invariant
            | ClauseKind::Rule
            | ClauseKind::MustNot
            | ClauseKind::Decreases
    )
}

/// Filter contract clauses into the verifiable set (shared Z3/CVC5 order).
pub(crate) fn collect_verifiable_clauses(clauses: &[Clause]) -> Vec<&Clause> {
    clauses
        .iter()
        .filter(|c| is_verifiable_clause_kind(&c.kind))
        .collect()
}

pub(crate) fn collect_requires_clauses(clauses: &[Clause]) -> Vec<&Clause> {
    clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .collect()
}

pub(crate) fn collect_ensures_clauses(clauses: &[Clause]) -> Vec<&Clause> {
    clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .collect()
}

pub(crate) fn collect_requires_exprs(clauses: &[Clause]) -> Vec<&SpExpr> {
    clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect()
}

/// Build frame checker from `modifies` clause bodies (Tier A3).
pub(crate) fn build_frame_checker(clauses: &[Clause]) -> assura_types::FrameChecker {
    let modifies_bodies: Vec<&SpExpr> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Modifies)
        .map(|c| &c.body)
        .collect();
    if modifies_bodies.is_empty() {
        assura_types::FrameChecker::empty()
    } else {
        assura_types::FrameChecker::new(&modifies_bodies)
    }
}

/// Dispatch `ClauseKind::Other` feature clauses via `smt_features` (shared brain).
pub(crate) fn dispatch_feature_clauses(
    contract_name: &str,
    clauses: &[Clause],
) -> Vec<VerificationResult> {
    let mut results = Vec::new();
    for clause in clauses {
        if let ClauseKind::Other(kind) = &clause.kind {
            results.extend(crate::smt_features::verify_feature_clause(
                kind,
                contract_name,
                &clause.body,
                clauses,
            ));
        }
    }
    results
}

/// One brain for contract clause partitioning + feature dispatch + frame setup.
///
/// Returns early feature results plus prepared state for solver backends.
pub(crate) fn prepare_contract_clauses<'a>(
    contract_name: &str,
    clauses: &'a [Clause],
    params: &[Param],
    constants: &[(String, i64)],
) -> (Vec<VerificationResult>, ContractClausePrep<'a>) {
    let feature_results = dispatch_feature_clauses(contract_name, clauses);
    let narrowings = derive_narrowings(constants);
    let requires_exprs = collect_requires_exprs(clauses);
    let frame_checker = build_frame_checker(clauses);
    let verifiable = collect_verifiable_clauses(clauses);
    let requires_clauses = collect_requires_clauses(clauses);
    let ensures_clauses = collect_ensures_clauses(clauses);
    let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();

    (
        feature_results,
        ContractClausePrep {
            narrowings,
            requires_exprs,
            frame_checker,
            verifiable,
            requires_clauses,
            ensures_clauses,
            param_names,
        },
    )
}

/// Per-kind check polarity (must stay identical across Z3 / CVC5 / SMT-LIB).
pub(crate) fn clause_check_polarity(kind: &ClauseKind) -> Option<ClauseCheckPolarity> {
    match kind {
        ClauseKind::Ensures | ClauseKind::Rule => Some(ClauseCheckPolarity::ValidityNegateBody),
        ClauseKind::Invariant => Some(ClauseCheckPolarity::SatisfiabilityAssertBody),
        ClauseKind::MustNot => Some(ClauseCheckPolarity::ValidityAssertBody),
        ClauseKind::Decreases => Some(ClauseCheckPolarity::DecreasesNonNeg),
        _ => None,
    }
}

/// Whether frame axioms apply for this clause (ensures + non-empty modifies set).
pub(crate) fn wants_frame_axioms(
    kind: &ClauseKind,
    frame_checker: &assura_types::FrameChecker,
) -> bool {
    *kind == ClauseKind::Ensures && frame_checker.has_modifies()
}

/// Frame axiom variable names for an ensures clause (or empty if not applicable).
pub(crate) fn frame_axiom_vars_for_clause(
    frame_checker: &assura_types::FrameChecker,
    kind: &ClauseKind,
    body: &SpExpr,
    param_names: &[String],
) -> Vec<String> {
    if !wants_frame_axioms(kind, frame_checker) {
        return Vec::new();
    }
    frame_checker.frame_axiom_vars_with_candidates(body, param_names)
}

/// Whether CVC5/SMT-LIB assert path should negate the body (coarse; Decreases handled
/// in Z3 with measure extraction; CVC5 currently folds Decreases into negate-body).
///
/// CVC5 native/shell use a simplified polarity: Invariant|MustNot assert body,
/// everything else negate. Z3 handles Decreases separately via measure terms.
pub(crate) fn cvc5_assert_negates_body(kind: &ClauseKind) -> bool {
    !matches!(kind, ClauseKind::Invariant | ClauseKind::MustNot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::{Expr, Literal, Spanned};

    fn sp(e: Expr) -> SpExpr {
        Spanned::no_span(e)
    }

    fn clause(kind: ClauseKind, body: Expr) -> Clause {
        Clause {
            kind,
            body: sp(body),
            effect_variables: Vec::new(),
        }
    }

    #[test]
    fn verifiable_kinds_match_z3_cvc5_filter() {
        assert!(is_verifiable_clause_kind(&ClauseKind::Ensures));
        assert!(is_verifiable_clause_kind(&ClauseKind::Invariant));
        assert!(is_verifiable_clause_kind(&ClauseKind::Rule));
        assert!(is_verifiable_clause_kind(&ClauseKind::MustNot));
        assert!(is_verifiable_clause_kind(&ClauseKind::Decreases));
        assert!(!is_verifiable_clause_kind(&ClauseKind::Requires));
        assert!(!is_verifiable_clause_kind(&ClauseKind::Modifies));
        assert!(!is_verifiable_clause_kind(&ClauseKind::Other(
            "sec.1".into()
        )));
    }

    #[test]
    fn polarity_table_matches_documented_semantics() {
        assert_eq!(
            clause_check_polarity(&ClauseKind::Ensures),
            Some(ClauseCheckPolarity::ValidityNegateBody)
        );
        assert_eq!(
            clause_check_polarity(&ClauseKind::Rule),
            Some(ClauseCheckPolarity::ValidityNegateBody)
        );
        assert_eq!(
            clause_check_polarity(&ClauseKind::Invariant),
            Some(ClauseCheckPolarity::SatisfiabilityAssertBody)
        );
        assert_eq!(
            clause_check_polarity(&ClauseKind::MustNot),
            Some(ClauseCheckPolarity::ValidityAssertBody)
        );
        assert_eq!(
            clause_check_polarity(&ClauseKind::Decreases),
            Some(ClauseCheckPolarity::DecreasesNonNeg)
        );
        assert_eq!(clause_check_polarity(&ClauseKind::Requires), None);
    }

    #[test]
    fn cvc5_coarse_polarity_matches_native_shell_match() {
        assert!(!cvc5_assert_negates_body(&ClauseKind::Invariant));
        assert!(!cvc5_assert_negates_body(&ClauseKind::MustNot));
        assert!(cvc5_assert_negates_body(&ClauseKind::Ensures));
        assert!(cvc5_assert_negates_body(&ClauseKind::Rule));
        assert!(cvc5_assert_negates_body(&ClauseKind::Decreases));
    }

    #[test]
    fn prepare_partitions_requires_ensures_verifiable() {
        let clauses = vec![
            clause(ClauseKind::Requires, Expr::Literal(Literal::Bool(true))),
            clause(ClauseKind::Ensures, Expr::Literal(Literal::Bool(true))),
            clause(ClauseKind::Modifies, Expr::Ident("x".into())),
        ];
        let (_features, prep) = prepare_contract_clauses("C", &clauses, &[], &[]);
        assert_eq!(prep.requires_clauses.len(), 1);
        assert_eq!(prep.ensures_clauses.len(), 1);
        assert_eq!(prep.verifiable.len(), 1);
        assert!(prep.frame_checker.has_modifies());
        assert!(wants_frame_axioms(
            &ClauseKind::Ensures,
            &prep.frame_checker
        ));
        assert!(!wants_frame_axioms(
            &ClauseKind::Invariant,
            &prep.frame_checker
        ));
    }

    #[test]
    fn feature_other_dispatches_without_entering_verifiable() {
        let clauses = vec![clause(
            ClauseKind::Other("fmt.1".into()),
            Expr::Literal(Literal::Bool(true)),
        )];
        let (features, prep) = prepare_contract_clauses("C", &clauses, &[], &[]);
        assert!(prep.verifiable.is_empty());
        // Feature may return Unknown/Verified depending on feature; must not panic.
        let _ = features.len();
    }
}
