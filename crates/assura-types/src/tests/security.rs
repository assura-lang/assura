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

    let bounds_expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("offset".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("len".into())),
        }),
        op: AstBinOp::Lte,
        rhs: Box::new(AstExpr::Field(
            Box::new(AstExpr::Ident("buf".into())),
            "len".into(),
        )),
    };

    let result = checker.check_bounds_in_requires("buf", &[&bounds_expr], &(0..10));
    assert!(result.is_none(), "should detect bounds check");
}

#[test]
fn memory_checker_bounds_check_missing() {
    // No bounds check -> A08101
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());

    // A requires clause that does not check buffer bounds
    let unrelated_expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("x".into())),
        op: AstBinOp::Gt,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
    };

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
    assert!(result.is_some());
    assert_eq!(result.unwrap().code, "A08102");
}

#[test]
fn memory_checker_bounds_check_with_capacity() {
    // buf.capacity pattern should also be recognized
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.capacity".into());

    let bounds_expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("idx".into())),
        op: AstBinOp::Lt,
        rhs: Box::new(AstExpr::Field(
            Box::new(AstExpr::Ident("buf".into())),
            "capacity".into(),
        )),
    };

    let result = checker.check_bounds_in_requires("buf", &[&bounds_expr], &(0..10));
    assert!(result.is_none(), "should detect capacity bounds check");
}

#[test]
fn memory_checker_bounds_check_in_conjunction() {
    // x > 0 and offset + len <= buf.len -> should detect bounds check
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());

    let bounds_expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("x".into())),
            op: AstBinOp::Gt,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
        }),
        op: AstBinOp::And,
        rhs: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("offset".into())),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Ident("len".into())),
            }),
            op: AstBinOp::Lte,
            rhs: Box::new(AstExpr::Field(
                Box::new(AstExpr::Ident("buf".into())),
                "len".into(),
            )),
        }),
    };

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

    let bounds_expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Field(
            Box::new(AstExpr::Ident("buf".into())),
            "len".into(),
        )),
        op: AstBinOp::Gte,
        rhs: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("offset".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("len".into())),
        }),
    };

    let result = checker.check_bounds_in_requires("buf", &[&bounds_expr], &(0..10));
    assert!(result.is_none(), "should detect buf.len >= expr pattern");
}

#[test]
fn expr_references_var_basic() {
    let expr = AstExpr::Ident("buf".into());
    assert!(expr_references_var(&expr, "buf"));
    assert!(!expr_references_var(&expr, "other"));
}

#[test]
fn expr_references_var_in_binop() {
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("buf".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
    };
    assert!(expr_references_var(&expr, "buf"));
    assert!(!expr_references_var(&expr, "other"));
}

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

    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("buf".into())),
        index: Box::new(AstExpr::Ident("idx".into())),
    };
    let errors = checker.check_expr(&expr, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A09101");
}

#[test]
fn taint_checker_validated_index_passes() {
    // Validated data used as index -> no error
    let mut checker = TaintChecker::new();
    checker.declare("idx".into(), TaintLabel::Validated);

    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("buf".into())),
        index: Box::new(AstExpr::Ident("idx".into())),
    };
    let errors = checker.check_expr(&expr, &(0..1));
    assert!(errors.is_empty(), "validated index should pass: {errors:?}");
}

#[test]
fn taint_checker_trusted_index_passes() {
    // Trusted (default) data -> no error
    let checker = TaintChecker::new();

    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("buf".into())),
        index: Box::new(AstExpr::Ident("idx".into())),
    };
    let errors = checker.check_expr(&expr, &(0..1));
    assert!(errors.is_empty(), "trusted index should pass: {errors:?}");
}

#[test]
fn taint_propagation_through_arithmetic() {
    // If any operand is untrusted, result is untrusted
    let mut checker = TaintChecker::new();
    checker.declare("tainted".into(), TaintLabel::Untrusted);
    checker.declare("safe".into(), TaintLabel::Trusted);

    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("tainted".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Ident("safe".into())),
    };
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Untrusted);
}

#[test]
fn taint_propagation_both_untrusted() {
    // Both operands untrusted -> result untrusted
    let mut checker = TaintChecker::new();
    checker.declare("a".into(), TaintLabel::Untrusted);
    checker.declare("b".into(), TaintLabel::Untrusted);

    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("a".into())),
        op: AstBinOp::Mul,
        rhs: Box::new(AstExpr::Ident("b".into())),
    };
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Untrusted);
}

#[test]
fn taint_validation_removes_taint() {
    // Calling a validation function produces Validated
    let mut checker = TaintChecker::new();
    checker.declare("raw".into(), TaintLabel::Untrusted);

    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("validate".into())),
        args: vec![AstExpr::Ident("raw".into())],
    };
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Validated);
}

#[test]
fn taint_checker_alloc_a09102() {
    // Untrusted data as allocation size -> A09102
    let mut checker = TaintChecker::new();
    checker.declare("sz".into(), TaintLabel::Untrusted);

    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("alloc".into())),
        args: vec![AstExpr::Ident("sz".into())],
    };
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

    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("memcpy_len".into())),
        args: vec![AstExpr::Ident("raw_len".into())],
    };
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

    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("memcpy_len".into())),
        args: vec![AstExpr::Ident("safe_len".into())],
    };
    let errors = checker.check_expr(&expr, &(0..1));
    assert!(errors.is_empty(), "validated data at sink should pass");
}

#[test]
fn taint_infer_literal_trusted() {
    let checker = TaintChecker::new();
    let expr = AstExpr::Literal(AstLit::Int("42".into()));
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Trusted);
}

#[test]
fn taint_infer_unknown_var_trusted() {
    // Undeclared variables default to Trusted
    let checker = TaintChecker::new();
    let expr = AstExpr::Ident("x".into());
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Trusted);
}

