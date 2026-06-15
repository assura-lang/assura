use super::*;
// T014: Expression type inference tests
// -----------------------------------------------------------------------

#[test]
fn infer_int_literal() {
    let env = TypeEnv::new();
    let expr = AstExpr::Literal(AstLit::Int("42".into()));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_float_literal() {
    let env = TypeEnv::new();
    let expr = AstExpr::Literal(AstLit::Float("3.14".into()));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Float);
}

#[test]
fn infer_string_literal() {
    let env = TypeEnv::new();
    let expr = AstExpr::Literal(AstLit::Str("hello".into()));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::String);
}

#[test]
fn infer_bool_literal() {
    let env = TypeEnv::new();
    let expr = AstExpr::Literal(AstLit::Bool(true));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_ident_known() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    let expr = AstExpr::Ident("x".into());
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_ident_unknown() {
    let env = TypeEnv::new();
    let expr = AstExpr::Ident("unknown_var".into());
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_arithmetic_add() {
    let env = TypeEnv::new();
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("2".into()))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_arithmetic_float_mul() {
    let env = TypeEnv::new();
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Float("1.0".into()))),
        op: AstBinOp::Mul,
        rhs: Box::new(AstExpr::Literal(AstLit::Float("2.0".into()))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Float);
}

#[test]
fn infer_arithmetic_numeric_types_compatible() {
    // Numeric types (Int, Float, Nat, etc.) are compatible in arithmetic
    let env = TypeEnv::new();
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Literal(AstLit::Float("2.0".into()))),
    };
    // Int + Float is accepted (numeric widening)
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_arithmetic_non_numeric() {
    let env = TypeEnv::new();
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Literal(AstLit::Bool(false))),
    };
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
    assert!(err.message.contains("numeric"));
}

