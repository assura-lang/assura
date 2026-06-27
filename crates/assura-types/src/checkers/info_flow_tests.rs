use super::*;
use assura_parser::ast::Spanned;

fn span() -> Range<usize> {
    0..10
}

fn ident(s: &str) -> SpExpr {
    Spanned::no_span(Expr::Ident(s.to_string()))
}

fn int_lit(n: i64) -> SpExpr {
    Spanned::no_span(Expr::Literal(Literal::Int(n.to_string())))
}

// ---- DependentTypeChecker ----

#[test]
fn dep_validate_nat_index() {
    let checker = DependentTypeChecker::new();
    let errs = checker.validate_index("n", "Nat", &span());
    assert!(errs.is_empty());
}

#[test]
fn dep_validate_bool_index() {
    let checker = DependentTypeChecker::new();
    let errs = checker.validate_index("flag", "Bool", &span());
    assert!(errs.is_empty());
}

#[test]
fn dep_validate_unknown_index_type() {
    let checker = DependentTypeChecker::new();
    let errs = checker.validate_index("x", "String", &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A03006");
}

#[test]
fn dep_validate_known_enum_index() {
    let mut checker = DependentTypeChecker::new();
    checker.register_enum("Mode".into(), vec!["Read".into(), "Write".into()]);
    let errs = checker.validate_index("m", "Mode", &span());
    assert!(errs.is_empty());
}

#[test]
fn dep_check_nat_index_expr_literal() {
    let checker = DependentTypeChecker::new();
    let errs = checker.check_index_expr(&int_lit(5), &DepIndex::Nat("n".into()), &span());
    assert!(errs.is_empty());
}

#[test]
fn dep_check_nat_index_expr_arithmetic() {
    let mut checker = DependentTypeChecker::new();
    checker.bind_index("n".into(), DepIndex::Nat("n".into()));
    let expr = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(ident("n")),
        op: BinOp::Add,
        rhs: Box::new(int_lit(1)),
    });
    let errs = checker.check_index_expr(&expr, &DepIndex::Nat("n".into()), &span());
    assert!(errs.is_empty());
}

#[test]
fn dep_check_bool_index_rejects_arithmetic() {
    let checker = DependentTypeChecker::new();
    let expr = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(int_lit(1)),
        op: BinOp::Add,
        rhs: Box::new(int_lit(2)),
    });
    let errs = checker.check_index_expr(&expr, &DepIndex::Bool("flag".into()), &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A03008");
}

#[test]
fn dep_check_dep_type_eq_base_mismatch() {
    let checker = DependentTypeChecker::new();
    let a = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Nat("n".into())],
    };
    let b = DepType {
        base: Type::Bool,
        indices: vec![DepIndex::Nat("n".into())],
    };
    let errs = checker.check_dep_type_eq(&a, &b, &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A03010");
}

#[test]
fn dep_check_dep_type_eq_index_count_mismatch() {
    let checker = DependentTypeChecker::new();
    let a = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Nat("n".into()), DepIndex::Nat("m".into())],
    };
    let b = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Nat("n".into())],
    };
    let errs = checker.check_dep_type_eq(&a, &b, &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A03010");
}

#[test]
fn dep_check_dep_type_eq_index_kind_mismatch() {
    let checker = DependentTypeChecker::new();
    let a = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Nat("n".into())],
    };
    let b = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Bool("flag".into())],
    };
    let errs = checker.check_dep_type_eq(&a, &b, &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A03011");
}

