use crate::VerificationResult;
use crate::cvc5_backend::{
    collect_vars, derive_narrowings, expr_to_smtlib, parse_smtlib_model, verify_contract_cvc5,
};
#[cfg(feature = "cvc5-verify")]
use crate::cvc5_quantifier_encode::infer_quantifier_patterns_cvc5;
use crate::encode_atom_policy::is_internal_encoder_var;
use crate::lemma_inject_policy::collect_apply_refs_from_expr;
use crate::unmodelable::{
    collect_unmodelable_reasons, expr_has_unmodelable_features, field_chain_depth_sp,
    flatten_field_chain_sp, has_deep_field_chain_sp, is_self_rooted_sp,
};
use assura_ast::{BinOp, Clause, ClauseKind, Expr, Literal, Pattern, Spanned, UnaryOp};
use std::collections::HashSet;

// -------------------------------------------------------------------
// derive_narrowings tests (#257)
// -------------------------------------------------------------------

#[test]
fn test_derive_narrowings_basic() {
    let narrowings = derive_narrowings(&[("max_size".into(), 100)]);
    assert_eq!(narrowings.len(), 1);
    assert_eq!(narrowings[0], ("size".into(), 100));
}

#[test]
fn test_derive_narrowings_empty() {
    let narrowings = derive_narrowings(&[]);
    assert!(narrowings.is_empty());
}

#[test]
fn test_derive_narrowings_no_prefix() {
    let narrowings = derive_narrowings(&[("size".into(), 50)]);
    assert!(narrowings.is_empty());
}

#[test]
fn test_derive_narrowings_uppercase_prefix() {
    let narrowings = derive_narrowings(&[("MAX_BUFFER".into(), 1024)]);
    assert_eq!(narrowings.len(), 2);
    assert_eq!(narrowings[0], ("BUFFER".into(), 1024));
    assert_eq!(narrowings[1], ("buffer".into(), 1024));
}

#[test]
fn test_derive_narrowings_multiple() {
    let narrowings = derive_narrowings(&[
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
    let expr = Spanned::no_span(Expr::Literal(Literal::Int("42".into())));
    assert_eq!(expr_to_smtlib(&expr), Some("42".into()));
}

#[test]
fn test_smtlib_int_negative() {
    let expr = Spanned::no_span(Expr::Literal(Literal::Int("-7".into())));
    assert_eq!(expr_to_smtlib(&expr), Some("(- 7)".into()));
}

#[test]
fn test_smtlib_bool_true() {
    let expr = Spanned::no_span(Expr::Literal(Literal::Bool(true)));
    assert_eq!(expr_to_smtlib(&expr), Some("true".into()));
}

#[test]
fn test_smtlib_bool_false() {
    let expr = Spanned::no_span(Expr::Literal(Literal::Bool(false)));
    assert_eq!(expr_to_smtlib(&expr), Some("false".into()));
}

#[test]
fn test_smtlib_string_encodes_as_named_const() {
    let expr = Spanned::no_span(Expr::Literal(Literal::Str("hello".into())));
    assert_eq!(expr_to_smtlib(&expr), Some("__str_hello".into()));
}

#[test]
fn test_smtlib_ident() {
    let expr = Spanned::no_span(Expr::Ident("x".into()));
    assert_eq!(expr_to_smtlib(&expr), Some("x".into()));
}

#[test]
fn test_smtlib_result_keyword() {
    let expr = Spanned::no_span(Expr::Ident("result".into()));
    assert_eq!(expr_to_smtlib(&expr), Some("__result".into()));
}

#[test]
fn test_smtlib_dotted_ident_sanitized() {
    let expr = Spanned::no_span(Expr::Ident("state.field".into()));
    assert_eq!(expr_to_smtlib(&expr), Some("state_field".into()));
}

#[test]
fn test_smtlib_binop_add() {
    let expr = Spanned::no_span(Expr::BinOp {
        op: BinOp::Add,
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
    });
    assert_eq!(expr_to_smtlib(&expr), Some("(+ x 1)".into()));
}

#[test]
fn test_smtlib_binop_neq() {
    let expr = Spanned::no_span(Expr::BinOp {
        op: BinOp::Neq,
        lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
    });
    assert_eq!(expr_to_smtlib(&expr), Some("(not (= a b))".into()));
}

#[test]
fn test_smtlib_binop_div_is_integer() {
    let expr = Spanned::no_span(Expr::BinOp {
        op: BinOp::Div,
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
    });
    assert_eq!(expr_to_smtlib(&expr), Some("(div x y)".into()));
}

#[test]
fn test_smtlib_binop_implies() {
    let expr = Spanned::no_span(Expr::BinOp {
        op: BinOp::Implies,
        lhs: Box::new(Spanned::no_span(Expr::Ident("p".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Ident("q".into()))),
    });
    assert_eq!(expr_to_smtlib(&expr), Some("(=> p q)".into()));
}

#[test]
fn test_smtlib_binop_range_encodes() {
    let expr = Spanned::no_span(Expr::BinOp {
        op: BinOp::Range,
        lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
    });
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
    let expr = Spanned::no_span(Expr::BinOp {
        op: BinOp::In,
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Ident("collection".into()))),
    });
    let s = expr_to_smtlib(&expr).expect("In should encode");
    assert!(s.contains("__contains"), "missing contains UF in: {s}");
    assert!(s.contains("collection"), "missing collection in: {s}");
    assert!(s.contains("x"), "missing element in: {s}");
}

#[test]
fn test_smtlib_binop_notin() {
    let expr = Spanned::no_span(Expr::BinOp {
        op: BinOp::NotIn,
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Ident("items".into()))),
    });
    let s = expr_to_smtlib(&expr).expect("NotIn should encode");
    assert!(s.contains("not"), "missing negation in NotIn: {s}");
    assert!(
        s.contains("__contains"),
        "missing contains UF in NotIn: {s}"
    );
}

