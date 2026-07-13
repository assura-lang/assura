use std::collections::HashSet;

use super::*;
use assura_ast::Spanned;
use assura_ast::*;

// ---- is_numeric_expr ----

#[test]
fn is_numeric_ident() {
    assert!(is_numeric_expr(&Spanned::no_span(Expr::Ident("x".into()))));
}

#[test]
fn is_numeric_int_literal() {
    assert!(is_numeric_expr(&Spanned::no_span(Expr::Literal(
        Literal::Int("42".into())
    ))));
}

#[test]
fn is_numeric_float_literal() {
    assert!(is_numeric_expr(&Spanned::no_span(Expr::Literal(
        Literal::Float("3.14".into())
    ))));
}

#[test]
fn is_not_numeric_str_literal() {
    assert!(!is_numeric_expr(&Spanned::no_span(Expr::Literal(
        Literal::Str("hello".into())
    ))));
}

#[test]
fn is_not_numeric_bool_literal() {
    assert!(!is_numeric_expr(&Spanned::no_span(Expr::Literal(
        Literal::Bool(true)
    ))));
}

#[test]
fn is_numeric_binop_add() {
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        op: BinOp::Add,
        rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
    });
    assert!(is_numeric_expr(&e));
}

#[test]
fn is_not_numeric_binop_and() {
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Bool(true)))),
        op: BinOp::And,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Bool(false)))),
    });
    assert!(!is_numeric_expr(&e));
}

#[test]
fn is_numeric_neg() {
    let e = Spanned::no_span(Expr::UnaryOp {
        op: UnaryOp::Neg,
        expr: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
    });
    assert!(is_numeric_expr(&e));
}

#[test]
fn is_not_numeric_not() {
    let e = Spanned::no_span(Expr::UnaryOp {
        op: UnaryOp::Not,
        expr: Box::new(Spanned::no_span(Expr::Literal(Literal::Bool(true)))),
    });
    assert!(!is_numeric_expr(&e));
}

#[test]
fn is_numeric_old() {
    let e = Spanned::no_span(Expr::Old(Box::new(Spanned::no_span(Expr::Ident(
        "x".into(),
    )))));
    assert!(is_numeric_expr(&e));
}

#[test]
fn is_numeric_field() {
    let e = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Ident("s".into()))),
        "len".into(),
    ));
    assert!(is_numeric_expr(&e));
}

#[test]
fn is_not_numeric_forall() {
    let e = Spanned::no_span(Expr::Forall {
        var: "x".into(),
        domain: Box::new(Spanned::no_span(Expr::Ident("xs".into()))),
        body: Box::new(Spanned::no_span(Expr::Literal(Literal::Bool(true)))),
    });
    assert!(!is_numeric_expr(&e));
}

// ---- expr_to_rust ----

#[test]
fn expr_to_rust_int_literal() {
    assert_eq!(
        expr_to_rust(&Spanned::no_span(Expr::Literal(Literal::Int("42".into())))),
        "42"
    );
}

#[test]
fn expr_to_rust_str_literal() {
    assert_eq!(
        expr_to_rust(&Spanned::no_span(Expr::Literal(Literal::Str(
            "hello".into()
        )))),
        "\"hello\""
    );
}

#[test]
fn expr_to_rust_bool_literal() {
    assert_eq!(
        expr_to_rust(&Spanned::no_span(Expr::Literal(Literal::Bool(true)))),
        "true"
    );
}

#[test]
fn expr_to_rust_result_ident() {
    assert_eq!(
        expr_to_rust(&Spanned::no_span(Expr::Ident("result".into()))),
        RESULT_VAR
    );
}

#[test]
fn expr_to_rust_normal_ident() {
    assert_eq!(
        expr_to_rust(&Spanned::no_span(Expr::Ident("x".into()))),
        "x"
    );
}

#[test]
fn expr_to_rust_field() {
    let e = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Ident("s".into()))),
        "len".into(),
    ));
    assert_eq!(expr_to_rust(&e), "s.len");
}

#[test]
fn expr_to_rust_method_call() {
    let e = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(Spanned::no_span(Expr::Ident("v".into()))),
        method: "push".into(),
        args: vec![Spanned::no_span(Expr::Literal(Literal::Int("1".into())))],
    });
    assert_eq!(expr_to_rust(&e), "v.push(1)");
}

#[test]
fn expr_to_rust_call() {
    let e = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("foo".into()))),
        args: vec![
            Spanned::no_span(Expr::Ident("a".into())),
            Spanned::no_span(Expr::Ident("b".into())),
        ],
    });
    assert_eq!(expr_to_rust(&e), "foo(a, b)");
}