#[test]
fn taint_checker_nested_index_propagation() {
    // Tainted data flows through arithmetic to index -> A09101
    let mut checker = TaintChecker::new();
    checker.declare("offset".into(), TaintLabel::Untrusted);

    let index_expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("offset".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
    };
    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("buf".into())),
        index: Box::new(index_expr),
    };
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

// --- T052: Dependent type tests ---

#[test]
fn dep_type_nat_index_valid() {
    let checker = DependentTypeChecker::new();
    let errors = checker.validate_index("n", "Nat", &(0..1));
    assert!(errors.is_empty(), "Nat should be a valid index type");
}

#[test]
fn dep_type_bool_index_valid() {
    let checker = DependentTypeChecker::new();
    let errors = checker.validate_index("flag", "Bool", &(0..1));
    assert!(errors.is_empty(), "Bool should be a valid index type");
}

#[test]
fn dep_type_enum_index_valid() {
    let mut checker = DependentTypeChecker::new();
    checker.register_enum("Mode".into(), vec!["Read".into(), "Write".into()]);
    let errors = checker.validate_index("mode", "Mode", &(0..1));
    assert!(errors.is_empty(), "known enum should be a valid index type");
}

#[test]
fn dep_type_unknown_type_a03006() {
    let checker = DependentTypeChecker::new();
    let errors = checker.validate_index("x", "String", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03006");
}

#[test]
fn dep_type_nat_arithmetic_valid() {
    let mut checker = DependentTypeChecker::new();
    checker.bind_index("n".into(), DepIndex::Nat("n".into()));
    // n + 1 is a valid Nat expression
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("n".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
    };
    let errors = checker.check_index_expr(&expr, &DepIndex::Nat("n".into()), &(0..1));
    assert!(errors.is_empty(), "n + 1 should be valid Nat arithmetic");
}

#[test]
fn dep_type_bool_arithmetic_rejected() {
    let mut checker = DependentTypeChecker::new();
    checker.bind_index("flag".into(), DepIndex::Bool("flag".into()));
    // flag + 1 is NOT valid for a Bool index
    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("flag".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
    };
    let errors = checker.check_index_expr(&expr, &DepIndex::Bool("flag".into()), &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03008");
}

#[test]
fn dep_type_enum_variant_valid() {
    let mut checker = DependentTypeChecker::new();
    checker.register_enum("Mode".into(), vec!["Read".into(), "Write".into()]);
    checker.bind_index(
        "m".into(),
        DepIndex::Enum {
            name: "m".into(),
            enum_type: "Mode".into(),
        },
    );
    let expr = AstExpr::Ident("Read".into());
    let idx = DepIndex::Enum {
        name: "m".into(),
        enum_type: "Mode".into(),
    };
    let errors = checker.check_index_expr(&expr, &idx, &(0..1));
    assert!(errors.is_empty(), "enum variant should be valid");
}

#[test]
fn dep_type_equality_matching() {
    let checker = DependentTypeChecker::new();
    let t1 = DepType {
        base: Type::List(Box::new(Type::Int)),
        indices: vec![DepIndex::Nat("n".into())],
    };
    let t2 = DepType {
        base: Type::List(Box::new(Type::Int)),
        indices: vec![DepIndex::Nat("m".into())],
    };
    let errors = checker.check_dep_type_eq(&t1, &t2, &(0..1));
    assert!(errors.is_empty(), "same structure should match");
}

#[test]
fn dep_type_equality_base_mismatch() {
    let checker = DependentTypeChecker::new();
    let t1 = DepType {
        base: Type::List(Box::new(Type::Int)),
        indices: vec![DepIndex::Nat("n".into())],
    };
    let t2 = DepType {
        base: Type::List(Box::new(Type::Float)),
        indices: vec![DepIndex::Nat("n".into())],
    };
    let errors = checker.check_dep_type_eq(&t1, &t2, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03010");
}

#[test]
fn dep_type_equality_index_count_mismatch() {
    let checker = DependentTypeChecker::new();
    let t1 = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Nat("n".into())],
    };
    let t2 = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Nat("n".into()), DepIndex::Bool("b".into())],
    };
    let errors = checker.check_dep_type_eq(&t1, &t2, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03010");
}

#[test]
fn dep_type_index_erasure_ghost_ok() {
    let mut checker = DependentTypeChecker::new();
    checker.bind_index("n".into(), DepIndex::Nat("n".into()));
    let expr = AstExpr::Ident("n".into());
    let errors = checker.check_index_erasure(&expr, true, &(0..1));
    assert!(errors.is_empty(), "index in ghost context is ok");
}

#[test]
fn dep_type_index_erasure_runtime_error() {
    let mut checker = DependentTypeChecker::new();
    checker.bind_index("n".into(), DepIndex::Nat("n".into()));
    let expr = AstExpr::Ident("n".into());
    let errors = checker.check_index_erasure(&expr, false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03012");
}

#[test]
fn dep_type_index_kind_mismatch() {
    let checker = DependentTypeChecker::new();
    let t1 = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Nat("n".into())],
    };
    let t2 = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Bool("b".into())],
    };
    let errors = checker.check_dep_type_eq(&t1, &t2, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03011");
}

#[test]
fn dep_type_display() {
    assert_eq!(DepIndex::Nat("n".into()).to_string(), "n: Nat");
    assert_eq!(DepIndex::Bool("flag".into()).to_string(), "flag: Bool");
    assert_eq!(
        DepIndex::Enum {
            name: "m".into(),
            enum_type: "Mode".into()
        }
        .to_string(),
        "m: Mode"
    );
}

// --- T058: FFI boundary contract tests ---

