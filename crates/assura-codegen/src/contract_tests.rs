use super::*;
use assura_ast::*;
use assura_ast::*;

fn mk_clause(kind: ClauseKind, body: SpExpr) -> Clause {
    Clause {
        kind,
        body,
        effect_variables: vec![],
    }
}

fn mk_contract(name: &str, clauses: Vec<Clause>) -> ContractDecl {
    ContractDecl {
        name: name.into(),
        type_params: vec![],
        clauses,
        fn_params: vec![],
    }
}

// ---- generate_enum_def ----

#[test]
fn enum_def_unit_variants() {
    let e = EnumDef {
        name: "Color".into(),
        type_params: vec![],
        variants: vec![
            EnumVariant {
                name: "Red".into(),
                fields: vec![],
            },
            EnumVariant {
                name: "Green".into(),
                fields: vec![],
            },
        ],
    };
    let mut code = String::new();
    generate_enum_def(&e, &mut code);
    assert!(code.contains("pub enum Color {"));
    assert!(code.contains("    Red,"));
    assert!(code.contains("    Green,"));
    // Display impl
    assert!(code.contains("impl std::fmt::Display for Color"));
    // Exhaustiveness check
    assert!(code.contains("__exhaustive_check_color"));
}

#[test]
fn enum_def_variant_with_fields() {
    let e = EnumDef {
        name: "Shape".into(),
        type_params: vec![],
        variants: vec![
            EnumVariant {
                name: "Circle".into(),
                fields: vec!["Float".into()],
            },
            EnumVariant {
                name: "Rect".into(),
                fields: vec!["Float".into(), "Float".into()],
            },
        ],
    };
    let mut code = String::new();
    generate_enum_def(&e, &mut code);
    assert!(code.contains("Circle(f64)"));
    assert!(code.contains("Rect(f64, f64)"));
    // Display shows (...) for fields
    assert!(code.contains("Circle(...)"));
}

#[test]
fn enum_def_generic_no_display() {
    let e = EnumDef {
        name: "Option".into(),
        type_params: vec!["T".into()],
        variants: vec![
            EnumVariant {
                name: "Some".into(),
                fields: vec!["T".into()],
            },
            EnumVariant {
                name: "None".into(),
                fields: vec![],
            },
        ],
    };
    let mut code = String::new();
    generate_enum_def(&e, &mut code);
    assert!(code.contains("pub enum Option<T>"));
    // Generic enums skip Display impl
    assert!(!code.contains("impl std::fmt::Display"));
    // Generic enums skip exhaustiveness check
    assert!(!code.contains("__exhaustive_check"));
}

#[test]
fn enum_def_empty_variants_no_exhaustive() {
    let e = EnumDef {
        name: "Empty".into(),
        type_params: vec![],
        variants: vec![],
    };
    let mut code = String::new();
    generate_enum_def(&e, &mut code);
    assert!(code.contains("pub enum Empty {"));
    assert!(!code.contains("__exhaustive_check"));
}

// ---- proptest_strategy_for_type ----

#[test]
fn proptest_strategy_known_types() {
    assert!(proptest_strategy_for_type("i64").contains("any::<i64>()"));
    assert!(proptest_strategy_for_type("bool").contains("any::<bool>()"));
    assert!(proptest_strategy_for_type("f64").contains("any::<f64>()"));
    assert!(proptest_strategy_for_type("u8").contains("any::<u8>()"));
}

#[test]
fn proptest_strategy_unknown_type() {
    let s = proptest_strategy_for_type("MyStruct");
    assert!(s.contains("any::<MyStruct>()"));
}

// ---- try_refine_strategy ----

#[test]
fn refine_neq_zero() {
    let expr = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        op: BinOp::Neq,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });
    let (param, strategy) = try_refine_strategy(&expr).unwrap();
    assert_eq!(param, "x");
    assert!(strategy.contains("1i64..=i64::MAX"));
}

#[test]
fn refine_gt_zero() {
    let expr = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        op: BinOp::Gt,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });
    let (param, strategy) = try_refine_strategy(&expr).unwrap();
    assert_eq!(param, "n");
    assert!(strategy.contains("1i64..=i64::MAX"));
}

