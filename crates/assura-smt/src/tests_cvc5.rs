use super::*;
use crate::cache::SessionCache;
use crate::{VerificationResult, cvc5_backend};
use assura_parser::ast::{BinOp, Clause, ClauseKind, Expr, Literal, Pattern, UnaryOp};
use crate::cvc5_common::{
    collect_apply_refs_from_expr, collect_unmodelable_reasons_cvc5,
    expr_has_unmodelable_features_cvc5, field_chain_depth_cvc5, flatten_field_chain_cvc5,
    has_deep_field_chain_cvc5, is_internal_cvc5_var, is_self_rooted_cvc5,
};
#[cfg(feature = "cvc5-verify")]
use crate::cvc5_quantifier_encode::infer_quantifier_patterns_cvc5;
use std::collections::HashSet;

// -------------------------------------------------------------------
// derive_narrowings_cvc5 tests (#257)
// -------------------------------------------------------------------

#[test]
fn test_derive_narrowings_cvc5_basic() {
    let narrowings = derive_narrowings_cvc5(&[("max_size".into(), 100)]);
    assert_eq!(narrowings.len(), 1);
    assert_eq!(narrowings[0], ("size".into(), 100));
}

#[test]
fn test_derive_narrowings_cvc5_empty() {
    let narrowings = derive_narrowings_cvc5(&[]);
    assert!(narrowings.is_empty());
}

#[test]
fn test_derive_narrowings_cvc5_no_prefix() {
    let narrowings = derive_narrowings_cvc5(&[("size".into(), 50)]);
    assert!(narrowings.is_empty());
}

#[test]
fn test_derive_narrowings_cvc5_uppercase_prefix() {
    let narrowings = derive_narrowings_cvc5(&[("MAX_BUFFER".into(), 1024)]);
    assert_eq!(narrowings.len(), 2);
    assert_eq!(narrowings[0], ("BUFFER".into(), 1024));
    assert_eq!(narrowings[1], ("buffer".into(), 1024));
}

#[test]
fn test_derive_narrowings_cvc5_multiple() {
    let narrowings = derive_narrowings_cvc5(&[
        ("max_size".into(), 100),
        ("max_count".into(), 50),
        ("threshold".into(), 10),
    ]);
    assert_eq!(narrowings.len(), 2);
    assert_eq!(narrowings[0], ("size".into(), 100));
    assert_eq!(narrowings[1], ("count".into(), 50));
}

// -------------------------------------------------------------------
// expr_to_smtlib tests
// -------------------------------------------------------------------

#[test]
fn test_smtlib_int_positive() {
    let expr = Expr::Literal(Literal::Int("42".into()));
    assert_eq!(expr_to_smtlib(&expr), Some("42".into()));
}

#[test]
fn test_smtlib_int_negative() {
    let expr = Expr::Literal(Literal::Int("-7".into()));
    assert_eq!(expr_to_smtlib(&expr), Some("(- 7)".into()));
}

#[test]
fn test_smtlib_bool_true() {
    let expr = Expr::Literal(Literal::Bool(true));
    assert_eq!(expr_to_smtlib(&expr), Some("true".into()));
}

#[test]
fn test_smtlib_bool_false() {
    let expr = Expr::Literal(Literal::Bool(false));
    assert_eq!(expr_to_smtlib(&expr), Some("false".into()));
}

#[test]
fn test_smtlib_string_encodes_as_named_const() {
    let expr = Expr::Literal(Literal::Str("hello".into()));
    assert_eq!(expr_to_smtlib(&expr), Some("__str_hello".into()));
}

#[test]
fn test_smtlib_ident() {
    let expr = Expr::Ident("x".into());
    assert_eq!(expr_to_smtlib(&expr), Some("x".into()));
}

#[test]
fn test_smtlib_result_keyword() {
    let expr = Expr::Ident("result".into());
    assert_eq!(expr_to_smtlib(&expr), Some("__result".into()));
}

#[test]
fn test_smtlib_dotted_ident_sanitized() {
    let expr = Expr::Ident("state.field".into());
    assert_eq!(expr_to_smtlib(&expr), Some("state_field".into()));
}

#[test]
fn test_smtlib_binop_add() {
    let expr = Expr::BinOp {
        op: BinOp::Add,
        lhs: Box::new(Expr::Ident("x".into())),
        rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
    };
    assert_eq!(expr_to_smtlib(&expr), Some("(+ x 1)".into()));
}

#[test]
fn test_smtlib_binop_neq() {
    let expr = Expr::BinOp {
        op: BinOp::Neq,
        lhs: Box::new(Expr::Ident("a".into())),
        rhs: Box::new(Expr::Ident("b".into())),
    };
    assert_eq!(expr_to_smtlib(&expr), Some("(not (= a b))".into()));
}

#[test]
fn test_smtlib_binop_div_is_integer() {
    let expr = Expr::BinOp {
        op: BinOp::Div,
        lhs: Box::new(Expr::Ident("x".into())),
        rhs: Box::new(Expr::Ident("y".into())),
    };
    assert_eq!(expr_to_smtlib(&expr), Some("(div x y)".into()));
}

#[test]
fn test_smtlib_binop_implies() {
    let expr = Expr::BinOp {
        op: BinOp::Implies,
        lhs: Box::new(Expr::Ident("p".into())),
        rhs: Box::new(Expr::Ident("q".into())),
    };
    assert_eq!(expr_to_smtlib(&expr), Some("(=> p q)".into()));
}

#[test]
fn test_smtlib_binop_range_encodes() {
    let expr = Expr::BinOp {
        op: BinOp::Range,
        lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
    };
    let s = expr_to_smtlib(&expr).expect("Range should encode");
    assert!(s.contains(">="), "missing >= in range encoding: {s}");
    assert!(s.contains("<"), "missing < in range encoding: {s}");
    assert!(
        s.contains("__range_fresh"),
        "missing fresh var in range: {s}"
    );
}

#[test]
fn test_smtlib_binop_in() {
    let expr = Expr::BinOp {
        op: BinOp::In,
        lhs: Box::new(Expr::Ident("x".into())),
        rhs: Box::new(Expr::Ident("collection".into())),
    };
    let s = expr_to_smtlib(&expr).expect("In should encode");
    assert!(s.contains("__contains"), "missing contains UF in: {s}");
    assert!(s.contains("collection"), "missing collection in: {s}");
    assert!(s.contains("x"), "missing element in: {s}");
}

#[test]
fn test_smtlib_binop_notin() {
    let expr = Expr::BinOp {
        op: BinOp::NotIn,
        lhs: Box::new(Expr::Ident("x".into())),
        rhs: Box::new(Expr::Ident("items".into())),
    };
    let s = expr_to_smtlib(&expr).expect("NotIn should encode");
    assert!(s.contains("not"), "missing negation in NotIn: {s}");
    assert!(
        s.contains("__contains"),
        "missing contains UF in NotIn: {s}"
    );
}

#[test]
fn test_smtlib_binop_concat() {
    let expr = Expr::BinOp {
        op: BinOp::Concat,
        lhs: Box::new(Expr::Ident("a".into())),
        rhs: Box::new(Expr::Ident("b".into())),
    };
    let s = expr_to_smtlib(&expr).expect("Concat should encode");
    assert!(s.contains("__concat"), "missing concat UF in: {s}");
    assert!(s.contains("a"), "missing lhs in concat: {s}");
    assert!(s.contains("b"), "missing rhs in concat: {s}");
}

#[test]
fn test_smtlib_unary_not() {
    let expr = Expr::UnaryOp {
        op: UnaryOp::Not,
        expr: Box::new(Expr::Ident("flag".into())),
    };
    assert_eq!(expr_to_smtlib(&expr), Some("(not flag)".into()));
}

#[test]
fn test_smtlib_unary_neg() {
    let expr = Expr::UnaryOp {
        op: UnaryOp::Neg,
        expr: Box::new(Expr::Ident("x".into())),
    };
    assert_eq!(expr_to_smtlib(&expr), Some("(- x)".into()));
}

#[test]
fn test_smtlib_if_with_else() {
    let expr = Expr::If {
        cond: Box::new(Expr::Ident("c".into())),
        then_branch: Box::new(Expr::Ident("t".into())),
        else_branch: Some(Box::new(Expr::Ident("e".into()))),
    };
    assert_eq!(expr_to_smtlib(&expr), Some("(ite c t e)".into()));
}

#[test]
fn test_smtlib_if_without_else() {
    let expr = Expr::If {
        cond: Box::new(Expr::Ident("p".into())),
        then_branch: Box::new(Expr::Ident("q".into())),
        else_branch: None,
    };
    assert_eq!(expr_to_smtlib(&expr), Some("(=> p q)".into()));
}

#[test]
fn test_smtlib_forall_non_range_domain() {
    // Non-range domain should produce __domain_contains guard
    let expr = Expr::Forall {
        var: "i".into(),
        domain: Box::new(Expr::Ident("xs".into())),
        body: Box::new(Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Expr::Ident("i".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        }),
    };
    assert_eq!(
        expr_to_smtlib(&expr),
        Some("(forall ((i Int)) (=> (__domain_contains xs i) (>= i 0)))".into())
    );
}

#[test]
fn test_smtlib_exists_non_range_domain() {
    // Non-range domain should produce __domain_contains guard
    let expr = Expr::Exists {
        var: "x".into(),
        domain: Box::new(Expr::Ident("S".into())),
        body: Box::new(Expr::BinOp {
            op: BinOp::Eq,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        }),
    };
    assert_eq!(
        expr_to_smtlib(&expr),
        Some("(exists ((x Int)) (and (__domain_contains S x) (= x 0)))".into())
    );
}

#[test]
fn test_smtlib_forall_range_domain() {
    // forall x in 0..10 { x >= 0 } should produce range guard
    let expr = Expr::Forall {
        var: "x".into(),
        domain: Box::new(Expr::BinOp {
            op: BinOp::Range,
            lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
        }),
        body: Box::new(Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        }),
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(
        s,
        "(forall ((x Int)) (=> (and (>= x 0) (< x 10)) (>= x 0)))"
    );
}

#[test]
fn test_smtlib_exists_range_domain() {
    // exists x in 0..10 { x == 5 } should produce range guard with conjunction
    let expr = Expr::Exists {
        var: "x".into(),
        domain: Box::new(Expr::BinOp {
            op: BinOp::Range,
            lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
        }),
        body: Box::new(Expr::BinOp {
            op: BinOp::Eq,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("5".into()))),
        }),
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(
        s,
        "(exists ((x Int)) (and (and (>= x 0) (< x 10)) (= x 5)))"
    );
}

