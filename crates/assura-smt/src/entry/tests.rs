//! Entry-point unit tests (jobs, verifiable clauses, evolution helpers).

use super::*;
use crate::{SolverChoice, VerificationResult};
use assura_ast::*;

fn make_clause(kind: ClauseKind) -> Clause {
    Clause {
        kind,
        body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
        effect_variables: vec![],
    }
}

fn make_source(decls: Vec<Decl>) -> SourceFile {
    SourceFile {
        project: None,
        module: None,
        imports: vec![],
        decls: decls
            .into_iter()
            .map(|d| Spanned {
                node: d,
                span: 0..1,
            })
            .collect(),
    }
}

// ---- has_verifiable_clauses tests ----

#[test]
fn has_verifiable_empty_source() {
    let source = make_source(vec![]);
    assert!(!has_verifiable_clauses(&source));
}

#[test]
fn has_verifiable_contract_with_ensures() {
    let source = make_source(vec![Decl::Contract(ContractDecl {
        name: "C".into(),
        type_params: vec![],
        clauses: vec![make_clause(ClauseKind::Ensures)],
        fn_params: vec![],
    })]);
    assert!(has_verifiable_clauses(&source));
}

#[test]
fn has_verifiable_contract_with_only_input() {
    let source = make_source(vec![Decl::Contract(ContractDecl {
        name: "C".into(),
        type_params: vec![],
        clauses: vec![make_clause(ClauseKind::Input)],
        fn_params: vec![],
    })]);
    assert!(!has_verifiable_clauses(&source));
}

#[test]
fn has_verifiable_fndef_with_requires() {
    let source = make_source(vec![Decl::FnDef(FnDef {
        name: "f".into(),
        is_ghost: false,
        is_lemma: false,
        params: vec![],
        return_ty: None,
        clauses: vec![make_clause(ClauseKind::Requires)],
    })]);
    assert!(has_verifiable_clauses(&source));
}

#[test]
fn has_verifiable_extern_with_invariant() {
    let source = make_source(vec![Decl::Extern(ExternDecl {
        name: "e".into(),
        params: vec![],
        return_ty: None,
        clauses: vec![make_clause(ClauseKind::Invariant)],
    })]);
    assert!(has_verifiable_clauses(&source));
}

#[test]
fn has_verifiable_service_operation() {
    let source = make_source(vec![Decl::Service(ServiceDecl {
        name: "S".into(),
        items: vec![ServiceItem::Operation {
            name: "op".into(),
            clauses: vec![make_clause(ClauseKind::Ensures)],
        }],
    })]);
    assert!(has_verifiable_clauses(&source));
}

#[test]
fn has_verifiable_service_invariant() {
    let source = make_source(vec![Decl::Service(ServiceDecl {
        name: "S".into(),
        items: vec![ServiceItem::Invariant(Spanned::no_span(Expr::Literal(
            Literal::Bool(true),
        )))],
    })]);
    assert!(has_verifiable_clauses(&source));
}

#[test]
fn has_verifiable_service_query_no_clauses() {
    let source = make_source(vec![Decl::Service(ServiceDecl {
        name: "S".into(),
        items: vec![ServiceItem::Query {
            name: "q".into(),
            clauses: vec![],
        }],
    })]);
    assert!(!has_verifiable_clauses(&source));
}

#[test]
fn has_verifiable_block_with_ensures() {
    let source = make_source(vec![Decl::Block {
        kind: BlockKind::Axiomatic,
        name: "b".into(),
        value: None,
        body: vec![make_clause(ClauseKind::Ensures)],
    }]);
    assert!(has_verifiable_clauses(&source));
}

#[test]
fn has_verifiable_bind_with_requires() {
    let source = make_source(vec![Decl::Bind(BindDecl {
        name: "bd".into(),
        target_path: "path".into(),
        params: vec![],
        return_ty: None,
        clauses: vec![make_clause(ClauseKind::Requires)],
    })]);
    assert!(has_verifiable_clauses(&source));
}

#[test]
fn has_verifiable_typedef_enum_prophecy() {
    let source = make_source(vec![
        Decl::TypeDef(TypeDef {
            name: "T".into(),
            type_params: vec![],
            body: TypeBody::Alias(vec!["Int".into()]),
        }),
        Decl::EnumDef(EnumDef {
            name: "E".into(),
            type_params: vec![],
            variants: vec![],
        }),
        Decl::Prophecy(ProphecyDecl {
            name: "p".into(),
            ty: None,
        }),
    ]);
    assert!(!has_verifiable_clauses(&source));
}

// ---- verify_contract tests ----

#[test]
fn verify_contract_no_clauses() {
    let results = verify_contract("Test", &[]);
    assert!(results.is_empty());
}

#[test]
fn verify_contract_input_only() {
    let results = verify_contract("Test", &[make_clause(ClauseKind::Input)]);
    assert!(results.is_empty());
}

#[test]
fn verify_contract_ensures_returns_result() {
    let results = verify_contract("Test", &[make_clause(ClauseKind::Ensures)]);
    assert_eq!(results.len(), 1);
}