#[test]
fn expr_to_rust_index() {
    let e = Spanned::no_span(Expr::Index {
        expr: Box::new(Spanned::no_span(Expr::Ident("arr".into()))),
        index: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });
    assert_eq!(expr_to_rust(&e), "arr[(0) as usize]");
}

#[test]
fn expr_to_rust_binop_add() {
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        op: BinOp::Add,
        rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
    });
    assert_eq!(expr_to_rust(&e), "(i128::from(a) + i128::from(b))");
}

#[test]
fn expr_to_rust_implies() {
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("p".into()))),
        op: BinOp::Implies,
        rhs: Box::new(Spanned::no_span(Expr::Ident("q".into()))),
    });
    assert_eq!(expr_to_rust(&e), "(!p || q)");
}

#[test]
fn expr_to_rust_in_operator() {
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        op: BinOp::In,
        rhs: Box::new(Spanned::no_span(Expr::Ident("s".into()))),
    });
    assert_eq!(expr_to_rust(&e), "s.contains(&x)");
}

#[test]
fn expr_to_rust_notin_operator() {
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        op: BinOp::NotIn,
        rhs: Box::new(Spanned::no_span(Expr::Ident("s".into()))),
    });
    assert_eq!(expr_to_rust(&e), "!s.contains(&x)");
}

#[test]
fn expr_to_rust_concat() {
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        op: BinOp::Concat,
        rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
    });
    assert_eq!(expr_to_rust(&e), "[a, b].concat()");
}

#[test]
fn expr_to_rust_numeric_cmp_casts_i128() {
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        op: BinOp::Lt,
        rhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
    });
    assert_eq!(expr_to_rust(&e), "(i128::from(x) < i128::from(y))");
}

#[test]
fn expr_to_rust_eq_no_cast() {
    // Equality on numeric idents casts to i128 (prevents mixed-type errors)
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        op: BinOp::Eq,
        rhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
    });
    assert_eq!(expr_to_rust(&e), "(i128::from(x) == i128::from(y))");
}

#[test]
fn expr_to_rust_unary_neg() {
    let e = Spanned::no_span(Expr::UnaryOp {
        op: UnaryOp::Neg,
        expr: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
    });
    assert_eq!(expr_to_rust(&e), "(-x)");
}

#[test]
fn expr_to_rust_unary_not() {
    let e = Spanned::no_span(Expr::UnaryOp {
        op: UnaryOp::Not,
        expr: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
    });
    assert_eq!(expr_to_rust(&e), "(!x)");
}

#[test]
fn expr_to_rust_old() {
    let e = Spanned::no_span(Expr::Old(Box::new(Spanned::no_span(Expr::Ident(
        "x".into(),
    )))));
    assert_eq!(expr_to_rust(&e), format!("{OLD_VAR_PREFIX}x"));
}

#[test]
fn expr_to_rust_forall() {
    let e = Spanned::no_span(Expr::Forall {
        var: "x".into(),
        domain: Box::new(Spanned::no_span(Expr::Ident("xs".into()))),
        body: Box::new(Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        })),
    });
    let result = expr_to_rust(&e);
    assert!(result.contains("iter().copied().all(|x|"));
}

#[test]
fn expr_to_rust_exists() {
    let e = Spanned::no_span(Expr::Exists {
        var: "x".into(),
        domain: Box::new(Spanned::no_span(Expr::Ident("xs".into()))),
        body: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
    });
    assert!(expr_to_rust(&e).contains("iter().copied().any(|x|"));
}

#[test]
fn expr_to_rust_if_else() {
    let e = Spanned::no_span(Expr::If {
        cond: Box::new(Spanned::no_span(Expr::Ident("c".into()))),
        then_branch: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        else_branch: Some(Box::new(Spanned::no_span(Expr::Literal(Literal::Int(
            "2".into(),
        ))))),
    });
    assert_eq!(expr_to_rust(&e), "if c { 1 } else { 2 }");
}

#[test]
fn expr_to_rust_if_no_else() {
    let e = Spanned::no_span(Expr::If {
        cond: Box::new(Spanned::no_span(Expr::Ident("c".into()))),
        then_branch: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        else_branch: None,
    });
    assert_eq!(expr_to_rust(&e), "if c { 1 }");
}

#[test]
fn expr_to_rust_list() {
    let e = Spanned::no_span(Expr::List(vec![
        Spanned::no_span(Expr::Literal(Literal::Int("1".into()))),
        Spanned::no_span(Expr::Literal(Literal::Int("2".into()))),
    ]));
    assert_eq!(expr_to_rust(&e), "vec![1, 2]");
}