#[test]
fn test_smtlib_binop_concat() {
    let expr = Spanned::no_span(Expr::BinOp {
        op: BinOp::Concat,
        lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
    });
    let s = expr_to_smtlib(&expr).expect("Concat should encode");
    assert!(s.contains("__concat"), "missing concat UF in: {s}");
    assert!(s.contains("a"), "missing lhs in concat: {s}");
    assert!(s.contains("b"), "missing rhs in concat: {s}");
}

#[test]
fn test_smtlib_unary_not() {
    let expr = Spanned::no_span(Expr::UnaryOp {
        op: UnaryOp::Not,
        expr: Box::new(Spanned::no_span(Expr::Ident("flag".into()))),
    });
    assert_eq!(expr_to_smtlib(&expr), Some("(not flag)".into()));
}

#[test]
fn test_smtlib_unary_neg() {
    let expr = Spanned::no_span(Expr::UnaryOp {
        op: UnaryOp::Neg,
        expr: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
    });
    assert_eq!(expr_to_smtlib(&expr), Some("(- x)".into()));
}

#[test]
fn test_smtlib_if_with_else() {
    let expr = Spanned::no_span(Expr::If {
        cond: Box::new(Spanned::no_span(Expr::Ident("c".into()))),
        then_branch: Box::new(Spanned::no_span(Expr::Ident("t".into()))),
        else_branch: Some(Box::new(Spanned::no_span(Expr::Ident("e".into())))),
    });
    assert_eq!(expr_to_smtlib(&expr), Some("(ite c t e)".into()));
}

#[test]
fn test_smtlib_if_without_else() {
    let expr = Spanned::no_span(Expr::If {
        cond: Box::new(Spanned::no_span(Expr::Ident("p".into()))),
        then_branch: Box::new(Spanned::no_span(Expr::Ident("q".into()))),
        else_branch: None,
    });
    assert_eq!(expr_to_smtlib(&expr), Some("(=> p q)".into()));
}

#[test]
fn test_smtlib_forall_non_range_domain() {
    // Non-range domain should produce __domain_contains guard
    let expr = Spanned::no_span(Expr::Forall {
        var: "i".into(),
        domain: Box::new(Spanned::no_span(Expr::Ident("xs".into()))),
        body: Box::new(Spanned::no_span(Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Spanned::no_span(Expr::Ident("i".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        })),
    });
    assert_eq!(
        expr_to_smtlib(&expr),
        Some("(forall ((i Int)) (=> (__domain_contains xs i) (>= i 0)))".into())
    );
}

#[test]
fn test_smtlib_exists_non_range_domain() {
    // Non-range domain should produce __domain_contains guard
    let expr = Spanned::no_span(Expr::Exists {
        var: "x".into(),
        domain: Box::new(Spanned::no_span(Expr::Ident("S".into()))),
        body: Box::new(Spanned::no_span(Expr::BinOp {
            op: BinOp::Eq,
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        })),
    });
    assert_eq!(
        expr_to_smtlib(&expr),
        Some("(exists ((x Int)) (and (__domain_contains S x) (= x 0)))".into())
    );
}

#[test]
fn test_smtlib_forall_range_domain() {
    // forall x in 0..10 { x >= 0 } should produce range guard
    let expr = Spanned::no_span(Expr::Forall {
        var: "x".into(),
        domain: Box::new(Spanned::no_span(Expr::BinOp {
            op: BinOp::Range,
            lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
        })),
        body: Box::new(Spanned::no_span(Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        })),
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(
        s,
        "(forall ((x Int)) (=> (and (>= x 0) (< x 10)) (>= x 0)))"
    );
}

#[test]
fn test_smtlib_exists_range_domain() {
    // exists x in 0..10 { x == 5 } should produce range guard with conjunction
    let expr = Spanned::no_span(Expr::Exists {
        var: "x".into(),
        domain: Box::new(Spanned::no_span(Expr::BinOp {
            op: BinOp::Range,
            lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
        })),
        body: Box::new(Spanned::no_span(Expr::BinOp {
            op: BinOp::Eq,
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
        })),
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(
        s,
        "(exists ((x Int)) (and (and (>= x 0) (< x 10)) (= x 5)))"
    );
}

#[test]
fn test_smtlib_forall_range_variable_bounds() {
    // forall i in 0..n { i >= 0 } -- variable upper bound
    let expr = Spanned::no_span(Expr::Forall {
        var: "i".into(),
        domain: Box::new(Spanned::no_span(Expr::BinOp {
            op: BinOp::Range,
            lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            rhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        })),
        body: Box::new(Spanned::no_span(Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Spanned::no_span(Expr::Ident("i".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        })),
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(forall ((i Int)) (=> (and (>= i 0) (< i n)) (>= i 0)))");
}

#[test]
fn test_smtlib_call_no_args() {
    let expr = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("foo".into()))),
        args: vec![],
    });
    assert_eq!(expr_to_smtlib(&expr), Some("foo".into()));
}

#[test]
fn test_smtlib_call_with_args() {
    let expr = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("f".into()))),
        args: vec![
            Spanned::no_span(Expr::Ident("x".into())),
            Spanned::no_span(Expr::Ident("y".into())),
        ],
    });
    assert_eq!(expr_to_smtlib(&expr), Some("(f x y)".into()));
}

#[test]
fn test_smtlib_old_adds_suffix() {
    let expr = Spanned::no_span(Expr::Old(Box::new(Spanned::no_span(Expr::Ident(
        "x".into(),
    )))));
    assert_eq!(expr_to_smtlib(&expr), Some("x__old".into()));
}

#[test]
fn test_smtlib_literal_int() {
    let expr = Spanned::no_span(Expr::Literal(Literal::Int("5".into())));
    assert_eq!(expr_to_smtlib(&expr), Some("5".into()));
}

