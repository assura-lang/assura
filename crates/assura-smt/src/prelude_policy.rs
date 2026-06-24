//! Shared contract **prelude** constraints and verify-step order (one compiler brain).
//!
//! Owns which type/constant/narrowing facts must be asserted before clause checks,
//! and documents the canonical step order for Z3 / CVC5-native / CVC5-SMT-LIB.
//! Backends only implement how each constraint becomes a solver term or SMT-LIB line.
//!
//! Complements [`crate::clause_policy`] (clause partition/polarity) and
//! [`crate::havoc_assume`] (havoc+assume order). Does **not** unify expression
//! encoding (`Encoder` vs `encode_expr_cvc5`).

use assura_ast::Param;

/// Solver-neutral prelude constraint (Nat bounds, named constants, feature_max caps).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreludeConstraint {
    /// `name >= 0` (Nat parameters / result slots).
    NatNonNegative(String),
    /// `name = value` (feature_max / named constants as concrete ints).
    ConstantEq(String, i64),
    /// `name <= bound` (refinement narrowing from feature_max on other names).
    NarrowingLe(String, i64),
}

/// Canonical verify-step order (documentation + tests; backends execute via their own APIs).
///
/// Divergence here is a correctness bug class (wrong assumptions, missing frame, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerifyPreludeStep {
    /// `clause_policy::prepare_contract_clauses` (feature Other + partition + frame).
    PrepareClauses,
    /// Encoder/solver setup (logic, timeout, BV infra, constants as symbols).
    SolverAndEncoderInit,
    /// Trigger name seeding from all contract clauses.
    SeedTriggers,
    /// Havoc+assume (structural + IR via `apply_havoc_assume_policy`).
    HavocAssume,
    /// Assert all `requires` once (incremental base; unsat-core labels when multi).
    AssertRequires,
    /// Nat / constant / narrowing prelude constraints.
    TypeAndConstantPrelude,
    /// Lemma ensures for `apply` targets in contract clauses.
    LemmaAssumptions,
    /// Per verifiable clause: unmodelable gate → cache → frame → encode → polarity → check-sat.
    PerClauseChecks,
}

/// Full ordered step list for contract verification (Z3 incremental and CVC5 contract paths).
pub(crate) const VERIFY_PRELUDE_ORDER: &[VerifyPreludeStep] = &[
    VerifyPreludeStep::PrepareClauses,
    VerifyPreludeStep::SolverAndEncoderInit,
    VerifyPreludeStep::SeedTriggers,
    VerifyPreludeStep::HavocAssume,
    VerifyPreludeStep::AssertRequires,
    VerifyPreludeStep::TypeAndConstantPrelude,
    VerifyPreludeStep::LemmaAssumptions,
    VerifyPreludeStep::PerClauseChecks,
];

/// Accessor so non-test builds retain the order table (guards against accidental deletion).
#[inline]
pub(crate) fn verify_prelude_order() -> &'static [VerifyPreludeStep] {
    VERIFY_PRELUDE_ORDER
}

/// True if type tokens are exactly `Nat`.
#[inline]
pub(crate) fn is_nat_type_tokens(ty: &[String]) -> bool {
    ty.len() == 1 && ty[0] == "Nat"
}

/// Param type tokens via shared entry helper (single source of truth).
pub(crate) fn param_type_tokens(param: &Param) -> Vec<String> {
    crate::entry::type_expr_to_token_vec(param.ty.as_ref())
}

