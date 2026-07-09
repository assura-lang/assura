use super::*;
// T016: Field access and function call type checking tests
// -----------------------------------------------------------------------

#[test]
fn infer_field_on_named_type_is_unknown() {
    let mut env = TypeEnv::new();
    env.insert("p".into(), Type::Named("Point".into()));
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("p".into()))),
        "x".into(),
    ));
    // Named type without struct field info returns Unknown
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_field_resolves_struct_field() {
    let mut env = TypeEnv::new();
    env.insert("p".into(), Type::Named("Point".into()));
    env.struct_fields.insert(
        "Point".into(),
        vec![("x".into(), Type::Int), ("y".into(), Type::Int)],
    );
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("p".into()))),
        "x".into(),
    ));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_field_unknown_field_on_known_struct() {
    let mut env = TypeEnv::new();
    env.insert("p".into(), Type::Named("Point".into()));
    env.struct_fields
        .insert("Point".into(), vec![("x".into(), Type::Int)]);
    // Accessing unknown field on registered struct emits A03005
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("p".into()))),
        "z".into(),
    ));
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03005");
    assert!(err.message.contains("unknown field `z`"));
}

#[test]
fn unknown_field_on_list_emits_a03005() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("xs".into()))),
        "bogus".into(),
    ));
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03005");
    assert!(err.message.contains("unknown field `bogus`"));
}

#[test]
fn unknown_method_on_list_emits_a03005() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("xs".into()))),
        method: "bogus_method".into(),
        args: vec![],
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03005");
    assert!(err.message.contains("unknown method `bogus_method`"));
}

#[test]
fn map_keys_returns_set() {
    let mut env = TypeEnv::new();
    env.insert(
        "m".into(),
        Type::Map(Box::new(Type::String), Box::new(Type::Int)),
    );
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("m".into()))),
        method: "keys".into(),
        args: vec![],
    });
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Set(Box::new(Type::String))
    );
}

#[test]
fn map_values_returns_list() {
    let mut env = TypeEnv::new();
    env.insert(
        "m".into(),
        Type::Map(Box::new(Type::String), Box::new(Type::Int)),
    );
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("m".into()))),
        method: "values".into(),
        args: vec![],
    });
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::List(Box::new(Type::Int))
    );
}

#[test]
fn set_union_returns_set() {
    let mut env = TypeEnv::new();
    env.insert("s".into(), Type::Set(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("s".into()))),
        method: "union".into(),
        args: vec![Spanned::no_span(AstExpr::Ident("s".into()))],
    });
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Set(Box::new(Type::Int))
    );
}

#[test]
fn string_split_returns_list() {
    let mut env = TypeEnv::new();
    env.insert("s".into(), Type::String);
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("s".into()))),
        method: "split".into(),
        args: vec![],
    });
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::List(Box::new(Type::String))
    );
}

#[test]
fn bytes_len_returns_nat() {
    let mut env = TypeEnv::new();
    env.insert("data".into(), Type::Bytes);
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("data".into()))),
        method: "len".into(),
        args: vec![],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Nat);
}

#[test]
fn unknown_method_on_bytes_emits_a03005() {
    let mut env = TypeEnv::new();
    env.insert("data".into(), Type::Bytes);
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("data".into()))),
        method: "bogus".into(),
        args: vec![],
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03005");
}

#[test]
fn unknown_field_on_option_emits_a03005() {
    let mut env = TypeEnv::new();
    env.insert("opt".into(), Type::Option(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("opt".into()))),
        "nope".into(),
    ));
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03005");
}

#[test]
fn unknown_field_on_named_without_struct_fields_is_unknown() {
    // Named type with NO registered struct_fields stays lenient
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Named("SomeExternalType".into()));
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        "anything".into(),
    ));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_field_collection_len() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("xs".into()))),
        "len".into(),
    ));
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Nat);
}

#[test]
fn infer_method_collection_contains() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("xs".into()))),
        method: "contains".into(),
        args: vec![Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_method_list_get() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("xs".into()))),
        method: "get".into(),
        args: vec![Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))],
    });
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Option(Box::new(Type::Int))
    );
}

