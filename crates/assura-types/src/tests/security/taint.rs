use super::*;

// -----------------------------------------------------------------------
// T047: Taint tracking (SEC.1) tests
// -----------------------------------------------------------------------

#[test]
fn taint_label_ordering() {
    assert!(TaintLabel::Untrusted < TaintLabel::Validated);
    assert!(TaintLabel::Validated < TaintLabel::Trusted);
    assert!(TaintLabel::Untrusted < TaintLabel::Trusted);
}

#[test]
fn extract_taint_from_tokens() {
    let tokens = vec![
        "U32".into(),
        "@".into(),
        "taint".into(),
        ":".into(),
        "untrusted".into(),
    ];
    assert_eq!(
        extract_taint_label_from_tokens(&tokens),
        Some(TaintLabel::Untrusted)
    );

    let tokens2 = vec![
        "ValidXlen".into(),
        "@".into(),
        "taint".into(),
        ":".into(),
        "validated".into(),
    ];
    assert_eq!(
        extract_taint_label_from_tokens(&tokens2),
        Some(TaintLabel::Validated)
    );

    let no_taint = vec!["Int".into()];
    assert_eq!(extract_taint_label_from_tokens(&no_taint), None);
}

#[test]
fn extract_taint_short_form() {
    let tokens = vec!["Bytes".into(), "@".into(), "untrusted".into()];
    assert_eq!(
        extract_taint_label_from_tokens(&tokens),
        Some(TaintLabel::Untrusted)
    );

    let tokens2 = vec!["Data".into(), "@".into(), "validated".into()];
    assert_eq!(
        extract_taint_label_from_tokens(&tokens2),
        Some(TaintLabel::Validated)
    );

    let tokens3 = vec!["Key".into(), "@".into(), "trusted".into()];
    assert_eq!(
        extract_taint_label_from_tokens(&tokens3),
        Some(TaintLabel::Trusted)
    );
}

#[test]
fn taint_checker_untrusted_index_a09101() {
    // Untrusted data used as array index -> A09101
    let mut checker = TaintChecker::new();
    checker.declare("idx".into(), TaintLabel::Untrusted);

    let expr = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("buf".into()))),
        index: Box::new(Spanned::no_span(AstExpr::Ident("idx".into()))),
    });
    let errors = checker.check_expr(&expr, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A09101");
}

#[test]
fn taint_checker_validated_index_passes() {
    // Validated data used as index -> no error
    let mut checker = TaintChecker::new();
    checker.declare("idx".into(), TaintLabel::Validated);

    let expr = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("buf".into()))),
        index: Box::new(Spanned::no_span(AstExpr::Ident("idx".into()))),
    });
    let errors = checker.check_expr(&expr, &(0..1));
    assert!(errors.is_empty(), "validated index should pass: {errors:?}");
}

#[test]
fn taint_checker_trusted_index_passes() {
    // Trusted (default) data -> no error
    let checker = TaintChecker::new();

    let expr = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("buf".into()))),
        index: Box::new(Spanned::no_span(AstExpr::Ident("idx".into()))),
    });
    let errors = checker.check_expr(&expr, &(0..1));
    assert!(errors.is_empty(), "trusted index should pass: {errors:?}");
}

#[test]
fn taint_propagation_through_arithmetic() {
    // If any operand is untrusted, result is untrusted
    let mut checker = TaintChecker::new();
    checker.declare("tainted".into(), TaintLabel::Untrusted);
    checker.declare("safe".into(), TaintLabel::Trusted);

    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("tainted".into()))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("safe".into()))),
    });
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Untrusted);
}

#[test]
fn taint_propagation_both_untrusted() {
    // Both operands untrusted -> result untrusted
    let mut checker = TaintChecker::new();
    checker.declare("a".into(), TaintLabel::Untrusted);
    checker.declare("b".into(), TaintLabel::Untrusted);

    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("a".into()))),
        op: AstBinOp::Mul,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("b".into()))),
    });
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Untrusted);
}

#[test]
fn taint_validation_removes_taint() {
    // Calling a validation function produces Validated
    let mut checker = TaintChecker::new();
    checker.declare("raw".into(), TaintLabel::Untrusted);

    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("validate".into()))),
        args: vec![Spanned::no_span(AstExpr::Ident("raw".into()))],
    });
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Validated);
}

#[test]
fn taint_checker_alloc_a09102() {
    // Untrusted data as allocation size -> A09102
    let mut checker = TaintChecker::new();
    checker.declare("sz".into(), TaintLabel::Untrusted);

    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("alloc".into()))),
        args: vec![Spanned::no_span(AstExpr::Ident("sz".into()))],
    });
    let errors = checker.check_expr(&expr, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A09102");
}

#[test]
fn taint_checker_trusted_sink_a09103() {
    // Untrusted data flowing to a trusted sink -> A09103
    let mut checker = TaintChecker::new();
    checker.declare("raw_len".into(), TaintLabel::Untrusted);
    checker.register_trusted_sink("memcpy_len".into(), vec![Some(TaintLabel::Validated)]);

    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("memcpy_len".into()))),
        args: vec![Spanned::no_span(AstExpr::Ident("raw_len".into()))],
    });
    let errors = checker.check_expr(&expr, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A09103");
}

#[test]
fn taint_checker_validated_at_sink_passes() {
    // Validated data at a sink that requires Validated -> no error
    let mut checker = TaintChecker::new();
    checker.declare("safe_len".into(), TaintLabel::Validated);
    checker.register_trusted_sink("memcpy_len".into(), vec![Some(TaintLabel::Validated)]);

    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("memcpy_len".into()))),
        args: vec![Spanned::no_span(AstExpr::Ident("safe_len".into()))],
    });
    let errors = checker.check_expr(&expr, &(0..1));
    assert!(errors.is_empty(), "validated data at sink should pass");
}

#[test]
fn taint_infer_literal_trusted() {
    let checker = TaintChecker::new();
    let expr = Spanned::no_span(AstExpr::Literal(AstLit::Int("42".into())));
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Trusted);
}

#[test]
fn taint_infer_unknown_var_trusted() {
    // Undeclared variables default to Trusted
    let checker = TaintChecker::new();
    let expr = Spanned::no_span(AstExpr::Ident("x".into()));
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Trusted);
}

#[test]
fn taint_checker_nested_index_propagation() {
    // Tainted data flows through arithmetic to index -> A09101
    let mut checker = TaintChecker::new();
    checker.declare("offset".into(), TaintLabel::Untrusted);

    let index_expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("offset".into()))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
    });
    let expr = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("buf".into()))),
        index: Box::new(index_expr),
    });
    let errors = checker.check_expr(&expr, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A09101");
}

#[test]
fn taint_checker_display() {
    assert_eq!(TaintLabel::Untrusted.to_string(), "untrusted");
    assert_eq!(TaintLabel::Validated.to_string(), "validated");
    assert_eq!(TaintLabel::Trusted.to_string(), "trusted");
}

