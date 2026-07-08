use super::*;
// Match expression exhaustiveness wiring tests (T017)
// -----------------------------------------------------------------------

#[test]
fn match_infer_type_from_first_arm() {
    // match x { A => 42, B => 0 } should infer Int from the first arm
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Ident("A".into()),
                body: Spanned::no_span(AstExpr::Literal(AstLit::Int("42".into()))),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Ident("B".into()),
                body: Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into()))),
            },
        ],
    });
    let result = infer_expr(&expr, &env);
    assert_eq!(result.unwrap(), Type::Int);
}

#[test]
fn match_incompatible_arms_emits_error() {
    // match x { A => 42, B => true } should emit A03001
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Ident("A".into()),
                body: Spanned::no_span(AstExpr::Literal(AstLit::Int("42".into()))),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Ident("B".into()),
                body: Spanned::no_span(AstExpr::Literal(AstLit::Bool(true))),
            },
        ],
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
    assert!(err.message.contains("incompatible"));
}

#[test]
fn match_compatible_arms_ok() {
    // match x { A => 42, B => 0 } all Int arms = ok
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Ident("A".into()),
                body: Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into()))),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Ident("B".into()),
                body: Spanned::no_span(AstExpr::Literal(AstLit::Int("2".into()))),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Wildcard,
                body: Spanned::no_span(AstExpr::Literal(AstLit::Int("3".into()))),
            },
        ],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn match_empty_arms_infers_unknown() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        arms: vec![],
    });
    let result = infer_expr(&expr, &env);
    assert_eq!(result.unwrap(), Type::Unknown);
}

#[test]
fn match_expr_references_var() {
    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Ident("status".into()))),
        arms: vec![assura_parser::ast::MatchArm {
            pattern: assura_parser::ast::Pattern::Ident("A".into()),
            body: Spanned::no_span(AstExpr::Ident("result".into())),
        }],
    });
    assert!(expr_references_var(&expr, "status"));
    assert!(expr_references_var(&expr, "result"));
    assert!(!expr_references_var(&expr, "other"));
}

#[test]
fn infer_cast_returns_target_type() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Cast {
        expr: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("42".into())))),
        ty: "Float".into(),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Float);
}

#[test]
fn infer_cast_to_u8() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Cast {
        expr: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int(
            "255".into(),
        )))),
        ty: "U8".into(),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::U8);
}

#[test]
fn infer_cast_to_named_type() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Cast {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        ty: "CustomType".into(),
    });
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Named("CustomType".into())
    );
}

#[test]
fn infer_let_binding_propagates_type() {
    let env = TypeEnv::new();
    // let x = 42 in x  =>  should infer Int from body
    let expr = Spanned::no_span(AstExpr::Let {
        name: "x".into(),
        value: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("42".into())))),
        body: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_match_checks_all_arms() {
    let env = TypeEnv::new();
    // match true { true => 1, false => 2 } => Int
    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Literal(AstLit::Bool(true)),
                body: Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into()))),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Literal(AstLit::Bool(false)),
                body: Spanned::no_span(AstExpr::Literal(AstLit::Int("2".into()))),
            },
        ],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_match_empty_arms_returns_unknown() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        arms: vec![],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_builtin_len_returns_nat() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("len".into()))),
        args: vec![Spanned::no_span(AstExpr::Ident("xs".into()))],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Nat);
}

#[test]
fn infer_builtin_contains_returns_bool() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("contains".into()))),
        args: vec![
            Spanned::no_span(AstExpr::Ident("xs".into())),
            Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into()))),
        ],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn result_bound_in_ensures_env() {
    // When `result` is bound in the env, infer_expr should return it
    let mut env = TypeEnv::new();
    env.insert("result".to_string(), Type::Int);
    let expr = Spanned::no_span(AstExpr::Ident("result".into()));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn result_unknown_without_binding() {
    // Without binding, `result` returns Unknown
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Ident("result".into()));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn result_type_threaded_through_ensures() {
    // Parse a function with an ensures clause using `result`
    let src = r#"
fn square(x: Int) -> Int
  ensures { result >= 0 }
"#;
    let file = assura_parser::parse_unwrap(src);
    let resolved = assura_resolve::resolve(&file).unwrap();
    // type_check should succeed; the `result >= 0` comparison is
    // Int >= Int which is valid
    let typed = type_check(resolved);
    typed.expect("type_check failed");
}

#[test]
fn tuple_infers_element_types() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Tuple(vec![
        Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into()))),
        Spanned::no_span(AstExpr::Literal(AstLit::Bool(true))),
    ]));
    let ty = infer_expr(&expr, &env).unwrap();
    assert_eq!(ty, Type::Tuple(vec![Type::Int, Type::Bool]));
}

#[test]
fn tuple_single_element() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Tuple(vec![Spanned::no_span(AstExpr::Literal(
        AstLit::Int("42".into()),
    ))]));
    let ty = infer_expr(&expr, &env).unwrap();
    assert_eq!(ty, Type::Tuple(vec![Type::Int]));
}

#[test]
fn tuple_empty() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Tuple(vec![]));
    let ty = infer_expr(&expr, &env).unwrap();
    assert_eq!(ty, Type::Tuple(vec![]));
}

#[test]
fn tuple_display() {
    let ty = Type::Tuple(vec![Type::Int, Type::Bool, Type::String]);
    assert_eq!(format!("{ty}"), "(Int, Bool, String)");
}