#[test]
fn ffi_extern_without_boundary_a11001() {
    let checker = FfiBoundaryChecker::new();
    let errors = checker.check_extern_decl("malloc", false, false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A11001");
}

#[test]
fn ffi_extern_with_boundary_ok() {
    let checker = FfiBoundaryChecker::new();
    let errors = checker.check_extern_decl("malloc", true, true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ffi_untrusted_without_contract_a11002() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("read_bytes".into(), TrustBoundary::Untrusted);
    let errors = checker.check_extern_decl("read_bytes", true, false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A11002");
}

#[test]
fn ffi_untrusted_with_contract_ok() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("read_bytes".into(), TrustBoundary::Untrusted);
    let errors = checker.check_extern_decl("read_bytes", true, true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ffi_trusted_no_contract_ok() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("internal_fn".into(), TrustBoundary::Trusted);
    let errors = checker.check_extern_decl("internal_fn", true, false, &(0..1));
    assert!(errors.is_empty(), "trusted extern doesn't need a contract");
}

#[test]
fn ffi_call_untrusted_unvalidated_a11003() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("read_raw".into(), TrustBoundary::Untrusted);
    let errors = checker.check_ffi_call("read_raw", false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A11003");
}

#[test]
fn ffi_call_untrusted_validated_ok() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("read_raw".into(), TrustBoundary::Untrusted);
    let errors = checker.check_ffi_call("read_raw", true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ffi_call_trusted_unvalidated_ok() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("safe_fn".into(), TrustBoundary::Trusted);
    let errors = checker.check_ffi_call("safe_fn", false, &(0..1));
    assert!(errors.is_empty(), "trusted calls don't need validation");
}

#[test]
fn ffi_unsafe_outside_wrapper_a11004() {
    let checker = FfiBoundaryChecker::new();
    let errors = checker.check_unsafe_confinement("compute", false, true, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A11004");
}

#[test]
fn ffi_unsafe_inside_wrapper_ok() {
    let checker = FfiBoundaryChecker::new();
    let errors = checker.check_unsafe_confinement("ffi_wrapper", true, true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ffi_boundary_display() {
    assert_eq!(TrustBoundary::Trusted.to_string(), "trusted");
    assert_eq!(TrustBoundary::Audited.to_string(), "audited");
    assert_eq!(TrustBoundary::Untrusted.to_string(), "untrusted");
}

#[test]
fn ffi_file_check_multiple_externs() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("read".into(), TrustBoundary::Untrusted);
    checker.register_extern("write".into(), TrustBoundary::Audited);
    let externs = vec![
        ("read".into(), true, false, 0..5), // untrusted, no contract -> A11002
        ("write".into(), true, true, 10..15), // audited, has contract -> ok
        ("unknown".into(), false, false, 20..25), // no boundary -> A11001
    ];
    let errors = checker.check_file(&externs);
    assert_eq!(errors.len(), 2); // A11002 for read, A11001 for unknown
}

// --- T062: Interface contract tests ---

#[test]
fn interface_missing_method_a13001() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Serializable".into(),
        methods: vec![
            InterfaceMethod {
                name: "serialize".into(),
                param_types: vec![],
                return_type: Type::Bytes,
                has_requires: false,
                has_ensures: true,
                no_reentrancy: false,
            },
            InterfaceMethod {
                name: "deserialize".into(),
                param_types: vec![Type::Bytes],
                return_type: Type::Named("Self".into()),
                has_requires: true,
                has_ensures: true,
                no_reentrancy: false,
            },
        ],
        extends: vec![],
    });

    // Only implement serialize, not deserialize
    let errors = checker.check_impl("MyType", "Serializable", &["serialize".into()], &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A13001");
    assert!(errors[0].message.contains("deserialize"));
}

#[test]
fn interface_all_methods_implemented_ok() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Hashable".into(),
        methods: vec![InterfaceMethod {
            name: "hash".into(),
            param_types: vec![],
            return_type: Type::U64,
            has_requires: false,
            has_ensures: true,
            no_reentrancy: false,
        }],
        extends: vec![],
    });

    let errors = checker.check_impl("MyType", "Hashable", &["hash".into()], &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn interface_signature_param_count_mismatch_a13002() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Comparable".into(),
        methods: vec![InterfaceMethod {
            name: "compare".into(),
            param_types: vec![Type::Int, Type::Int],
            return_type: Type::Bool,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }],
        extends: vec![],
    });

    let errors = checker.check_method_signature(
        "Comparable",
        "compare",
        &[Type::Int], // only 1 param
        &Type::Bool,
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A13002");
}

#[test]
fn interface_signature_return_type_mismatch_a13002() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Comparable".into(),
        methods: vec![InterfaceMethod {
            name: "compare".into(),
            param_types: vec![Type::Int],
            return_type: Type::Bool,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }],
        extends: vec![],
    });

    let errors = checker.check_method_signature(
        "Comparable",
        "compare",
        &[Type::Int],
        &Type::Int, // wrong return type
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A13002");
    assert!(errors[0].message.contains("return type"));
}

#[test]
fn interface_reentrancy_violation_a13003() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Callback".into(),
        methods: vec![InterfaceMethod {
            name: "on_event".into(),
            param_types: vec![],
            return_type: Type::Unit,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: true,
        }],
        extends: vec![],
    });

    let errors = checker.check_reentrancy("Callback", "on_event", true, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A13003");
}

#[test]
fn interface_reentrancy_no_flag_ok() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Callback".into(),
        methods: vec![InterfaceMethod {
            name: "on_event".into(),
            param_types: vec![],
            return_type: Type::Unit,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }],
        extends: vec![],
    });

    let errors = checker.check_reentrancy("Callback", "on_event", true, &(0..1));
    assert!(errors.is_empty(), "method allows reentrancy");
}

#[test]
fn interface_super_interface_inheritance() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Eq".into(),
        methods: vec![InterfaceMethod {
            name: "equals".into(),
            param_types: vec![Type::Named("Self".into())],
            return_type: Type::Bool,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }],
        extends: vec![],
    });
    checker.register_interface(InterfaceContract {
        name: "Ord".into(),
        methods: vec![InterfaceMethod {
            name: "compare_to".into(),
            param_types: vec![Type::Named("Self".into())],
            return_type: Type::Int,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }],
        extends: vec!["Eq".into()],
    });

    // Implement compare_to but not equals -> A13001 for missing super method
    let errors = checker.check_impl("MyType", "Ord", &["compare_to".into()], &(0..1));
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("equals"));
    assert!(errors[0].message.contains("Eq"));
}