#[test]
fn dep_check_index_erasure_in_runtime() {
    let mut checker = DependentTypeChecker::new();
    checker.bind_index("n".into(), DepIndex::Nat("n".into()));
    let errs = checker.check_index_erasure(&ident("n"), false, &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A03012");
}

#[test]
fn dep_check_index_erasure_in_ghost_ok() {
    let mut checker = DependentTypeChecker::new();
    checker.bind_index("n".into(), DepIndex::Nat("n".into()));
    let errs = checker.check_index_erasure(&ident("n"), true, &span());
    assert!(errs.is_empty());
}

// ---- InfoFlowChecker ----

#[test]
fn ifc_assignment_upward_ok() {
    let checker = InfoFlowChecker::new();
    let err = checker.check_assignment(SecurityLabel::Confidential, SecurityLabel::Public, &span());
    assert!(err.is_none());
}

#[test]
fn ifc_assignment_downward_error() {
    let checker = InfoFlowChecker::new();
    let err = checker.check_assignment(SecurityLabel::Public, SecurityLabel::Confidential, &span());
    assert_eq!(err.unwrap().code.as_ref(), "A08001");
}

#[test]
fn ifc_declassify_without_annotation() {
    let checker = InfoFlowChecker::new();
    let err = checker.check_declassify(
        SecurityLabel::Restricted,
        SecurityLabel::Public,
        false,
        &span(),
    );
    assert_eq!(err.unwrap().code.as_ref(), "A08002");
}

#[test]
fn ifc_declassify_with_annotation_ok() {
    let checker = InfoFlowChecker::new();
    let err = checker.check_declassify(
        SecurityLabel::Restricted,
        SecurityLabel::Public,
        true,
        &span(),
    );
    assert!(err.is_none());
}

#[test]
fn ifc_purpose_label_mismatch() {
    let mut checker = InfoFlowChecker::new();
    checker.declare_purpose("email".into(), "billing".into());
    let err = checker.check_purpose_label("email", "marketing", &span());
    assert_eq!(err.unwrap().code.as_ref(), "A08003");
}

#[test]
fn ifc_purpose_label_match() {
    let mut checker = InfoFlowChecker::new();
    checker.declare_purpose("email".into(), "billing".into());
    let err = checker.check_purpose_label("email", "billing", &span());
    assert!(err.is_none());
}

#[test]
fn ifc_implicit_flow_error() {
    let checker = InfoFlowChecker::new();
    let err =
        checker.check_implicit_flow(SecurityLabel::Restricted, SecurityLabel::Public, &span());
    assert_eq!(err.unwrap().code.as_ref(), "A08004");
}

#[test]
fn ifc_covert_channel_sleep() {
    let checker = InfoFlowChecker::new();
    let err = checker.check_covert_channel(SecurityLabel::Confidential, "sleep", &span());
    assert_eq!(err.unwrap().code.as_ref(), "A08005");
}

#[test]
fn ifc_covert_channel_public_ok() {
    let checker = InfoFlowChecker::new();
    let err = checker.check_covert_channel(SecurityLabel::Public, "sleep", &span());
    assert!(err.is_none());
}

#[test]
fn ifc_infer_label_ident() {
    let mut checker = InfoFlowChecker::new();
    checker.declare("secret".into(), SecurityLabel::Restricted);
    assert_eq!(
        checker.infer_label(&ident("secret")),
        SecurityLabel::Restricted
    );
}

#[test]
fn ifc_infer_label_binop_max() {
    let mut checker = InfoFlowChecker::new();
    checker.declare("a".into(), SecurityLabel::Internal);
    checker.declare("b".into(), SecurityLabel::Confidential);
    let expr = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(ident("a")),
        op: BinOp::Add,
        rhs: Box::new(ident("b")),
    });
    assert_eq!(checker.infer_label(&expr), SecurityLabel::Confidential);
}

#[test]
fn ifc_infer_label_literal_is_public() {
    let checker = InfoFlowChecker::new();
    assert_eq!(checker.infer_label(&int_lit(42)), SecurityLabel::Public);
}

#[test]
fn ifc_check_expr_covert_channel_in_if() {
    let mut checker = InfoFlowChecker::new();
    checker.declare("secret".into(), SecurityLabel::Restricted);
    let expr = Spanned::no_span(Expr::If {
        cond: Box::new(ident("secret")),
        then_branch: Box::new(Spanned::no_span(Expr::Call {
            func: Box::new(ident("sleep")),
            args: vec![int_lit(1)],
        })),
        else_branch: None,
    });
    let errs = checker.check_expr(&expr, &span());
    assert!(!errs.is_empty());
    assert!(errs.iter().any(|e| e.code.as_ref() == "A08005"));
}

#[test]
fn ifc_has_labels() {
    let mut checker = InfoFlowChecker::new();
    assert!(!checker.has_labels());
    checker.declare("x".into(), SecurityLabel::Public);
    assert!(checker.has_labels());
}

#[test]
fn ifc_security_label_ordering() {
    assert!(SecurityLabel::Public < SecurityLabel::Internal);
    assert!(SecurityLabel::Internal < SecurityLabel::Confidential);
    assert!(SecurityLabel::Confidential < SecurityLabel::Restricted);
}
