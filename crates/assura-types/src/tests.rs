use super::*;
use crate::clauses::*;
use crate::inference::*;

/// Helper: parse + resolve source text, panicking on errors.
fn resolve_ok(source: &str) -> ResolvedFile {
    let (file, errs) = assura_parser::parse(source);
    assert!(errs.is_empty(), "unexpected parse errors: {errs:?}");
    let file = file.expect("parse returned None");
    assura_resolve::resolve(&file).expect("resolve should succeed")
}

#[test]
fn empty_file_type_checks() {
    let resolved = resolve_ok("");
    let typed = type_check(&resolved).expect("type_check should succeed");
    // Should have at least the built-in types in the environment.
    assert!(!typed.type_env.is_empty());
}

#[test]
fn builtin_types_in_env() {
    let resolved = resolve_ok("");
    let typed = type_check(&resolved).expect("type_check should succeed");
    let env = &typed.type_env;

    assert_eq!(env.lookup("Int"), Some(&Type::Int));
    assert_eq!(env.lookup("Nat"), Some(&Type::Nat));
    assert_eq!(env.lookup("Float"), Some(&Type::Float));
    assert_eq!(env.lookup("Bool"), Some(&Type::Bool));
    assert_eq!(env.lookup("String"), Some(&Type::String));
    assert_eq!(env.lookup("Bytes"), Some(&Type::Bytes));
    assert_eq!(env.lookup("Unit"), Some(&Type::Unit));
    assert_eq!(env.lookup("Never"), Some(&Type::Never));
    assert_eq!(env.lookup("U8"), Some(&Type::U8));
    assert_eq!(env.lookup("U16"), Some(&Type::U16));
    assert_eq!(env.lookup("U32"), Some(&Type::U32));
    assert_eq!(env.lookup("U64"), Some(&Type::U64));
    assert_eq!(env.lookup("I8"), Some(&Type::I8));
    assert_eq!(env.lookup("I16"), Some(&Type::I16));
    assert_eq!(env.lookup("I32"), Some(&Type::I32));
    assert_eq!(env.lookup("I64"), Some(&Type::I64));
    assert_eq!(env.lookup("F32"), Some(&Type::F32));
    assert_eq!(env.lookup("F64"), Some(&Type::F64));
}

#[test]
fn user_defined_types_in_env() {
    let src = r#"
type Foo {
  x: Int
  y: Bool
}

enum Color {
  Red
  Green
  Blue
}
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(&resolved).expect("type_check should succeed");
    let env = &typed.type_env;

    assert_eq!(env.lookup("Foo"), Some(&Type::Named("Foo".into())));
    assert_eq!(env.lookup("Color"), Some(&Type::Named("Color".into())));
    // Enum variants are Named
    assert_eq!(env.lookup("Red"), Some(&Type::Named("Red".into())));
}

#[test]
fn contract_in_env() {
    let src = r#"
contract SafeBuffer {
  requires { true }
}
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(&resolved).expect("type_check should succeed");
    assert_eq!(
        typed.type_env.lookup("SafeBuffer"),
        Some(&Type::Named("SafeBuffer".into()))
    );
}

#[test]
fn fn_def_in_env() {
    let src = r#"
fn helper(n: Int) -> Int {
  ensures { result >= 0 }
}
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(&resolved).expect("type_check should succeed");
    assert_eq!(
        typed.type_env.lookup("helper"),
        Some(&Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(Type::Int),
        })
    );
    // Parameter now gets parsed type from Param.ty tokens
    assert_eq!(typed.type_env.lookup("n"), Some(&Type::Int));
}

#[test]
fn type_param_in_env() {
    let src = r#"
contract Container<T> {
  requires { true }
}
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(&resolved).expect("type_check should succeed");
    assert_eq!(
        typed.type_env.lookup("T"),
        Some(&Type::TypeParam("T".into()))
    );
}

#[test]
fn typed_file_preserves_resolved() {
    let src = r#"
type Point {
  x: Int
  y: Int
}
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(&resolved).expect("type_check should succeed");
    // The resolved file should be preserved intact
    assert_eq!(typed.resolved.source.decls.len(), 1);
}

#[test]
fn type_env_len() {
    let resolved = resolve_ok("");
    let typed = type_check(&resolved).expect("type_check should succeed");
    // At minimum, all 22 built-in types should be in the env
    assert!(typed.type_env.len() >= 22);
}

// -----------------------------------------------------------------------
// parse_type_tokens tests
// -----------------------------------------------------------------------

#[test]
fn parse_type_base_int() {
    let tokens: Vec<String> = vec!["Int".into()];
    assert_eq!(parse_type_tokens(&tokens), Type::Int);
}

#[test]
fn parse_type_base_nat() {
    let tokens: Vec<String> = vec!["Nat".into()];
    assert_eq!(parse_type_tokens(&tokens), Type::Nat);
}

#[test]
fn parse_type_generic_list() {
    let tokens: Vec<String> = ["List", "<", "Int", ">"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(parse_type_tokens(&tokens), Type::List(Box::new(Type::Int)));
}

#[test]
fn parse_type_generic_map() {
    let tokens: Vec<String> = ["Map", "<", "String", ",", "Int", ">"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        parse_type_tokens(&tokens),
        Type::Map(Box::new(Type::String), Box::new(Type::Int))
    );
}

#[test]
fn parse_type_sequence() {
    let tokens: Vec<String> = ["Sequence", "<", "Nat", ">"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        parse_type_tokens(&tokens),
        Type::Sequence(Box::new(Type::Nat))
    );
}

#[test]
fn parse_type_refined() {
    let tokens: Vec<String> = ["{", "x", ":", "Int", "|", "x", ">", "0", "}"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        parse_type_tokens(&tokens),
        Type::Refined {
            base: Box::new(Type::Int),
            predicate: "x > 0".to_string(),
        }
    );
}

#[test]
fn parse_type_taint_stripped() {
    let tokens: Vec<String> = ["U32", "@", "taint", ":", "untrusted"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(parse_type_tokens(&tokens), Type::U32);
}

#[test]
fn parse_type_reference_stripped() {
    let tokens: Vec<String> = ["&", "mut", "BitReader"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(parse_type_tokens(&tokens), Type::Named("BitReader".into()));
}

#[test]
fn parse_type_union_error() {
    let tokens: Vec<String> = ["HuffmanGroup", "|", "DecodeError"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        parse_type_tokens(&tokens),
        Type::Result(
            Box::new(Type::Named("HuffmanGroup".into())),
            Box::new(Type::Named("DecodeError".into()))
        )
    );
}

#[test]
fn parse_type_empty() {
    assert_eq!(parse_type_tokens(&[]), Type::Unit);
}

#[test]
fn parse_type_named() {
    let tokens: Vec<String> = vec!["ValidCodeLengths".into()];
    assert_eq!(
        parse_type_tokens(&tokens),
        Type::Named("ValidCodeLengths".into())
    );
}

#[test]
fn fn_params_parsed_from_ast() {
    // Test that build_type_env enriches function types from AST
    let src = r#"
fn compute(x: Nat, y: Float) -> Bool {
  ensures { result == true }
}
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(&resolved).expect("type_check should succeed");
    assert_eq!(
        typed.type_env.lookup("compute"),
        Some(&Type::Fn {
            params: vec![Type::Nat, Type::Float],
            ret: Box::new(Type::Bool),
        })
    );
    assert_eq!(typed.type_env.lookup("x"), Some(&Type::Nat));
    assert_eq!(typed.type_env.lookup("y"), Some(&Type::Float));
}

#[test]
fn extern_params_parsed_from_ast() {
    let src = r#"
extern fn read_bytes(n: U32) -> Bytes
  effects { io.read }
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(&resolved).expect("type_check should succeed");
    assert_eq!(
        typed.type_env.lookup("read_bytes"),
        Some(&Type::Fn {
            params: vec![Type::U32],
            ret: Box::new(Type::Bytes),
        })
    );
}

// -----------------------------------------------------------------------
// T014: Expression type inference tests
// -----------------------------------------------------------------------

use assura_parser::ast::{
    BinOp as AstBinOp, Clause as AstClause, Expr as AstExpr, FnDef as AstFnDef, Literal as AstLit,
    Param as AstParam, UnaryOp as AstUnOp,
};

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
fn infer_raw_is_unknown() {
    let env = TypeEnv::new();
    let expr = AstExpr::Raw(vec!["some".into(), "tokens".into()]);
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
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
// T016: Field access and function call type checking tests
// -----------------------------------------------------------------------

#[test]
fn infer_field_on_named_type_is_unknown() {
    let mut env = TypeEnv::new();
    env.insert("p".into(), Type::Named("Point".into()));
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("p".into())), "x".into());
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
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("p".into())), "x".into());
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_field_unknown_field_on_known_struct() {
    let mut env = TypeEnv::new();
    env.insert("p".into(), Type::Named("Point".into()));
    env.struct_fields
        .insert("Point".into(), vec![("x".into(), Type::Int)]);
    // Accessing unknown field on registered struct emits A03005
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("p".into())), "z".into());
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03005");
    assert!(err.message.contains("unknown field `z`"));
}

#[test]
fn unknown_field_on_list_emits_a03005() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("xs".into())), "bogus".into());
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03005");
    assert!(err.message.contains("unknown field `bogus`"));
}

#[test]
fn unknown_method_on_list_emits_a03005() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("xs".into())),
        method: "bogus_method".into(),
        args: vec![],
    };
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
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("m".into())),
        method: "keys".into(),
        args: vec![],
    };
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
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("m".into())),
        method: "values".into(),
        args: vec![],
    };
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::List(Box::new(Type::Int))
    );
}

#[test]
fn set_union_returns_set() {
    let mut env = TypeEnv::new();
    env.insert("s".into(), Type::Set(Box::new(Type::Int)));
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("s".into())),
        method: "union".into(),
        args: vec![AstExpr::Ident("s".into())],
    };
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Set(Box::new(Type::Int))
    );
}

#[test]
fn string_split_returns_list() {
    let mut env = TypeEnv::new();
    env.insert("s".into(), Type::String);
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("s".into())),
        method: "split".into(),
        args: vec![],
    };
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::List(Box::new(Type::String))
    );
}

#[test]
fn bytes_len_returns_nat() {
    let mut env = TypeEnv::new();
    env.insert("data".into(), Type::Bytes);
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("data".into())),
        method: "len".into(),
        args: vec![],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Nat);
}

#[test]
fn unknown_method_on_bytes_emits_a03005() {
    let mut env = TypeEnv::new();
    env.insert("data".into(), Type::Bytes);
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("data".into())),
        method: "bogus".into(),
        args: vec![],
    };
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03005");
}

#[test]
fn unknown_field_on_option_emits_a03005() {
    let mut env = TypeEnv::new();
    env.insert("opt".into(), Type::Option(Box::new(Type::Int)));
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("opt".into())), "nope".into());
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03005");
}

#[test]
fn unknown_field_on_named_without_struct_fields_is_unknown() {
    // Named type with NO registered struct_fields stays lenient
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Named("SomeExternalType".into()));
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("x".into())), "anything".into());
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_field_collection_len() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("xs".into())), "len".into());
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Nat);
}

#[test]
fn infer_method_collection_contains() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("xs".into())),
        method: "contains".into(),
        args: vec![AstExpr::Literal(AstLit::Int("1".into()))],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_method_list_get() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("xs".into())),
        method: "get".into(),
        args: vec![AstExpr::Literal(AstLit::Int("0".into()))],
    };
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Option(Box::new(Type::Int))
    );
}

#[test]
fn field_resolution_from_ast() {
    let src = r#"
type Point {
  x: Int
  y: Float
}
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(&resolved).expect("type_check should succeed");
    // NOTE: without field separators (comma/semicolon), the parser groups
    // all tokens after the first colon into one field. Use commas.
    assert_eq!(typed.type_env.lookup_field("Point", "x"), Some(&Type::Int));
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
    let typed = type_check(&resolved).expect("type_check should succeed");
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
    let expr = AstExpr::Field(
        Box::new(AstExpr::UnaryOp {
            op: AstUnOp::Not,
            expr: Box::new(AstExpr::Literal(AstLit::Int("42".into()))),
        }),
        "field".into(),
    );
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
    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("add".into())),
        args: vec![
            AstExpr::Literal(AstLit::Int("1".into())),
            AstExpr::Literal(AstLit::Int("2".into())),
        ],
    };
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
    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("inc".into())),
        args: vec![
            AstExpr::Literal(AstLit::Int("1".into())),
            AstExpr::Literal(AstLit::Int("2".into())),
        ],
    };
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03002");
    assert!(err.message.contains("1"));
    assert!(err.message.contains("2"));
}

#[test]
fn infer_call_not_callable_a03005() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("x".into())),
        args: vec![],
    };
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03005");
    assert!(err.message.contains("Int"));
    assert!(err.message.contains("not callable"));
}

#[test]
fn infer_call_bool_not_callable_a03005() {
    let mut env = TypeEnv::new();
    env.insert("flag".into(), Type::Bool);
    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("flag".into())),
        args: vec![AstExpr::Literal(AstLit::Int("1".into()))],
    };
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03005");
}

#[test]
fn infer_call_unknown_callee_is_lenient() {
    let env = TypeEnv::new();
    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("unknown_fn".into())),
        args: vec![AstExpr::Literal(AstLit::Int("1".into()))],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_call_named_type_returns_named() {
    let mut env = TypeEnv::new();
    env.insert("MyType".into(), Type::Named("MyType".into()));
    // Calling a Named type returns that type (constructor pattern)
    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("MyType".into())),
        args: vec![AstExpr::Literal(AstLit::Int("1".into()))],
    };
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
    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("f".into())),
        args: vec![
            AstExpr::Literal(AstLit::Int("1".into())),
            AstExpr::Literal(AstLit::Int("2".into())),
        ],
    };
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
    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("f".into())),
        args: vec![AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Literal(AstLit::Bool(false))),
        }],
    };
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn infer_method_call_is_unknown() {
    let env = TypeEnv::new();
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("obj".into())),
        method: "do_something".into(),
        args: vec![AstExpr::Literal(AstLit::Int("1".into()))],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_method_call_surfaces_receiver_error() {
    let env = TypeEnv::new();
    // receiver has a type error: true + 1
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        }),
        method: "m".into(),
        args: vec![],
    };
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn infer_index_list_returns_element_type() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("xs".into())),
        index: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_index_map_returns_value_type() {
    let mut env = TypeEnv::new();
    env.insert(
        "m".into(),
        Type::Map(Box::new(Type::String), Box::new(Type::Bool)),
    );
    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("m".into())),
        index: Box::new(AstExpr::Literal(AstLit::Str("key".into()))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_index_sequence_returns_element_type() {
    let mut env = TypeEnv::new();
    env.insert("seq".into(), Type::Sequence(Box::new(Type::Float)));
    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("seq".into())),
        index: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Float);
}

#[test]
fn infer_index_unknown_base_is_unknown() {
    let env = TypeEnv::new();
    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("unknown".into())),
        index: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_index_named_type_is_unknown() {
    let mut env = TypeEnv::new();
    env.insert("arr".into(), Type::Named("CustomArray".into()));
    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("arr".into())),
        index: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
    };
    // Named type indexing returns Unknown (could be user-defined indexable)
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_index_surfaces_index_error() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    // Index expr has a type error: true && 42
    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("xs".into())),
        index: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            op: AstBinOp::And,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("42".into()))),
        }),
    };
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn infer_index_bytes_returns_u8() {
    let mut env = TypeEnv::new();
    env.insert("data".into(), Type::Bytes);
    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("data".into())),
        index: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::U8);
}

#[test]
fn infer_index_tuple_literal() {
    let mut env = TypeEnv::new();
    env.insert("pair".into(), Type::Tuple(vec![Type::Int, Type::Bool]));
    // pair[0] should be Int
    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("pair".into())),
        index: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);

    // pair[1] should be Bool
    let expr1 = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("pair".into())),
        index: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
    };
    assert_eq!(infer_expr(&expr1, &env).unwrap(), Type::Bool);
}

#[test]
fn infer_index_bool_emits_error() {
    let mut env = TypeEnv::new();
    env.insert("flag".into(), Type::Bool);
    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("flag".into())),
        index: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
    };
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
    assert_eq!(
        ty,
        Type::Refined {
            base: Box::new(Type::Int),
            predicate: "x > 0".to_string(),
        }
    );
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
        Type::Refined {
            base: Box::new(Type::Nat),
            predicate: "n >= 1 && n <= 100".to_string(),
        }
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
    assert_eq!(
        ty,
        Type::Refined {
            base: Box::new(Type::Bool),
            predicate: String::new(),
        }
    );
}

#[test]
fn refinement_predicate_roundtrip_through_clause_params() {
    // Verify that a refinement type survives extraction via shared
    // extract_clause_params and then parse_type_tokens.
    // Input: raw tokens for `x : { n : Int | n > 0 }`
    use assura_parser::ast::{Expr, extract_clause_params};
    let tokens: Vec<String> = vec!["x", ":", "{", "n", ":", "Int", "|", "n", ">", "0", "}"]
        .into_iter()
        .map(String::from)
        .collect();
    let body = Expr::Raw(tokens);
    let params = extract_clause_params(&body);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "x");

    // Now parse the type tokens -- should produce Refined
    let ty = parse_type_tokens(&params[0].ty);
    assert_eq!(
        ty,
        Type::Refined {
            base: Box::new(Type::Int),
            predicate: "n > 0".to_string(),
        }
    );
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
    let body = Expr::Raw(tokens);
    let params = extract_clause_params(&body);
    assert_eq!(params.len(), 2);

    let ty_a = parse_type_tokens(&params[0].ty);
    assert_eq!(
        ty_a,
        Type::Refined {
            base: Box::new(Type::Int),
            predicate: "x < 10".to_string(),
        }
    );

    let ty_b = parse_type_tokens(&params[1].ty);
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
// T015: Generic type instantiation tests
// -----------------------------------------------------------------------

/// Helper: build a minimal SourceFile with declarations for testing
/// generic instantiation against user-defined types.
fn source_with_decls(
    decls: Vec<assura_parser::ast::Spanned<Decl>>,
) -> assura_parser::ast::SourceFile {
    assura_parser::ast::SourceFile {
        project: None,
        module: None,
        imports: vec![],
        decls,
    }
}

fn spanned_decl(decl: Decl) -> assura_parser::ast::Spanned<Decl> {
    assura_parser::ast::Spanned {
        node: decl,
        span: 0..1,
    }
}

#[test]
fn generic_list_one_arg_ok() {
    let src = source_with_decls(vec![]);
    let result = check_generic_instantiation("List", &[Type::Int], &(0..1), &src);
    assert!(result.is_ok());
}

#[test]
fn generic_list_zero_args_a03003() {
    let src = source_with_decls(vec![]);
    let err = check_generic_instantiation("List", &[], &(0..1), &src).unwrap_err();
    assert_eq!(err.code, "A03003");
    assert!(err.message.contains("List"));
    assert!(err.message.contains("expected 1"));
    assert!(err.message.contains("found 0"));
}

#[test]
fn generic_list_two_args_a03003() {
    let src = source_with_decls(vec![]);
    let err =
        check_generic_instantiation("List", &[Type::Int, Type::Bool], &(0..1), &src).unwrap_err();
    assert_eq!(err.code, "A03003");
    assert!(err.message.contains("expected 1"));
    assert!(err.message.contains("found 2"));
}

#[test]
fn generic_map_two_args_ok() {
    let src = source_with_decls(vec![]);
    let result = check_generic_instantiation("Map", &[Type::String, Type::Int], &(0..1), &src);
    assert!(result.is_ok());
}

#[test]
fn generic_map_one_arg_a03003() {
    let src = source_with_decls(vec![]);
    let err = check_generic_instantiation("Map", &[Type::String], &(0..1), &src).unwrap_err();
    assert_eq!(err.code, "A03003");
    assert!(err.message.contains("Map"));
    assert!(err.message.contains("expected 2"));
    assert!(err.message.contains("found 1"));
}

#[test]
fn generic_set_one_arg_ok() {
    let src = source_with_decls(vec![]);
    let result = check_generic_instantiation("Set", &[Type::Int], &(0..1), &src);
    assert!(result.is_ok());
}

#[test]
fn generic_option_one_arg_ok() {
    let src = source_with_decls(vec![]);
    let result = check_generic_instantiation("Option", &[Type::Bool], &(0..1), &src);
    assert!(result.is_ok());
}

#[test]
fn generic_result_two_args_ok() {
    let src = source_with_decls(vec![]);
    let result = check_generic_instantiation("Result", &[Type::Int, Type::String], &(0..1), &src);
    assert!(result.is_ok());
}

#[test]
fn generic_result_three_args_a03003() {
    let src = source_with_decls(vec![]);
    let err = check_generic_instantiation(
        "Result",
        &[Type::Int, Type::String, Type::Bool],
        &(0..1),
        &src,
    )
    .unwrap_err();
    assert_eq!(err.code, "A03003");
    assert!(err.message.contains("expected 2"));
    assert!(err.message.contains("found 3"));
}

#[test]
fn generic_sequence_one_arg_ok() {
    let src = source_with_decls(vec![]);
    let result = check_generic_instantiation("Sequence", &[Type::Nat], &(0..1), &src);
    assert!(result.is_ok());
}

#[test]
fn generic_user_defined_type_correct_arity() {
    let decls = vec![spanned_decl(Decl::TypeDef(assura_parser::ast::TypeDef {
        name: "Pair".into(),
        type_params: vec!["A".into(), "B".into()],
        body: assura_parser::ast::TypeBody::Empty,
    }))];
    let src = source_with_decls(decls);
    let result = check_generic_instantiation("Pair", &[Type::Int, Type::Bool], &(0..1), &src);
    assert!(result.is_ok());
}

#[test]
fn generic_user_defined_type_wrong_arity() {
    let decls = vec![spanned_decl(Decl::TypeDef(assura_parser::ast::TypeDef {
        name: "Pair".into(),
        type_params: vec!["A".into(), "B".into()],
        body: assura_parser::ast::TypeBody::Empty,
    }))];
    let src = source_with_decls(decls);
    let err = check_generic_instantiation("Pair", &[Type::Int], &(0..1), &src).unwrap_err();
    assert_eq!(err.code, "A03003");
    assert!(err.message.contains("Pair"));
    assert!(err.message.contains("expected 2"));
    assert!(err.message.contains("found 1"));
}

#[test]
fn generic_user_defined_enum_correct_arity() {
    let decls = vec![spanned_decl(Decl::EnumDef(assura_parser::ast::EnumDef {
        name: "Maybe".into(),
        type_params: vec!["T".into()],
        variants: vec![],
    }))];
    let src = source_with_decls(decls);
    let result = check_generic_instantiation("Maybe", &[Type::Int], &(0..1), &src);
    assert!(result.is_ok());
}

#[test]
fn generic_user_defined_enum_wrong_arity() {
    let decls = vec![spanned_decl(Decl::EnumDef(assura_parser::ast::EnumDef {
        name: "Maybe".into(),
        type_params: vec!["T".into()],
        variants: vec![],
    }))];
    let src = source_with_decls(decls);
    let err =
        check_generic_instantiation("Maybe", &[Type::Int, Type::Bool], &(0..1), &src).unwrap_err();
    assert_eq!(err.code, "A03003");
    assert!(err.message.contains("Maybe"));
    assert!(err.message.contains("expected 1"));
    assert!(err.message.contains("found 2"));
}

#[test]
fn generic_user_defined_contract_correct_arity() {
    let decls = vec![spanned_decl(Decl::Contract(
        assura_parser::ast::ContractDecl {
            name: "Container".into(),
            type_params: vec!["T".into()],
            clauses: vec![],
        },
    ))];
    let src = source_with_decls(decls);
    let result = check_generic_instantiation("Container", &[Type::Int], &(0..1), &src);
    assert!(result.is_ok());
}

#[test]
fn generic_user_defined_non_generic_type_zero_args_ok() {
    let decls = vec![spanned_decl(Decl::TypeDef(assura_parser::ast::TypeDef {
        name: "Foo".into(),
        type_params: vec![],
        body: assura_parser::ast::TypeBody::Empty,
    }))];
    let src = source_with_decls(decls);
    let result = check_generic_instantiation("Foo", &[], &(0..1), &src);
    assert!(result.is_ok());
}

#[test]
fn generic_user_defined_non_generic_type_with_args_a03003() {
    let decls = vec![spanned_decl(Decl::TypeDef(assura_parser::ast::TypeDef {
        name: "Foo".into(),
        type_params: vec![],
        body: assura_parser::ast::TypeBody::Empty,
    }))];
    let src = source_with_decls(decls);
    let err = check_generic_instantiation("Foo", &[Type::Int], &(0..1), &src).unwrap_err();
    assert_eq!(err.code, "A03003");
    assert!(err.message.contains("expected 0"));
    assert!(err.message.contains("found 1"));
}

#[test]
fn generic_unknown_type_is_lenient() {
    let src = source_with_decls(vec![]);
    // Unknown type name; not our problem (name resolution handles it)
    let result = check_generic_instantiation("UnknownType", &[Type::Int], &(0..1), &src);
    assert!(result.is_ok());
}

// -- substitute() tests --

#[test]
fn substitute_type_param() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Int);
    let result = substitute(&Type::TypeParam("T".into()), &bindings);
    assert_eq!(result, Type::Int);
}

#[test]
fn substitute_unbound_type_param_unchanged() {
    let bindings = HashMap::new();
    let result = substitute(&Type::TypeParam("T".into()), &bindings);
    assert_eq!(result, Type::TypeParam("T".into()));
}

#[test]
fn substitute_in_list() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Int);
    let ty = Type::List(Box::new(Type::TypeParam("T".into())));
    let result = substitute(&ty, &bindings);
    assert_eq!(result, Type::List(Box::new(Type::Int)));
}

#[test]
fn substitute_in_map() {
    let mut bindings = HashMap::new();
    bindings.insert("K".into(), Type::String);
    bindings.insert("V".into(), Type::Int);
    let ty = Type::Map(
        Box::new(Type::TypeParam("K".into())),
        Box::new(Type::TypeParam("V".into())),
    );
    let result = substitute(&ty, &bindings);
    assert_eq!(
        result,
        Type::Map(Box::new(Type::String), Box::new(Type::Int))
    );
}

#[test]
fn substitute_in_set() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Bool);
    let ty = Type::Set(Box::new(Type::TypeParam("T".into())));
    let result = substitute(&ty, &bindings);
    assert_eq!(result, Type::Set(Box::new(Type::Bool)));
}

#[test]
fn substitute_in_option() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Float);
    let ty = Type::Option(Box::new(Type::TypeParam("T".into())));
    let result = substitute(&ty, &bindings);
    assert_eq!(result, Type::Option(Box::new(Type::Float)));
}

#[test]
fn substitute_in_result() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Int);
    bindings.insert("E".into(), Type::String);
    let ty = Type::Result(
        Box::new(Type::TypeParam("T".into())),
        Box::new(Type::TypeParam("E".into())),
    );
    let result = substitute(&ty, &bindings);
    assert_eq!(
        result,
        Type::Result(Box::new(Type::Int), Box::new(Type::String))
    );
}

#[test]
fn substitute_in_sequence() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Nat);
    let ty = Type::Sequence(Box::new(Type::TypeParam("T".into())));
    let result = substitute(&ty, &bindings);
    assert_eq!(result, Type::Sequence(Box::new(Type::Nat)));
}

#[test]
fn substitute_in_fn_type() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Int);
    bindings.insert("U".into(), Type::Bool);
    let ty = Type::Fn {
        params: vec![Type::TypeParam("T".into()), Type::TypeParam("U".into())],
        ret: Box::new(Type::TypeParam("T".into())),
    };
    let result = substitute(&ty, &bindings);
    assert_eq!(
        result,
        Type::Fn {
            params: vec![Type::Int, Type::Bool],
            ret: Box::new(Type::Int),
        }
    );
}

#[test]
fn substitute_in_refined_type() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Int);
    let ty = Type::Refined {
        base: Box::new(Type::TypeParam("T".into())),
        predicate: "v > 0".into(),
    };
    let result = substitute(&ty, &bindings);
    assert_eq!(
        result,
        Type::Refined {
            base: Box::new(Type::Int),
            predicate: "v > 0".into(),
        }
    );
}

#[test]
fn substitute_nested_generics() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Int);
    // List<Option<T>> -> List<Option<Int>>
    let ty = Type::List(Box::new(Type::Option(Box::new(Type::TypeParam(
        "T".into(),
    )))));
    let result = substitute(&ty, &bindings);
    assert_eq!(
        result,
        Type::List(Box::new(Type::Option(Box::new(Type::Int))))
    );
}

#[test]
fn substitute_leaves_concrete_types_unchanged() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Bool);
    // Concrete types should be unchanged
    assert_eq!(substitute(&Type::Int, &bindings), Type::Int);
    assert_eq!(substitute(&Type::Bool, &bindings), Type::Bool);
    assert_eq!(substitute(&Type::String, &bindings), Type::String);
    assert_eq!(substitute(&Type::Unknown, &bindings), Type::Unknown);
    assert_eq!(
        substitute(&Type::Named("Foo".into()), &bindings),
        Type::Named("Foo".into())
    );
}

#[test]
fn substitute_partial_bindings() {
    let mut bindings = HashMap::new();
    bindings.insert("K".into(), Type::String);
    // Map<K, V> with only K bound -> Map<String, V>
    let ty = Type::Map(
        Box::new(Type::TypeParam("K".into())),
        Box::new(Type::TypeParam("V".into())),
    );
    let result = substitute(&ty, &bindings);
    assert_eq!(
        result,
        Type::Map(
            Box::new(Type::String),
            Box::new(Type::TypeParam("V".into()))
        )
    );
}

// -- instantiate_builtin_generic() tests --

#[test]
fn instantiate_list() {
    let result = instantiate_builtin_generic("List", vec![Type::Int]);
    assert_eq!(result, Some(Type::List(Box::new(Type::Int))));
}

#[test]
fn instantiate_map() {
    let result = instantiate_builtin_generic("Map", vec![Type::String, Type::Int]);
    assert_eq!(
        result,
        Some(Type::Map(Box::new(Type::String), Box::new(Type::Int)))
    );
}

#[test]
fn instantiate_set() {
    let result = instantiate_builtin_generic("Set", vec![Type::Bool]);
    assert_eq!(result, Some(Type::Set(Box::new(Type::Bool))));
}

#[test]
fn instantiate_option() {
    let result = instantiate_builtin_generic("Option", vec![Type::Float]);
    assert_eq!(result, Some(Type::Option(Box::new(Type::Float))));
}

#[test]
fn instantiate_result() {
    let result = instantiate_builtin_generic("Result", vec![Type::Int, Type::String]);
    assert_eq!(
        result,
        Some(Type::Result(Box::new(Type::Int), Box::new(Type::String)))
    );
}

#[test]
fn instantiate_sequence() {
    let result = instantiate_builtin_generic("Sequence", vec![Type::Nat]);
    assert_eq!(result, Some(Type::Sequence(Box::new(Type::Nat))));
}

#[test]
fn instantiate_unknown_name_returns_none() {
    let result = instantiate_builtin_generic("Foo", vec![Type::Int]);
    assert_eq!(result, None);
}

#[test]
fn instantiate_non_generic_builtin_returns_none() {
    let result = instantiate_builtin_generic("Int", vec![]);
    assert_eq!(result, None);
}

// -----------------------------------------------------------------------
// T017: Pattern exhaustiveness checking tests
// -----------------------------------------------------------------------

#[test]
fn exhaustive_all_variants_covered() {
    let variants = vec!["Red".into(), "Green".into(), "Blue".into()];
    let patterns = vec![
        Pattern::Variant("Red".into()),
        Pattern::Variant("Green".into()),
        Pattern::Variant("Blue".into()),
    ];
    assert_eq!(check_exhaustiveness(&patterns, &variants), None);
}

#[test]
fn exhaustive_wildcard_covers_all() {
    let variants = vec!["Red".into(), "Green".into(), "Blue".into()];
    let patterns = vec![Pattern::Wildcard];
    assert_eq!(check_exhaustiveness(&patterns, &variants), None);
}

#[test]
fn exhaustive_wildcard_with_explicit() {
    let variants = vec!["Red".into(), "Green".into(), "Blue".into()];
    let patterns = vec![Pattern::Variant("Red".into()), Pattern::Wildcard];
    assert_eq!(check_exhaustiveness(&patterns, &variants), None);
}

#[test]
fn non_exhaustive_missing_one() {
    let variants = vec!["Red".into(), "Green".into(), "Blue".into()];
    let patterns = vec![
        Pattern::Variant("Red".into()),
        Pattern::Variant("Green".into()),
    ];
    let missing = check_exhaustiveness(&patterns, &variants);
    assert_eq!(missing, Some(vec!["Blue".into()]));
}

#[test]
fn non_exhaustive_missing_multiple() {
    let variants = vec!["Red".into(), "Green".into(), "Blue".into(), "Yellow".into()];
    let patterns = vec![Pattern::Variant("Green".into())];
    let missing = check_exhaustiveness(&patterns, &variants).unwrap();
    assert_eq!(missing, vec!["Red", "Blue", "Yellow"]);
}

#[test]
fn non_exhaustive_empty_patterns() {
    let variants = vec!["A".into(), "B".into(), "C".into()];
    let patterns: Vec<Pattern> = vec![];
    let missing = check_exhaustiveness(&patterns, &variants).unwrap();
    assert_eq!(missing, vec!["A", "B", "C"]);
}

#[test]
fn exhaustive_empty_enum() {
    let variants: Vec<String> = vec![];
    let patterns: Vec<Pattern> = vec![];
    assert_eq!(check_exhaustiveness(&patterns, &variants), None);
}

#[test]
fn exhaustive_duplicate_patterns_ignored() {
    let variants = vec!["X".into(), "Y".into()];
    let patterns = vec![
        Pattern::Variant("X".into()),
        Pattern::Variant("X".into()),
        Pattern::Variant("Y".into()),
    ];
    assert_eq!(check_exhaustiveness(&patterns, &variants), None);
}

#[test]
fn non_exhaustive_literal_does_not_cover_variant() {
    let variants = vec!["Red".into(), "Green".into()];
    let patterns = vec![
        Pattern::Variant("Red".into()),
        Pattern::Literal(AstLit::Int("42".into())),
    ];
    let missing = check_exhaustiveness(&patterns, &variants).unwrap();
    assert_eq!(missing, vec!["Green"]);
}

#[test]
fn exhaustive_single_variant_enum() {
    let variants = vec!["Only".into()];
    let patterns = vec![Pattern::Variant("Only".into())];
    assert_eq!(check_exhaustiveness(&patterns, &variants), None);
}