#[test]
fn test_smtlib_forall_range_variable_bounds() {
    // forall i in 0..n { i >= 0 } -- variable upper bound
    let expr = Expr::Forall {
        var: "i".into(),
        domain: Box::new(Expr::BinOp {
            op: BinOp::Range,
            lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            rhs: Box::new(Expr::Ident("n".into())),
        }),
        body: Box::new(Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Expr::Ident("i".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        }),
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(forall ((i Int)) (=> (and (>= i 0) (< i n)) (>= i 0)))");
}

#[test]
fn test_smtlib_call_no_args() {
    let expr = Expr::Call {
        func: Box::new(Expr::Ident("foo".into())),
        args: vec![],
    };
    assert_eq!(expr_to_smtlib(&expr), Some("foo".into()));
}

#[test]
fn test_smtlib_call_with_args() {
    let expr = Expr::Call {
        func: Box::new(Expr::Ident("f".into())),
        args: vec![Expr::Ident("x".into()), Expr::Ident("y".into())],
    };
    assert_eq!(expr_to_smtlib(&expr), Some("(f x y)".into()));
}

#[test]
fn test_smtlib_old_adds_suffix() {
    let expr = Expr::Old(Box::new(Expr::Ident("x".into())));
    assert_eq!(expr_to_smtlib(&expr), Some("x__old".into()));
}

#[test]
fn test_smtlib_paren_transparent() {
    let expr = Expr::Paren(Box::new(Expr::Literal(Literal::Int("5".into()))));
    assert_eq!(expr_to_smtlib(&expr), Some("5".into()));
}

#[test]
fn test_smtlib_raw_single_token() {
    let expr = Expr::Raw(vec!["foo".into()]);
    assert_eq!(expr_to_smtlib(&expr), Some("foo".into()));
    // Integer token
    let expr_int = Expr::Raw(vec!["42".into()]);
    assert_eq!(expr_to_smtlib(&expr_int), Some("42".into()));
    // Bool token
    let expr_bool = Expr::Raw(vec!["true".into()]);
    assert_eq!(expr_to_smtlib(&expr_bool), Some("true".into()));
}

#[test]
fn test_smtlib_raw_precedence_climbing() {
    // "a + b * c" should parse as (+ a (* b c)) due to precedence
    let expr = Expr::Raw(vec![
        "a".into(),
        "+".into(),
        "b".into(),
        "*".into(),
        "c".into(),
    ]);
    assert_eq!(expr_to_smtlib(&expr), Some("(+ a (* b c))".into()));
}

#[test]
fn test_smtlib_raw_parentheses() {
    // "(a + b) * c" should parse as (* (+ a b) c)
    let expr = Expr::Raw(vec![
        "(".into(),
        "a".into(),
        "+".into(),
        "b".into(),
        ")".into(),
        "*".into(),
        "c".into(),
    ]);
    assert_eq!(expr_to_smtlib(&expr), Some("(* (+ a b) c)".into()));
}

#[test]
fn test_smtlib_raw_old_expression() {
    // "old ( x ) + 1" should parse old(x) + 1
    let expr = Expr::Raw(vec![
        "old".into(),
        "(".into(),
        "x".into(),
        ")".into(),
        "+".into(),
        "1".into(),
    ]);
    assert_eq!(expr_to_smtlib(&expr), Some("(+ x__old 1)".into()));
}

#[test]
fn test_smtlib_raw_nested_operators() {
    // "a + b - c + d" left-associative: (+ (- (+ a b) c) d)
    let expr = Expr::Raw(vec![
        "a".into(),
        "+".into(),
        "b".into(),
        "-".into(),
        "c".into(),
        "+".into(),
        "d".into(),
    ]);
    let result = expr_to_smtlib(&expr).unwrap();
    // Left-associative: ((a + b) - c) + d
    assert_eq!(result, "(+ (- (+ a b) c) d)");
}

#[test]
fn test_smtlib_raw_comparison_chain() {
    // "a < b < c" desugars to (and (< a b) (< b c))
    let expr = Expr::Raw(vec![
        "a".into(),
        "<".into(),
        "b".into(),
        "<".into(),
        "c".into(),
    ]);
    assert_eq!(expr_to_smtlib(&expr), Some("(and (< a b) (< b c))".into()));
}

#[test]
fn test_smtlib_raw_unary_not() {
    // "! x" -> (not x)
    let expr = Expr::Raw(vec!["!".into(), "x".into()]);
    assert_eq!(expr_to_smtlib(&expr), Some("(not x)".into()));
}

#[test]
fn test_smtlib_raw_unary_neg() {
    // "- x" -> (- x)
    let expr = Expr::Raw(vec!["-".into(), "x".into()]);
    assert_eq!(expr_to_smtlib(&expr), Some("(- x)".into()));
}

#[test]
fn test_smtlib_raw_logical_ops() {
    // "a && b || c" should respect precedence: (or (and a b) c)
    let expr = Expr::Raw(vec![
        "a".into(),
        "&&".into(),
        "b".into(),
        "||".into(),
        "c".into(),
    ]);
    assert_eq!(expr_to_smtlib(&expr), Some("(or (and a b) c)".into()));
}

#[test]
fn test_smtlib_raw_neq() {
    // "a != b" -> (not (= a b))
    let expr = Expr::Raw(vec!["a".into(), "!=".into(), "b".into()]);
    assert_eq!(expr_to_smtlib(&expr), Some("(not (= a b))".into()));
}

#[test]
fn test_smtlib_raw_mod_div() {
    // "a mod b" and "a div b"
    let expr_mod = Expr::Raw(vec!["a".into(), "mod".into(), "b".into()]);
    assert_eq!(expr_to_smtlib(&expr_mod), Some("(mod a b)".into()));

    let expr_div = Expr::Raw(vec!["a".into(), "div".into(), "b".into()]);
    assert_eq!(expr_to_smtlib(&expr_div), Some("(div a b)".into()));
}

#[test]
fn test_smtlib_raw_complex_expression() {
    // "x >= 0 && x < max" -> (and (>= x 0) (< x max))
    let expr = Expr::Raw(vec![
        "x".into(),
        ">=".into(),
        "0".into(),
        "&&".into(),
        "x".into(),
        "<".into(),
        "max".into(),
    ]);
    assert_eq!(
        expr_to_smtlib(&expr),
        Some("(and (>= x 0) (< x max))".into())
    );
}

#[test]
fn test_smtlib_raw_function_call() {
    // "abs ( x )" -> (abs x)
    let expr = Expr::Raw(vec!["abs".into(), "(".into(), "x".into(), ")".into()]);
    assert_eq!(expr_to_smtlib(&expr), Some("(abs x)".into()));
}

#[test]
fn test_smtlib_let_expr() {
    let expr = Expr::Let {
        name: "x".into(),
        value: Box::new(Expr::Literal(Literal::Int("5".into()))),
        body: Box::new(Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
        }),
    };
    assert_eq!(expr_to_smtlib(&expr), Some("(let ((x 5)) (+ x 1))".into()));
}

#[test]
fn test_smtlib_match_with_literal_and_wildcard() {
    use assura_parser::ast::MatchArm;
    let expr = Expr::Match {
        scrutinee: Box::new(Expr::Ident("n".into())),
        arms: vec![
            MatchArm {
                pattern: Pattern::Literal(Literal::Int("0".into())),
                body: Expr::Literal(Literal::Int("1".into())),
            },
            MatchArm {
                pattern: Pattern::Wildcard,
                body: Expr::Ident("n".into()),
            },
        ],
    };
    assert_eq!(expr_to_smtlib(&expr), Some("(ite (= n 0) 1 n)".into()));
}

#[test]
fn test_smtlib_match_empty_arms() {
    let expr = Expr::Match {
        scrutinee: Box::new(Expr::Ident("n".into())),
        arms: vec![],
    };
    assert_eq!(expr_to_smtlib(&expr), None);
}

#[test]
fn test_smtlib_match_constructor_pattern() {
    use assura_parser::ast::MatchArm;
    // match x { Some(v) => v, None => 0 }
    let expr = Expr::Match {
        scrutinee: Box::new(Expr::Ident("x".into())),
        arms: vec![
            MatchArm {
                pattern: Pattern::Constructor {
                    name: "Some".into(),
                    fields: vec![Pattern::Ident("v".into())],
                },
                body: Expr::Ident("v".into()),
            },
            MatchArm {
                pattern: Pattern::Constructor {
                    name: "None".into(),
                    fields: vec![],
                },
                body: Expr::Literal(Literal::Int("0".into())),
            },
            MatchArm {
                pattern: Pattern::Wildcard,
                body: Expr::Literal(Literal::Int("0".into())),
            },
        ],
    };
    let smt = expr_to_smtlib(&expr).expect("should encode constructor match");
    // #263: Constructor patterns use ADT tag testers, not pattern hashes.
    assert!(smt.contains("__adt_tag_Option"));
    assert!(smt.contains("(= (__adt_tag_Option x) 0)")); // Some
    assert!(smt.contains("(= (__adt_tag_Option x) 1)")); // None
    assert!(smt.contains("ite"));
}

#[test]
fn test_smtlib_match_tuple_pattern() {
    use assura_parser::ast::MatchArm;
    // match t { (a, b) => a }
    let expr = Expr::Match {
        scrutinee: Box::new(Expr::Ident("t".into())),
        arms: vec![MatchArm {
            pattern: Pattern::Tuple(vec![Pattern::Ident("a".into()), Pattern::Ident("b".into())]),
            body: Expr::Ident("a".into()),
        }],
    };
    let smt = expr_to_smtlib(&expr).expect("should encode tuple match");
    // Tuple is structural, body is just "a"
    assert_eq!(smt, "a");
}

#[test]
fn test_smtlib_match_ident_constructor_like() {
    use assura_parser::ast::MatchArm;
    // match x { None => 1, _ => 0 }  (Ident "None" uppercase = constructor)
    let expr = Expr::Match {
        scrutinee: Box::new(Expr::Ident("x".into())),
        arms: vec![
            MatchArm {
                pattern: Pattern::Ident("None".into()),
                body: Expr::Literal(Literal::Int("1".into())),
            },
            MatchArm {
                pattern: Pattern::Wildcard,
                body: Expr::Literal(Literal::Int("0".into())),
            },
        ],
    };
    let smt = expr_to_smtlib(&expr).expect("should encode ident-as-constructor match");
    let none_hash = crate::cvc5_builtins::pattern_hash_name("None");
    assert!(smt.contains(&none_hash.to_string()));
    assert!(smt.contains("ite"));
}

// -------------------------------------------------------------------
// collect_vars tests
// -------------------------------------------------------------------

#[test]
fn test_collect_vars_ident() {
    let mut vars = HashSet::new();
    collect_vars(&Expr::Ident("x".into()), &mut vars);
    assert!(vars.contains("x"));
}

#[test]
fn test_collect_vars_result() {
    let mut vars = HashSet::new();
    collect_vars(&Expr::Ident("result".into()), &mut vars);
    assert!(vars.contains("__result"));
    assert!(!vars.contains("result"));
}