#[test]
fn test_smtlib_raw_single_token() {
    let expr = Spanned::no_span(Expr::Raw(vec!["foo".into()]));
    assert_eq!(expr_to_smtlib(&expr), Some("foo".into()));
    // Integer token
    let expr_int = Spanned::no_span(Expr::Raw(vec!["42".into()]));
    assert_eq!(expr_to_smtlib(&expr_int), Some("42".into()));
    // Bool token
    let expr_bool = Spanned::no_span(Expr::Raw(vec!["true".into()]));
    assert_eq!(expr_to_smtlib(&expr_bool), Some("true".into()));
}

#[test]
fn test_smtlib_raw_precedence_climbing() {
    // "a + b * c" should parse as (+ a (* b c)) due to precedence
    let expr = Spanned::no_span(Expr::Raw(vec![
        "a".into(),
        "+".into(),
        "b".into(),
        "*".into(),
        "c".into(),
    ]));
    assert_eq!(expr_to_smtlib(&expr), Some("(+ a (* b c))".into()));
}

#[test]
fn test_smtlib_raw_parentheses() {
    // "(a + b) * c" should parse as (* (+ a b) c)
    let expr = Spanned::no_span(Expr::Raw(vec![
        "(".into(),
        "a".into(),
        "+".into(),
        "b".into(),
        ")".into(),
        "*".into(),
        "c".into(),
    ]));
    assert_eq!(expr_to_smtlib(&expr), Some("(* (+ a b) c)".into()));
}

#[test]
fn test_smtlib_raw_old_expression() {
    // "old ( x ) + 1" should parse old(x) + 1
    let expr = Spanned::no_span(Expr::Raw(vec![
        "old".into(),
        "(".into(),
        "x".into(),
        ")".into(),
        "+".into(),
        "1".into(),
    ]));
    assert_eq!(expr_to_smtlib(&expr), Some("(+ x__old 1)".into()));
}

#[test]
fn test_smtlib_raw_nested_operators() {
    // "a + b - c + d" left-associative: (+ (- (+ a b) c) d)
    let expr = Spanned::no_span(Expr::Raw(vec![
        "a".into(),
        "+".into(),
        "b".into(),
        "-".into(),
        "c".into(),
        "+".into(),
        "d".into(),
    ]));
    let result = expr_to_smtlib(&expr).unwrap();
    // Left-associative: ((a + b) - c) + d
    assert_eq!(result, "(+ (- (+ a b) c) d)");
}

#[test]
fn test_smtlib_raw_comparison_chain() {
    // "a < b < c" desugars to (and (< a b) (< b c))
    let expr = Spanned::no_span(Expr::Raw(vec![
        "a".into(),
        "<".into(),
        "b".into(),
        "<".into(),
        "c".into(),
    ]));
    assert_eq!(expr_to_smtlib(&expr), Some("(and (< a b) (< b c))".into()));
}

#[test]
fn test_smtlib_raw_unary_not() {
    // "! x" -> (not x)
    let expr = Spanned::no_span(Expr::Raw(vec!["!".into(), "x".into()]));
    assert_eq!(expr_to_smtlib(&expr), Some("(not x)".into()));
}

#[test]
fn test_smtlib_raw_unary_neg() {
    // "- x" -> (- x)
    let expr = Spanned::no_span(Expr::Raw(vec!["-".into(), "x".into()]));
    assert_eq!(expr_to_smtlib(&expr), Some("(- x)".into()));
}

#[test]
fn test_smtlib_raw_logical_ops() {
    // "a && b || c" should respect precedence: (or (and a b) c)
    let expr = Spanned::no_span(Expr::Raw(vec![
        "a".into(),
        "&&".into(),
        "b".into(),
        "||".into(),
        "c".into(),
    ]));
    assert_eq!(expr_to_smtlib(&expr), Some("(or (and a b) c)".into()));
}

#[test]
fn test_smtlib_raw_neq() {
    // "a != b" -> (not (= a b))
    let expr = Spanned::no_span(Expr::Raw(vec!["a".into(), "!=".into(), "b".into()]));
    assert_eq!(expr_to_smtlib(&expr), Some("(not (= a b))".into()));
}

#[test]
fn test_smtlib_raw_mod_div() {
    // "a mod b" and "a div b"
    let expr_mod = Spanned::no_span(Expr::Raw(vec!["a".into(), "mod".into(), "b".into()]));
    assert_eq!(expr_to_smtlib(&expr_mod), Some("(mod a b)".into()));

    let expr_div = Spanned::no_span(Expr::Raw(vec!["a".into(), "div".into(), "b".into()]));
    assert_eq!(expr_to_smtlib(&expr_div), Some("(div a b)".into()));
}

#[test]
fn test_smtlib_raw_complex_expression() {
    // "x >= 0 && x < max" -> (and (>= x 0) (< x max))
    let expr = Spanned::no_span(Expr::Raw(vec![
        "x".into(),
        ">=".into(),
        "0".into(),
        "&&".into(),
        "x".into(),
        "<".into(),
        "max".into(),
    ]));
    assert_eq!(
        expr_to_smtlib(&expr),
        Some("(and (>= x 0) (< x max))".into())
    );
}

#[test]
fn test_smtlib_raw_function_call() {
    // "abs ( x )" -> (abs x)
    let expr = Spanned::no_span(Expr::Raw(vec![
        "abs".into(),
        "(".into(),
        "x".into(),
        ")".into(),
    ]));
    assert_eq!(expr_to_smtlib(&expr), Some("(abs x)".into()));
}

#[test]
fn test_smtlib_let_expr() {
    let expr = Spanned::no_span(Expr::Let {
        name: "x".into(),
        value: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
        body: Box::new(Spanned::no_span(Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        })),
    });
    assert_eq!(expr_to_smtlib(&expr), Some("(let ((x 5)) (+ x 1))".into()));
}