#[test]
fn tuple_field_access_numeric() {
    let mut env = TypeEnv::new();
    env.insert("pair".into(), Type::Tuple(vec![Type::Int, Type::String]));
    // pair.0 should be Int
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("pair".into()))),
        "0".into(),
    ));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
    // pair.1 should be String
    let expr1 = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("pair".into()))),
        "1".into(),
    ));
    assert_eq!(infer_expr(&expr1, &env).unwrap(), Type::String);
}

#[test]
fn tuple_field_access_out_of_range_a03005() {
    let mut env = TypeEnv::new();
    env.insert("pair".into(), Type::Tuple(vec![Type::Int, Type::Bool]));
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("pair".into()))),
        "2".into(),
    ));
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03005");
    assert!(
        err.message.contains("out of range"),
        "expected out-of-range message, got {}",
        err.message
    );
}

#[test]
fn tuple_field_access_non_numeric_a03005() {
    let mut env = TypeEnv::new();
    env.insert("pair".into(), Type::Tuple(vec![Type::Int, Type::Bool]));
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("pair".into()))),
        "x".into(),
    ));
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03005");
}

#[test]
fn tuple_compatibility() {
    assert!(types_compatible(
        &Type::Tuple(vec![Type::Int, Type::Bool]),
        &Type::Tuple(vec![Type::Int, Type::Bool])
    ));
    // Different arities are incompatible
    assert!(!types_compatible(
        &Type::Tuple(vec![Type::Int]),
        &Type::Tuple(vec![Type::Int, Type::Bool])
    ));
    // Int/Nat are compatible within tuples
    assert!(types_compatible(
        &Type::Tuple(vec![Type::Nat]),
        &Type::Tuple(vec![Type::Int])
    ));
}

#[test]
fn list_field_head_returns_option() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("xs".into()))),
        "head".into(),
    ));
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Option(Box::new(Type::Int))
    );
}

#[test]
fn list_field_tail_returns_list() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("xs".into()))),
        "tail".into(),
    ));
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::List(Box::new(Type::Int))
    );
}

#[test]
fn option_flatten_reduces_nesting() {
    let mut env = TypeEnv::new();
    env.insert(
        "x".into(),
        Type::Option(Box::new(Type::Option(Box::new(Type::Int)))),
    );
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        method: "flatten".into(),
        args: vec![],
    });
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Option(Box::new(Type::Int))
    );
}

#[test]
fn option_ok_or_returns_result() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Option(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        method: "ok_or".into(),
        args: vec![Spanned::no_span(AstExpr::Literal(AstLit::Str(
            "err".into(),
        )))],
    });
    let ty = infer_expr(&expr, &env).unwrap();
    match ty {
        Type::Result(ok, _) => assert_eq!(*ok, Type::Int),
        other => panic!("expected Result, got {other:?}"),
    }
}

#[test]
fn result_map_err_preserves_ok_type() {
    let mut env = TypeEnv::new();
    env.insert(
        "r".into(),
        Type::Result(Box::new(Type::Int), Box::new(Type::String)),
    );
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("r".into()))),
        method: "map_err".into(),
        args: vec![],
    });
    let ty = infer_expr(&expr, &env).unwrap();
    match ty {
        Type::Result(ok, _) => assert_eq!(*ok, Type::Int),
        other => panic!("expected Result, got {other:?}"),
    }
}

#[test]
fn result_ok_returns_option() {
    let mut env = TypeEnv::new();
    env.insert(
        "r".into(),
        Type::Result(Box::new(Type::Nat), Box::new(Type::String)),
    );
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("r".into()))),
        method: "ok".into(),
        args: vec![],
    });
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Option(Box::new(Type::Nat))
    );
}

#[test]
fn result_err_returns_option() {
    let mut env = TypeEnv::new();
    env.insert(
        "r".into(),
        Type::Result(Box::new(Type::Nat), Box::new(Type::String)),
    );
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("r".into()))),
        method: "err".into(),
        args: vec![],
    });
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Option(Box::new(Type::String))
    );
}

#[test]
fn result_unwrap_err_returns_error_type() {
    let mut env = TypeEnv::new();
    env.insert(
        "r".into(),
        Type::Result(Box::new(Type::Int), Box::new(Type::String)),
    );
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("r".into()))),
        method: "unwrap_err".into(),
        args: vec![],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::String);
}

#[test]
fn range_returns_list_int() {
    // Range expression returns List<Int>
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
fn range_rejects_non_numeric() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Str("a".into())))),
        op: AstBinOp::Range,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("10".into())))),
    });
    assert!(infer_expr(&expr, &env).is_err());
}

#[test]
fn in_operator_rejects_non_collection_rhs() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    env.insert("y".into(), Type::Int);
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        op: AstBinOp::In,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("y".into()))),
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert!(err.message.contains("collection"), "got: {}", err.message);
}

#[test]
fn in_operator_accepts_list() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        op: AstBinOp::In,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("xs".into()))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn in_operator_accepts_set() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    env.insert("s".into(), Type::Set(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        op: AstBinOp::In,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("s".into()))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn division_by_zero_literal_emits_error() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("10".into())))),
        op: AstBinOp::Div,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03010");
    assert!(
        err.message.contains("division by zero"),
        "got: {}",
        err.message
    );
}

#[test]
fn modulo_by_zero_literal_emits_error() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("10".into())))),
        op: AstBinOp::Mod,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03010");
    assert!(
        err.message.contains("modulo by zero"),
        "got: {}",
        err.message
    );
}