#[test]
fn field_resolution_from_ast() {
    let src = r#"
type Point {
  x: Int,
  y: Float
}
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(resolved).expect("type_check should succeed");
    assert_eq!(typed.type_env.lookup_field("Point", "x"), Some(&Type::Int));
    assert_eq!(
        typed.type_env.lookup_field("Point", "y"),
        Some(&Type::Float)
    );
}

#[test]
fn field_resolution_with_commas() {
    let src = r#"
type Point {
  x: Int,
  y: Float
}
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(resolved).expect("type_check should succeed");
    assert_eq!(typed.type_env.lookup_field("Point", "x"), Some(&Type::Int));
    assert_eq!(
        typed.type_env.lookup_field("Point", "y"),
        Some(&Type::Float)
    );
    assert_eq!(typed.type_env.lookup_field("Point", "z"), None);
}

#[test]
fn infer_field_surfaces_receiver_error() {
    let env = TypeEnv::new();
    // Field access on an expression that has an error inside:
    // (!42).field -> error inside unary !
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::UnaryOp {
            op: AstUnOp::Not,
            expr: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("42".into())))),
        })),
        "field".into(),
    ));
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn infer_call_fn_type_returns_ret() {
    let mut env = TypeEnv::new();
    env.insert(
        "add".into(),
        Type::Fn {
            params: vec![Type::Int, Type::Int],
            ret: Box::new(Type::Int),
        },
    );
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("add".into()))),
        args: vec![
            Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into()))),
            Spanned::no_span(AstExpr::Literal(AstLit::Int("2".into()))),
        ],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_call_wrong_arg_count_a03002() {
    let mut env = TypeEnv::new();
    env.insert(
        "inc".into(),
        Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(Type::Int),
        },
    );
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("inc".into()))),
        args: vec![
            Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into()))),
            Spanned::no_span(AstExpr::Literal(AstLit::Int("2".into()))),
        ],
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03002");
    assert!(err.message.contains("1"));
    assert!(err.message.contains("2"));
}

#[test]
fn infer_call_not_callable_a03001() {
    // Not-callable is A03001 (type mismatch), not A03005 (unknown field). #903
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        args: vec![],
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
    assert!(err.message.contains("Int"));
    assert!(err.message.contains("not callable"));
}

#[test]
fn infer_call_bool_not_callable_a03001() {
    let mut env = TypeEnv::new();
    env.insert("flag".into(), Type::Bool);
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("flag".into()))),
        args: vec![Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))],
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn infer_call_unknown_callee_is_lenient() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("unknown_fn".into()))),
        args: vec![Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_call_named_type_returns_named() {
    let mut env = TypeEnv::new();
    env.insert("MyType".into(), Type::Named("MyType".into()));
    // Calling a Named type returns that type (constructor pattern)
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("MyType".into()))),
        args: vec![Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))],
    });
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Named("MyType".into())
    );
}

#[test]
fn infer_call_fn_empty_params_skips_count_check() {
    let mut env = TypeEnv::new();
    // Functions from symbol table have empty params (not yet resolved)
    env.insert(
        "f".into(),
        Type::Fn {
            params: vec![],
            ret: Box::new(Type::Bool),
        },
    );
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("f".into()))),
        args: vec![
            Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into()))),
            Spanned::no_span(AstExpr::Literal(AstLit::Int("2".into()))),
        ],
    });
    // Empty params means we skip count check, return ret type
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_call_surfaces_arg_error() {
    let mut env = TypeEnv::new();
    env.insert(
        "f".into(),
        Type::Fn {
            params: vec![],
            ret: Box::new(Type::Unknown),
        },
    );
    // Argument has a type error inside it: true + false
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("f".into()))),
        args: vec![Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
            op: AstBinOp::Add,
            rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(false)))),
        })],
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn infer_method_call_is_unknown() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("obj".into()))),
        method: "do_something".into(),
        args: vec![Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_method_call_surfaces_receiver_error() {
    let env = TypeEnv::new();
    // receiver has a type error: true + 1
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
            op: AstBinOp::Add,
            rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        })),
        method: "m".into(),
        args: vec![],
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn infer_index_list_returns_element_type() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("xs".into()))),
        index: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_index_map_returns_value_type() {
    let mut env = TypeEnv::new();
    env.insert(
        "m".into(),
        Type::Map(Box::new(Type::String), Box::new(Type::Bool)),
    );
    let expr = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("m".into()))),
        index: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Str(
            "key".into(),
        )))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_index_sequence_returns_element_type() {
    let mut env = TypeEnv::new();
    env.insert("seq".into(), Type::Sequence(Box::new(Type::Float)));
    let expr = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("seq".into()))),
        index: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Float);
}