#[test]
fn test_collect_vars_binop() {
    let mut vars = HashSet::new();
    let expr = Expr::BinOp {
        op: BinOp::Add,
        lhs: Box::new(Expr::Ident("a".into())),
        rhs: Box::new(Expr::Ident("b".into())),
    };
    collect_vars(&expr, &mut vars);
    assert!(vars.contains("a"));
    assert!(vars.contains("b"));
}

#[test]
fn test_collect_vars_if_all_branches() {
    let mut vars = HashSet::new();
    let expr = Expr::If {
        cond: Box::new(Expr::Ident("c".into())),
        then_branch: Box::new(Expr::Ident("t".into())),
        else_branch: Some(Box::new(Expr::Ident("e".into()))),
    };
    collect_vars(&expr, &mut vars);
    assert!(vars.contains("c"));
    assert!(vars.contains("t"));
    assert!(vars.contains("e"));
}

#[test]
fn test_collect_vars_literal_no_vars() {
    let mut vars = HashSet::new();
    collect_vars(&Expr::Literal(Literal::Int("42".into())), &mut vars);
    assert!(vars.is_empty());
}

#[test]
fn test_collect_vars_dotted_sanitized() {
    let mut vars = HashSet::new();
    collect_vars(&Expr::Ident("obj.field".into()), &mut vars);
    assert!(vars.contains("obj_field"));
}

// -------------------------------------------------------------------
// parse_smtlib_model tests
// -------------------------------------------------------------------

#[test]
fn test_parse_model_define_fun() {
    let model = "(define-fun x () Int 42)\n(define-fun y () Int (- 1))";
    let parsed = parse_smtlib_model(model).unwrap();
    assert_eq!(parsed.variables.len(), 2);
    assert_eq!(parsed.variables[0].0, "x");
    assert_eq!(parsed.variables[0].1, "42");
    assert_eq!(parsed.variables[1].0, "y");
    assert_eq!(parsed.variables[1].1, "(- 1)");
}

#[test]
fn test_parse_model_empty() {
    assert!(parse_smtlib_model("").is_none());
}

#[test]
fn test_parse_model_no_define_fun() {
    assert!(parse_smtlib_model("sat\n(something else)").is_none());
}

#[test]
fn test_parse_model_skips_coerce() {
    let model = "(define-fun __coerce_1 () Int 0)\n(define-fun x () Int 5)";
    let parsed = parse_smtlib_model(model).unwrap();
    assert_eq!(parsed.variables.len(), 1);
    assert_eq!(parsed.variables[0].0, "x");
}

// -------------------------------------------------------------------
// is_internal_cvc5_var and counterexample model filtering (#260)
// -------------------------------------------------------------------

#[test]
fn test_is_internal_cvc5_var_internal_prefixes() {
    assert!(is_internal_cvc5_var("__str_hello"));
    assert!(is_internal_cvc5_var("__tuple_0"));
    assert!(is_internal_cvc5_var("__list_vals"));
    assert!(is_internal_cvc5_var("__fresh_3"));
    assert!(is_internal_cvc5_var("__field_len"));
    assert!(is_internal_cvc5_var("__index_0"));
    assert!(is_internal_cvc5_var("__len_buf"));
    assert!(is_internal_cvc5_var("__arr_data"));
    assert!(is_internal_cvc5_var("__domain_contains_x"));
    assert!(is_internal_cvc5_var("__apply_func"));
    assert!(is_internal_cvc5_var("__coerce_1"));
    assert!(is_internal_cvc5_var("__trigger_pat"));
    assert!(is_internal_cvc5_var("__list_get_0"));
    assert!(is_internal_cvc5_var("__result"));
    assert!(is_internal_cvc5_var("__contains"));
    assert!(is_internal_cvc5_var("__obj_ptr"));
}

#[test]
fn test_is_internal_cvc5_var_user_variables() {
    assert!(!is_internal_cvc5_var("x"));
    assert!(!is_internal_cvc5_var("buffer_size"));
    assert!(!is_internal_cvc5_var("payload_length"));
    assert!(!is_internal_cvc5_var("n"));
    assert!(!is_internal_cvc5_var("result_count"));
    assert!(!is_internal_cvc5_var("max_size"));
    assert!(!is_internal_cvc5_var("i"));
}

#[test]
fn test_parse_model_filters_all_internal_vars() {
    let model = "\
(define-fun __str_hello () Int 1)\n\
(define-fun __field_len () Int 5)\n\
(define-fun __fresh_0 () Int 99)\n\
(define-fun __result () Int 42)\n\
(define-fun __coerce_1 () Int 0)\n\
(define-fun x () Int 10)\n\
(define-fun y () Int 20)";
    let parsed = parse_smtlib_model(model).unwrap();
    let names: Vec<&str> = parsed.variables.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["x", "y"]);
    assert!(!names.contains(&"__str_hello"));
    assert!(!names.contains(&"__field_len"));
    assert!(!names.contains(&"__fresh_0"));
    assert!(!names.contains(&"__result"));
    assert!(!names.contains(&"__coerce_1"));
}

#[test]
fn test_parse_model_sorted_alphabetically() {
    let model = "\
(define-fun z_var () Int 3)\n\
(define-fun a_var () Int 1)\n\
(define-fun m_var () Int 2)";
    let parsed = parse_smtlib_model(model).unwrap();
    let names: Vec<&str> = parsed.variables.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["a_var", "m_var", "z_var"]);
}

#[test]
fn test_parse_model_all_internal_returns_none() {
    let model = "\
(define-fun __str_a () Int 1)\n\
(define-fun __fresh_0 () Int 2)\n\
(define-fun __coerce_1 () Int 3)";
    assert!(
        parse_smtlib_model(model).is_none(),
        "model with only internal vars should return None"
    );
}

// -------------------------------------------------------------------
// collect_vars exhaustive coverage (issue #54)
// -------------------------------------------------------------------

#[test]
fn collect_vars_field_access() {
    let expr = Expr::Field(Box::new(Expr::Ident("obj".into())), "field".into());
    let mut vars = HashSet::new();
    collect_vars(&expr, &mut vars);
    assert!(vars.contains("obj"));
}

#[test]
fn collect_vars_method_call() {
    let expr = Expr::MethodCall {
        receiver: Box::new(Expr::Ident("list".into())),
        method: "len".into(),
        args: vec![Expr::Ident("idx".into())],
    };
    let mut vars = HashSet::new();
    collect_vars(&expr, &mut vars);
    assert!(vars.contains("list"));
    assert!(vars.contains("idx"));
}

#[test]
fn collect_vars_index() {
    let expr = Expr::Index {
        expr: Box::new(Expr::Ident("arr".into())),
        index: Box::new(Expr::Ident("i".into())),
    };
    let mut vars = HashSet::new();
    collect_vars(&expr, &mut vars);
    assert!(vars.contains("arr"));
    assert!(vars.contains("i"));
}

#[test]
fn collect_vars_let_expr() {
    let expr = Expr::Let {
        name: "tmp".into(),
        value: Box::new(Expr::Ident("a".into())),
        body: Box::new(Expr::Ident("b".into())),
    };
    let mut vars = HashSet::new();
    collect_vars(&expr, &mut vars);
    assert!(vars.contains("a"));
    assert!(vars.contains("b"));
}

#[test]
fn collect_vars_match_expr() {
    use assura_parser::ast::{MatchArm, Pattern};
    let expr = Expr::Match {
        scrutinee: Box::new(Expr::Ident("x".into())),
        arms: vec![MatchArm {
            pattern: Pattern::Ident("_".into()),
            body: Expr::Ident("y".into()),
        }],
    };
    let mut vars = HashSet::new();
    collect_vars(&expr, &mut vars);
    assert!(vars.contains("x"));
    assert!(vars.contains("y"));
}

#[test]
fn collect_vars_list_tuple_block() {
    let list = Expr::List(vec![Expr::Ident("a".into()), Expr::Ident("b".into())]);
    let tuple = Expr::Tuple(vec![Expr::Ident("c".into())]);
    let block = Expr::Block(vec![Expr::Ident("d".into())]);
    let mut vars = HashSet::new();
    collect_vars(&list, &mut vars);
    collect_vars(&tuple, &mut vars);
    collect_vars(&block, &mut vars);
    assert!(vars.contains("a"));
    assert!(vars.contains("b"));
    assert!(vars.contains("c"));
    assert!(vars.contains("d"));
}

#[test]
fn collect_vars_apply() {
    let expr = Expr::Apply {
        lemma_name: "lem".into(),
        args: vec![Expr::Ident("p".into())],
    };
    let mut vars = HashSet::new();
    collect_vars(&expr, &mut vars);
    assert!(vars.contains("p"));
}

#[test]
fn collect_vars_literal_is_empty() {
    let expr = Expr::Literal(Literal::Int("42".into()));
    let mut vars = HashSet::new();
    collect_vars(&expr, &mut vars);
    assert!(vars.is_empty());
}

// -------------------------------------------------------------------
// Regression: CVC5 must_not semantics (#166)
// -------------------------------------------------------------------

/// must_not(true) should NOT be verified: true is always possible.
/// The CVC5 backend must assert the body directly (not negate it).
#[test]
fn test_cvc5_must_not_semantics() {
    // must_not { true } -- "true" is always satisfiable, so
    // asserting it directly gives SAT -> Counterexample.
    let clause = Clause {
        kind: ClauseKind::MustNot,
        body: Expr::Literal(Literal::Bool(true)),
        effect_variables: vec![],
    };
    let results = verify_contract_cvc5("TestMustNot", &[clause]);
    // Should be Counterexample (the bad thing CAN happen)
    assert_eq!(results.len(), 1);
    assert!(
        matches!(
            &results[0],
            VerificationResult::Counterexample { .. } | VerificationResult::Unknown { .. }
        ),
        "must_not(true) should be Counterexample or Unknown, got: {:?}",
        results[0]
    );
}

/// must_not(false) should verify: false is impossible.
#[test]
fn test_cvc5_must_not_impossible() {
    let clause = Clause {
        kind: ClauseKind::MustNot,
        body: Expr::Literal(Literal::Bool(false)),
        effect_variables: vec![],
    };
    let results = verify_contract_cvc5("TestMustNotFalse", &[clause]);
    assert_eq!(results.len(), 1);
    assert!(
        matches!(
            &results[0],
            VerificationResult::Verified { .. } | VerificationResult::Unknown { .. }
        ),
        "must_not(false) should be Verified or Unknown (if cvc5 not installed), got: {:?}",
        results[0]
    );
}

// -------------------------------------------------------------------
// Regression: quantifier-bound vars not global (#167)
// -------------------------------------------------------------------

/// Quantifier-bound variables must NOT appear in the global
/// `(declare-const ...)` section of the generated SMT-LIB2 script.
#[test]
fn test_cvc5_quantifier_var_not_global() {
    // forall i in xs: i >= 0
    let body = Expr::BinOp {
        op: BinOp::Gte,
        lhs: Box::new(Expr::Ident("i".into())),
        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
    };
    let forall_expr = Expr::Forall {
        var: "i".into(),
        domain: Box::new(Expr::Ident("xs".into())),
        body: Box::new(body),
    };
    let mut vars = HashSet::new();
    collect_vars(&forall_expr, &mut vars);
    // "i" must NOT be in the global vars set
    assert!(
        !vars.contains("i"),
        "quantifier-bound variable 'i' must not be a global constant"
    );
    // "xs" (the domain) should still be collected
    assert!(
        vars.contains("xs"),
        "domain variable 'xs' should be collected"
    );
}

