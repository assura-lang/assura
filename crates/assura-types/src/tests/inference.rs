use super::*;
use assura_parser::ast::Spanned;
// T014: Expression type inference tests
// -----------------------------------------------------------------------

#[test]
fn infer_int_literal() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Literal(AstLit::Int("42".into())));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_float_literal() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Literal(AstLit::Float("3.14".into())));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Float);
}

#[test]
fn infer_string_literal() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Literal(AstLit::Str("hello".into())));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::String);
}

#[test]
fn infer_bool_literal() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_ident_known() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    let expr = Spanned::no_span(AstExpr::Ident("x".into()));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_ident_unknown() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Ident("unknown_var".into()));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_arithmetic_add() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("2".into())))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_arithmetic_float_mul() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Float(
            "1.0".into(),
        )))),
        op: AstBinOp::Mul,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Float(
            "2.0".into(),
        )))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Float);
}

#[test]
fn infer_arithmetic_numeric_types_compatible() {
    // Numeric types (Int, Float, Nat, etc.) are compatible in arithmetic
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Float(
            "2.0".into(),
        )))),
    });
    // Int + Float is accepted (numeric widening)
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_arithmetic_non_numeric() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(false)))),
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
    assert!(err.message.contains("numeric"));
}

#[test]
fn infer_comparison_same_type() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        op: AstBinOp::Lt,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("2".into())))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_comparison_mismatch() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        op: AstBinOp::Eq,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn infer_logical_and() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        op: AstBinOp::And,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(false)))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_logical_non_bool() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        op: AstBinOp::And,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
    assert!(err.message.contains("Bool"));
}

#[test]
fn infer_implies() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        op: AstBinOp::Implies,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(false)))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_unary_neg() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::UnaryOp {
        op: AstUnOp::Neg,
        expr: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("5".into())))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_unary_neg_non_numeric() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::UnaryOp {
        op: AstUnOp::Neg,
        expr: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn infer_unary_not() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::UnaryOp {
        op: AstUnOp::Not,
        expr: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(false)))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_unary_not_non_bool() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::UnaryOp {
        op: AstUnOp::Not,
        expr: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn infer_if_then_else() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int(
            "2".into(),
        ))))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_if_branch_mismatch() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(
            false,
        ))))),
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
    assert!(err.message.contains("different types"));
}

#[test]
fn infer_if_non_bool_cond() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("2".into())))),
        else_branch: None,
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
    assert!(err.message.contains("Bool"));
}

#[test]
fn infer_if_nat_int_branches_compatible() {
    // Nat and Int in different branches should be compatible
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Nat);
    env.insert("y".into(), Type::Int);
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Ident("y".into())))),
    });
    // Should succeed (Nat and Int are compatible)
    infer_expr(&expr, &env).unwrap();
}

#[test]
fn infer_old_preserves_type() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    let expr = Spanned::no_span(AstExpr::Old(Box::new(Spanned::no_span(AstExpr::Ident(
        "x".into(),
    )))));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_float_literal_type() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Literal(AstLit::Float("1.5".into())));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Float);
}

#[test]
fn infer_forall_is_bool() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Forall {
        var: "i".into(),
        domain: Box::new(Spanned::no_span(AstExpr::Ident("S".into()))),
        body: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_exists_is_bool() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Exists {
        var: "i".into(),
        domain: Box::new(Spanned::no_span(AstExpr::Ident("S".into()))),
        body: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn forall_binds_variable_from_list_domain() {
    let mut env = TypeEnv::new();
    // xs: List<Int>
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    // forall x in xs: x > 0  -- x should be inferred as Int
    let expr = Spanned::no_span(AstExpr::Forall {
        var: "x".into(),
        domain: Box::new(Spanned::no_span(AstExpr::Ident("xs".into()))),
        body: Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
            op: assura_parser::ast::BinOp::Gt,
            rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
        })),
    });
    // Should not error because x is bound as Int in the body
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn exists_binds_variable_from_set_domain() {
    let mut env = TypeEnv::new();
    // s: Set<String>
    env.insert("s".into(), Type::Set(Box::new(Type::String)));
    let expr = Spanned::no_span(AstExpr::Exists {
        var: "elem".into(),
        domain: Box::new(Spanned::no_span(AstExpr::Ident("s".into()))),
        body: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn forall_binds_variable_from_map_domain() {
    let mut env = TypeEnv::new();
    // m: Map<String, Int>  -- iterating over a map yields keys
    env.insert(
        "m".into(),
        Type::Map(Box::new(Type::String), Box::new(Type::Int)),
    );
    let expr = Spanned::no_span(AstExpr::Forall {
        var: "k".into(),
        domain: Box::new(Spanned::no_span(AstExpr::Ident("m".into()))),
        body: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn element_type_of_returns_correct_types() {
    assert_eq!(element_type_of(&Type::List(Box::new(Type::Int))), Type::Int);
    assert_eq!(
        element_type_of(&Type::Set(Box::new(Type::String))),
        Type::String
    );
    assert_eq!(
        element_type_of(&Type::Sequence(Box::new(Type::Bool))),
        Type::Bool
    );
    assert_eq!(
        element_type_of(&Type::Map(Box::new(Type::String), Box::new(Type::Int))),
        Type::String
    );
    assert_eq!(element_type_of(&Type::Int), Type::Int);
    assert_eq!(element_type_of(&Type::Named("Foo".into())), Type::Unknown);
}

#[test]
fn infer_list_uniform() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::List(vec![
        Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into()))),
        Spanned::no_span(AstExpr::Literal(AstLit::Int("2".into()))),
        Spanned::no_span(AstExpr::Literal(AstLit::Int("3".into()))),
    ]));
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::List(Box::new(Type::Int))
    );
}

#[test]
fn infer_list_empty() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::List(vec![]));
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::List(Box::new(Type::Unknown))
    );
}

#[test]
fn infer_list_type_mismatch() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::List(vec![
        Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into()))),
        Spanned::no_span(AstExpr::Literal(AstLit::Bool(true))),
    ]));
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
    assert!(err.message.contains("list"));
}

#[test]
fn infer_binop_propagates_known_type_past_unknown() {
    let env = TypeEnv::new();
    // unknown_var + 1: unknown ident on one side, Int literal on the other.
    // Inference should propagate the known type (Int) from the RHS.
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("unknown_var".into()))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_range_op() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
        op: AstBinOp::Range,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("10".into())))),
    });
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::List(Box::new(Type::Int))
    );
}

#[test]
fn infer_in_op() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        op: AstBinOp::In,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("collection".into()))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_raw_is_error() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Raw(vec!["some".into(), "tokens".into()]));
    // Raw tokens yield Error (not Unknown) since they cannot be parsed
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Error);
}

#[test]
fn infer_field_is_unknown() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        "len".into(),
    ));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_call_is_unknown() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("f".into()))),
        args: vec![],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

/// Regression test for #172: Int > String must produce A03001
#[test]
fn test_clause_body_rejects_cross_type_comparison() {
    let src = r#"
contract Bad {
    requires x > "hello"
    fn bad(x: Int) -> Int
}
"#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    assert!(
        result.is_err(),
        "cross-type comparison should produce a type error"
    );
    let errors = result.unwrap_err();
    let a03001 = errors.iter().filter(|e| e.code == "A03001").count();
    assert!(
        a03001 >= 1,
        "expected A03001 for Int > String, got errors: {errors:?}"
    );
}

// -----------------------------------------------------------------------