#[test]
fn refine_gte_zero() {
    let expr = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        op: BinOp::Gte,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });
    let (_, strategy) = try_refine_strategy(&expr).unwrap();
    assert!(strategy.contains("0i64..=i64::MAX"));
}

#[test]
fn refine_lt_bound() {
    let expr = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        op: BinOp::Lt,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("100".into())))),
    });
    let (_, strategy) = try_refine_strategy(&expr).unwrap();
    assert!(strategy.contains("100i64"));
    assert!(strategy.contains("i64::MIN"));
}

#[test]
fn refine_lte_bound() {
    let expr = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        op: BinOp::Lte,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("50".into())))),
    });
    let (_, strategy) = try_refine_strategy(&expr).unwrap();
    assert!(strategy.contains("=50i64"));
}

#[test]
fn refine_non_ident_lhs_returns_none() {
    let expr = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        op: BinOp::Gt,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });
    assert!(try_refine_strategy(&expr).is_none());
}

#[test]
fn refine_non_binop_returns_none() {
    let expr = Spanned::no_span(Expr::Ident("x".into()));
    assert!(try_refine_strategy(&expr).is_none());
}

// ---- contract_is_testable ----

#[test]
fn testable_contract_has_input_and_ensures() {
    let c = mk_contract(
        "Div",
        vec![
            mk_clause(ClauseKind::Input, Spanned::no_span(Expr::Ident("x".into()))),
            mk_clause(
                ClauseKind::Ensures,
                Spanned::no_span(Expr::Literal(Literal::Bool(true))),
            ),
        ],
    );
    assert!(contract_is_testable(&c));
}

#[test]
fn not_testable_missing_ensures() {
    let c = mk_contract(
        "Div",
        vec![mk_clause(
            ClauseKind::Input,
            Spanned::no_span(Expr::Ident("x".into())),
        )],
    );
    assert!(!contract_is_testable(&c));
}

#[test]
fn not_testable_missing_input() {
    let c = mk_contract(
        "Div",
        vec![mk_clause(
            ClauseKind::Ensures,
            Spanned::no_span(Expr::Literal(Literal::Bool(true))),
        )],
    );
    assert!(!contract_is_testable(&c));
}

// ---- extract_output_type ----

#[test]
fn output_type_from_cast() {
    let body = Spanned::no_span(Expr::Cast {
        expr: Box::new(Spanned::no_span(Expr::Ident("result".into()))),
        ty: "Int".into(),
    });
    assert_eq!(extract_output_type(&body), "i64");
}

#[test]
fn output_type_from_ident() {
    let body = Spanned::no_span(Expr::Ident("Bool".into()));
    assert_eq!(extract_output_type(&body), "bool");
}

#[test]
fn output_type_from_float_ident() {
    let body = Spanned::no_span(Expr::Ident("Float".into()));
    assert_eq!(extract_output_type(&body), "f64");
}

#[test]
fn output_type_from_raw_colon() {
    let body = Spanned::no_span(Expr::Raw(vec!["result".into(), ":".into(), "Int".into()]));
    assert_eq!(extract_output_type(&body), "i64");
}

#[test]
fn output_type_unknown_returns_unit() {
    let body = Spanned::no_span(Expr::Literal(Literal::Int("42".into())));
    assert_eq!(extract_output_type(&body), "()");
}

// ---- extract_error_variants ----

#[test]
fn error_variants_single_ident() {
    let body = Spanned::no_span(Expr::Ident("DivByZero".into()));
    assert_eq!(extract_error_variants(&body), vec!["DivByZero"]);
}

#[test]
fn error_variants_tuple() {
    let body = Spanned::no_span(Expr::Tuple(vec![
        Spanned::no_span(Expr::Ident("DivByZero".into())),
        Spanned::no_span(Expr::Ident("Overflow".into())),
    ]));
    let vars = extract_error_variants(&body);
    assert_eq!(vars, vec!["DivByZero", "Overflow"]);
}

#[test]
fn error_variants_raw_tokens() {
    let body = Spanned::no_span(Expr::Raw(vec![
        "DivByZero".into(),
        ",".into(),
        "Overflow".into(),
    ]));
    let vars = extract_error_variants(&body);
    assert_eq!(vars, vec!["DivByZero", "Overflow"]);
}