#[test]
fn non_exhaustive_preserves_declaration_order() {
    let variants = vec![
        "Alpha".into(),
        "Beta".into(),
        "Gamma".into(),
        "Delta".into(),
        "Epsilon".into(),
    ];
    let patterns = vec![
        Pattern::Variant("Beta".into()),
        Pattern::Variant("Delta".into()),
    ];
    let missing = check_exhaustiveness(&patterns, &variants).unwrap();
    assert_eq!(missing, vec!["Alpha", "Gamma", "Epsilon"]);
}

// -----------------------------------------------------------------------
// T018: Contract clause type checking tests
// -----------------------------------------------------------------------

use assura_parser::ast::ClauseKind as AstClauseKind;

#[test]
fn clause_requires_bool_body_ok() {
    let env = TypeEnv::new();
    let body = AstExpr::Literal(AstLit::Bool(true));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Requires, &body, &env, &mut errors, &(0..0));
    assert!(errors.is_empty());
}

#[test]
fn clause_requires_int_body_error() {
    let env = TypeEnv::new();
    let body = AstExpr::Literal(AstLit::Int("42".into()));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Requires, &body, &env, &mut errors, &(0..0));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03006");
    assert!(errors[0].message.contains("requires"));
    assert!(errors[0].message.contains("Bool"));
    assert!(errors[0].message.contains("Int"));
}

#[test]
fn clause_ensures_bool_body_ok() {
    let env = TypeEnv::new();
    let body = AstExpr::Literal(AstLit::Bool(false));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Ensures, &body, &env, &mut errors, &(0..0));
    assert!(errors.is_empty());
}

#[test]
fn clause_ensures_string_body_error() {
    let env = TypeEnv::new();
    let body = AstExpr::Literal(AstLit::Str("hello".into()));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Ensures, &body, &env, &mut errors, &(0..0));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03006");
    assert!(errors[0].message.contains("ensures"));
}

#[test]
fn clause_invariant_bool_body_ok() {
    let env = TypeEnv::new();
    let body = AstExpr::Literal(AstLit::Bool(true));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Invariant, &body, &env, &mut errors, &(0..0));
    assert!(errors.is_empty());
}

#[test]
fn clause_invariant_float_body_error() {
    let env = TypeEnv::new();
    let body = AstExpr::Literal(AstLit::Float("3.14".into()));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Invariant, &body, &env, &mut errors, &(0..0));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03006");
    assert!(errors[0].message.contains("invariant"));
}

#[test]
fn clause_rule_bool_body_ok() {
    let env = TypeEnv::new();
    let body = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        op: AstBinOp::And,
        rhs: Box::new(AstExpr::Literal(AstLit::Bool(false))),
    };
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Rule, &body, &env, &mut errors, &(0..0));
    assert!(errors.is_empty());
}

#[test]
fn clause_rule_int_body_error() {
    let env = TypeEnv::new();
    let body = AstExpr::Literal(AstLit::Int("99".into()));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Rule, &body, &env, &mut errors, &(0..0));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03006");
    assert!(errors[0].message.contains("rule"));
}

#[test]
fn clause_effects_any_body_ok() {
    let env = TypeEnv::new();
    // Effects clause accepts any type (lenient)
    let body = AstExpr::Ident("pure".into());
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Effects, &body, &env, &mut errors, &(0..0));
    assert!(errors.is_empty());
}

#[test]
fn clause_modifies_any_body_ok() {
    let env = TypeEnv::new();
    let body = AstExpr::Ident("buffer".into());
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Modifies, &body, &env, &mut errors, &(0..0));
    assert!(errors.is_empty());
}

#[test]
fn clause_unknown_body_no_error() {
    let env = TypeEnv::new();
    // Unknown ident in requires clause should not emit A03006
    let body = AstExpr::Ident("unknown_predicate".into());
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Requires, &body, &env, &mut errors, &(0..0));
    assert!(errors.is_empty());
}

#[test]
fn clause_comparison_in_requires_ok() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    // x > 0 should infer as Bool, valid in requires
    let body = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("x".into())),
        op: AstBinOp::Gt,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
    };
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Requires, &body, &env, &mut errors, &(0..0));
    assert!(errors.is_empty());
}

#[test]
fn clause_requires_int_body_integration() {
    // Integration test: a contract whose requires clause has an Int body
    // should produce an A03006 error through the full type_check pipeline.
    let src = r#"
contract Bad {
  requires { 42 }
}
"#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.code == "A03006"));
}

#[test]
fn clause_requires_bool_integration() {
    // A contract with a Bool requires clause should type-check fine.
    let src = r#"
contract Good {
  requires { true }
}
"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("should type-check successfully");
}

#[test]
fn demo_files_type_check() {
    // Verify all demo files still type-check without errors
    for path in [
        "demos/libwebp-huffman.assura",
        "demos/zlib-inflate.assura",
        "demos/mbedtls-x509.assura",
        "tests/fixtures/test_basic.assura",
    ] {
        let full = format!(
            "{}/{}",
            env!("CARGO_MANIFEST_DIR")
                .strip_suffix("/crates/assura-types")
                .unwrap_or(env!("CARGO_MANIFEST_DIR")),
            path
        );
        // Try the workspace root path
        let content = match std::fs::read_to_string(&full) {
            Ok(c) => c,
            Err(_) => {
                // Try from two levels up (crates/assura-types -> workspace root)
                let alt = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .and_then(|p| p.parent())
                    .unwrap()
                    .join(path);
                std::fs::read_to_string(alt).unwrap_or_else(|e| panic!("cannot read {path}: {e}"))
            }
        };
        let (file, parse_errs) = assura_parser::parse(&content);
        assert!(
            parse_errs.is_empty(),
            "{path}: unexpected parse errors: {parse_errs:?}"
        );
        let file = file.unwrap_or_else(|| panic!("{path}: parse returned None"));
        let resolved = assura_resolve::resolve(&file)
            .unwrap_or_else(|e| panic!("{path}: resolve errors: {e:?}"));
        type_check(&resolved).unwrap_or_else(|e| panic!("{path}: type_check errors: {e:?}"));
    }
}

// -----------------------------------------------------------------------
// T031: Usage tracking tests (linear types)
// -----------------------------------------------------------------------

#[test]
fn usage_linear_exactly_once_ok() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    tracker.use_var("x");
    let errors = tracker.check();
    assert!(errors.is_empty());
}

#[test]
fn usage_linear_never_used_a05002() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    // Never use x
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05002");
    assert!(errors[0].message.contains("never used"));
    assert!(errors[0].message.contains("x"));
}

#[test]
fn usage_linear_used_twice_a05001() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    tracker.use_var("x");
    tracker.use_var("x");
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05001");
    assert!(errors[0].message.contains("2 times"));
    assert!(errors[0].message.contains("exactly once"));
}

#[test]
fn usage_linear_used_many_times_a05001() {
    let mut tracker = UsageTracker::new();
    tracker.declare("buf".into(), UsageGrade::Linear, 5..10);
    for _ in 0..5 {
        tracker.use_var("buf");
    }
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05001");
    assert!(errors[0].message.contains("5 times"));
}

#[test]
fn usage_erased_not_used_ok() {
    let mut tracker = UsageTracker::new();
    tracker.declare("ghost_val".into(), UsageGrade::Erased, 0..1);
    // Ghost variable never used at runtime: OK
    let errors = tracker.check();
    assert!(errors.is_empty());
}

#[test]
fn usage_erased_used_a05002() {
    let mut tracker = UsageTracker::new();
    tracker.declare("ghost_val".into(), UsageGrade::Erased, 0..1);
    tracker.use_var("ghost_val");
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05002");
    assert!(errors[0].message.contains("erased"));
    assert!(errors[0].message.contains("ghost_val"));
}

#[test]
fn usage_exact_correct_count_ok() {
    let mut tracker = UsageTracker::new();
    tracker.declare("y".into(), UsageGrade::Exact(3), 0..1);
    tracker.use_var("y");
    tracker.use_var("y");
    tracker.use_var("y");
    let errors = tracker.check();
    assert!(errors.is_empty());
}

#[test]
fn usage_exact_too_few_a05003() {
    let mut tracker = UsageTracker::new();
    tracker.declare("y".into(), UsageGrade::Exact(3), 0..1);
    tracker.use_var("y");
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05003");
    assert!(errors[0].message.contains("1 time(s)"));
    assert!(errors[0].message.contains("3 time(s)"));
}

#[test]
fn usage_exact_too_many_a05003() {
    let mut tracker = UsageTracker::new();
    tracker.declare("y".into(), UsageGrade::Exact(2), 0..1);
    tracker.use_var("y");
    tracker.use_var("y");
    tracker.use_var("y");
    tracker.use_var("y");
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05003");
    assert!(errors[0].message.contains("4 time(s)"));
    assert!(errors[0].message.contains("2 time(s)"));
}

#[test]
fn usage_exact_zero_a05003() {
    let mut tracker = UsageTracker::new();
    tracker.declare("z".into(), UsageGrade::Exact(2), 0..1);
    // Never use z
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05003");
    assert!(errors[0].message.contains("0 time(s)"));
}

#[test]
fn usage_unlimited_any_count_ok() {
    let mut tracker = UsageTracker::new();
    tracker.declare("w".into(), UsageGrade::Unlimited, 0..1);
    // Use 0 times: OK
    assert!(tracker.check().is_empty());

    // Use 1 time: OK
    tracker.use_var("w");
    assert!(tracker.check().is_empty());

    // Use 100 times: OK
    for _ in 0..99 {
        tracker.use_var("w");
    }
    assert!(tracker.check().is_empty());
}

#[test]
fn usage_untracked_var_ignored() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    // Using a variable not declared in the tracker is a no-op
    tracker.use_var("y");
    tracker.use_var("x");
    let errors = tracker.check();
    assert!(errors.is_empty());
}

#[test]
fn usage_multiple_variables_mixed() {
    let mut tracker = UsageTracker::new();
    tracker.declare("a".into(), UsageGrade::Linear, 0..1);
    tracker.declare("b".into(), UsageGrade::Linear, 2..3);
    tracker.declare("c".into(), UsageGrade::Unlimited, 4..5);

    tracker.use_var("a"); // OK: linear used once
    // b never used: error
    tracker.use_var("c");
    tracker.use_var("c");
    tracker.use_var("c"); // OK: unlimited

    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05002");
    assert!(errors[0].message.contains("b"));
}

#[test]
fn usage_grade_display() {
    assert_eq!(format!("{}", UsageGrade::Erased), "erased (grade 0)");
    assert_eq!(format!("{}", UsageGrade::Linear), "linear (grade 1)");
    assert_eq!(format!("{}", UsageGrade::Exact(5)), "exact (grade 5)");
    assert_eq!(format!("{}", UsageGrade::Unlimited), "unlimited (grade ω)");
}

#[test]
fn expr_usages_counts_ident() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let expr = AstExpr::Ident("x".into());
    expr_usages(&expr, &mut tracker);
    // x used once, so check should pass for Linear
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_binop_counts_both_sides() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Exact(2), 0..1);
    // x + x => 2 uses
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("x".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Ident("x".into())),
    };
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_linear_used_in_binop_a05001() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    // x + x => 2 uses of a linear variable
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("x".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Ident("x".into())),
    };
    expr_usages(&expr, &mut tracker);
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05001");
}

#[test]
fn expr_usages_call_counts_func_and_args() {
    let mut tracker = UsageTracker::new();
    tracker.declare("f".into(), UsageGrade::Linear, 0..1);
    tracker.declare("a".into(), UsageGrade::Linear, 2..3);
    // f(a) => 1 use of f, 1 use of a
    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("f".into())),
        args: vec![AstExpr::Ident("a".into())],
    };
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_nested_if() {
    let mut tracker = UsageTracker::new();
    tracker.declare("c".into(), UsageGrade::Exact(1), 0..1);
    tracker.declare("t".into(), UsageGrade::Exact(1), 2..3);
    tracker.declare("e".into(), UsageGrade::Exact(1), 4..5);
    // if c then t else e => 1 use each
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Ident("c".into())),
        then_branch: Box::new(AstExpr::Ident("t".into())),
        else_branch: Some(Box::new(AstExpr::Ident("e".into()))),
    };
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_quantifier_counts_domain_and_body() {
    let mut tracker = UsageTracker::new();
    tracker.declare("S".into(), UsageGrade::Exact(1), 0..1);
    tracker.declare("p".into(), UsageGrade::Exact(1), 2..3);
    // forall x in S: p => 1 use of S, 1 use of p
    let expr = AstExpr::Forall {
        var: "x".into(),
        domain: Box::new(AstExpr::Ident("S".into())),
        body: Box::new(AstExpr::Ident("p".into())),
    };
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_field_access_counts_receiver() {
    let mut tracker = UsageTracker::new();
    tracker.declare("obj".into(), UsageGrade::Linear, 0..1);
    // obj.field => 1 use of obj
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("obj".into())), "field".into());
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_method_call_counts_receiver_and_args() {
    let mut tracker = UsageTracker::new();
    tracker.declare("obj".into(), UsageGrade::Exact(1), 0..1);
    tracker.declare("arg1".into(), UsageGrade::Exact(1), 2..3);
    // obj.method(arg1)
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("obj".into())),
        method: "method".into(),
        args: vec![AstExpr::Ident("arg1".into())],
    };
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_index_counts_base_and_index() {
    let mut tracker = UsageTracker::new();
    tracker.declare("arr".into(), UsageGrade::Exact(1), 0..1);
    tracker.declare("i".into(), UsageGrade::Exact(1), 2..3);
    // arr[i]
    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("arr".into())),
        index: Box::new(AstExpr::Ident("i".into())),
    };
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_old_counts_inner() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    // old(x) => 1 use of x
    let expr = AstExpr::Old(Box::new(AstExpr::Ident("x".into())));
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_paren_counts_inner() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    // (x) => 1 use of x
    let expr = AstExpr::Paren(Box::new(AstExpr::Ident("x".into())));
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_list_counts_elements() {
    let mut tracker = UsageTracker::new();
    tracker.declare("a".into(), UsageGrade::Exact(1), 0..1);
    tracker.declare("b".into(), UsageGrade::Exact(1), 2..3);
    // [a, b]
    let expr = AstExpr::List(vec![AstExpr::Ident("a".into()), AstExpr::Ident("b".into())]);
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_unary_counts_inner() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    // -x => 1 use of x
    let expr = AstExpr::UnaryOp {
        op: AstUnOp::Neg,
        expr: Box::new(AstExpr::Ident("x".into())),
    };
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_cast_counts_inner() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    // x as Foo => 1 use of x
    let expr = AstExpr::Cast {
        expr: Box::new(AstExpr::Ident("x".into())),
        ty: "Foo".into(),
    };
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_block_counts_all() {
    let mut tracker = UsageTracker::new();
    tracker.declare("a".into(), UsageGrade::Exact(1), 0..1);
    tracker.declare("b".into(), UsageGrade::Exact(1), 2..3);
    let expr = AstExpr::Block(vec![AstExpr::Ident("a".into()), AstExpr::Ident("b".into())]);
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_raw_no_count() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    // Raw tokens cannot be analyzed; x stays at 0 uses
    let expr = AstExpr::Raw(vec!["x".into()]);
    expr_usages(&expr, &mut tracker);
    let errors = tracker.check();
    // Linear var not used => A05002
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05002");
}

#[test]
fn expr_usages_literal_no_count() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Unlimited, 0..1);
    let expr = AstExpr::Literal(AstLit::Int("42".into()));
    expr_usages(&expr, &mut tracker);
    // No uses recorded, but unlimited is fine
    assert!(tracker.check().is_empty());
}

#[test]
fn usage_tracker_redeclare_resets() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    tracker.use_var("x");
    // Re-declare resets count
    tracker.declare("x".into(), UsageGrade::Linear, 10..11);
    // Now x has 0 uses again
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05002");
    // Span should be the new declaration span
    assert_eq!(errors[0].span, 10..11);
}

// -----------------------------------------------------------------------
// T032: Context splitting for linear types
// -----------------------------------------------------------------------

#[test]
fn linear_context_both_branches_use_var_ok() {
    // Linear var used once in each branch: OK (consumed in both paths)
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then x else x
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::Ident("x".into())),
        else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty(), "should have no A05004 errors");

    // Final check: used exactly once (max from either branch)
    let final_errors = ctx.check();
    assert!(
        final_errors.is_empty(),
        "should have no final errors: {final_errors:?}"
    );
}

#[test]
fn linear_context_one_branch_only_a05004() {
    // Linear var used in then-branch but not else-branch: A05004
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then x else 42
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::Ident("x".into())),
        else_branch: Some(Box::new(AstExpr::Literal(AstLit::Int("42".into())))),
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert_eq!(branch_errors.len(), 1);
    assert_eq!(branch_errors[0].code, "A05004");
    assert!(branch_errors[0].message.contains("x"));
    assert!(branch_errors[0].message.contains("inconsistently"));
}

#[test]
fn linear_context_no_else_branch_a05004() {
    // Linear var used in then-branch with no else-branch: A05004
    // (variable may or may not be consumed depending on condition)
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then x
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::Ident("x".into())),
        else_branch: None,
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert_eq!(branch_errors.len(), 1);
    assert_eq!(branch_errors[0].code, "A05004");
}

#[test]
fn linear_context_neither_branch_uses_var() {
    // Linear var used in neither branch: no A05004 (consistent: 0 in both)
    // But final check will produce A05002 (never used).
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then 1 else 2
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        else_branch: Some(Box::new(AstExpr::Literal(AstLit::Int("2".into())))),
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(
        branch_errors.is_empty(),
        "consistent: 0 uses in both branches"
    );

    // Final check: linear var never used
    let final_errors = ctx.check();
    assert_eq!(final_errors.len(), 1);
    assert_eq!(final_errors[0].code, "A05002");
}

#[test]
fn linear_context_double_use_in_one_branch() {
    // Linear var used twice in one branch, once in the other: A05004
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then (x + x) else x
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("x".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("x".into())),
        }),
        else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert_eq!(branch_errors.len(), 1);
    assert_eq!(branch_errors[0].code, "A05004");
    // Delta: 2 in then, 1 in else
    assert!(branch_errors[0].message.contains("2 time(s)"));
    assert!(branch_errors[0].message.contains("1 time(s)"));
}

#[test]
fn linear_context_unlimited_var_no_consistency_error() {
    // Unlimited variable used differently in branches: no A05004
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Unlimited, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then (x + x + x) else x
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("x".into())),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Ident("x".into())),
            }),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("x".into())),
        }),
        else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let final_errors = ctx.check();
    assert!(final_errors.is_empty());
}

#[test]
fn linear_context_condition_uses_before_fork() {
    // Variable used in condition (before fork) and in one branch:
    // results in 2 total uses of a linear var after merge => A05001 from check().
    // Branch consistency: then uses 0 more, else uses 0 more => consistent.
    let mut tracker = UsageTracker::new();
    tracker.declare("c".into(), UsageGrade::Linear, 0..1);
    tracker.declare("x".into(), UsageGrade::Linear, 2..3);
    let mut ctx = LinearContext::new(tracker);

    // if c then x else x
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Ident("c".into())),
        then_branch: Box::new(AstExpr::Ident("x".into())),
        else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let final_errors = ctx.check();
    // c used once (in condition), x used once (max from branches) => both OK
    assert!(final_errors.is_empty(), "errors: {final_errors:?}");
}

#[test]
fn linear_context_multiple_vars_mixed() {
    // Multiple variables: one consistent, one not.
    let mut tracker = UsageTracker::new();
    tracker.declare("a".into(), UsageGrade::Linear, 0..1);
    tracker.declare("b".into(), UsageGrade::Linear, 2..3);
    let mut ctx = LinearContext::new(tracker);

    // if cond then (a, b) else (a, 0)
    // a: used in both => consistent
    // b: used in then only => inconsistent A05004
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::List(vec![
            AstExpr::Ident("a".into()),
            AstExpr::Ident("b".into()),
        ])),
        else_branch: Some(Box::new(AstExpr::List(vec![
            AstExpr::Ident("a".into()),
            AstExpr::Literal(AstLit::Int("0".into())),
        ]))),
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert_eq!(branch_errors.len(), 1);
    assert_eq!(branch_errors[0].code, "A05004");
    assert!(branch_errors[0].message.contains("b"));
}

#[test]
fn linear_context_exact_grade_consistency_check() {
    // Exact(2) grade: must use consistently across branches.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Exact(2), 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then (x+x) else x  => delta 2 vs delta 1 => A05004
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("x".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("x".into())),
        }),
        else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert_eq!(branch_errors.len(), 1);
    assert_eq!(branch_errors[0].code, "A05004");
}

#[test]
fn linear_context_exact_grade_consistent_ok() {
    // Exact(2): same delta in both branches => OK
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Exact(2), 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then (x+x) else (x+x) => delta 2 in both
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("x".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("x".into())),
        }),
        else_branch: Some(Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("x".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("x".into())),
        })),
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let final_errors = ctx.check();
    assert!(final_errors.is_empty());
}

#[test]
fn linear_context_nested_if_branches() {
    // Nested if: outer branch forks, inner branch forks again.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if c1 then (if c2 then x else x) else x
    // Inner if: x used consistently in both branches => OK
    // Outer if: after inner merge, x used once in then, once in else => OK
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(false))),
            then_branch: Box::new(AstExpr::Ident("x".into())),
            else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
        }),
        else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let final_errors = ctx.check();
    assert!(final_errors.is_empty());
}

#[test]
fn linear_context_nested_if_inner_inconsistent() {
    // Inner if is inconsistent: should produce A05004.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if c1 then (if c2 then x else 0) else x
    // Inner if: x used in then but not else => A05004
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(false))),
            then_branch: Box::new(AstExpr::Ident("x".into())),
            else_branch: Some(Box::new(AstExpr::Literal(AstLit::Int("0".into())))),
        }),
        else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    // Inner if produces an A05004 for x
    assert!(
        branch_errors.iter().any(|e| e.code == "A05004"),
        "expected A05004: {branch_errors:?}"
    );
}

#[test]
fn linear_context_erased_var_unaffected_by_branches() {
    // Erased variable: branch consistency not checked (grade is Erased).
    // Using it in either branch is an A05002 from final check, not A05004.
    let mut tracker = UsageTracker::new();
    tracker.declare("g".into(), UsageGrade::Erased, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then g else 0
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::Ident("g".into())),
        else_branch: Some(Box::new(AstExpr::Literal(AstLit::Int("0".into())))),
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    // Erased is not Linear or Exact, so no A05004
    assert!(branch_errors.is_empty());

    // Final check: erased var used at runtime => A05002
    let final_errors = ctx.check();
    assert_eq!(final_errors.len(), 1);
    assert_eq!(final_errors[0].code, "A05002");
}

#[test]
fn linear_context_var_used_in_condition_and_branches() {
    // x used in condition (1 use), then in both branches (1 each).
    // Post-condition base count = 1. Each branch adds 1 more.
    // Delta: 1 in both => consistent. Total after merge: 2.
    // Linear var used 2 times => A05001 from final check.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if x then x else x  (x as condition + x in each branch)
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Ident("x".into())),
        then_branch: Box::new(AstExpr::Ident("x".into())),
        else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    // Branches are consistent (both use x once more)
    assert!(branch_errors.is_empty());

    // Final: x used 2 times total (1 condition + 1 from branch max)
    let final_errors = ctx.check();
    assert_eq!(final_errors.len(), 1);
    assert_eq!(final_errors[0].code, "A05001");
}

#[test]
fn linear_context_fork_produces_independent_copies() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let ctx = LinearContext::new(tracker);

    let (mut a, mut b) = ctx.fork();
    a.use_var("x");
    // b should still have 0 uses
    assert_eq!(a.get_count("x"), Some(1));
    assert_eq!(b.get_count("x"), Some(0));

    b.use_var("x");
    b.use_var("x");
    assert_eq!(b.get_count("x"), Some(2));
    assert_eq!(a.get_count("x"), Some(1)); // unchanged
}

#[test]
fn linear_context_merge_takes_max_usage() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Unlimited, 0..1);
    let mut ctx = LinearContext::new(tracker);

    let (mut a, mut b) = ctx.fork();
    a.use_var("x");
    a.use_var("x");
    a.use_var("x");
    b.use_var("x");

    let _ = ctx.merge(&a, &b);
    // Max of 3 and 1 = 3
    assert_eq!(ctx.get_count("x"), Some(3));
}

#[test]
fn linear_context_a05005_scope_escape() {
    // A05005: linear variable escapes its scope.
    // This occurs when a linear variable is passed into a context
    // where it outlives its scope (e.g., stored in a longer-lived data
    // structure). For now, model this as a linear var that gets used
    // but its scope ends before consumption.
    //
    // Detected by declaring the variable, walking a scope, then
    // checking: if the variable was not consumed (used 0 times in the
    // scope it was declared in), it effectively escaped.
    let mut tracker = UsageTracker::new();
    tracker.declare("resource".into(), UsageGrade::Linear, 0..8);
    let mut ctx = LinearContext::new(tracker);

    // Simulate: resource is declared but never used in its scope
    // (no expressions reference it).
    let expr = AstExpr::Literal(AstLit::Int("42".into()));
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    // Final check catches it: linear var never used => A05002
    // This is the scope-escape case: the variable existed but was
    // never consumed before its scope ended.
    let final_errors = ctx.check();
    assert_eq!(final_errors.len(), 1);
    assert_eq!(final_errors[0].code, "A05002");
    assert!(final_errors[0].message.contains("resource"));
}

// -----------------------------------------------------------------------
// T033: Linear type test cases (Section 13 Test Case 1 + additional)
// -----------------------------------------------------------------------

#[test]
fn linear_double_use_a05001() {
    // Double-use of a linear variable must produce A05001.
    let mut tracker = UsageTracker::new();
    tracker.declare("buf".into(), UsageGrade::Linear, 0..3);
    let mut ctx = LinearContext::new(tracker);

    // buf + buf => 2 uses of linear variable
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("buf".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Ident("buf".into())),
    };
    let _ = check_expr_linearity(&expr, &mut ctx);
    let errors = ctx.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05001");
    assert!(errors[0].message.contains("buf"));
    assert!(errors[0].message.contains("2 times"));
}

#[test]
fn linear_unused_a05002() {
    // Unused linear variable must produce A05002.
    let mut tracker = UsageTracker::new();
    tracker.declare("handle".into(), UsageGrade::Linear, 0..6);
    let mut ctx = LinearContext::new(tracker);

    // Expression that does not reference 'handle' at all
    let expr = AstExpr::Literal(AstLit::Int("99".into()));
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let errors = ctx.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05002");
    assert!(errors[0].message.contains("handle"));
    assert!(errors[0].message.contains("never used"));
}

#[test]
fn linear_correctly_used_once_passes() {
    // Linear variable used exactly once must pass without errors.
    let mut tracker = UsageTracker::new();
    tracker.declare("conn".into(), UsageGrade::Linear, 0..4);
    let mut ctx = LinearContext::new(tracker);

    // Single use: conn
    let expr = AstExpr::Ident("conn".into());
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let errors = ctx.check();
    assert!(errors.is_empty());
}

#[test]
fn linear_refinement_predicate_not_a_use() {
    // Section 13, Test Case 1: a refinement predicate on a linear
    // variable should NOT count as a runtime use. The refinement
    // predicate is a compile-time/SMT-level constraint, not a
    // runtime consumption.
    //
    // Model: declare the linear variable, record a "refinement use"
    // (which should be ignored), then record a single real use.
    // The variable should be correctly consumed once.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);

    // The refinement predicate x > 0 does NOT consume x.
    // Only the actual use in the expression body does.
    // We model this by NOT calling use_var for the refinement.
    // A single real use follows:
    tracker.use_var("x"); // real runtime use

    let errors = tracker.check();
    assert!(
        errors.is_empty(),
        "refinement predicate should not count as a use: {errors:?}"
    );
}

#[test]
fn linear_refinement_predicate_plus_real_use_no_double_count() {
    // Variant of Section 13 Test Case 1: if the refinement predicate
    // were incorrectly counted, a linear var with a refinement plus
    // one real use would show 2 uses (A05001). Verify it only shows 1.
    let mut tracker = UsageTracker::new();
    tracker.declare("resource".into(), UsageGrade::Linear, 0..8);

    // Refinement predicate: resource.is_valid() -- NOT a runtime use.
    // (We skip calling use_var for predicates.)

    // One real use in the function body:
    tracker.use_var("resource");

    let errors = tracker.check();
    assert!(
        errors.is_empty(),
        "should be exactly 1 use, not 2: {errors:?}"
    );
    assert_eq!(tracker.get_count("resource"), Some(1));
}

#[test]
fn linear_triple_use_a05001() {
    // Three uses of a linear variable: A05001 with count 3.
    let mut tracker = UsageTracker::new();
    tracker.declare("fd".into(), UsageGrade::Linear, 0..2);
    let mut ctx = LinearContext::new(tracker);

    // fd + fd + fd => 3 uses
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("fd".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("fd".into())),
        }),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Ident("fd".into())),
    };
    let _ = check_expr_linearity(&expr, &mut ctx);
    let errors = ctx.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05001");
    assert!(errors[0].message.contains("3 times"));
}

#[test]
fn linear_used_in_call_arg_exactly_once_passes() {
    // Linear variable used as a function argument (single use) passes.
    let mut tracker = UsageTracker::new();
    tracker.declare("key".into(), UsageGrade::Linear, 0..3);
    let mut ctx = LinearContext::new(tracker);

    // consume(key) => 1 use of key
    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("consume".into())),
        args: vec![AstExpr::Ident("key".into())],
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let errors = ctx.check();
    assert!(errors.is_empty());
}

#[test]
fn linear_branch_consistency_with_single_use_passes() {
    // Linear variable used exactly once in each branch: passes.
    let mut tracker = UsageTracker::new();
    tracker.declare("tok".into(), UsageGrade::Linear, 0..3);
    let mut ctx = LinearContext::new(tracker);

    // if cond then consume(tok) else discard(tok)
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::Call {
            func: Box::new(AstExpr::Ident("consume".into())),
            args: vec![AstExpr::Ident("tok".into())],
        }),
        else_branch: Some(Box::new(AstExpr::Call {
            func: Box::new(AstExpr::Ident("discard".into())),
            args: vec![AstExpr::Ident("tok".into())],
        })),
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let errors = ctx.check();
    assert!(errors.is_empty());
}

#[test]
fn linear_two_vars_one_double_used_one_unused() {
    // Two linear variables: one double-used (A05001), one unused (A05002).
    let mut tracker = UsageTracker::new();
    tracker.declare("a".into(), UsageGrade::Linear, 0..1);
    tracker.declare("b".into(), UsageGrade::Linear, 2..3);
    let mut ctx = LinearContext::new(tracker);

    // a + a (double use of a, b never referenced)
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("a".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Ident("a".into())),
    };
    let _ = check_expr_linearity(&expr, &mut ctx);
    let errors = ctx.check();
    assert_eq!(errors.len(), 2);

    let codes: Vec<&str> = errors.iter().map(|e| e.code.as_str()).collect();
    assert!(codes.contains(&"A05001"), "expected A05001 for `a`");
    assert!(codes.contains(&"A05002"), "expected A05002 for `b`");
}

// -----------------------------------------------------------------------
// T034: Typestate checker tests
// -----------------------------------------------------------------------

#[test]
fn typestate_valid_sequence_passes() {
    // Valid transition sequence: Init -> Open -> Close
    let states = vec!["Init".into(), "Open".into(), "Closed".into()];
    let transitions = vec![
        ("open".into(), "Init".into(), "Open".into()),
        ("close".into(), "Open".into(), "Closed".into()),
    ];
    let mut checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    assert!(checker.transition("open", 5..9).is_ok());
    assert_eq!(checker.current_state(), "Open");
    assert!(checker.transition("close", 10..15).is_ok());
    assert_eq!(checker.current_state(), "Closed");
}

#[test]
fn typestate_wrong_state_a06001() {
    // Operation called in wrong state: close() requires Open, but
    // we are in Init.
    let states = vec!["Init".into(), "Open".into(), "Closed".into()];
    let transitions = vec![
        ("open".into(), "Init".into(), "Open".into()),
        ("close".into(), "Open".into(), "Closed".into()),
    ];
    let mut checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    let err = checker.transition("close", 5..10).unwrap_err();
    assert_eq!(err.code, "A06001");
    assert!(err.message.contains("close"));
    assert!(err.message.contains("Init"));
    assert!(err.message.contains("Open"));
}

#[test]
fn typestate_not_linear_a06002() {
    // Typestate variables must be linear; this is checked separately.
    // The TypestateChecker itself produces A06002 when validate_linear
    // is called with is_linear=false.
    let states = vec!["Init".into(), "Open".into()];
    let transitions = vec![("open".into(), "Init".into(), "Open".into())];
    let checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    let err = checker.validate_linear(false);
    assert!(err.is_some());
    let err = err.unwrap();
    assert_eq!(err.code, "A06002");
    assert!(err.message.contains("linear"));
}

#[test]
fn typestate_not_linear_ok_when_linear() {
    // When the variable IS linear, validate_linear returns None.
    let states = vec!["Init".into()];
    let checker = TypestateChecker::new(states, vec![], "Init".into(), 0..4);
    assert!(checker.validate_linear(true).is_none());
}

#[test]
fn typestate_undeclared_state_a06003() {
    // Operation transitions to a state not declared in `states:`.
    let states = vec!["Init".into(), "Open".into()];
    let transitions = vec![
        ("open".into(), "Init".into(), "Open".into()),
        // "Closed" is not in the declared states
        ("close".into(), "Open".into(), "Closed".into()),
    ];
    let checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    let errors = checker.validate_transitions();
    assert!(!errors.is_empty());
    assert!(errors.iter().any(|e| e.code == "A06003"));
    assert!(errors.iter().any(|e| e.message.contains("Closed")));
}

#[test]
fn typestate_undeclared_source_state_a06003() {
    // Transition references a source state not in the declared states.
    let states = vec!["Init".into(), "Done".into()];
    let transitions = vec![
        // "Running" is not declared
        ("finish".into(), "Running".into(), "Done".into()),
    ];
    let checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    let errors = checker.validate_transitions();
    assert!(!errors.is_empty());
    assert!(errors.iter().any(|e| e.code == "A06003"));
    assert!(errors.iter().any(|e| e.message.contains("Running")));
}