#[test]
fn infer_comparison_same_type() {
    let env = TypeEnv::new();
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        op: AstBinOp::Lt,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("2".into()))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_comparison_mismatch() {
    let env = TypeEnv::new();
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        op: AstBinOp::Eq,
        rhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
    };
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn infer_logical_and() {
    let env = TypeEnv::new();
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        op: AstBinOp::And,
        rhs: Box::new(AstExpr::Literal(AstLit::Bool(false))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_logical_non_bool() {
    let env = TypeEnv::new();
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        op: AstBinOp::And,
        rhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
    };
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
    assert!(err.message.contains("Bool"));
}

#[test]
fn infer_implies() {
    let env = TypeEnv::new();
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        op: AstBinOp::Implies,
        rhs: Box::new(AstExpr::Literal(AstLit::Bool(false))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_unary_neg() {
    let env = TypeEnv::new();
    let expr = AstExpr::UnaryOp {
        op: AstUnOp::Neg,
        expr: Box::new(AstExpr::Literal(AstLit::Int("5".into()))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_unary_neg_non_numeric() {
    let env = TypeEnv::new();
    let expr = AstExpr::UnaryOp {
        op: AstUnOp::Neg,
        expr: Box::new(AstExpr::Literal(AstLit::Bool(true))),
    };
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn infer_unary_not() {
    let env = TypeEnv::new();
    let expr = AstExpr::UnaryOp {
        op: AstUnOp::Not,
        expr: Box::new(AstExpr::Literal(AstLit::Bool(false))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_unary_not_non_bool() {
    let env = TypeEnv::new();
    let expr = AstExpr::UnaryOp {
        op: AstUnOp::Not,
        expr: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
    };
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn infer_if_then_else() {
    let env = TypeEnv::new();
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        else_branch: Some(Box::new(AstExpr::Literal(AstLit::Int("2".into())))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_if_branch_mismatch() {
    let env = TypeEnv::new();
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        else_branch: Some(Box::new(AstExpr::Literal(AstLit::Bool(false)))),
    };
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
    assert!(err.message.contains("different types"));
}

#[test]
fn infer_if_non_bool_cond() {
    let env = TypeEnv::new();
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        then_branch: Box::new(AstExpr::Literal(AstLit::Int("2".into()))),
        else_branch: None,
    };
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
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::Ident("x".into())),
        else_branch: Some(Box::new(AstExpr::Ident("y".into()))),
    };
    // Should succeed (Nat and Int are compatible)
    assert!(infer_expr(&expr, &env).is_ok());
}

#[test]
fn infer_old_preserves_type() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    let expr = AstExpr::Old(Box::new(AstExpr::Ident("x".into())));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_paren_preserves_type() {
    let env = TypeEnv::new();
    let expr = AstExpr::Paren(Box::new(AstExpr::Literal(AstLit::Float("1.5".into()))));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Float);
}

#[test]
fn infer_forall_is_bool() {
    let env = TypeEnv::new();
    let expr = AstExpr::Forall {
        var: "i".into(),
        domain: Box::new(AstExpr::Ident("S".into())),
        body: Box::new(AstExpr::Literal(AstLit::Bool(true))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_exists_is_bool() {
    let env = TypeEnv::new();
    let expr = AstExpr::Exists {
        var: "i".into(),
        domain: Box::new(AstExpr::Ident("S".into())),
        body: Box::new(AstExpr::Literal(AstLit::Bool(true))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn forall_binds_variable_from_list_domain() {
    let mut env = TypeEnv::new();
    // xs: List<Int>
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    // forall x in xs: x > 0  -- x should be inferred as Int
    let expr = AstExpr::Forall {
        var: "x".into(),
        domain: Box::new(AstExpr::Ident("xs".into())),
        body: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("x".into())),
            op: assura_parser::ast::BinOp::Gt,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
        }),
    };
    // Should not error because x is bound as Int in the body
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn exists_binds_variable_from_set_domain() {
    let mut env = TypeEnv::new();
    // s: Set<String>
    env.insert("s".into(), Type::Set(Box::new(Type::String)));
    let expr = AstExpr::Exists {
        var: "elem".into(),
        domain: Box::new(AstExpr::Ident("s".into())),
        body: Box::new(AstExpr::Literal(AstLit::Bool(true))),
    };
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
    let expr = AstExpr::Forall {
        var: "k".into(),
        domain: Box::new(AstExpr::Ident("m".into())),
        body: Box::new(AstExpr::Literal(AstLit::Bool(true))),
    };
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
    let expr = AstExpr::List(vec![
        AstExpr::Literal(AstLit::Int("1".into())),
        AstExpr::Literal(AstLit::Int("2".into())),
        AstExpr::Literal(AstLit::Int("3".into())),
    ]);
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::List(Box::new(Type::Int))
    );
}

#[test]
fn infer_list_empty() {
    let env = TypeEnv::new();
    let expr = AstExpr::List(vec![]);
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::List(Box::new(Type::Unknown))
    );
}

#[test]
fn infer_list_type_mismatch() {
    let env = TypeEnv::new();
    let expr = AstExpr::List(vec![
        AstExpr::Literal(AstLit::Int("1".into())),
        AstExpr::Literal(AstLit::Bool(true)),
    ]);
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
    assert!(err.message.contains("list"));
}

#[test]
fn infer_unknown_no_error_in_binop() {
    let env = TypeEnv::new();
    // unknown_var + 1 should not error (unknown_var is Unknown)
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("unknown_var".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
    };
    // Should succeed with Int (known side propagated)
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_range_op() {
    let env = TypeEnv::new();
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
        op: AstBinOp::Range,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("10".into()))),
    };
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::List(Box::new(Type::Int))
    );
}

#[test]
fn infer_in_op() {
    let env = TypeEnv::new();
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        op: AstBinOp::In,
        rhs: Box::new(AstExpr::Ident("collection".into())),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_raw_is_error() {
    let env = TypeEnv::new();
    let expr = AstExpr::Raw(vec!["some".into(), "tokens".into()]);
    // Raw tokens yield Error (not Unknown) since they cannot be parsed
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Error);
}

#[test]
fn infer_field_is_unknown() {
    let env = TypeEnv::new();
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("x".into())), "len".into());
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_call_is_unknown() {
    let env = TypeEnv::new();
    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("f".into())),
        args: vec![],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

// -----------------------------------------------------------------------