#[test]
fn error_variants_ident() {
    let body = Spanned::no_span(Expr::Ident("Err".into()));
    assert_eq!(extract_error_variants(&body), vec!["Err"]);
}

// ---- collect_error_variants ----

#[test]
fn collect_errors_from_clauses() {
    let clauses = vec![
        mk_clause(
            ClauseKind::Requires,
            Spanned::no_span(Expr::Literal(Literal::Bool(true))),
        ),
        mk_clause(
            ClauseKind::Errors,
            Spanned::no_span(Expr::Ident("DivByZero".into())),
        ),
        mk_clause(
            ClauseKind::Errors,
            Spanned::no_span(Expr::Ident("Overflow".into())),
        ),
    ];
    let vars = collect_error_variants(&clauses);
    assert_eq!(vars, vec!["DivByZero", "Overflow"]);
}

#[test]
fn collect_errors_empty() {
    let clauses = vec![mk_clause(
        ClauseKind::Requires,
        Spanned::no_span(Expr::Literal(Literal::Bool(true))),
    )];
    assert!(collect_error_variants(&clauses).is_empty());
}

// ---- generate_error_enum ----

#[test]
fn error_enum_basic() {
    let mut code = String::new();
    generate_error_enum("Div", &["DivByZero".into(), "Overflow".into()], &mut code);
    assert!(code.contains("pub enum DivError"));
    assert!(code.contains("#[derive(Debug, thiserror::Error)]"));
    assert!(code.contains("#[error(\"DivByZero\")]"));
    assert!(code.contains("DivByZero,"));
    assert!(code.contains("Overflow,"));
}

// ---- generate_contract ----

#[test]
fn contract_wraps_in_pub_mod() {
    let c = mk_contract(
        "SafeDiv",
        vec![mk_clause(
            ClauseKind::Requires,
            Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
                op: BinOp::Neq,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
        )],
    );
    let mut code = String::new();
    generate_contract(&c, &mut code, None);
    assert!(code.contains("pub mod contract_safediv"));
    assert!(code.contains("/// Contract: SafeDiv"));
}

#[test]
fn contract_interface_generates_trait() {
    let c = mk_contract(
        "Hashable",
        vec![mk_clause(
            ClauseKind::Other("interface".into()),
            Spanned::no_span(Expr::Literal(Literal::Bool(true))),
        )],
    );
    let mut code = String::new();
    generate_contract(&c, &mut code, None);
    assert!(code.contains("pub trait Hashable"));
    assert!(!code.contains("pub mod"));
}

// ---- generate_contract_contents ----

#[test]
fn contract_contents_with_requires_and_ensures() {
    let c = mk_contract(
        "SafeDiv",
        vec![
            mk_clause(
                ClauseKind::Requires,
                Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
                    op: BinOp::Neq,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
            ),
            mk_clause(
                ClauseKind::Ensures,
                Spanned::no_span(Expr::Literal(Literal::Bool(true))),
            ),
        ],
    );
    let mut code = String::new();
    generate_contract_contents(&c, &mut code, None);
    assert!(code.contains("pub fn check("));
    assert!(code.contains("debug_assert!"));
    assert!(code.contains(RESULT_VAR));
}

#[test]
fn contract_contents_with_errors_generates_result() {
    let c = mk_contract(
        "Div",
        vec![mk_clause(
            ClauseKind::Errors,
            Spanned::no_span(Expr::Ident("DivByZero".into())),
        )],
    );
    let mut code = String::new();
    generate_contract_contents(&c, &mut code, None);
    assert!(code.contains("pub enum DivError"));
    assert!(code.contains("Result<"));
    assert!(code.contains("DivError"));
}

#[test]
fn contract_contents_no_ensures_emits_todo() {
    let c = mk_contract("Simple", vec![]);
    let mut code = String::new();
    generate_contract_contents(&c, &mut code, None);
    assert!(code.contains("todo!(\"implementation provided by AI agent\")"));
}

#[test]
fn contract_contents_with_implements() {
    let c = mk_contract(
        "MyImpl",
        vec![mk_clause(
            ClauseKind::Other("implements".into()),
            Spanned::no_span(Expr::Ident("Hashable".into())),
        )],
    );
    let mut code = String::new();
    generate_contract_contents(&c, &mut code, None);
    assert!(code.contains("pub struct MyImpl;"));
    assert!(code.contains("impl Hashable for MyImpl"));
}