#[test]
fn verify_contract_with_requires_and_ensures() {
    let results = verify_contract(
        "Test",
        &[
            make_clause(ClauseKind::Requires),
            make_clause(ClauseKind::Ensures),
        ],
    );
    // Only the ensures clause produces a verification result
    assert_eq!(results.len(), 1);
}

#[test]
fn verify_contract_multiple_ensures() {
    let results = verify_contract(
        "Test",
        &[
            make_clause(ClauseKind::Ensures),
            make_clause(ClauseKind::Invariant),
            make_clause(ClauseKind::Rule),
        ],
    );
    assert_eq!(results.len(), 3);
}

#[test]
fn verify_contract_cvc5_solver() {
    let results = verify_contract_with_solver(
        "Test",
        &[make_clause(ClauseKind::Ensures)],
        SolverChoice::Cvc5,
    );
    assert_eq!(results.len(), 1);
}

#[test]
fn verify_contract_portfolio_solver() {
    let results = verify_contract_with_solver(
        "Test",
        &[make_clause(ClauseKind::Ensures)],
        SolverChoice::Portfolio,
    );
    assert_eq!(results.len(), 1);
}

#[test]
fn verify_contract_decreases() {
    let results = verify_contract("Test", &[make_clause(ClauseKind::Decreases)]);
    assert_eq!(results.len(), 1);
}

#[test]
fn verify_contract_must_not() {
    let results = verify_contract("Test", &[make_clause(ClauseKind::MustNot)]);
    assert_eq!(results.len(), 1);
}

#[test]
fn verify_contract_clause_desc_format() {
    let results = verify_contract("MyContract", &[make_clause(ClauseKind::Ensures)]);
    assert_eq!(results.len(), 1);
    // The description should contain the contract name
    match &results[0] {
        VerificationResult::Verified { clause_desc, .. }
        | VerificationResult::Counterexample { clause_desc, .. }
        | VerificationResult::Timeout { clause_desc }
        | VerificationResult::Unknown { clause_desc, .. } => {
            assert!(
                clause_desc.contains("MyContract"),
                "clause_desc should contain contract name: {clause_desc}"
            );
        }
    }
}

// ---- extract_output_return_type tests ----

#[test]
fn extract_output_return_type_nat() {
    let clauses = vec![Clause {
        kind: ClauseKind::Output,
        body: Spanned::no_span(Expr::Raw(vec!["result".into(), ":".into(), "Nat".into()])),
        effect_variables: vec![],
    }];
    assert_eq!(extract_output_return_type(&clauses), vec!["Nat"]);
}

#[test]
fn extract_output_return_type_complex() {
    let clauses = vec![Clause {
        kind: ClauseKind::Output,
        body: Spanned::no_span(Expr::Raw(vec![
            "result".into(),
            ":".into(),
            "List".into(),
            "<".into(),
            "Int".into(),
            ">".into(),
        ])),
        effect_variables: vec![],
    }];
    assert_eq!(
        extract_output_return_type(&clauses),
        vec!["List", "<", "Int", ">"]
    );
}

#[test]
fn extract_output_return_type_no_colon_fallback() {
    // Fallback path: tokens without ":" at position 1 are returned as-is
    let clauses = vec![Clause {
        kind: ClauseKind::Output,
        body: Spanned::no_span(Expr::Raw(vec!["Nat".into()])),
        effect_variables: vec![],
    }];
    assert_eq!(extract_output_return_type(&clauses), vec!["Nat"]);
}

#[test]
fn extract_output_return_type_missing() {
    let clauses = vec![Clause {
        kind: ClauseKind::Requires,
        body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
        effect_variables: vec![],
    }];
    assert!(extract_output_return_type(&clauses).is_empty());
}

#[test]
fn extract_output_return_type_non_raw_body() {
    // Output clause with non-Raw body (should be skipped)
    let clauses = vec![Clause {
        kind: ClauseKind::Output,
        body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
        effect_variables: vec![],
    }];
    assert!(extract_output_return_type(&clauses).is_empty());
}

// ---- extract_input_params tests ----

#[test]
fn extract_input_params_single() {
    let clauses = vec![Clause {
        kind: ClauseKind::Input,
        body: Spanned::no_span(Expr::Raw(vec![
            "raw_data".into(),
            ":".into(),
            "Bytes".into(),
        ])),
        effect_variables: vec![],
    }];
    let params = extract_input_params(&clauses);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "raw_data");
    assert_eq!(params[0].ty, Some(TypeExpr::Named("Bytes".into())));
}

#[test]
fn extract_input_params_multiple() {
    let clauses = vec![Clause {
        kind: ClauseKind::Input,
        body: Spanned::no_span(Expr::Raw(vec![
            "x".into(),
            ":".into(),
            "Int".into(),
            ",".into(),
            "y".into(),
            ":".into(),
            "Nat".into(),
        ])),
        effect_variables: vec![],
    }];
    let params = extract_input_params(&clauses);
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "x");
    assert_eq!(params[0].ty, Some(TypeExpr::Named("Int".into())));
    assert_eq!(params[1].name, "y");
    assert_eq!(params[1].ty, Some(TypeExpr::Named("Nat".into())));
}

