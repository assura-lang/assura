use super::*;

// --- T059: Constant-time execution tests ---

#[test]
fn ct_branch_on_secret_a14001() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("key".into());
    let cond = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("key".into()))),
        op: AstBinOp::Eq,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
    });
    let errors = checker.check_branch(&cond, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A14001");
}

#[test]
fn ct_branch_on_public_ok() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("key".into());
    let cond = Spanned::no_span(AstExpr::Ident("public_val".into()));
    let errors = checker.check_branch(&cond, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ct_index_on_secret_a14002() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("secret_idx".into());
    let idx = Spanned::no_span(AstExpr::Ident("secret_idx".into()));
    let errors = checker.check_index(&idx, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A14002");
}

#[test]
fn ct_index_on_public_ok() {
    let checker = ConstantTimeChecker::new();
    let idx = Spanned::no_span(AstExpr::Ident("i".into()));
    let errors = checker.check_index(&idx, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ct_nested_secret_in_condition() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("password".into());
    // password + 1 == 42
    let cond = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::Ident("password".into()))),
            op: AstBinOp::Add,
            rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        })),
        op: AstBinOp::Eq,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("42".into())))),
    });
    let errors = checker.check_branch(&cond, &(0..1));
    assert_eq!(errors.len(), 1);
}

#[test]
fn ct_check_expr_if_with_secret() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("s".into());
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Ident("s".into()))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int(
            "0".into(),
        ))))),
    });
    let errors = checker.check_expr(&expr, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A14001");
}

#[test]
fn ct_references_secret_field() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("key".into());
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("key".into()))),
        "len".into(),
    ));
    assert!(checker.references_secret(&expr));
}