#[test]
fn test_smtlib_match_with_literal_and_wildcard() {
    use assura_ast::MatchArm;
    let expr = Spanned::no_span(Expr::Match {
        scrutinee: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        arms: vec![
            MatchArm {
                pattern: Pattern::Literal(Literal::Int("0".into())),
                body: Spanned::no_span(Expr::Literal(Literal::Int("1".into()))),
            },
            MatchArm {
                pattern: Pattern::Wildcard,
                body: Spanned::no_span(Expr::Ident("n".into())),
            },
        ],
    });
    assert_eq!(expr_to_smtlib(&expr), Some("(ite (= n 0) 1 n)".into()));
}

#[test]
fn test_smtlib_match_empty_arms() {
    let expr = Spanned::no_span(Expr::Match {
        scrutinee: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        arms: vec![],
    });
    assert_eq!(expr_to_smtlib(&expr), None);
}

#[test]
fn test_smtlib_match_constructor_pattern() {
    use assura_ast::MatchArm;
    // match x { Some(v) => v, None => 0 }
    let expr = Spanned::no_span(Expr::Match {
        scrutinee: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        arms: vec![
            MatchArm {
                pattern: Pattern::Constructor {
                    name: "Some".into(),
                    fields: vec![Pattern::Ident("v".into())],
                },
                body: Spanned::no_span(Expr::Ident("v".into())),
            },
            MatchArm {
                pattern: Pattern::Constructor {
                    name: "None".into(),
                    fields: vec![],
                },
                body: Spanned::no_span(Expr::Literal(Literal::Int("0".into()))),
            },
            MatchArm {
                pattern: Pattern::Wildcard,
                body: Spanned::no_span(Expr::Literal(Literal::Int("0".into()))),
            },
        ],
    });
    let smt = expr_to_smtlib(&expr).expect("should encode constructor match");
    // #263: Constructor patterns use ADT tag testers, not pattern hashes.
    assert!(smt.contains("__adt_tag_Option"));
    assert!(smt.contains("(= (__adt_tag_Option x) 0)")); // Some
    assert!(smt.contains("(= (__adt_tag_Option x) 1)")); // None
    assert!(smt.contains("ite"));
}

#[test]
fn test_smtlib_match_tuple_pattern() {
    use assura_ast::MatchArm;
    // match t { (a, b) => a }
    let expr = Spanned::no_span(Expr::Match {
        scrutinee: Box::new(Spanned::no_span(Expr::Ident("t".into()))),
        arms: vec![MatchArm {
            pattern: Pattern::Tuple(vec![Pattern::Ident("a".into()), Pattern::Ident("b".into())]),
            body: Spanned::no_span(Expr::Ident("a".into())),
        }],
    });
    let smt = expr_to_smtlib(&expr).expect("should encode tuple match");
    // Tuple is structural, body is just "a"
    assert_eq!(smt, "a");
}

#[test]
fn test_smtlib_match_ident_constructor_like() {
    use assura_ast::MatchArm;
    // match x { None => 1, _ => 0 }  (Ident "None" uppercase = constructor)
    let expr = Spanned::no_span(Expr::Match {
        scrutinee: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        arms: vec![
            MatchArm {
                pattern: Pattern::Ident("None".into()),
                body: Spanned::no_span(Expr::Literal(Literal::Int("1".into()))),
            },
            MatchArm {
                pattern: Pattern::Wildcard,
                body: Spanned::no_span(Expr::Literal(Literal::Int("0".into()))),
            },
        ],
    });
    let smt = expr_to_smtlib(&expr).expect("should encode ident-as-constructor match");
    let none_hash = crate::encode_method_policy::pattern_hash_name("None");
    assert!(smt.contains(&none_hash.to_string()));
    assert!(smt.contains("ite"));
}

// -------------------------------------------------------------------
// collect_vars tests
// -------------------------------------------------------------------

#[test]
fn test_collect_vars_ident() {
    let mut vars = HashSet::new();
    collect_vars(&Spanned::no_span(Expr::Ident("x".into())), &mut vars);
    assert!(vars.contains("x"));
}

#[test]
fn test_collect_vars_result() {
    let mut vars = HashSet::new();
    collect_vars(&Spanned::no_span(Expr::Ident("result".into())), &mut vars);
    assert!(vars.contains("__result"));
    assert!(!vars.contains("result"));
}

#[test]
fn test_collect_vars_binop() {
    let mut vars = HashSet::new();
    let expr = Spanned::no_span(Expr::BinOp {
        op: BinOp::Add,
        lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
    });
    collect_vars(&expr, &mut vars);
    assert!(vars.contains("a"));
    assert!(vars.contains("b"));
}

#[test]
fn test_collect_vars_if_all_branches() {
    let mut vars = HashSet::new();
    let expr = Spanned::no_span(Expr::If {
        cond: Box::new(Spanned::no_span(Expr::Ident("c".into()))),
        then_branch: Box::new(Spanned::no_span(Expr::Ident("t".into()))),
        else_branch: Some(Box::new(Spanned::no_span(Expr::Ident("e".into())))),
    });
    collect_vars(&expr, &mut vars);
    assert!(vars.contains("c"));
    assert!(vars.contains("t"));
    assert!(vars.contains("e"));
}

#[test]
fn test_collect_vars_literal_no_vars() {
    let mut vars = HashSet::new();
    collect_vars(
        &Spanned::no_span(Expr::Literal(Literal::Int("42".into()))),
        &mut vars,
    );
    assert!(vars.is_empty());
}

