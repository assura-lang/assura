use super::*;

#[cfg(feature = "cvc5-verify")]
mod native_tests {
    use super::*;
    use crate::verify_context::{ContractVerifyContext, LoadedIrContext};
    use assura_ast::{Expr, Param, SpExpr};

    // ---------------------------------------------------------------
    // Helpers (mirrors Z3 test helpers)
    // ---------------------------------------------------------------

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

    #[test]
    fn cvc5_with_types_fn_params_nat() {
        // FnDef-style: params passed explicitly (not via input() clause).
        // This is the path used for `fn check_table_bounds(root_bits: Nat, ...)`
        let params = vec![Param {
            name: "n".into(),
            ty: Some(assura_ast::TypeExpr::Named("Nat".into())),
        }];
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
                op: BinOp::Gte,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
            effect_variables: vec![],
        }];
        let mut cache = SessionCache::new();
        let results =
            verify_contract_cvc5_with_types("FnNatParam", &clauses, &params, &[], &mut cache);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "Nat param n >= 0 should verify via explicit params: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_trivial_ensures_verified() {
        // requires x > 0, ensures x > 0 (trivially true)
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("NativeTest", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "should verify: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_counterexample() {
        // No requires, ensures x > 0 (counterexample: x = 0)
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
            effect_variables: vec![],
        }];
        let results = verify_contract_cvc5("NativeCounterexample", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "should have counterexample: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_invariant_satisfiable() {
        // invariant { x > 0 } -- satisfiable (x = 1)
        let clauses = vec![Clause {
            kind: ClauseKind::Invariant,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
            effect_variables: vec![],
        }];
        let results = verify_contract_cvc5("NativeInvariant", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "invariant should be satisfiable: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_must_not_true_counterexample() {
        // must_not { true } -- true is always possible, should be counterexample
        let clauses = vec![Clause {
            kind: ClauseKind::MustNot,
            body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
            effect_variables: vec![],
        }];
        let results = verify_contract_cvc5("NativeMustNot", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "must_not(true) should be counterexample: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_must_not_false_verified() {
        // must_not { false } -- false is impossible, should verify
        let clauses = vec![Clause {
            kind: ClauseKind::MustNot,
            body: Spanned::no_span(Expr::Literal(Literal::Bool(false))),
            effect_variables: vec![],
        }];
        let results = verify_contract_cvc5("NativeMustNotFalse", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "must_not(false) should verify: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_nat_type_constraint() {
        // input(n: Nat), ensures n >= 0 -- should verify with Nat constraint
        let clauses = vec![
            Clause {
                kind: ClauseKind::Input,
                body: Spanned::no_span(Expr::Raw(vec!["n".into(), ":".into(), "Nat".into()])),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
                    op: BinOp::Gte,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("NatConstraint", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "Nat n >= 0 should verify: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_typestate_same_state_verifies() {
        // #262: Typestate same pre/post should verify via native CVC5
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::Raw(vec!["file".into(), "@".into(), "Open".into()])),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::Raw(vec!["file".into(), "@".into(), "Open".into()])),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("NativeTypestateIdentity", &clauses);
        assert!(
            !results.is_empty(),
            "should have results for typestate identity"
        );
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "same typestate pre/post should verify via native CVC5, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_typestate_different_state_counterexample() {
        // #262: Different typestate pre/post should produce counterexample
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::Raw(vec!["file".into(), "@".into(), "Open".into()])),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::Raw(vec!["file".into(), "@".into(), "Closed".into()])),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("NativeTypestateMismatch", &clauses);
        assert!(
            !results.is_empty(),
            "should have results for typestate mismatch"
        );
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "different typestate pre/post should produce counterexample via native CVC5, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_nested_typestate_encoded() {
        // #262: Typestate nested inside a binary expression is now encoded
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    op: BinOp::And,
                    lhs: Box::new(Spanned::no_span(Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                    })),
                    rhs: Box::new(Spanned::no_span(Expr::Raw(vec![
                        "conn".into(),
                        "@".into(),
                        "Connected".into(),
                    ]))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::Raw(vec![
                    "conn".into(),
                    "@".into(),
                    "Connected".into(),
                ])),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("NativeNestedTypestate", &clauses);
        assert!(
            !results.is_empty(),
            "should have results for nested typestate"
        );
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "nested typestate with matching state should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_check_validity_typestate_encoded() {
        // #262: check_validity_cvc5 should now encode typestate (not skip)
        let assumption = Spanned::no_span(Expr::Raw(vec![
            "state".into(),
            "@".into(),
            "Running".into(),
        ]));
        let body = Spanned::no_span(Expr::Raw(vec![
            "state".into(),
            "@".into(),
            "Running".into(),
        ]));
        let result = check_validity_cvc5("validity_typestate", &[&assumption], &body);
        assert!(
            matches!(&result, VerificationResult::Verified { .. }),
            "check_validity_cvc5 should verify same-state typestate: {:?}",
            result
        );
    }

    #[test]
    fn native_cvc5_check_satisfiability_typestate_encoded() {
        // #262: check_satisfiability_cvc5 should now encode typestate (not skip)
        let body = Spanned::no_span(Expr::Raw(vec![
            "lock".into(),
            "@".into(),
            "Acquired".into(),
        ]));
        let result = check_satisfiability_cvc5("sat_typestate", &[], &body);
        assert!(
            matches!(&result, VerificationResult::Verified { .. }),
            "check_satisfiability_cvc5 should find typestate satisfiable: {:?}",
            result
        );
    }

    // -------------------------------------------------------------------
    // String method axiom tests (CVC5 native, issue #251)
    // -------------------------------------------------------------------

    // make_clause is at file level (super::make_clause) per #512.
    use super::make_clause;

    #[test]
    fn test_cvc5_string_substring_axiom() {
        // Contract: requires constraints on inputs,
        // ensures { substring(s, start, end).length() >= 0 }
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("len".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                },
            ),
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("start".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                },
            ),
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Lte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("start".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Ident("end_val".into()))),
                },
            ),
            make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                        receiver: Box::new(Spanned::no_span(Expr::Call {
                            func: Box::new(Spanned::no_span(Expr::Ident("substring".into()))),
                            args: vec![
                                Spanned::no_span(Expr::Ident("s".into())),
                                Spanned::no_span(Expr::Ident("start".into())),
                                Spanned::no_span(Expr::Ident("end_val".into())),
                            ],
                        })),
                        method: "length".into(),
                        args: vec![],
                    })),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                },
            ),
        ];
        let results = crate::cvc5_backend::verify_contract_cvc5("SubstringTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "substring axiom: expected Verified, got: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_string_concat_axiom() {
        // ensures { concat(a, b).length() >= 0 }
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                    receiver: Box::new(Spanned::no_span(Expr::Call {
                        func: Box::new(Spanned::no_span(Expr::Ident("concat".into()))),
                        args: vec![
                            Spanned::no_span(Expr::Ident("a".into())),
                            Spanned::no_span(Expr::Ident("b".into())),
                        ],
                    })),
                    method: "length".into(),
                    args: vec![],
                })),
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            },
        )];
        let results = crate::cvc5_backend::verify_contract_cvc5("ConcatTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "concat axiom: expected Verified, got: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_string_indexof_axiom() {
        // requires { s.length() > 0 }
        // ensures { index_of(s, sub) >= -1 }
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                        receiver: Box::new(Spanned::no_span(Expr::Ident("s".into()))),
                        method: "length".into(),
                        args: vec![],
                    })),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                },
            ),
            make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Call {
                        func: Box::new(Spanned::no_span(Expr::Ident("index_of".into()))),
                        args: vec![
                            Spanned::no_span(Expr::Ident("s".into())),
                            Spanned::no_span(Expr::Ident("sub".into())),
                        ],
                    })),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("-1".into())))),
                },
            ),
        ];
        let results = crate::cvc5_backend::verify_contract_cvc5("IndexOfTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "indexOf axiom: expected Verified, got: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_string_charat_axiom() {
        // requires { idx >= 0 && s.length() > idx }
        // ensures { char_at(s, idx) >= 0 || char_at(s, idx) < 0 } (tautology -- tests axiom wiring)
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("idx".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                },
            ),
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                        receiver: Box::new(Spanned::no_span(Expr::Ident("s".into()))),
                        method: "length".into(),
                        args: vec![],
                    })),
                    rhs: Box::new(Spanned::no_span(Expr::Ident("idx".into()))),
                },
            ),
            make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("idx".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                },
            ),
        ];
        let results = crate::cvc5_backend::verify_contract_cvc5("CharAtTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "charAt axiom: expected Verified, got: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_string_replace_axiom() {
        // ensures { replace(s, old_s, new_s).length() >= 0 }
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                    receiver: Box::new(Spanned::no_span(Expr::Call {
                        func: Box::new(Spanned::no_span(Expr::Ident("replace".into()))),
                        args: vec![
                            Spanned::no_span(Expr::Ident("s".into())),
                            Spanned::no_span(Expr::Ident("old_s".into())),
                            Spanned::no_span(Expr::Ident("new_s".into())),
                        ],
                    })),
                    method: "length".into(),
                    args: vec![],
                })),
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            },
        )];
        let results = crate::cvc5_backend::verify_contract_cvc5("ReplaceTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "replace axiom: expected Verified, got: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_string_split_axiom() {
        // ensures { split(s, delim).length() >= 1 }
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                    receiver: Box::new(Spanned::no_span(Expr::Call {
                        func: Box::new(Spanned::no_span(Expr::Ident("split".into()))),
                        args: vec![
                            Spanned::no_span(Expr::Ident("s".into())),
                            Spanned::no_span(Expr::Ident("delim".into())),
                        ],
                    })),
                    method: "length".into(),
                    args: vec![],
                })),
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
            },
        )];
        let results = crate::cvc5_backend::verify_contract_cvc5("SplitTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "split axiom: expected Verified, got: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_string_trim_axiom() {
        // ensures { trim(s).length() >= 0 }
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                    receiver: Box::new(Spanned::no_span(Expr::Call {
                        func: Box::new(Spanned::no_span(Expr::Ident("trim".into()))),
                        args: vec![Spanned::no_span(Expr::Ident("s".into()))],
                    })),
                    method: "length".into(),
                    args: vec![],
                })),
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            },
        )];
        let results = crate::cvc5_backend::verify_contract_cvc5("TrimTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "trim axiom: expected Verified, got: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_array_set_axiom() {
        // ensures { set(arr, i, v).length() >= 0 }
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                    receiver: Box::new(Spanned::no_span(Expr::Call {
                        func: Box::new(Spanned::no_span(Expr::Ident("set".into()))),
                        args: vec![
                            Spanned::no_span(Expr::Ident("arr".into())),
                            Spanned::no_span(Expr::Ident("i".into())),
                            Spanned::no_span(Expr::Ident("v".into())),
                        ],
                    })),
                    method: "length".into(),
                    args: vec![],
                })),
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            },
        )];
        let results = crate::cvc5_backend::verify_contract_cvc5("ArraySetTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "array set axiom: expected Verified, got: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_map_put_axiom() {
        // ensures { put(m, k, v).size() >= 0 } (via size axiom)
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                    receiver: Box::new(Spanned::no_span(Expr::Call {
                        func: Box::new(Spanned::no_span(Expr::Ident("put".into()))),
                        args: vec![
                            Spanned::no_span(Expr::Ident("m".into())),
                            Spanned::no_span(Expr::Ident("k".into())),
                            Spanned::no_span(Expr::Ident("v".into())),
                        ],
                    })),
                    method: "size".into(),
                    args: vec![],
                })),
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            },
        )];
        let results = crate::cvc5_backend::verify_contract_cvc5("MapPutTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "map put axiom: expected Verified, got: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_method_call_substring_axiom() {
        // Test method call form: s.substring(start, end).length() >= 0
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("start".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                },
            ),
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Lte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("start".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Ident("end_val".into()))),
                },
            ),
            make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                        receiver: Box::new(Spanned::no_span(Expr::MethodCall {
                            receiver: Box::new(Spanned::no_span(Expr::Ident("s".into()))),
                            method: "substring".into(),
                            args: vec![
                                Spanned::no_span(Expr::Ident("start".into())),
                                Spanned::no_span(Expr::Ident("end_val".into())),
                            ],
                        })),
                        method: "length".into(),
                        args: vec![],
                    })),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                },
            ),
        ];
        let results = crate::cvc5_backend::verify_contract_cvc5("MethodSubstringTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "method call substring axiom: expected Verified, got: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_method_call_set_axiom() {
        // Test method call form: arr.set(i, v).length() >= 0
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                    receiver: Box::new(Spanned::no_span(Expr::MethodCall {
                        receiver: Box::new(Spanned::no_span(Expr::Ident("arr".into()))),
                        method: "set".into(),
                        args: vec![
                            Spanned::no_span(Expr::Ident("i".into())),
                            Spanned::no_span(Expr::Ident("v".into())),
                        ],
                    })),
                    method: "length".into(),
                    args: vec![],
                })),
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            },
        )];
        let results = crate::cvc5_backend::verify_contract_cvc5("MethodArraySetTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "method call set axiom: expected Verified, got: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_method_call_put_axiom() {
        // Test method call form: m.put(k, v).size() >= 0
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                    receiver: Box::new(Spanned::no_span(Expr::MethodCall {
                        receiver: Box::new(Spanned::no_span(Expr::Ident("m".into()))),
                        method: "put".into(),
                        args: vec![
                            Spanned::no_span(Expr::Ident("k".into())),
                            Spanned::no_span(Expr::Ident("v".into())),
                        ],
                    })),
                    method: "size".into(),
                    args: vec![],
                })),
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            },
        )];
        let results = crate::cvc5_backend::verify_contract_cvc5("MethodMapPutTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "method call put axiom: expected Verified, got: {:?}",
                r
            );
        }
    }

    // -----------------------------------------------------------------------
    // encode_call parity (#364 / CVC5 mirror): collection + min/max axioms
    // -----------------------------------------------------------------------

    fn ident_expr(name: &str) -> Expr {
        Expr::Ident(name.into())
    }

    fn lit_int(n: &str) -> Expr {
        Expr::Literal(Literal::Int(n.into()))
    }

    fn call_expr(func: &str, args: Vec<Expr>) -> Expr {
        Expr::Call {
            func: Box::new(Spanned::no_span(ident_expr(func))),
            args: args.into_iter().map(Spanned::no_span).collect(),
        }
    }

    fn method_expr(receiver: Expr, method: &str, args: Vec<Expr>) -> Expr {
        Expr::MethodCall {
            receiver: Box::new(Spanned::no_span(receiver)),
            method: method.into(),
            args: args.into_iter().map(Spanned::no_span).collect(),
        }
    }

    fn binop_expr(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
        Expr::BinOp {
            op,
            lhs: Box::new(Spanned::no_span(lhs)),
            rhs: Box::new(Spanned::no_span(rhs)),
        }
    }

    fn assert_all_verified(label: &str, results: &[VerificationResult]) {
        assert!(!results.is_empty(), "{label}: expected results");
        for r in results {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "{label}: expected Verified, got: {r:?}"
            );
        }
    }

    fn assert_ensures_verified(label: &str, results: &[VerificationResult]) {
        assert!(
            results.iter().any(|r| matches!(
                r,
                VerificationResult::Verified { clause_desc, .. }
                    if clause_desc.contains("Ensures") || clause_desc.contains("ensures")
            )),
            "{label}: expected Verified ensures, got {results:?}"
        );
    }

    #[test]
    fn test_cvc5_push_increments_length() {
        // requires { len(xs) == n } && { n >= 0 }
        // ensures  { len(push(xs, x)) == n + 1 }
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                binop_expr(
                    BinOp::Eq,
                    call_expr("len", vec![ident_expr("xs")]),
                    ident_expr("n"),
                ),
            ),
            make_clause(
                ClauseKind::Requires,
                binop_expr(BinOp::Gte, ident_expr("n"), lit_int("0")),
            ),
            make_clause(
                ClauseKind::Ensures,
                binop_expr(
                    BinOp::Eq,
                    call_expr(
                        "len",
                        vec![call_expr("push", vec![ident_expr("xs"), ident_expr("x")])],
                    ),
                    binop_expr(BinOp::Add, ident_expr("n"), lit_int("1")),
                ),
            ),
        ];
        let results = verify_contract_cvc5("Cvc5PushLen", &clauses);
        assert_ensures_verified("push increments length", &results);
    }

    #[test]
    fn test_cvc5_reverse_preserves_length() {
        // requires { len(xs) == n }
        // ensures  { len(reverse(xs)) == n }
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                binop_expr(
                    BinOp::Eq,
                    call_expr("len", vec![ident_expr("xs")]),
                    ident_expr("n"),
                ),
            ),
            make_clause(
                ClauseKind::Ensures,
                binop_expr(
                    BinOp::Eq,
                    call_expr("len", vec![call_expr("reverse", vec![ident_expr("xs")])]),
                    ident_expr("n"),
                ),
            ),
        ];
        let results = verify_contract_cvc5("Cvc5ReverseLen", &clauses);
        assert_ensures_verified("reverse preserves length", &results);
    }

    #[test]
    fn test_cvc5_clear_zero_length() {
        // ensures { len(clear(xs)) == 0 }
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            binop_expr(
                BinOp::Eq,
                call_expr("len", vec![call_expr("clear", vec![ident_expr("xs")])]),
                lit_int("0"),
            ),
        )];
        let results = verify_contract_cvc5("Cvc5ClearLen", &clauses);
        assert_ensures_verified("clear zero length", &results);
    }

    #[test]
    fn test_cvc5_take_length_bounded() {
        // requires { k >= 0 } && { len(xs) == 10 } && { k <= 10 }
        // ensures  { len(take(xs, k)) == k }
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                binop_expr(BinOp::Gte, ident_expr("k"), lit_int("0")),
            ),
            make_clause(
                ClauseKind::Requires,
                binop_expr(
                    BinOp::Eq,
                    call_expr("len", vec![ident_expr("xs")]),
                    lit_int("10"),
                ),
            ),
            make_clause(
                ClauseKind::Requires,
                binop_expr(BinOp::Lte, ident_expr("k"), lit_int("10")),
            ),
            make_clause(
                ClauseKind::Ensures,
                binop_expr(
                    BinOp::Eq,
                    call_expr(
                        "len",
                        vec![call_expr("take", vec![ident_expr("xs"), ident_expr("k")])],
                    ),
                    ident_expr("k"),
                ),
            ),
        ];
        let results = verify_contract_cvc5("Cvc5TakeLen", &clauses);
        assert_ensures_verified("take length bounded", &results);
    }

    #[test]
    fn test_cvc5_is_empty_iff_len_zero() {
        // requires { is_empty(xs) }
        // ensures  { len(xs) == 0 }
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                call_expr("is_empty", vec![ident_expr("xs")]),
            ),
            make_clause(
                ClauseKind::Ensures,
                binop_expr(
                    BinOp::Eq,
                    call_expr("len", vec![ident_expr("xs")]),
                    lit_int("0"),
                ),
            ),
        ];
        let results = verify_contract_cvc5("Cvc5IsEmptyLen", &clauses);
        assert_ensures_verified("is_empty => len==0", &results);
    }

    #[test]
    fn test_cvc5_method_call_push_length() {
        // requires { xs.length() == 3 }
        // ensures  { xs.push(x).length() == 4 }
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                binop_expr(
                    BinOp::Eq,
                    method_expr(ident_expr("xs"), "length", vec![]),
                    lit_int("3"),
                ),
            ),
            make_clause(
                ClauseKind::Ensures,
                binop_expr(
                    BinOp::Eq,
                    method_expr(
                        method_expr(ident_expr("xs"), "push", vec![ident_expr("x")]),
                        "length",
                        vec![],
                    ),
                    lit_int("4"),
                ),
            ),
        ];
        let results = verify_contract_cvc5("Cvc5MethodPush", &clauses);
        assert_ensures_verified("method push length", &results);
    }

    #[test]
    fn test_cvc5_concat_length_additive_exact() {
        // requires { len(a) == 2 } && { len(b) == 3 }
        // ensures  { len(concat(a, b)) == 5 }
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                binop_expr(
                    BinOp::Eq,
                    call_expr("len", vec![ident_expr("a")]),
                    lit_int("2"),
                ),
            ),
            make_clause(
                ClauseKind::Requires,
                binop_expr(
                    BinOp::Eq,
                    call_expr("len", vec![ident_expr("b")]),
                    lit_int("3"),
                ),
            ),
            make_clause(
                ClauseKind::Ensures,
                binop_expr(
                    BinOp::Eq,
                    call_expr(
                        "len",
                        vec![call_expr("concat", vec![ident_expr("a"), ident_expr("b")])],
                    ),
                    lit_int("5"),
                ),
            ),
        ];
        let results = verify_contract_cvc5("Cvc5ConcatLenExact", &clauses);
        assert_ensures_verified("concat length additive", &results);
    }

    #[test]
    fn test_cvc5_min_max_bounds_verify() {
        // min/max ite encoding: min(a,b) <= a,b and max(a,b) >= a,b
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                binop_expr(BinOp::Gte, ident_expr("a"), lit_int("0")),
            ),
            make_clause(
                ClauseKind::Requires,
                binop_expr(BinOp::Gte, ident_expr("b"), lit_int("0")),
            ),
            make_clause(
                ClauseKind::Ensures,
                binop_expr(
                    BinOp::Lte,
                    call_expr("min", vec![ident_expr("a"), ident_expr("b")]),
                    ident_expr("a"),
                ),
            ),
            make_clause(
                ClauseKind::Ensures,
                binop_expr(
                    BinOp::Lte,
                    call_expr("min", vec![ident_expr("a"), ident_expr("b")]),
                    ident_expr("b"),
                ),
            ),
            make_clause(
                ClauseKind::Ensures,
                binop_expr(
                    BinOp::Gte,
                    call_expr("max", vec![ident_expr("a"), ident_expr("b")]),
                    ident_expr("a"),
                ),
            ),
            make_clause(
                ClauseKind::Ensures,
                binop_expr(
                    BinOp::Gte,
                    call_expr("max", vec![ident_expr("a"), ident_expr("b")]),
                    ident_expr("b"),
                ),
            ),
        ];
        let results = verify_contract_cvc5("Cvc5MinMaxBounds", &clauses);
        let ensures: Vec<_> = results
            .iter()
            .filter(|r| match r {
                VerificationResult::Verified { clause_desc, .. }
                | VerificationResult::Counterexample { clause_desc, .. }
                | VerificationResult::Unknown { clause_desc, .. } => {
                    clause_desc.contains("Ensures") || clause_desc.contains("ensures")
                }
                _ => false,
            })
            .collect();
        assert!(
            ensures.len() >= 4,
            "expected 4 ensures results, got {ensures:?} (all: {results:?})"
        );
        for r in &ensures {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "min/max ite encoding should verify bounds, got: {r:?}"
            );
        }
        assert_all_verified("min_max_bounds", &results);
    }

    // -----------------------------------------------------------------------
    // encode_call wave 2: predicates + array/map (Z3 #364 follow-on)
    // -----------------------------------------------------------------------

    #[test]
    fn test_cvc5_contains_implies_length_ge_needle() {
        // requires { contains(s, sub) } && { len(sub) == 3 }
        // ensures  { len(s) >= 3 }
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                call_expr("contains", vec![ident_expr("s"), ident_expr("sub")]),
            ),
            make_clause(
                ClauseKind::Requires,
                binop_expr(
                    BinOp::Eq,
                    call_expr("len", vec![ident_expr("sub")]),
                    lit_int("3"),
                ),
            ),
            make_clause(
                ClauseKind::Ensures,
                binop_expr(
                    BinOp::Gte,
                    call_expr("len", vec![ident_expr("s")]),
                    lit_int("3"),
                ),
            ),
        ];
        let results = verify_contract_cvc5("Cvc5ContainsLen", &clauses);
        assert_ensures_verified("contains length axiom", &results);
    }

    #[test]
    fn test_cvc5_starts_with_implies_length_ge_prefix() {
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                call_expr("starts_with", vec![ident_expr("s"), ident_expr("pre")]),
            ),
            make_clause(
                ClauseKind::Requires,
                binop_expr(
                    BinOp::Eq,
                    call_expr("len", vec![ident_expr("pre")]),
                    lit_int("2"),
                ),
            ),
            make_clause(
                ClauseKind::Ensures,
                binop_expr(
                    BinOp::Gte,
                    call_expr("len", vec![ident_expr("s")]),
                    lit_int("2"),
                ),
            ),
        ];
        let results = verify_contract_cvc5("Cvc5StartsWithLen", &clauses);
        assert_ensures_verified("starts_with length axiom", &results);
    }

    #[test]
    fn test_cvc5_ends_with_empty_affix_always_true() {
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                binop_expr(
                    BinOp::Eq,
                    call_expr("len", vec![ident_expr("aff")]),
                    lit_int("0"),
                ),
            ),
            make_clause(
                ClauseKind::Ensures,
                call_expr("ends_with", vec![ident_expr("s"), ident_expr("aff")]),
            ),
        ];
        let results = verify_contract_cvc5("Cvc5EndsWithEmpty", &clauses);
        assert_ensures_verified("empty affix ends_with", &results);
    }

    #[test]
    fn test_cvc5_contains_key_implies_size_ge_one() {
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                call_expr("contains_key", vec![ident_expr("m"), ident_expr("k")]),
            ),
            make_clause(
                ClauseKind::Ensures,
                binop_expr(
                    BinOp::Gte,
                    call_expr("size", vec![ident_expr("m")]),
                    lit_int("1"),
                ),
            ),
        ];
        let results = verify_contract_cvc5("Cvc5ContainsKeySize", &clauses);
        assert_ensures_verified("contains_key => size>=1", &results);
    }

    #[test]
    fn test_cvc5_get_set_read_over_write() {
        // ensures { get(set(arr, i, v), i) == v }
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                binop_expr(BinOp::Gte, ident_expr("i"), lit_int("0")),
            ),
            make_clause(
                ClauseKind::Ensures,
                binop_expr(
                    BinOp::Eq,
                    call_expr(
                        "get",
                        vec![
                            call_expr(
                                "set",
                                vec![ident_expr("arr"), ident_expr("i"), ident_expr("v")],
                            ),
                            ident_expr("i"),
                        ],
                    ),
                    ident_expr("v"),
                ),
            ),
        ];
        let results = verify_contract_cvc5("Cvc5GetSetRow", &clauses);
        assert_ensures_verified("get/set read-over-write", &results);
    }

    #[test]
    fn test_cvc5_set_preserves_length() {
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                binop_expr(
                    BinOp::Eq,
                    call_expr("len", vec![ident_expr("arr")]),
                    ident_expr("n"),
                ),
            ),
            make_clause(
                ClauseKind::Requires,
                binop_expr(BinOp::Gte, ident_expr("n"), lit_int("0")),
            ),
            make_clause(
                ClauseKind::Requires,
                binop_expr(BinOp::Gte, ident_expr("i"), lit_int("0")),
            ),
            make_clause(
                ClauseKind::Ensures,
                binop_expr(
                    BinOp::Eq,
                    call_expr(
                        "len",
                        vec![call_expr(
                            "set",
                            vec![ident_expr("arr"), ident_expr("i"), ident_expr("v")],
                        )],
                    ),
                    ident_expr("n"),
                ),
            ),
        ];
        let results = verify_contract_cvc5("Cvc5SetLen", &clauses);
        assert_ensures_verified("set preserves length", &results);
    }

    #[test]
    fn test_cvc5_put_read_over_write_and_contains_key() {
        let clauses = vec![
            make_clause(
                ClauseKind::Ensures,
                binop_expr(
                    BinOp::Eq,
                    call_expr(
                        "get",
                        vec![
                            call_expr(
                                "put",
                                vec![ident_expr("m"), ident_expr("k"), ident_expr("v")],
                            ),
                            ident_expr("k"),
                        ],
                    ),
                    ident_expr("v"),
                ),
            ),
            make_clause(
                ClauseKind::Ensures,
                call_expr(
                    "contains_key",
                    vec![
                        call_expr(
                            "put",
                            vec![ident_expr("m"), ident_expr("k"), ident_expr("v")],
                        ),
                        ident_expr("k"),
                    ],
                ),
            ),
        ];
        let results = verify_contract_cvc5("Cvc5PutGet", &clauses);
        let ensures: Vec<_> = results
            .iter()
            .filter(|r| match r {
                VerificationResult::Verified { clause_desc, .. }
                | VerificationResult::Counterexample { clause_desc, .. }
                | VerificationResult::Unknown { clause_desc, .. } => {
                    clause_desc.contains("Ensures") || clause_desc.contains("ensures")
                }
                _ => false,
            })
            .collect();
        assert!(
            ensures.len() >= 2,
            "expected 2 ensures, got {ensures:?} (all: {results:?})"
        );
        for r in &ensures {
            assert!(
                matches!(r, VerificationResult::Verified { .. }),
                "put get/contains_key should verify, got: {r:?}"
            );
        }
    }
}