#[test]
fn expr_to_rust_cast() {
    let e = Spanned::no_span(Expr::Cast {
        expr: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        ty: "u32".into(),
    });
    assert_eq!(expr_to_rust(&e), "(x as u32)");
}

#[test]
fn expr_to_rust_ghost_erased() {
    let e = Spanned::no_span(Expr::Ghost(Box::new(Spanned::no_span(Expr::Ident(
        "x".into(),
    )))));
    assert_eq!(expr_to_rust(&e), "/* ghost erased */()");
}

#[test]
fn expr_to_rust_apply_erased() {
    let e = Spanned::no_span(Expr::Apply {
        lemma_name: "L1".into(),
        args: vec![],
    });
    assert_eq!(expr_to_rust(&e), "/* lemma L1 applied */");
}

#[test]
fn expr_to_rust_let_binding() {
    let e = Spanned::no_span(Expr::Let {
        name: "v".into(),
        value: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
        body: Box::new(Spanned::no_span(Expr::Ident("v".into()))),
    });
    assert_eq!(expr_to_rust(&e), "{ let v = 5; v }");
}

#[test]
fn expr_to_rust_tuple() {
    let e = Spanned::no_span(Expr::Tuple(vec![
        Spanned::no_span(Expr::Literal(Literal::Int("1".into()))),
        Spanned::no_span(Expr::Literal(Literal::Int("2".into()))),
    ]));
    assert_eq!(expr_to_rust(&e), "(1, 2)");
}

#[test]
fn expr_to_rust_match_with_wildcard_fallback() {
    use assura_ast::{MatchArm, Pattern, Spanned};
    let e = Spanned::no_span(Expr::Match {
        scrutinee: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        arms: vec![MatchArm {
            pattern: Pattern::Constructor {
                name: "Some".into(),
                fields: vec![Pattern::Ident("v".into())],
            },
            body: Spanned::no_span(Expr::Ident("v".into())),
        }],
    });
    let result = expr_to_rust(&e);
    assert!(result.contains("match x"));
    assert!(result.contains("Some(v) => v,"));
    assert!(result.contains("_ => unreachable!"));
}

#[test]
fn expr_to_rust_match_has_wildcard() {
    use assura_ast::{MatchArm, Pattern};
    let e = Spanned::no_span(Expr::Match {
        scrutinee: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        arms: vec![
            MatchArm {
                pattern: Pattern::Literal(Literal::Int("1".into())),
                body: Spanned::no_span(Expr::Ident("a".into())),
            },
            MatchArm {
                pattern: Pattern::Wildcard,
                body: Spanned::no_span(Expr::Ident("b".into())),
            },
        ],
    });
    let result = expr_to_rust(&e);
    assert!(result.contains("_ => b,"));
    assert!(!result.contains("unreachable!"));
}

// ---- raw_tokens_to_rust ----

#[test]
fn raw_tokens_empty() {
    assert_eq!(raw_tokens_to_rust(&[]), "");
}

#[test]
fn raw_tokens_forall_quantifier() {
    let tokens: Vec<String> = vec!["forall", "x", "in", "items", ":", "x"]
        .into_iter()
        .map(String::from)
        .collect();
    let result = raw_tokens_to_rust(&tokens);
    assert!(result.contains(".iter().copied().all(|x|"), "got: {result}");
}

#[test]
fn raw_tokens_exists_quantifier() {
    let tokens: Vec<String> = vec!["exists", "x", "in", "items", ":", "x"]
        .into_iter()
        .map(String::from)
        .collect();
    let result = raw_tokens_to_rust(&tokens);
    assert!(result.contains(".iter().copied().any(|x|"), "got: {result}");
}

#[test]
fn raw_tokens_typestate_annotation() {
    let tokens: Vec<String> = vec!["conn", "@", "Connected"]
        .into_iter()
        .map(String::from)
        .collect();
    let result = raw_tokens_to_rust(&tokens);
    assert!(result.starts_with("true /* typestate:"), "got: {result}");
    assert!(result.contains("Connected"));
}

#[test]
fn raw_tokens_result_replacement() {
    let tokens: Vec<String> = vec!["result"].into_iter().map(String::from).collect();
    assert_eq!(raw_tokens_to_rust(&tokens), RESULT_VAR);
}

// ---- has_deep_field_access ----

#[test]
fn no_deep_field_plain() {
    assert!(!has_deep_field_access("x > 0"));
}

#[test]
fn has_deep_field_struct() {
    assert!(has_deep_field_access("state.head.extra"));
}