#[test]
fn test_collect_vars_dotted_sanitized() {
    let mut vars = HashSet::new();
    collect_vars(
        &Spanned::no_span(Expr::Ident("obj.field".into())),
        &mut vars,
    );
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
// is_internal_encoder_var and counterexample model filtering (#260)
// -------------------------------------------------------------------

#[test]
fn test_is_internal_encoder_var_internal_prefixes() {
    assert!(is_internal_encoder_var("__str_hello"));
    assert!(is_internal_encoder_var("__tuple_0"));
    assert!(is_internal_encoder_var("__list_vals"));
    assert!(is_internal_encoder_var("__fresh_3"));
    assert!(is_internal_encoder_var("__field_len"));
    assert!(is_internal_encoder_var("__index_0"));
    assert!(is_internal_encoder_var("__len_buf"));
    assert!(is_internal_encoder_var("__arr_data"));
    assert!(is_internal_encoder_var("__domain_contains_x"));
    assert!(is_internal_encoder_var("__apply_func"));
    assert!(is_internal_encoder_var("__coerce_1"));
    assert!(is_internal_encoder_var("__trigger_pat"));
    assert!(is_internal_encoder_var("__list_get_0"));
    assert!(is_internal_encoder_var("__result"));
    assert!(is_internal_encoder_var("__contains"));
    assert!(is_internal_encoder_var("__obj_ptr"));
}

#[test]
fn test_is_internal_encoder_var_user_variables() {
    assert!(!is_internal_encoder_var("x"));
    assert!(!is_internal_encoder_var("buffer_size"));
    assert!(!is_internal_encoder_var("payload_length"));
    assert!(!is_internal_encoder_var("n"));
    assert!(!is_internal_encoder_var("result_count"));
    assert!(!is_internal_encoder_var("max_size"));
    assert!(!is_internal_encoder_var("i"));
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
    // `__result` is contract `result`; cleaned to `result` by counterexample_display_name.
    assert_eq!(names, vec!["result", "x", "y"]);
    assert!(!names.contains(&"__str_hello"));
    assert!(!names.contains(&"__field_len"));
    assert!(!names.contains(&"__fresh_0"));
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
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Ident("obj".into()))),
        "field".into(),
    ));
    let mut vars = HashSet::new();
    collect_vars(&expr, &mut vars);
    assert!(vars.contains("obj"));
}

#[test]
fn collect_vars_method_call() {
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(Spanned::no_span(Expr::Ident("list".into()))),
        method: "len".into(),
        args: vec![Spanned::no_span(Expr::Ident("idx".into()))],
    });
    let mut vars = HashSet::new();
    collect_vars(&expr, &mut vars);
    assert!(vars.contains("list"));
    assert!(vars.contains("idx"));
}

#[test]
fn collect_vars_index() {
    let expr = Spanned::no_span(Expr::Index {
        expr: Box::new(Spanned::no_span(Expr::Ident("arr".into()))),
        index: Box::new(Spanned::no_span(Expr::Ident("i".into()))),
    });
    let mut vars = HashSet::new();
    collect_vars(&expr, &mut vars);
    assert!(vars.contains("arr"));
    assert!(vars.contains("i"));
}

#[test]
fn collect_vars_let_expr() {
    let expr = Spanned::no_span(Expr::Let {
        name: "tmp".into(),
        value: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        body: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
    });
    let mut vars = HashSet::new();
    collect_vars(&expr, &mut vars);
    assert!(vars.contains("a"));
    assert!(vars.contains("b"));
}

#[test]
fn collect_vars_match_expr() {
    use assura_ast::{MatchArm, Pattern};
    let expr = Spanned::no_span(Expr::Match {
        scrutinee: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        arms: vec![MatchArm {
            pattern: Pattern::Ident("_".into()),
            body: Spanned::no_span(Expr::Ident("y".into())),
        }],
    });
    let mut vars = HashSet::new();
    collect_vars(&expr, &mut vars);
    assert!(vars.contains("x"));
    assert!(vars.contains("y"));
}

#[test]
fn collect_vars_list_tuple_block() {
    let list = Spanned::no_span(Expr::List(vec![
        Spanned::no_span(Expr::Ident("a".into())),
        Spanned::no_span(Expr::Ident("b".into())),
    ]));
    let tuple = Spanned::no_span(Expr::Tuple(vec![Spanned::no_span(Expr::Ident("c".into()))]));
    let block = Spanned::no_span(Expr::Block(vec![Spanned::no_span(Expr::Ident("d".into()))]));
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
    let expr = Spanned::no_span(Expr::Apply {
        lemma_name: "lem".into(),
        args: vec![Spanned::no_span(Expr::Ident("p".into()))],
    });
    let mut vars = HashSet::new();
    collect_vars(&expr, &mut vars);
    assert!(vars.contains("p"));
}

#[test]
fn collect_vars_literal_is_empty() {
    let expr = Spanned::no_span(Expr::Literal(Literal::Int("42".into())));
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
        body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
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
        body: Spanned::no_span(Expr::Literal(Literal::Bool(false))),
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
    let body = Spanned::no_span(Expr::BinOp {
        op: BinOp::Gte,
        lhs: Box::new(Spanned::no_span(Expr::Ident("i".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });
    let forall_expr = Spanned::no_span(Expr::Forall {
        var: "i".into(),
        domain: Box::new(Spanned::no_span(Expr::Ident("xs".into()))),
        body: Box::new(body),
    });
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
    let expr = Spanned::no_span(Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]));
    assert!(
        !expr_has_unmodelable_features(&expr),
        "typestate @ annotation should be modelable after #262"
    );
}

#[test]
fn test_no_unmodelable_reason_for_typestate() {
    // #262: Typestate no longer produces unmodelable reasons
    let expr = Spanned::no_span(Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]));
    let reasons = collect_unmodelable_reasons(&expr);
    assert!(
        reasons.is_empty(),
        "typestate should produce no unmodelable reasons after #262, got: {:?}",
        reasons
    );
}

