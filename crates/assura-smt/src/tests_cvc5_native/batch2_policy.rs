use super::*;

// -------------------------------------------------------------------
// Batch 2: Policy alignment tests (#452, #455, #456, #461, #465, #466, #467)
// -------------------------------------------------------------------

#[cfg(feature = "cvc5-verify")]
mod batch2_policy_tests {
    use super::*;

    /// #452/#465: Timeout constants unified between native and shell to 10s.
    #[test]
    fn timeout_policy_shared_between_native_and_shell() {
        use crate::encode_timeout_policy::{
            DEFAULT_SOLVER_TIMEOUT_MS, DEFAULT_SOLVER_TIMEOUT_TLIMIT,
        };
        assert_eq!(DEFAULT_SOLVER_TIMEOUT_MS, 10_000);
        assert_eq!(DEFAULT_SOLVER_TIMEOUT_TLIMIT, "10000");
    }

    /// #461: use_incremental_clause_push_pop shared policy is used.
    #[test]
    fn incremental_policy_boundary() {
        assert!(!crate::prelude_policy::use_incremental_clause_push_pop(1));
        assert!(crate::prelude_policy::use_incremental_clause_push_pop(2));
    }

    /// #467: ConstantEq is skipped in solver prelude (no double-assertion).
    #[test]
    fn constant_eq_skipped_in_prelude() {
        use crate::cvc5_verify_shared::collect_cvc5_type_constraints;
        let mut vars = std::collections::HashSet::new();
        vars.insert("MAX".into());
        let constraints =
            collect_cvc5_type_constraints(&vars, &[], &[], &[("MAX".into(), 100)], &[]);
        // Should contain ConstantEq but new_cvc5_solver_prelude skips it.
        // Verify the constraint is produced (the policy skip is in the
        // solver prelude, not the constraint collector).
        assert!(
            constraints
                .iter()
                .any(|c| matches!(c, crate::prelude_policy::PreludeConstraint::ConstantEq(..))),
            "ConstantEq should appear in constraints (skipped at solver level): {constraints:?}"
        );
    }

    /// #455: Per-clause contract path now extracts unsat cores when
    /// multiple requires exist (tracked via check_sat_assuming).
    #[test]
    fn unsat_core_extracted_in_contract_path() {
        // Two requires + trivially-derivable ensures: the unsat core
        // should be non-empty because both requires contribute.
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Lt,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("100".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("UnsatCoreTest", &clauses);
        assert_eq!(results.len(), 1, "expected 1 ensures result");
        match &results[0] {
            VerificationResult::Verified { unsat_core, .. } => {
                assert!(
                    unsat_core.is_some(),
                    "unsat core should be present with 2 requires"
                );
                let core = unsat_core.as_ref().unwrap();
                assert!(!core.is_empty(), "unsat core should be non-empty");
            }
            other => panic!("expected Verified, got: {other:?}"),
        }
    }

    /// #455: Single requires should NOT use unsat core tracking
    /// (matches prelude_policy::track_requires_unsat_cores).
    #[test]
    fn single_requires_no_unsat_core() {
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("SingleReqNoCore", &clauses);
        assert_eq!(results.len(), 1);
        match &results[0] {
            VerificationResult::Verified { unsat_core, .. } => {
                assert!(
                    unsat_core.is_none(),
                    "single requires should not produce unsat core"
                );
            }
            other => panic!("expected Verified, got: {other:?}"),
        }
    }

    /// #456: cvc5_clause_sat_outcome correctly classifies outcomes.
    #[test]
    fn clause_sat_outcome_maps_correctly() {
        use crate::cvc5_verify_native_solver::cvc5_clause_sat_outcome;
        use crate::solver_outcome_policy::ClauseSatOutcome;

        // Create a simple solver and verify UNSAT outcome (validity check).
        let tm = cvc5::TermManager::new();
        let mut solver = cvc5::Solver::new(&tm);
        solver.set_logic("ALL");
        solver.set_option("produce-models", "true");

        // assert false => UNSAT
        let f = tm.mk_false();
        solver.assert_formula(f);
        let sat_result = solver.check_sat();
        assert!(sat_result.is_unsat());

        let outcome =
            cvc5_clause_sat_outcome(&sat_result, &solver, &std::collections::HashMap::new(), &[]);
        assert!(
            matches!(outcome, ClauseSatOutcome::Unsat { .. }),
            "false assertion should give Unsat outcome: {outcome:?}"
        );
    }
}

// ---------------------------------------------------------------
// Batch 3: #457 (var caching), #464 (base name extraction)
// ---------------------------------------------------------------

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_raw_var_caching() {
    // Fix #457: CVC5 raw encoder should cache newly created variables.
    // Two references to the same name must return the same Term.
    use crate::cvc5_backend::cvc5_raw_native::encode_raw_tokens_cvc5;
    use std::collections::HashMap;

    let tm = cvc5::TermManager::new();
    let mut vars: HashMap<String, cvc5::Term> = HashMap::new();
    let mut state = crate::cvc5_encoder_state::default_cvc5_encoder_state();

    // First reference to "x" should create and cache it.
    let tokens_1 = vec!["x".to_string()];
    let _v1 = encode_raw_tokens_cvc5(&tm, &tokens_1, &mut vars, &mut state);
    assert!(vars.contains_key("x"), "first reference should cache 'x'");

    // Second reference should hit the cache (map already has "x").
    let before_len = vars.len();
    let _v2 = encode_raw_tokens_cvc5(&tm, &tokens_1, &mut vars, &mut state);
    assert_eq!(
        vars.len(),
        before_len,
        "second reference should not add new entries"
    );
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_raw_result_var_caching() {
    // Fix #457: `result` keyword should also cache in the var map.
    use crate::cvc5_backend::cvc5_raw_native::encode_raw_tokens_cvc5;
    use std::collections::HashMap;

    let tm = cvc5::TermManager::new();
    let mut vars: HashMap<String, cvc5::Term> = HashMap::new();
    let mut state = crate::cvc5_encoder_state::default_cvc5_encoder_state();

    let tokens = vec!["result".to_string()];
    let _v = encode_raw_tokens_cvc5(&tm, &tokens, &mut vars, &mut state);
    assert!(
        vars.contains_key(crate::encode_atom_policy::RESULT_VAR_NAME),
        "result keyword should cache under RESULT_VAR_NAME"
    );
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_raw_base_name_extraction() {
    // Fix #464: base name extraction uses shared policy function.
    use crate::encode_atom_policy::extract_raw_base_name;
    // After dotted segments are collapsed with _, rsplit('_') extracts the base.
    assert_eq!(extract_raw_base_name("obj_length"), "length");
    assert_eq!(extract_raw_base_name("a_b_min"), "min");
}