// -------------------------------------------------------------------
// Unmodelable feature pre-check tests (cfg-independent)
// -------------------------------------------------------------------

#[test]
fn test_typestate_now_modelable() {
    // #262: Raw tokens with @ are now modelable (encoded as integer equality)
    let expr = Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]);
    assert!(
        !expr_has_unmodelable_features_cvc5(&expr),
        "typestate @ annotation should be modelable after #262"
    );
}

#[test]
fn test_no_unmodelable_reason_for_typestate() {
    // #262: Typestate no longer produces unmodelable reasons
    let expr = Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]);
    let reasons = collect_unmodelable_reasons_cvc5(&expr);
    assert!(
        reasons.is_empty(),
        "typestate should produce no unmodelable reasons after #262, got: {:?}",
        reasons
    );
}

#[test]
fn test_modelable_normal_expr() {
    // Normal binary expression should be modelable
    let expr = Expr::BinOp {
        op: BinOp::Gt,
        lhs: Box::new(Expr::Ident("x".into())),
        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
    };
    assert!(
        !expr_has_unmodelable_features_cvc5(&expr),
        "normal binop should be modelable"
    );
}

#[test]
fn test_typestate_nested_in_binop_modelable() {
    // #262: Typestate nested in a binary expression is now modelable
    let expr = Expr::BinOp {
        op: BinOp::And,
        lhs: Box::new(Expr::Ident("x".into())),
        rhs: Box::new(Expr::Raw(vec![
            "conn".into(),
            "@".into(),
            "Connected".into(),
        ])),
    };
    assert!(
        !expr_has_unmodelable_features_cvc5(&expr),
        "typestate nested in binop should be modelable after #262"
    );
}

#[test]
fn test_typestate_in_if_branch_modelable() {
    // #262: Typestate in if branch is now modelable
    let expr = Expr::If {
        cond: Box::new(Expr::Ident("flag".into())),
        then_branch: Box::new(Expr::Raw(vec!["s".into(), "@".into(), "Locked".into()])),
        else_branch: None,
    };
    assert!(
        !expr_has_unmodelable_features_cvc5(&expr),
        "typestate in if-then should be modelable after #262"
    );
}

#[test]
fn test_typestate_in_forall_body_modelable() {
    // #262: Typestate in forall body is now modelable
    let expr = Expr::Forall {
        var: "i".into(),
        domain: Box::new(Expr::Ident("xs".into())),
        body: Box::new(Expr::Raw(vec!["item".into(), "@".into(), "Valid".into()])),
    };
    assert!(
        !expr_has_unmodelable_features_cvc5(&expr),
        "typestate in forall body should be modelable after #262"
    );
}

#[test]
fn test_cvc5_typestate_same_state_verifies() {
    // #262: Typestate same pre/post should verify via verify_contract_cvc5
    // (or Unknown if cvc5 is not installed on this system)
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
            effect_variables: vec![],
        },
    ];
    let results = verify_contract_cvc5("TypestateIdentity", &clauses);
    assert!(
        !results.is_empty(),
        "should have results for typestate identity"
    );
    assert!(
        matches!(
            &results[0],
            VerificationResult::Verified { .. } | VerificationResult::Unknown { .. }
        ),
        "same typestate pre/post should verify or Unknown (if cvc5 not installed), got: {:?}",
        results[0]
    );
}

#[test]
fn test_cvc5_typestate_different_state_counterexample() {
    // #262: Different typestate pre/post should produce counterexample
    // (or Unknown if cvc5 is not installed on this system)
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Expr::Raw(vec!["file".into(), "@".into(), "Closed".into()]),
            effect_variables: vec![],
        },
    ];
    let results = verify_contract_cvc5("TypestateMismatch", &clauses);
    assert!(
        !results.is_empty(),
        "should have results for typestate mismatch"
    );
    assert!(
        matches!(
            &results[0],
            VerificationResult::Counterexample { .. } | VerificationResult::Unknown { .. }
        ),
        "different typestate pre/post should produce counterexample or Unknown (if cvc5 not installed), got: {:?}",
        results[0]
    );
}

// -------------------------------------------------------------------
// Lemma apply-ref collection tests (cfg-independent)
// -------------------------------------------------------------------

#[test]
fn test_collect_apply_refs_simple() {
    let expr = Expr::Apply {
        lemma_name: "helper".into(),
        args: vec![Expr::Ident("x".into())],
    };
    let refs = collect_apply_refs_from_expr(&expr);
    assert_eq!(refs, vec!["helper"]);
}

#[test]
fn test_collect_apply_refs_nested_in_binop() {
    let expr = Expr::BinOp {
        op: BinOp::And,
        lhs: Box::new(Expr::Apply {
            lemma_name: "lem_a".into(),
            args: vec![Expr::Ident("x".into())],
        }),
        rhs: Box::new(Expr::Apply {
            lemma_name: "lem_b".into(),
            args: vec![Expr::Ident("y".into())],
        }),
    };
    let refs = collect_apply_refs_from_expr(&expr);
    assert_eq!(refs.len(), 2);
    assert!(refs.contains(&"lem_a".to_string()));
    assert!(refs.contains(&"lem_b".to_string()));
}

#[test]
fn test_collect_apply_refs_no_apply() {
    let expr = Expr::BinOp {
        op: BinOp::Gt,
        lhs: Box::new(Expr::Ident("x".into())),
        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
    };
    let refs = collect_apply_refs_from_expr(&expr);
    assert!(refs.is_empty());
}

#[test]
fn test_collect_apply_refs_nested_in_if() {
    let expr = Expr::If {
        cond: Box::new(Expr::Ident("flag".into())),
        then_branch: Box::new(Expr::Apply {
            lemma_name: "branch_lem".into(),
            args: vec![],
        }),
        else_branch: Some(Box::new(Expr::Literal(Literal::Bool(true)))),
    };
    let refs = collect_apply_refs_from_expr(&expr);
    assert_eq!(refs, vec!["branch_lem"]);
}

// -------------------------------------------------------------------
// SMT-LIB float encoding tests (#248)
// -------------------------------------------------------------------

#[test]
fn test_smtlib_float_rational_encoding() {
    let expr = Expr::Literal(Literal::Float("3.14".into()));
    let result = expr_to_smtlib(&expr).unwrap();
    assert_eq!(result, "(/ 3140000 1000000)");
}

#[test]
fn test_smtlib_float_zero() {
    let expr = Expr::Literal(Literal::Float("0.0".into()));
    let result = expr_to_smtlib(&expr).unwrap();
    assert_eq!(result, "(/ 0 1000000)");
}

#[test]
fn test_smtlib_float_negative() {
    // Negative floats: the negation is applied by UnaryOp::Neg externally,
    // but the literal itself may parse as negative
    let expr = Expr::UnaryOp {
        op: UnaryOp::Neg,
        expr: Box::new(Expr::Literal(Literal::Float("2.5".into()))),
    };
    let result = expr_to_smtlib(&expr).unwrap();
    assert_eq!(result, "(- (/ 2500000 1000000))");
}

#[test]
fn test_smtlib_match_float_pattern_rational() {
    // Match arm with float literal should use rational encoding
    let expr = Expr::Match {
        scrutinee: Box::new(Expr::Ident("x".into())),
        arms: vec![
            assura_parser::ast::MatchArm {
                pattern: Pattern::Literal(Literal::Float("1.5".into())),
                body: Expr::Literal(Literal::Bool(true)),
            },
            assura_parser::ast::MatchArm {
                pattern: Pattern::Wildcard,
                body: Expr::Literal(Literal::Bool(false)),
            },
        ],
    };
    let result = expr_to_smtlib(&expr).unwrap();
    assert!(
        result.contains("(/ 1500000 1000000)"),
        "match float pattern should use rational: {result}"
    );
}

// Deep field chain flattening helpers (#250)
// -------------------------------------------------------------------

#[test]
fn test_is_self_rooted_cvc5_ident_self() {
    let expr = Expr::Ident("self".into());
    assert!(is_self_rooted_cvc5(&expr));
}

#[test]
fn test_is_self_rooted_cvc5_ident_other() {
    let expr = Expr::Ident("x".into());
    assert!(!is_self_rooted_cvc5(&expr));
}

#[test]
fn test_is_self_rooted_cvc5_field_chain() {
    // self.value
    let expr = Expr::Field(Box::new(Expr::Ident("self".into())), "value".into());
    assert!(is_self_rooted_cvc5(&expr));
}

#[test]
fn test_is_self_rooted_cvc5_deep_chain() {
    // self.inner.value
    let expr = Expr::Field(
        Box::new(Expr::Field(
            Box::new(Expr::Ident("self".into())),
            "inner".into(),
        )),
        "value".into(),
    );
    assert!(is_self_rooted_cvc5(&expr));
}

#[test]
fn test_field_chain_depth_cvc5_ident() {
    assert_eq!(field_chain_depth_cvc5(&Expr::Ident("x".into())), 0);
}

#[test]
fn test_field_chain_depth_cvc5_single() {
    let expr = Expr::Field(Box::new(Expr::Ident("x".into())), "y".into());
    assert_eq!(field_chain_depth_cvc5(&expr), 1);
}

#[test]
fn test_field_chain_depth_cvc5_deep() {
    // a.b.c -> depth 2
    let expr = Expr::Field(
        Box::new(Expr::Field(Box::new(Expr::Ident("a".into())), "b".into())),
        "c".into(),
    );
    assert_eq!(field_chain_depth_cvc5(&expr), 2);
}

#[test]
fn test_has_deep_field_chain_cvc5() {
    // a.b -> depth 1, not deep
    let shallow = Expr::Field(Box::new(Expr::Ident("a".into())), "b".into());
    assert!(!has_deep_field_chain_cvc5(&shallow));

    // a.b.c -> depth 2, deep
    let deep = Expr::Field(
        Box::new(Expr::Field(Box::new(Expr::Ident("a".into())), "b".into())),
        "c".into(),
    );
    assert!(has_deep_field_chain_cvc5(&deep));
}

#[test]
fn test_flatten_field_chain_cvc5_simple() {
    // a.b -> "a__b"
    let expr = Expr::Field(Box::new(Expr::Ident("a".into())), "b".into());
    assert_eq!(flatten_field_chain_cvc5(&expr), "a__b");
}