#[test]
fn extract_input_params_no_type() {
    // Parameter without a type annotation
    let clauses = vec![Clause {
        kind: ClauseKind::Input,
        body: Spanned::no_span(Expr::Raw(vec!["x".into()])),
        effect_variables: vec![],
    }];
    let params = extract_input_params(&clauses);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "x");
    assert!(params[0].ty.is_none());
}

#[test]
fn extract_input_params_empty() {
    let clauses = vec![Clause {
        kind: ClauseKind::Requires,
        body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
        effect_variables: vec![],
    }];
    assert!(extract_input_params(&clauses).is_empty());
}

#[test]
fn extract_input_params_non_raw_body() {
    // Input clause with non-Raw body (should be skipped)
    let clauses = vec![Clause {
        kind: ClauseKind::Input,
        body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
        effect_variables: vec![],
    }];
    assert!(extract_input_params(&clauses).is_empty());
}

// ---- #199: Contract evolution verification tests ----

#[test]
fn evolution_identical_contracts_pass() {
    // Same requires and ensures; evolution should be compatible
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
    let result = verify_evolution("TestContract", &clauses, &clauses);
    assert!(
        matches!(
            result.precondition_weakening,
            VerificationResult::Verified { .. }
        ),
        "identical preconditions should pass weakening: {:?}",
        result.precondition_weakening
    );
    assert!(
        matches!(
            result.postcondition_strengthening,
            VerificationResult::Verified { .. }
        ),
        "identical postconditions should pass strengthening: {:?}",
        result.postcondition_strengthening
    );
}

#[test]
fn evolution_weakened_precondition_passes() {
    // Old: requires x > 10
    // New: requires x > 0 (weaker, accepts more inputs)
    let old_clauses = vec![Clause {
        kind: ClauseKind::Requires,
        body: Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
        }),
        effect_variables: vec![],
    }];
    let new_clauses = vec![Clause {
        kind: ClauseKind::Requires,
        body: Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        }),
        effect_variables: vec![],
    }];
    let result = verify_evolution("TestContract", &old_clauses, &new_clauses);
    assert!(
        matches!(
            result.precondition_weakening,
            VerificationResult::Verified { .. }
        ),
        "weakened precondition should pass: {:?}",
        result.precondition_weakening
    );
}

#[test]
fn evolution_strengthened_precondition_fails() {
    // Old: requires x > 0
    // New: requires x > 10 (stronger, rejects inputs old accepted)
    let old_clauses = vec![Clause {
        kind: ClauseKind::Requires,
        body: Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        }),
        effect_variables: vec![],
    }];
    let new_clauses = vec![Clause {
        kind: ClauseKind::Requires,
        body: Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
        }),
        effect_variables: vec![],
    }];
    let result = verify_evolution("TestContract", &old_clauses, &new_clauses);
    assert!(
        matches!(
            result.precondition_weakening,
            VerificationResult::Counterexample { .. }
        ),
        "strengthened precondition should fail weakening: {:?}",
        result.precondition_weakening
    );
}

#[test]
fn evolution_dropped_ensures_fails() {
    // Old: ensures x > 0
    // New: no ensures (lost guarantees)
    let old_clauses = vec![Clause {
        kind: ClauseKind::Ensures,
        body: Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        }),
        effect_variables: vec![],
    }];
    let new_clauses: Vec<Clause> = vec![];
    let result = verify_evolution("TestContract", &old_clauses, &new_clauses);
    assert!(
        matches!(
            result.postcondition_strengthening,
            VerificationResult::Counterexample { .. }
        ),
        "dropping ensures should fail strengthening: {:?}",
        result.postcondition_strengthening
    );
}

#[test]
fn evolution_no_requires_accepts_anything() {
    // Old: no requires (accepts everything)
    // New: requires x > 0 (restricts inputs)
    // This should FAIL weakening because old accepted x = -1 but new rejects it
    let old_clauses: Vec<Clause> = vec![];
    let new_clauses = vec![Clause {
        kind: ClauseKind::Requires,
        body: Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        }),
        effect_variables: vec![],
    }];
    let result = verify_evolution("TestContract", &old_clauses, &new_clauses);
    // old has no requires, so old_requires is trivially true.
    // new_requires is x > 0. Is true => x > 0 valid? No (x could be -1).
    assert!(
        matches!(
            result.precondition_weakening,
            VerificationResult::Counterexample { .. }
        ),
        "adding requires to previously unconstrained should fail: {:?}",
        result.precondition_weakening
    );
}

#[test]
fn evolution_new_removes_requires_passes() {
    // Old: requires x > 0
    // New: no requires (accepts everything; strictly weaker)
    let old_clauses = vec![Clause {
        kind: ClauseKind::Requires,
        body: Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        }),
        effect_variables: vec![],
    }];
    let new_clauses: Vec<Clause> = vec![];
    let result = verify_evolution("TestContract", &old_clauses, &new_clauses);
    assert!(
        matches!(
            result.precondition_weakening,
            VerificationResult::Verified { .. }
        ),
        "removing requires (accepting everything) should pass: {:?}",
        result.precondition_weakening
    );
}