#[test]
fn infer_index_unknown_base_is_unknown() {
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("unknown".into()))),
        index: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_index_named_type_is_unknown() {
    let mut env = TypeEnv::new();
    env.insert("arr".into(), Type::Named("CustomArray".into()));
    let expr = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("arr".into()))),
        index: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
    });
    // Named type indexing returns Unknown (could be user-defined indexable)
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_index_surfaces_index_error() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    // Index expr has a type error: true && 42
    let expr = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("xs".into()))),
        index: Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
            op: AstBinOp::And,
            rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("42".into())))),
        })),
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn infer_index_bytes_returns_u8() {
    let mut env = TypeEnv::new();
    env.insert("data".into(), Type::Bytes);
    let expr = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("data".into()))),
        index: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::U8);
}

#[test]
fn infer_index_tuple_literal() {
    let mut env = TypeEnv::new();
    env.insert("pair".into(), Type::Tuple(vec![Type::Int, Type::Bool]));
    // pair[0] should be Int
    let expr = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("pair".into()))),
        index: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);

    // pair[1] should be Bool
    let expr1 = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("pair".into()))),
        index: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
    });
    assert_eq!(infer_expr(&expr1, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_index_bool_emits_error() {
    let mut env = TypeEnv::new();
    env.insert("flag".into(), Type::Bool);
    let expr = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("flag".into()))),
        index: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
    });
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03005");
}

#[test]
fn type_display_basic() {
    assert_eq!(format!("{}", Type::Int), "Int");
    assert_eq!(format!("{}", Type::Bool), "Bool");
    assert_eq!(format!("{}", Type::List(Box::new(Type::Int))), "List<Int>");
    assert_eq!(format!("{}", Type::Unknown), "Unknown");
}