#[test]
fn division_by_nonzero_ok() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("10".into())))),
        op: AstBinOp::Div,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("3".into())))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

// -----------------------------------------------------------------------
// Field access on built-in types (Option, Result, Map, Set)
// -----------------------------------------------------------------------

#[test]
fn field_option_value() {
    let mut env = TypeEnv::new();
    env.insert("opt".into(), Type::Option(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("opt".into()))),
        "value".into(),
    ));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn field_option_is_some() {
    let mut env = TypeEnv::new();
    env.insert("opt".into(), Type::Option(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("opt".into()))),
        "is_some".into(),
    ));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn field_result_ok_type() {
    let mut env = TypeEnv::new();
    env.insert(
        "r".into(),
        Type::Result(Box::new(Type::String), Box::new(Type::Int)),
    );
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("r".into()))),
        "ok".into(),
    ));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::String);
}

#[test]
fn field_result_err_type() {
    let mut env = TypeEnv::new();
    env.insert(
        "r".into(),
        Type::Result(Box::new(Type::String), Box::new(Type::Int)),
    );
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("r".into()))),
        "err".into(),
    ));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn field_result_is_ok() {
    let mut env = TypeEnv::new();
    env.insert(
        "r".into(),
        Type::Result(Box::new(Type::String), Box::new(Type::Int)),
    );
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("r".into()))),
        "is_ok".into(),
    ));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn field_map_keys_values() {
    let mut env = TypeEnv::new();
    env.insert(
        "m".into(),
        Type::Map(Box::new(Type::String), Box::new(Type::Int)),
    );
    let keys_expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("m".into()))),
        "keys".into(),
    ));
    assert_eq!(
        infer_expr(&keys_expr, &env).unwrap(),
        Type::List(Box::new(Type::String))
    );
    let vals_expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("m".into()))),
        "values".into(),
    ));
    assert_eq!(
        infer_expr(&vals_expr, &env).unwrap(),
        Type::List(Box::new(Type::Int))
    );
}

#[test]
fn field_collection_is_empty() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("xs".into()))),
        "is_empty".into(),
    ));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

// -----------------------------------------------------------------------
// Method call on String, Option, Result types
// -----------------------------------------------------------------------

#[test]
fn method_string_contains() {
    let mut env = TypeEnv::new();
    env.insert("s".into(), Type::String);
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("s".into()))),
        method: "contains".into(),
        args: vec![Spanned::no_span(AstExpr::Literal(AstLit::Str("x".into())))],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn method_string_to_uppercase() {
    let mut env = TypeEnv::new();
    env.insert("s".into(), Type::String);
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("s".into()))),
        method: "to_uppercase".into(),
        args: vec![],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::String);
}

#[test]
fn method_option_unwrap() {
    let mut env = TypeEnv::new();
    env.insert("opt".into(), Type::Option(Box::new(Type::Float)));
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("opt".into()))),
        method: "unwrap".into(),
        args: vec![],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Float);
}

#[test]
fn method_option_is_some() {
    let mut env = TypeEnv::new();
    env.insert("opt".into(), Type::Option(Box::new(Type::Float)));
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("opt".into()))),
        method: "is_some".into(),
        args: vec![],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn method_result_unwrap() {
    let mut env = TypeEnv::new();
    env.insert(
        "r".into(),
        Type::Result(Box::new(Type::Nat), Box::new(Type::String)),
    );
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("r".into()))),
        method: "unwrap".into(),
        args: vec![],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Nat);
}

#[test]
fn method_result_is_ok() {
    let mut env = TypeEnv::new();
    env.insert(
        "r".into(),
        Type::Result(Box::new(Type::Nat), Box::new(Type::String)),
    );
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("r".into()))),
        method: "is_ok".into(),
        args: vec![],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn method_set_insert() {
    let mut env = TypeEnv::new();
    env.insert("s".into(), Type::Set(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("s".into()))),
        method: "insert".into(),
        args: vec![Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unit);
}

// -----------------------------------------------------------------------
// Match pattern variable binding
// -----------------------------------------------------------------------

#[test]
fn match_binds_ident_pattern_to_scrutinee_type() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    // match x { val => val + 1 } should bind `val` to Int
    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        arms: vec![assura_parser::ast::MatchArm {
            pattern: assura_parser::ast::Pattern::Ident("val".into()),
            body: Spanned::no_span(AstExpr::BinOp {
                lhs: Box::new(Spanned::no_span(AstExpr::Ident("val".into()))),
                op: AstBinOp::Add,
                rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
            }),
        }],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn match_wildcard_does_not_bind() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("42".into())))),
        arms: vec![assura_parser::ast::MatchArm {
            pattern: assura_parser::ast::Pattern::Wildcard,
            body: Spanned::no_span(AstExpr::Literal(AstLit::Bool(true))),
        }],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn match_tuple_pattern_binds_element_types() {
    let mut env = TypeEnv::new();
    env.insert("pair".into(), Type::Tuple(vec![Type::Int, Type::Bool]));
    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Ident("pair".into()))),
        arms: vec![assura_parser::ast::MatchArm {
            pattern: assura_parser::ast::Pattern::Tuple(vec![
                assura_parser::ast::Pattern::Ident("a".into()),
                assura_parser::ast::Pattern::Ident("b".into()),
            ]),
            // body uses 'a' which should be Int from pair[0]
            body: Spanned::no_span(AstExpr::Ident("a".into())),
        }],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn match_literal_pattern_does_not_bind() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Literal(AstLit::Int("1".into())),
                body: Spanned::no_span(AstExpr::Literal(AstLit::Str("one".into()))),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Wildcard,
                body: Spanned::no_span(AstExpr::Literal(AstLit::Str("other".into()))),
            },
        ],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::String);
}

