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
    /// `0 <= name <= 1` (Bool parameters / result as 0/1 Int encoding).
    /// Without this, free Int models assign Bool vars values like 2 and break
    /// match/ITE encodings that only distinguish 0 vs non-zero or 0 vs 1.
    BoolZeroOrOne(String),
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

/// True if type tokens are exactly `Bool`.
#[inline]
pub(crate) fn is_bool_type_tokens(ty: &[String]) -> bool {
    ty.len() == 1 && ty[0].eq_ignore_ascii_case("Bool")
}

/// Return bit width and signedness for fixed-width type tokens.
///
/// Accepts language names (`U8`, `I32`, …) and lowercase aliases (`u8`, `i32`).
/// Shared between Z3 and CVC5 backends (parity: #453, #851).
pub(crate) fn fixed_width_bits(ty: &[String]) -> Option<(u32, bool)> {
    if ty.len() != 1 {
        return None;
    }
    // Case-insensitive: surface language uses U8/I32; some paths use u8/i32.
    match ty[0].as_str() {
        "u8" | "U8" => Some((8, false)),
        "u16" | "U16" => Some((16, false)),
        "u32" | "U32" => Some((32, false)),
        "u64" | "U64" => Some((64, false)),
        "i8" | "I8" => Some((8, true)),
        "i16" | "I16" => Some((16, true)),
        "i32" | "I32" => Some((32, true)),
        "i64" | "I64" => Some((64, true)),
        _ => None,
    }
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
        if is_bool_type_tokens(&pt) {
            out.push(PreludeConstraint::BoolZeroOrOne(param.name.clone()));
        }
    }

    if is_nat_type_tokens(return_ty) {
        // Z3 always asserts both; CVC5 filters if the var was not collected in the script/map.
        out.push(PreludeConstraint::NatNonNegative("result".into()));
        out.push(PreludeConstraint::NatNonNegative(
            crate::encode_atom_policy::RESULT_VAR_NAME.into(),
        ));
    }
    if is_bool_type_tokens(return_ty) {
        out.push(PreludeConstraint::BoolZeroOrOne("result".into()));
        out.push(PreludeConstraint::BoolZeroOrOne(
            crate::encode_atom_policy::RESULT_VAR_NAME.into(),
        ));
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
                let key = if name == "result" || name == crate::encode_atom_policy::RESULT_VAR_NAME
                {
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
            PreludeConstraint::BoolZeroOrOne(name) => {
                let key = if name == "result" || name == crate::encode_atom_policy::RESULT_VAR_NAME
                {
                    name.clone()
                } else {
                    sanitize(name)
                };
                let key_sanitized = sanitize(name);
                if vars.contains(name) || vars.contains(&key) || vars.contains(&key_sanitized) {
                    Some(PreludeConstraint::BoolZeroOrOne(if vars.contains(name) {
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

    fn param_bool(name: &str) -> Param {
        Param {
            name: name.into(),
            ty: Some(TypeExpr::named("Bool")),
        }
    }

    #[test]
    fn collect_prelude_includes_bool_zero_or_one() {
        let params = vec![param_bool("flag")];
        let cs = collect_prelude_constraints(&params, &["Bool".into()], &[], &[]);
        assert!(cs.contains(&PreludeConstraint::BoolZeroOrOne("flag".into())));
        assert!(cs.contains(&PreludeConstraint::BoolZeroOrOne("result".into())));
        assert!(cs.contains(&PreludeConstraint::BoolZeroOrOne(
            crate::encode_atom_policy::RESULT_VAR_NAME.into()
        )));
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
        assert!(cs.contains(&PreludeConstraint::NatNonNegative(
            crate::encode_atom_policy::RESULT_VAR_NAME.into()
        )));
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

    #[test]
    fn fixed_width_bits_accepts_language_and_lowercase_names() {
        // #851: surface language uses U8/I32; shared policy must match both.
        assert_eq!(fixed_width_bits(&["U8".into()]), Some((8, false)));
        assert_eq!(fixed_width_bits(&["u8".into()]), Some((8, false)));
        assert_eq!(fixed_width_bits(&["I32".into()]), Some((32, true)));
        assert_eq!(fixed_width_bits(&["i32".into()]), Some((32, true)));
        assert_eq!(fixed_width_bits(&["U64".into()]), Some((64, false)));
        assert_eq!(fixed_width_bits(&["I8".into()]), Some((8, true)));
        assert_eq!(fixed_width_bits(&["Int".into()]), None);
        assert_eq!(fixed_width_bits(&["Nat".into()]), None);
        assert_eq!(fixed_width_bits(&["u8".into(), "extra".into()]), None);
    }
}