#[test]
fn parse_type_tokens_tuple() {
    let tokens: Vec<String> = vec!["(", "Int", ",", "Bool", ")"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(
        parse_type_tokens(&tokens),
        Type::Tuple(vec![Type::Int, Type::Bool])
    );
}

#[test]
fn parse_type_tokens_nested_tuple() {
    let tokens: Vec<String> = vec!["(", "Int", ",", "(", "Bool", ",", "String", ")", ")"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(
        parse_type_tokens(&tokens),
        Type::Tuple(vec![Type::Int, Type::Tuple(vec![Type::Bool, Type::String])])
    );
}

#[test]
fn parse_type_tokens_empty_tuple_is_unit() {
    let tokens: Vec<String> = vec!["(", ")"].into_iter().map(String::from).collect();
    assert_eq!(parse_type_tokens(&tokens), Type::Unit);
}

#[test]
fn parse_type_tokens_refinement_preserves_predicate() {
    // { x : Int | x > 0 }
    let tokens: Vec<String> = vec!["{", "x", ":", "Int", "|", "x", ">", "0", "}"]
        .into_iter()
        .map(String::from)
        .collect();
    let ty = parse_type_tokens(&tokens);
    assert_eq!(ty, Type::refined_from_str(Type::Int, "x", "x > 0"));
}

#[test]
fn parse_type_tokens_refinement_complex_predicate() {
    // { n : Nat | n >= 1 && n <= 100 }
    let tokens: Vec<String> = vec![
        "{", "n", ":", "Nat", "|", "n", ">=", "1", "&&", "n", "<=", "100", "}",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let ty = parse_type_tokens(&tokens);
    assert_eq!(
        ty,
        Type::refined_from_str(Type::Nat, "n", "n >= 1 && n <= 100")
    );
}

#[test]
fn parse_type_tokens_refinement_no_predicate() {
    // { x : Bool } (no pipe)
    let tokens: Vec<String> = vec!["{", "x", ":", "Bool", "}"]
        .into_iter()
        .map(String::from)
        .collect();
    let ty = parse_type_tokens(&tokens);
    assert_eq!(ty, Type::refined_from_str(Type::Bool, "x", ""));
}

#[test]
fn refinement_predicate_roundtrip_through_clause_params() {
    // Verify that a refinement type survives extraction via shared
    // extract_clause_params and then parse_type_tokens.
    // Input: raw tokens for `x : { n : Int | n > 0 }`
    use assura_parser::ast::{Expr, Spanned, extract_clause_params};
    let tokens: Vec<String> = vec!["x", ":", "{", "n", ":", "Int", "|", "n", ">", "0", "}"]
        .into_iter()
        .map(String::from)
        .collect();
    let body = Spanned::no_span(Expr::Raw(tokens));
    let params = extract_clause_params(&body);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "x");

    // Now parse the type tokens -- should produce Refined
    let ty_tokens = params[0]
        .ty
        .as_ref()
        .map(|t| t.to_tokens())
        .unwrap_or_default();
    let ty = parse_type_tokens(&ty_tokens);
    if let Type::Refined { ref base, .. } = ty {
        assert_eq!(**base, Type::Int);
        assert_eq!(ty.predicate_str(), Some("n > 0".into()));
    } else {
        panic!("expected Refined, got {ty:?}");
    }
}

#[test]
fn refinement_predicate_with_less_than_in_multi_param() {
    // Two params: a : { x : Int | x < 10 }, b : Bool
    // The `<` inside the refinement must not break param splitting.
    use assura_parser::ast::{Expr, extract_clause_params};
    let tokens: Vec<String> = vec![
        "a", ":", "{", "x", ":", "Int", "|", "x", "<", "10", "}", ",", "b", ":", "Bool",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let body = Spanned::no_span(Expr::Raw(tokens));
    let params = extract_clause_params(&body);
    assert_eq!(params.len(), 2);

    let ty_a_tokens = params[0]
        .ty
        .as_ref()
        .map(|t| t.to_tokens())
        .unwrap_or_default();
    let ty_a = parse_type_tokens(&ty_a_tokens);
    if let Type::Refined { ref base, .. } = ty_a {
        assert_eq!(**base, Type::Int);
        // Predicate may be stored as "x < 10" (single token) or "x < 10" (split)
        let pred = ty_a.predicate_str().unwrap_or_default();
        assert!(
            pred.contains("x") && pred.contains("10"),
            "expected x < 10, got {pred}"
        );
    } else {
        panic!("expected Refined, got {ty_a:?}");
    }

    let ty_b_tokens = params[1]
        .ty
        .as_ref()
        .map(|t| t.to_tokens())
        .unwrap_or_default();
    let ty_b = parse_type_tokens(&ty_b_tokens);
    assert_eq!(ty_b, Type::Bool);
}

#[test]
fn parse_type_tokens_fn_with_return() {
    // fn ( Int , Bool ) -> String
    let tokens: Vec<String> = vec!["fn", "(", "Int", ",", "Bool", ")", "->", "String"]
        .into_iter()
        .map(String::from)
        .collect();
    let ty = parse_type_tokens(&tokens);
    assert_eq!(
        ty,
        Type::Fn {
            params: vec![Type::Int, Type::Bool],
            ret: Box::new(Type::String),
        }
    );
}

#[test]
fn parse_type_tokens_fn_no_return() {
    // fn ( Nat ) -> Unit (implicit)
    let tokens: Vec<String> = vec!["fn", "(", "Nat", ")"]
        .into_iter()
        .map(String::from)
        .collect();
    let ty = parse_type_tokens(&tokens);
    assert_eq!(
        ty,
        Type::Fn {
            params: vec![Type::Nat],
            ret: Box::new(Type::Unit),
        }
    );
}

#[test]
fn parse_type_tokens_fn_no_params() {
    // fn ( ) -> Bool
    let tokens: Vec<String> = vec!["fn", "(", ")", "->", "Bool"]
        .into_iter()
        .map(String::from)
        .collect();
    let ty = parse_type_tokens(&tokens);
    assert_eq!(
        ty,
        Type::Fn {
            params: vec![],
            ret: Box::new(Type::Bool),
        }
    );
}

// -----------------------------------------------------------------------