#[test]
fn test_modelable_normal_expr() {
    // Normal binary expression should be modelable
    let expr = Spanned::no_span(Expr::BinOp {
        op: BinOp::Gt,
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });
    assert!(
        !expr_has_unmodelable_features(&expr),
        "normal binop should be modelable"
    );
}

#[test]
fn test_typestate_nested_in_binop_modelable() {
    // #262: Typestate nested in a binary expression is now modelable
    let expr = Spanned::no_span(Expr::BinOp {
        op: BinOp::And,
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Raw(vec![
            "conn".into(),
            "@".into(),
            "Connected".into(),
        ]))),
    });
    assert!(
        !expr_has_unmodelable_features(&expr),
        "typestate nested in binop should be modelable after #262"
    );
}

#[test]
fn test_typestate_in_if_branch_modelable() {
    // #262: Typestate in if branch is now modelable
    let expr = Spanned::no_span(Expr::If {
        cond: Box::new(Spanned::no_span(Expr::Ident("flag".into()))),
        then_branch: Box::new(Spanned::no_span(Expr::Raw(vec![
            "s".into(),
            "@".into(),
            "Locked".into(),
        ]))),
        else_branch: None,
    });
    assert!(
        !expr_has_unmodelable_features(&expr),
        "typestate in if-then should be modelable after #262"
    );
}

#[test]
fn test_typestate_in_forall_body_modelable() {
    // #262: Typestate in forall body is now modelable
    let expr = Spanned::no_span(Expr::Forall {
        var: "i".into(),
        domain: Box::new(Spanned::no_span(Expr::Ident("xs".into()))),
        body: Box::new(Spanned::no_span(Expr::Raw(vec![
            "item".into(),
            "@".into(),
            "Valid".into(),
        ]))),
    });
    assert!(
        !expr_has_unmodelable_features(&expr),
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
            body: Spanned::no_span(Expr::Raw(vec!["file".into(), "@".into(), "Open".into()])),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::Raw(vec!["file".into(), "@".into(), "Open".into()])),
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
            body: Spanned::no_span(Expr::Raw(vec!["file".into(), "@".into(), "Open".into()])),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::Raw(vec!["file".into(), "@".into(), "Closed".into()])),
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
    let expr = Spanned::no_span(Expr::Apply {
        lemma_name: "helper".into(),
        args: vec![Spanned::no_span(Expr::Ident("x".into()))],
    });
    let refs = collect_apply_refs_from_expr(&expr);
    assert_eq!(refs, vec!["helper"]);
}

#[test]
fn test_collect_apply_refs_nested_in_binop() {
    let expr = Spanned::no_span(Expr::BinOp {
        op: BinOp::And,
        lhs: Box::new(Spanned::no_span(Expr::Apply {
            lemma_name: "lem_a".into(),
            args: vec![Spanned::no_span(Expr::Ident("x".into()))],
        })),
        rhs: Box::new(Spanned::no_span(Expr::Apply {
            lemma_name: "lem_b".into(),
            args: vec![Spanned::no_span(Expr::Ident("y".into()))],
        })),
    });
    let refs = collect_apply_refs_from_expr(&expr);
    assert_eq!(refs.len(), 2);
    assert!(refs.contains(&"lem_a".to_string()));
    assert!(refs.contains(&"lem_b".to_string()));
}

#[test]
fn test_collect_apply_refs_no_apply() {
    let expr = Spanned::no_span(Expr::BinOp {
        op: BinOp::Gt,
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });
    let refs = collect_apply_refs_from_expr(&expr);
    assert!(refs.is_empty());
}

#[test]
fn test_collect_apply_refs_nested_in_if() {
    let expr = Spanned::no_span(Expr::If {
        cond: Box::new(Spanned::no_span(Expr::Ident("flag".into()))),
        then_branch: Box::new(Spanned::no_span(Expr::Apply {
            lemma_name: "branch_lem".into(),
            args: vec![],
        })),
        else_branch: Some(Box::new(Spanned::no_span(Expr::Literal(Literal::Bool(
            true,
        ))))),
    });
    let refs = collect_apply_refs_from_expr(&expr);
    assert_eq!(refs, vec!["branch_lem"]);
}

// -------------------------------------------------------------------
// SMT-LIB float encoding tests (#248)
// -------------------------------------------------------------------

#[test]
fn test_smtlib_float_rational_encoding() {
    let expr = Spanned::no_span(Expr::Literal(Literal::Float("3.14".into())));
    let result = expr_to_smtlib(&expr).unwrap();
    assert_eq!(result, "(/ 3140000 1000000)");
}

#[test]
fn test_smtlib_float_zero() {
    let expr = Spanned::no_span(Expr::Literal(Literal::Float("0.0".into())));
    let result = expr_to_smtlib(&expr).unwrap();
    assert_eq!(result, "(/ 0 1000000)");
}

#[test]
fn test_smtlib_float_negative() {
    // Negative floats: the negation is applied by UnaryOp::Neg externally,
    // but the literal itself may parse as negative
    let expr = Spanned::no_span(Expr::UnaryOp {
        op: UnaryOp::Neg,
        expr: Box::new(Spanned::no_span(Expr::Literal(Literal::Float(
            "2.5".into(),
        )))),
    });
    let result = expr_to_smtlib(&expr).unwrap();
    assert_eq!(result, "(- (/ 2500000 1000000))");
}

#[test]
fn test_smtlib_match_float_pattern_rational() {
    // Match arm with float literal should use rational encoding
    let expr = Spanned::no_span(Expr::Match {
        scrutinee: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        arms: vec![
            assura_ast::MatchArm {
                pattern: Pattern::Literal(Literal::Float("1.5".into())),
                body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
            },
            assura_ast::MatchArm {
                pattern: Pattern::Wildcard,
                body: Spanned::no_span(Expr::Literal(Literal::Bool(false))),
            },
        ],
    });
    let result = expr_to_smtlib(&expr).unwrap();
    assert!(
        result.contains("(/ 1500000 1000000)"),
        "match float pattern should use rational: {result}"
    );
}