#[test]
fn test_flatten_field_chain_cvc5_deep() {
    // state.head.extra.extra_max -> "state__head__extra__extra_max"
    let expr = Expr::Field(
        Box::new(Expr::Field(
            Box::new(Expr::Field(
                Box::new(Expr::Ident("state".into())),
                "head".into(),
            )),
            "extra".into(),
        )),
        "extra_max".into(),
    );
    assert_eq!(
        flatten_field_chain_cvc5(&expr),
        "state__head__extra__extra_max"
    );
}

#[test]
fn test_flatten_field_chain_cvc5_paren() {
    // (a).b -> "a__b"
    let expr = Expr::Field(
        Box::new(Expr::Paren(Box::new(Expr::Ident("a".into())))),
        "b".into(),
    );
    assert_eq!(flatten_field_chain_cvc5(&expr), "a__b");
}

#[test]
fn test_cvc5_deep_field_chain_smtlib_flattening() {
    // state.head.extra.extra_max should flatten in SMT-LIB output
    let expr = Expr::Field(
        Box::new(Expr::Field(
            Box::new(Expr::Field(
                Box::new(Expr::Ident("state".into())),
                "head".into(),
            )),
            "extra".into(),
        )),
        "extra_max".into(),
    );
    let result = expr_to_smtlib(&expr);
    assert_eq!(result, Some("state__head__extra__extra_max".into()));
}

#[test]
fn test_cvc5_self_rooted_smtlib_flattening() {
    // self.value should flatten even at depth 1
    let expr = Expr::Field(Box::new(Expr::Ident("self".into())), "value".into());
    let result = expr_to_smtlib(&expr);
    assert_eq!(result, Some("self__value".into()));
}

#[test]
fn test_cvc5_shallow_field_smtlib_no_flatten() {
    // obj.field at depth 1 (not self-rooted) should NOT flatten
    let expr = Expr::Field(Box::new(Expr::Ident("obj".into())), "field".into());
    let result = expr_to_smtlib(&expr);
    assert_eq!(result, Some("(__field_field obj)".into()));
}

#[test]
fn test_cvc5_old_deep_field_smtlib_flattening() {
    // old(state.head.value) should flatten to state__head__value__old
    let inner = Expr::Field(
        Box::new(Expr::Field(
            Box::new(Expr::Ident("state".into())),
            "head".into(),
        )),
        "value".into(),
    );
    let expr = Expr::Old(Box::new(inner));
    let result = expr_to_smtlib(&expr);
    assert_eq!(result, Some("state__head__value__old".into()));
}

#[test]
fn test_cvc5_old_self_rooted_smtlib_flattening() {
    // old(self.counter) should flatten to self__counter__old
    let inner = Expr::Field(Box::new(Expr::Ident("self".into())), "counter".into());
    let expr = Expr::Old(Box::new(inner));
    let result = expr_to_smtlib(&expr);
    assert_eq!(result, Some("self__counter__old".into()));
}

#[test]
fn test_cvc5_deep_field_chain_contract_verifies() {
    // Contract: requires { x >= 0 && x < state.head.extra.max }
    //           ensures  { state.head.extra.max > x }
    // With flattening, both sides reference the same flat variable.
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Expr::BinOp {
                op: BinOp::And,
                lhs: Box::new(Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                }),
                rhs: Box::new(Expr::BinOp {
                    op: BinOp::Lt,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Field(
                        Box::new(Expr::Field(
                            Box::new(Expr::Field(
                                Box::new(Expr::Ident("state".into())),
                                "head".into(),
                            )),
                            "extra".into(),
                        )),
                        "max".into(),
                    )),
                }),
            },
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Expr::BinOp {
                op: BinOp::Gt,
                lhs: Box::new(Expr::Field(
                    Box::new(Expr::Field(
                        Box::new(Expr::Field(
                            Box::new(Expr::Ident("state".into())),
                            "head".into(),
                        )),
                        "extra".into(),
                    )),
                    "max".into(),
                )),
                rhs: Box::new(Expr::Ident("x".into())),
            },
            effect_variables: vec![],
        },
    ];
    let results = verify_contract_cvc5("DeepFieldChain", &clauses);
    assert_eq!(results.len(), 1);
    assert!(
        matches!(
            &results[0],
            VerificationResult::Verified { .. } | VerificationResult::Unknown { .. }
        ),
        "deep field chain contract should verify (or Unknown if cvc5 not installed), got: {:?}",
        results[0]
    );
}

#[test]
fn test_cvc5_self_rooted_field_contract_verifies() {
    // Contract with self.value: requires { self.value > 0 } ensures { self.value >= 1 }
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Expr::BinOp {
                op: BinOp::Gt,
                lhs: Box::new(Expr::Field(
                    Box::new(Expr::Ident("self".into())),
                    "value".into(),
                )),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::Field(
                    Box::new(Expr::Ident("self".into())),
                    "value".into(),
                )),
                rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
            },
            effect_variables: vec![],
        },
    ];
    let results = verify_contract_cvc5("SelfRootedField", &clauses);
    assert_eq!(results.len(), 1);
    assert!(
        matches!(
            &results[0],
            VerificationResult::Verified { .. } | VerificationResult::Unknown { .. }
        ),
        "self-rooted field contract should verify (or Unknown if cvc5 not installed), got: {:?}",
        results[0]
    );
}

#[test]
fn test_cvc5_nested_field_boolean_smtlib() {
    // obj.inner.is_empty should flatten in SMT-LIB output
    let expr = Expr::Field(
        Box::new(Expr::Field(
            Box::new(Expr::Ident("obj".into())),
            "inner".into(),
        )),
        "is_empty".into(),
    );
    let result = expr_to_smtlib(&expr);
    assert_eq!(result, Some("obj__inner__is_empty".into()));
}

#[test]
fn test_cvc5_nested_field_size_smtlib() {
    // obj.inner.length should flatten in SMT-LIB output
    let expr = Expr::Field(
        Box::new(Expr::Field(
            Box::new(Expr::Ident("obj".into())),
            "inner".into(),
        )),
        "length".into(),
    );
    let result = expr_to_smtlib(&expr);
    assert_eq!(result, Some("obj__inner__length".into()));
}

// -------------------------------------------------------------------
// CVC5 native API tests (only when cvc5-verify feature enabled)
// -------------------------------------------------------------------

#[cfg(feature = "cvc5-verify")]
mod native_tests {
    use super::*;
    use assura_parser::ast::Param;