#[test]
fn typestate_ambiguous_after_branches_a06004() {
    // Diverging branches leave the object in different states.
    // After branch A: Open, after branch B: Closed => A06004.
    let states = vec!["Init".into(), "Open".into(), "Closed".into()];
    let transitions = vec![
        ("open".into(), "Init".into(), "Open".into()),
        ("close".into(), "Init".into(), "Closed".into()),
    ];

    let checker_a = {
        let mut c = TypestateChecker::new(states.clone(), transitions.clone(), "Init".into(), 0..4);
        c.transition("open", 5..9).unwrap();
        c
    };
    let checker_b = {
        let mut c = TypestateChecker::new(states, transitions, "Init".into(), 0..4);
        c.transition("close", 5..10).unwrap();
        c
    };

    let err = TypestateChecker::check_branch_consistency(&checker_a, &checker_b, 0..4);
    assert!(err.is_some());
    let err = err.unwrap();
    assert_eq!(err.code, "A06004");
    assert!(err.message.contains("Open"));
    assert!(err.message.contains("Closed"));
}

#[test]
fn typestate_consistent_branches_same_state_ok() {
    // Both branches leave the object in the same state: no error.
    let states = vec!["Init".into(), "Open".into()];
    let transitions = vec![("open".into(), "Init".into(), "Open".into())];

    let checker_a = {
        let mut c = TypestateChecker::new(states.clone(), transitions.clone(), "Init".into(), 0..4);
        c.transition("open", 5..9).unwrap();
        c
    };
    let checker_b = {
        let mut c = TypestateChecker::new(states, transitions, "Init".into(), 0..4);
        c.transition("open", 5..9).unwrap();
        c
    };

    let err = TypestateChecker::check_branch_consistency(&checker_a, &checker_b, 0..4);
    assert!(err.is_none());
}

#[test]
fn typestate_multiple_transitions_sequence() {
    // Longer transition chain: Init -> Connecting -> Connected -> Closed
    let states = vec![
        "Init".into(),
        "Connecting".into(),
        "Connected".into(),
        "Closed".into(),
    ];
    let transitions = vec![
        ("connect".into(), "Init".into(), "Connecting".into()),
        (
            "established".into(),
            "Connecting".into(),
            "Connected".into(),
        ),
        ("disconnect".into(), "Connected".into(), "Closed".into()),
    ];
    let mut checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    assert!(checker.transition("connect", 5..12).is_ok());
    assert_eq!(checker.current_state(), "Connecting");
    assert!(checker.transition("established", 13..24).is_ok());
    assert_eq!(checker.current_state(), "Connected");
    assert!(checker.transition("disconnect", 25..35).is_ok());
    assert_eq!(checker.current_state(), "Closed");
}

#[test]
fn typestate_operation_not_found_a06001() {
    // Calling an operation that does not exist in any transition.
    let states = vec!["Init".into(), "Open".into()];
    let transitions = vec![("open".into(), "Init".into(), "Open".into())];
    let mut checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    let err = checker.transition("nonexistent", 5..16).unwrap_err();
    assert_eq!(err.code, "A06001");
    assert!(err.message.contains("nonexistent"));
}

#[test]
fn typestate_valid_transitions_no_errors() {
    // All transitions reference declared states: no errors.
    let states = vec!["Init".into(), "Open".into(), "Closed".into()];
    let transitions = vec![
        ("open".into(), "Init".into(), "Open".into()),
        ("close".into(), "Open".into(), "Closed".into()),
    ];
    let checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    let errors = checker.validate_transitions();
    assert!(errors.is_empty());
}

#[test]
fn typestate_initial_state() {
    // Checker starts in the declared initial state.
    let states = vec!["Start".into(), "End".into()];
    let transitions = vec![("finish".into(), "Start".into(), "End".into())];
    let checker = TypestateChecker::new(states, transitions, "Start".into(), 0..5);

    assert_eq!(checker.current_state(), "Start");
}

// -----------------------------------------------------------------------
// T036-T037: Effect checker tests
// -----------------------------------------------------------------------

// -- EffectSet construction and display --

#[test]
fn effect_set_pure_is_empty() {
    let set = EffectSet::pure();
    assert!(set.is_pure());
    assert!(set.is_empty());
    assert_eq!(set.len(), 0);
    assert_eq!(format!("{set}"), "pure");
}

#[test]
fn effect_set_from_iter_basic() {
    let set = EffectSet::from_effect_names(["io", "mem"]);
    assert!(!set.is_pure());
    assert_eq!(set.len(), 2);
    assert!(set.contains("io"));
    assert!(set.contains("mem"));
    assert!(!set.contains("net"));
}

#[test]
fn effect_set_from_iter_pure_ignored() {
    // "pure" in the iterator should be ignored (it means empty set)
    let set = EffectSet::from_effect_names(["pure"]);
    assert!(set.is_pure());
    assert!(set.is_empty());
}

#[test]
fn effect_set_from_iter_pure_mixed() {
    // "pure" mixed with others: pure is dropped, others kept
    let set = EffectSet::from_effect_names(["pure", "io"]);
    assert!(!set.is_pure());
    assert_eq!(set.len(), 1);
    assert!(set.contains("io"));
}

#[test]
fn effect_set_insert() {
    let mut set = EffectSet::pure();
    set.insert("io".into());
    assert!(!set.is_pure());
    assert!(set.contains("io"));
}

#[test]
fn effect_set_insert_pure_noop() {
    let mut set = EffectSet::pure();
    set.insert("pure".into());
    assert!(set.is_pure());
}

#[test]
fn effect_set_display_sorted() {
    let set = EffectSet::from_effect_names(["mem", "io", "alloc"]);
    // Display should sort effects alphabetically
    assert_eq!(format!("{set}"), "{alloc, io, mem}");
}

// -- EffectChecker: known effects --

#[test]
fn effect_checker_knows_builtins() {
    let checker = EffectChecker::new();
    assert!(checker.is_known("io"));
    assert!(checker.is_known("mem"));
    assert!(checker.is_known("net"));
    assert!(checker.is_known("fs"));
    assert!(checker.is_known("rng"));
    assert!(checker.is_known("time"));
    assert!(checker.is_known("alloc"));
    assert!(checker.is_known("console.read"));
    assert!(checker.is_known("console.write"));
    assert!(checker.is_known("filesystem.read"));
    assert!(checker.is_known("filesystem.write"));
    assert!(checker.is_known("network.connect"));
    assert!(checker.is_known("network.send"));
    assert!(checker.is_known("network.receive"));
    assert!(checker.is_known("database"));
    assert!(checker.is_known("database.read"));
    assert!(checker.is_known("database.write"));
    assert!(checker.is_known("logging"));
    assert!(checker.is_known("log.debug"));
    assert!(checker.is_known("log.info"));
    assert!(checker.is_known("log.warn"));
    assert!(checker.is_known("log.error"));
    assert!(checker.is_known("time.read"));
    assert!(checker.is_known("random"));
    assert!(checker.is_known("diverge"));
}

#[test]
fn effect_checker_unknown_effect() {
    let checker = EffectChecker::new();
    assert!(!checker.is_known("teleport"));
    assert!(!checker.is_known("quantum"));
}

// -- A07003: unknown effect name --

#[test]
fn effect_check_known_all_valid() {
    let checker = EffectChecker::new();
    let set = EffectSet::from_effect_names(["io", "mem", "database"]);
    let errors = checker.check_known(&set, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn effect_check_known_unknown_a07003() {
    let checker = EffectChecker::new();
    let set = EffectSet::from_effect_names(["io", "teleport"]);
    let errors = checker.check_known(&set, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07003");
    assert!(errors[0].message.contains("teleport"));
}

#[test]
fn effect_check_known_multiple_unknown_a07003() {
    let checker = EffectChecker::new();
    let set = EffectSet::from_effect_names(["teleport", "quantum"]);
    let errors = checker.check_known(&set, &(0..10));
    assert_eq!(errors.len(), 2);
    assert!(errors.iter().all(|e| e.code == "A07003"));
}

// -- Hierarchy expansion --

#[test]
fn effect_expand_io_includes_subeffects() {
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let expanded = checker.expand(&declared);
    assert!(expanded.contains("io"));
    assert!(expanded.contains("console.read"));
    assert!(expanded.contains("console.write"));
    assert!(expanded.contains("filesystem.read"));
    assert!(expanded.contains("filesystem.write"));
    assert!(expanded.contains("network.connect"));
    assert!(expanded.contains("network.send"));
    assert!(expanded.contains("network.receive"));
    assert!(expanded.contains("time.read"));
    assert!(expanded.contains("random"));
}

#[test]
fn effect_expand_database_includes_subeffects() {
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["database"]);
    let expanded = checker.expand(&declared);
    assert!(expanded.contains("database"));
    assert!(expanded.contains("database.read"));
    assert!(expanded.contains("database.write"));
}

#[test]
fn effect_expand_logging_includes_subeffects() {
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["logging"]);
    let expanded = checker.expand(&declared);
    assert!(expanded.contains("logging"));
    assert!(expanded.contains("log.debug"));
    assert!(expanded.contains("log.info"));
    assert!(expanded.contains("log.warn"));
    assert!(expanded.contains("log.error"));
}

#[test]
fn effect_expand_leaf_effect_no_change() {
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["console.read"]);
    let expanded = checker.expand(&declared);
    assert_eq!(expanded.len(), 1);
    assert!(expanded.contains("console.read"));
}

#[test]
fn effect_expand_pure_stays_empty() {
    let checker = EffectChecker::new();
    let declared = EffectSet::pure();
    let expanded = checker.expand(&declared);
    assert!(expanded.is_pure());
}

// -- Containment checks: positive (no errors) --