// Deep field chain flattening helpers (#250)
// -------------------------------------------------------------------

#[test]
fn test_is_self_rooted_sp_ident_self() {
    let expr = Spanned::no_span(Expr::Ident("self".into()));
    assert!(is_self_rooted_sp(&expr));
}

#[test]
fn test_is_self_rooted_sp_ident_other() {
    let expr = Spanned::no_span(Expr::Ident("x".into()));
    assert!(!is_self_rooted_sp(&expr));
}

#[test]
fn test_is_self_rooted_sp_field_chain() {
    // self.value
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Ident("self".into()))),
        "value".into(),
    ));
    assert!(is_self_rooted_sp(&expr));
}

#[test]
fn test_is_self_rooted_sp_deep_chain() {
    // self.inner.value
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Field(
            Box::new(Spanned::no_span(Expr::Ident("self".into()))),
            "inner".into(),
        ))),
        "value".into(),
    ));
    assert!(is_self_rooted_sp(&expr));
}

#[test]
fn test_field_chain_depth_sp_ident() {
    assert_eq!(
        field_chain_depth_sp(&Spanned::no_span(Expr::Ident("x".into()))),
        0
    );
}

#[test]
fn test_field_chain_depth_sp_single() {
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        "y".into(),
    ));
    assert_eq!(field_chain_depth_sp(&expr), 1);
}

#[test]
fn test_field_chain_depth_sp_deep() {
    // a.b.c -> depth 2
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Field(
            Box::new(Spanned::no_span(Expr::Ident("a".into()))),
            "b".into(),
        ))),
        "c".into(),
    ));
    assert_eq!(field_chain_depth_sp(&expr), 2);
}

#[test]
fn test_has_deep_field_chain_sp() {
    // a.b -> depth 1, not deep
    let shallow = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        "b".into(),
    ));
    assert!(!has_deep_field_chain_sp(&shallow));

    // a.b.c -> depth 2, deep
    let deep = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Field(
            Box::new(Spanned::no_span(Expr::Ident("a".into()))),
            "b".into(),
        ))),
        "c".into(),
    ));
    assert!(has_deep_field_chain_sp(&deep));
}

#[test]
fn test_flatten_field_chain_sp_simple() {
    // a.b -> "a__b"
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        "b".into(),
    ));
    assert_eq!(flatten_field_chain_sp(&expr), "a__b");
}

#[test]
fn test_flatten_field_chain_sp_deep() {
    // state.head.extra.extra_max -> "state__head__extra__extra_max"
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Field(
            Box::new(Spanned::no_span(Expr::Field(
                Box::new(Spanned::no_span(Expr::Ident("state".into()))),
                "head".into(),
            ))),
            "extra".into(),
        ))),
        "extra_max".into(),
    ));
    assert_eq!(
        flatten_field_chain_sp(&expr),
        "state__head__extra__extra_max"
    );
}

#[test]
fn test_flatten_field_chain_sp_ident() {
    // a.b -> "a__b"
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        "b".into(),
    ));
    assert_eq!(flatten_field_chain_sp(&expr), "a__b");
}

#[test]
fn test_cvc5_deep_field_chain_smtlib_flattening() {
    // state.head.extra.extra_max should flatten in SMT-LIB output
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Field(
            Box::new(Spanned::no_span(Expr::Field(
                Box::new(Spanned::no_span(Expr::Ident("state".into()))),
                "head".into(),
            ))),
            "extra".into(),
        ))),
        "extra_max".into(),
    ));
    let result = expr_to_smtlib(&expr);
    assert_eq!(result, Some("state__head__extra__extra_max".into()));
}

#[test]
fn test_cvc5_self_rooted_smtlib_flattening() {
    // self.value should flatten even at depth 1
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Ident("self".into()))),
        "value".into(),
    ));
    let result = expr_to_smtlib(&expr);
    assert_eq!(result, Some("self__value".into()));
}

#[test]
fn test_cvc5_shallow_field_smtlib_no_flatten() {
    // obj.field at depth 1 (not self-rooted) should NOT flatten
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Ident("obj".into()))),
        "field".into(),
    ));
    let result = expr_to_smtlib(&expr);
    assert_eq!(result, Some("(__field_field obj)".into()));
}

#[test]
fn test_cvc5_old_deep_field_smtlib_flattening() {
    // old(state.head.value) should flatten to state__head__value__old
    let inner = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Field(
            Box::new(Spanned::no_span(Expr::Ident("state".into()))),
            "head".into(),
        ))),
        "value".into(),
    ));
    let expr = Spanned::no_span(Expr::Old(Box::new(inner)));
    let result = expr_to_smtlib(&expr);
    assert_eq!(result, Some("state__head__value__old".into()));
}