#[test]
fn interface_unknown_interface_a13001() {
    let checker = InterfaceChecker::new();
    let errors = checker.check_impl("MyType", "Unknown", &[], &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A13001");
    assert!(errors[0].message.contains("Unknown"));
}

// --- T059: Constant-time execution tests ---

#[test]
fn ct_branch_on_secret_a14001() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("key".into());
    let cond = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("key".into())),
        op: AstBinOp::Eq,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
    };
    let errors = checker.check_branch(&cond, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A14001");
}

#[test]
fn ct_branch_on_public_ok() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("key".into());
    let cond = AstExpr::Ident("public_val".into());
    let errors = checker.check_branch(&cond, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ct_index_on_secret_a14002() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("secret_idx".into());
    let idx = AstExpr::Ident("secret_idx".into());
    let errors = checker.check_index(&idx, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A14002");
}

#[test]
fn ct_index_on_public_ok() {
    let checker = ConstantTimeChecker::new();
    let idx = AstExpr::Ident("i".into());
    let errors = checker.check_index(&idx, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ct_nested_secret_in_condition() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("password".into());
    // password + 1 == 42
    let cond = AstExpr::BinOp {
        lhs: Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("password".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        }),
        op: AstBinOp::Eq,
        rhs: Box::new(AstExpr::Literal(AstLit::Int("42".into()))),
    };
    let errors = checker.check_branch(&cond, &(0..1));
    assert_eq!(errors.len(), 1);
}

#[test]
fn ct_check_expr_if_with_secret() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("s".into());
    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Ident("s".into())),
        then_branch: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        else_branch: Some(Box::new(AstExpr::Literal(AstLit::Int("0".into())))),
    };
    let errors = checker.check_expr(&expr, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A14001");
}

#[test]
fn ct_references_secret_field() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("key".into());
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("key".into())), "len".into());
    assert!(checker.references_secret(&expr));
}

// --- T063: Recursive structural invariant tests ---

#[test]
fn struct_inv_tree_balance_valid() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("AVLTree".into(), vec!["left".into(), "right".into()]);
    let errors = checker.check_invariant_applicability(
        "AVLTree",
        &InvariantKind::TreeBalance { max_diff: 1 },
        &(0..1),
    );
    assert!(errors.is_empty());
}

#[test]
fn struct_inv_on_non_recursive_a15001() {
    let checker = StructuralInvariantChecker::new();
    let errors = checker.check_invariant_applicability(
        "Point",
        &InvariantKind::Sorted { descending: false },
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A15001");
}

#[test]
fn struct_inv_tree_on_list_a15002() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("LinkedList".into(), vec!["next".into()]);
    let errors =
        checker.check_invariant_applicability("LinkedList", &InvariantKind::BstOrdering, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A15002");
}

#[test]
fn struct_inv_sort_on_tree_a15003() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("BTree".into(), vec!["left".into(), "right".into()]);
    let errors = checker.check_invariant_applicability(
        "BTree",
        &InvariantKind::Sorted { descending: false },
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A15003");
}

#[test]
fn struct_inv_acyclic_valid_for_any_recursive() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("Graph".into(), vec!["children".into()]);
    let errors = checker.check_invariant_applicability("Graph", &InvariantKind::Acyclic, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn struct_inv_operation_no_proof_a15004() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("BST".into(), vec!["left".into(), "right".into()]);
    checker.register_invariant(StructuralInvariant {
        name: "bst_order".into(),
        type_name: "BST".into(),
        kind: InvariantKind::BstOrdering,
    });
    let errors = checker.check_operation_preserves(
        "BST",
        "insert",
        true,  // modifies structure
        false, // no preservation proof
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A15004");
}

#[test]
fn struct_inv_operation_with_proof_ok() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("BST".into(), vec!["left".into(), "right".into()]);
    checker.register_invariant(StructuralInvariant {
        name: "bst_order".into(),
        type_name: "BST".into(),
        kind: InvariantKind::BstOrdering,
    });
    let errors = checker.check_operation_preserves(
        "BST",
        "insert",
        true, // modifies structure
        true, // has preservation proof
        &(0..1),
    );
    assert!(errors.is_empty());
}

#[test]
fn struct_inv_readonly_trivially_preserves() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("BST".into(), vec!["left".into(), "right".into()]);
    checker.register_invariant(StructuralInvariant {
        name: "bst_order".into(),
        type_name: "BST".into(),
        kind: InvariantKind::BstOrdering,
    });
    let errors = checker.check_operation_preserves(
        "BST",
        "search",
        false, // read-only
        false, // no proof needed
        &(0..1),
    );
    assert!(errors.is_empty(), "read-only ops preserve invariants");
}

#[test]
fn struct_inv_kind_display() {
    assert_eq!(
        InvariantKind::TreeBalance { max_diff: 1 }.to_string(),
        "tree_balance(max_diff=1)"
    );
    assert_eq!(
        InvariantKind::Sorted { descending: false }.to_string(),
        "sorted(asc)"
    );
    assert_eq!(InvariantKind::Acyclic.to_string(), "acyclic");
    assert_eq!(InvariantKind::BstOrdering.to_string(), "bst_ordering");
    assert_eq!(
        InvariantKind::HeapProperty { min_heap: true }.to_string(),
        "min_heap"
    );
}

#[test]
fn struct_inv_get_invariants() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("AVL".into(), vec!["left".into(), "right".into()]);
    checker.register_invariant(StructuralInvariant {
        name: "balance".into(),
        type_name: "AVL".into(),
        kind: InvariantKind::TreeBalance { max_diff: 1 },
    });
    checker.register_invariant(StructuralInvariant {
        name: "order".into(),
        type_name: "AVL".into(),
        kind: InvariantKind::BstOrdering,
    });
    assert_eq!(checker.get_invariants("AVL").len(), 2);
    assert!(checker.get_invariants("Unknown").is_empty());
}