#[test]
fn no_deep_field_method_chain() {
    assert!(!has_deep_field_access("v.iter().all()"));
}

#[test]
fn has_deep_field_result() {
    assert!(has_deep_field_access(&format!("{RESULT_VAR}.value")));
}

#[test]
fn no_deep_field_result_method() {
    assert!(!has_deep_field_access(&format!("{RESULT_VAR}.is_some()")));
}

// ---- RustStmt::Assert rendering (replaces generate_debug_assert) ----

#[test]
fn debug_assert_simple() {
    let mut code = String::new();
    crate::hir::render_stmt(
        &crate::hir::RustStmt::Assert {
            cond: "x > 0".into(),
            label: "requires".into(),
        },
        &mut code,
        1,
    );
    assert!(code.contains("debug_assert!(x > 0,"));
    assert!(code.contains("requires"));
}

#[test]
fn debug_assert_deep_field_becomes_comment() {
    let mut code = String::new();
    crate::hir::render_stmt(
        &crate::hir::RustStmt::Assert {
            cond: "state.head.extra".into(),
            label: "ensures".into(),
        },
        &mut code,
        1,
    );
    assert!(code.starts_with("    // ensures:"));
    assert!(!code.contains("debug_assert!"));
}

#[test]
fn debug_assert_multiline() {
    let mut code = String::new();
    crate::hir::render_stmt(
        &crate::hir::RustStmt::Assert {
            cond: "x > 0\n&& y > 0".into(),
            label: "requires".into(),
        },
        &mut code,
        1,
    );
    assert!(code.contains("debug_assert!({"));
}

#[test]
fn debug_assert_indented() {
    let mut code = String::new();
    crate::hir::render_stmt(
        &crate::hir::RustStmt::Assert {
            cond: "x > 0".into(),
            label: "test".into(),
        },
        &mut code,
        2,
    );
    assert!(code.starts_with("        debug_assert!"));
}

// ---- pattern_to_rust ----

#[test]
fn pattern_ident() {
    use assura_ast::Pattern;
    assert_eq!(pattern_to_rust(&Pattern::Ident("x".into())), "x");
}

#[test]
fn pattern_wildcard() {
    use assura_ast::Pattern;
    assert_eq!(pattern_to_rust(&Pattern::Wildcard), "_");
}

#[test]
fn pattern_literal() {
    use assura_ast::Pattern;
    assert_eq!(
        pattern_to_rust(&Pattern::Literal(Literal::Int("42".into()))),
        "42"
    );
}

#[test]
fn pattern_constructor() {
    use assura_ast::Pattern;
    let p = Pattern::Constructor {
        name: "Some".into(),
        fields: vec![Pattern::Ident("v".into())],
    };
    assert_eq!(pattern_to_rust(&p), "Some(v)");
}

#[test]
fn pattern_constructor_empty() {
    use assura_ast::Pattern;
    let p = Pattern::Constructor {
        name: "None".into(),
        fields: vec![],
    };
    assert_eq!(pattern_to_rust(&p), "None");
}

#[test]
fn pattern_tuple() {
    use assura_ast::Pattern;
    let p = Pattern::Tuple(vec![Pattern::Ident("a".into()), Pattern::Ident("b".into())]);
    assert_eq!(pattern_to_rust(&p), "(a, b)");
}

// ---- old_var_name ----

#[test]
fn old_var_name_ident() {
    assert_eq!(
        old_var_name(&Spanned::no_span(Expr::Ident("x".into()))),
        "x"
    );
}

#[test]
fn old_var_name_field() {
    let e = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Ident("buf".into()))),
        "len".into(),
    ));
    assert_eq!(old_var_name(&e), "buf_len");
}

#[test]
fn old_var_name_index() {
    let e = Spanned::no_span(Expr::Index {
        expr: Box::new(Spanned::no_span(Expr::Ident("arr".into()))),
        index: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });
    assert_eq!(old_var_name(&e), "arr_idx");
}

#[test]
fn old_var_name_binop() {
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        op: BinOp::Add,
        rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
    });
    assert_eq!(old_var_name(&e), "a_add_b");
}

// ---- collect_old_exprs ----

#[test]
fn collect_old_empty() {
    assert!(collect_old_exprs(&Spanned::no_span(Expr::Ident("x".into()))).is_empty());
}

#[test]
fn collect_old_single() {
    let e = Spanned::no_span(Expr::Old(Box::new(Spanned::no_span(Expr::Ident(
        "x".into(),
    )))));
    let result = collect_old_exprs(&e);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "x");
    assert_eq!(result[0].1, "x");
}

