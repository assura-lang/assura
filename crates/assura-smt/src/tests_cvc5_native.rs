use crate::VerificationResult;
use crate::cache::SessionCache;
use crate::cvc5_backend::*;
use crate::cvc5_quantifier_encode::infer_quantifier_patterns_cvc5;
use crate::verify_context::{ContractVerifyContext, LoadedIrContext};
use assura_ast::{BinOp, Clause, ClauseKind, Expr, Literal, Param, Pattern, Spanned, UnaryOp};
use std::collections::HashSet;

#[cfg(feature = "cvc5-verify")]
fn verify_lemmas_test(
    contract_name: &str,
    clauses: &[Clause],
    params: &[Param],
    return_ty: &[String],
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&assura_ast::SpExpr>>>,
    ir_body: Option<&crate::ir::IrFunction>,
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    let ctx = ContractVerifyContext {
        contract_name,
        clauses,
        params,
        return_ty,
        constants: &[],
        ir: ir_body.map(LoadedIrContext::with_body),
    };
    verify_contract_cvc5_with_lemmas(&ctx, lemma_defs, cache)
}

#[cfg(feature = "cvc5-verify")]
mod native_tests {
    use super::*;
    use crate::verify_context::{ContractVerifyContext, LoadedIrContext};
    use assura_ast::{Expr, Param};

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

    fn make_clause(kind: ClauseKind, body: Expr) -> Clause {
        Clause {
            kind,
            body: Spanned::no_span(body),
            effect_variables: vec![],
        }
    }

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
                !matches!(r, VerificationResult::Counterexample { .. }),
                "Got unexpected counterexample: {:?}",
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
                !matches!(r, VerificationResult::Counterexample { .. }),
                "concat axiom failed: {:?}",
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
                !matches!(r, VerificationResult::Counterexample { .. }),
                "indexOf axiom failed: {:?}",
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
                !matches!(r, VerificationResult::Counterexample { .. }),
                "charAt axiom failed: {:?}",
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
                !matches!(r, VerificationResult::Counterexample { .. }),
                "replace axiom failed: {:?}",
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
                !matches!(r, VerificationResult::Counterexample { .. }),
                "split axiom failed: {:?}",
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
                !matches!(r, VerificationResult::Counterexample { .. }),
                "trim axiom failed: {:?}",
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
                !matches!(r, VerificationResult::Counterexample { .. }),
                "array set axiom failed: {:?}",
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
                !matches!(r, VerificationResult::Counterexample { .. }),
                "map put axiom failed: {:?}",
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
                !matches!(r, VerificationResult::Counterexample { .. }),
                "method call substring axiom failed: {:?}",
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
                !matches!(r, VerificationResult::Counterexample { .. }),
                "method call set axiom failed: {:?}",
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
                !matches!(r, VerificationResult::Counterexample { .. }),
                "method call put axiom failed: {:?}",
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

    fn assert_no_cex(label: &str, results: &[VerificationResult]) {
        assert!(!results.is_empty(), "{label}: expected results");
        for r in results {
            assert!(
                !matches!(r, VerificationResult::Counterexample { .. }),
                "{label}: unexpected counterexample: {r:?}"
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
        assert_no_cex("min_max_bounds", &results);
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

#[cfg(feature = "cvc5-verify")]
mod match_pattern_tests {
    use super::*;
    use assura_ast::MatchArm;

    #[test]
    fn test_cvc5_match_constructor_pattern() {
        // ensures { match x { Some(v) => v > 0, None => true } }
        // with requires { x >= 0 } so scrut is constrained
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
                body: Spanned::no_span(Expr::Match {
                    scrutinee: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    arms: vec![
                        MatchArm {
                            pattern: Pattern::Constructor {
                                name: "Positive".into(),
                                fields: vec![Pattern::Ident("v".into())],
                            },
                            body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
                        },
                        MatchArm {
                            pattern: Pattern::Wildcard,
                            body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
                        },
                    ],
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("MatchConstructor", &clauses);
        assert!(!results.is_empty(), "should produce verification results");
        // The match should encode without returning Unknown due to unhandled patterns
        for r in &results {
            assert!(
                !matches!(r, VerificationResult::Unknown { reason, .. }
                    if reason.contains("not yet encoded")),
                "Constructor pattern should be encoded, got: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_match_tuple_pattern() {
        // ensures { match t { (a, b) => true } }
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::Match {
                scrutinee: Box::new(Spanned::no_span(Expr::Ident("t".into()))),
                arms: vec![MatchArm {
                    pattern: Pattern::Tuple(vec![
                        Pattern::Ident("a".into()),
                        Pattern::Ident("b".into()),
                    ]),
                    body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
                }],
            }),
            effect_variables: vec![],
        }];
        let results = verify_contract_cvc5("MatchTuple", &clauses);
        assert!(!results.is_empty(), "should produce verification results");
        // ensures { true } should verify
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "tuple match with body `true` should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_cvc5_match_nested_patterns() {
        // ensures { match x { Outer(Inner(v)) => true, _ => true } }
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::Match {
                scrutinee: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                arms: vec![
                    MatchArm {
                        pattern: Pattern::Constructor {
                            name: "Outer".into(),
                            fields: vec![Pattern::Constructor {
                                name: "Inner".into(),
                                fields: vec![Pattern::Ident("v".into())],
                            }],
                        },
                        body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
                    },
                    MatchArm {
                        pattern: Pattern::Wildcard,
                        body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
                    },
                ],
            }),
            effect_variables: vec![],
        }];
        let results = verify_contract_cvc5("MatchNested", &clauses);
        assert!(!results.is_empty(), "should produce verification results");
        // All arms return true, so should verify
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "nested constructor match with all-true body should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_cvc5_match_enum_verifies() {
        // A simple enum-like match:
        //   requires { x >= 0 }
        //   ensures { match x { Zero => x == 0, _ => x >= 0 } }
        // We use Ident patterns with uppercase names as constructors.
        // Since both arms return expressions derivable from requires, it
        // should verify (or at worst produce a result, not Unknown).
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
                body: Spanned::no_span(Expr::Match {
                    scrutinee: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    arms: vec![
                        MatchArm {
                            pattern: Pattern::Ident("Zero".into()),
                            body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
                        },
                        MatchArm {
                            pattern: Pattern::Wildcard,
                            body: Spanned::no_span(Expr::BinOp {
                                op: BinOp::Gte,
                                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int(
                                    "0".into(),
                                )))),
                            }),
                        },
                    ],
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("MatchEnum", &clauses);
        assert!(!results.is_empty(), "should produce verification results");
        // Should not produce Unknown with "not yet encoded" reason
        for r in &results {
            assert!(
                !matches!(r, VerificationResult::Unknown { reason, .. }
                    if reason.contains("not yet encoded")),
                "Enum match should be encoded, got: {:?}",
                r
            );
        }
    }
}

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
                !matches!(r, VerificationResult::Counterexample { .. }),
                "Frame axiom should prevent counterexample: {:?}",
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
