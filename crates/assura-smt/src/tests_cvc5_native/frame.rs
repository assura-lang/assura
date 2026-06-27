use super::*;

// -------------------------------------------------------------------
// Frame axiom tests (CVC5 native, issue #256)
// -------------------------------------------------------------------

#[cfg(feature = "cvc5-verify")]
mod frame_tests {
    use super::*;

    #[test]
    fn test_cvc5_frame_axiom_injection() {
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Modifies,
                body: Spanned::no_span(Expr::Ident("y".into())),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
        ];
        let results = crate::cvc5_backend::verify_contract_cvc5("FrameTest", &clauses);
        assert!(!results.is_empty());
    }

    #[test]
    fn test_cvc5_modifies_preserves_unmodified() {
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Eq,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Modifies,
                body: Spanned::no_span(Expr::Ident("y".into())),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Eq,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
                }),
                effect_variables: vec![],
            },
        ];
        let results = crate::cvc5_backend::verify_contract_cvc5("FramePreserve", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "Frame axiom: expected Verified, got: {:?}",
                r
            );
        }
    }

    // ---------------------------------------------------------------
    // Lemma injection tests (#254)
    // ---------------------------------------------------------------

    #[test]
    fn native_cvc5_lemma_injection_basic() {
        // Contract with apply(lemma): the ensures body contains an
        // apply expression, which should be encoded as a named bool.
        // Without lemma defs, this just produces a result (not a panic).
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::Apply {
                    lemma_name: "helper_lemma".into(),
                    args: vec![Spanned::no_span(Expr::Ident("x".into()))],
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("LemmaTest", &clauses);
        assert!(!results.is_empty(), "should produce at least one result");
    }

    #[test]
    fn native_cvc5_lemma_postcondition_injected() {
        // Build a lemma_defs map where "pos_lemma" ensures x >= 0.
        // The ensures clause uses `apply pos_lemma(x)` inside a
        // conjunction with `true`. With the lemma postcondition
        // injected as an assumption, this should not produce false
        // counterexamples for the apply sub-expression.
        let mut lemma_defs = std::collections::HashMap::new();
        let lemma_ensures = Spanned::no_span(Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        });
        lemma_defs.insert("pos_lemma".to_string(), vec![&lemma_ensures]);

        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::And,
                    lhs: Box::new(Spanned::no_span(Expr::Apply {
                        lemma_name: "pos_lemma".into(),
                        args: vec![Spanned::no_span(Expr::Ident("x".into()))],
                    })),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Bool(true)))),
                }),
                effect_variables: vec![],
            },
        ];
        let mut cache = SessionCache::new();
        let results = verify_lemmas_test(
            "ApplyPostcondTest",
            &clauses,
            &[],
            &[],
            Some(&lemma_defs),
            None,
            &mut cache,
        );
        assert!(
            !results.is_empty(),
            "should produce at least one result with lemma injection"
        );
    }

    #[test]
    fn native_cvc5_lemma_injection_verifies_with_postcondition() {
        // The ensures clause says: x >= 0 (trivially follows from requires).
        // We also have an apply expression in the clause. With lemma defs
        // injecting x >= 0, the combined clause should still verify.
        let mut lemma_defs = std::collections::HashMap::new();
        let lemma_ensures = Spanned::no_span(Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        });
        lemma_defs.insert("helper".to_string(), vec![&lemma_ensures]);

        // requires { x > 0 }
        // ensures { x >= 0 }  (trivially true from requires)
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
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
        ];
        let mut cache = SessionCache::new();
        let results = verify_lemmas_test(
            "LemmaVerifTest",
            &clauses,
            &[],
            &[],
            Some(&lemma_defs),
            None,
            &mut cache,
        );
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "should verify with lemma injection: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_no_lemma_defs_still_works() {
        // When lemma_defs is None, the apply expression is just
        // encoded as a named boolean (no postcondition injected).
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::Apply {
                lemma_name: "unknown_lemma".into(),
                args: vec![Spanned::no_span(Expr::Ident("x".into()))],
            }),
            effect_variables: vec![],
        }];
        let mut cache = SessionCache::new();
        let results = verify_lemmas_test("NoLemmaDefs", &clauses, &[], &[], None, None, &mut cache);
        assert!(
            !results.is_empty(),
            "should produce results even without lemma defs"
        );
    }

    // ---------------------------------------------------------------
    // CVC5 Real sort float encoding tests (#248)
    // ---------------------------------------------------------------

    #[test]
    fn test_cvc5_float_real_sort() {
        // Float literal in requires/ensures should encode as CVC5 Real sort.
        // requires { x > 0 }, requires { x < 1000000 },
        // ensures { x > 0 } -- trivially true from precondition
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Float(
                        "0.0".into(),
                    )))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Float(
                        "0.0".into(),
                    )))),
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("FloatRealSort", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "float Real sort should verify: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_cvc5_real_ite_promotion() {
        // ITE with mixed Int/Real branches should sort-promote.
        // requires { x > 0 }
        // ensures { if x > 0 then 1.5 else 0 > 0 }
        // The then branch is Real (1.5), else is Int (0).
        // Sort promotion converts the Int to Real so ITE succeeds.
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
                    lhs: Box::new(Spanned::no_span(Expr::If {
                        cond: Box::new(Spanned::no_span(Expr::BinOp {
                            op: BinOp::Gt,
                            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int(
                                "0".into(),
                            )))),
                        })),
                        then_branch: Box::new(Spanned::no_span(Expr::Literal(Literal::Float(
                            "1.5".into(),
                        )))),
                        else_branch: Some(Box::new(Spanned::no_span(Expr::Literal(Literal::Int(
                            "0".into(),
                        ))))),
                    })),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Float(
                        "0.0".into(),
                    )))),
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("ItePromotion", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "ITE sort promotion should verify: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_cvc5_real_negation() {
        // Negated float should work with Real sort.
        // requires { x > 1.0 }, ensures { -x < 0.0 }
        // True because x > 1.0 implies -x < -1.0 < 0.0
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Float(
                        "1.0".into(),
                    )))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Lt,
                    lhs: Box::new(Spanned::no_span(Expr::UnaryOp {
                        op: UnaryOp::Neg,
                        expr: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    })),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Float(
                        "0.0".into(),
                    )))),
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("RealNeg", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "negated float Real should verify: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_cvc5_float_arithmetic_verifies() {
        // Float arithmetic: requires { x > 2.0 }, ensures { x + 1.0 > 3.0 }
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Float(
                        "2.0".into(),
                    )))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Spanned::no_span(Expr::BinOp {
                        op: BinOp::Add,
                        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Float(
                            "1.0".into(),
                        )))),
                    })),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Float(
                        "3.0".into(),
                    )))),
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("FloatArith", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "float arithmetic should verify: {:?}",
            results[0]
        );
    }

    // ---------------------------------------------------------------
    // CVC5 quantifier trigger pattern inference tests (#247)
    // ---------------------------------------------------------------

    #[test]
    fn test_cvc5_quantifier_trigger_inference() {
        let tm = cvc5::TermManager::new();
        let bound = tm.mk_var(tm.integer_sort(), "i");

        let body = Spanned::no_span(Expr::BinOp {
            op: BinOp::Gt,
            lhs: Box::new(Spanned::no_span(Expr::Call {
                func: Box::new(Spanned::no_span(Expr::Ident("f".into()))),
                args: vec![Spanned::no_span(Expr::Ident("i".into()))],
            })),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        });

        let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
        assert!(
            !patterns.is_empty(),
            "should infer trigger from f(i) call in quantifier body"
        );
    }

    #[test]
    fn test_cvc5_trigger_no_call_no_pattern() {
        let tm = cvc5::TermManager::new();
        let bound = tm.mk_var(tm.integer_sort(), "i");

        let body = Spanned::no_span(Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Spanned::no_span(Expr::Ident("i".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        });

        let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
        assert!(
            patterns.is_empty(),
            "no function calls means no triggers: got {:?}",
            patterns.len()
        );
    }

    #[test]
    fn test_cvc5_trigger_nested_call() {
        let tm = cvc5::TermManager::new();
        let bound = tm.mk_var(tm.integer_sort(), "i");

        let body = Spanned::no_span(Expr::BinOp {
            op: BinOp::Gt,
            lhs: Box::new(Spanned::no_span(Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(Spanned::no_span(Expr::Call {
                    func: Box::new(Spanned::no_span(Expr::Ident("g".into()))),
                    args: vec![Spanned::no_span(Expr::Ident("i".into()))],
                })),
                rhs: Box::new(Spanned::no_span(Expr::Call {
                    func: Box::new(Spanned::no_span(Expr::Ident("h".into()))),
                    args: vec![Spanned::no_span(Expr::Ident("i".into()))],
                })),
            })),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        });

        let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
        assert!(
            patterns.len() >= 2,
            "should infer triggers from both g(i) and h(i): got {}",
            patterns.len()
        );
    }

    #[test]
    fn test_cvc5_trigger_manager_integration() {
        let tm = cvc5::TermManager::new();
        let bound = tm.mk_var(tm.integer_sort(), "i");

        let body = Spanned::no_span(Expr::Call {
            func: Box::new(Spanned::no_span(Expr::Ident("lookup".into()))),
            args: vec![Spanned::no_span(Expr::Ident("i".into()))],
        });

        let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
        assert!(
            !patterns.is_empty(),
            "should infer trigger from lookup(i) via direct scan fallback"
        );
    }

    #[test]
    fn test_cvc5_quantified_with_trigger_verifies() {
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
                body: Spanned::no_span(Expr::Forall {
                    var: "i".into(),
                    domain: Box::new(Spanned::no_span(Expr::BinOp {
                        op: BinOp::Range,
                        lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                        rhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    })),
                    body: Box::new(Spanned::no_span(Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Spanned::no_span(Expr::Ident("i".into()))),
                        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                    })),
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("QuantTriggerTest", &clauses);
        assert!(!results.is_empty(), "should produce verification results");
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "quantified contract should verify: {:?}",
            results[0]
        );
    }

    /// Contract-level TriggerManager is seeded from clauses and used when
    /// encoding quantifiers that mention known functions (e.g. `f(i)`).
    #[test]
    fn test_cvc5_contract_trigger_manager_seeds_from_clauses() {
        use crate::cvc5_encoder_state::{
            default_cvc5_encoder_state, seed_cvc5_trigger_manager_from_clauses,
        };

        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::Call {
                    func: Box::new(Spanned::no_span(Expr::Ident("lookup".into()))),
                    args: vec![Spanned::no_span(Expr::Ident("x".into()))],
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::Forall {
                    var: "i".into(),
                    domain: Box::new(Spanned::no_span(Expr::BinOp {
                        op: BinOp::Range,
                        lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                        rhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    })),
                    body: Box::new(Spanned::no_span(Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Spanned::no_span(Expr::Call {
                            func: Box::new(Spanned::no_span(Expr::Ident("lookup".into()))),
                            args: vec![Spanned::no_span(Expr::Ident("i".into()))],
                        })),
                        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                    })),
                }),
                effect_variables: vec![],
            },
        ];

        let mut state = default_cvc5_encoder_state();
        seed_cvc5_trigger_manager_from_clauses(&mut state, &clauses);
        assert!(
            state
                .trigger_manager
                .known_functions()
                .iter()
                .any(|f| f == "lookup"),
            "lookup should be registered from requires/ensures: {:?}",
            state.trigger_manager.known_functions()
        );

        let body = &clauses[1].body;
        if let Expr::Forall { body: qbody, .. } = &body.node {
            let trigger = state
                .trigger_manager
                .infer_trigger_from_expr(qbody, "i")
                .expect("should infer lookup(i) trigger from seeded manager");
            assert!(
                trigger.terms.iter().any(|t| t.contains("lookup")),
                "expected lookup trigger, got {:?}",
                trigger.terms
            );
        } else {
            panic!("expected Forall in ensures");
        }
    }

    #[test]
    fn test_cvc5_multi_arg_trigger() {
        let tm = cvc5::TermManager::new();
        let bound = tm.mk_var(tm.integer_sort(), "i");

        let body = Spanned::no_span(Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Spanned::no_span(Expr::Call {
                func: Box::new(Spanned::no_span(Expr::Ident("lookup".into()))),
                args: vec![
                    Spanned::no_span(Expr::Ident("table".into())),
                    Spanned::no_span(Expr::Ident("i".into())),
                ],
            })),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        });

        let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
        assert!(
            !patterns.is_empty(),
            "should infer trigger from multi-arg lookup(table, i)"
        );
    }

    // -------------------------------------------------------------------
    // CVC5 session cache tests (#253)
    // -------------------------------------------------------------------

    #[test]
    fn test_cvc5_session_cache_hit() {
        // Verify same contract twice; second call should return cached result
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
        ];

        let mut cache = SessionCache::new();

        // First call: cache miss, runs CVC5
        let results1 = verify_lemmas_test("CacheTest", &clauses, &[], &[], None, None, &mut cache);
        assert_eq!(results1.len(), 1);
        assert!(matches!(&results1[0], VerificationResult::Verified { .. }));
        assert_eq!(cache.entry_count(), 1);

        // Second call: cache hit, should not invoke CVC5
        let results2 = verify_lemmas_test("CacheTest", &clauses, &[], &[], None, None, &mut cache);
        assert_eq!(results2.len(), 1);
        assert!(matches!(&results2[0], VerificationResult::Verified { .. }));
        // Cache should still have 1 entry (same key), with 1 hit
        assert_eq!(cache.entry_count(), 1);
        assert!(cache.hit_rate() > 0.0);
    }

    #[test]
    fn test_cvc5_session_cache_miss() {
        // Two different contracts should be cache misses
        let clauses_a = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
        ];
        let clauses_b = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
        ];

        let mut cache = SessionCache::new();

        let results_a = verify_lemmas_test("CacheA", &clauses_a, &[], &[], None, None, &mut cache);
        assert_eq!(results_a.len(), 1);
        assert_eq!(cache.entry_count(), 1);

        let results_b = verify_lemmas_test("CacheB", &clauses_b, &[], &[], None, None, &mut cache);
        assert_eq!(results_b.len(), 1);
        // Both should be cache misses, so 2 entries
        assert_eq!(cache.entry_count(), 2);
    }

    // -------------------------------------------------------------------
    // #263: CVC5 ADT encoding tests
    // -------------------------------------------------------------------

    #[test]
    fn test_cvc5_adt_constructor() {
        // Define Option = Some(value: Int) | None using CVC5 native API.
        // Verify that constructor tags are distinct and accessors work.
        let tm = cvc5::TermManager::new();
        let mut solver = cvc5::Solver::new(&tm);
        solver.set_logic("ALL");
        solver.set_option("produce-models", "true");
        solver.set_option("tlimit", "2000");

        let (adt_def, adt_symbols) = super::define_adt_cvc5_native(
            &tm,
            &mut solver,
            "Option",
            &[("Some", &["value"]), ("None", &[])],
        );

        // Construct Some(42)
        let some_ctor = adt_def
            .constructors
            .iter()
            .find(|c| c.name == "Some")
            .unwrap();
        let none_ctor = adt_def
            .constructors
            .iter()
            .find(|c| c.name == "None")
            .unwrap();

        let mut axioms = Vec::new();
        let mut fresh = 0usize;

        let forty_two = tm.mk_integer(42);
        let some_val = super::adt_constructor_cvc5_native(
            &tm,
            &adt_symbols,
            some_ctor,
            &[forty_two.clone()],
            &mut axioms,
            &mut fresh,
        );
        let none_val = super::adt_constructor_cvc5_native(
            &tm,
            &adt_symbols,
            none_ctor,
            &[],
            &mut axioms,
            &mut fresh,
        );

        // Assert all axioms
        for axiom in &axioms {
            solver.assert_formula(axiom.clone());
        }

        // Verify tags are distinct
        let is_some =
            super::adt_is_constructor_cvc5_native(&tm, &adt_symbols, some_ctor, &some_val);
        let is_none =
            super::adt_is_constructor_cvc5_native(&tm, &adt_symbols, none_ctor, &none_val);
        solver.assert_formula(is_some);
        solver.assert_formula(is_none);

        // Verify accessor: value(some_val) == 42
        let accessed = super::adt_accessor_cvc5_native(&tm, &adt_symbols, "value", &some_val);
        let eq_42 = tm.mk_term(cvc5::Kind::Equal, &[accessed, forty_two]);
        let not_eq_42 = tm.mk_term(cvc5::Kind::Not, &[eq_42]);
        solver.push(1);
        solver.assert_formula(not_eq_42);
        let result = solver.check_sat();
        assert!(
            result.is_unsat(),
            "accessor(Some(42)) must equal 42 (negation should be UNSAT)"
        );
        solver.pop(1);

        // Verify exhaustiveness: tag(x) == 99 should be UNSAT
        let x = tm.mk_const(tm.integer_sort(), "x_adt_exh");
        let tag_x = tm.mk_term(cvc5::Kind::ApplyUf, &[adt_symbols.tag_fn.clone(), x]);
        let bad_tag = tm.mk_term(cvc5::Kind::Equal, &[tag_x, tm.mk_integer(99)]);
        solver.push(1);
        solver.assert_formula(bad_tag);
        let result = solver.check_sat();
        assert!(
            result.is_unsat(),
            "tag(x) == 99 should be UNSAT with only tags 0 and 1"
        );
        solver.pop(1);
    }

    #[test]
    fn test_cvc5_adt_smtlib_generation() {
        // Test that the SMT-LIB2 generation functions produce valid output
        let (adt_def, assertions) =
            super::define_adt_cvc5("Option", &[("Some", &["value"]), ("None", &[])]);

        // Should have 3 declarations + 1 exhaustiveness + 2 injectivity = 6
        assert!(
            assertions.len() >= 5,
            "should have at least 5 SMT-LIB2 assertions, got {}",
            assertions.len()
        );

        // Check tag function declaration
        assert!(
            assertions.iter().any(|a| a.contains("__adt_tag_Option")),
            "should declare tag function"
        );

        // Check accessor function declaration
        assert!(
            assertions.iter().any(|a| a.contains("__adt_Option_value")),
            "should declare value accessor"
        );

        // Check exhaustiveness axiom
        assert!(
            assertions
                .iter()
                .any(|a| a.contains("forall") && a.contains("or")),
            "should have exhaustiveness axiom with forall/or"
        );

        // Test constructor tester SMT generation
        let tester = super::adt_is_constructor_smt("Option", "Some", "x", &adt_def);
        assert_eq!(tester, "(= (__adt_tag_Option x) 0)");

        let tester_none = super::adt_is_constructor_smt("Option", "None", "x", &adt_def);
        assert_eq!(tester_none, "(= (__adt_tag_Option x) 1)");

        // Test accessor SMT generation
        let acc = super::adt_accessor_smt("Option", "value", "x");
        assert_eq!(acc, "(__adt_Option_value x)");
    }

    // -------------------------------------------------------------------
    // #265: CVC5 bitvector wrapping test
    // -------------------------------------------------------------------

    #[test]
    fn test_cvc5_unsat_core_extraction() {
        use assura_ast::{BinOp, Literal};

        let int_lit = |n: &str| Spanned::no_span(Expr::Literal(Literal::Int(n.into())));
        let var = |name: &str| Spanned::no_span(Expr::Ident(name.into()));
        let cmp = |name: &str, op: BinOp, n: &str| {
            Spanned::no_span(Expr::BinOp {
                lhs: Box::new(var(name)),
                op,
                rhs: Box::new(int_lit(n)),
            })
        };

        let req0 = cmp("x", BinOp::Gt, "50");
        let req1 = cmp("x", BinOp::Lt, "100");
        let ensures = cmp("x", BinOp::Gt, "10");

        let result = check_validity_cvc5("unsat_core_test", &[&req0, &req1], &ensures);
        match result {
            VerificationResult::Verified { unsat_core, .. } => {
                let core = unsat_core
                    .as_ref()
                    .expect("CVC5 verified result should include unsat core");
                assert!(
                    core.iter().any(|l| l.contains("req_0")),
                    "core should include req_0, got: {core:?}"
                );
            }
            other => panic!("expected verified result, got: {other:?}"),
        }
    }

    #[test]
    fn test_cvc5_bitvector_wrapping() {
        let tm = cvc5::TermManager::new();
        let mut solver = cvc5::Solver::new(&tm);
        solver.set_logic("QF_BV");
        solver.set_option("produce-models", "true");

        let eight = tm.mk_bv_sort(8);
        let a = tm.mk_const(eight.clone(), "a");
        let b = tm.mk_const(eight, "b");
        let two_five_five = tm.mk_bv(8, 255);
        let one = tm.mk_bv(8, 1);
        let zero = tm.mk_bv(8, 0);

        solver.assert_formula(tm.mk_term(cvc5::Kind::Equal, &[a.clone(), two_five_five]));
        solver.assert_formula(tm.mk_term(cvc5::Kind::Equal, &[b.clone(), one]));
        let sum = tm.mk_term(cvc5::Kind::BitvectorAdd, &[a, b]);
        solver.assert_formula(tm.mk_term(cvc5::Kind::Equal, &[sum, zero]));

        assert!(
            solver.check_sat().is_sat(),
            "255 + 1 should wrap to 0 in 8-bit BV"
        );
    }
}
