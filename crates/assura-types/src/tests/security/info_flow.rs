use super::*;

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
    assert_eq!(err.unwrap().code, "A08001");
}

#[test]
fn info_flow_restricted_to_internal_a08001() {
    // Restricted -> Internal is a violation (A08001)
    let checker = InfoFlowChecker::new();
    let err = checker.check_assignment(SecurityLabel::Internal, SecurityLabel::Restricted, &(0..1));
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

    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("secret".into()))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("pub_val".into()))),
    });
    assert_eq!(checker.infer_label(&expr), SecurityLabel::Confidential);
}

#[test]
fn info_flow_label_propagation_both_restricted() {
    // Both operands Restricted -> result Restricted
    let mut checker = InfoFlowChecker::new();
    checker.declare("a".into(), SecurityLabel::Restricted);
    checker.declare("b".into(), SecurityLabel::Restricted);

    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("a".into()))),
        op: AstBinOp::Mul,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("b".into()))),
    });
    assert_eq!(checker.infer_label(&expr), SecurityLabel::Restricted);
}

#[test]
fn info_flow_infer_literal_public() {
    // Literals are always Public
    let checker = InfoFlowChecker::new();
    let expr = Spanned::no_span(AstExpr::Literal(AstLit::Int("42".into())));
    assert_eq!(checker.infer_label(&expr), SecurityLabel::Public);
}

#[test]
fn info_flow_infer_unknown_var_public() {
    // Undeclared variables default to Public
    let checker = InfoFlowChecker::new();
    let expr = Spanned::no_span(AstExpr::Ident("x".into()));
    assert_eq!(checker.infer_label(&expr), SecurityLabel::Public);
}

#[test]
fn info_flow_purpose_label_mismatch_a08003() {
    // Purpose mismatch -> A08003
    let mut checker = InfoFlowChecker::new();
    checker.declare_purpose("email".into(), "marketing".into());
    let err = checker.check_purpose_label("email", "billing", &(0..1));
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

    let inner = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("pub_val".into()))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("conf".into()))),
    });
    let outer = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(inner),
        op: AstBinOp::Mul,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("restr".into()))),
    });
    assert_eq!(checker.infer_label(&outer), SecurityLabel::Restricted);
}

#[test]
fn info_flow_label_field_access() {
    // Field access propagates receiver label
    let mut checker = InfoFlowChecker::new();
    checker.declare("secret_obj".into(), SecurityLabel::Confidential);
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("secret_obj".into()))),
        "name".into(),
    ));
    assert_eq!(checker.infer_label(&expr), SecurityLabel::Confidential);
}

#[test]
fn info_flow_check_expr_if_covert_channel() {
    // If a confidential condition controls a sleep call inside a
    // branch, check_expr should detect the covert channel (A08005).
    let mut checker = InfoFlowChecker::new();
    checker.declare("is_admin".into(), SecurityLabel::Confidential);

    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Ident("is_admin".into()))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Call {
            func: Box::new(Spanned::no_span(AstExpr::Ident("sleep".into()))),
            args: vec![Spanned::no_span(AstExpr::Literal(AstLit::Int(
                "100".into(),
            )))],
        })),
        else_branch: None,
    });
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
        checker.infer_label(&Spanned::no_span(AstExpr::Ident("pub_data".into()))),
        SecurityLabel::Public
    );
    assert_eq!(
        checker.infer_label(&Spanned::no_span(AstExpr::Ident("restr_data".into()))),
        SecurityLabel::Restricted
    );
}

#[test]
fn info_flow_checker_default() {
    // Default implementation matches new()
    let checker: InfoFlowChecker = Default::default();
    assert!(!checker.has_labels());
}
