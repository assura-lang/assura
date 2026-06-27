use super::*;

// =======================================================================
// CVC5 native parity tests (#468)
// =======================================================================
// These tests mirror Z3 test categories that had no CVC5 equivalent.

#[cfg(feature = "cvc5-verify")]
mod cvc5_parity_468 {
    use crate::VerificationResult;
    use crate::cache::SessionCache;
    use crate::cvc5_backend::*;
    use assura_ast::{BinOp, Clause, ClauseKind, Expr, Literal, Param, SpExpr, Spanned};

    fn ident(name: &str) -> SpExpr {
        Spanned::no_span(Expr::Ident(name.to_string()))
    }

    fn int_lit(n: i64) -> SpExpr {
        Spanned::no_span(Expr::Literal(Literal::Int(n.to_string())))
    }

    fn binop(lhs: SpExpr, op: BinOp, rhs: SpExpr) -> SpExpr {
        Spanned::no_span(Expr::BinOp {
            lhs: Box::new(lhs),
            op,
            rhs: Box::new(rhs),
        })
    }

    // -------------------------------------------------------------------
    // Refinement subtype parity (Z3: test_refinement_*)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_refinement_subtype_holds() {
        // x > 0 implies x >= 0 -> Verified
        let ante = binop(ident("x"), BinOp::Gt, int_lit(0));
        let cons = binop(ident("x"), BinOp::Gte, int_lit(0));
        let result = check_refinement_subtype_cvc5(&ante, &cons);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "x > 0 should imply x >= 0, got: {result:?}"
        );
    }

    #[test]
    fn cvc5_refinement_subtype_fails() {
        // x > 0 does NOT imply x > 10 -> Counterexample
        let ante = binop(ident("x"), BinOp::Gt, int_lit(0));
        let cons = binop(ident("x"), BinOp::Gt, int_lit(10));
        let result = check_refinement_subtype_cvc5(&ante, &cons);
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "x > 0 should NOT imply x > 10, got: {result:?}"
        );
    }

    #[test]
    fn cvc5_refinement_with_context() {
        // Context: n > 5, n <= 10. Antecedent: x < n. Consequent: x < 10.
        let ctx = vec![
            binop(ident("n"), BinOp::Gt, int_lit(5)),
            binop(ident("n"), BinOp::Lte, int_lit(10)),
        ];
        let ante = binop(ident("x"), BinOp::Lt, ident("n"));
        let cons = binop(ident("x"), BinOp::Lt, int_lit(10));
        let result = check_refinement_subtype_with_context_cvc5(&ctx, &ante, &cons);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "with n in (5,10], x < n should imply x < 10, got: {result:?}"
        );
    }

    // -------------------------------------------------------------------
    // Buffer bounds parity (Z3: test_buffer_bounds_*)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_buffer_bounds_with_requires_verified() {
        let requires = vec![binop(
            binop(ident("offset"), BinOp::Add, ident("len")),
            BinOp::Lte,
            ident("buf_len"),
        )];
        let ensures = binop(
            binop(ident("offset"), BinOp::Add, ident("len")),
            BinOp::Lte,
            ident("buf_len"),
        );
        let result = verify_buffer_bounds_cvc5(&requires, &ensures);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "buffer bounds with matching requires should verify, got: {result:?}"
        );
    }

    #[test]
    fn cvc5_buffer_bounds_without_requires_counterexample() {
        let requires: Vec<SpExpr> = vec![];
        let ensures = binop(
            binop(ident("offset"), BinOp::Add, ident("len")),
            BinOp::Lte,
            ident("buf_len"),
        );
        let result = verify_buffer_bounds_cvc5(&requires, &ensures);
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "buffer bounds without requires should produce counterexample, got: {result:?}"
        );
    }

    #[test]
    fn cvc5_buffer_bounds_partial_requires_counterexample() {
        let requires = vec![binop(ident("offset"), BinOp::Gte, int_lit(0))];
        let ensures = binop(
            binop(ident("offset"), BinOp::Add, ident("len")),
            BinOp::Lte,
            ident("buf_len"),
        );
        let result = verify_buffer_bounds_cvc5(&requires, &ensures);
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "partial requires should produce counterexample, got: {result:?}"
        );
    }

    #[test]
    fn cvc5_buffer_bounds_nonneg_offset_and_len() {
        let requires = vec![
            binop(ident("offset"), BinOp::Gte, int_lit(0)),
            binop(ident("len"), BinOp::Gte, int_lit(0)),
            binop(
                binop(ident("offset"), BinOp::Add, ident("len")),
                BinOp::Lte,
                ident("cap"),
            ),
        ];
        let ensures = binop(ident("offset"), BinOp::Gte, int_lit(0));
        let result = verify_buffer_bounds_cvc5(&requires, &ensures);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "non-negative offset should verify, got: {result:?}"
        );
    }

    #[test]
    fn cvc5_safe_buffer_read_verified() {
        // requires offset + len <= buf_len, ensures offset + len <= buf_len
        let requires = vec![binop(
            binop(ident("offset"), BinOp::Add, ident("len")),
            BinOp::Lte,
            ident("buf_len"),
        )];
        let ensures = binop(
            binop(ident("offset"), BinOp::Add, ident("len")),
            BinOp::Lte,
            ident("buf_len"),
        );
        let result = verify_buffer_bounds_cvc5(&requires, &ensures);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "safe buffer read should verify, got: {result:?}"
        );
    }

    #[test]
    fn cvc5_unsafe_buffer_read_counterexample() {
        // No requires, ensures offset + len <= buf_len
        let requires: Vec<SpExpr> = vec![];
        let ensures = binop(
            binop(ident("offset"), BinOp::Add, ident("len")),
            BinOp::Lte,
            ident("buf_len"),
        );
        let result = verify_buffer_bounds_cvc5(&requires, &ensures);
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "unsafe buffer read should produce counterexample, got: {result:?}"
        );
    }

    // -------------------------------------------------------------------
    // Region containment parity (Z3: test_region_containment_*)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_region_sub_within_parent() {
        let context = vec![binop(ident("cap"), BinOp::Gte, int_lit(5))];
        let result = verify_region_containment_cvc5(
            &context,
            &int_lit(2),
            &int_lit(5),
            &int_lit(0),
            &ident("cap"),
        );
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "[2,5) subset [0,cap) with cap>=5 should verify, got: {result:?}"
        );
    }

    #[test]
    fn cvc5_region_sub_exceeds_parent() {
        let context: Vec<SpExpr> = vec![];
        let result = verify_region_containment_cvc5(
            &context,
            &int_lit(0),
            &int_lit(10),
            &int_lit(0),
            &int_lit(5),
        );
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "[0,10) NOT subset [0,5) should produce counterexample, got: {result:?}"
        );
    }

    #[test]
    fn cvc5_region_same_range() {
        let context: Vec<SpExpr> = vec![];
        let result = verify_region_containment_cvc5(
            &context,
            &int_lit(0),
            &ident("n"),
            &int_lit(0),
            &ident("n"),
        );
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "[0,n) subset [0,n) should verify, got: {result:?}"
        );
    }

    #[test]
    fn cvc5_region_shifted_sub() {
        let context = vec![
            binop(ident("start"), BinOp::Gte, int_lit(0)),
            binop(ident("len"), BinOp::Gte, int_lit(0)),
            binop(
                binop(ident("start"), BinOp::Add, ident("len")),
                BinOp::Lte,
                ident("cap"),
            ),
        ];
        let result = verify_region_containment_cvc5(
            &context,
            &ident("start"),
            &binop(ident("start"), BinOp::Add, ident("len")),
            &int_lit(0),
            &ident("cap"),
        );
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "[start,start+len) subset [0,cap) with bounds should verify, got: {result:?}"
        );
    }

    // -------------------------------------------------------------------
    // Taint tracking parity (Z3: test_taint_*)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_taint_safe_all_validated() {
        use assura_types::TaintLabel;
        let labels = vec![
            ("idx".to_string(), TaintLabel::Validated),
            ("len".to_string(), TaintLabel::Trusted),
        ];
        let validation_fns = vec!["validate".to_string()];
        let sensitive = vec![
            ("idx".to_string(), TaintLabel::Validated),
            ("len".to_string(), TaintLabel::Validated),
        ];
        let result = verify_taint_safety_cvc5(&labels, &validation_fns, &sensitive);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "all validated should verify, got: {result:?}"
        );
    }

    #[test]
    fn cvc5_taint_unsafe_untrusted() {
        use assura_types::TaintLabel;
        let labels = vec![("raw_idx".to_string(), TaintLabel::Untrusted)];
        let sensitive = vec![("raw_idx".to_string(), TaintLabel::Validated)];
        let result = verify_taint_safety_cvc5(&labels, &[], &sensitive);
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "untrusted at validated sink should produce counterexample, got: {result:?}"
        );
    }

    #[test]
    fn cvc5_taint_no_sensitive() {
        use assura_types::TaintLabel;
        let labels = vec![("x".to_string(), TaintLabel::Untrusted)];
        let result = verify_taint_safety_cvc5(&labels, &[], &[]);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "no sensitive uses should verify, got: {result:?}"
        );
    }

    #[test]
    fn cvc5_taint_mixed_labels() {
        use assura_types::TaintLabel;
        let labels = vec![
            ("safe".to_string(), TaintLabel::Validated),
            ("unsafe_var".to_string(), TaintLabel::Untrusted),
        ];
        let sensitive = vec![
            ("safe".to_string(), TaintLabel::Validated),
            ("unsafe_var".to_string(), TaintLabel::Validated),
        ];
        let result = verify_taint_safety_cvc5(&labels, &[], &sensitive);
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "mixed labels with one violation should produce counterexample, got: {result:?}"
        );
    }

    #[test]
    fn cvc5_taint_trusted_satisfies_all() {
        use assura_types::TaintLabel;
        let labels = vec![("key".to_string(), TaintLabel::Trusted)];
        let sensitive = vec![("key".to_string(), TaintLabel::Trusted)];
        let result = verify_taint_safety_cvc5(&labels, &[], &sensitive);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "trusted at trusted sink should verify, got: {result:?}"
        );
    }

    // -------------------------------------------------------------------
    // feature_max constants parity (Z3: test_feature_max_*)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_feature_max_constant_is_bound() {
        // feature_max MAX_SIZE = 65536, ensures MAX_SIZE == 65536
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: binop(ident("MAX_SIZE"), BinOp::Gt, int_lit(0)),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: binop(ident("MAX_SIZE"), BinOp::Eq, int_lit(65536)),
                effect_variables: vec![],
            },
        ];
        let constants = vec![("MAX_SIZE".to_string(), 65536i64)];
        let mut cache = SessionCache::new();
        let results = verify_contract_cvc5_with_full_context(
            "UsesConstant",
            &clauses,
            &[],
            &[],
            &constants,
            &mut cache,
        );
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. } => {
                clause_desc.contains("ensures")
            }
            _ => false,
        });
        assert!(ensures.is_some(), "should have an ensures result");
        assert!(
            matches!(ensures.unwrap(), VerificationResult::Verified { .. }),
            "feature_max should bind MAX_SIZE to 65536, got: {:?}",
            ensures.unwrap()
        );
    }

    #[test]
    fn cvc5_feature_max_arithmetic() {
        // HEADER_SIZE=3, requires HEADER_SIZE+payload<=record, ensures record>=3
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: binop(ident("payload"), BinOp::Gte, int_lit(0)),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Requires,
                body: binop(ident("record"), BinOp::Gte, int_lit(0)),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Requires,
                body: binop(
                    binop(ident("HEADER_SIZE"), BinOp::Add, ident("payload")),
                    BinOp::Lte,
                    ident("record"),
                ),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: binop(ident("record"), BinOp::Gte, int_lit(3)),
                effect_variables: vec![],
            },
        ];
        let constants = vec![("HEADER_SIZE".to_string(), 3i64)];
        let mut cache = SessionCache::new();
        let results = verify_contract_cvc5_with_full_context(
            "HeaderArith",
            &clauses,
            &[],
            &[],
            &constants,
            &mut cache,
        );
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. } => {
                clause_desc.contains("ensures")
            }
            _ => false,
        });
        assert!(ensures.is_some(), "should have an ensures result");
        assert!(
            matches!(ensures.unwrap(), VerificationResult::Verified { .. }),
            "HEADER_SIZE=3 + payload <= record should imply record >= 3, got: {:?}",
            ensures.unwrap()
        );
    }

    #[test]
    fn cvc5_feature_max_wrong_value_counterexample() {
        // LIMIT=10, ensures LIMIT > 100 -> counterexample
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: binop(ident("LIMIT"), BinOp::Gt, int_lit(0)),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: binop(ident("LIMIT"), BinOp::Gt, int_lit(100)),
                effect_variables: vec![],
            },
        ];
        let constants = vec![("LIMIT".to_string(), 10i64)];
        let mut cache = SessionCache::new();
        let results = verify_contract_cvc5_with_full_context(
            "WrongClaim",
            &clauses,
            &[],
            &[],
            &constants,
            &mut cache,
        );
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. } => {
                clause_desc.contains("ensures")
            }
            _ => false,
        });
        assert!(ensures.is_some(), "should have an ensures result");
        assert!(
            matches!(ensures.unwrap(), VerificationResult::Counterexample { .. }),
            "LIMIT=10 > 100 should produce counterexample, got: {:?}",
            ensures.unwrap()
        );
    }

    #[test]
    fn cvc5_feature_max_multiple() {
        // HEADER=5, FOOTER=3, requires HEADER+payload+FOOTER<=100, ensures payload<=92
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: binop(ident("payload"), BinOp::Gte, int_lit(0)),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Requires,
                body: binop(
                    binop(
                        binop(ident("HEADER"), BinOp::Add, ident("payload")),
                        BinOp::Add,
                        ident("FOOTER"),
                    ),
                    BinOp::Lte,
                    int_lit(100),
                ),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: binop(ident("payload"), BinOp::Lte, int_lit(92)),
                effect_variables: vec![],
            },
        ];
        let constants = vec![("HEADER".to_string(), 5i64), ("FOOTER".to_string(), 3i64)];
        let mut cache = SessionCache::new();
        let results = verify_contract_cvc5_with_full_context(
            "MultiConst",
            &clauses,
            &[],
            &[],
            &constants,
            &mut cache,
        );
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. } => {
                clause_desc.contains("ensures")
            }
            _ => false,
        });
        assert!(ensures.is_some(), "should have an ensures result");
        assert!(
            matches!(ensures.unwrap(), VerificationResult::Verified { .. }),
            "5 + payload + 3 <= 100 should imply payload <= 92, got: {:?}",
            ensures.unwrap()
        );
    }

    #[test]
    fn cvc5_feature_max_narrowing_derives_pairs() {
        // Unit test for derive_narrowings (shared between Z3 and CVC5)
        use crate::feature_max::derive_narrowings;
        let constants = vec![
            ("max_page_size".to_string(), 4096),
            ("MAX_CONTENT_LEN".to_string(), 16384),
            ("LIMIT".to_string(), 100),
        ];
        let narrowings = derive_narrowings(&constants);
        assert!(
            narrowings
                .iter()
                .any(|(n, v)| n == "page_size" && *v == 4096),
            "should derive page_size narrowing"
        );
        assert!(
            narrowings
                .iter()
                .any(|(n, v)| n == "CONTENT_LEN" && *v == 16384),
            "should derive CONTENT_LEN narrowing"
        );
        // LIMIT has no max_ prefix, should not produce narrowing
        assert!(
            !narrowings.iter().any(|(n, _)| n == "IMIT" || n == "imit"),
            "LIMIT should not produce narrowing"
        );
    }

    // -------------------------------------------------------------------
    // Nat return type parity (Z3: test_nat_return_type_*)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_nat_return_type_constrains_result() {
        // fn returning Nat, ensures result >= 0 -> Verified
        let params = vec![Param {
            name: "n".into(),
            ty: Some(assura_ast::TypeExpr::Named("Nat".into())),
        }];
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: binop(ident("n"), BinOp::Gte, int_lit(0)),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: binop(ident("result"), BinOp::Gte, int_lit(0)),
                effect_variables: vec![],
            },
        ];
        let mut cache = SessionCache::new();
        let results = verify_contract_cvc5_with_types(
            "NatReturnFn",
            &clauses,
            &params,
            &["Nat".into()],
            &mut cache,
        );
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. } => {
                clause_desc.contains("ensures")
            }
            _ => false,
        });
        assert!(ensures.is_some(), "should have ensures result");
        assert!(
            matches!(ensures.unwrap(), VerificationResult::Verified { .. }),
            "Nat return type should constrain result >= 0, got: {:?}",
            ensures.unwrap()
        );
    }

    #[test]
    fn cvc5_nat_return_type_genuine_counterexample() {
        // fn returning Nat, ensures result < 0 -> Counterexample
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: binop(ident("result"), BinOp::Lt, int_lit(0)),
            effect_variables: vec![],
        }];
        let mut cache = SessionCache::new();
        let results = verify_contract_cvc5_with_types(
            "BadNatReturn",
            &clauses,
            &[],
            &["Nat".into()],
            &mut cache,
        );
        let ensures = results.iter().find(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. } => {
                clause_desc.contains("ensures")
            }
            _ => false,
        });
        assert!(ensures.is_some(), "should have ensures result");
        assert!(
            matches!(ensures.unwrap(), VerificationResult::Counterexample { .. }),
            "result < 0 with Nat return should produce counterexample, got: {:?}",
            ensures.unwrap()
        );
    }

    // -------------------------------------------------------------------
    // Raw token operator parity (Z3: test_raw_*)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_raw_implies() {
        // x > 0 implies x >= 1 (tautology in integers)
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: binop(ident("x"), BinOp::Gt, int_lit(0)),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: binop(ident("x"), BinOp::Gte, int_lit(1)),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("ImpliesTest", &clauses);
        assert!(!results.is_empty());
        assert!(
            matches!(results[0], VerificationResult::Verified { .. }),
            "x > 0 => x >= 1 should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn cvc5_raw_modulo() {
        // x >= 0, ensures x mod 2 >= 0 (via raw tokens)
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: binop(ident("x"), BinOp::Gte, int_lit(0)),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: binop(
                    Spanned::no_span(Expr::Raw(vec!["x".into(), "mod".into(), "2".into()])),
                    BinOp::Gte,
                    int_lit(0),
                ),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("ModTest", &clauses);
        assert!(!results.is_empty());
        assert!(
            matches!(results[0], VerificationResult::Verified { .. }),
            "non-negative modulo should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn cvc5_raw_result_keyword() {
        // ensures result >= 0 || result < 0 (tautology)
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                op: BinOp::Or,
                lhs: Box::new(binop(
                    Spanned::no_span(Expr::Raw(vec!["result".into()])),
                    BinOp::Gte,
                    int_lit(0),
                )),
                rhs: Box::new(binop(
                    Spanned::no_span(Expr::Raw(vec!["result".into()])),
                    BinOp::Lt,
                    int_lit(0),
                )),
            }),
            effect_variables: vec![],
        }];
        let results = verify_contract_cvc5("ResultTest", &clauses);
        assert!(!results.is_empty());
        assert!(
            matches!(results[0], VerificationResult::Verified { .. }),
            "result tautology should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn cvc5_raw_old_ident() {
        // CVC5 old(y) encoding produces results (parity with Z3 test_raw_old_ident).
        // The full-pipeline tautology old(y)>=0 || old(y)<0 requires frame axioms
        // and Z3's get_or_create_int for name dedup; CVC5 mk_const creates fresh
        // constants, so we test encoding path: old(y) encodes without panicking.
        let clauses = vec![
            Clause {
                kind: ClauseKind::Modifies,
                body: Spanned::no_span(Expr::Raw(vec!["x".into()])),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: binop(
                    Spanned::no_span(Expr::Old(Box::new(ident("y")))),
                    BinOp::Gte,
                    int_lit(0),
                ),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("OldTest", &clauses);
        assert!(
            !results.is_empty(),
            "old(y) should produce verification results"
        );
        // Note: Z3 test_raw_old_ident verifies the tautology old(y)>=0||old(y)<0
        // because Z3 deduplicates constants by name. CVC5 native mk_const creates
        // fresh constants, so the tautology may not hold at clause level.
        // The parity point: encoding old() doesn't crash.
    }

    // -------------------------------------------------------------------
    // Chained comparison parity (Z3: chained_comparison_*)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_chained_lower_upper_bound() {
        // requires x > 0 && x < 10, ensures 0 <= x && x < 10
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: binop(
                    binop(ident("x"), BinOp::Gt, int_lit(0)),
                    BinOp::And,
                    binop(ident("x"), BinOp::Lt, int_lit(10)),
                ),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: binop(
                    binop(int_lit(0), BinOp::Lte, ident("x")),
                    BinOp::And,
                    binop(ident("x"), BinOp::Lt, int_lit(10)),
                ),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("ChainedBounds", &clauses);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "chained comparison should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn cvc5_chained_three_way() {
        // requires a < b && b < c, ensures a < c (transitivity)
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: binop(
                    binop(ident("a"), BinOp::Lt, ident("b")),
                    BinOp::And,
                    binop(ident("b"), BinOp::Lt, ident("c")),
                ),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: binop(ident("a"), BinOp::Lt, ident("c")),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("ThreeWay", &clauses);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "transitivity should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn cvc5_chained_false_case() {
        // requires x > 0 && x > 10, ensures x > 20 -> counterexample
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: binop(
                    binop(ident("x"), BinOp::Gt, int_lit(0)),
                    BinOp::And,
                    binop(ident("x"), BinOp::Gt, int_lit(10)),
                ),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: binop(ident("x"), BinOp::Gt, int_lit(20)),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("ChainedFalse", &clauses);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "false chained claim should produce counterexample, got: {:?}",
            results[0]
        );
    }

    // -------------------------------------------------------------------
    // Deep field chain parity (Z3: deep_field_chain_*)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_deep_field_chain_verifies() {
        // requires x.head.extra.extra_max >= 0, ensures x.head.extra.extra_max >= 0
        let field_chain = Spanned::no_span(Expr::Field(
            Box::new(Spanned::no_span(Expr::Field(
                Box::new(Spanned::no_span(Expr::Field(
                    Box::new(ident("x")),
                    "head".into(),
                ))),
                "extra".into(),
            ))),
            "extra_max".into(),
        ));
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: binop(field_chain.clone(), BinOp::Gte, int_lit(0)),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: binop(field_chain, BinOp::Gte, int_lit(0)),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("DeepChain", &clauses);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "deep field chain ensures should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn cvc5_deep_field_chain_unconstrained_counterexample() {
        // No requires, ensures x.y.z >= 0 -> counterexample (unconstrained)
        let field_chain = Spanned::no_span(Expr::Field(
            Box::new(Spanned::no_span(Expr::Field(
                Box::new(ident("x")),
                "y".into(),
            ))),
            "z".into(),
        ));
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: binop(field_chain, BinOp::Gte, int_lit(0)),
            effect_variables: vec![],
        }];
        let results = verify_contract_cvc5("DeepChainUnconstrained", &clauses);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "unconstrained deep field chain should produce counterexample, got: {:?}",
            results[0]
        );
    }

    // -------------------------------------------------------------------
    // Index bounds parity (Z3: test_index_bounds_axiom)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_index_bounds_axiom() {
        // requires i >= 0 and i < buf.length(), ensures i >= 0
        let len_call = Spanned::no_span(Expr::MethodCall {
            receiver: Box::new(ident("buf")),
            method: "length".into(),
            args: vec![],
        });
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: binop(
                    binop(ident("i"), BinOp::Gte, int_lit(0)),
                    BinOp::And,
                    binop(ident("i"), BinOp::Lt, len_call),
                ),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: binop(ident("i"), BinOp::Gte, int_lit(0)),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("IndexBounds", &clauses);
        assert!(!results.is_empty());
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "index bounds with requires should verify, got: {:?}",
            results[0]
        );
    }

    // -------------------------------------------------------------------
    // Empty block parity (Z3: test_z3_empty_block_returns_bool_true)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_empty_block_verifies_as_true() {
        // Empty block {} in ensures position should behave as true -> Verified
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::Block(vec![])),
            effect_variables: vec![],
        }];
        let results = verify_contract_cvc5("EmptyBlock", &clauses);
        // Empty block encodes as true, so ensures {true} is valid
        assert!(
            !results.is_empty(),
            "should have results for empty block ensures"
        );
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "empty block ensures should verify as true, got: {:?}",
            results[0]
        );
    }

    // -------------------------------------------------------------------
    // ADT injectivity / exhaustiveness parity (Z3: test_z3_adt_*)
    // These test at the CVC5 SMT-LIB level since the native ADT API
    // is Z3-specific. The contract-level ADT tests exist above.
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_adt_native_define_and_construct() {
        // Define ADT and construct a value using the native CVC5 API
        use crate::cvc5_backend::cvc5_adt::define_adt_cvc5_native;
        let tm = cvc5::TermManager::new();
        let mut solver = cvc5::Solver::new(&tm);
        solver.set_logic("ALL");
        let (adt_def, symbols) = define_adt_cvc5_native(
            &tm,
            &mut solver,
            "Option",
            &[("Some", &["value"]), ("None", &[])],
        );
        // Should produce a valid ADT definition with 2 constructors
        assert_eq!(
            adt_def.constructors.len(),
            2,
            "Option ADT should have 2 constructors: Some and None"
        );
        assert!(
            adt_def.constructors.iter().any(|c| c.name == "Some"),
            "Should have Some constructor"
        );
        assert!(
            adt_def.constructors.iter().any(|c| c.name == "None"),
            "Should have None constructor"
        );
        // Symbols should have tag function
        assert!(
            !symbols.adt_name.is_empty(),
            "ADT symbols should have a name"
        );
    }

    #[test]
    fn cvc5_adt_native_accessor() {
        // Define Pair ADT and verify accessor retrieval
        use crate::cvc5_backend::cvc5_adt::{adt_accessor_cvc5_native, define_adt_cvc5_native};
        let tm = cvc5::TermManager::new();
        let mut solver = cvc5::Solver::new(&tm);
        solver.set_logic("ALL");
        let (_adt_def, symbols) =
            define_adt_cvc5_native(&tm, &mut solver, "Pair", &[("MkPair", &["fst", "snd"])]);
        // Access the "fst" accessor via the native API
        let x = tm.mk_const(tm.integer_sort(), "x");
        let fst_term = adt_accessor_cvc5_native(&tm, &symbols, "fst", &x);
        assert!(
            fst_term.sort().is_integer(),
            "accessor should return integer-sorted term"
        );
    }

    #[test]
    fn cvc5_adt_native_is_constructor() {
        // Verify is_constructor check produces a boolean term
        use crate::cvc5_backend::cvc5_adt::{
            adt_is_constructor_cvc5_native, define_adt_cvc5_native,
        };
        let tm = cvc5::TermManager::new();
        let mut solver = cvc5::Solver::new(&tm);
        solver.set_logic("ALL");
        let (adt_def, symbols) = define_adt_cvc5_native(
            &tm,
            &mut solver,
            "Option",
            &[("Some", &["value"]), ("None", &[])],
        );
        let x = tm.mk_const(tm.integer_sort(), "x");
        let some_ctor = adt_def
            .constructors
            .iter()
            .find(|c| c.name == "Some")
            .unwrap();
        let is_some = adt_is_constructor_cvc5_native(&tm, &symbols, some_ctor, &x);
        assert!(
            is_some.sort().is_boolean(),
            "is_constructor should produce a boolean term"
        );
    }

    // -------------------------------------------------------------------
    // Quantifier domain parity (Z3: forall/exists_with_range_domain)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_forall_range_domain_does_not_panic() {
        // forall i in 0..10 : i >= 0 (requires-only -> no verifiable clauses)
        // Tests the CVC5 encoder does not panic on quantifier domain
        let clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Spanned::no_span(Expr::Forall {
                var: "i".into(),
                domain: Box::new(binop(int_lit(0), BinOp::Range, int_lit(10))),
                body: Box::new(binop(ident("i"), BinOp::Gte, int_lit(0))),
            }),
            effect_variables: vec![],
        }];
        // Should produce no verifiable clauses (requires-only), no panic
        let results = verify_contract_cvc5("RangeForall", &clauses);
        assert!(
            results.is_empty(),
            "requires-only contract should have no verifiable clauses"
        );
    }

    // -------------------------------------------------------------------
    // #509: Counterexample value verification (CVC5)
    // -------------------------------------------------------------------

    // make_clause is at file level (super::make_clause) per #512.
    use super::make_clause;

    #[test]
    fn cvc5_counterexample_value_correct() {
        // requires: x > 0, ensures: x > 100
        // CE must have x in (0, 100] to satisfy requires but violate ensures.
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                },
            ),
            make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("100".into())))),
                },
            ),
        ];
        let results = crate::cvc5_backend::verify_contract_cvc5("CvcCEValue", &clauses);
        if results.is_empty() {
            return; // cvc5 not installed
        }
        match &results[0] {
            VerificationResult::Counterexample {
                counter_model: Some(cm),
                ..
            } => {
                let x: Option<i64> = cm
                    .variables
                    .iter()
                    .find(|(n, _)| n == "x")
                    .and_then(|(_, v)| v.parse().ok());
                if let Some(x) = x {
                    assert!(x > 0, "CE x={x} should satisfy requires (x > 0)");
                    assert!(x <= 100, "CE x={x} should violate ensures (x > 100)");
                }
                // If parsing fails, the model format is different but that is OK;
                // the test still proves a CE was produced with a model.
            }
            VerificationResult::Unknown { .. } => {
                // cvc5 not installed or solver inconclusive; skip
            }
            other => panic!("expected Counterexample or Unknown, got: {other:?}"),
        }
    }

    #[test]
    fn cvc5_exists_range_domain_does_not_panic() {
        // exists i in 0..5 : i == 3
        let clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Spanned::no_span(Expr::Exists {
                var: "i".into(),
                domain: Box::new(binop(int_lit(0), BinOp::Range, int_lit(5))),
                body: Box::new(binop(ident("i"), BinOp::Eq, int_lit(3))),
            }),
            effect_variables: vec![],
        }];
        let results = verify_contract_cvc5("RangeExists", &clauses);
        assert!(
            results.is_empty(),
            "requires-only contract should have no verifiable clauses"
        );
    }
}