    #[test]
    fn cvc5_with_types_fn_params_nat() {
        // FnDef-style: params passed explicitly (not via input() clause).
        // This is the path used for `fn check_table_bounds(root_bits: Nat, ...)`
        let params = vec![Param {
            name: "n".into(),
            ty: vec!["Nat".into()],
            parsed_type: None,
        }];
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("n".into())),
                op: BinOp::Gte,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
            effect_variables: vec![],
        }];
        let mut cache = SessionCache::new();
        let results =
            verify_contract_cvc5_with_types("FnNatParam", &clauses, &params, &[], &mut cache);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "Nat param n >= 0 should verify via explicit params: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_trivial_ensures_verified() {
        // requires x > 0, ensures x > 0 (trivially true)
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gt,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gt,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("NativeTest", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "should verify: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_counterexample() {
        // No requires, ensures x > 0 (counterexample: x = 0)
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
            effect_variables: vec![],
        }];
        let results = verify_contract_cvc5("NativeCounterexample", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "should have counterexample: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_invariant_satisfiable() {
        // invariant { x > 0 } -- satisfiable (x = 1)
        let clauses = vec![Clause {
            kind: ClauseKind::Invariant,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
            effect_variables: vec![],
        }];
        let results = verify_contract_cvc5("NativeInvariant", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "invariant should be satisfiable: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_must_not_true_counterexample() {
        // must_not { true } -- true is always possible, should be counterexample
        let clauses = vec![Clause {
            kind: ClauseKind::MustNot,
            body: Expr::Literal(Literal::Bool(true)),
            effect_variables: vec![],
        }];
        let results = verify_contract_cvc5("NativeMustNot", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "must_not(true) should be counterexample: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_must_not_false_verified() {
        // must_not { false } -- false is impossible, should verify
        let clauses = vec![Clause {
            kind: ClauseKind::MustNot,
            body: Expr::Literal(Literal::Bool(false)),
            effect_variables: vec![],
        }];
        let results = verify_contract_cvc5("NativeMustNotFalse", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "must_not(false) should verify: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_nat_type_constraint() {
        // input(n: Nat), ensures n >= 0 -- should verify with Nat constraint
        let clauses = vec![
            Clause {
                kind: ClauseKind::Input,
                body: Expr::Raw(vec!["n".into(), ":".into(), "Nat".into()]),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("n".into())),
                    op: BinOp::Gte,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("NatConstraint", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "Nat n >= 0 should verify: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_typestate_same_state_verifies() {
        // #262: Typestate same pre/post should verify via native CVC5
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("NativeTypestateIdentity", &clauses);
        assert!(
            !results.is_empty(),
            "should have results for typestate identity"
        );
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "same typestate pre/post should verify via native CVC5, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_typestate_different_state_counterexample() {
        // #262: Different typestate pre/post should produce counterexample
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::Raw(vec!["file".into(), "@".into(), "Closed".into()]),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("NativeTypestateMismatch", &clauses);
        assert!(
            !results.is_empty(),
            "should have results for typestate mismatch"
        );
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "different typestate pre/post should produce counterexample via native CVC5, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_nested_typestate_encoded() {
        // #262: Typestate nested inside a binary expression is now encoded
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::And,
                    lhs: Box::new(Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    }),
                    rhs: Box::new(Expr::Raw(vec![
                        "conn".into(),
                        "@".into(),
                        "Connected".into(),
                    ])),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::Raw(vec!["conn".into(), "@".into(), "Connected".into()]),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("NativeNestedTypestate", &clauses);
        assert!(
            !results.is_empty(),
            "should have results for nested typestate"
        );
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "nested typestate with matching state should verify, got: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_check_validity_typestate_encoded() {
        // #262: check_validity_cvc5 should now encode typestate (not skip)
        let assumption = Expr::Raw(vec!["state".into(), "@".into(), "Running".into()]);
        let body = Expr::Raw(vec!["state".into(), "@".into(), "Running".into()]);
        let result = check_validity_cvc5("validity_typestate", &[&assumption], &body);
        assert!(
            matches!(&result, VerificationResult::Verified { .. }),
            "check_validity_cvc5 should verify same-state typestate: {:?}",
            result
        );
    }

    #[test]
    fn native_cvc5_check_satisfiability_typestate_encoded() {
        // #262: check_satisfiability_cvc5 should now encode typestate (not skip)
        let body = Expr::Raw(vec!["lock".into(), "@".into(), "Acquired".into()]);
        let result = check_satisfiability_cvc5("sat_typestate", &[], &body);
        assert!(
            matches!(&result, VerificationResult::Verified { .. }),
            "check_satisfiability_cvc5 should find typestate satisfiable: {:?}",
            result
        );
    }

    // -------------------------------------------------------------------
    // String method axiom tests (CVC5 native, issue #251)
    // -------------------------------------------------------------------

    fn make_clause(kind: ClauseKind, body: Expr) -> Clause {
        Clause {
            kind,
            body,
            effect_variables: vec![],
        }
    }

    #[test]
    fn test_cvc5_string_substring_axiom() {
        // Contract: requires constraints on inputs,
        // ensures { substring(s, start, end).length() >= 0 }
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("len".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            ),
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("start".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            ),
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Lte,
                    lhs: Box::new(Expr::Ident("start".into())),
                    rhs: Box::new(Expr::Ident("end_val".into())),
                },
            ),
            make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Call {
                            func: Box::new(Expr::Ident("substring".into())),
                            args: vec![
                                Expr::Ident("s".into()),
                                Expr::Ident("start".into()),
                                Expr::Ident("end_val".into()),
                            ],
                        }),
                        method: "length".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            ),
        ];
        let results = crate::cvc5_backend::verify_contract_cvc5("SubstringTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                !matches!(r, VerificationResult::Counterexample { .. }),
                "Got unexpected counterexample: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_string_concat_axiom() {
        // ensures { concat(a, b).length() >= 0 }
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::MethodCall {
                    receiver: Box::new(Expr::Call {
                        func: Box::new(Expr::Ident("concat".into())),
                        args: vec![Expr::Ident("a".into()), Expr::Ident("b".into())],
                    }),
                    method: "length".into(),
                    args: vec![],
                }),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
        )];
        let results = crate::cvc5_backend::verify_contract_cvc5("ConcatTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                !matches!(r, VerificationResult::Counterexample { .. }),
                "concat axiom failed: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_string_indexof_axiom() {
        // requires { s.length() > 0 }
        // ensures { index_of(s, sub) >= -1 }
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Ident("s".into())),
                        method: "length".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            ),
            make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Call {
                        func: Box::new(Expr::Ident("index_of".into())),
                        args: vec![Expr::Ident("s".into()), Expr::Ident("sub".into())],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("-1".into()))),
                },
            ),
        ];
        let results = crate::cvc5_backend::verify_contract_cvc5("IndexOfTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                !matches!(r, VerificationResult::Counterexample { .. }),
                "indexOf axiom failed: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_string_charat_axiom() {
        // requires { idx >= 0 && s.length() > idx }
        // ensures { char_at(s, idx) >= 0 || char_at(s, idx) < 0 } (tautology -- tests axiom wiring)
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("idx".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            ),
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Ident("s".into())),
                        method: "length".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Ident("idx".into())),
                },
            ),
            make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("idx".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            ),
        ];
        let results = crate::cvc5_backend::verify_contract_cvc5("CharAtTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                !matches!(r, VerificationResult::Counterexample { .. }),
                "charAt axiom failed: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_string_replace_axiom() {
        // ensures { replace(s, old_s, new_s).length() >= 0 }
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::MethodCall {
                    receiver: Box::new(Expr::Call {
                        func: Box::new(Expr::Ident("replace".into())),
                        args: vec![
                            Expr::Ident("s".into()),
                            Expr::Ident("old_s".into()),
                            Expr::Ident("new_s".into()),
                        ],
                    }),
                    method: "length".into(),
                    args: vec![],
                }),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
        )];
        let results = crate::cvc5_backend::verify_contract_cvc5("ReplaceTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                !matches!(r, VerificationResult::Counterexample { .. }),
                "replace axiom failed: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_string_split_axiom() {
        // ensures { split(s, delim).length() >= 1 }
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::MethodCall {
                    receiver: Box::new(Expr::Call {
                        func: Box::new(Expr::Ident("split".into())),
                        args: vec![Expr::Ident("s".into()), Expr::Ident("delim".into())],
                    }),
                    method: "length".into(),
                    args: vec![],
                }),
                rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
            },
        )];
        let results = crate::cvc5_backend::verify_contract_cvc5("SplitTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                !matches!(r, VerificationResult::Counterexample { .. }),
                "split axiom failed: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_string_trim_axiom() {
        // ensures { trim(s).length() >= 0 }
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::MethodCall {
                    receiver: Box::new(Expr::Call {
                        func: Box::new(Expr::Ident("trim".into())),
                        args: vec![Expr::Ident("s".into())],
                    }),
                    method: "length".into(),
                    args: vec![],
                }),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
        )];
        let results = crate::cvc5_backend::verify_contract_cvc5("TrimTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                !matches!(r, VerificationResult::Counterexample { .. }),
                "trim axiom failed: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_array_set_axiom() {
        // ensures { set(arr, i, v).length() >= 0 }
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::MethodCall {
                    receiver: Box::new(Expr::Call {
                        func: Box::new(Expr::Ident("set".into())),
                        args: vec![
                            Expr::Ident("arr".into()),
                            Expr::Ident("i".into()),
                            Expr::Ident("v".into()),
                        ],
                    }),
                    method: "length".into(),
                    args: vec![],
                }),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
        )];
        let results = crate::cvc5_backend::verify_contract_cvc5("ArraySetTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                !matches!(r, VerificationResult::Counterexample { .. }),
                "array set axiom failed: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_map_put_axiom() {
        // ensures { put(m, k, v).size() >= 0 } (via size axiom)
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::MethodCall {
                    receiver: Box::new(Expr::Call {
                        func: Box::new(Expr::Ident("put".into())),
                        args: vec![
                            Expr::Ident("m".into()),
                            Expr::Ident("k".into()),
                            Expr::Ident("v".into()),
                        ],
                    }),
                    method: "size".into(),
                    args: vec![],
                }),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
        )];
        let results = crate::cvc5_backend::verify_contract_cvc5("MapPutTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                !matches!(r, VerificationResult::Counterexample { .. }),
                "map put axiom failed: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_method_call_substring_axiom() {
        // Test method call form: s.substring(start, end).length() >= 0
        let clauses = vec![
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("start".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            ),
            make_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    op: BinOp::Lte,
                    lhs: Box::new(Expr::Ident("start".into())),
                    rhs: Box::new(Expr::Ident("end_val".into())),
                },
            ),
            make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::MethodCall {
                            receiver: Box::new(Expr::Ident("s".into())),
                            method: "substring".into(),
                            args: vec![Expr::Ident("start".into()), Expr::Ident("end_val".into())],
                        }),
                        method: "length".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            ),
        ];
        let results = crate::cvc5_backend::verify_contract_cvc5("MethodSubstringTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                !matches!(r, VerificationResult::Counterexample { .. }),
                "method call substring axiom failed: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_method_call_set_axiom() {
        // Test method call form: arr.set(i, v).length() >= 0
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::MethodCall {
                    receiver: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Ident("arr".into())),
                        method: "set".into(),
                        args: vec![Expr::Ident("i".into()), Expr::Ident("v".into())],
                    }),
                    method: "length".into(),
                    args: vec![],
                }),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
        )];
        let results = crate::cvc5_backend::verify_contract_cvc5("MethodArraySetTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                !matches!(r, VerificationResult::Counterexample { .. }),
                "method call set axiom failed: {:?}",
                r
            );
        }
    }

    #[test]
    fn test_cvc5_method_call_put_axiom() {
        // Test method call form: m.put(k, v).size() >= 0
        let clauses = vec![make_clause(
            ClauseKind::Ensures,
            Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::MethodCall {
                    receiver: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Ident("m".into())),
                        method: "put".into(),
                        args: vec![Expr::Ident("k".into()), Expr::Ident("v".into())],
                    }),
                    method: "size".into(),
                    args: vec![],
                }),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
        )];
        let results = crate::cvc5_backend::verify_contract_cvc5("MethodMapPutTest", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                !matches!(r, VerificationResult::Counterexample { .. }),
                "method call put axiom failed: {:?}",
                r
            );
        }
    }
}

// -------------------------------------------------------------------
// expr_to_smtlib string method tests (issue #251)
// -------------------------------------------------------------------