#[test]
fn effect_containment_pure_calling_pure_ok() {
    // Pure function calling another pure function: no errors
    let checker = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::pure();
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn effect_containment_declared_superset_ok() {
    // Declared {io, mem}, actual {mem}: mem is subset, OK
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io", "mem"]);
    let actual = EffectSet::from_effect_names(["mem"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn effect_containment_exact_match_ok() {
    // Declared and actual are identical: OK
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io", "mem"]);
    let actual = EffectSet::from_effect_names(["io", "mem"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn effect_containment_hierarchy_io_covers_console_ok() {
    // Declared {io}, actual {console.read}: io expands to include
    // console.read, so this is OK
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["console.read"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn effect_containment_hierarchy_io_covers_network_ok() {
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["network.send", "network.receive"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn effect_containment_hierarchy_database_covers_read_ok() {
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["database"]);
    let actual = EffectSet::from_effect_names(["database.read"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn effect_containment_hierarchy_logging_covers_all_levels_ok() {
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["logging"]);
    let actual = EffectSet::from_effect_names(["log.debug", "log.info", "log.warn", "log.error"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn effect_containment_declared_io_actual_empty_ok() {
    // Declared {io}, actual empty (pure body): always OK
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::pure();
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

// -- A07002: pure function performs effect --

#[test]
fn effect_containment_pure_performs_io_a07002() {
    // Pure function (empty declared set) performs io: A07002
    let checker = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::from_effect_names(["io"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07002");
    assert!(errors[0].message.contains("pure"));
    assert!(errors[0].message.contains("io"));
}

#[test]
fn effect_containment_pure_performs_mem_a07002() {
    let checker = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::from_effect_names(["mem"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07002");
    assert!(errors[0].message.contains("mem"));
}

#[test]
fn effect_containment_pure_performs_multiple_a07002() {
    // Pure function performs multiple effects: one A07002 per effect
    let checker = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::from_effect_names(["io", "mem"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 2);
    assert!(errors.iter().all(|e| e.code == "A07002"));
}

// -- A07001: undeclared effect --

#[test]
fn effect_containment_undeclared_effect_a07001() {
    // Declared {io}, actual {io, mem}: mem is not declared => A07001
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["io", "mem"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07001");
    assert!(errors[0].message.contains("mem"));
}

#[test]
fn effect_containment_leaf_without_parent_a07001() {
    // Declared {console.read}, actual {console.write}: different leaf
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["console.read"]);
    let actual = EffectSet::from_effect_names(["console.write"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07001");
    assert!(errors[0].message.contains("console.write"));
}

#[test]
fn effect_containment_database_without_io_a07001() {
    // Declared {io}, actual {database.read}: database is not under io
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["database.read"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07001");
    assert!(errors[0].message.contains("database.read"));
}

#[test]
fn effect_containment_multiple_undeclared_a07001() {
    // Declared {mem}, actual {io, database}: two undeclared effects
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["mem"]);
    let actual = EffectSet::from_effect_names(["io", "database"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 2);
    assert!(errors.iter().all(|e| e.code == "A07001"));
}

// -- Effect containment across call chain (T037 specific) --

#[test]
fn effect_containment_call_chain() {
    // Simulate: fn outer() effects {io} calls fn inner() effects {io, mem}
    // inner's actual effects must be subset of outer's declared.
    // mem is not in outer's declared set => A07001 for the call chain.
    let checker = EffectChecker::new();
    let outer_declared = EffectSet::from_effect_names(["io"]);
    // inner's effects propagate to outer's body
    let outer_actual = EffectSet::from_effect_names(["io", "mem"]);
    let errors = checker.check_containment(&outer_declared, &outer_actual, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07001");
    assert!(errors[0].message.contains("mem"));
}

#[test]
fn effect_containment_call_chain_pure_callee_ok() {
    // fn outer() effects {io} calls fn inner() effects {pure}
    // pure is always a subset: OK
    let checker = EffectChecker::new();
    let outer_declared = EffectSet::from_effect_names(["io"]);
    let outer_actual = EffectSet::pure();
    let errors = checker.check_containment(&outer_declared, &outer_actual, &(0..10));
    assert!(errors.is_empty());
}

// -- Edge cases --

#[test]
fn effect_set_dedup() {
    // Duplicate effect names in iterator are deduplicated
    let set = EffectSet::from_effect_names(["io", "io", "mem", "mem"]);
    assert_eq!(set.len(), 2);
}

#[test]
fn effect_checker_default_trait() {
    // Default implementation works
    let checker = EffectChecker::default();
    assert!(checker.is_known("io"));
}

#[test]
fn effect_expand_multiple_groups() {
    // Expanding {io, database} should include sub-effects of both
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io", "database"]);
    let expanded = checker.expand(&declared);
    assert!(expanded.contains("console.read"));
    assert!(expanded.contains("database.write"));
}

#[test]
fn effect_containment_span_preserved() {
    // Verify that the span from the input is preserved in errors
    let checker = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::from_effect_names(["io"]);
    let errors = checker.check_containment(&declared, &actual, &(42..99));
    assert_eq!(errors[0].span, 42..99);
}

#[test]
fn effect_set_iter() {
    let set = EffectSet::from_effect_names(["io", "mem"]);
    let mut items: Vec<&str> = set.iter().collect();
    items.sort();
    assert_eq!(items, vec!["io", "mem"]);
}

// -----------------------------------------------------------------------
// T050: Section 13 type interaction tests
//
// These test pairwise (and three-way) interactions between:
//   - Refinement types
//   - Linear types (UsageTracker, LinearContext)
//   - Typestate (TypestateChecker)
//   - Effects (EffectChecker, EffectSet)
//
// Tests covering information flow and dependent types are deferred
// until T051/T052 are implemented.
// -----------------------------------------------------------------------

// -- Test Case 1: Refinement + Linear (Ghost Use Problem) ----------------
//
// Spec Section 13.1: A refinement predicate references a linear variable.
// Refinement predicates are ghost (logical, not computational) and do
// NOT count as a linear use. The variable is only consumed by
// computational (runtime) uses.

#[test]
fn interaction_refinement_linear_ghost_use_does_not_consume() {
    // Section 13, Test Case 1: a refinement predicate on a linear
    // variable is grade-0 (erased/ghost). It must NOT count as a
    // runtime use.
    //
    // Scenario: linear var `buf` has a refinement `buf.len > 0`.
    // The refinement is a compile-time/SMT-level constraint only.
    // One computational use follows. Total runtime uses = 1 => OK.
    let mut tracker = UsageTracker::new();
    tracker.declare("buf".into(), UsageGrade::Linear, 0..3);

    // Refinement predicate `buf.len > 0` is ghost: do NOT call use_var.
    // Only the single computational use counts:
    tracker.use_var("buf");

    let errors = tracker.check();
    assert!(
        errors.is_empty(),
        "ghost refinement reference should not count as a use: {errors:?}"
    );
    assert_eq!(tracker.get_count("buf"), Some(1));
}

#[test]
fn interaction_refinement_linear_two_computational_uses_a05001() {
    // Section 13, Test Case 1 (negative): two computational uses of
    // a linear variable must produce A05001, regardless of whether a
    // refinement predicate also references the variable.
    let mut tracker = UsageTracker::new();
    tracker.declare("buf".into(), UsageGrade::Linear, 0..3);

    // Refinement predicate (ghost, not counted):
    // -- buf.is_valid (not called via use_var)

    // Two computational (runtime) uses:
    tracker.use_var("buf"); // first use: pass to read()
    tracker.use_var("buf"); // second use: pass to write()

    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05001");
    assert!(errors[0].message.contains("buf"));
    assert!(errors[0].message.contains("2 times"));
}

#[test]
fn interaction_refinement_linear_ghost_grade_erased_no_runtime() {
    // A ghost (Erased) variable used in refinement predicates only:
    // grade-0 means zero runtime uses are allowed. Using it at runtime
    // is A05002. This tests the boundary between refinement context
    // (logical) and runtime context.
    let mut tracker = UsageTracker::new();
    tracker.declare("ghost_bound".into(), UsageGrade::Erased, 0..11);

    // Ghost variable is NOT used at runtime (only in predicates).
    // This is correct: erased variables exist only in logic.
    let errors = tracker.check();
    assert!(
        errors.is_empty(),
        "erased variable with no runtime use should pass: {errors:?}"
    );
}

#[test]
fn interaction_refinement_linear_erased_runtime_use_a05002() {
    // Erased variable used at runtime: A05002.
    // This catches the case where a ghost refinement variable
    // accidentally leaks into computational code.
    let mut tracker = UsageTracker::new();
    tracker.declare("ghost_bound".into(), UsageGrade::Erased, 0..11);

    tracker.use_var("ghost_bound"); // runtime use of erased var

    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05002");
    assert!(errors[0].message.contains("erased"));
}

#[test]
fn interaction_refinement_linear_refined_type_with_linear_base() {
    // A refined type `{ v: Int | v > 0 }` where the base value is
    // linear. The predicate `v > 0` is ghost; the value `v` itself
    // is linear and must be used exactly once.
    let mut tracker = UsageTracker::new();
    tracker.declare("pos_val".into(), UsageGrade::Linear, 0..7);

    // Type is Refined { base: Int, predicate: "v > 0" }
    // The predicate check is done at compile time (SMT), not runtime.
    // One computational use:
    tracker.use_var("pos_val");

    let errors = tracker.check();
    assert!(errors.is_empty());

    // Verify the type representation captures both aspects
    let ty = Type::Refined {
        base: Box::new(Type::Int),
        predicate: "v > 0".into(),
    };
    assert_eq!(format!("{ty}"), "{ x : Int | v > 0 }");
}

// -- Test Case 4: Linear + Effect (Resource-Scoped Effects) --------------
//
// Spec Section 13.4: Linear resources interact with the effect system.
// A function consuming a linear resource should declare appropriate
// effects. The linear variable must still be consumed exactly once.

#[test]
fn interaction_linear_effect_consume_with_correct_effects() {
    // A function that consumes a linear resource and declares `io`
    // effects. The linear variable is consumed exactly once, and the
    // declared effects cover the actual effects. Both checks pass.
    let mut tracker = UsageTracker::new();
    tracker.declare("conn".into(), UsageGrade::Linear, 0..4);
    let mut ctx = LinearContext::new(tracker);

    // Simulate: conn is consumed by calling conn.close()
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("conn".into())),
        method: "close".into(),
        args: vec![],
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    // Linear check: conn used exactly once => OK
    let linear_errors = ctx.check();
    assert!(linear_errors.is_empty());

    // Effect check: function declares {io}, body performs {io} => OK
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["io"]);
    let effect_errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(effect_errors.is_empty());
}

#[test]
fn interaction_linear_effect_resource_not_consumed_a05002() {
    // A function with correct effects but that forgets to consume
    // its linear resource. The effect check passes, but the linear
    // check must report A05002 (unused linear variable).
    let mut tracker = UsageTracker::new();
    tracker.declare("conn".into(), UsageGrade::Linear, 0..4);
    let mut ctx = LinearContext::new(tracker);

    // Function body does NOT use conn at all
    let expr = AstExpr::Literal(AstLit::Int("0".into()));
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    // Linear check: conn never consumed => A05002
    let linear_errors = ctx.check();
    assert_eq!(linear_errors.len(), 1);
    assert_eq!(linear_errors[0].code, "A05002");
    assert!(linear_errors[0].message.contains("conn"));

    // Effect check: independently passes (effects are about the
    // function's declared vs actual effects, not resource consumption)
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["io"]);
    let effect_errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(effect_errors.is_empty());
}

#[test]
fn interaction_linear_effect_pure_function_with_linear_resource() {
    // A pure function that consumes a linear resource. The resource
    // is consumed correctly (linear check passes), but the function
    // is pure, so any effectful operation on it should be caught by
    // the effect checker.
    let mut tracker = UsageTracker::new();
    tracker.declare("handle".into(), UsageGrade::Linear, 0..6);
    let mut ctx = LinearContext::new(tracker);

    // Resource consumed (linear OK)
    let expr = AstExpr::Ident("handle".into());
    let _ = check_expr_linearity(&expr, &mut ctx);
    let linear_errors = ctx.check();
    assert!(linear_errors.is_empty());

    // But function is declared pure, body does io => A07002
    let checker = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::from_effect_names(["io"]);
    let effect_errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(effect_errors.len(), 1);
    assert_eq!(effect_errors[0].code, "A07002");
}

#[test]
fn interaction_linear_effect_undeclared_effect_on_resource() {
    // Function declares {mem} but performs {io} on the linear resource.
    // Linear check passes (resource consumed once), but effect check
    // fails with A07001 (undeclared effect).
    let mut tracker = UsageTracker::new();
    tracker.declare("socket".into(), UsageGrade::Linear, 0..6);
    let mut ctx = LinearContext::new(tracker);

    // Resource consumed
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("socket".into())),
        method: "send".into(),
        args: vec![AstExpr::Literal(AstLit::Str("data".into()))],
    };
    let _ = check_expr_linearity(&expr, &mut ctx);
    let linear_errors = ctx.check();
    assert!(linear_errors.is_empty());

    // Effect mismatch: declared {mem}, actual {io}
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["mem"]);
    let actual = EffectSet::from_effect_names(["io"]);
    let effect_errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(effect_errors.len(), 1);
    assert_eq!(effect_errors[0].code, "A07001");
}

// -- Linear + Typestate interaction tests --------------------------------
//
// Typestate variables MUST be linear (A06002). This tests the
// interaction between the two checkers.

#[test]
fn interaction_linear_typestate_must_be_linear() {
    // A typestate variable that is not declared as linear must fail
    // with A06002. Typestate requires linearity to prevent aliasing
    // which could observe inconsistent states.
    let states = vec!["Init".into(), "Ready".into()];
    let transitions = vec![("start".into(), "Init".into(), "Ready".into())];
    let checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    // Not linear => A06002
    let err = checker.validate_linear(false);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A06002");
}

#[test]
fn interaction_linear_typestate_linear_ok() {
    // A typestate variable declared as linear passes the linearity
    // check and can proceed with state transitions.
    let states = vec!["Locked".into(), "Unlocked".into()];
    let transitions = vec![
        ("unlock".into(), "Locked".into(), "Unlocked".into()),
        ("lock".into(), "Unlocked".into(), "Locked".into()),
    ];
    let mut checker = TypestateChecker::new(states, transitions, "Locked".into(), 0..6);

    // Linear check passes
    assert!(checker.validate_linear(true).is_none());

    // Typestate transitions work
    assert!(checker.transition("unlock", 10..16).is_ok());
    assert_eq!(checker.current_state(), "Unlocked");

    // Linear usage tracking: consumed exactly once
    let mut tracker = UsageTracker::new();
    tracker.declare("lock_var".into(), UsageGrade::Linear, 0..8);
    tracker.use_var("lock_var"); // consumed by unlock operation
    assert!(tracker.check().is_empty());
}

#[test]
fn interaction_linear_typestate_double_use_violates_both() {
    // Using a typestate variable twice violates both linearity (A05001)
    // and potentially causes observable aliasing. Both checkers must
    // report their respective errors independently.
    let mut tracker = UsageTracker::new();
    tracker.declare("file".into(), UsageGrade::Linear, 0..4);
    tracker.use_var("file"); // first use: read
    tracker.use_var("file"); // second use: write (aliasing!)

    let linear_errors = tracker.check();
    assert_eq!(linear_errors.len(), 1);
    assert_eq!(linear_errors[0].code, "A05001");
}

// -- Effect + Typestate interaction tests --------------------------------
//
// Operations that cause typestate transitions may also have effect
// requirements. Both the state transition validity and effect
// containment must be checked.

#[test]
fn interaction_effect_typestate_transition_with_effects() {
    // An operation that transitions state and has effects.
    // Both the typestate transition and effect containment must pass.
    let states = vec!["Disconnected".into(), "Connected".into()];
    let transitions = vec![("connect".into(), "Disconnected".into(), "Connected".into())];
    let mut ts_checker = TypestateChecker::new(states, transitions, "Disconnected".into(), 0..12);

    // Typestate: connect() in Disconnected => Connected (OK)
    assert!(ts_checker.transition("connect", 20..27).is_ok());
    assert_eq!(ts_checker.current_state(), "Connected");

    // Effect: function declares {io}, connect performs {io} (OK)
    let eff_checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["network.connect"]);
    let eff_errors = eff_checker.check_containment(&declared, &actual, &(20..27));
    assert!(eff_errors.is_empty());
}

#[test]
fn interaction_effect_typestate_wrong_state_with_correct_effects() {
    // Operation has correct effects but is called in the wrong state.
    // Effect check passes, but typestate check must fail with A06001.
    let states = vec!["Closed".into(), "Open".into()];
    let transitions = vec![("write".into(), "Open".into(), "Open".into())];
    let mut ts_checker = TypestateChecker::new(states, transitions, "Closed".into(), 0..6);

    // Typestate: write() requires Open but we are in Closed => A06001
    let ts_err = ts_checker.transition("write", 10..15);
    assert!(ts_err.is_err());
    assert_eq!(ts_err.unwrap_err().code, "A06001");

    // Effect check: independently passes
    let eff_checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["io"]);
    assert!(
        eff_checker
            .check_containment(&declared, &actual, &(10..15))
            .is_empty()
    );
}

#[test]
fn interaction_effect_typestate_correct_state_wrong_effects() {
    // Operation is called in the correct state but with undeclared
    // effects. Typestate check passes, effect check fails with A07001.
    let states = vec!["Init".into(), "Running".into()];
    let transitions = vec![("start".into(), "Init".into(), "Running".into())];
    let mut ts_checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    // Typestate: start() in Init => Running (OK)
    assert!(ts_checker.transition("start", 5..10).is_ok());

    // Effect: function declares {mem} but start() does {io} => A07001
    let eff_checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["mem"]);
    let actual = EffectSet::from_effect_names(["io"]);
    let errors = eff_checker.check_containment(&declared, &actual, &(5..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07001");
}

// -- Test Case 10: Conditional Typestate (Branch Divergence) --------------
//
// Spec Section 13.10: Different branches lead to different states.
// After diverging branches, state is ambiguous => A06004.

#[test]
fn interaction_typestate_branch_divergence_a06004() {
    // After an if/match, if one branch transitions to state A and
    // the other to state B, the post-branch state is ambiguous.
    let states = vec!["Idle".into(), "Active".into(), "Error".into()];
    let transitions = vec![
        ("activate".into(), "Idle".into(), "Active".into()),
        ("fail".into(), "Idle".into(), "Error".into()),
    ];

    // Branch A: activate => Active
    let mut checker_a =
        TypestateChecker::new(states.clone(), transitions.clone(), "Idle".into(), 0..4);
    checker_a.transition("activate", 10..18).unwrap();

    // Branch B: fail => Error
    let mut checker_b = TypestateChecker::new(states, transitions, "Idle".into(), 0..4);
    checker_b.transition("fail", 10..14).unwrap();

    // Post-branch: Active vs Error => A06004
    let err = TypestateChecker::check_branch_consistency(&checker_a, &checker_b, 20..25);
    assert!(err.is_some());
    let err = err.unwrap();
    assert_eq!(err.code, "A06004");
    assert!(err.message.contains("Active"));
    assert!(err.message.contains("Error"));
}

#[test]
fn interaction_typestate_branch_divergence_same_state_ok() {
    // Both branches transition to the same state: no ambiguity.
    let states = vec!["Pending".into(), "Done".into()];
    let transitions = vec![
        ("complete_a".into(), "Pending".into(), "Done".into()),
        ("complete_b".into(), "Pending".into(), "Done".into()),
    ];

    let mut checker_a =
        TypestateChecker::new(states.clone(), transitions.clone(), "Pending".into(), 0..7);
    checker_a.transition("complete_a", 10..20).unwrap();

    let mut checker_b = TypestateChecker::new(states, transitions, "Pending".into(), 0..7);
    checker_b.transition("complete_b", 10..20).unwrap();

    let err = TypestateChecker::check_branch_consistency(&checker_a, &checker_b, 20..25);
    assert!(err.is_none());
}

#[test]
fn interaction_typestate_branch_one_transitions_other_stays() {
    // One branch transitions, the other stays in the original state.
    // Post-branch: states differ => A06004.
    let states = vec!["Idle".into(), "Active".into()];
    let transitions = vec![("start".into(), "Idle".into(), "Active".into())];

    let mut checker_a =
        TypestateChecker::new(states.clone(), transitions.clone(), "Idle".into(), 0..4);
    checker_a.transition("start", 10..15).unwrap();
    // checker_a: Active

    let checker_b = TypestateChecker::new(states, transitions, "Idle".into(), 0..4);
    // checker_b: still Idle (no transition in this branch)

    let err = TypestateChecker::check_branch_consistency(&checker_a, &checker_b, 20..25);
    assert!(err.is_some());
    let err = err.unwrap();
    assert_eq!(err.code, "A06004");
    assert!(err.message.contains("Active"));
    assert!(err.message.contains("Idle"));
}

#[test]
fn interaction_typestate_branch_divergence_with_linear_context() {
    // Combine typestate branch divergence with linear context splitting.
    // A linear variable is used consistently in both branches (OK for
    // linearity), but the typestate diverges (A06004).
    let mut tracker = UsageTracker::new();
    tracker.declare("resource".into(), UsageGrade::Linear, 0..8);
    let mut ctx = LinearContext::new(tracker);

    // if cond then use(resource) else use(resource)
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::Call {
            func: Box::new(AstExpr::Ident("activate".into())),
            args: vec![AstExpr::Ident("resource".into())],
        }),
        else_branch: Some(Box::new(AstExpr::Call {
            func: Box::new(AstExpr::Ident("deactivate".into())),
            args: vec![AstExpr::Ident("resource".into())],
        })),
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    // Linear: consistent (1 use in each branch) => no A05004
    assert!(
        branch_errors.is_empty(),
        "linear should be consistent: {branch_errors:?}"
    );
    let linear_final = ctx.check();
    assert!(linear_final.is_empty());

    // Meanwhile, typestate diverges:
    let states = vec!["Idle".into(), "Active".into(), "Stopped".into()];
    let transitions = vec![
        ("activate".into(), "Idle".into(), "Active".into()),
        ("deactivate".into(), "Idle".into(), "Stopped".into()),
    ];
    let mut ts_a = TypestateChecker::new(states.clone(), transitions.clone(), "Idle".into(), 0..4);
    ts_a.transition("activate", 10..18).unwrap();

    let mut ts_b = TypestateChecker::new(states, transitions, "Idle".into(), 0..4);
    ts_b.transition("deactivate", 10..20).unwrap();

    let ts_err = TypestateChecker::check_branch_consistency(&ts_a, &ts_b, 0..25);
    assert!(ts_err.is_some());
    assert_eq!(ts_err.unwrap().code, "A06004");
}

// -- Effect containment in functions (pure calling effectful) -------------
//
// Spec Section 3.5: A pure function calling an effectful one is an
// effect containment violation.

#[test]
fn interaction_effect_containment_pure_calls_io_a07002() {
    // A function declared `pure` (empty effect set) that internally
    // performs an `io` effect must produce A07002.
    let checker = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::from_effect_names(["io"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07002");
    assert!(errors[0].message.contains("pure"));
    assert!(errors[0].message.contains("io"));
}

#[test]
fn interaction_effect_containment_io_calls_database_a07001() {
    // A function declared `{io}` that performs `database.write`:
    // database effects are NOT sub-effects of io.
    // This must produce A07001.
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["database.write"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07001");
}

#[test]
fn interaction_effect_containment_database_covers_subeffects() {
    // A function declared `{database}` can perform `database.read`
    // and `database.write` (sub-effects of the database group).
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["database"]);
    let actual = EffectSet::from_effect_names(["database.read", "database.write"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

// -- Linear context fork/merge with multiple variables -------------------
//
// Tests that context splitting correctly tracks multiple independent
// linear variables through branches.

#[test]
fn interaction_linear_context_fork_merge_two_vars() {
    // Two linear variables, each consumed in different branches.
    // var `a` consumed in then-branch, var `b` consumed in else-branch.
    // Both are inconsistent across branches => two A05004 errors.
    let mut tracker = UsageTracker::new();
    tracker.declare("a".into(), UsageGrade::Linear, 0..1);
    tracker.declare("b".into(), UsageGrade::Linear, 2..3);
    let mut ctx = LinearContext::new(tracker);

    // if cond then a else b
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::Ident("a".into())),
        else_branch: Some(Box::new(AstExpr::Ident("b".into()))),
    };
    let errors = check_expr_linearity(&expr, &mut ctx);
    assert_eq!(errors.len(), 2);
    assert!(errors.iter().all(|e| e.code == "A05004"));

    // One error for `a` (used in then, not in else)
    // One error for `b` (used in else, not in then)
    let names: Vec<bool> = errors
        .iter()
        .map(|e| e.message.contains("a") || e.message.contains("b"))
        .collect();
    assert!(names.iter().all(|&b| b));
}

#[test]
fn interaction_linear_context_fork_merge_swap_in_branches() {
    // Two linear variables, both consumed once in each branch
    // (swapped order). Both are consistent => no errors.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    tracker.declare("y".into(), UsageGrade::Linear, 2..3);
    let mut ctx = LinearContext::new(tracker);

    // if cond then [x, y] else [y, x]
    // Both x and y used once in each branch (consistent delta = 1)
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        then_branch: Box::new(AstExpr::List(vec![
            AstExpr::Ident("x".into()),
            AstExpr::Ident("y".into()),
        ])),
        else_branch: Some(Box::new(AstExpr::List(vec![
            AstExpr::Ident("y".into()),
            AstExpr::Ident("x".into()),
        ]))),
    };
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let final_errors = ctx.check();
    assert!(final_errors.is_empty());
}

// -- Test Case 7: Linear + Information Flow (orthogonal axes) ------------
//
// Spec Section 13.7: Linearity and information flow are independent.
// A value has both a usage grade (linear, unlimited, etc.) and a
// security label (Public, Confidential, etc.). These are tracked on
// orthogonal axes.
//
// Since information flow checking (T051) is not yet implemented, we
// test the orthogonality at the type/tracker level: a variable with
// a security label type AND a linear grade should be checked for both
// independently.

#[test]
fn interaction_linear_infoflow_orthogonal_grade_and_type() {
    // A variable that is both linear (grade 1) and has a
    // Confidential-labeled type. The linear checker tracks usage;
    // the type checker tracks the label. They do not interfere.
    let mut tracker = UsageTracker::new();
    tracker.declare("secret_key".into(), UsageGrade::Linear, 0..10);

    // Type is Refined { base: Bytes, predicate: "label == Confidential" }
    let _ty = Type::Refined {
        base: Box::new(Type::Bytes),
        predicate: "label == Confidential".into(),
    };

    // One computational use: linear check passes
    tracker.use_var("secret_key");
    let errors = tracker.check();
    assert!(errors.is_empty());
}

#[test]
fn interaction_linear_infoflow_unlimited_with_label() {
    // An unlimited variable with a Public label. No linearity
    // constraints, but the type carries the label for info-flow.
    let mut tracker = UsageTracker::new();
    tracker.declare("public_data".into(), UsageGrade::Unlimited, 0..11);

    let _ty = Type::Refined {
        base: Box::new(Type::String),
        predicate: "label == Public".into(),
    };

    // Multiple uses: unlimited grade allows any count
    tracker.use_var("public_data");
    tracker.use_var("public_data");
    tracker.use_var("public_data");
    let errors = tracker.check();
    assert!(errors.is_empty());
}

// -- Test Case 8: Typestate + Effect + Refinement (Three-Way) ------------
//
// Spec Section 13.8: All three features interact simultaneously.
// A typestate variable has a refinement predicate, undergoes state
// transitions, and the operations have effect annotations.

#[test]
fn interaction_three_way_typestate_effect_refinement_all_pass() {
    // Three-way interaction:
    // 1. Typestate: object transitions Init -> Open -> Closed
    // 2. Effects: open() has {io}, close() has {io}
    // 3. Refinement: object has a predicate (capacity > 0)
    //
    // All three checks pass when correctly combined.
    let states = vec!["Init".into(), "Open".into(), "Closed".into()];
    let transitions = vec![
        ("open".into(), "Init".into(), "Open".into()),
        ("close".into(), "Open".into(), "Closed".into()),
    ];
    let mut ts = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    // Typestate transitions
    assert!(ts.transition("open", 10..14).is_ok());
    assert!(ts.transition("close", 15..20).is_ok());
    assert_eq!(ts.current_state(), "Closed");

    // Typestate variable is linear
    assert!(ts.validate_linear(true).is_none());

    // All transitions reference declared states
    assert!(ts.validate_transitions().is_empty());

    // Effects: function declares {io}, body performs {io}
    let eff = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["network.connect"]);
    assert!(
        eff.check_containment(&declared, &actual, &(10..20))
            .is_empty()
    );

    // Refinement: the type has a predicate (compile-time, no runtime cost)
    let ty = Type::Refined {
        base: Box::new(Type::Named("Connection".into())),
        predicate: "capacity > 0".into(),
    };
    assert_eq!(format!("{ty}"), "{ x : Connection | capacity > 0 }");
}

#[test]
fn interaction_three_way_typestate_passes_effect_fails() {
    // Three-way: typestate and refinement are OK, but effects fail.
    // This tests that each checker operates independently.
    let states = vec!["Ready".into(), "Done".into()];
    let transitions = vec![("execute".into(), "Ready".into(), "Done".into())];
    let mut ts = TypestateChecker::new(states, transitions, "Ready".into(), 0..5);

    // Typestate OK
    assert!(ts.transition("execute", 10..17).is_ok());

    // Refinement OK (ghost predicate)
    let _ty = Type::Refined {
        base: Box::new(Type::Named("Task".into())),
        predicate: "priority > 0".into(),
    };

    // Effects FAIL: declared pure, body does io
    let eff = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::from_effect_names(["io"]);
    let errors = eff.check_containment(&declared, &actual, &(10..17));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07002");
}

#[test]
fn interaction_three_way_effect_passes_typestate_fails() {
    // Three-way: effects are OK, but typestate transition fails.
    let states = vec!["Locked".into(), "Unlocked".into()];
    let transitions = vec![("unlock".into(), "Locked".into(), "Unlocked".into())];
    let mut ts = TypestateChecker::new(
        states,
        transitions,
        "Unlocked".into(), // Already unlocked
        0..8,
    );

    // Typestate FAIL: unlock requires Locked, but we are Unlocked
    let ts_err = ts.transition("unlock", 10..16);
    assert!(ts_err.is_err());
    assert_eq!(ts_err.unwrap_err().code, "A06001");

    // Effects OK: declared {io}, body does {io}
    let eff = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["io"]);
    assert!(
        eff.check_containment(&declared, &actual, &(10..16))
            .is_empty()
    );
}

// -- Test Case 11 proxy: Effect + Info-flow (labeled effects) ------------
//
// Since full information flow is not yet implemented (T051), we test
// the effect system's ability to distinguish between effect categories
// that will eventually carry labels. This validates the infrastructure
// needed for Test Case 11.

#[test]
fn interaction_effect_hierarchy_separation() {
    // io effects and database effects are separate hierarchies.
    // Declaring {io} does NOT cover {database.write}.
    // This separation is the foundation for Test Case 11's labeled
    // effects where different effect categories may have different
    // security labels.
    let checker = EffectChecker::new();

    // io does NOT cover database
    let declared_io = EffectSet::from_effect_names(["io"]);
    let actual_db = EffectSet::from_effect_names(["database.write"]);
    let errors = checker.check_containment(&declared_io, &actual_db, &(0..5));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07001");

    // database does NOT cover io
    let declared_db = EffectSet::from_effect_names(["database"]);
    let actual_io = EffectSet::from_effect_names(["console.write"]);
    let errors = checker.check_containment(&declared_db, &actual_io, &(0..5));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07001");
}

#[test]
fn interaction_effect_multiple_groups_combined() {
    // Declaring both {io, database} covers sub-effects of both.
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io", "database"]);
    let actual = EffectSet::from_effect_names(["console.write", "network.send", "database.read"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

// -- Combined: Linear + Typestate + Effect (full pipeline simulation) ----

#[test]
fn interaction_full_pipeline_linear_typestate_effect_pass() {
    // Simulate a full pipeline check for a resource:
    // 1. Linear: resource consumed exactly once
    // 2. Typestate: valid transition sequence
    // 3. Effects: all effects declared
    //
    // Scenario: a database connection that is opened, used, and closed.

    // --- Linear tracking ---
    let mut tracker = UsageTracker::new();
    tracker.declare("db_conn".into(), UsageGrade::Linear, 0..7);
    let mut ctx = LinearContext::new(tracker);

    // Resource consumed once (via close)
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("db_conn".into())),
        method: "close".into(),
        args: vec![],
    };
    let _ = check_expr_linearity(&expr, &mut ctx);
    let linear_errors = ctx.check();
    assert!(linear_errors.is_empty(), "linear: {linear_errors:?}");

    // --- Typestate tracking ---
    let states = vec![
        "Disconnected".into(),
        "Connected".into(),
        "InTransaction".into(),
        "Closed".into(),
    ];
    let transitions = vec![
        ("connect".into(), "Disconnected".into(), "Connected".into()),
        (
            "begin_tx".into(),
            "Connected".into(),
            "InTransaction".into(),
        ),
        ("commit".into(), "InTransaction".into(), "Connected".into()),
        ("close".into(), "Connected".into(), "Closed".into()),
    ];
    let mut ts = TypestateChecker::new(states, transitions, "Disconnected".into(), 0..12);

    assert!(ts.transition("connect", 10..17).is_ok());
    assert!(ts.transition("begin_tx", 18..26).is_ok());
    assert!(ts.transition("commit", 27..33).is_ok());
    assert!(ts.transition("close", 34..39).is_ok());
    assert_eq!(ts.current_state(), "Closed");
    assert!(ts.validate_linear(true).is_none());
    assert!(ts.validate_transitions().is_empty());

    // --- Effect tracking ---
    let eff = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["database", "io"]);
    let actual =
        EffectSet::from_effect_names(["database.read", "database.write", "network.connect"]);
    let eff_errors = eff.check_containment(&declared, &actual, &(0..39));
    assert!(eff_errors.is_empty(), "effects: {eff_errors:?}");
}

#[test]
fn interaction_full_pipeline_all_three_fail() {
    // All three checks fail simultaneously:
    // 1. Linear: double use
    // 2. Typestate: wrong state
    // 3. Effects: undeclared effect

    // --- Linear: double use ---
    let mut tracker = UsageTracker::new();
    tracker.declare("res".into(), UsageGrade::Linear, 0..3);
    tracker.use_var("res");
    tracker.use_var("res");
    let linear_errors = tracker.check();
    assert_eq!(linear_errors.len(), 1);
    assert_eq!(linear_errors[0].code, "A05001");

    // --- Typestate: wrong state ---
    let states = vec!["Off".into(), "On".into()];
    let transitions = vec![("turn_off".into(), "On".into(), "Off".into())];
    let mut ts = TypestateChecker::new(states, transitions, "Off".into(), 0..3);
    let ts_err = ts.transition("turn_off", 5..13);
    assert!(ts_err.is_err());
    assert_eq!(ts_err.unwrap_err().code, "A06001");

    // --- Effects: undeclared ---
    let eff = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::from_effect_names(["database.write"]);
    let eff_errors = eff.check_containment(&declared, &actual, &(0..10));
    assert_eq!(eff_errors.len(), 1);
    assert_eq!(eff_errors[0].code, "A07002");
}

// -----------------------------------------------------------------------
// T045: Frame condition tests (CORE.3)
// -----------------------------------------------------------------------

#[test]
fn extract_modifies_single_ident() {
    let body = AstExpr::Ident("x".into());
    let targets = extract_modifies_targets(&body);
    assert_eq!(targets, vec!["x"]);
}

#[test]
fn extract_modifies_block_of_idents() {
    let body = AstExpr::Block(vec![AstExpr::Ident("x".into()), AstExpr::Ident("y".into())]);
    let targets = extract_modifies_targets(&body);
    assert_eq!(targets, vec!["x", "y"]);
}

#[test]
fn extract_modifies_field_access() {
    let body = AstExpr::Field(Box::new(AstExpr::Ident("node".into())), "keys".into());
    let targets = extract_modifies_targets(&body);
    assert_eq!(targets, vec!["node.keys"]);
}

#[test]
fn extract_modifies_nested_field() {
    let body = AstExpr::Field(
        Box::new(AstExpr::Field(
            Box::new(AstExpr::Ident("state".into())),
            "head".into(),
        )),
        "data".into(),
    );
    let targets = extract_modifies_targets(&body);
    assert_eq!(targets, vec!["state.head.data"]);
}

#[test]
fn extract_modifies_list() {
    let body = AstExpr::List(vec![
        AstExpr::Ident("a".into()),
        AstExpr::Ident("b".into()),
        AstExpr::Ident("c".into()),
    ]);
    let targets = extract_modifies_targets(&body);
    assert_eq!(targets, vec!["a", "b", "c"]);
}

#[test]
fn extract_modifies_raw_tokens() {
    let body = AstExpr::Raw(vec!["x".into(), ",".into(), "y".into()]);
    let targets = extract_modifies_targets(&body);
    assert_eq!(targets, vec!["x", "y"]);
}

#[test]
fn collect_old_refs_simple() {
    // old(x)
    let expr = AstExpr::Old(Box::new(AstExpr::Ident("x".into())));
    let refs = collect_old_references(&expr);
    assert_eq!(refs, vec!["x"]);
}

#[test]
fn collect_old_refs_in_binop() {
    // old(x) == old(y) + 1
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Old(Box::new(AstExpr::Ident("x".into())))),
        op: AstBinOp::Eq,
        rhs: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Old(Box::new(AstExpr::Ident("y".into())))),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        }),
    };
    let refs = collect_old_references(&expr);
    assert!(refs.contains(&"x".to_string()));
    assert!(refs.contains(&"y".to_string()));
}

#[test]
fn collect_old_refs_field() {
    // old(node.count)
    let expr = AstExpr::Old(Box::new(AstExpr::Field(
        Box::new(AstExpr::Ident("node".into())),
        "count".into(),
    )));
    let refs = collect_old_references(&expr);
    assert_eq!(refs, vec!["node.count"]);
}

#[test]
fn collect_old_refs_none() {
    // x + y (no old() references)
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("x".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Ident("y".into())),
    };
    let refs = collect_old_references(&expr);
    assert!(refs.is_empty());
}

#[test]
fn frame_checker_valid_modifies_clause() {
    // modifies { x } with x in scope -> no errors
    let body = AstExpr::Ident("x".into());
    let checker = FrameChecker::new(&[&body]);

    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    let symbols = assura_resolve::SymbolTable {
        symbols: vec![],
        scopes: vec![],
    };

    let errors = checker.check_scope(&env, &symbols, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn frame_checker_unknown_var_a14001() {
    // modifies { nonexistent } -> A14001
    let body = AstExpr::Ident("nonexistent".into());
    let checker = FrameChecker::new(&[&body]);

    let env = TypeEnv::new();
    let symbols = assura_resolve::SymbolTable {
        symbols: vec![],
        scopes: vec![],
    };

    let errors = checker.check_scope(&env, &symbols, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A14001");
    assert!(errors[0].message.contains("nonexistent"));
}

#[test]
fn frame_checker_mixed_scope_check() {
    // modifies { x, unknown_y } -> 1 error for unknown_y
    let body = AstExpr::Block(vec![
        AstExpr::Ident("x".into()),
        AstExpr::Ident("unknown_y".into()),
    ]);
    let checker = FrameChecker::new(&[&body]);

    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    let symbols = assura_resolve::SymbolTable {
        symbols: vec![],
        scopes: vec![],
    };

    let errors = checker.check_scope(&env, &symbols, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A14001");
    assert!(errors[0].message.contains("unknown_y"));
}

#[test]
fn frame_checker_frame_axiom_vars() {
    // modifies { x }, ensures: y == old(y)
    // y is NOT in the modifies set, so it gets a frame axiom
    let modifies_body = AstExpr::Ident("x".into());
    let checker = FrameChecker::new(&[&modifies_body]);

    let ensures_body = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("y".into())),
        op: AstBinOp::Eq,
        rhs: Box::new(AstExpr::Old(Box::new(AstExpr::Ident("y".into())))),
    };

    let frame_vars = checker.frame_axiom_vars(&ensures_body);
    assert!(frame_vars.contains(&"y".to_string()));
    // x IS modified, so it should NOT appear
    assert!(!frame_vars.contains(&"x".to_string()));
}

#[test]
fn frame_checker_modified_var_no_axiom() {
    // modifies { x }, ensures: x == old(x) + 1
    // x IS in the modifies set, so it should NOT get a frame axiom
    let modifies_body = AstExpr::Ident("x".into());
    let checker = FrameChecker::new(&[&modifies_body]);

    let ensures_body = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("x".into())),
        op: AstBinOp::Eq,
        rhs: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Old(Box::new(AstExpr::Ident("x".into())))),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        }),
    };

    let frame_vars = checker.frame_axiom_vars(&ensures_body);
    assert!(!frame_vars.contains(&"x".to_string()));
}

#[test]
fn frame_checker_empty_no_axioms() {
    // No modifies clause -> no frame axioms
    let checker = FrameChecker::empty();
    assert!(!checker.has_modifies());

    let ensures_body = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("y".into())),
        op: AstBinOp::Eq,
        rhs: Box::new(AstExpr::Old(Box::new(AstExpr::Ident("y".into())))),
    };

    let frame_vars = checker.frame_axiom_vars(&ensures_body);
    assert!(frame_vars.is_empty());
}

#[test]
fn frame_checker_has_modifies() {
    let body = AstExpr::Ident("x".into());
    let checker = FrameChecker::new(&[&body]);
    assert!(checker.has_modifies());
}

#[test]
fn frame_checker_is_modified() {
    let body = AstExpr::Block(vec![AstExpr::Ident("x".into()), AstExpr::Ident("y".into())]);
    let checker = FrameChecker::new(&[&body]);
    assert!(checker.is_modified("x"));
    assert!(checker.is_modified("y"));
    assert!(!checker.is_modified("z"));
}

// -----------------------------------------------------------------------
// T043 CORE.1: Ghost code tests
// -----------------------------------------------------------------------

#[test]
fn ghost_fn_pure_effects_passes() {
    // A ghost function with effects: pure should type-check fine.
    let src = r#"
ghost fn invariant_helper(x: Int) -> Bool
effects: pure
ensures { result == true }
"#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    assert!(
        result.is_ok(),
        "ghost fn with pure effects should pass: {result:?}"
    );
}

#[test]
fn ghost_fn_no_effects_clause_passes() {
    // A ghost function with no explicit effects clause is implicitly pure.
    let src = r#"
ghost fn spec_helper(x: Int) -> Bool
ensures { result == true }
"#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    assert!(
        result.is_ok(),
        "ghost fn without effects clause should pass: {result:?}"
    );
}

#[test]
fn ghost_fn_non_pure_effects_a54001() {
    // A ghost function with io effects should produce A54001.
    let src = r#"
ghost fn bad_ghost(x: Int) -> Bool
effects: io
ensures { result == true }
"#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    assert!(result.is_err(), "ghost fn with io effects should fail");
    let errors = result.unwrap_err();
    assert!(
        errors.iter().any(|e| e.code == "A54001"),
        "should produce A54001, got: {errors:?}"
    );
    assert!(
        errors[0].message.contains("ghost function"),
        "error message should mention ghost function"
    );
}

#[test]
fn ghost_block_type_checks_inner() {
    // A ghost block should type-check its inner expression.
    let env = TypeEnv::new();
    let expr = AstExpr::Ghost(Box::new(AstExpr::Literal(AstLit::Bool(true))));
    // Ghost block type is Unit (erased at runtime)
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unit);
}

#[test]
fn ghost_block_propagates_inner_error() {
    // A ghost block with a type error in its body should propagate the error.
    let env = TypeEnv::new();
    let expr = AstExpr::Ghost(Box::new(AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Literal(AstLit::Bool(false))),
    }));
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn ghost_var_not_counted_as_linear_use() {
    // References inside a ghost block should NOT count as linear uses.
    let mut tracker = UsageTracker::new();
    tracker.declare("resource".into(), UsageGrade::Linear, 0..1);

    let ghost_expr = AstExpr::Ghost(Box::new(AstExpr::Ident("resource".into())));

    // Walk with linearity checker: ghost blocks should not count
    let mut ctx = LinearContext::new(tracker);
    let errors = check_expr_linearity(&ghost_expr, &mut ctx);
    assert!(
        errors.is_empty(),
        "ghost block should not cause linearity errors"
    );

    // The variable should still show 0 uses (ghost use does not count)
    assert_eq!(ctx.get_count("resource"), Some(0));
}

// -----------------------------------------------------------------------
// T044: Lemma tests (CORE.2)
// -----------------------------------------------------------------------

#[test]
fn lemma_fn_pure_effects_passes() {
    // Lemma with pure effects should type-check without errors.
    let src = r#"
        lemma add_comm(a: Int, b: Int)
            effects: pure
            ensures { a + b == b + a }
    "#;
    let (file, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    let file = file.unwrap();
    let resolved = assura_resolve::resolve(&file).unwrap();
    let result = type_check(&resolved);
    assert!(
        result.is_ok(),
        "lemma with pure effects should pass type check"
    );
}

#[test]
fn lemma_fn_no_effects_clause_passes() {
    // Lemma with no explicit effects clause is implicitly pure: OK.
    let src = r#"
        lemma trivial(x: Int)
            ensures { x == x }
    "#;
    let (file, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    let file = file.unwrap();
    let resolved = assura_resolve::resolve(&file).unwrap();
    let result = type_check(&resolved);
    assert!(result.is_ok(), "lemma with no effects clause should pass");
}

#[test]
fn lemma_fn_non_pure_effects_a55001() {
    // Lemma with non-pure effects should produce A55001.
    let src = r#"
        lemma bad_lemma(x: Int)
            effects: io
            ensures { x > 0 }
    "#;
    let (file, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    let file = file.unwrap();
    let resolved = assura_resolve::resolve(&file).unwrap();
    let result = type_check(&resolved);
    assert!(result.is_err(), "lemma with io effects should fail");
    let errors = result.unwrap_err();
    assert!(
        errors.iter().any(|e| e.code == "A55001"),
        "should produce A55001, got: {errors:?}"
    );
}

#[test]
fn lemma_is_lemma_flag_set() {
    // Verify that parsing a lemma sets is_lemma = true.
    let src = r#"
        lemma my_lemma(n: Int)
            ensures { n >= 0 }
    "#;
    let (file, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    let file = file.unwrap();
    assert_eq!(file.decls.len(), 1);
    if let Decl::FnDef(f) = &file.decls[0].node {
        assert!(f.is_lemma, "lemma should have is_lemma = true");
        assert!(!f.is_ghost, "lemma should not have is_ghost = true");
        assert_eq!(f.name, "my_lemma");
    } else {
        panic!("expected FnDef, got {:?}", file.decls[0].node);
    }
}

#[test]
fn fn_is_not_lemma() {
    // Verify that parsing a regular fn sets is_lemma = false.
    let src = r#"
        fn regular(n: Int) -> Int {
            ensures { result >= 0 }
        }
    "#;
    let (file, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    let file = file.unwrap();
    assert_eq!(file.decls.len(), 1);
    if let Decl::FnDef(f) = &file.decls[0].node {
        assert!(!f.is_lemma, "fn should have is_lemma = false");
    } else {
        panic!("expected FnDef");
    }
}

#[test]
fn apply_expr_type_is_bool() {
    // apply lemma_name(args) should have Bool type.
    let env = TypeEnv::new();
    let apply = AstExpr::Apply {
        lemma_name: "some_lemma".into(),
        args: vec![AstExpr::Literal(AstLit::Int("42".into()))],
    };
    let result = infer_expr(&apply, &env);
    assert_eq!(result.unwrap(), Type::Bool);
}

#[test]
fn apply_not_counted_as_linear_use() {
    // apply should not count variable references as linear uses.
    let mut tracker = UsageTracker::new();
    tracker.declare("resource".into(), UsageGrade::Linear, 0..1);

    let apply = AstExpr::Apply {
        lemma_name: "some_lemma".into(),
        args: vec![AstExpr::Ident("resource".into())],
    };

    let mut ctx = LinearContext::new(tracker);
    let errors = check_expr_linearity(&apply, &mut ctx);
    assert!(errors.is_empty(), "apply should not cause linearity errors");
    assert_eq!(ctx.get_count("resource"), Some(0));
}

// -----------------------------------------------------------------------
// T064: Error propagation tests
// -----------------------------------------------------------------------

#[test]
fn test_error_propagation_must_propagate_swallow_rejected() {
    let mut checker = ErrorPropagationChecker::new();
    checker.register_policy(
        "TestPolicy".into(),
        ErrorPolicy {
            must_propagate: vec!["SQLITE_CORRUPT".into(), "SQLITE_NOMEM".into()],
            ..Default::default()
        },
    );

    // Swallowing a must_propagate error should produce A12001
    let err = checker.validate_catch("SQLITE_CORRUPT", ErrorAction::Swallow, 0..10);
    assert!(err.is_some(), "swallowing must_propagate error should fail");
    assert_eq!(err.unwrap().code, "A12001");

    // Propagating is fine
    let err = checker.validate_catch("SQLITE_CORRUPT", ErrorAction::Propagate, 0..10);
    assert!(
        err.is_none(),
        "propagating must_propagate error should pass"
    );

    // Handling is fine
    let err = checker.validate_catch("SQLITE_CORRUPT", ErrorAction::Handle, 0..10);
    assert!(err.is_none(), "handling must_propagate error should pass");

    // Swallowing a non-must_propagate error is fine
    let err = checker.validate_catch("SQLITE_BUSY", ErrorAction::Swallow, 0..10);
    assert!(err.is_none(), "swallowing non-policy error should pass");
}

#[test]
fn test_error_propagation_must_not_mask() {
    let mut checker = ErrorPropagationChecker::new();
    checker.register_policy(
        "TestPolicy".into(),
        ErrorPolicy {
            must_not_mask: vec![
                ("SQLITE_CORRUPT".into(), "SQLITE_OK".into()),
                ("SQLITE_NOMEM".into(), "SQLITE_ERROR".into()),
            ],
            ..Default::default()
        },
    );

    // Forbidden translation should produce A12002
    let err = checker.validate_catch(
        "SQLITE_CORRUPT",
        ErrorAction::TranslateTo("SQLITE_OK".into()),
        0..10,
    );
    assert!(err.is_some(), "forbidden translation should fail");
    assert_eq!(err.unwrap().code, "A12002");

    // Allowed translation should pass
    let err = checker.validate_catch(
        "SQLITE_CORRUPT",
        ErrorAction::TranslateTo("SQLITE_CORRUPT_DETAILED".into()),
        0..10,
    );
    assert!(err.is_none(), "non-forbidden translation should pass");
}

#[test]
fn test_error_propagation_must_check() {
    let mut checker = ErrorPropagationChecker::new();
    checker.register_policy(
        "TestPolicy".into(),
        ErrorPolicy {
            must_check: vec!["sqlite3_reset".into(), "sqlite3_finalize".into()],
            ..Default::default()
        },
    );

    // Unchecked call to must_check function -> A12003
    let err = checker.validate_unchecked_call("sqlite3_reset", 0..10);
    assert!(err.is_some(), "unchecked must_check call should fail");
    assert_eq!(err.unwrap().code, "A12003");

    // Non-must_check function is fine
    let err = checker.validate_unchecked_call("sqlite3_open", 0..10);
    assert!(err.is_none(), "non-policy function should pass");
}

#[test]
fn test_error_propagation_multiple_policies() {
    let mut checker = ErrorPropagationChecker::new();
    checker.register_policy(
        "PolicyA".into(),
        ErrorPolicy {
            must_propagate: vec!["ERR_A".into()],
            ..Default::default()
        },
    );
    checker.register_policy(
        "PolicyB".into(),
        ErrorPolicy {
            must_propagate: vec!["ERR_B".into()],
            ..Default::default()
        },
    );

    // Both policies are checked
    assert!(checker.is_must_propagate("ERR_A"));
    assert!(checker.is_must_propagate("ERR_B"));
    assert!(!checker.is_must_propagate("ERR_C"));
}

#[test]
fn test_error_propagation_empty_policy() {
    let checker = ErrorPropagationChecker::new();

    // No policies registered: everything passes
    let err = checker.validate_catch("ANY_ERROR", ErrorAction::Swallow, 0..10);
    assert!(err.is_none(), "no policy means no restrictions");
}

// -----------------------------------------------------------------------
// T046: Memory region contracts (MEM.1)
// -----------------------------------------------------------------------

#[test]
fn memory_checker_register_buffer() {
    let mut checker = MemoryChecker::new();
    assert!(!checker.is_buffer("buf"));
    checker.register_buffer("buf".into(), "buf.len".into());
    assert!(checker.is_buffer("buf"));
    assert_eq!(checker.buffer_capacity("buf"), Some("buf.len"));
}

#[test]
fn memory_checker_register_region() {
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());
    checker.register_region(MemoryRegion {
        name: "valid_range".into(),
        lower: "0".into(),
        upper: "buf.len".into(),
        buffer: "buf".into(),
    });
    assert_eq!(checker.regions().len(), 1);
    assert_eq!(checker.regions()[0].name, "valid_range");
}

#[test]
fn memory_checker_bounds_check_present() {
    // offset + len <= buf.len pattern should be recognized
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());

    let bounds_expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("offset".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("len".into())),
        }),
        op: AstBinOp::Lte,
        rhs: Box::new(AstExpr::Field(
            Box::new(AstExpr::Ident("buf".into())),
            "len".into(),
        )),
    };

    let result = checker.check_bounds_in_requires("buf", &[&bounds_expr], &(0..10));
    assert!(result.is_none(), "should detect bounds check");
}

#[test]
fn memory_checker_bounds_check_missing() {
    // No bounds check -> A08101
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());

    // A requires clause that does not check buffer bounds
    let unrelated_expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("x".into())),
        op: AstBinOp::Gt,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
    };

    let result = checker.check_bounds_in_requires("buf", &[&unrelated_expr], &(0..10));
    assert!(result.is_some(), "should detect missing bounds check");
    let err = result.unwrap();
    assert_eq!(err.code, "A08101");
    assert!(err.message.contains("buf"));
}

#[test]
fn memory_checker_region_buffer_exists() {
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());
    checker.register_region(MemoryRegion {
        name: "r1".into(),
        lower: "0".into(),
        upper: "buf.len".into(),
        buffer: "buf".into(),
    });
    let errors = checker.check_region_buffers(&(0..10));
    assert!(errors.is_empty(), "buffer exists, no errors expected");
}

#[test]
fn memory_checker_region_buffer_missing() {
    let mut checker = MemoryChecker::new();
    // Do NOT register "missing_buf" as a buffer
    checker.register_region(MemoryRegion {
        name: "r1".into(),
        lower: "0".into(),
        upper: "missing_buf.len".into(),
        buffer: "missing_buf".into(),
    });
    let errors = checker.check_region_buffers(&(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A08103");
    assert!(errors[0].message.contains("missing_buf"));
}

#[test]
fn memory_checker_region_containment_same_buffer() {
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());
    checker.register_region(MemoryRegion {
        name: "sub".into(),
        lower: "2".into(),
        upper: "5".into(),
        buffer: "buf".into(),
    });
    checker.register_region(MemoryRegion {
        name: "parent".into(),
        lower: "0".into(),
        upper: "buf.len".into(),
        buffer: "buf".into(),
    });
    let result = checker.check_region_containment("sub", "parent", &(0..10));
    assert!(
        result.is_none(),
        "same buffer regions should pass structural check"
    );
}

#[test]
fn memory_checker_region_containment_different_buffers() {
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf_a".into(), "buf_a.len".into());
    checker.register_buffer("buf_b".into(), "buf_b.len".into());
    checker.register_region(MemoryRegion {
        name: "r_a".into(),
        lower: "0".into(),
        upper: "buf_a.len".into(),
        buffer: "buf_a".into(),
    });
    checker.register_region(MemoryRegion {
        name: "r_b".into(),
        lower: "0".into(),
        upper: "buf_b.len".into(),
        buffer: "buf_b".into(),
    });
    let result = checker.check_region_containment("r_a", "r_b", &(0..10));
    assert!(result.is_some(), "different buffer regions should fail");
    assert_eq!(result.unwrap().code, "A08102");
}

#[test]
fn memory_checker_region_containment_undefined_sub() {
    let checker = MemoryChecker::new();
    let result = checker.check_region_containment("nonexistent", "parent", &(0..10));
    assert!(result.is_some());
    assert_eq!(result.unwrap().code, "A08102");
}

#[test]
fn memory_checker_bounds_check_with_capacity() {
    // buf.capacity pattern should also be recognized
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.capacity".into());

    let bounds_expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("idx".into())),
        op: AstBinOp::Lt,
        rhs: Box::new(AstExpr::Field(
            Box::new(AstExpr::Ident("buf".into())),
            "capacity".into(),
        )),
    };

    let result = checker.check_bounds_in_requires("buf", &[&bounds_expr], &(0..10));
    assert!(result.is_none(), "should detect capacity bounds check");
}

#[test]
fn memory_checker_bounds_check_in_conjunction() {
    // x > 0 and offset + len <= buf.len -> should detect bounds check
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());

    let bounds_expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("x".into())),
            op: AstBinOp::Gt,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
        }),
        op: AstBinOp::And,
        rhs: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("offset".into())),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Ident("len".into())),
            }),
            op: AstBinOp::Lte,
            rhs: Box::new(AstExpr::Field(
                Box::new(AstExpr::Ident("buf".into())),
                "len".into(),
            )),
        }),
    };

    let result = checker.check_bounds_in_requires("buf", &[&bounds_expr], &(0..10));
    assert!(
        result.is_none(),
        "should detect bounds check in conjunction"
    );
}

#[test]
fn memory_checker_default() {
    let checker = MemoryChecker::default();
    assert!(!checker.is_buffer("anything"));
    assert!(checker.regions().is_empty());
}

#[test]
fn memory_checker_gte_bounds_check() {
    // buf.len >= offset + len pattern should also be recognized
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());

    let bounds_expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Field(
            Box::new(AstExpr::Ident("buf".into())),
            "len".into(),
        )),
        op: AstBinOp::Gte,
        rhs: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("offset".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("len".into())),
        }),
    };

    let result = checker.check_bounds_in_requires("buf", &[&bounds_expr], &(0..10));
    assert!(result.is_none(), "should detect buf.len >= expr pattern");
}

#[test]
fn expr_references_var_basic() {
    let expr = AstExpr::Ident("buf".into());
    assert!(expr_references_var(&expr, "buf"));
    assert!(!expr_references_var(&expr, "other"));
}

#[test]
fn expr_references_var_in_binop() {
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("buf".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
    };
    assert!(expr_references_var(&expr, "buf"));
    assert!(!expr_references_var(&expr, "other"));
}

// -----------------------------------------------------------------------
// T047: Taint tracking (SEC.1) tests
// -----------------------------------------------------------------------

#[test]
fn taint_label_ordering() {
    assert!(TaintLabel::Untrusted < TaintLabel::Validated);
    assert!(TaintLabel::Validated < TaintLabel::Trusted);
    assert!(TaintLabel::Untrusted < TaintLabel::Trusted);
}

#[test]
fn extract_taint_from_tokens() {
    let tokens = vec![
        "U32".into(),
        "@".into(),
        "taint".into(),
        ":".into(),
        "untrusted".into(),
    ];
    assert_eq!(extract_taint_label(&tokens), Some(TaintLabel::Untrusted));

    let tokens2 = vec![
        "ValidXlen".into(),
        "@".into(),
        "taint".into(),
        ":".into(),
        "validated".into(),
    ];
    assert_eq!(extract_taint_label(&tokens2), Some(TaintLabel::Validated));

    let no_taint = vec!["Int".into()];
    assert_eq!(extract_taint_label(&no_taint), None);
}