#[test]
fn match_constructor_pattern_binds_field_types() {
    // Register an enum variant constructor: Some(Int) -> Option
    let mut env = TypeEnv::new();
    env.insert(
        "Some".into(),
        Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(Type::Named("Option".into())),
        },
    );
    env.insert("val".into(), Type::Named("Option".into()));
    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Ident("val".into()))),
        arms: vec![assura_parser::ast::MatchArm {
            pattern: assura_parser::ast::Pattern::Constructor {
                name: "Some".into(),
                fields: vec![assura_parser::ast::Pattern::Ident("x".into())],
            },
            // body uses 'x' which should be Int from Some's first param
            body: Spanned::no_span(AstExpr::Ident("x".into())),
        }],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn match_constructor_pattern_multi_field() {
    // Register a constructor: Pair(Int, Bool) -> Pair
    let mut env = TypeEnv::new();
    env.insert(
        "Pair".into(),
        Type::Fn {
            params: vec![Type::Int, Type::Bool],
            ret: Box::new(Type::Named("PairType".into())),
        },
    );
    env.insert("p".into(), Type::Named("PairType".into()));
    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Ident("p".into()))),
        arms: vec![assura_parser::ast::MatchArm {
            pattern: assura_parser::ast::Pattern::Constructor {
                name: "Pair".into(),
                fields: vec![
                    assura_parser::ast::Pattern::Ident("a".into()),
                    assura_parser::ast::Pattern::Ident("b".into()),
                ],
            },
            // body uses 'b' which should be Bool from Pair's second param
            body: Spanned::no_span(AstExpr::Ident("b".into())),
        }],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn self_in_service_context_resolves_to_named_type() {
    let mut env = TypeEnv::new();
    env.insert("self".to_string(), Type::Named("FileStore".into()));
    let expr = Spanned::no_span(AstExpr::Ident("self".into()));
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Named("FileStore".into())
    );
}

#[test]
fn self_field_access_in_service() {
    let mut env = TypeEnv::new();
    env.insert("self".to_string(), Type::Named("FileStore".into()));
    env.struct_fields.insert(
        "FileStore".into(),
        vec![("state".into(), Type::Named("State".into()))],
    );
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("self".into()))),
        "state".into(),
    ));
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Named("State".into())
    );
}

#[test]
fn self_without_binding_returns_unknown() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Ident("self".into()));
    // Outside a service context, self is Unknown
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn extract_output_type_from_raw_tokens() {
    // output(result: Nat) is parsed as Raw(["result", ":", "Nat"])
    let body = Spanned::no_span(AstExpr::Raw(vec![
        "result".into(),
        ":".into(),
        "Nat".into(),
    ]));
    assert_eq!(extract_output_type_from_body(&body), Type::Nat);
}

#[test]
fn extract_output_type_from_cast() {
    let body = Spanned::no_span(AstExpr::Cast {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("result".into()))),
        ty: "Int".into(),
    });
    assert_eq!(extract_output_type_from_body(&body), Type::Int);
}

#[test]
fn extract_output_type_from_raw_generic() {
    // output(result: List<Int>) from raw tokens
    let body = Spanned::no_span(AstExpr::Raw(vec![
        "result".into(),
        ":".into(),
        "List".into(),
        "<".into(),
        "Int".into(),
        ">".into(),
    ]));
    assert_eq!(
        extract_output_type_from_body(&body),
        Type::List(Box::new(Type::Int))
    );
}

#[test]
fn service_query_output_binds_result_type() {
    // Full pipeline: service with query that has output(result: Nat)
    // and ensures { result >= 0 } should type-check without errors
    let src = r#"
service Counter {
  query Value {
output(result: Nat)
ensures { result >= 0 }
  }
}
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(resolved);
    assert!(
        typed.is_ok(),
        "query with output(result: Nat) and ensures should pass: {:?}",
        typed.err()
    );
}

#[test]
fn service_operation_registered_as_fn_type() {
    // After type checking, service operations should be registered as
    // Type::Fn with the proper param types and return type in the env.
    let src = r#"
service UserService {
  operation CreateUser {
input(name: String, age: Nat)
output(result: Bool)
requires { age > 0 }
ensures { result == true }
  }
  query GetAge {
input(name: String)
output(result: Nat)
ensures { result >= 0 }
  }
}
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(resolved);
    assert!(
        typed.is_ok(),
        "service with typed ops should pass: {:?}",
        typed.err()
    );
    let typed = typed.unwrap();
    // Check that CreateUser is registered as a function type
    if let Some(ty) = typed.type_env.lookup("CreateUser") {
        match ty {
            Type::Fn { params, ret } => {
                assert_eq!(params.len(), 2, "CreateUser should have 2 params");
                assert_eq!(**ret, Type::Bool, "CreateUser should return Bool");
            }
            other => panic!("CreateUser should be Fn, got {:?}", other),
        }
    } else {
        panic!("CreateUser should be in type env");
    }
    // Check that GetAge is registered as a function type
    if let Some(ty) = typed.type_env.lookup("GetAge") {
        match ty {
            Type::Fn { params, ret } => {
                assert_eq!(params.len(), 1, "GetAge should have 1 param");
                assert_eq!(params[0], Type::String, "GetAge param should be String");
                assert_eq!(**ret, Type::Nat, "GetAge should return Nat");
            }
            other => panic!("GetAge should be Fn, got {:?}", other),
        }
    } else {
        panic!("GetAge should be in type env");
    }
}