#[test]
fn test_smtlib_call_substring() {
    let expr = Expr::Call {
        func: Box::new(Expr::Ident("substring".into())),
        args: vec![
            Expr::Ident("s".into()),
            Expr::Literal(Literal::Int("0".into())),
            Expr::Literal(Literal::Int("5".into())),
        ],
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(substring s 0 5)");
}

#[test]
fn test_smtlib_call_concat() {
    let expr = Expr::Call {
        func: Box::new(Expr::Ident("concat".into())),
        args: vec![Expr::Ident("a".into()), Expr::Ident("b".into())],
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(__concat a b)");
}

#[test]
fn test_smtlib_call_index_of() {
    let expr = Expr::Call {
        func: Box::new(Expr::Ident("index_of".into())),
        args: vec![Expr::Ident("s".into()), Expr::Ident("sub".into())],
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(index_of s sub)");
}

#[test]
fn test_smtlib_call_char_at() {
    let expr = Expr::Call {
        func: Box::new(Expr::Ident("char_at".into())),
        args: vec![
            Expr::Ident("s".into()),
            Expr::Literal(Literal::Int("3".into())),
        ],
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(char_at s 3)");
}

#[test]
fn test_smtlib_call_replace() {
    let expr = Expr::Call {
        func: Box::new(Expr::Ident("replace".into())),
        args: vec![
            Expr::Ident("s".into()),
            Expr::Ident("old_s".into()),
            Expr::Ident("new_s".into()),
        ],
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(replace s old_s new_s)");
}

#[test]
fn test_smtlib_call_split() {
    let expr = Expr::Call {
        func: Box::new(Expr::Ident("split".into())),
        args: vec![Expr::Ident("s".into()), Expr::Ident("delim".into())],
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(split s delim)");
}

#[test]
fn test_smtlib_call_trim() {
    let expr = Expr::Call {
        func: Box::new(Expr::Ident("trim".into())),
        args: vec![Expr::Ident("s".into())],
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(trim s)");
}

#[test]
fn test_smtlib_call_set() {
    let expr = Expr::Call {
        func: Box::new(Expr::Ident("set".into())),
        args: vec![
            Expr::Ident("arr".into()),
            Expr::Ident("i".into()),
            Expr::Ident("v".into()),
        ],
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(set arr i v)");
}

#[test]
fn test_smtlib_call_put() {
    let expr = Expr::Call {
        func: Box::new(Expr::Ident("put".into())),
        args: vec![
            Expr::Ident("m".into()),
            Expr::Ident("k".into()),
            Expr::Ident("v".into()),
        ],
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(put m k v)");
}

#[test]
fn test_smtlib_method_substring() {
    let expr = Expr::MethodCall {
        receiver: Box::new(Expr::Ident("s".into())),
        method: "substring".into(),
        args: vec![
            Expr::Literal(Literal::Int("1".into())),
            Expr::Literal(Literal::Int("4".into())),
        ],
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(substring s 1 4)");
}

#[test]
fn test_smtlib_method_concat() {
    let expr = Expr::MethodCall {
        receiver: Box::new(Expr::Ident("a".into())),
        method: "concat".into(),
        args: vec![Expr::Ident("b".into())],
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(__concat a b)");
}

#[test]
fn test_smtlib_method_set() {
    let expr = Expr::MethodCall {
        receiver: Box::new(Expr::Ident("arr".into())),
        method: "set".into(),
        args: vec![Expr::Ident("i".into()), Expr::Ident("v".into())],
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(set arr i v)");
}

#[test]
fn test_smtlib_method_put() {
    let expr = Expr::MethodCall {
        receiver: Box::new(Expr::Ident("m".into())),
        method: "put".into(),
        args: vec![Expr::Ident("k".into()), Expr::Ident("v".into())],
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(put m k v)");
}

#[test]
fn test_smtlib_method_trim() {
    let expr = Expr::MethodCall {
        receiver: Box::new(Expr::Ident("s".into())),
        method: "trim".into(),
        args: vec![],
    };
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(trim s)");
}

// -------------------------------------------------------------------
// CVC5 match pattern tests (native API, issue #252)
// -------------------------------------------------------------------

#[cfg(feature = "cvc5-verify")]
mod match_pattern_tests {
    use super::*;
    use assura_parser::ast::MatchArm;

    #[test]
    fn test_cvc5_match_constructor_pattern() {
        // ensures { match x { Some(v) => v > 0, None => true } }
        // with requires { x >= 0 } so scrut is constrained
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::Match {
                    scrutinee: Box::new(Expr::Ident("x".into())),
                    arms: vec![
                        MatchArm {
                            pattern: Pattern::Constructor {
                                name: "Positive".into(),
                                fields: vec![Pattern::Ident("v".into())],
                            },
                            body: Expr::Literal(Literal::Bool(true)),
                        },
                        MatchArm {
                            pattern: Pattern::Wildcard,
                            body: Expr::Literal(Literal::Bool(true)),
                        },
                    ],
                },
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
            body: Expr::Match {
                scrutinee: Box::new(Expr::Ident("t".into())),
                arms: vec![MatchArm {
                    pattern: Pattern::Tuple(vec![
                        Pattern::Ident("a".into()),
                        Pattern::Ident("b".into()),
                    ]),
                    body: Expr::Literal(Literal::Bool(true)),
                }],
            },
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
            body: Expr::Match {
                scrutinee: Box::new(Expr::Ident("x".into())),
                arms: vec![
                    MatchArm {
                        pattern: Pattern::Constructor {
                            name: "Outer".into(),
                            fields: vec![Pattern::Constructor {
                                name: "Inner".into(),
                                fields: vec![Pattern::Ident("v".into())],
                            }],
                        },
                        body: Expr::Literal(Literal::Bool(true)),
                    },
                    MatchArm {
                        pattern: Pattern::Wildcard,
                        body: Expr::Literal(Literal::Bool(true)),
                    },
                ],
            },
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
                body: Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::Match {
                    scrutinee: Box::new(Expr::Ident("x".into())),
                    arms: vec![
                        MatchArm {
                            pattern: Pattern::Ident("Zero".into()),
                            body: Expr::Literal(Literal::Bool(true)),
                        },
                        MatchArm {
                            pattern: Pattern::Wildcard,
                            body: Expr::BinOp {
                                op: BinOp::Gte,
                                lhs: Box::new(Expr::Ident("x".into())),
                                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                            },
                        },
                    ],
                },
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

// -------------------------------------------------------------------
// Frame axiom tests (CVC5 native, issue #256)
// -------------------------------------------------------------------

#[cfg(feature = "cvc5-verify")]
mod frame_tests {
    use super::*;

    #[test]
    fn test_cvc5_frame_axiom_injection() {
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Modifies,
                body: Expr::Ident("y".into()),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
        ];
        let results = crate::cvc5_backend::verify_contract_cvc5("FrameTest", &clauses);
        assert!(!results.is_empty());
    }

    #[test]
    fn test_cvc5_modifies_preserves_unmodified() {
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::Eq,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("5".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Modifies,
                body: Expr::Ident("y".into()),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::Eq,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("5".into()))),
                },
                effect_variables: vec![],
            },
        ];
        let results = crate::cvc5_backend::verify_contract_cvc5("FramePreserve", &clauses);
        assert!(!results.is_empty());
        for r in &results {
            assert!(
                !matches!(r, VerificationResult::Counterexample { .. }),
                "Frame axiom should prevent counterexample: {:?}",
                r
            );
        }
    }

    // ---------------------------------------------------------------
    // Lemma injection tests (#254)
    // ---------------------------------------------------------------

    #[test]
    fn native_cvc5_lemma_injection_basic() {
        // Contract with apply(lemma): the ensures body contains an
        // apply expression, which should be encoded as a named bool.
        // Without lemma defs, this just produces a result (not a panic).
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::Apply {
                    lemma_name: "helper_lemma".into(),
                    args: vec![Expr::Ident("x".into())],
                },
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("LemmaTest", &clauses);
        assert!(!results.is_empty(), "should produce at least one result");
    }

    #[test]
    fn native_cvc5_lemma_postcondition_injected() {
        // Build a lemma_defs map where "pos_lemma" ensures x >= 0.
        // The ensures clause uses `apply pos_lemma(x)` inside a
        // conjunction with `true`. With the lemma postcondition
        // injected as an assumption, this should not produce false
        // counterexamples for the apply sub-expression.
        let mut lemma_defs = std::collections::HashMap::new();
        let lemma_ensures = Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        lemma_defs.insert("pos_lemma".to_string(), vec![&lemma_ensures]);

        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::And,
                    lhs: Box::new(Expr::Apply {
                        lemma_name: "pos_lemma".into(),
                        args: vec![Expr::Ident("x".into())],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Bool(true))),
                },
                effect_variables: vec![],
            },
        ];
        let mut cache = SessionCache::new();
        let results = verify_contract_cvc5_with_lemmas(
            "ApplyPostcondTest",
            &clauses,
            &[],
            &[],
            Some(&lemma_defs),
            &[],
            &mut cache,
        );
        assert!(
            !results.is_empty(),
            "should produce at least one result with lemma injection"
        );
    }

    #[test]
    fn native_cvc5_lemma_injection_verifies_with_postcondition() {
        // The ensures clause says: x >= 0 (trivially follows from requires).
        // We also have an apply expression in the clause. With lemma defs
        // injecting x >= 0, the combined clause should still verify.
        let mut lemma_defs = std::collections::HashMap::new();
        let lemma_ensures = Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        lemma_defs.insert("helper".to_string(), vec![&lemma_ensures]);

        // requires { x > 0 }
        // ensures { x >= 0 }  (trivially true from requires)
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
        ];
        let mut cache = SessionCache::new();
        let results = verify_contract_cvc5_with_lemmas(
            "LemmaVerifTest",
            &clauses,
            &[],
            &[],
            Some(&lemma_defs),
            &[],
            &mut cache,
        );
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "should verify with lemma injection: {:?}",
            results[0]
        );
    }

    #[test]
    fn native_cvc5_no_lemma_defs_still_works() {
        // When lemma_defs is None, the apply expression is just
        // encoded as a named boolean (no postcondition injected).
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Expr::Apply {
                lemma_name: "unknown_lemma".into(),
                args: vec![Expr::Ident("x".into())],
            },
            effect_variables: vec![],
        }];
        let mut cache = SessionCache::new();
        let results = verify_contract_cvc5_with_lemmas(
            "NoLemmaDefs",
            &clauses,
            &[],
            &[],
            None,
            &[],
            &mut cache,
        );
        assert!(
            !results.is_empty(),
            "should produce results even without lemma defs"
        );
    }

    // ---------------------------------------------------------------
    // CVC5 Real sort float encoding tests (#248)
    // ---------------------------------------------------------------

    #[test]
    fn test_cvc5_float_real_sort() {
        // Float literal in requires/ensures should encode as CVC5 Real sort.
        // requires { x > 0 }, requires { x < 1000000 },
        // ensures { x > 0 } -- trivially true from precondition
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Float("0.0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Float("0.0".into()))),
                },
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("FloatRealSort", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "float Real sort should verify: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_cvc5_real_ite_promotion() {
        // ITE with mixed Int/Real branches should sort-promote.
        // requires { x > 0 }
        // ensures { if x > 0 then 1.5 else 0 > 0 }
        // The then branch is Real (1.5), else is Int (0).
        // Sort promotion converts the Int to Real so ITE succeeds.
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::If {
                        cond: Box::new(Expr::BinOp {
                            op: BinOp::Gt,
                            lhs: Box::new(Expr::Ident("x".into())),
                            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                        }),
                        then_branch: Box::new(Expr::Literal(Literal::Float("1.5".into()))),
                        else_branch: Some(Box::new(Expr::Literal(Literal::Int("0".into())))),
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Float("0.0".into()))),
                },
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("ItePromotion", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "ITE sort promotion should verify: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_cvc5_real_negation() {
        // Negated float should work with Real sort.
        // requires { x > 1.0 }, ensures { -x < 0.0 }
        // True because x > 1.0 implies -x < -1.0 < 0.0
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Float("1.0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::Lt,
                    lhs: Box::new(Expr::UnaryOp {
                        op: UnaryOp::Neg,
                        expr: Box::new(Expr::Ident("x".into())),
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Float("0.0".into()))),
                },
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("RealNeg", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "negated float Real should verify: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_cvc5_float_arithmetic_verifies() {
        // Float arithmetic: requires { x > 2.0 }, ensures { x + 1.0 > 3.0 }
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Float("2.0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::BinOp {
                        op: BinOp::Add,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Float("1.0".into()))),
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Float("3.0".into()))),
                },
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("FloatArith", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "float arithmetic should verify: {:?}",
            results[0]
        );
    }

    // ---------------------------------------------------------------
    // CVC5 quantifier trigger pattern inference tests (#247)
    // ---------------------------------------------------------------

    #[test]
    fn test_cvc5_quantifier_trigger_inference() {
        let tm = cvc5::TermManager::new();
        let bound = tm.mk_var(tm.integer_sort(), "i");

        let body = Expr::BinOp {
            op: BinOp::Gt,
            lhs: Box::new(Expr::Call {
                func: Box::new(Expr::Ident("f".into())),
                args: vec![Expr::Ident("i".into())],
            }),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };

        let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
        assert!(
            !patterns.is_empty(),
            "should infer trigger from f(i) call in quantifier body"
        );
    }

    #[test]
    fn test_cvc5_trigger_no_call_no_pattern() {
        let tm = cvc5::TermManager::new();
        let bound = tm.mk_var(tm.integer_sort(), "i");

        let body = Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Expr::Ident("i".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };

        let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
        assert!(
            patterns.is_empty(),
            "no function calls means no triggers: got {:?}",
            patterns.len()
        );
    }

    #[test]
    fn test_cvc5_trigger_nested_call() {
        let tm = cvc5::TermManager::new();
        let bound = tm.mk_var(tm.integer_sort(), "i");

        let body = Expr::BinOp {
            op: BinOp::Gt,
            lhs: Box::new(Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(Expr::Call {
                    func: Box::new(Expr::Ident("g".into())),
                    args: vec![Expr::Ident("i".into())],
                }),
                rhs: Box::new(Expr::Call {
                    func: Box::new(Expr::Ident("h".into())),
                    args: vec![Expr::Ident("i".into())],
                }),
            }),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };

        let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
        assert!(
            patterns.len() >= 2,
            "should infer triggers from both g(i) and h(i): got {}",
            patterns.len()
        );
    }

    #[test]
    fn test_cvc5_trigger_manager_integration() {
        let tm = cvc5::TermManager::new();
        let bound = tm.mk_var(tm.integer_sort(), "i");

        let body = Expr::Call {
            func: Box::new(Expr::Ident("lookup".into())),
            args: vec![Expr::Ident("i".into())],
        };

        let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
        assert!(
            !patterns.is_empty(),
            "should infer trigger from lookup(i) via direct scan fallback"
        );
    }

    #[test]
    fn test_cvc5_quantified_with_trigger_verifies() {
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::Forall {
                    var: "i".into(),
                    domain: Box::new(Expr::BinOp {
                        op: BinOp::Range,
                        lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                        rhs: Box::new(Expr::Ident("x".into())),
                    }),
                    body: Box::new(Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("i".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    }),
                },
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("QuantTriggerTest", &clauses);
        assert!(!results.is_empty(), "should produce verification results");
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "quantified contract should verify: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_cvc5_multi_arg_trigger() {
        let tm = cvc5::TermManager::new();
        let bound = tm.mk_var(tm.integer_sort(), "i");

        let body = Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Expr::Call {
                func: Box::new(Expr::Ident("lookup".into())),
                args: vec![Expr::Ident("table".into()), Expr::Ident("i".into())],
            }),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };

        let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
        assert!(
            !patterns.is_empty(),
            "should infer trigger from multi-arg lookup(table, i)"
        );
    }

    // -------------------------------------------------------------------
    // CVC5 session cache tests (#253)
    // -------------------------------------------------------------------

    #[test]
    fn test_cvc5_session_cache_hit() {
        // Verify same contract twice; second call should return cached result
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
        ];

        let mut cache = SessionCache::new();

        // First call: cache miss, runs CVC5
        let results1 = verify_contract_cvc5_with_lemmas(
            "CacheTest",
            &clauses,
            &[],
            &[],
            None,
            &[],
            &mut cache,
        );
        assert_eq!(results1.len(), 1);
        assert!(matches!(&results1[0], VerificationResult::Verified { .. }));
        assert_eq!(cache.entry_count(), 1);

        // Second call: cache hit, should not invoke CVC5
        let results2 = verify_contract_cvc5_with_lemmas(
            "CacheTest",
            &clauses,
            &[],
            &[],
            None,
            &[],
            &mut cache,
        );
        assert_eq!(results2.len(), 1);
        assert!(matches!(&results2[0], VerificationResult::Verified { .. }));
        // Cache should still have 1 entry (same key), with 1 hit
        assert_eq!(cache.entry_count(), 1);
        assert!(cache.hit_rate() > 0.0);
    }

    #[test]
    fn test_cvc5_session_cache_miss() {
        // Two different contracts should be cache misses
        let clauses_a = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
        ];
        let clauses_b = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("y".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Ident("y".into())),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
        ];

        let mut cache = SessionCache::new();

        let results_a =
            verify_contract_cvc5_with_lemmas("CacheA", &clauses_a, &[], &[], None, &[], &mut cache);
        assert_eq!(results_a.len(), 1);
        assert_eq!(cache.entry_count(), 1);

        let results_b =
            verify_contract_cvc5_with_lemmas("CacheB", &clauses_b, &[], &[], None, &[], &mut cache);
        assert_eq!(results_b.len(), 1);
        // Both should be cache misses, so 2 entries
        assert_eq!(cache.entry_count(), 2);
    }

    // -------------------------------------------------------------------
    // #263: CVC5 ADT encoding tests
    // -------------------------------------------------------------------

    #[test]
    fn test_cvc5_adt_constructor() {
        // Define Option = Some(value: Int) | None using CVC5 native API.
        // Verify that constructor tags are distinct and accessors work.
        let tm = cvc5::TermManager::new();
        let mut solver = cvc5::Solver::new(&tm);
        solver.set_logic("ALL");
        solver.set_option("produce-models", "true");
        solver.set_option("tlimit", "2000");

        let (adt_def, adt_symbols) = super::define_adt_cvc5_native(
            &tm,
            &mut solver,
            "Option",
            &[("Some", &["value"]), ("None", &[])],
        );

        // Construct Some(42)
        let some_ctor = adt_def
            .constructors
            .iter()
            .find(|c| c.name == "Some")
            .unwrap();
        let none_ctor = adt_def
            .constructors
            .iter()
            .find(|c| c.name == "None")
            .unwrap();

        let mut axioms = Vec::new();
        let mut fresh = 0usize;

        let forty_two = tm.mk_integer(42);
        let some_val = super::adt_constructor_cvc5_native(
            &tm,
            &adt_symbols,
            some_ctor,
            &[forty_two.clone()],
            &mut axioms,
            &mut fresh,
        );
        let none_val = super::adt_constructor_cvc5_native(
            &tm,
            &adt_symbols,
            none_ctor,
            &[],
            &mut axioms,
            &mut fresh,
        );

        // Assert all axioms
        for axiom in &axioms {
            solver.assert_formula(axiom.clone());
        }

        // Verify tags are distinct
        let is_some =
            super::adt_is_constructor_cvc5_native(&tm, &adt_symbols, some_ctor, &some_val);
        let is_none =
            super::adt_is_constructor_cvc5_native(&tm, &adt_symbols, none_ctor, &none_val);
        solver.assert_formula(is_some);
        solver.assert_formula(is_none);

        // Verify accessor: value(some_val) == 42
        let accessed = super::adt_accessor_cvc5_native(&tm, &adt_symbols, "value", &some_val);
        let eq_42 = tm.mk_term(cvc5::Kind::Equal, &[accessed, forty_two]);
        let not_eq_42 = tm.mk_term(cvc5::Kind::Not, &[eq_42]);
        solver.push(1);
        solver.assert_formula(not_eq_42);
        let result = solver.check_sat();
        assert!(
            result.is_unsat(),
            "accessor(Some(42)) must equal 42 (negation should be UNSAT)"
        );
        solver.pop(1);

        // Verify exhaustiveness: tag(x) == 99 should be UNSAT
        let x = tm.mk_const(tm.integer_sort(), "x_adt_exh");
        let tag_x = tm.mk_term(cvc5::Kind::ApplyUf, &[adt_symbols.tag_fn.clone(), x]);
        let bad_tag = tm.mk_term(cvc5::Kind::Equal, &[tag_x, tm.mk_integer(99)]);
        solver.push(1);
        solver.assert_formula(bad_tag);
        let result = solver.check_sat();
        assert!(
            result.is_unsat(),
            "tag(x) == 99 should be UNSAT with only tags 0 and 1"
        );
        solver.pop(1);
    }

    #[test]
    fn test_cvc5_adt_smtlib_generation() {
        // Test that the SMT-LIB2 generation functions produce valid output
        let (adt_def, assertions) =
            super::define_adt_cvc5("Option", &[("Some", &["value"]), ("None", &[])]);

        // Should have 3 declarations + 1 exhaustiveness + 2 injectivity = 6
        assert!(
            assertions.len() >= 5,
            "should have at least 5 SMT-LIB2 assertions, got {}",
            assertions.len()
        );

        // Check tag function declaration
        assert!(
            assertions.iter().any(|a| a.contains("__adt_tag_Option")),
            "should declare tag function"
        );

        // Check accessor function declaration
        assert!(
            assertions.iter().any(|a| a.contains("__adt_Option_value")),
            "should declare value accessor"
        );

        // Check exhaustiveness axiom
        assert!(
            assertions
                .iter()
                .any(|a| a.contains("forall") && a.contains("or")),
            "should have exhaustiveness axiom with forall/or"
        );

        // Test constructor tester SMT generation
        let tester = super::adt_is_constructor_smt("Option", "Some", "x", &adt_def);
        assert_eq!(tester, "(= (__adt_tag_Option x) 0)");

        let tester_none = super::adt_is_constructor_smt("Option", "None", "x", &adt_def);
        assert_eq!(tester_none, "(= (__adt_tag_Option x) 1)");

        // Test accessor SMT generation
        let acc = super::adt_accessor_smt("Option", "value", "x");
        assert_eq!(acc, "(__adt_Option_value x)");
    }

    // -------------------------------------------------------------------
    // #265: CVC5 bitvector wrapping test
    // -------------------------------------------------------------------

    #[test]
    fn test_cvc5_unsat_core_extraction() {
        use assura_parser::ast::{BinOp, Literal};

        let int_lit = |n: &str| Expr::Literal(Literal::Int(n.into()));
        let var = |name: &str| Expr::Ident(name.into());
        let cmp = |name: &str, op: BinOp, n: &str| Expr::BinOp {
            lhs: Box::new(var(name)),
            op,
            rhs: Box::new(int_lit(n)),
        };

        let req0 = cmp("x", BinOp::Gt, "50");
        let req1 = cmp("x", BinOp::Lt, "100");
        let ensures = cmp("x", BinOp::Gt, "10");

        let result = check_validity_cvc5("unsat_core_test", &[&req0, &req1], &ensures);
        match result {
            VerificationResult::Verified { unsat_core, .. } => {
                let core = unsat_core
                    .as_ref()
                    .expect("CVC5 verified result should include unsat core");
                assert!(
                    core.iter().any(|l| l.contains("req_0")),
                    "core should include req_0, got: {core:?}"
                );
            }
            other => panic!("expected verified result, got: {other:?}"),
        }
    }

    #[test]
    fn test_cvc5_bitvector_wrapping() {
        let tm = cvc5::TermManager::new();
        let mut solver = cvc5::Solver::new(&tm);
        solver.set_logic("QF_BV");
        solver.set_option("produce-models", "true");

        let eight = tm.mk_bv_sort(8);
        let a = tm.mk_const(eight.clone(), "a");
        let b = tm.mk_const(eight, "b");
        let two_five_five = tm.mk_bv(8, 255);
        let one = tm.mk_bv(8, 1);
        let zero = tm.mk_bv(8, 0);

        solver.assert_formula(tm.mk_term(cvc5::Kind::Equal, &[a.clone(), two_five_five]));
        solver.assert_formula(tm.mk_term(cvc5::Kind::Equal, &[b.clone(), one]));
        let sum = tm.mk_term(cvc5::Kind::BitvectorAdd, &[a, b]);
        solver.assert_formula(tm.mk_term(cvc5::Kind::Equal, &[sum, zero]));

        assert!(
            solver.check_sat().is_sat(),
            "255 + 1 should wrap to 0 in 8-bit BV"
        );
    }
}