// --- T060: Secure erasure tests ---

#[test]
fn secure_erasure_not_zeroized_a16001() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("private_key".into());
    let errors = checker.check_scope_exit("private_key", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A16001");
}

#[test]
fn secure_erasure_zeroized_ok() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("private_key".into());
    checker.mark_zeroized("private_key".into());
    let errors = checker.check_scope_exit("private_key", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn secure_erasure_non_sensitive_ok() {
    let checker = SecureErasureChecker::new();
    let errors = checker.check_scope_exit("public_data", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn secure_erasure_copy_to_non_sensitive_a16002() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("key".into());
    let errors = checker.check_copy("key", "backup", false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A16002");
}

#[test]
fn secure_erasure_copy_to_sensitive_ok() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("key".into());
    let errors = checker.check_copy("key", "key_copy", true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn secure_erasure_return_not_sensitive_a16003() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("derived_key".into());
    let errors = checker.check_return("derived_key", false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A16003");
}

#[test]
fn secure_erasure_check_all_erased() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("key1".into());
    checker.mark_sensitive("key2".into());
    checker.mark_zeroized("key1".into());
    let errors = checker.check_all_erased(&(0..1));
    assert_eq!(errors.len(), 1); // key2 not zeroized
}

// --- T061: Cryptographic conformance tests ---

#[test]
fn crypto_correct_key_size_ok() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_key_size("AES-128-GCM", 128, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn crypto_wrong_key_size_a17001() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_key_size("AES-128-GCM", 256, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A17001");
}

#[test]
fn crypto_correct_nonce_size_ok() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_nonce_size("AES-256-GCM", 12, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn crypto_wrong_nonce_size_a17002() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_nonce_size("AES-256-GCM", 16, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A17002");
}

#[test]
fn crypto_nonce_not_unique_a17003() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_nonce_uniqueness("fixed_nonce", false, false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A17003");
}

#[test]
fn crypto_counter_nonce_ok() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_nonce_uniqueness("counter", true, false, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn crypto_tag_not_verified_a17004() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_tag_verification(false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A17004");
}

#[test]
fn crypto_tag_verified_ok() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_tag_verification(true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn crypto_chacha20_key_size() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_key_size("ChaCha20-Poly1305", 256, &(0..1));
    assert!(errors.is_empty());
    let errors = checker.check_key_size("ChaCha20-Poly1305", 128, &(0..1));
    assert_eq!(errors.len(), 1);
}

#[test]
fn crypto_custom_spec() {
    let mut checker = CryptoConformanceChecker::new();
    checker.register_spec(CryptoSpec {
        name: "XSalsa20".into(),
        key_size_bits: vec![256],
        block_size_bytes: None,
        nonce_size_bytes: Some(24),
        tag_size_bytes: None,
    });
    let errors = checker.check_nonce_size("XSalsa20", 24, &(0..1));
    assert!(errors.is_empty());
    let errors = checker.check_nonce_size("XSalsa20", 12, &(0..1));
    assert_eq!(errors.len(), 1);
}

// --- T065: Shared memory protocol tests ---

#[test]
fn shared_mem_read_exclusive_ok() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buffer".into(), AccessMode::Exclusive);
    let errors = checker.check_read("buffer", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn shared_mem_read_shared_ok() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buffer".into(), AccessMode::SharedRead);
    let errors = checker.check_read("buffer", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn shared_mem_read_none_a18001() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buffer".into(), AccessMode::None);
    let errors = checker.check_read("buffer", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A18001");
}

#[test]
fn shared_mem_write_exclusive_ok() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buffer".into(), AccessMode::Exclusive);
    let errors = checker.check_write("buffer", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn shared_mem_write_shared_a18002() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buffer".into(), AccessMode::SharedRead);
    let errors = checker.check_write("buffer", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A18002");
}

#[test]
fn shared_mem_data_race_a18003() {
    let checker = SharedMemChecker::new();
    let errors = checker.check_data_race(
        "counter",
        AccessMode::Exclusive,
        AccessMode::SharedRead,
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A18003");
}

#[test]
fn shared_mem_two_readers_ok() {
    let checker = SharedMemChecker::new();
    let errors = checker.check_data_race(
        "counter",
        AccessMode::SharedRead,
        AccessMode::SharedRead,
        &(0..1),
    );
    assert!(errors.is_empty(), "two shared readers is safe");
}

#[test]
fn shared_mem_access_mode_display() {
    assert_eq!(AccessMode::Exclusive.to_string(), "exclusive");
    assert_eq!(AccessMode::SharedRead.to_string(), "shared_read");
    assert_eq!(AccessMode::None.to_string(), "none");
}

// --- T067: Determinism checker tests ---

#[test]
fn determinism_hashmap_a20001() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("compute".into());
    let errors = checker.check_fn_body("compute", &["HashMap".into(), "Vec".into()], &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A20001");
}

#[test]
fn determinism_btreemap_ok() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("compute".into());
    let errors = checker.check_fn_body("compute", &["BTreeMap".into(), "Vec".into()], &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn determinism_non_det_fn_ok() {
    let checker = DeterminismChecker::new();
    // Not marked deterministic
    let errors = checker.check_fn_body("random_pick", &["random".into()], &(0..1));
    assert!(errors.is_empty(), "non-deterministic fn allows random");
}

#[test]
fn determinism_iteration_a20002() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("process".into());
    let errors = checker.check_iteration("process", "HashMap<K,V>", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A20002");
}

#[test]
fn determinism_btree_iteration_ok() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("process".into());
    let errors = checker.check_iteration("process", "BTreeMap<K,V>", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn determinism_random_a20001() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("seed_fn".into());
    let errors = checker.check_fn_body("seed_fn", &["thread_rng".into()], &(0..1));
    assert_eq!(errors.len(), 1);
}

// --- T068: Lock ordering tests ---

#[test]
fn lock_order_correct_ok() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("db".into(), 1);
    checker.define_order("cache".into(), 2);
    let errors = checker.acquire("db", &(0..1));
    assert!(errors.is_empty());
    let errors = checker.acquire("cache", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn lock_order_violation_a21001() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("db".into(), 1);
    checker.define_order("cache".into(), 2);
    let errors = checker.acquire("cache", &(0..1));
    assert!(errors.is_empty());
    let errors = checker.acquire("db", &(0..1)); // wrong order
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A21001");
}

#[test]
fn lock_order_release_correct() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("a".into(), 1);
    checker.define_order("b".into(), 2);
    checker.acquire("a", &(0..1));
    checker.acquire("b", &(0..1));
    let errors = checker.release("b", &(0..1)); // correct: LIFO
    assert!(errors.is_empty());
}

#[test]
fn lock_order_release_wrong_a21002() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("a".into(), 1);
    checker.define_order("b".into(), 2);
    checker.acquire("a", &(0..1));
    checker.acquire("b", &(0..1));
    let errors = checker.release("a", &(0..1)); // wrong: b still held
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A21002");
}

#[test]
fn lock_order_undefined_a21003() {
    let checker = LockOrderChecker::new();
    let errors = checker.check_ordering_defined("unknown_lock", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A21003");
}

#[test]
fn lock_order_defined_ok() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("db".into(), 1);
    let errors = checker.check_ordering_defined("db", &(0..1));
    assert!(errors.is_empty());
}

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
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A08001");
}

#[test]
fn info_flow_restricted_to_internal_a08001() {
    // Restricted -> Internal is a violation (A08001)
    let checker = InfoFlowChecker::new();
    let err = checker.check_assignment(SecurityLabel::Internal, SecurityLabel::Restricted, &(0..1));
    assert!(err.is_some());
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
    assert!(err.is_some());
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

    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("secret".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Ident("pub_val".into())),
    };
    assert_eq!(checker.infer_label(&expr), SecurityLabel::Confidential);
}

#[test]
fn info_flow_label_propagation_both_restricted() {
    // Both operands Restricted -> result Restricted
    let mut checker = InfoFlowChecker::new();
    checker.declare("a".into(), SecurityLabel::Restricted);
    checker.declare("b".into(), SecurityLabel::Restricted);

    let expr = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("a".into())),
        op: AstBinOp::Mul,
        rhs: Box::new(AstExpr::Ident("b".into())),
    };
    assert_eq!(checker.infer_label(&expr), SecurityLabel::Restricted);
}

#[test]
fn info_flow_infer_literal_public() {
    // Literals are always Public
    let checker = InfoFlowChecker::new();
    let expr = AstExpr::Literal(AstLit::Int("42".into()));
    assert_eq!(checker.infer_label(&expr), SecurityLabel::Public);
}

#[test]
fn info_flow_infer_unknown_var_public() {
    // Undeclared variables default to Public
    let checker = InfoFlowChecker::new();
    let expr = AstExpr::Ident("x".into());
    assert_eq!(checker.infer_label(&expr), SecurityLabel::Public);
}

#[test]
fn info_flow_purpose_label_mismatch_a08003() {
    // Purpose mismatch -> A08003
    let mut checker = InfoFlowChecker::new();
    checker.declare_purpose("email".into(), "marketing".into());
    let err = checker.check_purpose_label("email", "billing", &(0..1));
    assert!(err.is_some());
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
    assert!(err.is_some());
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
    assert!(err.is_some());
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

    let inner = AstExpr::BinOp {
        lhs: Box::new(AstExpr::Ident("pub_val".into())),
        op: AstBinOp::Add,
        rhs: Box::new(AstExpr::Ident("conf".into())),
    };
    let outer = AstExpr::BinOp {
        lhs: Box::new(inner),
        op: AstBinOp::Mul,
        rhs: Box::new(AstExpr::Ident("restr".into())),
    };
    assert_eq!(checker.infer_label(&outer), SecurityLabel::Restricted);
}

#[test]
fn info_flow_label_field_access() {
    // Field access propagates receiver label
    let mut checker = InfoFlowChecker::new();
    checker.declare("secret_obj".into(), SecurityLabel::Confidential);
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("secret_obj".into())), "name".into());
    assert_eq!(checker.infer_label(&expr), SecurityLabel::Confidential);
}

#[test]
fn info_flow_check_expr_if_covert_channel() {
    // If a confidential condition controls a sleep call inside a
    // branch, check_expr should detect the covert channel (A08005).
    let mut checker = InfoFlowChecker::new();
    checker.declare("is_admin".into(), SecurityLabel::Confidential);

    let expr = AstExpr::If {
        cond: Box::new(AstExpr::Ident("is_admin".into())),
        then_branch: Box::new(AstExpr::Call {
            func: Box::new(AstExpr::Ident("sleep".into())),
            args: vec![AstExpr::Literal(AstLit::Int("100".into()))],
        }),
        else_branch: None,
    };
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
        checker.infer_label(&AstExpr::Ident("pub_data".into())),
        SecurityLabel::Public
    );
    assert_eq!(
        checker.infer_label(&AstExpr::Ident("restr_data".into())),
        SecurityLabel::Restricted
    );
}

#[test]
fn info_flow_checker_default() {
    // Default implementation matches new()
    let checker: InfoFlowChecker = Default::default();
    assert!(!checker.has_labels());
}

// --- T053 test helpers ---

fn make_fn_def(name: &str, params: Vec<(&str, &[&str])>, clauses: Vec<AstClause>) -> AstFnDef {
    AstFnDef {
        name: name.into(),
        is_ghost: false,
        is_lemma: false,
        params: params
            .into_iter()
            .map(|(n, ty)| {
                let tokens: Vec<String> = ty.iter().map(|s| s.to_string()).collect();
                AstParam {
                    name: n.into(),
                    ty: assura_parser::ast::try_parse_type_tokens(&tokens),
                }
            })
            .collect(),
        return_ty: assura_parser::ast::try_parse_type_tokens(&["Int".to_string()]),
        clauses,
    }
}

fn decreases_clause(body: AstExpr) -> AstClause {
    AstClause {
        kind: ClauseKind::Other("decreases".into()),
        body,
        effect_variables: vec![],
    }
}

fn requires_clause(body: AstExpr) -> AstClause {
    AstClause {
        kind: ClauseKind::Requires,
        body,
        effect_variables: vec![],
    }
}

fn partial_clause() -> AstClause {
    AstClause {
        kind: ClauseKind::Other("partial".into()),
        body: AstExpr::Literal(AstLit::Bool(true)),
        effect_variables: vec![],
    }
}

fn ensures_with_recursive_call(fn_name: &str, args: Vec<AstExpr>) -> AstClause {
    AstClause {
        kind: ClauseKind::Ensures,
        body: AstExpr::Call {
            func: Box::new(AstExpr::Ident(fn_name.into())),
            args,
        },
        effect_variables: vec![],
    }
}

#[test]
fn totality_non_recursive_trivially_total() {
    // Non-recursive function passes without decreases
    let fn_def = make_fn_def("add", vec![("a", &["Int"]), ("b", &["Int"])], vec![]);
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..10));
    assert!(
        errors.is_empty(),
        "non-recursive function should be trivially total"
    );
}