#[test]
fn service_operation_raw_token_params() {
    // Test that raw token input clauses are also properly typed
    let src = r#"
service Store {
  operation Insert {
input(key: String, value: Int)
output(result: Bool)
  }
}
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(resolved);
    assert!(
        typed.is_ok(),
        "store service should pass: {:?}",
        typed.err()
    );
    let typed = typed.unwrap();
    if let Some(Type::Fn { params, ret }) = typed.type_env.lookup("Insert") {
        assert_eq!(params.len(), 2);
        assert_eq!(**ret, Type::Bool);
    } else {
        panic!("Insert should be Fn type in env");
    }
}

#[test]
fn service_operation_no_input_output_clauses() {
    // Operation with only requires/ensures but no input/output
    // should register as Fn { params: [], ret: Unit }
    let src = r#"
service Pinger {
  operation Ping {
ensures { true }
  }
}
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(resolved);
    let typed = typed.expect("no-io op should pass");
    if let Some(Type::Fn { params, ret }) = typed.type_env.lookup("Ping") {
        assert!(params.is_empty(), "Ping should have 0 params");
        assert_eq!(**ret, Type::Unit, "Ping should return Unit");
    } else {
        panic!("Ping should be Fn type in env");
    }
}

#[test]
fn service_invariant_only_no_crash() {
    // Service with only invariants and no operations should not crash
    let src = r#"
service Guardian {
  invariant { true }
}
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(resolved);
    assert!(
        typed.is_ok(),
        "invariant-only service should pass: {:?}",
        typed.err()
    );
}

// ---- register_input_clause_params coverage ----

#[test]
fn input_clause_single_ident() {
    // input { x } should register x as Unknown
    let mut env = TypeEnv::new();
    let body = Spanned::no_span(Expr::Ident("x".into()));
    register_input_clause_params(&body, &mut env);
    assert_eq!(env.lookup("x"), Some(&Type::Unknown));
}

#[test]
fn input_clause_single_cast() {
    // input(a as Int) at top level
    let mut env = TypeEnv::new();
    let body = Spanned::no_span(Expr::Cast {
        expr: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        ty: "Int".into(),
    });
    register_input_clause_params(&body, &mut env);
    assert_eq!(env.lookup("a"), Some(&Type::Int));
}

#[test]
fn input_clause_paren_wraps_call() {
    // Paren-wrapped call: input((a as Int))
    let mut env = TypeEnv::new();
    let inner_call = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("input".into()))),
        args: vec![Spanned::no_span(Expr::Cast {
            expr: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
            ty: "Int".into(),
        })],
    });
    let body = inner_call;
    register_input_clause_params(&body, &mut env);
    assert_eq!(env.lookup("a"), Some(&Type::Int));
}

#[test]
fn input_clause_raw_with_as() {
    // Raw tokens: "a as Int , b as String"
    let mut env = TypeEnv::new();
    let tokens = vec![
        "a".into(),
        "as".into(),
        "Int".into(),
        ",".into(),
        "b".into(),
        "as".into(),
        "String".into(),
    ];
    let body = Spanned::no_span(Expr::Raw(tokens));
    register_input_clause_params(&body, &mut env);
    assert_eq!(env.lookup("a"), Some(&Type::Int));
    assert_eq!(env.lookup("b"), Some(&Type::String));
}

#[test]
fn input_clause_raw_bare_idents() {
    // Raw tokens: "buf , n" — bare identifiers without type annotations
    let mut env = TypeEnv::new();
    let tokens = vec!["buf".into(), ",".into(), "n".into()];
    let body = Spanned::no_span(Expr::Raw(tokens));
    register_input_clause_params(&body, &mut env);
    assert_eq!(env.lookup("buf"), Some(&Type::Unknown));
    assert_eq!(env.lookup("n"), Some(&Type::Unknown));
}

#[test]
fn collect_input_types_single_cast() {
    let body = Spanned::no_span(Expr::Cast {
        expr: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        ty: "Int".into(),
    });
    let mut out = Vec::new();
    collect_input_param_types(&body, &mut out);
    assert_eq!(out, vec![Type::Int]);
}

#[test]
fn collect_input_types_single_ident() {
    let body = Spanned::no_span(Expr::Ident("x".into()));
    let mut out = Vec::new();
    collect_input_param_types(&body, &mut out);
    assert_eq!(out, vec![Type::Unknown]);
}

#[test]
fn collect_input_types_raw_as() {
    let tokens = vec![
        "a".into(),
        "as".into(),
        "Int".into(),
        ",".into(),
        "b".into(),
        "as".into(),
        "Bool".into(),
    ];
    let body = Spanned::no_span(Expr::Raw(tokens));
    let mut out = Vec::new();
    collect_input_param_types(&body, &mut out);
    assert_eq!(out, vec![Type::Int, Type::Bool]);
}