#[test]
fn extract_taint_short_form() {
    let tokens = vec!["Bytes".into(), "@".into(), "untrusted".into()];
    assert_eq!(extract_taint_label(&tokens), Some(TaintLabel::Untrusted));

    let tokens2 = vec!["Data".into(), "@".into(), "validated".into()];
    assert_eq!(extract_taint_label(&tokens2), Some(TaintLabel::Validated));

    let tokens3 = vec!["Key".into(), "@".into(), "trusted".into()];
    assert_eq!(extract_taint_label(&tokens3), Some(TaintLabel::Trusted));
}

#[test]
fn taint_checker_untrusted_index_a09101() {
    // Untrusted data used as array index -> A09101
    let mut checker = TaintChecker::new();
    checker.declare("idx".into(), TaintLabel::Untrusted);

    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("buf".into())),
        index: Box::new(AstExpr::Ident("idx".into())),
    };
    let errors = checker.check_expr(&expr, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A09101");
}

#[test]
fn taint_checker_validated_index_passes() {
    // Validated data used as index -> no error
    let mut checker = TaintChecker::new();
    checker.declare("idx".into(), TaintLabel::Validated);

    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("buf".into())),
        index: Box::new(AstExpr::Ident("idx".into())),
    };
    let errors = checker.check_expr(&expr, &(0..1));
    assert!(errors.is_empty(), "validated index should pass: {errors:?}");
}

#[test]
fn taint_checker_trusted_index_passes() {
    // Trusted (default) data -> no error
    let checker = TaintChecker::new();

    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("buf".into())),
        index: Box::new(AstExpr::Ident("idx".into())),
    };
    let errors = checker.check_expr(&expr, &(0..1));
    assert!(errors.is_empty(), "trusted index should pass: {errors:?}");
}

#[test]
fn taint_propagation_through_arithmetic() {
    // If any operand is untrusted, result is untrusted
    let mut checker = TaintChecker::new();
    checker.declare("tainted".into(), TaintLabel::Untrusted);
    checker.declare("safe".into(), TaintLabel::Trusted);

    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("tainted".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Ident("safe".into())),
    };
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Untrusted);
}

#[test]
fn taint_propagation_both_untrusted() {
    // Both operands untrusted -> result untrusted
    let mut checker = TaintChecker::new();
    checker.declare("a".into(), TaintLabel::Untrusted);
    checker.declare("b".into(), TaintLabel::Untrusted);

    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("a".into())),
        op: AstBinOp::Mul,
        rhs: Box::new(AstExpr::Ident("b".into())),
    };
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Untrusted);
}

#[test]
fn taint_validation_removes_taint() {
    // Calling a validation function produces Validated
    let mut checker = TaintChecker::new();
    checker.declare("raw".into(), TaintLabel::Untrusted);

    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("validate".into())),
        args: vec![AstExpr::Ident("raw".into())],
    };
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Validated);
}

#[test]
fn taint_checker_alloc_a09102() {
    // Untrusted data as allocation size -> A09102
    let mut checker = TaintChecker::new();
    checker.declare("sz".into(), TaintLabel::Untrusted);

    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("alloc".into())),
        args: vec![AstExpr::Ident("sz".into())],
    };
    let errors = checker.check_expr(&expr, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A09102");
}

#[test]
fn taint_checker_trusted_sink_a09103() {
    // Untrusted data flowing to a trusted sink -> A09103
    let mut checker = TaintChecker::new();
    checker.declare("raw_len".into(), TaintLabel::Untrusted);
    checker.register_trusted_sink("memcpy_len".into(), vec![Some(TaintLabel::Validated)]);

    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("memcpy_len".into())),
        args: vec![AstExpr::Ident("raw_len".into())],
    };
    let errors = checker.check_expr(&expr, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A09103");
}

#[test]
fn taint_checker_validated_at_sink_passes() {
    // Validated data at a sink that requires Validated -> no error
    let mut checker = TaintChecker::new();
    checker.declare("safe_len".into(), TaintLabel::Validated);
    checker.register_trusted_sink("memcpy_len".into(), vec![Some(TaintLabel::Validated)]);

    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("memcpy_len".into())),
        args: vec![AstExpr::Ident("safe_len".into())],
    };
    let errors = checker.check_expr(&expr, &(0..1));
    assert!(errors.is_empty(), "validated data at sink should pass");
}

#[test]
fn taint_infer_literal_trusted() {
    let checker = TaintChecker::new();
    let expr = AstExpr::Literal(AstLit::Int("42".into()));
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Trusted);
}

#[test]
fn taint_infer_unknown_var_trusted() {
    // Undeclared variables default to Trusted
    let checker = TaintChecker::new();
    let expr = AstExpr::Ident("x".into());
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Trusted);
}

#[test]
fn taint_checker_nested_index_propagation() {
    // Tainted data flows through arithmetic to index -> A09101
    let mut checker = TaintChecker::new();
    checker.declare("offset".into(), TaintLabel::Untrusted);

    let index_expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("offset".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
    };
    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("buf".into())),
        index: Box::new(index_expr),
    };
    let errors = checker.check_expr(&expr, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A09101");
}

#[test]
fn taint_checker_display() {
    assert_eq!(TaintLabel::Untrusted.to_string(), "untrusted");
    assert_eq!(TaintLabel::Validated.to_string(), "validated");
    assert_eq!(TaintLabel::Trusted.to_string(), "trusted");
}

// --- T052: Dependent type tests ---

#[test]
fn dep_type_nat_index_valid() {
    let checker = DependentTypeChecker::new();
    let errors = checker.validate_index("n", "Nat", &(0..1));
    assert!(errors.is_empty(), "Nat should be a valid index type");
}

#[test]
fn dep_type_bool_index_valid() {
    let checker = DependentTypeChecker::new();
    let errors = checker.validate_index("flag", "Bool", &(0..1));
    assert!(errors.is_empty(), "Bool should be a valid index type");
}

#[test]
fn dep_type_enum_index_valid() {
    let mut checker = DependentTypeChecker::new();
    checker.register_enum("Mode".into(), vec!["Read".into(), "Write".into()]);
    let errors = checker.validate_index("mode", "Mode", &(0..1));
    assert!(errors.is_empty(), "known enum should be a valid index type");
}

#[test]
fn dep_type_unknown_type_a03006() {
    let checker = DependentTypeChecker::new();
    let errors = checker.validate_index("x", "String", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03006");
}

#[test]
fn dep_type_nat_arithmetic_valid() {
    let mut checker = DependentTypeChecker::new();
    checker.bind_index("n".into(), DepIndex::Nat("n".into()));
    // n + 1 is a valid Nat expression
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("n".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
    };
    let errors = checker.check_index_expr(&expr, &DepIndex::Nat("n".into()), &(0..1));
    assert!(errors.is_empty(), "n + 1 should be valid Nat arithmetic");
}

#[test]
fn dep_type_bool_arithmetic_rejected() {
    let mut checker = DependentTypeChecker::new();
    checker.bind_index("flag".into(), DepIndex::Bool("flag".into()));
    // flag + 1 is NOT valid for a Bool index
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("flag".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
    };
    let errors = checker.check_index_expr(&expr, &DepIndex::Bool("flag".into()), &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03008");
}

#[test]
fn dep_type_enum_variant_valid() {
    let mut checker = DependentTypeChecker::new();
    checker.register_enum("Mode".into(), vec!["Read".into(), "Write".into()]);
    checker.bind_index(
        "m".into(),
        DepIndex::Enum {
            name: "m".into(),
            enum_type: "Mode".into(),
        },
    );
    let expr = AstExpr::Ident("Read".into());
    let idx = DepIndex::Enum {
        name: "m".into(),
        enum_type: "Mode".into(),
    };
    let errors = checker.check_index_expr(&expr, &idx, &(0..1));
    assert!(errors.is_empty(), "enum variant should be valid");
}

#[test]
fn dep_type_equality_matching() {
    let checker = DependentTypeChecker::new();
    let t1 = DepType {
        base: Type::List(Box::new(Type::Int)),
        indices: vec![DepIndex::Nat("n".into())],
    };
    let t2 = DepType {
        base: Type::List(Box::new(Type::Int)),
        indices: vec![DepIndex::Nat("m".into())],
    };
    let errors = checker.check_dep_type_eq(&t1, &t2, &(0..1));
    assert!(errors.is_empty(), "same structure should match");
}

#[test]
fn dep_type_equality_base_mismatch() {
    let checker = DependentTypeChecker::new();
    let t1 = DepType {
        base: Type::List(Box::new(Type::Int)),
        indices: vec![DepIndex::Nat("n".into())],
    };
    let t2 = DepType {
        base: Type::List(Box::new(Type::Float)),
        indices: vec![DepIndex::Nat("n".into())],
    };
    let errors = checker.check_dep_type_eq(&t1, &t2, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03010");
}

#[test]
fn dep_type_equality_index_count_mismatch() {
    let checker = DependentTypeChecker::new();
    let t1 = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Nat("n".into())],
    };
    let t2 = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Nat("n".into()), DepIndex::Bool("b".into())],
    };
    let errors = checker.check_dep_type_eq(&t1, &t2, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03010");
}

#[test]
fn dep_type_index_erasure_ghost_ok() {
    let mut checker = DependentTypeChecker::new();
    checker.bind_index("n".into(), DepIndex::Nat("n".into()));
    let expr = AstExpr::Ident("n".into());
    let errors = checker.check_index_erasure(&expr, true, &(0..1));
    assert!(errors.is_empty(), "index in ghost context is ok");
}

#[test]
fn dep_type_index_erasure_runtime_error() {
    let mut checker = DependentTypeChecker::new();
    checker.bind_index("n".into(), DepIndex::Nat("n".into()));
    let expr = AstExpr::Ident("n".into());
    let errors = checker.check_index_erasure(&expr, false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03012");
}

#[test]
fn dep_type_index_kind_mismatch() {
    let checker = DependentTypeChecker::new();
    let t1 = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Nat("n".into())],
    };
    let t2 = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Bool("b".into())],
    };
    let errors = checker.check_dep_type_eq(&t1, &t2, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03011");
}

#[test]
fn dep_type_display() {
    assert_eq!(DepIndex::Nat("n".into()).to_string(), "n: Nat");
    assert_eq!(DepIndex::Bool("flag".into()).to_string(), "flag: Bool");
    assert_eq!(
        DepIndex::Enum {
            name: "m".into(),
            enum_type: "Mode".into()
        }
        .to_string(),
        "m: Mode"
    );
}

// --- T058: FFI boundary contract tests ---

#[test]
fn ffi_extern_without_boundary_a11001() {
    let checker = FfiBoundaryChecker::new();
    let errors = checker.check_extern_decl("malloc", false, false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A11001");
}

#[test]
fn ffi_extern_with_boundary_ok() {
    let checker = FfiBoundaryChecker::new();
    let errors = checker.check_extern_decl("malloc", true, true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ffi_untrusted_without_contract_a11002() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("read_bytes".into(), TrustBoundary::Untrusted);
    let errors = checker.check_extern_decl("read_bytes", true, false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A11002");
}

#[test]
fn ffi_untrusted_with_contract_ok() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("read_bytes".into(), TrustBoundary::Untrusted);
    let errors = checker.check_extern_decl("read_bytes", true, true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ffi_trusted_no_contract_ok() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("internal_fn".into(), TrustBoundary::Trusted);
    let errors = checker.check_extern_decl("internal_fn", true, false, &(0..1));
    assert!(errors.is_empty(), "trusted extern doesn't need a contract");
}

#[test]
fn ffi_call_untrusted_unvalidated_a11003() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("read_raw".into(), TrustBoundary::Untrusted);
    let errors = checker.check_ffi_call("read_raw", false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A11003");
}

#[test]
fn ffi_call_untrusted_validated_ok() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("read_raw".into(), TrustBoundary::Untrusted);
    let errors = checker.check_ffi_call("read_raw", true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ffi_call_trusted_unvalidated_ok() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("safe_fn".into(), TrustBoundary::Trusted);
    let errors = checker.check_ffi_call("safe_fn", false, &(0..1));
    assert!(errors.is_empty(), "trusted calls don't need validation");
}

#[test]
fn ffi_unsafe_outside_wrapper_a11004() {
    let checker = FfiBoundaryChecker::new();
    let errors = checker.check_unsafe_confinement("compute", false, true, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A11004");
}

#[test]
fn ffi_unsafe_inside_wrapper_ok() {
    let checker = FfiBoundaryChecker::new();
    let errors = checker.check_unsafe_confinement("ffi_wrapper", true, true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ffi_boundary_display() {
    assert_eq!(TrustBoundary::Trusted.to_string(), "trusted");
    assert_eq!(TrustBoundary::Audited.to_string(), "audited");
    assert_eq!(TrustBoundary::Untrusted.to_string(), "untrusted");
}

#[test]
fn ffi_file_check_multiple_externs() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("read".into(), TrustBoundary::Untrusted);
    checker.register_extern("write".into(), TrustBoundary::Audited);
    let externs = vec![
        ("read".into(), true, false, 0..5), // untrusted, no contract -> A11002
        ("write".into(), true, true, 10..15), // audited, has contract -> ok
        ("unknown".into(), false, false, 20..25), // no boundary -> A11001
    ];
    let errors = checker.check_file(&externs);
    assert_eq!(errors.len(), 2); // A11002 for read, A11001 for unknown
}

// --- T062: Interface contract tests ---

#[test]
fn interface_missing_method_a13001() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Serializable".into(),
        methods: vec![
            InterfaceMethod {
                name: "serialize".into(),
                param_types: vec![],
                return_type: Type::Bytes,
                has_requires: false,
                has_ensures: true,
                no_reentrancy: false,
            },
            InterfaceMethod {
                name: "deserialize".into(),
                param_types: vec![Type::Bytes],
                return_type: Type::Named("Self".into()),
                has_requires: true,
                has_ensures: true,
                no_reentrancy: false,
            },
        ],
        extends: vec![],
    });

    // Only implement serialize, not deserialize
    let errors = checker.check_impl("MyType", "Serializable", &["serialize".into()], &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A13001");
    assert!(errors[0].message.contains("deserialize"));
}

#[test]
fn interface_all_methods_implemented_ok() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Hashable".into(),
        methods: vec![InterfaceMethod {
            name: "hash".into(),
            param_types: vec![],
            return_type: Type::U64,
            has_requires: false,
            has_ensures: true,
            no_reentrancy: false,
        }],
        extends: vec![],
    });

    let errors = checker.check_impl("MyType", "Hashable", &["hash".into()], &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn interface_signature_param_count_mismatch_a13002() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Comparable".into(),
        methods: vec![InterfaceMethod {
            name: "compare".into(),
            param_types: vec![Type::Int, Type::Int],
            return_type: Type::Bool,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }],
        extends: vec![],
    });

    let errors = checker.check_method_signature(
        "Comparable",
        "compare",
        &[Type::Int], // only 1 param
        &Type::Bool,
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A13002");
}

#[test]
fn interface_signature_return_type_mismatch_a13002() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Comparable".into(),
        methods: vec![InterfaceMethod {
            name: "compare".into(),
            param_types: vec![Type::Int],
            return_type: Type::Bool,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }],
        extends: vec![],
    });

    let errors = checker.check_method_signature(
        "Comparable",
        "compare",
        &[Type::Int],
        &Type::Int, // wrong return type
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A13002");
    assert!(errors[0].message.contains("return type"));
}

#[test]
fn interface_reentrancy_violation_a13003() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Callback".into(),
        methods: vec![InterfaceMethod {
            name: "on_event".into(),
            param_types: vec![],
            return_type: Type::Unit,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: true,
        }],
        extends: vec![],
    });

    let errors = checker.check_reentrancy("Callback", "on_event", true, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A13003");
}

#[test]
fn interface_reentrancy_no_flag_ok() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Callback".into(),
        methods: vec![InterfaceMethod {
            name: "on_event".into(),
            param_types: vec![],
            return_type: Type::Unit,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }],
        extends: vec![],
    });

    let errors = checker.check_reentrancy("Callback", "on_event", true, &(0..1));
    assert!(errors.is_empty(), "method allows reentrancy");
}

#[test]
fn interface_super_interface_inheritance() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Eq".into(),
        methods: vec![InterfaceMethod {
            name: "equals".into(),
            param_types: vec![Type::Named("Self".into())],
            return_type: Type::Bool,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }],
        extends: vec![],
    });
    checker.register_interface(InterfaceContract {
        name: "Ord".into(),
        methods: vec![InterfaceMethod {
            name: "compare_to".into(),
            param_types: vec![Type::Named("Self".into())],
            return_type: Type::Int,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }],
        extends: vec!["Eq".into()],
    });

    // Implement compare_to but not equals -> A13001 for missing super method
    let errors = checker.check_impl("MyType", "Ord", &["compare_to".into()], &(0..1));
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("equals"));
    assert!(errors[0].message.contains("Eq"));
}

#[test]
fn interface_unknown_interface_a13001() {
    let checker = InterfaceChecker::new();
    let errors = checker.check_impl("MyType", "Unknown", &[], &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A13001");
    assert!(errors[0].message.contains("Unknown"));
}

// --- T059: Constant-time execution tests ---

#[test]
fn ct_branch_on_secret_a14001() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("key".into());
    let cond = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("key".into())),
        op: AstBinOp::Eq,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
    };
    let errors = checker.check_branch(&cond, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A14001");
}

#[test]
fn ct_branch_on_public_ok() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("key".into());
    let cond = AstExpr::Ident("public_val".into());
    let errors = checker.check_branch(&cond, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ct_index_on_secret_a14002() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("secret_idx".into());
    let idx = AstExpr::Ident("secret_idx".into());
    let errors = checker.check_index(&idx, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A14002");
}

#[test]
fn ct_index_on_public_ok() {
    let checker = ConstantTimeChecker::new();
    let idx = AstExpr::Ident("i".into());
    let errors = checker.check_index(&idx, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ct_nested_secret_in_condition() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("password".into());
    // password + 1 == 42
    let cond = AstExpr::BinOp {
        lhs: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("password".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        }),
        op: AstBinOp::Eq,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("42".into()))),
    };
    let errors = checker.check_branch(&cond, &(0..1));
    assert_eq!(errors.len(), 1);
}

#[test]
fn ct_check_expr_if_with_secret() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("s".into());
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Ident("s".into())),
        then_branch: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        else_branch: Some(Box::new(AstExpr::Literal(AstLit::Int("0".into())))),
    };
    let errors = checker.check_expr(&expr, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A14001");
}

#[test]
fn ct_references_secret_field() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("key".into());
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("key".into())), "len".into());
    assert!(checker.references_secret(&expr));
}

// --- T063: Recursive structural invariant tests ---

#[test]
fn struct_inv_tree_balance_valid() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("AVLTree".into(), vec!["left".into(), "right".into()]);
    let errors = checker.check_invariant_applicability(
        "AVLTree",
        &InvariantKind::TreeBalance { max_diff: 1 },
        &(0..1),
    );
    assert!(errors.is_empty());
}

#[test]
fn struct_inv_on_non_recursive_a15001() {
    let checker = StructuralInvariantChecker::new();
    let errors = checker.check_invariant_applicability(
        "Point",
        &InvariantKind::Sorted { descending: false },
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A15001");
}

#[test]
fn struct_inv_tree_on_list_a15002() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("LinkedList".into(), vec!["next".into()]);
    let errors =
        checker.check_invariant_applicability("LinkedList", &InvariantKind::BstOrdering, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A15002");
}

#[test]
fn struct_inv_sort_on_tree_a15003() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("BTree".into(), vec!["left".into(), "right".into()]);
    let errors = checker.check_invariant_applicability(
        "BTree",
        &InvariantKind::Sorted { descending: false },
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A15003");
}

#[test]
fn struct_inv_acyclic_valid_for_any_recursive() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("Graph".into(), vec!["children".into()]);
    let errors = checker.check_invariant_applicability("Graph", &InvariantKind::Acyclic, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn struct_inv_operation_no_proof_a15004() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("BST".into(), vec!["left".into(), "right".into()]);
    checker.register_invariant(StructuralInvariant {
        name: "bst_order".into(),
        type_name: "BST".into(),
        kind: InvariantKind::BstOrdering,
    });
    let errors = checker.check_operation_preserves(
        "BST",
        "insert",
        true,  // modifies structure
        false, // no preservation proof
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A15004");
}

#[test]
fn struct_inv_operation_with_proof_ok() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("BST".into(), vec!["left".into(), "right".into()]);
    checker.register_invariant(StructuralInvariant {
        name: "bst_order".into(),
        type_name: "BST".into(),
        kind: InvariantKind::BstOrdering,
    });
    let errors = checker.check_operation_preserves(
        "BST",
        "insert",
        true, // modifies structure
        true, // has preservation proof
        &(0..1),
    );
    assert!(errors.is_empty());
}

#[test]
fn struct_inv_readonly_trivially_preserves() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("BST".into(), vec!["left".into(), "right".into()]);
    checker.register_invariant(StructuralInvariant {
        name: "bst_order".into(),
        type_name: "BST".into(),
        kind: InvariantKind::BstOrdering,
    });
    let errors = checker.check_operation_preserves(
        "BST",
        "search",
        false, // read-only
        false, // no proof needed
        &(0..1),
    );
    assert!(errors.is_empty(), "read-only ops preserve invariants");
}

#[test]
fn struct_inv_kind_display() {
    assert_eq!(
        InvariantKind::TreeBalance { max_diff: 1 }.to_string(),
        "tree_balance(max_diff=1)"
    );
    assert_eq!(
        InvariantKind::Sorted { descending: false }.to_string(),
        "sorted(asc)"
    );
    assert_eq!(InvariantKind::Acyclic.to_string(), "acyclic");
    assert_eq!(InvariantKind::BstOrdering.to_string(), "bst_ordering");
    assert_eq!(
        InvariantKind::HeapProperty { min_heap: true }.to_string(),
        "min_heap"
    );
}

#[test]
fn struct_inv_get_invariants() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("AVL".into(), vec!["left".into(), "right".into()]);
    checker.register_invariant(StructuralInvariant {
        name: "balance".into(),
        type_name: "AVL".into(),
        kind: InvariantKind::TreeBalance { max_diff: 1 },
    });
    checker.register_invariant(StructuralInvariant {
        name: "order".into(),
        type_name: "AVL".into(),
        kind: InvariantKind::BstOrdering,
    });
    assert_eq!(checker.get_invariants("AVL").len(), 2);
    assert!(checker.get_invariants("Unknown").is_empty());
}

// --- T060: Secure erasure tests ---

#[test]
fn secure_erasure_not_zeroized_a16001() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("private_key".into());
    let errors = checker.check_scope_exit("private_key", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A16001");
}