// ---- IR body injection ----

#[test]
fn contract_with_ir_body_replaces_todo() {
    let c = mk_contract(
        "AddOne",
        vec![
            mk_clause(
                ClauseKind::Input,
                Spanned::no_span(Expr::Cast {
                    expr: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    ty: "Int".into(),
                }),
            ),
            mk_clause(
                ClauseKind::Output,
                Spanned::no_span(Expr::Cast {
                    expr: Box::new(Spanned::no_span(Expr::Ident("result".into()))),
                    ty: "Int".into(),
                }),
            ),
        ],
    );
    // Without IR body: should contain todo!()
    let mut code_no_ir = String::new();
    generate_contract_contents(&c, &mut code_no_ir, None);
    assert!(
        code_no_ir.contains("todo!"),
        "without IR body, should have todo!(): {code_no_ir}"
    );

    // With IR body: todo!() replaced
    let mut ir_bodies = std::collections::HashMap::new();
    ir_bodies.insert(
        "AddOne".to_string(),
        format!("    let {RESULT_VAR}: i64 = (x + 1_i64);\n    {RESULT_VAR}\n"),
    );
    let mut code_with_ir = String::new();
    generate_contract_contents(&c, &mut code_with_ir, Some(&ir_bodies));
    assert!(
        !code_with_ir.contains("todo!"),
        "with IR body, should NOT have todo!(): {code_with_ir}"
    );
    assert!(
        code_with_ir.contains("(x + 1_i64)"),
        "IR body should be present: {code_with_ir}"
    );
}

// ---- generate_interface_trait_from_contract ----

#[test]
fn interface_trait_simple() {
    let c = mk_contract(
        "Serializable",
        vec![
            mk_clause(
                ClauseKind::Other("interface".into()),
                Spanned::no_span(Expr::Literal(Literal::Bool(true))),
            ),
            mk_clause(
                ClauseKind::Other("method".into()),
                Spanned::no_span(Expr::Ident("serialize".into())),
            ),
        ],
    );
    let mut code = String::new();
    generate_interface_trait_from_contract(&c, &mut code);
    assert!(code.contains("pub trait Serializable"));
    assert!(code.contains("fn serialize(&self)"));
}

#[test]
fn interface_trait_with_extends() {
    let c = mk_contract(
        "AdvHash",
        vec![
            mk_clause(
                ClauseKind::Other("interface".into()),
                Spanned::no_span(Expr::Literal(Literal::Bool(true))),
            ),
            mk_clause(
                ClauseKind::Other("extends".into()),
                Spanned::no_span(Expr::Ident("Hashable".into())),
            ),
        ],
    );
    let mut code = String::new();
    generate_interface_trait_from_contract(&c, &mut code);
    assert!(code.contains("pub trait AdvHash: Hashable"));
}

#[test]
fn interface_trait_with_invariant() {
    let c = mk_contract(
        "Bounded",
        vec![
            mk_clause(
                ClauseKind::Other("interface".into()),
                Spanned::no_span(Expr::Literal(Literal::Bool(true))),
            ),
            mk_clause(
                ClauseKind::Invariant,
                Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
            ),
        ],
    );
    let mut code = String::new();
    generate_interface_trait_from_contract(&c, &mut code);
    assert!(code.contains("fn check_invariant(&self)"));
    assert!(code.contains("debug_assert!"));
}

// ---- extract_input_params ----

#[test]
fn extract_input_from_cast() {
    let body = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("input".into()))),
        args: vec![Spanned::no_span(Expr::Cast {
            expr: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            ty: "Int".into(),
        })],
    });
    let mut params = Vec::new();
    extract_input_params(&body, &mut params);
    // extract_clause_params from the parser handles this
    // Behavior depends on extract_clause_params implementation
    // At minimum, should not panic
}

#[test]
fn extract_input_from_ident() {
    let body = Spanned::no_span(Expr::Ident("x".into()));
    let mut params = Vec::new();
    extract_input_params(&body, &mut params);
    // Single ident extraction depends on extract_clause_params
}