#[test]
fn collect_old_nested_binop() {
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Old(Box::new(Spanned::no_span(
            Expr::Ident("a".into()),
        ))))),
        op: BinOp::Add,
        rhs: Box::new(Spanned::no_span(Expr::Old(Box::new(Spanned::no_span(
            Expr::Ident("b".into()),
        ))))),
    });
    let result = collect_old_exprs(&e);
    assert_eq!(result.len(), 2);
}

#[test]
fn collect_old_deduplicates() {
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Old(Box::new(Spanned::no_span(
            Expr::Ident("x".into()),
        ))))),
        op: BinOp::Add,
        rhs: Box::new(Spanned::no_span(Expr::Old(Box::new(Spanned::no_span(
            Expr::Ident("x".into()),
        ))))),
    });
    let result = collect_old_exprs(&e);
    assert_eq!(result.len(), 1);
}

// ---- resolve_ordering_variant ----

#[test]
fn ordering_sequentially_consistent() {
    let e = Spanned::no_span(Expr::Ident("seq_cst".into()));
    assert_eq!(resolve_ordering_variant(&e), Some("SeqCst"));
}

#[test]
fn ordering_relaxed() {
    let e = Spanned::no_span(Expr::Ident("relaxed".into()));
    assert_eq!(resolve_ordering_variant(&e), Some("Relaxed"));
}

#[test]
fn ordering_unknown() {
    let e = Spanned::no_span(Expr::Ident("garbage".into()));
    assert_eq!(resolve_ordering_variant(&e), None);
}

// ---- has_float_expr ----

#[test]
fn has_float_expr_literal() {
    let e = Spanned::no_span(Expr::Literal(Literal::Float("3.14".into())));
    assert!(has_float_expr(&e, &HashSet::new()));
}

#[test]
fn has_float_expr_ident_in_set() {
    let vars: HashSet<String> = ["x".into()].into_iter().collect();
    let e = Spanned::no_span(Expr::Ident("x".into()));
    assert!(has_float_expr(&e, &vars));
}

#[test]
fn has_float_expr_ident_not_in_set() {
    let e = Spanned::no_span(Expr::Ident("x".into()));
    assert!(!has_float_expr(&e, &HashSet::new()));
}

#[test]
fn has_float_expr_binop_with_float_literal() {
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        op: BinOp::Add,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Float("1.0".into())))),
    });
    assert!(has_float_expr(&e, &HashSet::new()));
}

#[test]
fn has_float_expr_nested_method_call() {
    let vars: HashSet<String> = ["x".into()].into_iter().collect();
    let e = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        method: "abs".into(),
        args: vec![],
    });
    assert!(has_float_expr(&e, &vars));
}

#[test]
fn has_float_expr_int_only() {
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        op: BinOp::Add,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("2".into())))),
    });
    assert!(!has_float_expr(&e, &HashSet::new()));
}

// ---- expr_to_rust_with_floats ----

#[test]
fn float_binop_skips_i128_wrapping() {
    let vars: HashSet<String> = ["x".into(), "y".into()].into_iter().collect();
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        op: BinOp::Lt,
        rhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
    });
    let result = expr_to_rust_with_floats(&e, vars);
    assert!(!result.contains("i128::from"), "Float vars must not use i128::from, got: {result}");
    assert!(result.contains("x") && result.contains("y"));
}

#[test]
fn float_literal_binop_skips_i128_wrapping() {
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Float("1.5".into())))),
        op: BinOp::Add,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Float("2.5".into())))),
    });
    let result = expr_to_rust_with_floats(&e, HashSet::new());
    assert!(!result.contains("i128::from"), "Float literals must not use i128::from, got: {result}");
}

#[test]
fn non_float_binop_still_uses_i128() {
    let e = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        op: BinOp::Add,
        rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
    });
    // No float vars, should use i128::from as before
    let result = expr_to_rust_with_floats(&e, HashSet::new());
    assert!(result.contains("i128::from"), "Non-float must use i128::from, got: {result}");
}

#[test]
fn mixed_float_and_int_in_if_skips_i128() {
    let vars: HashSet<String> = ["x".into()].into_iter().collect();
    let e = Spanned::no_span(Expr::If {
        cond: Box::new(Spanned::no_span(Expr::Ident("c".into()))),
        then_branch: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        else_branch: Some(Box::new(Spanned::no_span(Expr::Literal(Literal::Float("0.0".into()))))),
    });
    let result = expr_to_rust_with_floats(&e, vars);
    assert!(!result.contains("i128::from"), "Float if-branches must not use i128::from, got: {result}");
}