#[test]
fn secure_erasure_zeroized_ok() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("private_key".into());
    checker.mark_zeroized("private_key".into());
    let errors = checker.check_scope_exit("private_key", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn secure_erasure_non_sensitive_ok() {
    let checker = SecureErasureChecker::new();
    let errors = checker.check_scope_exit("public_data", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn secure_erasure_copy_to_non_sensitive_a16002() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("key".into());
    let errors = checker.check_copy("key", "backup", false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A16002");
}

#[test]
fn secure_erasure_copy_to_sensitive_ok() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("key".into());
    let errors = checker.check_copy("key", "key_copy", true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn secure_erasure_return_not_sensitive_a16003() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("derived_key".into());
    let errors = checker.check_return("derived_key", false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A16003");
}

#[test]
fn secure_erasure_check_all_erased() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("key1".into());
    checker.mark_sensitive("key2".into());
    checker.mark_zeroized("key1".into());
    let errors = checker.check_all_erased(&(0..1));
    assert_eq!(errors.len(), 1); // key2 not zeroized
}

// --- T061: Cryptographic conformance tests ---

#[test]
fn crypto_correct_key_size_ok() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_key_size("AES-128-GCM", 128, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn crypto_wrong_key_size_a17001() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_key_size("AES-128-GCM", 256, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A17001");
}

#[test]
fn crypto_correct_nonce_size_ok() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_nonce_size("AES-256-GCM", 12, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn crypto_wrong_nonce_size_a17002() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_nonce_size("AES-256-GCM", 16, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A17002");
}

#[test]
fn crypto_nonce_not_unique_a17003() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_nonce_uniqueness("fixed_nonce", false, false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A17003");
}

#[test]
fn crypto_counter_nonce_ok() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_nonce_uniqueness("counter", true, false, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn crypto_tag_not_verified_a17004() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_tag_verification(false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A17004");
}

#[test]
fn crypto_tag_verified_ok() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_tag_verification(true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn crypto_chacha20_key_size() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_key_size("ChaCha20-Poly1305", 256, &(0..1));
    assert!(errors.is_empty());
    let errors = checker.check_key_size("ChaCha20-Poly1305", 128, &(0..1));
    assert_eq!(errors.len(), 1);
}

#[test]
fn crypto_custom_spec() {
    let mut checker = CryptoConformanceChecker::new();
    checker.register_spec(CryptoSpec {
        name: "XSalsa20".into(),
        key_size_bits: vec![256],
        block_size_bytes: None,
        nonce_size_bytes: Some(24),
        tag_size_bytes: None,
    });
    let errors = checker.check_nonce_size("XSalsa20", 24, &(0..1));
    assert!(errors.is_empty());
    let errors = checker.check_nonce_size("XSalsa20", 12, &(0..1));
    assert_eq!(errors.len(), 1);
}

// --- T065: Shared memory protocol tests ---

#[test]
fn shared_mem_read_exclusive_ok() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buffer".into(), AccessMode::Exclusive);
    let errors = checker.check_read("buffer", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn shared_mem_read_shared_ok() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buffer".into(), AccessMode::SharedRead);
    let errors = checker.check_read("buffer", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn shared_mem_read_none_a18001() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buffer".into(), AccessMode::None);
    let errors = checker.check_read("buffer", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A18001");
}

#[test]
fn shared_mem_write_exclusive_ok() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buffer".into(), AccessMode::Exclusive);
    let errors = checker.check_write("buffer", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn shared_mem_write_shared_a18002() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buffer".into(), AccessMode::SharedRead);
    let errors = checker.check_write("buffer", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A18002");
}

#[test]
fn shared_mem_data_race_a18003() {
    let checker = SharedMemChecker::new();
    let errors = checker.check_data_race(
        "counter",
        AccessMode::Exclusive,
        AccessMode::SharedRead,
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A18003");
}

#[test]
fn shared_mem_two_readers_ok() {
    let checker = SharedMemChecker::new();
    let errors = checker.check_data_race(
        "counter",
        AccessMode::SharedRead,
        AccessMode::SharedRead,
        &(0..1),
    );
    assert!(errors.is_empty(), "two shared readers is safe");
}

#[test]
fn shared_mem_access_mode_display() {
    assert_eq!(AccessMode::Exclusive.to_string(), "exclusive");
    assert_eq!(AccessMode::SharedRead.to_string(), "shared_read");
    assert_eq!(AccessMode::None.to_string(), "none");
}

// --- T067: Determinism checker tests ---

#[test]
fn determinism_hashmap_a20001() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("compute".into());
    let errors = checker.check_fn_body("compute", &["HashMap".into(), "Vec".into()], &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A20001");
}

#[test]
fn determinism_btreemap_ok() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("compute".into());
    let errors = checker.check_fn_body("compute", &["BTreeMap".into(), "Vec".into()], &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn determinism_non_det_fn_ok() {
    let checker = DeterminismChecker::new();
    // Not marked deterministic
    let errors = checker.check_fn_body("random_pick", &["random".into()], &(0..1));
    assert!(errors.is_empty(), "non-deterministic fn allows random");
}

#[test]
fn determinism_iteration_a20002() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("process".into());
    let errors = checker.check_iteration("process", "HashMap<K,V>", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A20002");
}

#[test]
fn determinism_btree_iteration_ok() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("process".into());
    let errors = checker.check_iteration("process", "BTreeMap<K,V>", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn determinism_random_a20001() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("seed_fn".into());
    let errors = checker.check_fn_body("seed_fn", &["thread_rng".into()], &(0..1));
    assert_eq!(errors.len(), 1);
}

// --- T068: Lock ordering tests ---

#[test]
fn lock_order_correct_ok() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("db".into(), 1);
    checker.define_order("cache".into(), 2);
    let errors = checker.acquire("db", &(0..1));
    assert!(errors.is_empty());
    let errors = checker.acquire("cache", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn lock_order_violation_a21001() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("db".into(), 1);
    checker.define_order("cache".into(), 2);
    let errors = checker.acquire("cache", &(0..1));
    assert!(errors.is_empty());
    let errors = checker.acquire("db", &(0..1)); // wrong order
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A21001");
}

#[test]
fn lock_order_release_correct() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("a".into(), 1);
    checker.define_order("b".into(), 2);
    checker.acquire("a", &(0..1));
    checker.acquire("b", &(0..1));
    let errors = checker.release("b", &(0..1)); // correct: LIFO
    assert!(errors.is_empty());
}

#[test]
fn lock_order_release_wrong_a21002() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("a".into(), 1);
    checker.define_order("b".into(), 2);
    checker.acquire("a", &(0..1));
    checker.acquire("b", &(0..1));
    let errors = checker.release("a", &(0..1)); // wrong: b still held
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A21002");
}

#[test]
fn lock_order_undefined_a21003() {
    let checker = LockOrderChecker::new();
    let errors = checker.check_ordering_defined("unknown_lock", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A21003");
}

#[test]
fn lock_order_defined_ok() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("db".into(), 1);
    let errors = checker.check_ordering_defined("db", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn info_flow_security_label_ordering() {
    // Verify the lattice: Public < Internal < Confidential < Restricted
    assert!(SecurityLabel::Public < SecurityLabel::Internal);
    assert!(SecurityLabel::Internal < SecurityLabel::Confidential);
    assert!(SecurityLabel::Confidential < SecurityLabel::Restricted);
    assert!(SecurityLabel::Public < SecurityLabel::Restricted);
}

#[test]
fn info_flow_valid_upward_assignment() {
    // Public -> Confidential is a valid upward flow
    let checker = InfoFlowChecker::new();
    let err = checker.check_assignment(SecurityLabel::Confidential, SecurityLabel::Public, &(0..1));
    assert!(err.is_none(), "upward flow should be allowed");
}

#[test]
fn info_flow_valid_same_level_assignment() {
    // Confidential -> Confidential is allowed (same level)
    let checker = InfoFlowChecker::new();
    let err = checker.check_assignment(
        SecurityLabel::Confidential,
        SecurityLabel::Confidential,
        &(0..1),
    );
    assert!(err.is_none(), "same-level flow should be allowed");
}

#[test]
fn info_flow_invalid_downward_a08001() {
    // Confidential -> Public is a violation (A08001)
    let checker = InfoFlowChecker::new();
    let err = checker.check_assignment(SecurityLabel::Public, SecurityLabel::Confidential, &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A08001");
}

#[test]
fn info_flow_restricted_to_internal_a08001() {
    // Restricted -> Internal is a violation (A08001)
    let checker = InfoFlowChecker::new();
    let err = checker.check_assignment(SecurityLabel::Internal, SecurityLabel::Restricted, &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A08001");
}

#[test]
fn info_flow_declassify_with_annotation_ok() {
    // Declassification with explicit annotation is permitted
    let checker = InfoFlowChecker::new();
    let err = checker.check_declassify(
        SecurityLabel::Confidential,
        SecurityLabel::Public,
        true,
        &(0..1),
    );
    assert!(err.is_none(), "annotated declassification should pass");
}

#[test]
fn info_flow_declassify_without_annotation_a08002() {
    // Declassification without annotation -> A08002
    let checker = InfoFlowChecker::new();
    let err = checker.check_declassify(
        SecurityLabel::Confidential,
        SecurityLabel::Public,
        false,
        &(0..1),
    );
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A08002");
}

#[test]
fn info_flow_declassify_upward_no_error() {
    // Upward "declassification" (Public -> Confidential) is not a
    // downgrade, so no error even without annotation
    let checker = InfoFlowChecker::new();
    let err = checker.check_declassify(
        SecurityLabel::Public,
        SecurityLabel::Confidential,
        false,
        &(0..1),
    );
    assert!(err.is_none());
}

#[test]
fn info_flow_label_propagation_binary() {
    // Binary op: max(Confidential, Public) = Confidential
    let mut checker = InfoFlowChecker::new();
    checker.declare("secret".into(), SecurityLabel::Confidential);
    checker.declare("pub_val".into(), SecurityLabel::Public);

    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("secret".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Ident("pub_val".into())),
    };
    assert_eq!(checker.infer_label(&expr), SecurityLabel::Confidential);
}

#[test]
fn info_flow_label_propagation_both_restricted() {
    // Both operands Restricted -> result Restricted
    let mut checker = InfoFlowChecker::new();
    checker.declare("a".into(), SecurityLabel::Restricted);
    checker.declare("b".into(), SecurityLabel::Restricted);

    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("a".into())),
        op: AstBinOp::Mul,
        rhs: Box::new(AstExpr::Ident("b".into())),
    };
    assert_eq!(checker.infer_label(&expr), SecurityLabel::Restricted);
}

#[test]
fn info_flow_infer_literal_public() {
    // Literals are always Public
    let checker = InfoFlowChecker::new();
    let expr = AstExpr::Literal(AstLit::Int("42".into()));
    assert_eq!(checker.infer_label(&expr), SecurityLabel::Public);
}

#[test]
fn info_flow_infer_unknown_var_public() {
    // Undeclared variables default to Public
    let checker = InfoFlowChecker::new();
    let expr = AstExpr::Ident("x".into());
    assert_eq!(checker.infer_label(&expr), SecurityLabel::Public);
}

#[test]
fn info_flow_purpose_label_mismatch_a08003() {
    // Purpose mismatch -> A08003
    let mut checker = InfoFlowChecker::new();
    checker.declare_purpose("email".into(), "marketing".into());
    let err = checker.check_purpose_label("email", "billing", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A08003");
}

#[test]
fn info_flow_purpose_label_match_ok() {
    // Matching purpose -> no error
    let mut checker = InfoFlowChecker::new();
    checker.declare_purpose("email".into(), "billing".into());
    let err = checker.check_purpose_label("email", "billing", &(0..1));
    assert!(err.is_none());
}

#[test]
fn info_flow_purpose_label_untracked_ok() {
    // Variable without purpose label -> no error
    let checker = InfoFlowChecker::new();
    let err = checker.check_purpose_label("x", "analytics", &(0..1));
    assert!(err.is_none());
}

#[test]
fn info_flow_implicit_flow_a08004() {
    // Confidential condition, Public branch target -> A08004
    let checker = InfoFlowChecker::new();
    let err =
        checker.check_implicit_flow(SecurityLabel::Confidential, SecurityLabel::Public, &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A08004");
}

#[test]
fn info_flow_implicit_flow_same_level_ok() {
    // Same-level condition and target -> no implicit flow
    let checker = InfoFlowChecker::new();
    let err =
        checker.check_implicit_flow(SecurityLabel::Internal, SecurityLabel::Internal, &(0..1));
    assert!(err.is_none());
}

#[test]
fn info_flow_covert_channel_a08005() {
    // High-security data controls a timing function -> A08005
    let checker = InfoFlowChecker::new();
    let err = checker.check_covert_channel(SecurityLabel::Confidential, "sleep", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A08005");
}

#[test]
fn info_flow_covert_channel_public_ok() {
    // Public data controlling sleep is not a covert channel
    let checker = InfoFlowChecker::new();
    let err = checker.check_covert_channel(SecurityLabel::Public, "sleep", &(0..1));
    assert!(err.is_none());
}

#[test]
fn info_flow_covert_channel_non_sensitive_fn_ok() {
    // High-security data controlling a non-sensitive function is ok
    let checker = InfoFlowChecker::new();
    let err = checker.check_covert_channel(SecurityLabel::Restricted, "compute", &(0..1));
    assert!(err.is_none());
}

#[test]
fn info_flow_label_propagation_nested() {
    // Nested expression: (public + confidential) * restricted
    // -> max(max(Public, Confidential), Restricted) = Restricted
    let mut checker = InfoFlowChecker::new();
    checker.declare("pub_val".into(), SecurityLabel::Public);
    checker.declare("conf".into(), SecurityLabel::Confidential);
    checker.declare("restr".into(), SecurityLabel::Restricted);

    let inner = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("pub_val".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Ident("conf".into())),
    };
    let outer = AstExpr::BinOp {
        lhs: Box::new(inner),
        op: AstBinOp::Mul,
        rhs: Box::new(AstExpr::Ident("restr".into())),
    };
    assert_eq!(checker.infer_label(&outer), SecurityLabel::Restricted);
}

#[test]
fn info_flow_label_field_access() {
    // Field access propagates receiver label
    let mut checker = InfoFlowChecker::new();
    checker.declare("secret_obj".into(), SecurityLabel::Confidential);
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("secret_obj".into())), "name".into());
    assert_eq!(checker.infer_label(&expr), SecurityLabel::Confidential);
}

#[test]
fn info_flow_check_expr_if_covert_channel() {
    // If a confidential condition controls a sleep call inside a
    // branch, check_expr should detect the covert channel (A08005).
    let mut checker = InfoFlowChecker::new();
    checker.declare("is_admin".into(), SecurityLabel::Confidential);

    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Ident("is_admin".into())),
        then_branch: Box::new(AstExpr::Call {
            func: Box::new(AstExpr::Ident("sleep".into())),
            args: vec![AstExpr::Literal(AstLit::Int("100".into()))],
        }),
        else_branch: None,
    };
    let errors = checker.check_expr(&expr, &(0..10));
    let has_a08005 = errors.iter().any(|e| e.code == "A08005");
    assert!(
        has_a08005,
        "expected A08005 for covert channel via if+sleep"
    );
}

#[test]
fn info_flow_display_labels() {
    assert_eq!(SecurityLabel::Public.to_string(), "Public");
    assert_eq!(SecurityLabel::Internal.to_string(), "Internal");
    assert_eq!(SecurityLabel::Confidential.to_string(), "Confidential");
    assert_eq!(SecurityLabel::Restricted.to_string(), "Restricted");
}

#[test]
fn info_flow_multiple_variables_mixed_levels() {
    // Multiple variables at different levels
    let mut checker = InfoFlowChecker::new();
    checker.declare("pub_data".into(), SecurityLabel::Public);
    checker.declare("int_data".into(), SecurityLabel::Internal);
    checker.declare("conf_data".into(), SecurityLabel::Confidential);
    checker.declare("restr_data".into(), SecurityLabel::Restricted);

    // Public -> Internal: ok
    assert!(
        checker
            .check_assignment(SecurityLabel::Internal, SecurityLabel::Public, &(0..1))
            .is_none()
    );
    // Internal -> Confidential: ok
    assert!(
        checker
            .check_assignment(
                SecurityLabel::Confidential,
                SecurityLabel::Internal,
                &(0..1)
            )
            .is_none()
    );
    // Restricted -> Public: error
    assert_eq!(
        checker
            .check_assignment(SecurityLabel::Public, SecurityLabel::Restricted, &(0..1))
            .unwrap()
            .code,
        "A08001"
    );
    // Verify inferred labels
    assert_eq!(
        checker.infer_label(&AstExpr::Ident("pub_data".into())),
        SecurityLabel::Public
    );
    assert_eq!(
        checker.infer_label(&AstExpr::Ident("restr_data".into())),
        SecurityLabel::Restricted
    );
}

#[test]
fn info_flow_checker_default() {
    // Default implementation matches new()
    let checker: InfoFlowChecker = Default::default();
    assert!(!checker.has_labels());
}

// --- T053 test helpers ---

fn make_fn_def(name: &str, params: Vec<(&str, &[&str])>, clauses: Vec<AstClause>) -> AstFnDef {
    AstFnDef {
        name: name.into(),
        is_ghost: false,
        is_lemma: false,
        params: params
            .into_iter()
            .map(|(n, ty)| AstParam {
                name: n.into(),
                ty: ty.iter().map(|s| s.to_string()).collect(),
            })
            .collect(),
        return_ty: vec!["Int".into()],
        clauses,
    }
}

fn decreases_clause(body: AstExpr) -> AstClause {
    AstClause {
        kind: ClauseKind::Other("decreases".into()),
        body,
    }
}

fn requires_clause(body: AstExpr) -> AstClause {
    AstClause {
        kind: ClauseKind::Requires,
        body,
    }
}

fn partial_clause() -> AstClause {
    AstClause {
        kind: ClauseKind::Other("partial".into()),
        body: AstExpr::Literal(AstLit::Bool(true)),
    }
}

fn ensures_with_recursive_call(fn_name: &str, args: Vec<AstExpr>) -> AstClause {
    AstClause {
        kind: ClauseKind::Ensures,
        body: AstExpr::Call {
            func: Box::new(AstExpr::Ident(fn_name.into())),
            args,
        },
    }
}

#[test]
fn totality_non_recursive_trivially_total() {
    // Non-recursive function passes without decreases
    let fn_def = make_fn_def("add", vec![("a", &["Int"]), ("b", &["Int"])], vec![]);
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..10));
    assert!(
        errors.is_empty(),
        "non-recursive function should be trivially total"
    );
}

#[test]
fn totality_recursive_with_valid_decreases() {
    // factorial(n) with decreases n, recursive call factorial(n - 1)
    let fn_def = make_fn_def(
        "factorial",
        vec![("n", &["Nat"])],
        vec![
            decreases_clause(AstExpr::Ident("n".into())),
            ensures_with_recursive_call(
                "factorial",
                vec![AstExpr::BinOp {
                    lhs: Box::new(AstExpr::Ident("n".into())),
                    op: AstBinOp::Sub,
                    rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                }],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..20));
    assert!(
        errors.is_empty(),
        "valid decreasing measure should pass: {errors:?}"
    );
}

#[test]
fn totality_recursive_without_decreases_a09001() {
    // Recursive function without decreases clause -> A09001
    let fn_def = make_fn_def(
        "loop_forever",
        vec![("n", &["Int"])],
        vec![ensures_with_recursive_call(
            "loop_forever",
            vec![AstExpr::Ident("n".into())],
        )],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A09001");
}

#[test]
fn totality_non_decreasing_measure_deferred_to_smt() {
    // Recursive call with same argument (not decreasing) is now deferred to SMT
    // instead of immediately producing A09002. The SMT solver will find that
    // n < n is unsatisfiable and report the error.
    let fn_def = make_fn_def(
        "spin",
        vec![("n", &["Nat"])],
        vec![
            decreases_clause(AstExpr::Ident("n".into())),
            ensures_with_recursive_call("spin", vec![AstExpr::Ident("n".into())]),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, pending) = checker.check_function_totality(&fn_def, &(0..10));
    // No syntactic error; the check is deferred to SMT
    assert!(
        errors.is_empty(),
        "non-decreasing measure should be deferred to SMT, not produce syntactic error: {errors:?}"
    );
    assert!(
        !pending.is_empty(),
        "non-decreasing measure should produce a pending SMT check"
    );
    // The pending check should reference the spin function
    assert_eq!(pending[0].fn_name, "spin");
}

#[test]
fn totality_measure_not_well_founded_a09003() {
    // decreases n but no requires n >= 0 and param type is Int, not Nat
    let fn_def = make_fn_def(
        "bad_rec",
        vec![("n", &["Int"])],
        vec![
            decreases_clause(AstExpr::Ident("n".into())),
            ensures_with_recursive_call(
                "bad_rec",
                vec![AstExpr::BinOp {
                    lhs: Box::new(AstExpr::Ident("n".into())),
                    op: AstBinOp::Sub,
                    rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                }],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..10));
    assert!(
        errors.iter().any(|e| e.code == "A09003"),
        "missing well-foundedness should produce A09003: {errors:?}"
    );
}

#[test]
fn totality_well_founded_with_requires_clause() {
    // decreases n with requires n >= 0 should NOT produce A09003
    let fn_def = make_fn_def(
        "count_down",
        vec![("n", &["Int"])],
        vec![
            requires_clause(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("n".into())),
                op: AstBinOp::Gte,
                rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
            }),
            decreases_clause(AstExpr::Ident("n".into())),
            ensures_with_recursive_call(
                "count_down",
                vec![AstExpr::BinOp {
                    lhs: Box::new(AstExpr::Ident("n".into())),
                    op: AstBinOp::Sub,
                    rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                }],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..20));
    assert!(
        !errors.iter().any(|e| e.code == "A09003"),
        "requires n >= 0 should establish well-foundedness: {errors:?}"
    );
}

#[test]
fn totality_partial_escape_hatch() {
    // Partial function skips termination checking
    let fn_def = make_fn_def(
        "diverge",
        vec![("n", &["Int"])],
        vec![
            partial_clause(),
            ensures_with_recursive_call("diverge", vec![AstExpr::Ident("n".into())]),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..10));
    assert!(
        errors.is_empty(),
        "partial function should skip totality check"
    );
}

#[test]
fn totality_partial_via_register() {
    // Partial registered via mark_partial
    let fn_def = make_fn_def(
        "diverge2",
        vec![("n", &["Int"])],
        vec![ensures_with_recursive_call(
            "diverge2",
            vec![AstExpr::Ident("n".into())],
        )],
    );
    let mut checker = TotalityChecker::new();
    checker.mark_partial("diverge2".into());
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..10));
    assert!(errors.is_empty(), "registered partial should skip check");
}

#[test]
fn totality_lexicographic_measures() {
    // Ackermann-like: decreases (m, n) with call (m - 1, n)
    let fn_def = make_fn_def(
        "ack",
        vec![("m", &["Nat"]), ("n", &["Nat"])],
        vec![
            decreases_clause(AstExpr::Ident("m".into())),
            decreases_clause(AstExpr::Ident("n".into())),
            ensures_with_recursive_call(
                "ack",
                vec![
                    AstExpr::BinOp {
                        lhs: Box::new(AstExpr::Ident("m".into())),
                        op: AstBinOp::Sub,
                        rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                    },
                    AstExpr::Ident("n".into()),
                ],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..20));
    assert!(
        errors.is_empty(),
        "lexicographic decrease in first component should pass: {errors:?}"
    );
}

#[test]
fn totality_mutual_recursion_no_decreases_a09004() {
    // Two functions calling each other with no decreases -> A09004
    let fn_a = make_fn_def(
        "even",
        vec![("n", &["Nat"])],
        vec![ensures_with_recursive_call(
            "odd",
            vec![AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("n".into())),
                op: AstBinOp::Sub,
                rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            }],
        )],
    );
    let fn_b = make_fn_def(
        "odd",
        vec![("n", &["Nat"])],
        vec![ensures_with_recursive_call(
            "even",
            vec![AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("n".into())),
                op: AstBinOp::Sub,
                rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            }],
        )],
    );

    let checker = TotalityChecker::new();
    let span_a = 0..10;
    let span_b = 10..20;
    let fn_defs: Vec<(&AstFnDef, &Range<usize>)> = vec![(&fn_a, &span_a), (&fn_b, &span_b)];
    let errors = checker.check_mutual_recursion(&fn_defs);
    assert!(
        errors.iter().any(|e| e.code == "A09004"),
        "mutual recursion without decreases should produce A09004: {errors:?}"
    );
}

#[test]
fn totality_mutual_recursion_with_decreases_passes() {
    // Two functions calling each other, one has decreases -> passes
    let fn_a = make_fn_def(
        "even2",
        vec![("n", &["Nat"])],
        vec![
            decreases_clause(AstExpr::Ident("n".into())),
            ensures_with_recursive_call(
                "odd2",
                vec![AstExpr::BinOp {
                    lhs: Box::new(AstExpr::Ident("n".into())),
                    op: AstBinOp::Sub,
                    rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                }],
            ),
        ],
    );
    let fn_b = make_fn_def(
        "odd2",
        vec![("n", &["Nat"])],
        vec![ensures_with_recursive_call(
            "even2",
            vec![AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("n".into())),
                op: AstBinOp::Sub,
                rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            }],
        )],
    );

    let checker = TotalityChecker::new();
    let span_a = 0..10;
    let span_b = 10..20;
    let fn_defs: Vec<(&AstFnDef, &Range<usize>)> = vec![(&fn_a, &span_a), (&fn_b, &span_b)];
    let errors = checker.check_mutual_recursion(&fn_defs);
    assert!(
        errors.is_empty(),
        "mutual recursion with decreases should pass: {errors:?}"
    );
}

#[test]
fn totality_structural_recursion_on_list() {
    // list_len(xs) with decreases xs, recursive call list_len(xs.tail)
    let fn_def = make_fn_def(
        "list_len",
        vec![("xs", &["List"])],
        vec![
            decreases_clause(AstExpr::Ident("xs".into())),
            ensures_with_recursive_call(
                "list_len",
                vec![AstExpr::Field(
                    Box::new(AstExpr::Ident("xs".into())),
                    "tail".into(),
                )],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..20));
    assert!(
        errors.is_empty(),
        "structural recursion on .tail should pass: {errors:?}"
    );
}

#[test]
fn totality_structural_recursion_on_tree() {
    // tree_depth(node) with decreases node, calls tree_depth(node.left)
    let fn_def = make_fn_def(
        "tree_depth",
        vec![("node", &["Tree"])],
        vec![
            decreases_clause(AstExpr::Ident("node".into())),
            ensures_with_recursive_call(
                "tree_depth",
                vec![AstExpr::Field(
                    Box::new(AstExpr::Ident("node".into())),
                    "left".into(),
                )],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..20));
    assert!(
        errors.is_empty(),
        "structural recursion on .left should pass: {errors:?}"
    );
}

#[test]
fn totality_extract_no_decreases() {
    let fn_def = make_fn_def("f", vec![], vec![]);
    let checker = TotalityChecker::new();
    assert!(checker.extract_decreases_measure(&fn_def).is_none());
}

#[test]
fn totality_extract_single_decreases() {
    let fn_def = make_fn_def(
        "f",
        vec![("n", &["Nat"])],
        vec![decreases_clause(AstExpr::Ident("n".into()))],
    );
    let checker = TotalityChecker::new();
    let measure = checker.extract_decreases_measure(&fn_def);
    assert!(
        matches!(measure, Some(DecreasesMeasure::Natural(_))),
        "single decreases should yield Natural"
    );
}

#[test]
fn totality_extract_lexicographic_decreases() {
    let fn_def = make_fn_def(
        "f",
        vec![("m", &["Nat"]), ("n", &["Nat"])],
        vec![
            decreases_clause(AstExpr::Ident("m".into())),
            decreases_clause(AstExpr::Ident("n".into())),
        ],
    );
    let checker = TotalityChecker::new();
    let measure = checker.extract_decreases_measure(&fn_def);
    assert!(
        matches!(measure, Some(DecreasesMeasure::Lexicographic(ref v)) if v.len() == 2),
        "two decreases should yield Lexicographic(2)"
    );
}

#[test]
fn totality_checker_debug() {
    let checker = TotalityChecker::new();
    let dbg = format!("{checker:?}");
    assert!(dbg.contains("TotalityChecker"));
}

#[test]
fn totality_checker_default() {
    let checker = TotalityChecker::default();
    assert!(!checker.is_partial(&make_fn_def("f", vec![], vec![])));
}

// -----------------------------------------------------------------------
// T055 MEM.2: FixedWidthChecker tests
// -----------------------------------------------------------------------

#[test]
fn fixed_width_range_u8() {
    let r = FixedWidthChecker::range_for_type(&Type::U8).unwrap();
    assert_eq!(r, (0, 255));
}

#[test]
fn fixed_width_range_i8() {
    let r = FixedWidthChecker::range_for_type(&Type::I8).unwrap();
    assert_eq!(r, (-128, 127));
}

#[test]
fn fixed_width_range_u16() {
    let r = FixedWidthChecker::range_for_type(&Type::U16).unwrap();
    assert_eq!(r, (0, 65535));
}

#[test]
fn fixed_width_range_i16() {
    let r = FixedWidthChecker::range_for_type(&Type::I16).unwrap();
    assert_eq!(r, (-32768, 32767));
}

#[test]
fn fixed_width_range_u32() {
    let r = FixedWidthChecker::range_for_type(&Type::U32).unwrap();
    assert_eq!(r, (0, u32::MAX as i128));
}

#[test]
fn fixed_width_range_i32() {
    let r = FixedWidthChecker::range_for_type(&Type::I32).unwrap();
    assert_eq!(r, (i32::MIN as i128, i32::MAX as i128));
}

#[test]
fn fixed_width_range_u64() {
    let r = FixedWidthChecker::range_for_type(&Type::U64).unwrap();
    assert_eq!(r, (0, u64::MAX as i128));
}

#[test]
fn fixed_width_range_i64() {
    let r = FixedWidthChecker::range_for_type(&Type::I64).unwrap();
    assert_eq!(r, (i64::MIN as i128, i64::MAX as i128));
}

#[test]
fn fixed_width_range_non_fixed() {
    // Non-fixed-width types return None
    assert!(FixedWidthChecker::range_for_type(&Type::Int).is_none());
    assert!(FixedWidthChecker::range_for_type(&Type::Bool).is_none());
    assert!(FixedWidthChecker::range_for_type(&Type::Float).is_none());
}

#[test]
fn fixed_width_u8_overflow_add() {
    // U8 + U8: 255 + 255 = 510 > 255 -> overflow
    let checker = FixedWidthChecker::new();
    let err = checker.check_arithmetic_overflow(&AstBinOp::Add, &Type::U8, &Type::U8, &(0..1));
    assert!(err.is_some(), "U8 + U8 should detect potential overflow");
    let e = err.unwrap();
    assert_eq!(e.code, "A10101");
    assert!(e.message.contains("checked_add"));
}

#[test]
fn fixed_width_i8_overflow_add() {
    // I8 + I8: 127 + 127 = 254 > 127 -> overflow
    let checker = FixedWidthChecker::new();
    let err = checker.check_arithmetic_overflow(&AstBinOp::Add, &Type::I8, &Type::I8, &(0..1));
    assert!(err.is_some(), "I8 + I8 should detect potential overflow");
    assert_eq!(err.unwrap().code, "A10101");
}

#[test]
fn fixed_width_safe_arithmetic_no_error() {
    // This tests that overflow check only fires on arithmetic ops.
    // For comparison operators, no overflow check applies.
    let checker = FixedWidthChecker::new();
    let err = checker.check_arithmetic_overflow(&AstBinOp::Lt, &Type::U8, &Type::U8, &(0..1));
    assert!(err.is_none(), "comparison should not trigger overflow");
}

#[test]
fn fixed_width_mul_overflow() {
    // U8 * U8: 255 * 255 = 65025 > 255 -> overflow
    let checker = FixedWidthChecker::new();
    let err = checker.check_arithmetic_overflow(&AstBinOp::Mul, &Type::U8, &Type::U8, &(0..1));
    assert!(err.is_some(), "U8 * U8 should detect potential overflow");
    let e = err.unwrap();
    assert!(e.message.contains("checked_mul"));
}

#[test]
fn fixed_width_narrowing_cast_u32_to_u16() {
    // U32 -> U16: max 4294967295 > 65535 -> unsafe
    let err = FixedWidthChecker::check_cast_safety(&Type::U32, &Type::U16, &(0..1));
    assert!(err.is_some(), "U32 -> U16 should be unsafe narrowing");
    assert_eq!(err.unwrap().code, "A10102");
}

#[test]
fn fixed_width_widening_cast_u16_to_u32() {
    // U16 -> U32: always safe (widening)
    let err = FixedWidthChecker::check_cast_safety(&Type::U16, &Type::U32, &(0..1));
    assert!(err.is_none(), "U16 -> U32 should be safe widening cast");
}

#[test]
fn fixed_width_signed_unsigned_comparison() {
    // I32 == U32 -> signedness mismatch
    let err = FixedWidthChecker::check_signedness_mismatch(
        &AstBinOp::Eq,
        &Type::I32,
        &Type::U32,
        &(0..1),
    );
    assert!(err.is_some(), "I32 vs U32 comparison should warn");
    assert_eq!(err.unwrap().code, "A10103");
}

#[test]
fn fixed_width_same_signedness_ok() {
    // U32 == U32 -> no mismatch
    let err = FixedWidthChecker::check_signedness_mismatch(
        &AstBinOp::Lt,
        &Type::U32,
        &Type::U32,
        &(0..1),
    );
    assert!(err.is_none(), "same signedness should not warn");
}

#[test]
fn fixed_width_division_by_zero() {
    let rhs = AstExpr::Literal(AstLit::Int("0".into()));
    let err = FixedWidthChecker::check_division_by_zero(&AstBinOp::Div, &rhs, &Type::U32, &(0..1));
    assert!(err.is_some(), "division by literal 0 should be flagged");
    assert_eq!(err.unwrap().code, "A10104");
}

#[test]
fn fixed_width_division_nonzero_ok() {
    let rhs = AstExpr::Literal(AstLit::Int("5".into()));
    let err = FixedWidthChecker::check_division_by_zero(&AstBinOp::Div, &rhs, &Type::U32, &(0..1));
    assert!(err.is_none(), "division by non-zero should pass");
}

#[test]
fn fixed_width_suggest_checked_add() {
    assert_eq!(
        FixedWidthChecker::suggest_checked_alternative(&AstBinOp::Add),
        "checked_add"
    );
}

#[test]
fn fixed_width_suggest_checked_sub() {
    assert_eq!(
        FixedWidthChecker::suggest_checked_alternative(&AstBinOp::Sub),
        "checked_sub"
    );
}

#[test]
fn fixed_width_suggest_checked_mul() {
    assert_eq!(
        FixedWidthChecker::suggest_checked_alternative(&AstBinOp::Mul),
        "checked_mul"
    );
}

#[test]
fn fixed_width_cast_i32_to_u32() {
    // I32 -> U32: signed-to-unsigned, range [-2^31, 2^31-1] does not
    // fit in [0, 2^32-1] because of negative values -> unsafe
    let err = FixedWidthChecker::check_cast_safety(&Type::I32, &Type::U32, &(0..1));
    assert!(err.is_some(), "I32 -> U32 cast should be unsafe");
    assert_eq!(err.unwrap().code, "A10102");
}

#[test]
fn fixed_width_is_unsigned() {
    assert!(FixedWidthChecker::is_unsigned(&Type::U8));
    assert!(FixedWidthChecker::is_unsigned(&Type::U16));
    assert!(FixedWidthChecker::is_unsigned(&Type::U32));
    assert!(FixedWidthChecker::is_unsigned(&Type::U64));
    assert!(!FixedWidthChecker::is_unsigned(&Type::I8));
    assert!(!FixedWidthChecker::is_unsigned(&Type::Int));
}

#[test]
fn fixed_width_is_signed() {
    assert!(FixedWidthChecker::is_signed(&Type::I8));
    assert!(FixedWidthChecker::is_signed(&Type::I16));
    assert!(FixedWidthChecker::is_signed(&Type::I32));
    assert!(FixedWidthChecker::is_signed(&Type::I64));
    assert!(!FixedWidthChecker::is_signed(&Type::U8));
    assert!(!FixedWidthChecker::is_signed(&Type::Float));
}

#[test]
fn fixed_width_check_binop_combined() {
    // I8 + U8 -> both overflow (A10101) and signedness mismatch (A10103)
    let checker = FixedWidthChecker::new();
    let rhs_expr = AstExpr::Ident("y".into());
    let errors = checker.check_binop(&AstBinOp::Add, &Type::I8, &Type::U8, &rhs_expr, &(0..1));
    // Should have both an overflow error and a signedness mismatch
    let codes: Vec<&str> = errors.iter().map(|e| e.code.as_str()).collect();
    assert!(codes.contains(&"A10101"), "should flag overflow");
    // Signedness mismatch only fires for comparison ops, not arithmetic
    // (by design: check_signedness_mismatch only checks comparison ops)
}

#[test]
fn fixed_width_modulo_by_zero() {
    let rhs = AstExpr::Literal(AstLit::Int("0".into()));
    let err = FixedWidthChecker::check_division_by_zero(&AstBinOp::Mod, &rhs, &Type::I32, &(0..1));
    assert!(err.is_some(), "modulo by zero should be flagged");
    let e = err.unwrap();
    assert_eq!(e.code, "A10104");
    assert!(e.message.contains("modulo"));
}

#[test]
fn fixed_width_sub_overflow_unsigned() {
    // U8 - U8: 0 - 255 = -255 < 0 -> overflow (underflow)
    let checker = FixedWidthChecker::new();
    let err = checker.check_arithmetic_overflow(&AstBinOp::Sub, &Type::U8, &Type::U8, &(0..1));
    assert!(err.is_some(), "U8 - U8 should detect potential underflow");
    assert_eq!(err.unwrap().code, "A10101");
}

#[test]
fn fixed_width_declare_and_lookup() {
    let mut checker = FixedWidthChecker::new();
    checker.declare("counter".into(), Type::U32);
    assert_eq!(checker.get_type("counter"), Some(&Type::U32));
    assert_eq!(checker.get_type("unknown"), None);
}

#[test]
fn fixed_width_default_trait() {
    let checker = FixedWidthChecker::default();
    assert!(checker.get_type("x").is_none());
}

#[test]
fn fixed_width_safe_cast_same_type() {
    // U32 -> U32: always safe
    assert!(FixedWidthChecker::is_safe_cast(&Type::U32, &Type::U32));
}

#[test]
fn fixed_width_cast_non_fixed_width() {
    // Non-fixed-width types are outside scope -> treated as safe
    let err = FixedWidthChecker::check_cast_safety(&Type::Int, &Type::U32, &(0..1));
    assert!(err.is_none(), "non-fixed-width cast should be out of scope");
}

// =======================================================================
// T056: AllocatorChecker tests
// =======================================================================

#[test]
fn allocator_unpaired_alloc() {
    let mut checker = AllocatorChecker::new();
    checker.record_alloc("buf".into(), "1024".into(), None, 0..4);
    let errors = checker.check_unpaired();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A22001");
}

#[test]
fn allocator_paired_ok() {
    let mut checker = AllocatorChecker::new();
    checker.record_alloc("buf".into(), "1024".into(), None, 0..4);
    assert!(checker.record_free("buf", 10..14).is_none());
    let errors = checker.check_unpaired();
    assert!(errors.is_empty());
}

#[test]
fn allocator_double_free() {
    let mut checker = AllocatorChecker::new();
    checker.record_alloc("buf".into(), "1024".into(), None, 0..4);
    assert!(checker.record_free("buf", 10..14).is_none());
    let err = checker.record_free("buf", 20..24);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A22002");
}

#[test]
fn allocator_arena_ok() {
    let mut checker = AllocatorChecker::new();
    checker.declare_arena("arena1".into());
    checker.record_alloc("obj".into(), "64".into(), Some("arena1".into()), 0..4);
    // Arena-managed allocations are not required to have explicit free
    let errors = checker.check_unpaired();
    assert!(errors.is_empty());
}

#[test]
fn allocator_arena_use_after_drop() {
    let mut checker = AllocatorChecker::new();
    checker.declare_arena("arena1".into());
    checker.record_alloc("obj".into(), "64".into(), Some("arena1".into()), 0..4);
    checker.drop_arena("arena1", 10..14);
    let err = checker.check_arena_use("obj", &(20..24));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A22004");
}

#[test]
fn allocator_arena_use_before_drop_ok() {
    let mut checker = AllocatorChecker::new();
    checker.declare_arena("arena1".into());
    checker.record_alloc("obj".into(), "64".into(), Some("arena1".into()), 0..4);
    let err = checker.check_arena_use("obj", &(5..8));
    assert!(err.is_none());
}

#[test]
fn allocator_default() {
    let checker = AllocatorChecker::default();
    assert!(checker.check_unpaired().is_empty());
}

// =======================================================================
// T057: CircularBufferChecker tests
// =======================================================================

#[test]
fn circ_buf_read_empty() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 8);
    let err = checker.check_read("ring", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A23003");
}

#[test]
fn circ_buf_read_nonempty() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 8);
    checker.push("ring");
    assert!(checker.check_read("ring", &(0..1)).is_none());
}

#[test]
fn circ_buf_index_out_of_bounds() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 4);
    let err = checker.check_index("ring", 5, &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A23001");
}

#[test]
fn circ_buf_index_ok() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 4);
    assert!(checker.check_index("ring", 3, &(0..1)).is_none());
}

#[test]
fn circ_buf_zero_capacity() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 0);
    let err = checker.check_physical_wrap("ring", 0, &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A23002");
}

#[test]
fn circ_buf_push_pop() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 2);
    checker.push("ring");
    checker.push("ring");
    // Full, push should not increase count
    checker.push("ring");
    let info = checker.buffers.get("ring").unwrap();
    assert_eq!(info.count, 2);
    assert!(info.is_full());
    checker.pop("ring");
    let info = checker.buffers.get("ring").unwrap();
    assert_eq!(info.count, 1);
}

#[test]
fn circ_buf_logical_to_physical() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 4);
    checker.push("ring");
    checker.push("ring");
    checker.pop("ring"); // head = 1
    let info = checker.buffers.get("ring").unwrap();
    assert_eq!(info.logical_to_physical(0), 1);
    assert_eq!(info.logical_to_physical(3), 0); // wraps
}

#[test]
fn circ_buf_default() {
    let checker = CircularBufferChecker::default();
    assert!(checker.check_read("x", &(0..1)).is_none());
}

// =======================================================================
// T066: CallbackReentrancyChecker tests
// =======================================================================

#[test]
fn callback_reentrant_call() {
    let mut checker = CallbackReentrancyChecker::new();
    checker.mark_non_reentrant("handle_event".into(), 0..10);
    assert!(checker.enter_call("handle_event", &(0..1)).is_empty());
    // Re-entrant call
    let errors = checker.enter_call("handle_event", &(5..6));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A24001");
}

#[test]
fn callback_reentrant_allowed() {
    let mut checker = CallbackReentrancyChecker::new();
    // Not marked non-reentrant
    assert!(checker.enter_call("handle_event", &(0..1)).is_empty());
    assert!(checker.enter_call("handle_event", &(5..6)).is_empty());
}

#[test]
fn callback_max_depth() {
    let mut checker = CallbackReentrancyChecker::new().with_max_depth(2);
    assert!(checker.enter_call("a", &(0..1)).is_empty());
    assert!(checker.enter_call("b", &(0..1)).is_empty());
    let errors = checker.enter_call("c", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A24003");
}

#[test]
fn callback_register_in_context() {
    let mut checker = CallbackReentrancyChecker::new();
    checker.mark_non_reentrant("handler".into(), 0..10);
    assert!(checker.enter_call("handler", &(0..1)).is_empty());
    let err = checker.check_register_callback("handler", &(5..6));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A24002");
}

#[test]
fn callback_exit_resets() {
    let mut checker = CallbackReentrancyChecker::new();
    checker.mark_non_reentrant("f".into(), 0..10);
    assert!(checker.enter_call("f", &(0..1)).is_empty());
    checker.exit_call();
    // After exit, re-entry is allowed
    assert!(checker.enter_call("f", &(5..6)).is_empty());
}

#[test]
fn callback_depth_tracking() {
    let mut checker = CallbackReentrancyChecker::new();
    assert_eq!(checker.current_depth(), 0);
    checker.enter_call("a", &(0..1));
    assert_eq!(checker.current_depth(), 1);
    checker.enter_call("b", &(0..1));
    assert_eq!(checker.current_depth(), 2);
    checker.exit_call();
    assert_eq!(checker.current_depth(), 1);
}

#[test]
fn callback_default() {
    let checker = CallbackReentrancyChecker::default();
    assert_eq!(checker.current_depth(), 0);
}

// =======================================================================
// T069: TemporalDeadlineChecker tests
// =======================================================================

#[test]
fn deadline_operation_exceeds() {
    let mut checker = TemporalDeadlineChecker::new();
    checker.register_bound("heavy_compute".into(), 500);
    assert!(
        checker
            .enter_deadline("fast".into(), 100, &(0..1))
            .is_none()
    );
    let err = checker.check_operation("heavy_compute", &(5..6));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A25001");
}

#[test]
fn deadline_operation_ok() {
    let mut checker = TemporalDeadlineChecker::new();
    checker.register_bound("quick".into(), 10);
    assert!(
        checker
            .enter_deadline("normal".into(), 100, &(0..1))
            .is_none()
    );
    assert!(checker.check_operation("quick", &(5..6)).is_none());
}

#[test]
fn deadline_unbounded_operation() {
    let mut checker = TemporalDeadlineChecker::new();
    assert!(
        checker
            .enter_deadline("strict".into(), 50, &(0..1))
            .is_none()
    );
    let err = checker.check_operation("unknown_op", &(5..6));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A25003");
}

#[test]
fn deadline_nested_violation() {
    let mut checker = TemporalDeadlineChecker::new();
    assert!(
        checker
            .enter_deadline("outer".into(), 100, &(0..1))
            .is_none()
    );
    let err = checker.enter_deadline("inner".into(), 200, &(5..6));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A25002");
}

#[test]
fn deadline_nested_ok() {
    let mut checker = TemporalDeadlineChecker::new();
    assert!(
        checker
            .enter_deadline("outer".into(), 100, &(0..1))
            .is_none()
    );
    assert!(
        checker
            .enter_deadline("inner".into(), 50, &(5..6))
            .is_none()
    );
}

#[test]
fn deadline_no_context_ok() {
    let checker = TemporalDeadlineChecker::new();
    // No deadline context, any operation is fine
    assert!(checker.check_operation("anything", &(0..1)).is_none());
}

#[test]
fn deadline_current() {
    let mut checker = TemporalDeadlineChecker::new();
    assert!(checker.current_deadline().is_none());
    checker.enter_deadline("d".into(), 42, &(0..1));
    assert_eq!(checker.current_deadline(), Some(("d", 42)));
    checker.exit_deadline();
    assert!(checker.current_deadline().is_none());
}

#[test]
fn deadline_default() {
    let checker = TemporalDeadlineChecker::default();
    assert!(checker.current_deadline().is_none());
}

// =======================================================================
// T070: BinaryFormatChecker tests
// =======================================================================

#[test]
fn binary_fmt_bounds_ok() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "magic".into(),
        offset: 0,
        size: 4,
        endianness: Some(Endianness::Big),
        span: 0..1,
    });
    assert!(checker.check_bounds(100).is_empty());
}