#[test]
fn collect_input_types_raw_bare_idents() {
    let tokens = vec!["x".into(), ",".into(), "y".into()];
    let body = Spanned::no_span(Expr::Raw(tokens));
    let mut out = Vec::new();
    collect_input_param_types(&body, &mut out);
    assert_eq!(out, vec![Type::Unknown, Type::Unknown]);
}

// ---- declare_linear_params_from_expr coverage ----

#[test]
fn linear_from_cast() {
    // input(handle as linear FileHandle)
    let mut tracker = UsageTracker::new();
    let body = Spanned::no_span(Expr::Cast {
        expr: Box::new(Spanned::no_span(Expr::Ident("handle".into()))),
        ty: "linear FileHandle".into(),
    });
    declare_linear_params_from_expr(&body, &mut tracker, &(0..1));
    assert_eq!(
        tracker.get_count("handle"),
        Some(0),
        "handle should be declared as linear"
    );
}

#[test]
fn linear_from_call_args() {
    // input(h as linear File, n as Int) — only h is linear
    let mut tracker = UsageTracker::new();
    let body = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("input".into()))),
        args: vec![
            Spanned::no_span(Expr::Cast {
                expr: Box::new(Spanned::no_span(Expr::Ident("h".into()))),
                ty: "linear File".into(),
            }),
            Spanned::no_span(Expr::Cast {
                expr: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
                ty: "Int".into(),
            }),
        ],
    });
    declare_linear_params_from_expr(&body, &mut tracker, &(0..1));
    assert_eq!(
        tracker.get_count("h"),
        Some(0),
        "h should be declared as linear"
    );
    assert_eq!(
        tracker.get_count("n"),
        None,
        "n should NOT be declared as linear"
    );
}

#[test]
fn linear_from_raw_with_colon() {
    // Raw tokens: "x : linear Int , y : Int"
    let mut tracker = UsageTracker::new();
    let tokens = vec![
        "x".into(),
        ":".into(),
        "linear".into(),
        "Int".into(),
        ",".into(),
        "y".into(),
        ":".into(),
        "Int".into(),
    ];
    let body = Spanned::no_span(Expr::Raw(tokens));
    declare_linear_params_from_expr(&body, &mut tracker, &(0..1));
    assert_eq!(
        tracker.get_count("x"),
        Some(0),
        "x should be declared as linear"
    );
    assert_eq!(
        tracker.get_count("y"),
        None,
        "y should NOT be declared as linear"
    );
}

#[test]
fn linear_from_raw_with_as() {
    // Raw tokens: "handle as linear Resource"
    let mut tracker = UsageTracker::new();
    let tokens = vec![
        "handle".into(),
        "as".into(),
        "linear".into(),
        "Resource".into(),
    ];
    let body = Spanned::no_span(Expr::Raw(tokens));
    declare_linear_params_from_expr(&body, &mut tracker, &(0..1));
    assert_eq!(
        tracker.get_count("handle"),
        Some(0),
        "handle should be declared as linear"
    );
}

#[test]
fn linear_from_cast_direct() {
    // Direct Cast
    let mut tracker = UsageTracker::new();
    let body = Spanned::no_span(Expr::Cast {
        expr: Box::new(Spanned::no_span(Expr::Ident("buf".into()))),
        ty: "linear Buffer".into(),
    });
    declare_linear_params_from_expr(&body, &mut tracker, &(0..1));
    assert_eq!(
        tracker.get_count("buf"),
        Some(0),
        "buf should be declared as linear via Cast"
    );
}

// -----------------------------------------------------------------------
// S002: Call-graph effect inference tests
// -----------------------------------------------------------------------

#[test]
fn effect_callgraph_caller_has_callee_effects_ok() {
    // Contract read_data declares effects {io}
    // Contract process declares effects {io} and calls read_data -> OK
    let resolved = resolve_ok(
        r#"
contract ReadData {
    effects { io }
    input(path: String)
    output(result: String)
    ensures: result.length() > 0
}

contract Process {
    effects { io }
    input(path: String)
    output(result: String)
    ensures: ReadData(path).length() > 0
}
"#,
    );
    let result = type_check(resolved);
    // The call-graph check for contracts is based on names in the effect map.
    // ReadData is a contract name with effects {io}, so if Process references
    // ReadData in its ensures, the inferred callee effects include io.
    // Since Process also declares io, this should pass.
    assert!(
        result.is_ok(),
        "caller with io calling callee with io should pass: {result:?}"
    );
}

#[test]
fn effect_callgraph_unit_check_containment() {
    // Direct unit test of EffectChecker::check_containment for call-graph scenario.
    // fn outer() effects {pure} calls fn inner() effects {io} -> A07002
    let checker = EffectChecker::new();
    let outer_declared = EffectSet::pure(); // empty = pure
    // The call-graph inference found that inner has io effects
    let inferred_callee_effects = EffectSet::from_effect_names(["io"]);
    let errors = checker.check_containment(&outer_declared, &inferred_callee_effects, &(0..10));
    assert_eq!(errors.len(), 1, "should have 1 error: {errors:?}");
    assert_eq!(errors[0].code, "A07002"); // pure function performs io
}

#[test]
fn effect_callgraph_missing_subset() {
    // fn outer() effects {database} calls fn inner() effects {io} -> A07001
    let checker = EffectChecker::new();
    let outer_declared = EffectSet::from_effect_names(["database"]);
    let inferred_callee_effects = EffectSet::from_effect_names(["io"]);
    let errors = checker.check_containment(&outer_declared, &inferred_callee_effects, &(0..10));
    assert_eq!(errors.len(), 1, "should have 1 error: {errors:?}");
    assert_eq!(errors[0].code, "A07001"); // undeclared effect
}