/// Collect prelude constraints for a contract (Z3 asserts all; CVC5 may filter by declared vars).
///
/// Order within the vector is stable: Nat params, then Nat `result`/`__result`, then
/// constants, then narrowings (matches prior Z3 / CVC5 behavior).
pub(crate) fn collect_prelude_constraints(
    params: &[Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    narrowings: &[(String, i64)],
) -> Vec<PreludeConstraint> {
    let mut out = Vec::new();

    for param in params {
        let pt = param_type_tokens(param);
        if is_nat_type_tokens(&pt) {
            out.push(PreludeConstraint::NatNonNegative(param.name.clone()));
        }
    }

    if is_nat_type_tokens(return_ty) {
        // Z3 always asserts both; CVC5 filters if the var was not collected in the script/map.
        out.push(PreludeConstraint::NatNonNegative("result".into()));
        out.push(PreludeConstraint::NatNonNegative("__result".into()));
    }

    for (name, value) in constants {
        out.push(PreludeConstraint::ConstantEq(name.clone(), *value));
    }

    for (name, bound) in narrowings {
        out.push(PreludeConstraint::NarrowingLe(name.clone(), *bound));
    }

    out
}

/// Filter prelude constraints to names present in `vars` (CVC5/SMT-LIB declare-only vars).
///
/// Constant and narrowing names are sanitized with `sanitize` when provided so they match
/// SMT-LIB identifier rules used in CVC5 backends.
pub(crate) fn filter_prelude_constraints_by_vars(
    constraints: &[PreludeConstraint],
    vars: &std::collections::HashSet<String>,
    sanitize: impl Fn(&str) -> String,
) -> Vec<PreludeConstraint> {
    constraints
        .iter()
        .filter_map(|c| match c {
            PreludeConstraint::NatNonNegative(name) => {
                let key = if name == "result" || name == "__result" {
                    name.clone()
                } else {
                    sanitize(name)
                };
                // Nat params: Z3 uses raw param names; CVC5 uses sanitized keys in var_map.
                let key_sanitized = sanitize(name);
                if vars.contains(name) || vars.contains(&key) || vars.contains(&key_sanitized) {
                    Some(PreludeConstraint::NatNonNegative(if vars.contains(name) {
                        name.clone()
                    } else if vars.contains(&key) {
                        key
                    } else {
                        key_sanitized
                    }))
                } else {
                    None
                }
            }
            PreludeConstraint::ConstantEq(name, value) => {
                let key = sanitize(name);
                if vars.contains(&key) || vars.contains(name) {
                    Some(PreludeConstraint::ConstantEq(
                        if vars.contains(&key) {
                            key
                        } else {
                            name.clone()
                        },
                        *value,
                    ))
                } else {
                    None
                }
            }
            PreludeConstraint::NarrowingLe(name, bound) => {
                let key = sanitize(name);
                if vars.contains(&key) || vars.contains(name) {
                    Some(PreludeConstraint::NarrowingLe(
                        if vars.contains(&key) {
                            key
                        } else {
                            name.clone()
                        },
                        *bound,
                    ))
                } else {
                    None
                }
            }
        })
        .collect()
}

/// CVC5/SMT-LIB path: collect prelude constraints filtered to declared vars (sanitized names).
pub(crate) fn collect_prelude_constraints_for_vars(
    vars: &std::collections::HashSet<String>,
    params: &[Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    narrowings: &[(String, i64)],
    sanitize: impl Fn(&str) -> String,
) -> Vec<PreludeConstraint> {
    let all = collect_prelude_constraints(params, return_ty, constants, narrowings);
    filter_prelude_constraints_by_vars(&all, vars, sanitize)
}

/// Whether unsat-core tracking labels should be used for requires (multi-require only).
#[inline]
pub(crate) fn track_requires_unsat_cores(requires_count: usize) -> bool {
    requires_count > 1
}

/// Whether incremental push/pop should be used per clause (multi-verifiable only).
#[inline]
pub(crate) fn use_incremental_clause_push_pop(verifiable_count: usize) -> bool {
    verifiable_count > 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::TypeExpr;
    use std::collections::HashSet;

    fn param_nat(name: &str) -> Param {
        Param {
            name: name.into(),
            ty: Some(TypeExpr::named("Nat")),
        }
    }

    #[test]
    fn verify_prelude_order_is_complete_and_starts_with_prepare() {
        assert_eq!(
            VERIFY_PRELUDE_ORDER.first().copied(),
            Some(VerifyPreludeStep::PrepareClauses)
        );
        assert_eq!(
            VERIFY_PRELUDE_ORDER.last().copied(),
            Some(VerifyPreludeStep::PerClauseChecks)
        );
        assert_eq!(VERIFY_PRELUDE_ORDER.len(), 8);
        // Havoc before requires before type prelude before lemmas (assumption order).
        let pos = |s| {
            VERIFY_PRELUDE_ORDER
                .iter()
                .position(|x| *x == s)
                .expect("step present")
        };
        assert!(pos(VerifyPreludeStep::HavocAssume) < pos(VerifyPreludeStep::AssertRequires));
        assert!(
            pos(VerifyPreludeStep::AssertRequires) < pos(VerifyPreludeStep::TypeAndConstantPrelude)
        );
        assert!(
            pos(VerifyPreludeStep::TypeAndConstantPrelude)
                < pos(VerifyPreludeStep::LemmaAssumptions)
        );
        assert!(pos(VerifyPreludeStep::LemmaAssumptions) < pos(VerifyPreludeStep::PerClauseChecks));
    }

    #[test]
    fn collect_prelude_includes_nat_param_result_constants_narrowings() {
        let params = vec![param_nat("n")];
        let constants = vec![("MAX".into(), 10i64)];
        let narrowings = vec![("x".into(), 5i64)];
        let cs = collect_prelude_constraints(&params, &["Nat".into()], &constants, &narrowings);
        assert!(cs.contains(&PreludeConstraint::NatNonNegative("n".into())));
        assert!(cs.contains(&PreludeConstraint::NatNonNegative("result".into())));
        assert!(cs.contains(&PreludeConstraint::NatNonNegative("__result".into())));
        assert!(cs.contains(&PreludeConstraint::ConstantEq("MAX".into(), 10)));
        assert!(cs.contains(&PreludeConstraint::NarrowingLe("x".into(), 5)));
    }

    #[test]
    fn filter_by_vars_drops_undeclared_names() {
        let params = vec![param_nat("n")];
        let all = collect_prelude_constraints(&params, &[], &[], &[]);
        let mut vars = HashSet::new();
        vars.insert("n".into());
        let filtered = filter_prelude_constraints_by_vars(&all, &vars, |s| s.to_string());
        assert_eq!(
            filtered,
            vec![PreludeConstraint::NatNonNegative("n".into())]
        );
    }

    #[test]
    fn incremental_flags_match_prior_z3_heuristics() {
        assert!(!track_requires_unsat_cores(0));
        assert!(!track_requires_unsat_cores(1));
        assert!(track_requires_unsat_cores(2));
        assert!(!use_incremental_clause_push_pop(1));
        assert!(use_incremental_clause_push_pop(2));
    }
}