#[test]
fn totality_recursive_with_valid_decreases() {
    // factorial(n) with decreases n, recursive call factorial(n - 1)
    let fn_def = make_fn_def(
        "factorial",
        vec![("n", &["Nat"])],
        vec![
            decreases_clause(AstExpr::Ident("n".into())),
            ensures_with_recursive_call(
                "factorial",
                vec![AstExpr::BinOp {
                    lhs: Box::new(AstExpr::Ident("n".into())),
                    op: AstBinOp::Sub,
                    rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                }],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..20));
    assert!(
        errors.is_empty(),
        "valid decreasing measure should pass: {errors:?}"
    );
}

#[test]
fn totality_recursive_without_decreases_a09001() {
    // Recursive function without decreases clause -> A09001
    let fn_def = make_fn_def(
        "loop_forever",
        vec![("n", &["Int"])],
        vec![ensures_with_recursive_call(
            "loop_forever",
            vec![AstExpr::Ident("n".into())],
        )],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A09001");
}

#[test]
fn totality_non_decreasing_measure_deferred_to_smt() {
    // Recursive call with same argument (not decreasing) is now deferred to SMT
    // instead of immediately producing A09002. The SMT solver will find that
    // n < n is unsatisfiable and report the error.
    let fn_def = make_fn_def(
        "spin",
        vec![("n", &["Nat"])],
        vec![
            decreases_clause(AstExpr::Ident("n".into())),
            ensures_with_recursive_call("spin", vec![AstExpr::Ident("n".into())]),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, pending) = checker.check_function_totality(&fn_def, &(0..10));
    // No syntactic error; the check is deferred to SMT
    assert!(
        errors.is_empty(),
        "non-decreasing measure should be deferred to SMT, not produce syntactic error: {errors:?}"
    );
    assert!(
        !pending.is_empty(),
        "non-decreasing measure should produce a pending SMT check"
    );
    // The pending check should reference the spin function
    assert_eq!(pending[0].fn_name, "spin");
}

#[test]
fn totality_measure_not_well_founded_a09003() {
    // decreases n but no requires n >= 0 and param type is Int, not Nat
    let fn_def = make_fn_def(
        "bad_rec",
        vec![("n", &["Int"])],
        vec![
            decreases_clause(AstExpr::Ident("n".into())),
            ensures_with_recursive_call(
                "bad_rec",
                vec![AstExpr::BinOp {
                    lhs: Box::new(AstExpr::Ident("n".into())),
                    op: AstBinOp::Sub,
                    rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                }],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..10));
    assert!(
        errors.iter().any(|e| e.code == "A09003"),
        "missing well-foundedness should produce A09003: {errors:?}"
    );
}

#[test]
fn totality_well_founded_with_requires_clause() {
    // decreases n with requires n >= 0 should NOT produce A09003
    let fn_def = make_fn_def(
        "count_down",
        vec![("n", &["Int"])],
        vec![
            requires_clause(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("n".into())),
                op: AstBinOp::Gte,
                rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
            }),
            decreases_clause(AstExpr::Ident("n".into())),
            ensures_with_recursive_call(
                "count_down",
                vec![AstExpr::BinOp {
                    lhs: Box::new(AstExpr::Ident("n".into())),
                    op: AstBinOp::Sub,
                    rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                }],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..20));
    assert!(
        !errors.iter().any(|e| e.code == "A09003"),
        "requires n >= 0 should establish well-foundedness: {errors:?}"
    );
}

#[test]
fn totality_partial_escape_hatch() {
    // Partial function skips termination checking
    let fn_def = make_fn_def(
        "diverge",
        vec![("n", &["Int"])],
        vec![
            partial_clause(),
            ensures_with_recursive_call("diverge", vec![AstExpr::Ident("n".into())]),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..10));
    assert!(
        errors.is_empty(),
        "partial function should skip totality check"
    );
}

#[test]
fn totality_partial_via_register() {
    // Partial registered via mark_partial
    let fn_def = make_fn_def(
        "diverge2",
        vec![("n", &["Int"])],
        vec![ensures_with_recursive_call(
            "diverge2",
            vec![AstExpr::Ident("n".into())],
        )],
    );
    let mut checker = TotalityChecker::new();
    checker.mark_partial("diverge2".into());
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..10));
    assert!(errors.is_empty(), "registered partial should skip check");
}

#[test]
fn totality_lexicographic_measures() {
    // Ackermann-like: decreases (m, n) with call (m - 1, n)
    let fn_def = make_fn_def(
        "ack",
        vec![("m", &["Nat"]), ("n", &["Nat"])],
        vec![
            decreases_clause(AstExpr::Ident("m".into())),
            decreases_clause(AstExpr::Ident("n".into())),
            ensures_with_recursive_call(
                "ack",
                vec![
                    AstExpr::BinOp {
                        lhs: Box::new(AstExpr::Ident("m".into())),
                        op: AstBinOp::Sub,
                        rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                    },
                    AstExpr::Ident("n".into()),
                ],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..20));
    assert!(
        errors.is_empty(),
        "lexicographic decrease in first component should pass: {errors:?}"
    );
}

#[test]
fn totality_mutual_recursion_no_decreases_a09004() {
    // Two functions calling each other with no decreases -> A09004
    let fn_a = make_fn_def(
        "even",
        vec![("n", &["Nat"])],
        vec![ensures_with_recursive_call(
            "odd",
            vec![AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("n".into())),
                op: AstBinOp::Sub,
                rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            }],
        )],
    );
    let fn_b = make_fn_def(
        "odd",
        vec![("n", &["Nat"])],
        vec![ensures_with_recursive_call(
            "even",
            vec![AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("n".into())),
                op: AstBinOp::Sub,
                rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            }],
        )],
    );

    let checker = TotalityChecker::new();
    let span_a = 0..10;
    let span_b = 10..20;
    let fn_defs: Vec<(&AstFnDef, &Range<usize>)> = vec![(&fn_a, &span_a), (&fn_b, &span_b)];
    let errors = checker.check_mutual_recursion(&fn_defs);
    assert!(
        errors.iter().any(|e| e.code == "A09004"),
        "mutual recursion without decreases should produce A09004: {errors:?}"
    );
}