#[test]
fn effect_callgraph_build_effect_map() {
    // Verify build_effect_map collects effects from contracts
    let resolved = resolve_ok(
        r#"
contract IoContract {
    effects { io }
    input(x: Int)
    output(result: Int)
    ensures: result >= 0
}

contract PureContract {
    input(x: Int)
    output(result: Int)
    ensures: result >= 0
}
"#,
    );
    let checker = EffectChecker::new();
    let map = super::build_effect_map(&resolved.source, &checker);
    // IoContract should be in the map with expanded io effects
    assert!(
        map.contains_key("IoContract"),
        "IoContract should be in effect map"
    );
    let io_effects = &map["IoContract"];
    assert!(
        !io_effects.is_pure(),
        "IoContract should have non-pure effects"
    );
    // PureContract has no effects clause so should NOT be in the map
    assert!(
        !map.contains_key("PureContract"),
        "PureContract without effects clause should not be in map"
    );
}

#[test]
fn effect_callgraph_pure_callee_ok() {
    // Contract with no effects clause is implicitly pure
    // Caller with effects {io} calling it -> OK
    let checker = EffectChecker::new();
    let caller_declared = EffectSet::from_effect_names(["io"]);
    let callee_effects = EffectSet::pure(); // no effects = pure
    let errors = checker.check_containment(&caller_declared, &callee_effects, &(0..10));
    assert!(
        errors.is_empty(),
        "calling pure function from effectful context should pass"
    );
}

// -----------------------------------------------------------------------
// S003: Information flow tracking tests
// -----------------------------------------------------------------------

#[test]
fn s003_info_flow_no_labels_no_errors() {
    // Contract without security labels should produce no info flow errors
    let resolved = resolve_ok(
        r#"
contract Plain {
    input(x: Int, y: Int)
    output(result: Int)
    ensures: result == x + y
}
"#,
    );
    let result = type_check(resolved);
    assert!(
        result.is_ok(),
        "contract without security labels should pass: {result:?}"
    );
}

#[test]
fn s003_info_flow_secret_to_result_a08001() {
    // Secret data flowing directly to result should produce A08001
    let checker = InfoFlowChecker::new();
    let mut checker = checker;
    checker.declare("key".into(), SecurityLabel::Restricted);

    // Simulate: result == key (secret data flows to public output)
    let err = checker.check_assignment(SecurityLabel::Public, SecurityLabel::Restricted, &(0..10));
    assert!(err.is_some(), "should detect secret->public flow");
    assert_eq!(err.unwrap().code, "A08001");
}

#[test]
fn s003_info_flow_implicit_flow_a08004() {
    // Secret condition controlling public output is implicit flow
    let checker = InfoFlowChecker::new();
    let err =
        checker.check_implicit_flow(SecurityLabel::Confidential, SecurityLabel::Public, &(0..10));
    assert!(err.is_some(), "should detect implicit flow");
    assert_eq!(err.unwrap().code, "A08004");
}

#[test]
fn s003_info_flow_same_level_ok() {
    // Same level assignment should produce no error
    let checker = InfoFlowChecker::new();
    let err = checker.check_assignment(
        SecurityLabel::Confidential,
        SecurityLabel::Confidential,
        &(0..10),
    );
    assert!(err.is_none(), "same-level assignment should pass");
}

#[test]
fn s003_info_flow_upward_flow_ok() {
    // Public -> Confidential is upward flow (allowed)
    let checker = InfoFlowChecker::new();
    let err =
        checker.check_assignment(SecurityLabel::Confidential, SecurityLabel::Public, &(0..10));
    assert!(err.is_none(), "upward flow should pass");
}

#[test]
fn s003_info_flow_label_inference_through_binop() {
    // Binary op on secret and public yields secret
    let mut checker = InfoFlowChecker::new();
    checker.declare("secret_key".into(), SecurityLabel::Restricted);
    checker.declare("public_data".into(), SecurityLabel::Public);

    let expr = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("secret_key".into()))),
        op: BinOp::Add,
        rhs: Box::new(Spanned::no_span(Expr::Ident("public_data".into()))),
    });
    let label = checker.infer_label(&expr);
    assert_eq!(
        label,
        SecurityLabel::Restricted,
        "binop with secret operand should be Restricted"
    );
}

#[test]
fn s003_info_flow_contract_with_secret_input() {
    // Contract with 'secret' keyword in input should trigger info flow checking
    let resolved = resolve_ok(
        r#"
contract SecureHash {
    input(secret key: Bytes, data: Bytes)
    output(result: Bytes)
    ensures: result.length() > 0
}
"#,
    );
    // This should type-check OK because ensures doesn't directly flow
    // secret key to result (just checks length)
    let result = type_check(resolved);
    assert!(
        result.is_ok(),
        "secret input not flowing to result should pass: {result:?}"
    );
}

// -----------------------------------------------------------------------
// S004: Context splitting for linear types at match arms + ghost uses
// -----------------------------------------------------------------------