#[test]
fn test_cvc5_old_self_rooted_smtlib_flattening() {
    // old(self.counter) should flatten to self__counter__old
    let inner = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Ident("self".into()))),
        "counter".into(),
    ));
    let expr = Spanned::no_span(Expr::Old(Box::new(inner)));
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
            body: Spanned::no_span(Expr::BinOp {
                op: BinOp::And,
                lhs: Box::new(Spanned::no_span(Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                })),
                rhs: Box::new(Spanned::no_span(Expr::BinOp {
                    op: BinOp::Lt,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Field(
                        Box::new(Spanned::no_span(Expr::Field(
                            Box::new(Spanned::no_span(Expr::Field(
                                Box::new(Spanned::no_span(Expr::Ident("state".into()))),
                                "head".into(),
                            ))),
                            "extra".into(),
                        ))),
                        "max".into(),
                    ))),
                })),
            }),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                op: BinOp::Gt,
                lhs: Box::new(Spanned::no_span(Expr::Field(
                    Box::new(Spanned::no_span(Expr::Field(
                        Box::new(Spanned::no_span(Expr::Field(
                            Box::new(Spanned::no_span(Expr::Ident("state".into()))),
                            "head".into(),
                        ))),
                        "extra".into(),
                    ))),
                    "max".into(),
                ))),
                rhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            }),
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
            body: Spanned::no_span(Expr::BinOp {
                op: BinOp::Gt,
                lhs: Box::new(Spanned::no_span(Expr::Field(
                    Box::new(Spanned::no_span(Expr::Ident("self".into()))),
                    "value".into(),
                ))),
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Spanned::no_span(Expr::Field(
                    Box::new(Spanned::no_span(Expr::Ident("self".into()))),
                    "value".into(),
                ))),
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
            }),
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
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Field(
            Box::new(Spanned::no_span(Expr::Ident("obj".into()))),
            "inner".into(),
        ))),
        "is_empty".into(),
    ));
    let result = expr_to_smtlib(&expr);
    assert_eq!(result, Some("obj__inner__is_empty".into()));
}

#[test]
fn test_cvc5_nested_field_size_smtlib() {
    // obj.inner.length should flatten in SMT-LIB output
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Field(
            Box::new(Spanned::no_span(Expr::Ident("obj".into()))),
            "inner".into(),
        ))),
        "length".into(),
    ));
    let result = expr_to_smtlib(&expr);
    assert_eq!(result, Some("obj__inner__length".into()));
}
// -------------------------------------------------------------------
// expr_to_smtlib string method tests (issue #251)
// -------------------------------------------------------------------

#[test]
fn test_smtlib_call_substring() {
    let expr = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("substring".into()))),
        args: vec![
            Spanned::no_span(Expr::Ident("s".into())),
            Spanned::no_span(Expr::Literal(Literal::Int("0".into()))),
            Spanned::no_span(Expr::Literal(Literal::Int("5".into()))),
        ],
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(substring s 0 5)");
}

#[test]
fn test_smtlib_call_concat() {
    let expr = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("concat".into()))),
        args: vec![
            Spanned::no_span(Expr::Ident("a".into())),
            Spanned::no_span(Expr::Ident("b".into())),
        ],
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(__concat a b)");
}

#[test]
fn test_smtlib_call_index_of() {
    let expr = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("index_of".into()))),
        args: vec![
            Spanned::no_span(Expr::Ident("s".into())),
            Spanned::no_span(Expr::Ident("sub".into())),
        ],
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(index_of s sub)");
}

#[test]
fn test_smtlib_call_char_at() {
    let expr = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("char_at".into()))),
        args: vec![
            Spanned::no_span(Expr::Ident("s".into())),
            Spanned::no_span(Expr::Literal(Literal::Int("3".into()))),
        ],
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(char_at s 3)");
}

#[test]
fn test_smtlib_call_replace() {
    let expr = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("replace".into()))),
        args: vec![
            Spanned::no_span(Expr::Ident("s".into())),
            Spanned::no_span(Expr::Ident("old_s".into())),
            Spanned::no_span(Expr::Ident("new_s".into())),
        ],
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(replace s old_s new_s)");
}

#[test]
fn test_smtlib_call_split() {
    let expr = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("split".into()))),
        args: vec![
            Spanned::no_span(Expr::Ident("s".into())),
            Spanned::no_span(Expr::Ident("delim".into())),
        ],
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(split s delim)");
}

#[test]
fn test_smtlib_call_trim() {
    let expr = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("trim".into()))),
        args: vec![Spanned::no_span(Expr::Ident("s".into()))],
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(trim s)");
}

#[test]
fn test_smtlib_call_set() {
    let expr = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("set".into()))),
        args: vec![
            Spanned::no_span(Expr::Ident("arr".into())),
            Spanned::no_span(Expr::Ident("i".into())),
            Spanned::no_span(Expr::Ident("v".into())),
        ],
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(set arr i v)");
}

#[test]
fn test_smtlib_call_put() {
    let expr = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("put".into()))),
        args: vec![
            Spanned::no_span(Expr::Ident("m".into())),
            Spanned::no_span(Expr::Ident("k".into())),
            Spanned::no_span(Expr::Ident("v".into())),
        ],
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(put m k v)");
}

#[test]
fn test_smtlib_method_substring() {
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(Spanned::no_span(Expr::Ident("s".into()))),
        method: "substring".into(),
        args: vec![
            Spanned::no_span(Expr::Literal(Literal::Int("1".into()))),
            Spanned::no_span(Expr::Literal(Literal::Int("4".into()))),
        ],
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(substring s 1 4)");
}

#[test]
fn test_smtlib_method_concat() {
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        method: "concat".into(),
        args: vec![Spanned::no_span(Expr::Ident("b".into()))],
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(__concat a b)");
}

#[test]
fn test_smtlib_method_set() {
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(Spanned::no_span(Expr::Ident("arr".into()))),
        method: "set".into(),
        args: vec![
            Spanned::no_span(Expr::Ident("i".into())),
            Spanned::no_span(Expr::Ident("v".into())),
        ],
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(set arr i v)");
}

#[test]
fn test_smtlib_method_put() {
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(Spanned::no_span(Expr::Ident("m".into()))),
        method: "put".into(),
        args: vec![
            Spanned::no_span(Expr::Ident("k".into())),
            Spanned::no_span(Expr::Ident("v".into())),
        ],
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(put m k v)");
}

#[test]
fn test_smtlib_method_trim() {
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(Spanned::no_span(Expr::Ident("s".into()))),
        method: "trim".into(),
        args: vec![],
    });
    let s = expr_to_smtlib(&expr).unwrap();
    assert_eq!(s, "(trim s)");
}

// -------------------------------------------------------------------
// CVC5 match pattern tests (native API, issue #252)
// -------------------------------------------------------------------
