use super::*;

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