#[test]
fn s004_match_consistent_usage_ok() {
    // Linear var used once in each match arm: consistent, no error.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Literal(AstLit::Int("1".into())),
                body: Spanned::no_span(AstExpr::Ident("x".into())),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Wildcard,
                body: Spanned::no_span(AstExpr::Ident("x".into())),
            },
        ],
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(
        branch_errors.is_empty(),
        "consistent match arms should have no A05004: {branch_errors:?}"
    );
    let final_errors = ctx.check();
    assert!(
        final_errors.is_empty(),
        "used exactly once from each arm: {final_errors:?}"
    );
}

#[test]
fn s004_match_inconsistent_usage_a05004() {
    // Linear var used in first arm but not second: A05004.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Literal(AstLit::Int("1".into())),
                body: Spanned::no_span(AstExpr::Ident("x".into())),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Wildcard,
                body: Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into()))),
            },
        ],
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert_eq!(branch_errors.len(), 1);
    assert_eq!(branch_errors[0].code, "A05004");
    assert!(branch_errors[0].message.contains("x"));
    assert!(branch_errors[0].message.contains("match arms"));
}

#[test]
fn s004_match_three_arms_one_differs_a05004() {
    // Three arms: first two use x, third does not.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Literal(AstLit::Int("1".into())),
                body: Spanned::no_span(AstExpr::Ident("x".into())),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Literal(AstLit::Int("2".into())),
                body: Spanned::no_span(AstExpr::Ident("x".into())),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Wildcard,
                body: Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into()))),
            },
        ],
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert_eq!(branch_errors.len(), 1, "one error for x: {branch_errors:?}");
    assert_eq!(branch_errors[0].code, "A05004");
}

#[test]
fn s004_match_scrutinee_uses_linear_var() {
    // Using a linear var in the scrutinee (always evaluated) plus in
    // each arm: total 2 uses, should produce A05001 (used more than once).
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    let expr = Spanned::no_span(AstExpr::Match {
        scrutinee: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Literal(AstLit::Int("1".into())),
                body: Spanned::no_span(AstExpr::Ident("x".into())),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Wildcard,
                body: Spanned::no_span(AstExpr::Ident("x".into())),
            },
        ],
    });
    let _branch_errors = check_expr_linearity(&expr, &mut ctx);
    // Final check: 1 (scrutinee) + 1 (max from arms) = 2 total.
    let final_errors = ctx.check();
    assert!(
        final_errors.iter().any(|e| e.code == "A05001"),
        "x used twice (scrutinee + arm) should produce A05001: {final_errors:?}"
    );
}

#[test]
fn s004_forall_body_is_ghost_use() {
    // A linear variable referenced in a forall body should NOT count
    // as a computational use (ghost/logical context per Spec 13.1).
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // forall i in range: i < x  (x is referenced but ghost)
    let expr = Spanned::no_span(AstExpr::Forall {
        var: "i".into(),
        domain: Box::new(Spanned::no_span(AstExpr::Ident("range".into()))),
        body: Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::Ident("i".into()))),
            op: AstBinOp::Lt,
            rhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        })),
    });
    let errors = check_expr_linearity(&expr, &mut ctx);
    assert!(errors.is_empty(), "forall body should not produce errors");

    // x is never used computationally, so count stays at 0.
    assert_eq!(ctx.get_count("x"), Some(0));
}

#[test]
fn s004_exists_body_is_ghost_use() {
    // Same as forall: exists body is ghost.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    let expr = Spanned::no_span(AstExpr::Exists {
        var: "i".into(),
        domain: Box::new(Spanned::no_span(AstExpr::Ident("range".into()))),
        body: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
    });
    let errors = check_expr_linearity(&expr, &mut ctx);
    assert!(errors.is_empty(), "exists body should not produce errors");
    assert_eq!(ctx.get_count("x"), Some(0));
}

#[test]
fn s004_old_expr_is_ghost_use() {
    // old(x) references pre-state, which is ghost/logical.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    let expr = Spanned::no_span(AstExpr::Old(Box::new(Spanned::no_span(AstExpr::Ident(
        "x".into(),
    )))));
    let errors = check_expr_linearity(&expr, &mut ctx);
    assert!(errors.is_empty(), "old(x) should not count as a use");
    assert_eq!(ctx.get_count("x"), Some(0));
}

#[test]
fn s004_ghost_block_is_not_a_use() {
    // Ghost blocks were already handled (existing behavior). Confirm.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    let expr = Spanned::no_span(AstExpr::Ghost(Box::new(Spanned::no_span(AstExpr::Ident(
        "x".into(),
    )))));
    let errors = check_expr_linearity(&expr, &mut ctx);
    assert!(errors.is_empty());
    assert_eq!(ctx.get_count("x"), Some(0));
}

#[test]
fn s004_merge_arms_unit_test() {
    // Direct test of merge_arms with 3 arms.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    tracker.declare("y".into(), UsageGrade::Linear, 0..1);
    let mut base = LinearContext::new(tracker);

    // Arm 1: uses x once
    let mut arm1 = base.clone();
    arm1.use_var("x");

    // Arm 2: uses x once (consistent with arm 1)
    let mut arm2 = base.clone();
    arm2.use_var("x");

    // Arm 3: uses x once (all consistent)
    let mut arm3 = base.clone();
    arm3.use_var("x");

    let errors = base.merge_arms(&[arm1, arm2, arm3]);
    assert!(errors.is_empty(), "all arms consistent: {errors:?}");

    // x should have count 1 after merge.
    assert_eq!(base.get_count("x"), Some(1));
    // y was not used in any arm; final check will catch it.
    assert_eq!(base.get_count("y"), Some(0));
}

// ===========================================================================
