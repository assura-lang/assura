use super::*;
use assura_parser::ast::{BinOp, Expr, Literal, SpExpr, Spanned, UnaryOp};

fn mk_int(s: &str) -> SpExpr {
    Spanned::no_span(Expr::Literal(Literal::Int(s.into())))
}

fn mk_float(s: &str) -> SpExpr {
    Spanned::no_span(Expr::Literal(Literal::Float(s.into())))
}

fn mk_bool(b: bool) -> SpExpr {
    Spanned::no_span(Expr::Literal(Literal::Bool(b)))
}

fn mk_str(s: &str) -> SpExpr {
    Spanned::no_span(Expr::Literal(Literal::Str(s.into())))
}

fn mk_ident(s: &str) -> SpExpr {
    Spanned::no_span(Expr::Ident(s.into()))
}

fn mk_binop(lhs: SpExpr, op: BinOp, rhs: SpExpr) -> SpExpr {
    Spanned::no_span(Expr::BinOp {
        lhs: Box::new(lhs),
        op,
        rhs: Box::new(rhs),
    })
}

// --- Literal inference ---

#[test]
fn literal_int() {
    assert_eq!(
        infer_expr(&mk_int("42"), &TypeEnv::new()).unwrap(),
        Type::Int
    );
}

#[test]
fn literal_float() {
    assert_eq!(
        infer_expr(&mk_float("3.14"), &TypeEnv::new()).unwrap(),
        Type::Float
    );
}

#[test]
fn literal_bool() {
    assert_eq!(
        infer_expr(&mk_bool(true), &TypeEnv::new()).unwrap(),
        Type::Bool
    );
}

#[test]
fn literal_string() {
    assert_eq!(
        infer_expr(&mk_str("hello"), &TypeEnv::new()).unwrap(),
        Type::String
    );
}

// --- Identifier inference ---

#[test]
fn ident_known() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    assert_eq!(infer_expr(&mk_ident("x"), &env).unwrap(), Type::Int);
}

#[test]
fn ident_unknown_returns_unknown() {
    assert_eq!(
        infer_expr(&mk_ident("missing"), &TypeEnv::new()).unwrap(),
        Type::Unknown
    );
}

#[test]
fn ident_true_is_bool() {
    assert_eq!(
        infer_expr(&mk_ident("true"), &TypeEnv::new()).unwrap(),
        Type::Bool
    );
}

#[test]
fn ident_false_is_bool() {
    assert_eq!(
        infer_expr(&mk_ident("false"), &TypeEnv::new()).unwrap(),
        Type::Bool
    );
}

// --- Binary operator inference ---

#[test]
fn binop_add_int() {
    let expr = mk_binop(mk_int("1"), BinOp::Add, mk_int("2"));
    assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Int);
}

#[test]
fn binop_add_float() {
    let expr = mk_binop(mk_float("1.0"), BinOp::Add, mk_float("2.0"));
    assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Float);
}

#[test]
fn binop_comparison_returns_bool() {
    let expr = mk_binop(mk_int("1"), BinOp::Gt, mk_int("2"));
    assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Bool);
}

#[test]
fn binop_equality_returns_bool() {
    let expr = mk_binop(mk_int("1"), BinOp::Eq, mk_int("2"));
    assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Bool);
}

#[test]
fn binop_and_returns_bool() {
    let expr = mk_binop(mk_bool(true), BinOp::And, mk_bool(false));
    assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Bool);
}

#[test]
fn binop_or_returns_bool() {
    let expr = mk_binop(mk_bool(true), BinOp::Or, mk_bool(false));
    assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Bool);
}

#[test]
fn binop_type_mismatch_a03001() {
    let expr = mk_binop(mk_int("1"), BinOp::Add, mk_str("hello"));
    let err = infer_expr(&expr, &TypeEnv::new()).unwrap_err();
    assert_eq!(err.code, "A03001");
}

// --- Unary operator inference ---

#[test]
fn unary_neg_int() {
    let expr = Spanned::no_span(Expr::UnaryOp {
        op: UnaryOp::Neg,
        expr: Box::new(mk_int("5")),
    });
    assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Int);
}

#[test]
fn unary_not_bool() {
    let expr = Spanned::no_span(Expr::UnaryOp {
        op: UnaryOp::Not,
        expr: Box::new(mk_bool(true)),
    });
    assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Bool);
}

#[test]
fn unary_neg_string_error() {
    let expr = Spanned::no_span(Expr::UnaryOp {
        op: UnaryOp::Neg,
        expr: Box::new(mk_str("hello")),
    });
    let err = infer_expr(&expr, &TypeEnv::new()).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn unary_not_int_error() {
    let expr = Spanned::no_span(Expr::UnaryOp {
        op: UnaryOp::Not,
        expr: Box::new(mk_int("5")),
    });
    let err = infer_expr(&expr, &TypeEnv::new()).unwrap_err();
    assert_eq!(err.code, "A03001");
}

// --- Paren ---

#[test]
fn int_literal_type() {
    let expr = mk_int("1");
    assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Int);
}