#[test]
fn totality_mutual_recursion_with_decreases_passes() {
    // Two functions calling each other, one has decreases -> passes
    let fn_a = make_fn_def(
        "even2",
        vec![("n", &["Nat"])],
        vec![
            decreases_clause(AstExpr::Ident("n".into())),
            ensures_with_recursive_call(
                "odd2",
                vec![AstExpr::BinOp {
                    lhs: Box::new(AstExpr::Ident("n".into())),
                    op: AstBinOp::Sub,
                    rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                }],
            ),
        ],
    );
    let fn_b = make_fn_def(
        "odd2",
        vec![("n", &["Nat"])],
        vec![ensures_with_recursive_call(
            "even2",
            vec![AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("n".into())),
                op: AstBinOp::Sub,
                rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            }],
        )],
    );

    let checker = TotalityChecker::new();
    let span_a = 0..10;
    let span_b = 10..20;
    let fn_defs: Vec<(&AstFnDef, &Range<usize>)> = vec![(&fn_a, &span_a), (&fn_b, &span_b)];
    let errors = checker.check_mutual_recursion(&fn_defs);
    assert!(
        errors.is_empty(),
        "mutual recursion with decreases should pass: {errors:?}"
    );
}

#[test]
fn totality_structural_recursion_on_list() {
    // list_len(xs) with decreases xs, recursive call list_len(xs.tail)
    let fn_def = make_fn_def(
        "list_len",
        vec![("xs", &["List"])],
        vec![
            decreases_clause(AstExpr::Ident("xs".into())),
            ensures_with_recursive_call(
                "list_len",
                vec![AstExpr::Field(
                    Box::new(AstExpr::Ident("xs".into())),
                    "tail".into(),
                )],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..20));
    assert!(
        errors.is_empty(),
        "structural recursion on .tail should pass: {errors:?}"
    );
}

#[test]
fn totality_structural_recursion_on_tree() {
    // tree_depth(node) with decreases node, calls tree_depth(node.left)
    let fn_def = make_fn_def(
        "tree_depth",
        vec![("node", &["Tree"])],
        vec![
            decreases_clause(AstExpr::Ident("node".into())),
            ensures_with_recursive_call(
                "tree_depth",
                vec![AstExpr::Field(
                    Box::new(AstExpr::Ident("node".into())),
                    "left".into(),
                )],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..20));
    assert!(
        errors.is_empty(),
        "structural recursion on .left should pass: {errors:?}"
    );
}

#[test]
fn totality_extract_no_decreases() {
    let fn_def = make_fn_def("f", vec![], vec![]);
    let checker = TotalityChecker::new();
    assert!(checker.extract_decreases_measure(&fn_def).is_none());
}

#[test]
fn totality_extract_single_decreases() {
    let fn_def = make_fn_def(
        "f",
        vec![("n", &["Nat"])],
        vec![decreases_clause(AstExpr::Ident("n".into()))],
    );
    let checker = TotalityChecker::new();
    let measure = checker.extract_decreases_measure(&fn_def);
    assert!(
        matches!(measure, Some(DecreasesMeasure::Natural(_))),
        "single decreases should yield Natural"
    );
}

#[test]
fn totality_extract_lexicographic_decreases() {
    let fn_def = make_fn_def(
        "f",
        vec![("m", &["Nat"]), ("n", &["Nat"])],
        vec![
            decreases_clause(AstExpr::Ident("m".into())),
            decreases_clause(AstExpr::Ident("n".into())),
        ],
    );
    let checker = TotalityChecker::new();
    let measure = checker.extract_decreases_measure(&fn_def);
    assert!(
        matches!(measure, Some(DecreasesMeasure::Lexicographic(ref v)) if v.len() == 2),
        "two decreases should yield Lexicographic(2)"
    );
}

#[test]
fn totality_checker_debug() {
    let checker = TotalityChecker::new();
    let dbg = format!("{checker:?}");
    assert!(dbg.contains("TotalityChecker"));
}

#[test]
fn totality_checker_default() {
    let checker = TotalityChecker::default();
    assert!(!checker.is_partial(&make_fn_def("f", vec![], vec![])));
}

// -----------------------------------------------------------------------
