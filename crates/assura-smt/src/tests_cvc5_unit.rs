use super::*;

#[test]
fn solver_choice_from_str() {
    assert_eq!(SolverChoice::from_str_loose("z3"), Some(SolverChoice::Z3));
    assert_eq!(SolverChoice::from_str_loose("Z3"), Some(SolverChoice::Z3));
    assert_eq!(
        SolverChoice::from_str_loose("cvc5"),
        Some(SolverChoice::Cvc5)
    );
    assert_eq!(
        SolverChoice::from_str_loose("CVC5"),
        Some(SolverChoice::Cvc5)
    );
    assert_eq!(
        SolverChoice::from_str_loose("portfolio"),
        Some(SolverChoice::Portfolio)
    );
    assert_eq!(SolverChoice::from_str_loose("invalid"), None);
}

#[test]
fn cvc5_expr_to_smtlib_literal() {
    use assura_ast::Literal;
    let e = Spanned::no_span(Expr::Literal(Literal::Int("42".into())));
    assert_eq!(cvc5_backend::expr_to_smtlib(&e), Some("42".to_string()));

    let e = Spanned::no_span(Expr::Literal(Literal::Bool(true)));
    assert_eq!(cvc5_backend::expr_to_smtlib(&e), Some("true".to_string()));

    let e = Spanned::no_span(Expr::Literal(Literal::Int("-5".into())));
    assert_eq!(cvc5_backend::expr_to_smtlib(&e), Some("(- 5)".to_string()));
}

#[test]
fn cvc5_expr_to_smtlib_ident() {
    let e = Spanned::no_span(Expr::Ident("x".to_string()));
    assert_eq!(cvc5_backend::expr_to_smtlib(&e), Some("x".to_string()));
}

#[test]
fn cvc5_expr_to_smtlib_binop() {
    use assura_ast::{BinOp, Literal};
    let e = Spanned::no_span(Expr::BinOp {
        op: BinOp::Add,
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
    });
    assert_eq!(
        cvc5_backend::expr_to_smtlib(&e),
        Some("(+ x 1)".to_string())
    );

    let e = Spanned::no_span(Expr::BinOp {
        op: BinOp::Neq,
        lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });
    assert_eq!(
        cvc5_backend::expr_to_smtlib(&e),
        Some("(not (= a 0))".to_string())
    );
}

#[test]
fn cvc5_expr_to_smtlib_unary() {
    use assura_ast::UnaryOp;
    let e = Spanned::no_span(Expr::UnaryOp {
        op: UnaryOp::Not,
        expr: Box::new(Spanned::no_span(Expr::Ident("p".into()))),
    });
    assert_eq!(
        cvc5_backend::expr_to_smtlib(&e),
        Some("(not p)".to_string())
    );
}

#[test]
fn cvc5_expr_to_smtlib_ite() {
    use assura_ast::Literal;
    let e = Spanned::no_span(Expr::If {
        cond: Box::new(Spanned::no_span(Expr::Ident("c".into()))),
        then_branch: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        else_branch: Some(Box::new(Spanned::no_span(Expr::Literal(Literal::Int(
            "0".into(),
        ))))),
    });
    assert_eq!(
        cvc5_backend::expr_to_smtlib(&e),
        Some("(ite c 1 0)".to_string())
    );
}

#[test]
fn cvc5_expr_to_smtlib_forall() {
    let e = Spanned::no_span(Expr::Forall {
        var: "i".to_string(),
        domain: Box::new(Spanned::no_span(Expr::Ident("S".into()))),
        body: Box::new(Spanned::no_span(Expr::BinOp {
            op: assura_ast::BinOp::Gt,
            lhs: Box::new(Spanned::no_span(Expr::Ident("i".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(assura_ast::Literal::Int(
                "0".into(),
            )))),
        })),
    });
    assert_eq!(
        cvc5_backend::expr_to_smtlib(&e),
        Some("(forall ((i Int)) (=> (__domain_contains S i) (> i 0)))".to_string())
    );
}

#[test]
fn cvc5_expr_to_smtlib_result() {
    let e = Spanned::no_span(Expr::Ident("result".to_string()));
    assert_eq!(
        cvc5_backend::expr_to_smtlib(&e),
        Some("__result".to_string())
    );
}

#[test]
fn cvc5_expr_to_smtlib_old() {
    let e = Spanned::no_span(Expr::Old(Box::new(Spanned::no_span(Expr::Ident(
        "x".into(),
    )))));
    assert_eq!(cvc5_backend::expr_to_smtlib(&e), Some("x__old".to_string()));
}

#[test]
fn cvc5_collect_vars() {
    use std::collections::HashSet;
    let e = Spanned::no_span(Expr::BinOp {
        op: assura_ast::BinOp::Add,
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
    });
    let mut vars = HashSet::new();
    cvc5_backend::collect_vars(&e, &mut vars);
    assert!(vars.contains("x"));
    assert!(vars.contains("y"));
}

#[test]
fn cvc5_parse_model() {
    let model = "(define-fun x () Int 42)\n(define-fun y () Int (- 1))";
    let cm = cvc5_backend::parse_smtlib_model(model).expect("model should parse");
    assert_eq!(cm.variables.len(), 2);
    assert!(cm.variables.iter().any(|(n, v)| n == "x" && v == "42"));
    assert!(cm.variables.iter().any(|(n, v)| n == "y" && v == "(- 1)"));
}

#[test]
fn cvc5_parse_empty_model() {
    let parsed = cvc5_backend::parse_smtlib_model("");
    assert!(parsed.is_none());
}

#[test]
fn cvc5_verify_without_binary() {
    // If cvc5 is not installed, verify_contract_cvc5 returns Error results
    use assura_ast::{Clause, ClauseKind, Literal};
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Spanned::no_span(Expr::BinOp {
                op: assura_ast::BinOp::Neq,
                lhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                op: assura_ast::BinOp::Gt,
                lhs: Box::new(Spanned::no_span(Expr::Ident("result".into()))),
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
            effect_variables: vec![],
        },
    ];
    let results = cvc5_backend::verify_contract_cvc5("TestContract", &clauses);
    // Should return 1 result (for ensures). May be Unknown if cvc5 not installed.
    assert_eq!(results.len(), 1);
}
