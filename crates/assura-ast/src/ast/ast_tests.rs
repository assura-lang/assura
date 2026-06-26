use crate::*;

#[test]
fn extract_params_refined_type_with_less_than() {
    // a : { x : Int | x < 10 }, b : Bool
    // The `<` inside the refinement must NOT be treated as an angle bracket.
    let tokens: Vec<String> = vec![
        "a", ":", "{", "x", ":", "Int", "|", "x", "<", "10", "}", ",", "b", ":", "Bool",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let body = Spanned::no_span(Expr::Raw(tokens));
    let params = extract_clause_params(&body);
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "a");
    // Refined type parses as Named("{ x : Int | x < 10 }") fallback
    params[0].ty.as_ref().unwrap();
    assert_eq!(params[1].name, "b");
    assert_eq!(params[1].ty, Some(TypeExpr::Named("Bool".into())));
}

#[test]
fn extract_params_refined_type_with_parens() {
    // val : ( Int , Bool )
    let tokens: Vec<String> = vec!["val", ":", "(", "Int", ",", "Bool", ")"]
        .into_iter()
        .map(String::from)
        .collect();
    let body = Spanned::no_span(Expr::Raw(tokens));
    let params = extract_clause_params(&body);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "val");
    // Tuple-like tokens parse as Named("( Int , Bool )") fallback
    params[0].ty.as_ref().unwrap();
}

#[test]
fn extract_params_generic_type() {
    // a : List < Int >, b : Map < String , Int >
    let tokens: Vec<String> = vec![
        "a", ":", "List", "<", "Int", ">", ",", "b", ":", "Map", "<", "String", ",", "Int", ">",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let body = Spanned::no_span(Expr::Raw(tokens));
    let params = extract_clause_params(&body);
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "a");
    assert_eq!(
        params[0].ty,
        Some(TypeExpr::Generic(
            "List".into(),
            vec![TypeExpr::Named("Int".into())]
        ))
    );
    assert_eq!(params[1].name, "b");
    assert_eq!(
        params[1].ty,
        Some(TypeExpr::Generic(
            "Map".into(),
            vec![
                TypeExpr::Named("String".into()),
                TypeExpr::Named("Int".into())
            ]
        ))
    );
}

#[test]
fn negate_expr_inverts_comparisons() {
    let sp = |e| Spanned::no_span(e);

    // Eq => Neq
    let e = sp(Expr::BinOp {
        lhs: Box::new(sp(Expr::Ident("a".into()))),
        op: BinOp::Eq,
        rhs: Box::new(sp(Expr::Ident("b".into()))),
    });
    match &negate_expr(&e).node {
        Expr::BinOp { op: BinOp::Neq, .. } => {}
        other => panic!("expected Neq, got {other:?}"),
    }

    // Lt => Gte
    let e = sp(Expr::BinOp {
        lhs: Box::new(sp(Expr::Ident("x".into()))),
        op: BinOp::Lt,
        rhs: Box::new(sp(Expr::Literal(Literal::Int("0".into())))),
    });
    match &negate_expr(&e).node {
        Expr::BinOp { op: BinOp::Gte, .. } => {}
        other => panic!("expected Gte, got {other:?}"),
    }

    // In => NotIn
    let e = sp(Expr::BinOp {
        lhs: Box::new(sp(Expr::Ident("x".into()))),
        op: BinOp::In,
        rhs: Box::new(sp(Expr::Ident("s".into()))),
    });
    match &negate_expr(&e).node {
        Expr::BinOp {
            op: BinOp::NotIn, ..
        } => {}
        other => panic!("expected NotIn, got {other:?}"),
    }
}

#[test]
fn negate_expr_de_morgan_laws() {
    let sp = |e| Spanned::no_span(e);

    // And => Or with negated operands
    let e = sp(Expr::BinOp {
        lhs: Box::new(sp(Expr::Ident("a".into()))),
        op: BinOp::And,
        rhs: Box::new(sp(Expr::Ident("b".into()))),
    });
    match &negate_expr(&e).node {
        Expr::BinOp {
            lhs,
            op: BinOp::Or,
            rhs,
        } => {
            assert!(matches!(
                &lhs.node,
                Expr::UnaryOp {
                    op: UnaryOp::Not,
                    ..
                }
            ));
            assert!(matches!(
                &rhs.node,
                Expr::UnaryOp {
                    op: UnaryOp::Not,
                    ..
                }
            ));
        }
        other => panic!("expected Or, got {other:?}"),
    }

    // Or => And with negated operands
    let e = sp(Expr::BinOp {
        lhs: Box::new(sp(Expr::Ident("a".into()))),
        op: BinOp::Or,
        rhs: Box::new(sp(Expr::Ident("b".into()))),
    });
    match &negate_expr(&e).node {
        Expr::BinOp {
            lhs,
            op: BinOp::And,
            rhs,
        } => {
            assert!(matches!(
                &lhs.node,
                Expr::UnaryOp {
                    op: UnaryOp::Not,
                    ..
                }
            ));
            assert!(matches!(
                &rhs.node,
                Expr::UnaryOp {
                    op: UnaryOp::Not,
                    ..
                }
            ));
        }
        other => panic!("expected And, got {other:?}"),
    }
}

#[test]
fn negate_expr_double_negation_elimination() {
    let sp = |e| Spanned::no_span(e);

    let e = sp(Expr::UnaryOp {
        op: UnaryOp::Not,
        expr: Box::new(sp(Expr::Ident("x".into()))),
    });
    match &negate_expr(&e).node {
        Expr::Ident(name) => assert_eq!(name, "x"),
        other => panic!("expected Ident, got {other:?}"),
    }
}

#[test]
fn negate_expr_bool_literal() {
    let sp = |e| Spanned::no_span(e);

    let e = sp(Expr::Literal(Literal::Bool(true)));
    match &negate_expr(&e).node {
        Expr::Literal(Literal::Bool(false)) => {}
        other => panic!("expected false, got {other:?}"),
    }

    let e = sp(Expr::Literal(Literal::Bool(false)));
    match &negate_expr(&e).node {
        Expr::Literal(Literal::Bool(true)) => {}
        other => panic!("expected true, got {other:?}"),
    }
}