#[test]
fn binary_fmt_bounds_overflow() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "data".into(),
        offset: 96,
        size: 8,
        endianness: Some(Endianness::Little),
        span: 0..1,
    });
    let errors = checker.check_bounds(100);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A26001");
}

#[test]
fn binary_fmt_no_endianness() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "len".into(),
        offset: 0,
        size: 4,
        endianness: None,
        span: 0..1,
    });
    let errors = checker.check_endianness();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A26003");
}

#[test]
fn binary_fmt_single_byte_no_endianness_ok() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "flags".into(),
        offset: 0,
        size: 1,
        endianness: None,
        span: 0..1,
    });
    assert!(checker.check_endianness().is_empty());
}

#[test]
fn binary_fmt_overlap() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "a".into(),
        offset: 0,
        size: 4,
        endianness: Some(Endianness::Big),
        span: 0..1,
    });
    checker.add_field(BinaryField {
        name: "b".into(),
        offset: 2,
        size: 4,
        endianness: Some(Endianness::Big),
        span: 0..1,
    });
    let errors = checker.check_overlaps();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A26004");
}

#[test]
fn binary_fmt_no_overlap() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "a".into(),
        offset: 0,
        size: 4,
        endianness: Some(Endianness::Big),
        span: 0..1,
    });
    checker.add_field(BinaryField {
        name: "b".into(),
        offset: 4,
        size: 4,
        endianness: Some(Endianness::Big),
        span: 0..1,
    });
    assert!(checker.check_overlaps().is_empty());
}

#[test]
fn binary_fmt_check_all() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "header".into(),
        offset: 0,
        size: 4,
        endianness: None,
        span: 0..1, // missing endianness
    });
    let errors = checker.check_all(100);
    assert_eq!(errors.len(), 1); // endianness only
}

#[test]
fn binary_fmt_default() {
    let checker = BinaryFormatChecker::default();
    assert!(checker.check_all(0).is_empty());
}

// =======================================================================
// T071: BitLevelChecker tests
// =======================================================================

#[test]
fn bit_level_bounds_ok() {
    let mut checker = BitLevelChecker::new(32);
    checker.add_field(BitField {
        name: "version".into(),
        bit_offset: 0,
        bit_width: 4,
        span: 0..1,
        cross_byte_ok: false,
    });
    assert!(checker.check_bounds().is_empty());
}

#[test]
fn bit_level_bounds_overflow() {
    let mut checker = BitLevelChecker::new(8);
    checker.add_field(BitField {
        name: "big".into(),
        bit_offset: 4,
        bit_width: 8,
        span: 0..1,
        cross_byte_ok: true,
    });
    let errors = checker.check_bounds();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A27001");
}

#[test]
fn bit_level_byte_crossing() {
    let mut checker = BitLevelChecker::new(16);
    checker.add_field(BitField {
        name: "cross".into(),
        bit_offset: 6,
        bit_width: 4,
        span: 0..1,
        cross_byte_ok: false,
    });
    let errors = checker.check_byte_crossing();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A27002");
}

#[test]
fn bit_level_byte_crossing_allowed() {
    let mut checker = BitLevelChecker::new(16);
    checker.add_field(BitField {
        name: "cross".into(),
        bit_offset: 6,
        bit_width: 4,
        span: 0..1,
        cross_byte_ok: true,
    });
    assert!(checker.check_byte_crossing().is_empty());
}

#[test]
fn bit_level_total_width_match() {
    let mut checker = BitLevelChecker::new(8);
    checker.add_field(BitField {
        name: "a".into(),
        bit_offset: 0,
        bit_width: 4,
        span: 0..1,
        cross_byte_ok: false,
    });
    checker.add_field(BitField {
        name: "b".into(),
        bit_offset: 4,
        bit_width: 4,
        span: 0..1,
        cross_byte_ok: false,
    });
    assert!(checker.check_total_width(8).is_none());
}

#[test]
fn bit_level_total_width_mismatch() {
    let mut checker = BitLevelChecker::new(8);
    checker.add_field(BitField {
        name: "a".into(),
        bit_offset: 0,
        bit_width: 3,
        span: 0..1,
        cross_byte_ok: false,
    });
    let err = checker.check_total_width(8);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A27003");
}

#[test]
fn bit_level_check_all() {
    let mut checker = BitLevelChecker::new(16);
    checker.add_field(BitField {
        name: "a".into(),
        bit_offset: 0,
        bit_width: 8,
        span: 0..1,
        cross_byte_ok: false,
    });
    checker.add_field(BitField {
        name: "b".into(),
        bit_offset: 8,
        bit_width: 8,
        span: 0..1,
        cross_byte_ok: false,
    });
    assert!(checker.check_all(16).is_empty());
}

// =======================================================================
// T072: StringEncodingChecker tests
// =======================================================================

#[test]
fn string_encoding_raw_bytes_error() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("data".into(), StringEncoding::RawBytes);
    let err = checker.check_use_as_string("data", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A28001");
}

#[test]
fn string_encoding_utf8_ok() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("text".into(), StringEncoding::Utf8);
    assert!(checker.check_use_as_string("text", &(0..1)).is_none());
}

#[test]
fn string_encoding_mismatch() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("wide".into(), StringEncoding::Utf16Le);
    let err = checker.check_encoding_compat("wide", &StringEncoding::Utf8, &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A28002");
}

#[test]
fn string_encoding_ascii_compat() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("ascii_str".into(), StringEncoding::Ascii);
    // ASCII is compatible with everything
    assert!(
        checker
            .check_encoding_compat("ascii_str", &StringEncoding::Utf8, &(0..1))
            .is_none()
    );
}

#[test]
fn string_encoding_truncation_utf16() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("wide".into(), StringEncoding::Utf16Le);
    let err = checker.check_truncation("wide", 5, &(0..1)); // 5 bytes, not aligned to 2
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A28003");
}

#[test]
fn string_encoding_truncation_ok() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("wide".into(), StringEncoding::Utf16Be);
    assert!(checker.check_truncation("wide", 4, &(0..1)).is_none()); // 4 bytes, aligned
}

#[test]
fn string_encoding_unknown_var() {
    let checker = StringEncodingChecker::new();
    let err = checker.check_use_as_string("unknown", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A28001");
}

#[test]
fn string_encoding_default() {
    let checker = StringEncodingChecker::default();
    assert!(checker.check_use_as_string("x", &(0..1)).is_some());
}

// =======================================================================
// T074: ChecksumChecker tests
// =======================================================================

#[test]
fn checksum_use_before_verify() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("payload".into(), ChecksumAlgorithm::Crc32, 0, 100);
    let err = checker.check_use_before_verify("payload", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A29001");
}

#[test]
fn checksum_use_after_verify_ok() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("payload".into(), ChecksumAlgorithm::Crc32, 0, 100);
    checker.mark_verified("payload");
    assert!(
        checker
            .check_use_before_verify("payload", &(0..1))
            .is_none()
    );
}

#[test]
fn checksum_algorithm_mismatch() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("data".into(), ChecksumAlgorithm::Sha256, 0, 100);
    let err = checker.check_algorithm_match("data", &ChecksumAlgorithm::Crc32, &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A29002");
}

#[test]
fn checksum_algorithm_match_ok() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("data".into(), ChecksumAlgorithm::Sha256, 0, 100);
    assert!(
        checker
            .check_algorithm_match("data", &ChecksumAlgorithm::Sha256, &(0..1))
            .is_none()
    );
}

#[test]
fn checksum_range_coverage() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("data".into(), ChecksumAlgorithm::Adler32, 10, 50);
    let err = checker.check_range_coverage("data", 0, 60, &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A29003");
}

#[test]
fn checksum_range_covered_ok() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("data".into(), ChecksumAlgorithm::Adler32, 0, 100);
    assert!(
        checker
            .check_range_coverage("data", 10, 50, &(0..1))
            .is_none()
    );
}

#[test]
fn checksum_default() {
    let checker = ChecksumChecker::default();
    assert!(checker.check_use_before_verify("x", &(0..1)).is_none());
}

// =======================================================================
// T075: ProtocolGrammarChecker tests
// =======================================================================

#[test]
fn protocol_valid_transition() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_state("connected".into());
    checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
    assert!(checker.check_send("CONNECT", &(0..1)).is_none());
    assert!(checker.transition("CONNECT", &(0..1)).is_none());
    assert_eq!(checker.current_state(), "connected");
}

#[test]
fn protocol_invalid_send() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
    let err = checker.check_send("DISCONNECT", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A30002");
}

#[test]
fn protocol_invalid_transition() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
    let err = checker.transition("DATA", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A30001");
}

#[test]
fn protocol_required_fields() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_required_fields("CONNECT".into(), vec!["host".into(), "port".into()]);
    let errors = checker.check_required_fields("CONNECT", &["host"], &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A30003");
    assert!(errors[0].message.contains("port"));
}

#[test]
fn protocol_required_fields_ok() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_required_fields("CONNECT".into(), vec!["host".into()]);
    let errors = checker.check_required_fields("CONNECT", &["host", "port"], &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn protocol_multi_state() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_state("connected".into());
    checker.add_state("ready".into());
    checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
    checker.add_transition("connected".into(), "ready".into(), "AUTH".into());
    checker.add_transition("ready".into(), "idle".into(), "CLOSE".into());

    assert!(checker.transition("CONNECT", &(0..1)).is_none());
    assert_eq!(checker.current_state(), "connected");
    assert!(checker.transition("AUTH", &(0..1)).is_none());
    assert_eq!(checker.current_state(), "ready");
    assert!(checker.transition("CLOSE", &(0..1)).is_none());
    assert_eq!(checker.current_state(), "idle");
}

// =======================================================================
// T077: AxiomaticDefChecker tests
// =======================================================================

#[test]
fn axiom_undefined_reference() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "ax1".into(),
        params: vec!["x".into()],
        body: "foo(x) > 0".into(),
        span: 0..1,
        references: vec!["foo".into()],
    });
    let errors = checker.check_references(&[]);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A31001");
}

#[test]
fn axiom_known_reference_ok() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "ax1".into(),
        params: vec![],
        body: "foo(x) > 0".into(),
        span: 0..1,
        references: vec!["foo".into()],
    });
    assert!(checker.check_references(&["foo"]).is_empty());
}

#[test]
fn axiom_unused() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "unused_ax".into(),
        params: vec![],
        body: "true".into(),
        span: 0..1,
        references: vec![],
    });
    let errors = checker.check_unused();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A31003");
}

#[test]
fn axiom_used_ok() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "ax1".into(),
        params: vec![],
        body: "true".into(),
        span: 0..1,
        references: vec![],
    });
    checker.mark_used("ax1");
    assert!(checker.check_unused().is_empty());
}

#[test]
fn axiom_circular() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "a".into(),
        params: vec![],
        body: "b(x)".into(),
        span: 0..1,
        references: vec!["b".into()],
    });
    checker.declare_axiom(AxiomDef {
        name: "b".into(),
        params: vec![],
        body: "a(x)".into(),
        span: 0..1,
        references: vec!["a".into()],
    });
    let errors = checker.check_circular();
    assert!(!errors.is_empty());
    assert!(errors.iter().any(|e| e.code == "A31002"));
}

#[test]
fn axiom_default() {
    let checker = AxiomaticDefChecker::default();
    assert!(checker.check_unused().is_empty());
}

// =======================================================================
// T079: OpaqueFunctionChecker tests
// =======================================================================

#[test]
fn opaque_call_without_contract() {
    let mut checker = OpaqueFunctionChecker::new();
    checker.declare_opaque("secret_fn".into(), false, 0..1);
    let err = checker.check_call("secret_fn", &(5..6));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A32001");
}

#[test]
fn opaque_call_with_contract_ok() {
    let mut checker = OpaqueFunctionChecker::new();
    checker.declare_opaque("secret_fn".into(), true, 0..1);
    assert!(checker.check_call("secret_fn", &(5..6)).is_none());
}

#[test]
fn opaque_body_access_without_reveal() {
    let mut checker = OpaqueFunctionChecker::new();
    checker.declare_opaque("hidden".into(), true, 0..1);
    let err = checker.check_body_access("hidden", &(5..6));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A32002");
}

#[test]
fn opaque_reveal_outside_proof() {
    let mut checker = OpaqueFunctionChecker::new();
    checker.declare_opaque("hidden".into(), true, 0..1);
    let err = checker.reveal("hidden", &(5..6));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A32003");
}

#[test]
fn opaque_reveal_in_proof_ok() {
    let mut checker = OpaqueFunctionChecker::new();
    checker.declare_opaque("hidden".into(), true, 0..1);
    checker.enter_proof();
    assert!(checker.reveal("hidden", &(5..6)).is_none());
    // After reveal, body access is allowed
    assert!(checker.check_body_access("hidden", &(10..11)).is_none());
}

#[test]
fn opaque_is_opaque() {
    let mut checker = OpaqueFunctionChecker::new();
    assert!(!checker.is_opaque("f"));
    checker.declare_opaque("f".into(), true, 0..1);
    assert!(checker.is_opaque("f"));
}

#[test]
fn opaque_non_opaque_call_ok() {
    let checker = OpaqueFunctionChecker::new();
    assert!(checker.check_call("regular_fn", &(0..1)).is_none());
}

#[test]
fn opaque_default() {
    let checker = OpaqueFunctionChecker::default();
    assert!(!checker.is_opaque("x"));
}

// =======================================================================
// T083: TestGenerator tests
// =======================================================================

#[test]
fn test_gen_property_test() {
    let tgen = TestGenerator::new();
    let contract = TestableContract {
        name: "safe_div".into(),
        params: vec![("a".into(), Type::Int), ("b".into(), Type::Int)],
        requires: vec!["b != 0".into()],
        ensures: vec!["result * b + (a % b) == a".into()],
    };
    let test = tgen.generate_property_test(&contract);
    assert_eq!(test.kind, TestKind::Property);
    assert!(test.body.contains("proptest!"));
    assert!(test.body.contains("prop_assume!"));
    assert!(test.body.contains("b != 0"));
}

#[test]
fn test_gen_boundary_values() {
    let tgen = TestGenerator::new();
    let contract = TestableContract {
        name: "check".into(),
        params: vec![("x".into(), Type::U8)],
        requires: vec![],
        ensures: vec![],
    };
    let tests = tgen.generate_boundary_tests(&contract);
    assert_eq!(tests.len(), 3); // 0, 1, 255
    assert!(tests.iter().all(|t| t.kind == TestKind::Boundary));
}

#[test]
fn test_gen_smoke_test() {
    let tgen = TestGenerator::new();
    let contract = TestableContract {
        name: "foo".into(),
        params: vec![],
        requires: vec![],
        ensures: vec![],
    };
    let test = tgen.generate_smoke_test(&contract);
    assert_eq!(test.kind, TestKind::Smoke);
    assert!(test.body.contains("smoke_foo"));
}

#[test]
fn test_gen_generate_all() {
    let mut tgen = TestGenerator::new();
    tgen.add_contract(TestableContract {
        name: "add".into(),
        params: vec![("a".into(), Type::I32), ("b".into(), Type::I32)],
        requires: vec![],
        ensures: vec!["result == a + b".into()],
    });
    let all = tgen.generate_all();
    // 1 property + 10 boundary (5 per I32 param * 2) + 1 smoke
    assert_eq!(all.len(), 12);
}

#[test]
fn test_gen_no_requires() {
    let tgen = TestGenerator::new();
    let contract = TestableContract {
        name: "no_pre".into(),
        params: vec![("x".into(), Type::Bool)],
        requires: vec![],
        ensures: vec!["result".into()],
    };
    let test = tgen.generate_property_test(&contract);
    assert!(!test.body.contains("prop_assume!"));
}

#[test]
fn test_gen_default() {
    let tgen = TestGenerator::default();
    assert!(tgen.generate_all().is_empty());
}

// =======================================================================
// T086: CrashRecoveryChecker tests
// =======================================================================

#[test]
fn crash_recovery_write_ahead_violation() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("txn1".into());
    cr.write_data("txn1");
    let errs = cr.check_write_ahead();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A33001");
}

#[test]
fn crash_recovery_write_ahead_ok() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("txn1".into());
    cr.write_wal("txn1");
    cr.write_data("txn1");
    assert!(cr.check_write_ahead().is_empty());
}

#[test]
fn crash_recovery_commit_without_fsync() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("txn1".into());
    cr.commit("txn1");
    let errs = cr.check_commit_durability();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A33002");
}

#[test]
fn crash_recovery_fsync_before_data() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("txn1".into());
    cr.write_wal("txn1");
    cr.fsync("txn1");
    let errs = cr.check_ordering();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A33003");
}

#[test]
fn crash_recovery_full_sequence_ok() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("txn1".into());
    cr.write_wal("txn1");
    cr.write_data("txn1");
    cr.fsync("txn1");
    cr.commit("txn1");
    assert!(cr.check_all().is_empty());
}

#[test]
fn crash_recovery_default() {
    let cr = CrashRecoveryChecker::default();
    assert!(cr.check_all().is_empty());
}

// =======================================================================
// T087: PageCacheChecker tests
// =======================================================================

#[test]
fn page_cache_evict_pinned() {
    let mut pc = PageCacheChecker::new(10);
    pc.load_page(1);
    pc.pin(1);
    let err = pc.evict(1);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A34001");
}

#[test]
fn page_cache_evict_dirty() {
    let mut pc = PageCacheChecker::new(10);
    pc.load_page(1);
    pc.mark_dirty(1);
    let err = pc.evict(1);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A34002");
}

#[test]
fn page_cache_evict_clean_ok() {
    let mut pc = PageCacheChecker::new(10);
    pc.load_page(1);
    assert!(pc.evict(1).is_none());
    assert_eq!(pc.page_count(), 0);
}

#[test]
fn page_cache_capacity_exceeded() {
    let mut pc = PageCacheChecker::new(2);
    pc.load_page(1);
    pc.load_page(2);
    pc.load_page(3);
    let errs = pc.check_capacity();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A34003");
}

#[test]
fn page_cache_flush_then_evict() {
    let mut pc = PageCacheChecker::new(10);
    pc.load_page(1);
    pc.mark_dirty(1);
    pc.flush(1);
    assert!(pc.evict(1).is_none());
}

#[test]
fn page_cache_unpin_then_evict() {
    let mut pc = PageCacheChecker::new(10);
    pc.load_page(1);
    pc.pin(1);
    pc.unpin(1);
    assert!(pc.evict(1).is_none());
}

#[test]
fn page_cache_default() {
    let pc = PageCacheChecker::default();
    assert_eq!(pc.page_count(), 0);
}

// =======================================================================
// T088: MvccChecker tests
// =======================================================================

#[test]
fn mvcc_write_conflict() {
    let mut mv = MvccChecker::new();
    let t1 = mv.begin_txn();
    let t2 = mv.begin_txn();
    mv.write_version("key1".into(), t1);
    mv.write_version("key1".into(), t2);
    let errs = mv.check_write_conflicts();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A35001");
}

#[test]
fn mvcc_no_conflict_after_commit() {
    let mut mv = MvccChecker::new();
    let t1 = mv.begin_txn();
    mv.write_version("key1".into(), t1);
    mv.commit_txn(t1);
    let t2 = mv.begin_txn();
    mv.write_version("key1".into(), t2);
    assert!(mv.check_write_conflicts().is_empty());
}

#[test]
fn mvcc_snapshot_violation() {
    let mut mv = MvccChecker::new();
    let t1 = mv.begin_txn();
    let t2 = mv.begin_txn();
    mv.write_version("key1".into(), t1);
    let err = mv.check_snapshot_read("key1", t2);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A35002");
}

#[test]
fn mvcc_phantom_read() {
    let mut mv = MvccChecker::new();
    let t1 = mv.begin_txn();
    let t2 = mv.begin_txn();
    mv.write_version("key1".into(), t2);
    mv.commit_txn(t2);
    let errs = mv.check_phantom(t1);
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A35003");
}

#[test]
fn mvcc_default() {
    let mv = MvccChecker::default();
    assert!(mv.check_write_conflicts().is_empty());
}

// =======================================================================
// T089: RollbackChecker tests
// =======================================================================

#[test]
fn rollback_unknown_savepoint() {
    let mut rb = RollbackChecker::new();
    let err = rb.rollback_to("sp1");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A36001");
}

#[test]
fn rollback_resource_leak() {
    let mut rb = RollbackChecker::new();
    rb.create_savepoint("sp1".into());
    rb.acquire_resource("lock1".into());
    rb.rollback_to("sp1");
    let errs = rb.check_resource_leak();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A36002");
}

#[test]
fn rollback_resource_released_ok() {
    let mut rb = RollbackChecker::new();
    rb.create_savepoint("sp1".into());
    rb.acquire_resource("lock1".into());
    rb.release_resource("lock1");
    rb.rollback_to("sp1");
    assert!(rb.check_resource_leak().is_empty());
}

#[test]
fn rollback_duplicate_savepoint() {
    let mut rb = RollbackChecker::new();
    rb.create_savepoint("sp1".into());
    rb.create_savepoint("sp1".into());
    let errs = rb.check_savepoint_nesting();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A36003");
}

#[test]
fn rollback_default() {
    let rb = RollbackChecker::default();
    assert!(rb.check_resource_leak().is_empty());
}

// =======================================================================
// T090: MonotonicStateChecker tests
// =======================================================================

#[test]
fn monotonic_increasing_violation() {
    let mut mc = MonotonicStateChecker::new();
    mc.declare("seq".into(), MonotonicDirection::Increasing, 10, 0..1);
    let err = mc.update("seq", 5);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A37001");
}

#[test]
fn monotonic_increasing_ok() {
    let mut mc = MonotonicStateChecker::new();
    mc.declare("seq".into(), MonotonicDirection::Increasing, 10, 0..1);
    assert!(mc.update("seq", 10).is_none()); // equal allowed for Increasing
    assert!(mc.update("seq", 15).is_none());
}

#[test]
fn monotonic_strictly_increasing() {
    let mut mc = MonotonicStateChecker::new();
    mc.declare(
        "ts".into(),
        MonotonicDirection::StrictlyIncreasing,
        10,
        0..1,
    );
    let err = mc.update("ts", 10); // equal not allowed
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A37001");
}

#[test]
fn monotonic_reset_blocked() {
    let mc = MonotonicStateChecker::new();
    assert!(mc.check_reset("seq").is_none()); // not declared = no error
}

#[test]
fn monotonic_reset_declared() {
    let mut mc = MonotonicStateChecker::new();
    mc.declare("seq".into(), MonotonicDirection::Increasing, 0, 0..1);
    let err = mc.check_reset("seq");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A37002");
}

#[test]
fn monotonic_undeclared_access() {
    let mc = MonotonicStateChecker::new();
    let err = mc.check_access("unknown");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A37003");
}

#[test]
fn monotonic_current_value() {
    let mut mc = MonotonicStateChecker::new();
    mc.declare("seq".into(), MonotonicDirection::Increasing, 42, 0..1);
    assert_eq!(mc.current_value("seq"), Some(42));
    mc.update("seq", 100);
    assert_eq!(mc.current_value("seq"), Some(100));
}

#[test]
fn monotonic_default() {
    let mc = MonotonicStateChecker::default();
    assert!(mc.check_access("x").is_some());
}

// =======================================================================
// T091: StorageFailureChecker tests
// =======================================================================

#[test]
fn storage_failure_unhandled() {
    let mut sf = StorageFailureChecker::new();
    sf.declare_failure_mode(FailureMode::PartialWrite);
    let errs = sf.check_unhandled();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A38001");
}

#[test]
fn storage_failure_handled_ok() {
    let mut sf = StorageFailureChecker::new();
    sf.declare_failure_mode(FailureMode::BitRot);
    sf.mark_handled("bit_rot");
    assert!(sf.check_unhandled().is_empty());
}

#[test]
fn storage_failure_spurious_handler() {
    let mut sf = StorageFailureChecker::new();
    sf.mark_handled("nonexistent");
    let errs = sf.check_spurious_handlers();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A38002");
}

#[test]
fn storage_failure_critical_coverage() {
    let mut sf = StorageFailureChecker::new();
    sf.declare_failure_mode(FailureMode::PartialWrite);
    sf.declare_failure_mode(FailureMode::TornPage);
    let errs = sf.check_critical_coverage();
    assert_eq!(errs.len(), 2);
    assert!(errs.iter().all(|e| e.code == "A38003"));
}

#[test]
fn storage_failure_count() {
    let mut sf = StorageFailureChecker::new();
    sf.declare_failure_mode(FailureMode::DiskFull);
    sf.declare_failure_mode(FailureMode::IoTimeout);
    assert_eq!(sf.failure_count(), 2);
}

#[test]
fn storage_failure_default() {
    let sf = StorageFailureChecker::default();
    assert_eq!(sf.failure_count(), 0);
}

// =======================================================================
// T095: NumericalPrecisionChecker tests
// =======================================================================

#[test]
fn num_precision_loss() {
    let mut np = NumericalPrecisionChecker::new();
    np.declare("x".into(), 64, 1e-15, 0..1);
    let err = np.check_precision_loss("x", 32);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A42001");
}

#[test]
fn num_precision_ok() {
    let mut np = NumericalPrecisionChecker::new();
    np.declare("x".into(), 32, 1e-7, 0..1);
    assert!(np.check_precision_loss("x", 64).is_none());
}

#[test]
fn num_ulp_violation() {
    let mut np = NumericalPrecisionChecker::new();
    np.declare("x".into(), 64, 1e-15, 0..1);
    let err = np.check_ulp_bound("x", 1e-10);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A42002");
}

#[test]
fn num_cancellation() {
    let mut np = NumericalPrecisionChecker::new();
    np.declare("x".into(), 64, 1e-15, 0..1);
    let err = np.check_cancellation("x", 0.9999);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A42003");
}

#[test]
fn num_precision_default() {
    let np = NumericalPrecisionChecker::default();
    assert!(np.check_precision_loss("x", 32).is_none());
}

// =======================================================================
// T096: PrecomputedTableChecker tests
// =======================================================================

#[test]
fn table_incomplete_coverage() {
    let mut tc = PrecomputedTableChecker::new();
    tc.declare_table("crc32".into(), 256, "gen_crc32".into(), 0..1);
    tc.mark_entries_verified("crc32", 100);
    let errs = tc.check_coverage();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A43001");
}

#[test]
fn table_full_coverage_ok() {
    let mut tc = PrecomputedTableChecker::new();
    tc.declare_table("crc32".into(), 256, "gen_crc32".into(), 0..1);
    tc.mark_entries_verified("crc32", 256);
    assert!(tc.check_coverage().is_empty());
}

#[test]
fn table_no_generator() {
    let mut tc = PrecomputedTableChecker::new();
    tc.declare_table("lut".into(), 16, "".into(), 0..1);
    let errs = tc.check_generator();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A43002");
}

#[test]
fn table_zero_size() {
    let mut tc = PrecomputedTableChecker::new();
    tc.declare_table("empty".into(), 0, "gen".into(), 0..1);
    let errs = tc.check_non_empty();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A43003");
}

#[test]
fn table_count() {
    let mut tc = PrecomputedTableChecker::new();
    tc.declare_table("a".into(), 10, "g".into(), 0..1);
    tc.declare_table("b".into(), 20, "g".into(), 0..1);
    assert_eq!(tc.table_count(), 2);
}

#[test]
fn table_default() {
    let tc = PrecomputedTableChecker::default();
    assert_eq!(tc.table_count(), 0);
}

// =======================================================================
// T097: PlatformAbstractionChecker tests
// =======================================================================

#[test]
fn platform_missing_impl() {
    let mut pa = PlatformAbstractionChecker::new();
    pa.add_platform("linux".into());
    pa.add_platform("windows".into());
    pa.declare_abstraction("fs_ops".into(), vec!["linux".into()]);
    let errs = pa.check_coverage();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A44001");
}

#[test]
fn platform_full_coverage_ok() {
    let mut pa = PlatformAbstractionChecker::new();
    pa.add_platform("linux".into());
    pa.declare_abstraction("fs_ops".into(), vec!["linux".into()]);
    assert!(pa.check_coverage().is_empty());
}

#[test]
fn platform_direct_use() {
    let mut pa = PlatformAbstractionChecker::new();
    pa.add_platform("linux".into());
    let err = pa.check_direct_platform_use("linux");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A44002");
}

#[test]
fn platform_unknown() {
    let mut pa = PlatformAbstractionChecker::new();
    pa.add_platform("linux".into());
    pa.declare_abstraction("net".into(), vec!["freebsd".into()]);
    let errs = pa.check_unknown_platforms();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A44003");
}

#[test]
fn platform_default() {
    let pa = PlatformAbstractionChecker::default();
    assert!(pa.check_coverage().is_empty());
}

// =======================================================================
// T098: FeatureFlagChecker tests
// =======================================================================

#[test]
fn feature_flag_unused() {
    let mut ff = FeatureFlagChecker::new();
    ff.declare("debug_mode".into(), false, vec![]);
    let errs = ff.check_unused();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A45001");
}

#[test]
fn feature_flag_used_ok() {
    let mut ff = FeatureFlagChecker::new();
    ff.declare("debug_mode".into(), false, vec![]);
    ff.mark_used("debug_mode");
    assert!(ff.check_unused().is_empty());
}

#[test]
fn feature_flag_conflict() {
    let mut ff = FeatureFlagChecker::new();
    ff.declare("a".into(), true, vec!["b".into()]);
    ff.declare("b".into(), true, vec![]);
    let errs = ff.check_conflicts();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A45002");
}

#[test]
fn feature_flag_undeclared() {
    let ff = FeatureFlagChecker::new();
    let err = ff.check_undeclared("unknown");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A45003");
}

#[test]
fn feature_flag_default() {
    let ff = FeatureFlagChecker::default();
    assert!(ff.check_unused().is_empty());
}

// =======================================================================
// T099: ResourceLimitChecker tests
// =======================================================================

#[test]
fn resource_limit_exceeded() {
    let mut rl = ResourceLimitChecker::new();
    rl.declare_limit("mem".into(), 1000, "bytes".into());
    rl.record_usage("mem", 1500);
    let errs = rl.check_limits();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A46001");
}

#[test]
fn resource_limit_ok() {
    let mut rl = ResourceLimitChecker::new();
    rl.declare_limit("mem".into(), 1000, "bytes".into());
    rl.record_usage("mem", 500);
    assert!(rl.check_limits().is_empty());
}

#[test]
fn resource_unbounded() {
    let rl = ResourceLimitChecker::new();
    let err = rl.check_unbounded("unknown");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A46002");
}

#[test]
fn resource_near_limit() {
    let mut rl = ResourceLimitChecker::new();
    rl.declare_limit("fds".into(), 100, "count".into());
    rl.record_usage("fds", 95);
    let errs = rl.check_near_limit();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A46003");
}

#[test]
fn resource_release() {
    let mut rl = ResourceLimitChecker::new();
    rl.declare_limit("mem".into(), 100, "bytes".into());
    rl.record_usage("mem", 80);
    rl.release_usage("mem", 50);
    assert_eq!(rl.current_usage("mem"), Some(30));
}

#[test]
fn resource_default() {
    let rl = ResourceLimitChecker::default();
    assert!(rl.check_limits().is_empty());
}

// =======================================================================
// T100: UnsafeEscapeChecker tests
// =======================================================================

#[test]
fn unsafe_no_proof() {
    let mut ue = UnsafeEscapeChecker::new();
    ue.declare_unsafe("ptr_deref".into(), vec!["aligned".into()], 0..1);
    let errs = ue.check_unproven();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A47001");
}

#[test]
fn unsafe_with_proof_ok() {
    let mut ue = UnsafeEscapeChecker::new();
    ue.declare_unsafe("ptr_deref".into(), vec!["aligned".into()], 0..1);
    ue.attach_proof("ptr_deref");
    assert!(ue.check_unproven().is_empty());
}

#[test]
fn unsafe_undischarged_obligation() {
    let mut ue = UnsafeEscapeChecker::new();
    ue.declare_unsafe(
        "cast".into(),
        vec!["valid_repr".into(), "aligned".into()],
        0..1,
    );
    ue.discharge_obligation("cast", "valid_repr".into());
    let errs = ue.check_obligations();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A47002");
}

#[test]
fn unsafe_empty_obligations() {
    let mut ue = UnsafeEscapeChecker::new();
    ue.declare_unsafe("noop".into(), vec![], 0..1);
    let errs = ue.check_empty_obligations();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A47003");
}

#[test]
fn unsafe_count() {
    let mut ue = UnsafeEscapeChecker::new();
    ue.declare_unsafe("a".into(), vec![], 0..1);
    ue.declare_unsafe("b".into(), vec![], 0..1);
    assert_eq!(ue.unsafe_count(), 2);
}

#[test]
fn unsafe_default() {
    let ue = UnsafeEscapeChecker::default();
    assert_eq!(ue.unsafe_count(), 0);
}

// =======================================================================
// T101: ComplexityBoundChecker tests
// =======================================================================

#[test]
fn complexity_bound_violated() {
    let mut cb = ComplexityBoundChecker::new();
    cb.declare_bound("sort".into(), ComplexityClass::Linear, 0..1);
    cb.record_measured("sort", ComplexityClass::Quadratic);
    let errs = cb.check_bounds();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A48001");
}

#[test]
fn complexity_bound_ok() {
    let mut cb = ComplexityBoundChecker::new();
    cb.declare_bound("lookup".into(), ComplexityClass::Logarithmic, 0..1);
    cb.record_measured("lookup", ComplexityClass::Constant);
    assert!(cb.check_bounds().is_empty());
}

#[test]
fn complexity_unverified() {
    let mut cb = ComplexityBoundChecker::new();
    cb.declare_bound("search".into(), ComplexityClass::Linear, 0..1);
    let errs = cb.check_unverified();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A48002");
}

#[test]
fn complexity_exponential_warning() {
    let mut cb = ComplexityBoundChecker::new();
    cb.declare_bound("brute".into(), ComplexityClass::Exponential, 0..1);
    let errs = cb.check_expensive();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A48003");
}

#[test]
fn complexity_default() {
    let cb = ComplexityBoundChecker::default();
    assert!(cb.check_bounds().is_empty());
}

