use super::*;

// T046: Memory region contracts (MEM.1)
// -----------------------------------------------------------------------

#[test]
fn memory_checker_register_buffer() {
    let mut checker = MemoryChecker::new();
    assert!(!checker.is_buffer("buf"));
    checker.register_buffer("buf".into(), "buf.len".into());
    assert!(checker.is_buffer("buf"));
    assert_eq!(checker.buffer_capacity("buf"), Some("buf.len"));
}

#[test]
fn memory_checker_register_region() {
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());
    checker.register_region(MemoryRegion {
        name: "valid_range".into(),
        lower: "0".into(),
        upper: "buf.len".into(),
        buffer: "buf".into(),
    });
    assert_eq!(checker.regions().len(), 1);
    assert_eq!(checker.regions()[0].name, "valid_range");
}

#[test]
fn memory_checker_bounds_check_present() {
    // offset + len <= buf.len pattern should be recognized
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());

    let bounds_expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::Ident("offset".into()))),
            op: AstBinOp::Add,
            rhs: Box::new(Spanned::no_span(AstExpr::Ident("len".into()))),
        })),
        op: AstBinOp::Lte,
        rhs: Box::new(Spanned::no_span(AstExpr::Field(
            Box::new(Spanned::no_span(AstExpr::Ident("buf".into()))),
            "len".into(),
        ))),
    });

    let result = checker.check_bounds_in_requires("buf", &[&bounds_expr], &(0..10));
    assert!(result.is_none(), "should detect bounds check");
}

#[test]
fn memory_checker_bounds_check_missing() {
    // No bounds check -> A08101
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());

    // A requires clause that does not check buffer bounds
    let unrelated_expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        op: AstBinOp::Gt,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
    });

    let result = checker.check_bounds_in_requires("buf", &[&unrelated_expr], &(0..10));
    assert!(result.is_some(), "should detect missing bounds check");
    let err = result.unwrap();
    assert_eq!(err.code, "A08101");
    assert!(err.message.contains("buf"));
}

#[test]
fn memory_checker_region_buffer_exists() {
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());
    checker.register_region(MemoryRegion {
        name: "r1".into(),
        lower: "0".into(),
        upper: "buf.len".into(),
        buffer: "buf".into(),
    });
    let errors = checker.check_region_buffers(&(0..10));
    assert!(errors.is_empty(), "buffer exists, no errors expected");
}

#[test]
fn memory_checker_region_buffer_missing() {
    let mut checker = MemoryChecker::new();
    // Do NOT register "missing_buf" as a buffer
    checker.register_region(MemoryRegion {
        name: "r1".into(),
        lower: "0".into(),
        upper: "missing_buf.len".into(),
        buffer: "missing_buf".into(),
    });
    let errors = checker.check_region_buffers(&(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A08103");
    assert!(errors[0].message.contains("missing_buf"));
}

#[test]
fn memory_checker_region_containment_same_buffer() {
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());
    checker.register_region(MemoryRegion {
        name: "sub".into(),
        lower: "2".into(),
        upper: "5".into(),
        buffer: "buf".into(),
    });
    checker.register_region(MemoryRegion {
        name: "parent".into(),
        lower: "0".into(),
        upper: "buf.len".into(),
        buffer: "buf".into(),
    });
    let result = checker.check_region_containment("sub", "parent", &(0..10));
    assert!(
        result.is_none(),
        "same buffer regions should pass structural check"
    );
}

#[test]
fn memory_checker_region_containment_different_buffers() {
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf_a".into(), "buf_a.len".into());
    checker.register_buffer("buf_b".into(), "buf_b.len".into());
    checker.register_region(MemoryRegion {
        name: "r_a".into(),
        lower: "0".into(),
        upper: "buf_a.len".into(),
        buffer: "buf_a".into(),
    });
    checker.register_region(MemoryRegion {
        name: "r_b".into(),
        lower: "0".into(),
        upper: "buf_b.len".into(),
        buffer: "buf_b".into(),
    });
    let result = checker.check_region_containment("r_a", "r_b", &(0..10));
    assert!(result.is_some(), "different buffer regions should fail");
    assert_eq!(result.unwrap().code, "A08102");
}

#[test]
fn memory_checker_region_containment_undefined_sub() {
    let checker = MemoryChecker::new();
    let result = checker.check_region_containment("nonexistent", "parent", &(0..10));
    assert_eq!(result.unwrap().code, "A08102");
}

#[test]
fn memory_checker_bounds_check_with_capacity() {
    // buf.capacity pattern should also be recognized
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.capacity".into());

    let bounds_expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("idx".into()))),
        op: AstBinOp::Lt,
        rhs: Box::new(Spanned::no_span(AstExpr::Field(
            Box::new(Spanned::no_span(AstExpr::Ident("buf".into()))),
            "capacity".into(),
        ))),
    });

    let result = checker.check_bounds_in_requires("buf", &[&bounds_expr], &(0..10));
    assert!(result.is_none(), "should detect capacity bounds check");
}

#[test]
fn memory_checker_bounds_check_in_conjunction() {
    // x > 0 and offset + len <= buf.len -> should detect bounds check
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());

    let bounds_expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
            op: AstBinOp::Gt,
            rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
        })),
        op: AstBinOp::And,
        rhs: Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::BinOp {
                lhs: Box::new(Spanned::no_span(AstExpr::Ident("offset".into()))),
                op: AstBinOp::Add,
                rhs: Box::new(Spanned::no_span(AstExpr::Ident("len".into()))),
            })),
            op: AstBinOp::Lte,
            rhs: Box::new(Spanned::no_span(AstExpr::Field(
                Box::new(Spanned::no_span(AstExpr::Ident("buf".into()))),
                "len".into(),
            ))),
        })),
    });

    let result = checker.check_bounds_in_requires("buf", &[&bounds_expr], &(0..10));
    assert!(
        result.is_none(),
        "should detect bounds check in conjunction"
    );
}

#[test]
fn memory_checker_default() {
    let checker = MemoryChecker::default();
    assert!(!checker.is_buffer("anything"));
    assert!(checker.regions().is_empty());
}

#[test]
fn memory_checker_gte_bounds_check() {
    // buf.len >= offset + len pattern should also be recognized
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());

    let bounds_expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Field(
            Box::new(Spanned::no_span(AstExpr::Ident("buf".into()))),
            "len".into(),
        ))),
        op: AstBinOp::Gte,
        rhs: Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::Ident("offset".into()))),
            op: AstBinOp::Add,
            rhs: Box::new(Spanned::no_span(AstExpr::Ident("len".into()))),
        })),
    });

    let result = checker.check_bounds_in_requires("buf", &[&bounds_expr], &(0..10));
    assert!(result.is_none(), "should detect buf.len >= expr pattern");
}

#[test]
fn expr_references_var_basic() {
    let expr = Spanned::no_span(AstExpr::Ident("buf".into()));
    assert!(expr_references_var(&expr, "buf"));
    assert!(!expr_references_var(&expr, "other"));
}

#[test]
fn expr_references_var_in_binop() {
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("buf".into()))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
    });
    assert!(expr_references_var(&expr, "buf"));
    assert!(!expr_references_var(&expr, "other"));
}

