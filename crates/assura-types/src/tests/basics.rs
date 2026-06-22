use super::*;
#[test]
fn empty_file_type_checks() {
    let resolved = resolve_ok("");
    let typed = type_check(&resolved).expect("type_check should succeed");
    // Should have at least the built-in types in the environment.
    assert!(!typed.type_env.is_empty());
}

#[test]
fn expr_span() {
    // Expression type errors should carry the span from the expr (not just the decl).
    // Exercises precise sub-expression spans from 11.04 + follow-up.
    let src = r#"
contract BadExpr {
    requires { 1 + true }
}
"#;
    let resolved = resolve_ok(src);
    let res = type_check(&resolved);
    assert!(res.is_err(), "expected type error");
    let errs = res.unwrap_err();
    // Find a relevant type error (A03001 or containing "numeric" or "Bool")
    let err = errs
        .iter()
        .find(|e| e.code == "A03001" || e.message.contains("numeric") || e.message.contains("Bool"))
        .expect("expected a type mismatch error from the bad subexpr");
    assert!(
        err.span != (0..0),
        "error span must be real (not no_span 0..0), got {:?}",
        err.span
    );
    let decl_span = resolved.source.decls[0].span.clone();
    assert!(err.span != decl_span, "should not be the whole decl span");
    // Tight relative to decl (subexpr precision)
    assert!(
        err.span.len() < decl_span.len() / 2 && err.span.len() > 0,
        "span should be tighter than decl, got {:?} vs decl {:?}",
        err.span,
        decl_span
    );
    // Per #333: the error for bad subexpr (the "true") should carry a span
    // contained within (or tightly around) the offending sub-expression text.
    let true_start = src.find("true").expect("source must contain 'true'");
    let true_end = true_start + 4;
    let overlaps_bad = err.span.start < true_end && err.span.end > true_start;
    assert!(
        overlaps_bad,
        "error span {:?} should overlap the bad subexpr 'true' at {}..{}",
        err.span, true_start, true_end
    );
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