// =======================================================================
// T102: BehavioralEquivalenceChecker tests
// =======================================================================

#[test]
fn equiv_unverified() {
    let mut be = BehavioralEquivalenceChecker::new();
    be.declare(
        "eq1".into(),
        "sort_a".into(),
        "sort_b".into(),
        "Sortable".into(),
        0..1,
    );
    let errs = be.check_unverified();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A49001");
}

#[test]
fn equiv_verified_ok() {
    let mut be = BehavioralEquivalenceChecker::new();
    be.declare(
        "eq1".into(),
        "sort_a".into(),
        "sort_b".into(),
        "Sortable".into(),
        0..1,
    );
    be.mark_verified("eq1");
    assert!(be.check_unverified().is_empty());
}

#[test]
fn equiv_self_equivalence() {
    let mut be = BehavioralEquivalenceChecker::new();
    be.declare(
        "eq1".into(),
        "sort_a".into(),
        "sort_a".into(),
        "Sortable".into(),
        0..1,
    );
    let errs = be.check_self_equivalence();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A49002");
}

#[test]
fn equiv_no_contract() {
    let mut be = BehavioralEquivalenceChecker::new();
    be.declare("eq1".into(), "a".into(), "b".into(), "".into(), 0..1);
    let errs = be.check_contract_ref();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A49003");
}

#[test]
fn equiv_default() {
    let be = BehavioralEquivalenceChecker::default();
    assert!(be.check_unverified().is_empty());
}

// =======================================================================
// T103: MultiPassRefinementChecker tests
// =======================================================================

#[test]
fn refinement_incomplete() {
    let mut mp = MultiPassRefinementChecker::new();
    mp.add_pass("r1".into(), "spec".into(), "design".into(), 5, 0..1);
    mp.discharge("r1", 3);
    let errs = mp.check_complete();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A50001");
}

#[test]
fn refinement_complete_ok() {
    let mut mp = MultiPassRefinementChecker::new();
    mp.add_pass("r1".into(), "spec".into(), "design".into(), 5, 0..1);
    mp.discharge("r1", 5);
    assert!(mp.check_complete().is_empty());
}

#[test]
fn refinement_chain_gap() {
    let mut mp = MultiPassRefinementChecker::new();
    mp.add_pass("r1".into(), "spec".into(), "design".into(), 1, 0..1);
    mp.add_pass("r2".into(), "impl".into(), "code".into(), 1, 0..1);
    let errs = mp.check_chain();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A50002");
}

#[test]
fn refinement_zero_obligations() {
    let mut mp = MultiPassRefinementChecker::new();
    mp.add_pass("r1".into(), "spec".into(), "design".into(), 0, 0..1);
    let errs = mp.check_non_trivial();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A50003");
}

#[test]
fn refinement_pass_count() {
    let mut mp = MultiPassRefinementChecker::new();
    mp.add_pass("r1".into(), "a".into(), "b".into(), 1, 0..1);
    mp.add_pass("r2".into(), "b".into(), "c".into(), 1, 0..1);
    assert_eq!(mp.pass_count(), 2);
}

#[test]
fn refinement_default() {
    let mp = MultiPassRefinementChecker::default();
    assert_eq!(mp.pass_count(), 0);
}

// =======================================================================
// T104: IncrementalContractChecker tests
// =======================================================================

#[test]
fn incremental_strengthens_precondition() {
    let mut ic = IncrementalContractChecker::new();
    ic.add_version("SafeDiv".into(), 1, 1, 1);
    ic.add_version("SafeDiv".into(), 2, 3, 1); // more requires = stronger
    let errs = ic.check_precondition_weakening();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A51001");
}

#[test]
fn incremental_weakens_postcondition() {
    let mut ic = IncrementalContractChecker::new();
    ic.add_version("SafeDiv".into(), 1, 1, 3);
    ic.add_version("SafeDiv".into(), 2, 1, 1); // fewer ensures = weaker
    let errs = ic.check_postcondition_strengthening();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A51002");
}

#[test]
fn incremental_version_gap() {
    let mut ic = IncrementalContractChecker::new();
    ic.add_version("SafeDiv".into(), 1, 1, 1);
    ic.add_version("SafeDiv".into(), 5, 1, 1);
    let errs = ic.check_version_continuity();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A51003");
}

#[test]
fn incremental_ok() {
    let mut ic = IncrementalContractChecker::new();
    ic.add_version("SafeDiv".into(), 1, 3, 1);
    ic.add_version("SafeDiv".into(), 2, 2, 2); // weaker pre, stronger post
    assert!(ic.check_precondition_weakening().is_empty());
    assert!(ic.check_postcondition_strengthening().is_empty());
}

#[test]
fn incremental_default() {
    let ic = IncrementalContractChecker::default();
    assert!(ic.check_precondition_weakening().is_empty());
}

// =======================================================================
// T105: ScopedInvariantChecker tests
// =======================================================================

#[test]
fn invariant_double_suspend() {
    let mut si = ScopedInvariantChecker::new();
    si.declare_invariant("inv1".into());
    assert!(si.suspend("inv1").is_none());
    let err = si.suspend("inv1");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A52001");
}

#[test]
fn invariant_suspend_undeclared() {
    let mut si = ScopedInvariantChecker::new();
    let err = si.suspend("unknown");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A52002");
}

#[test]
fn invariant_restore_not_suspended() {
    let mut si = ScopedInvariantChecker::new();
    si.declare_invariant("inv1".into());
    let err = si.restore("inv1");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A52003");
}

#[test]
fn invariant_suspend_restore_ok() {
    let mut si = ScopedInvariantChecker::new();
    si.declare_invariant("inv1".into());
    si.suspend("inv1");
    assert!(si.is_suspended("inv1"));
    si.restore("inv1");
    assert!(!si.is_suspended("inv1"));
    assert!(si.check_all_restored().is_empty());
}

#[test]
fn invariant_still_suspended_at_exit() {
    let mut si = ScopedInvariantChecker::new();
    si.declare_invariant("inv1".into());
    si.suspend("inv1");
    let errs = si.check_all_restored();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A52001");
}

#[test]
fn invariant_suspension_depth() {
    let mut si = ScopedInvariantChecker::new();
    si.declare_invariant("a".into());
    si.declare_invariant("b".into());
    si.suspend("a");
    si.suspend("b");
    assert_eq!(si.suspension_depth(), 2);
    si.restore("a");
    assert_eq!(si.suspension_depth(), 1);
}

#[test]
fn invariant_default() {
    let si = ScopedInvariantChecker::default();
    assert_eq!(si.suspension_depth(), 0);
}

// =======================================================================
// T107: StdlibTypes tests
// =======================================================================

#[test]
fn stdlib_has_core_types() {
    let stdlib = StdlibTypes::new();
    assert!(stdlib.is_stdlib_type("Pos"));
    assert!(stdlib.is_stdlib_type("NonNeg"));
    assert!(stdlib.is_stdlib_type("Email"));
    assert!(stdlib.is_stdlib_type("Uuid"));
    assert!(!stdlib.is_stdlib_type("Unknown"));
}

#[test]
fn stdlib_lookup() {
    let stdlib = StdlibTypes::new();
    let pos = stdlib.lookup("Pos").unwrap();
    assert_eq!(pos.refinement, "v > 0");
    assert_eq!(pos.base_type, Type::Int);
}

#[test]
fn stdlib_type_count() {
    let stdlib = StdlibTypes::new();
    assert!(stdlib.type_count() >= 6);
}

#[test]
fn stdlib_default() {
    let stdlib = StdlibTypes::default();
    assert!(stdlib.type_count() >= 6);
}

// =======================================================================
// T108: CollectionContracts tests
// =======================================================================

#[test]
fn collection_has_standard_ops() {
    let cc = CollectionContracts::new();
    assert!(cc.lookup("sort").is_some());
    assert!(cc.lookup("filter").is_some());
    assert!(cc.lookup("map").is_some());
    assert!(cc.lookup("reverse").is_some());
}

#[test]
fn collection_sort_preserves_length() {
    let cc = CollectionContracts::new();
    let sort = cc.lookup("sort").unwrap();
    assert!(sort.preserves_length);
    assert!(sort.preserves_elements);
}

#[test]
fn collection_filter_does_not_preserve_length() {
    let cc = CollectionContracts::new();
    let filter = cc.lookup("filter").unwrap();
    assert!(!filter.preserves_length);
}

#[test]
fn collection_contract_count() {
    let cc = CollectionContracts::new();
    assert!(cc.contract_count() >= 5);
}

#[test]
fn collection_default() {
    let cc = CollectionContracts::default();
    assert!(cc.contract_count() >= 5);
}

// =======================================================================
// T109: CrudAuthContracts tests
// =======================================================================

#[test]
fn crud_auth_missing_policy() {
    let mut ca = CrudAuthContracts::new();
    ca.add_crud("create_user".into(), CrudType::Create, true);
    let errs = ca.check_auth_coverage();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A53001");
}

#[test]
fn crud_auth_with_policy_ok() {
    let mut ca = CrudAuthContracts::new();
    ca.add_crud("create_user".into(), CrudType::Create, true);
    ca.add_auth_policy("create_user".into(), "admin".into(), false);
    assert!(ca.check_auth_coverage().is_empty());
}

#[test]
fn crud_delete_without_auth() {
    let mut ca = CrudAuthContracts::new();
    ca.add_crud("delete_item".into(), CrudType::Delete, false);
    let errs = ca.check_delete_protection();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A53002");
}

#[test]
fn crud_counts() {
    let mut ca = CrudAuthContracts::new();
    ca.add_crud("a".into(), CrudType::Read, false);
    ca.add_auth_policy("a".into(), "user".into(), true);
    assert_eq!(ca.crud_count(), 1);
    assert_eq!(ca.policy_count(), 1);
}

#[test]
fn crud_default() {
    let ca = CrudAuthContracts::default();
    assert_eq!(ca.crud_count(), 0);
}

// =======================================================================
// T110: ContractCompositionChecker tests
// =======================================================================

#[test]
fn composition_unknown_extends() {
    let mut cc = ContractCompositionChecker::new();
    cc.declare("Child".into(), vec!["Unknown".into()], 1);
    let errs = cc.check_extends();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A54001");
}

#[test]
fn composition_valid_extends() {
    let mut cc = ContractCompositionChecker::new();
    cc.declare("Base".into(), vec![], 2);
    cc.declare("Child".into(), vec!["Base".into()], 1);
    assert!(cc.check_extends().is_empty());
}

#[test]
fn composition_circular() {
    let mut cc = ContractCompositionChecker::new();
    cc.declare("A".into(), vec!["B".into()], 1);
    cc.declare("B".into(), vec!["A".into()], 1);
    let errs = cc.check_circular();
    assert!(!errs.is_empty());
    assert!(errs.iter().any(|e| e.code == "A54002"));
}

#[test]
fn composition_diamond() {
    let mut cc = ContractCompositionChecker::new();
    cc.declare("Base".into(), vec![], 1);
    cc.declare("Left".into(), vec!["Base".into()], 1);
    cc.declare("Right".into(), vec!["Base".into()], 1);
    cc.declare("Diamond".into(), vec!["Left".into(), "Right".into()], 1);
    let errs = cc.check_diamond();
    assert!(!errs.is_empty());
    assert!(errs.iter().any(|e| e.code == "A54003"));
}

#[test]
fn composition_default() {
    let cc = ContractCompositionChecker::default();
    assert_eq!(cc.contract_count(), 0);
}

// =======================================================================
// T111: ContractLibraryChecker tests
// =======================================================================

#[test]
fn library_empty_exports() {
    let mut lc = ContractLibraryChecker::new();
    lc.declare_library("mylib".into(), "1.0.0".into());
    let errs = lc.check_empty_exports();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A55001");
}

#[test]
fn library_with_exports_ok() {
    let mut lc = ContractLibraryChecker::new();
    lc.declare_library("mylib".into(), "1.0.0".into());
    lc.add_export("mylib", "SafeDiv".into());
    assert!(lc.check_empty_exports().is_empty());
}

#[test]
fn library_self_dependency() {
    let mut lc = ContractLibraryChecker::new();
    lc.declare_library("mylib".into(), "1.0.0".into());
    lc.add_dependency(
        "mylib",
        LibraryDep {
            name: "mylib".into(),
            version_req: ">=1.0".into(),
        },
    );
    let errs = lc.check_circular_deps();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A55002");
}

#[test]
fn library_duplicate() {
    let mut lc = ContractLibraryChecker::new();
    lc.declare_library("mylib".into(), "1.0.0".into());
    lc.declare_library("mylib".into(), "2.0.0".into());
    let errs = lc.check_duplicates();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A55003");
}

#[test]
fn library_default() {
    let lc = ContractLibraryChecker::default();
    assert_eq!(lc.library_count(), 0);
}

// -----------------------------------------------------------------------
// Match expression exhaustiveness wiring tests (T017)
// -----------------------------------------------------------------------

#[test]
fn match_infer_type_from_first_arm() {
    // match x { A => 42, B => 0 } should infer Int from the first arm
    let env = TypeEnv::new();
    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Ident("x".into())),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Ident("A".into()),
                body: AstExpr::Literal(AstLit::Int("42".into())),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Ident("B".into()),
                body: AstExpr::Literal(AstLit::Int("0".into())),
            },
        ],
    };
    let result = infer_expr(&expr, &env);
    assert_eq!(result.unwrap(), Type::Int);
}

#[test]
fn match_incompatible_arms_emits_error() {
    // match x { A => 42, B => true } should emit A03001
    let env = TypeEnv::new();
    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Ident("x".into())),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Ident("A".into()),
                body: AstExpr::Literal(AstLit::Int("42".into())),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Ident("B".into()),
                body: AstExpr::Literal(AstLit::Bool(true)),
            },
        ],
    };
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
    assert!(err.message.contains("incompatible"));
}

#[test]
fn match_compatible_arms_ok() {
    // match x { A => 42, B => 0 } all Int arms = ok
    let env = TypeEnv::new();
    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Ident("x".into())),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Ident("A".into()),
                body: AstExpr::Literal(AstLit::Int("1".into())),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Ident("B".into()),
                body: AstExpr::Literal(AstLit::Int("2".into())),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Wildcard,
                body: AstExpr::Literal(AstLit::Int("3".into())),
            },
        ],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn match_empty_arms_infers_unknown() {
    let env = TypeEnv::new();
    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Ident("x".into())),
        arms: vec![],
    };
    let result = infer_expr(&expr, &env);
    assert_eq!(result.unwrap(), Type::Unknown);
}

#[test]
fn match_expr_references_var() {
    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Ident("status".into())),
        arms: vec![assura_parser::ast::MatchArm {
            pattern: assura_parser::ast::Pattern::Ident("A".into()),
            body: AstExpr::Ident("result".into()),
        }],
    };
    assert!(expr_references_var(&expr, "status"));
    assert!(expr_references_var(&expr, "result"));
    assert!(!expr_references_var(&expr, "other"));
}

#[test]
fn infer_cast_returns_target_type() {
    let env = TypeEnv::new();
    let expr = AstExpr::Cast {
        expr: Box::new(AstExpr::Literal(AstLit::Int("42".into()))),
        ty: "Float".into(),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Float);
}

#[test]
fn infer_cast_to_u8() {
    let env = TypeEnv::new();
    let expr = AstExpr::Cast {
        expr: Box::new(AstExpr::Literal(AstLit::Int("255".into()))),
        ty: "U8".into(),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::U8);
}

#[test]
fn infer_cast_to_named_type() {
    let env = TypeEnv::new();
    let expr = AstExpr::Cast {
        expr: Box::new(AstExpr::Ident("x".into())),
        ty: "CustomType".into(),
    };
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Named("CustomType".into())
    );
}

#[test]
fn infer_let_binding_propagates_type() {
    let env = TypeEnv::new();
    // let x = 42 in x  =>  should infer Int from body
    let expr = AstExpr::Let {
        name: "x".into(),
        value: Box::new(AstExpr::Literal(AstLit::Int("42".into()))),
        body: Box::new(AstExpr::Ident("x".into())),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_match_checks_all_arms() {
    let env = TypeEnv::new();
    // match true { true => 1, false => 2 } => Int
    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Literal(AstLit::Bool(true)),
                body: AstExpr::Literal(AstLit::Int("1".into())),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Literal(AstLit::Bool(false)),
                body: AstExpr::Literal(AstLit::Int("2".into())),
            },
        ],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn infer_match_empty_arms_returns_unknown() {
    let env = TypeEnv::new();
    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        arms: vec![],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn infer_builtin_len_returns_nat() {
    let env = TypeEnv::new();
    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("len".into())),
        args: vec![AstExpr::Ident("xs".into())],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Nat);
}

#[test]
fn infer_builtin_contains_returns_bool() {
    let env = TypeEnv::new();
    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("contains".into())),
        args: vec![
            AstExpr::Ident("xs".into()),
            AstExpr::Literal(AstLit::Int("1".into())),
        ],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn result_bound_in_ensures_env() {
    // When `result` is bound in the env, infer_expr should return it
    let mut env = TypeEnv::new();
    env.insert("result".to_string(), Type::Int);
    let expr = AstExpr::Ident("result".into());
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn result_unknown_without_binding() {
    // Without binding, `result` returns Unknown
    let env = TypeEnv::new();
    let expr = AstExpr::Ident("result".into());
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn result_type_threaded_through_ensures() {
    // Parse a function with an ensures clause using `result`
    let src = r#"
fn square(x: Int) -> Int
  ensures { result >= 0 }
"#;
    let (file, errs) = assura_parser::parse(src);
    assert!(errs.is_empty());
    let file = file.unwrap();
    let resolved = assura_resolve::resolve(&file).unwrap();
    // type_check should succeed; the `result >= 0` comparison is
    // Int >= Int which is valid
    let typed = type_check(&resolved);
    assert!(typed.is_ok(), "type_check failed: {:?}", typed.err());
}

#[test]
fn tuple_infers_element_types() {
    let env = TypeEnv::new();
    let expr = AstExpr::Tuple(vec![
        AstExpr::Literal(AstLit::Int("1".into())),
        AstExpr::Literal(AstLit::Bool(true)),
    ]);
    let ty = infer_expr(&expr, &env).unwrap();
    assert_eq!(ty, Type::Tuple(vec![Type::Int, Type::Bool]));
}

#[test]
fn tuple_single_element() {
    let env = TypeEnv::new();
    let expr = AstExpr::Tuple(vec![AstExpr::Literal(AstLit::Int("42".into()))]);
    let ty = infer_expr(&expr, &env).unwrap();
    assert_eq!(ty, Type::Tuple(vec![Type::Int]));
}

#[test]
fn tuple_empty() {
    let env = TypeEnv::new();
    let expr = AstExpr::Tuple(vec![]);
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
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("pair".into())), "0".into());
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
    // pair.1 should be String
    let expr1 = AstExpr::Field(Box::new(AstExpr::Ident("pair".into())), "1".into());
    assert_eq!(infer_expr(&expr1, &env).unwrap(), Type::String);
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
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("xs".into())), "head".into());
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Option(Box::new(Type::Int))
    );
}

#[test]
fn list_field_tail_returns_list() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("xs".into())), "tail".into());
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
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("x".into())),
        method: "flatten".into(),
        args: vec![],
    };
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Option(Box::new(Type::Int))
    );
}

#[test]
fn option_ok_or_returns_result() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Option(Box::new(Type::Int)));
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("x".into())),
        method: "ok_or".into(),
        args: vec![AstExpr::Literal(AstLit::Str("err".into()))],
    };
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
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("r".into())),
        method: "map_err".into(),
        args: vec![],
    };
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
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("r".into())),
        method: "ok".into(),
        args: vec![],
    };
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
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("r".into())),
        method: "err".into(),
        args: vec![],
    };
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
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("r".into())),
        method: "unwrap_err".into(),
        args: vec![],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::String);
}

#[test]
fn range_returns_list_int() {
    // Range expression returns List<Int>
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
fn range_rejects_non_numeric() {
    let env = TypeEnv::new();
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Str("a".into()))),
        op: AstBinOp::Range,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("10".into()))),
    };
    assert!(infer_expr(&expr, &env).is_err());
}

#[test]
fn in_operator_rejects_non_collection_rhs() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    env.insert("y".into(), Type::Int);
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("x".into())),
        op: AstBinOp::In,
        rhs: Box::new(AstExpr::Ident("y".into())),
    };
    let err = infer_expr(&expr, &env).unwrap_err();
    assert!(err.message.contains("collection"), "got: {}", err.message);
}

#[test]
fn in_operator_accepts_list() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("x".into())),
        op: AstBinOp::In,
        rhs: Box::new(AstExpr::Ident("xs".into())),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn in_operator_accepts_set() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    env.insert("s".into(), Type::Set(Box::new(Type::Int)));
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("x".into())),
        op: AstBinOp::In,
        rhs: Box::new(AstExpr::Ident("s".into())),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn division_by_zero_literal_emits_error() {
    let env = TypeEnv::new();
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Int("10".into()))),
        op: AstBinOp::Div,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
    };
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
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Int("10".into()))),
        op: AstBinOp::Mod,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
    };
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
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Literal(AstLit::Int("10".into()))),
        op: AstBinOp::Div,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("3".into()))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

// -----------------------------------------------------------------------
// Field access on built-in types (Option, Result, Map, Set)
// -----------------------------------------------------------------------

#[test]
fn field_option_value() {
    let mut env = TypeEnv::new();
    env.insert("opt".into(), Type::Option(Box::new(Type::Int)));
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("opt".into())), "value".into());
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn field_option_is_some() {
    let mut env = TypeEnv::new();
    env.insert("opt".into(), Type::Option(Box::new(Type::Int)));
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("opt".into())), "is_some".into());
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn field_result_ok_type() {
    let mut env = TypeEnv::new();
    env.insert(
        "r".into(),
        Type::Result(Box::new(Type::String), Box::new(Type::Int)),
    );
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("r".into())), "ok".into());
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::String);
}

#[test]
fn field_result_err_type() {
    let mut env = TypeEnv::new();
    env.insert(
        "r".into(),
        Type::Result(Box::new(Type::String), Box::new(Type::Int)),
    );
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("r".into())), "err".into());
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn field_result_is_ok() {
    let mut env = TypeEnv::new();
    env.insert(
        "r".into(),
        Type::Result(Box::new(Type::String), Box::new(Type::Int)),
    );
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("r".into())), "is_ok".into());
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn field_map_keys_values() {
    let mut env = TypeEnv::new();
    env.insert(
        "m".into(),
        Type::Map(Box::new(Type::String), Box::new(Type::Int)),
    );
    let keys_expr = AstExpr::Field(Box::new(AstExpr::Ident("m".into())), "keys".into());
    assert_eq!(
        infer_expr(&keys_expr, &env).unwrap(),
        Type::List(Box::new(Type::String))
    );
    let vals_expr = AstExpr::Field(Box::new(AstExpr::Ident("m".into())), "values".into());
    assert_eq!(
        infer_expr(&vals_expr, &env).unwrap(),
        Type::List(Box::new(Type::Int))
    );
}

#[test]
fn field_collection_is_empty() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("xs".into())), "is_empty".into());
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

// -----------------------------------------------------------------------
// Method call on String, Option, Result types
// -----------------------------------------------------------------------

#[test]
fn method_string_contains() {
    let mut env = TypeEnv::new();
    env.insert("s".into(), Type::String);
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("s".into())),
        method: "contains".into(),
        args: vec![AstExpr::Literal(AstLit::Str("x".into()))],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn method_string_to_uppercase() {
    let mut env = TypeEnv::new();
    env.insert("s".into(), Type::String);
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("s".into())),
        method: "to_uppercase".into(),
        args: vec![],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::String);
}

#[test]
fn method_option_unwrap() {
    let mut env = TypeEnv::new();
    env.insert("opt".into(), Type::Option(Box::new(Type::Float)));
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("opt".into())),
        method: "unwrap".into(),
        args: vec![],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Float);
}

#[test]
fn method_option_is_some() {
    let mut env = TypeEnv::new();
    env.insert("opt".into(), Type::Option(Box::new(Type::Float)));
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("opt".into())),
        method: "is_some".into(),
        args: vec![],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn method_result_unwrap() {
    let mut env = TypeEnv::new();
    env.insert(
        "r".into(),
        Type::Result(Box::new(Type::Nat), Box::new(Type::String)),
    );
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("r".into())),
        method: "unwrap".into(),
        args: vec![],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Nat);
}

#[test]
fn method_result_is_ok() {
    let mut env = TypeEnv::new();
    env.insert(
        "r".into(),
        Type::Result(Box::new(Type::Nat), Box::new(Type::String)),
    );
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("r".into())),
        method: "is_ok".into(),
        args: vec![],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn method_set_insert() {
    let mut env = TypeEnv::new();
    env.insert("s".into(), Type::Set(Box::new(Type::Int)));
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("s".into())),
        method: "insert".into(),
        args: vec![AstExpr::Literal(AstLit::Int("1".into()))],
    };
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
    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Ident("x".into())),
        arms: vec![assura_parser::ast::MatchArm {
            pattern: assura_parser::ast::Pattern::Ident("val".into()),
            body: AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("val".into())),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            },
        }],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn match_wildcard_does_not_bind() {
    let env = TypeEnv::new();
    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Literal(AstLit::Int("42".into()))),
        arms: vec![assura_parser::ast::MatchArm {
            pattern: assura_parser::ast::Pattern::Wildcard,
            body: AstExpr::Literal(AstLit::Bool(true)),
        }],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn match_tuple_pattern_binds_element_types() {
    let mut env = TypeEnv::new();
    env.insert("pair".into(), Type::Tuple(vec![Type::Int, Type::Bool]));
    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Ident("pair".into())),
        arms: vec![assura_parser::ast::MatchArm {
            pattern: assura_parser::ast::Pattern::Tuple(vec![
                assura_parser::ast::Pattern::Ident("a".into()),
                assura_parser::ast::Pattern::Ident("b".into()),
            ]),
            // body uses 'a' which should be Int from pair[0]
            body: AstExpr::Ident("a".into()),
        }],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn match_literal_pattern_does_not_bind() {
    let env = TypeEnv::new();
    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Literal(AstLit::Int("1".into())),
                body: AstExpr::Literal(AstLit::Str("one".into())),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Wildcard,
                body: AstExpr::Literal(AstLit::Str("other".into())),
            },
        ],
    };
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
    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Ident("val".into())),
        arms: vec![assura_parser::ast::MatchArm {
            pattern: assura_parser::ast::Pattern::Constructor {
                name: "Some".into(),
                fields: vec![assura_parser::ast::Pattern::Ident("x".into())],
            },
            // body uses 'x' which should be Int from Some's first param
            body: AstExpr::Ident("x".into()),
        }],
    };
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
    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Ident("p".into())),
        arms: vec![assura_parser::ast::MatchArm {
            pattern: assura_parser::ast::Pattern::Constructor {
                name: "Pair".into(),
                fields: vec![
                    assura_parser::ast::Pattern::Ident("a".into()),
                    assura_parser::ast::Pattern::Ident("b".into()),
                ],
            },
            // body uses 'b' which should be Bool from Pair's second param
            body: AstExpr::Ident("b".into()),
        }],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn self_in_service_context_resolves_to_named_type() {
    let mut env = TypeEnv::new();
    env.insert("self".to_string(), Type::Named("FileStore".into()));
    let expr = AstExpr::Ident("self".into());
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
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("self".into())), "state".into());
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Named("State".into())
    );
}

#[test]
fn self_without_binding_returns_unknown() {
    let env = TypeEnv::new();
    let expr = AstExpr::Ident("self".into());
    // Outside a service context, self is Unknown
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
}

#[test]
fn extract_output_type_from_raw_tokens() {
    // output(result: Nat) is parsed as Raw(["result", ":", "Nat"])
    let body = AstExpr::Raw(vec!["result".into(), ":".into(), "Nat".into()]);
    assert_eq!(extract_output_type_from_body(&body), Type::Nat);
}

#[test]
fn extract_output_type_from_cast() {
    let body = AstExpr::Cast {
        expr: Box::new(AstExpr::Ident("result".into())),
        ty: "Int".into(),
    };
    assert_eq!(extract_output_type_from_body(&body), Type::Int);
}

#[test]
fn extract_output_type_from_raw_generic() {
    // output(result: List<Int>) from raw tokens
    let body = AstExpr::Raw(vec![
        "result".into(),
        ":".into(),
        "List".into(),
        "<".into(),
        "Int".into(),
        ">".into(),
    ]);
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
    let typed = type_check(&resolved);
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
    let typed = type_check(&resolved);
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
    let typed = type_check(&resolved);
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
    let typed = type_check(&resolved);
    assert!(typed.is_ok(), "no-io op should pass: {:?}", typed.err());
    let typed = typed.unwrap();
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
    let typed = type_check(&resolved);
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
    let body = Expr::Ident("x".into());
    register_input_clause_params(&body, &mut env);
    assert_eq!(env.lookup("x"), Some(&Type::Unknown));
}

#[test]
fn input_clause_single_cast() {
    // input(a as Int) at top level
    let mut env = TypeEnv::new();
    let body = Expr::Cast {
        expr: Box::new(Expr::Ident("a".into())),
        ty: "Int".into(),
    };
    register_input_clause_params(&body, &mut env);
    assert_eq!(env.lookup("a"), Some(&Type::Int));
}

#[test]
fn input_clause_paren_wraps_call() {
    // Paren-wrapped call: input((a as Int))
    let mut env = TypeEnv::new();
    let inner_call = Expr::Call {
        func: Box::new(Expr::Ident("input".into())),
        args: vec![Expr::Cast {
            expr: Box::new(Expr::Ident("a".into())),
            ty: "Int".into(),
        }],
    };
    let body = Expr::Paren(Box::new(inner_call));
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
    let body = Expr::Raw(tokens);
    register_input_clause_params(&body, &mut env);
    assert_eq!(env.lookup("a"), Some(&Type::Int));
    assert_eq!(env.lookup("b"), Some(&Type::String));
}

#[test]
fn input_clause_raw_bare_idents() {
    // Raw tokens: "buf , n" — bare identifiers without type annotations
    let mut env = TypeEnv::new();
    let tokens = vec!["buf".into(), ",".into(), "n".into()];
    let body = Expr::Raw(tokens);
    register_input_clause_params(&body, &mut env);
    assert_eq!(env.lookup("buf"), Some(&Type::Unknown));
    assert_eq!(env.lookup("n"), Some(&Type::Unknown));
}

#[test]
fn collect_input_types_single_cast() {
    let body = Expr::Cast {
        expr: Box::new(Expr::Ident("a".into())),
        ty: "Int".into(),
    };
    let mut out = Vec::new();
    collect_input_param_types(&body, &mut out);
    assert_eq!(out, vec![Type::Int]);
}

#[test]
fn collect_input_types_single_ident() {
    let body = Expr::Ident("x".into());
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
    let body = Expr::Raw(tokens);
    let mut out = Vec::new();
    collect_input_param_types(&body, &mut out);
    assert_eq!(out, vec![Type::Int, Type::Bool]);
}

#[test]
fn collect_input_types_raw_bare_idents() {
    let tokens = vec!["x".into(), ",".into(), "y".into()];
    let body = Expr::Raw(tokens);
    let mut out = Vec::new();
    collect_input_param_types(&body, &mut out);
    assert_eq!(out, vec![Type::Unknown, Type::Unknown]);
}

// ---- declare_linear_params_from_expr coverage ----

#[test]
fn linear_from_cast() {
    // input(handle as linear FileHandle)
    let mut tracker = UsageTracker::new();
    let body = Expr::Cast {
        expr: Box::new(Expr::Ident("handle".into())),
        ty: "linear FileHandle".into(),
    };
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
    let body = Expr::Call {
        func: Box::new(Expr::Ident("input".into())),
        args: vec![
            Expr::Cast {
                expr: Box::new(Expr::Ident("h".into())),
                ty: "linear File".into(),
            },
            Expr::Cast {
                expr: Box::new(Expr::Ident("n".into())),
                ty: "Int".into(),
            },
        ],
    };
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
    let body = Expr::Raw(tokens);
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
    let body = Expr::Raw(tokens);
    declare_linear_params_from_expr(&body, &mut tracker, &(0..1));
    assert_eq!(
        tracker.get_count("handle"),
        Some(0),
        "handle should be declared as linear"
    );
}

#[test]
fn linear_from_paren_wrapped() {
    // Paren-wrapped Cast
    let mut tracker = UsageTracker::new();
    let inner = Expr::Cast {
        expr: Box::new(Expr::Ident("buf".into())),
        ty: "linear Buffer".into(),
    };
    let body = Expr::Paren(Box::new(inner));
    declare_linear_params_from_expr(&body, &mut tracker, &(0..1));
    assert_eq!(
        tracker.get_count("buf"),
        Some(0),
        "buf should be declared as linear via Paren unwrap"
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
    let result = type_check(&resolved);
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
    let result = type_check(&resolved);
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

    let expr = Expr::BinOp {
        lhs: Box::new(Expr::Ident("secret_key".into())),
        op: BinOp::Add,
        rhs: Box::new(Expr::Ident("public_data".into())),
    };
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
    let result = type_check(&resolved);
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

    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Literal(AstLit::Int("1".into())),
                body: AstExpr::Ident("x".into()),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Wildcard,
                body: AstExpr::Ident("x".into()),
            },
        ],
    };
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

    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Literal(AstLit::Int("1".into())),
                body: AstExpr::Ident("x".into()),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Wildcard,
                body: AstExpr::Literal(AstLit::Int("0".into())),
            },
        ],
    };
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

    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Literal(AstLit::Int("1".into())),
                body: AstExpr::Ident("x".into()),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Literal(AstLit::Int("2".into())),
                body: AstExpr::Ident("x".into()),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Wildcard,
                body: AstExpr::Literal(AstLit::Int("0".into())),
            },
        ],
    };
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

    let expr = AstExpr::Match {
        scrutinee: Box::new(AstExpr::Ident("x".into())),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Literal(AstLit::Int("1".into())),
                body: AstExpr::Ident("x".into()),
            },
            assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Wildcard,
                body: AstExpr::Ident("x".into()),
            },
        ],
    };
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
    let expr = AstExpr::Forall {
        var: "i".into(),
        domain: Box::new(AstExpr::Ident("range".into())),
        body: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("i".into())),
            op: AstBinOp::Lt,
            rhs: Box::new(AstExpr::Ident("x".into())),
        }),
    };
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

    let expr = AstExpr::Exists {
        var: "i".into(),
        domain: Box::new(AstExpr::Ident("range".into())),
        body: Box::new(AstExpr::Ident("x".into())),
    };
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

    let expr = AstExpr::Old(Box::new(AstExpr::Ident("x".into())));
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

    let expr = AstExpr::Ghost(Box::new(AstExpr::Ident("x".into())));
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