// --- If-then-else ---

#[test]
fn if_then_else_matching_branches() {
    let expr = Spanned::no_span(Expr::If {
        cond: Box::new(mk_bool(true)),
        then_branch: Box::new(mk_int("1")),
        else_branch: Some(Box::new(mk_int("2"))),
    });
    assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Int);
}

// --- Helper functions ---

#[test]
fn is_literal_zero_int_zero() {
    assert!(is_literal_zero(&mk_int("0")));
}

#[test]
fn is_literal_zero_int_nonzero() {
    assert!(!is_literal_zero(&mk_int("1")));
}

#[test]
fn is_literal_zero_float_zero() {
    assert!(is_literal_zero(&mk_float("0.0")));
}

#[test]
fn is_numeric_basic_types() {
    assert!(is_numeric(&Type::Int));
    assert!(is_numeric(&Type::Nat));
    assert!(is_numeric(&Type::Float));
    assert!(is_numeric(&Type::U32));
    assert!(is_numeric(&Type::I64));
}

#[test]
fn is_numeric_non_numeric() {
    assert!(!is_numeric(&Type::Bool));
    assert!(!is_numeric(&Type::String));
    assert!(!is_numeric(&Type::Unit));
}

#[test]
fn is_numeric_refined_base() {
    let ty = Type::Refined {
        base: Box::new(Type::Int),
        predicate: "x > 0".into(),
    };
    assert!(is_numeric(&ty));
}

#[test]
fn element_type_of_list() {
    assert_eq!(element_type_of(&Type::List(Box::new(Type::Int))), Type::Int);
}

#[test]
fn element_type_of_set() {
    assert_eq!(
        element_type_of(&Type::Set(Box::new(Type::String))),
        Type::String
    );
}

#[test]
fn element_type_of_map_returns_key() {
    assert_eq!(
        element_type_of(&Type::Map(Box::new(Type::String), Box::new(Type::Int))),
        Type::String
    );
}

#[test]
fn element_type_of_int_range() {
    assert_eq!(element_type_of(&Type::Int), Type::Int);
}

#[test]
fn element_type_of_unknown() {
    assert_eq!(element_type_of(&Type::Bool), Type::Unknown);
}

// --- Method call type inference ---

#[test]
fn method_call_len_on_list() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(mk_ident("xs")),
        method: "len".into(),
        args: vec![],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Nat);
}

#[test]
fn method_call_contains_on_list() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(mk_ident("xs")),
        method: "contains".into(),
        args: vec![mk_int("1")],
    });
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

#[test]
fn method_call_on_unknown_returns_unknown() {
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(mk_ident("unknown_var")),
        method: "foo".into(),
        args: vec![],
    });
    assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Unknown);
}

#[test]
fn method_call_pop_on_list() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(mk_ident("xs")),
        method: "pop".into(),
        args: vec![],
    });
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Option(Box::new(Type::Int))
    );
}

#[test]
fn method_call_fold_on_list() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(mk_ident("xs")),
        method: "fold".into(),
        args: vec![mk_int("0")],
    });
    // fold with int accumulator returns Int
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
}

#[test]
fn method_call_find_on_list() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::String)));
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(mk_ident("xs")),
        method: "find".into(),
        args: vec![],
    });
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Option(Box::new(Type::String))
    );
}

#[test]
fn method_call_enumerate_on_list() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(mk_ident("xs")),
        method: "enumerate".into(),
        args: vec![],
    });
    let result = infer_expr(&expr, &env).unwrap();
    assert!(
        matches!(&result, Type::List(inner) if matches!(inner.as_ref(), Type::Tuple(elems) if elems.len() == 2)),
        "expected List<(Nat, Int)>, got: {result:?}"
    );
}

#[test]
fn method_call_entries_on_map() {
    let mut env = TypeEnv::new();
    env.insert(
        "m".into(),
        Type::Map(Box::new(Type::String), Box::new(Type::Int)),
    );
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(mk_ident("m")),
        method: "entries".into(),
        args: vec![],
    });
    let result = infer_expr(&expr, &env).unwrap();
    assert!(
        matches!(&result, Type::List(inner) if matches!(inner.as_ref(), Type::Tuple(elems) if elems.len() == 2)),
        "expected List<(String, Int)>, got: {result:?}"
    );
}

#[test]
fn method_call_to_list_on_set() {
    let mut env = TypeEnv::new();
    env.insert("s".into(), Type::Set(Box::new(Type::Int)));
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(mk_ident("s")),
        method: "to_list".into(),
        args: vec![],
    });
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::List(Box::new(Type::Int))
    );
}

#[test]
fn method_call_position_on_list() {
    let mut env = TypeEnv::new();
    env.insert("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(mk_ident("xs")),
        method: "position".into(),
        args: vec![],
    });
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Option(Box::new(Type::Nat))
    );
}
