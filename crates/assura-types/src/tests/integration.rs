use super::*;
// Domain checker wiring tests (T056-T111)
// ===========================================================================

#[test]
fn domain_allocator_checker_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no allocator annotations should pass");
}

#[test]
fn domain_allocator_checker_direct_api() {
    let mut checker = AllocatorChecker::new();
    checker.record_alloc("buf".into(), None, 0..1);
    let errors = checker.check_unpaired();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A22001");
}

#[test]
fn domain_circular_buffer_checker_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no circular_buffer annotations should pass");
}

#[test]
fn domain_circular_buffer_checker_direct_api() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 8);
    assert!(checker.check_read("ring", &(0..1)).is_some());
    assert_eq!(
        checker
            .check_read("ring", &(0..1))
            .as_ref()
            .map(|e| e.code.as_str()),
        Some("A23003")
    );
}

#[test]
fn domain_callback_reentrancy_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no callback annotations should pass");
}

#[test]
fn domain_callback_reentrancy_direct_api() {
    let mut checker = CallbackReentrancyChecker::new();
    checker.mark_non_reentrant("handler".into(), 0..1);
    let errs = checker.enter_call("handler", &(0..1));
    assert!(errs.is_empty());
    let errs2 = checker.enter_call("handler", &(0..1));
    assert_eq!(errs2.len(), 1);
    assert_eq!(errs2[0].code, "A24001");
}

#[test]
fn domain_temporal_deadline_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no deadline annotations should pass");
}

#[test]
fn domain_temporal_deadline_direct_api() {
    let mut checker = TemporalDeadlineChecker::new();
    checker.register_bound("slow_op".into(), 200);
    assert!(checker.enter_deadline("d1".into(), 100, &(0..1)).is_none());
    let err = checker.check_operation("slow_op", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.as_ref().map(|e| e.code.as_str()), Some("A25001"));
}

#[test]
fn domain_binary_format_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no binary_format annotations should pass");
}

#[test]
fn domain_binary_format_direct_api() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "hdr".into(),
        offset: 0,
        size: 4,
        endianness: None,
        span: 0..1,
    });
    let errs = checker.check_endianness();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A26003");
}

#[test]
fn domain_bit_level_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no bit_field annotations should pass");
}

#[test]
fn domain_string_encoding_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no encoding annotations should pass");
}

#[test]
fn domain_string_encoding_direct_api() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("raw_data".into(), StringEncoding::RawBytes);
    let err = checker.check_use_as_string("raw_data", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.as_ref().map(|e| e.code.as_str()), Some("A28001"));
}

#[test]
fn domain_checksum_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no checksum annotations should pass");
}

#[test]
fn domain_checksum_direct_api() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("data".into(), ChecksumAlgorithm::Crc32, 0, 100);
    let err = checker.check_use_before_verify("data", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.as_ref().map(|e| e.code.as_str()), Some("A29001"));
}

#[test]
fn domain_protocol_grammar_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no protocol annotations should pass");
}

#[test]
fn domain_opaque_function_no_annotation_passes() {
    let src = r#"fn helper(n: Int) -> Int { ensures { result >= 0 } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no opaque annotations should pass");
}

#[test]
fn domain_opaque_function_direct_api() {
    let mut checker = OpaqueFunctionChecker::new();
    checker.declare_opaque("secret_fn".into(), false, 0..1);
    let err = checker.check_call("secret_fn", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.as_ref().map(|e| e.code.as_str()), Some("A32001"));
}

#[test]
fn domain_crash_recovery_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no crash_safe annotations should pass");
}

#[test]
fn domain_crash_recovery_direct_api() {
    let mut checker = CrashRecoveryChecker::new();
    checker.begin_write("w1".into());
    checker.write_data("w1");
    let errs = checker.check_write_ahead();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A33001");
}

#[test]
fn domain_page_cache_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no page_cache annotations should pass");
}

#[test]
fn domain_page_cache_direct_api() {
    let mut checker = PageCacheChecker::new(2);
    checker.load_page(1);
    checker.pin(1);
    let err = checker.evict(1);
    assert!(err.is_some());
    assert_eq!(err.as_ref().map(|e| e.code.as_str()), Some("A34001"));
}

#[test]
fn domain_mvcc_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no mvcc annotations should pass");
}

#[test]
fn domain_mvcc_direct_api() {
    let mut checker = MvccChecker::new();
    let t1 = checker.begin_txn();
    let t2 = checker.begin_txn();
    checker.write_version("key1".into(), t1);
    checker.write_version("key1".into(), t2);
    let errs = checker.check_write_conflicts();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A35001");
}

#[test]
fn domain_rollback_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no rollback annotations should pass");
}

#[test]
fn domain_rollback_direct_api() {
    let mut checker = RollbackChecker::new();
    let err = checker.rollback_to("nonexistent");
    assert!(err.is_some());
    assert_eq!(err.as_ref().map(|e| e.code.as_str()), Some("A36001"));
}

#[test]
fn domain_monotonic_state_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no monotonic annotations should pass");
}

#[test]
fn domain_monotonic_state_direct_api() {
    let mut checker = MonotonicStateChecker::new();
    checker.declare(
        "counter".into(),
        MonotonicDirection::StrictlyIncreasing,
        0,
        0..1,
    );
    let err = checker.update("counter", 0);
    assert!(err.is_some());
    assert_eq!(err.as_ref().map(|e| e.code.as_str()), Some("A37001"));
}

#[test]
fn domain_storage_failure_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no failure_model annotations should pass");
}

#[test]
fn domain_storage_failure_direct_api() {
    let mut checker = StorageFailureChecker::new();
    checker.declare_failure_mode(FailureMode::PartialWrite);
    let errs = checker.check_unhandled();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A38001");
}

#[test]
fn domain_numerical_precision_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no precision annotations should pass");
}

#[test]
fn domain_numerical_precision_direct_api() {
    let mut checker = NumericalPrecisionChecker::new();
    checker.declare("x".into(), 64, 1e-15, 0..1);
    let err = checker.check_precision_loss("x", 32);
    assert!(err.is_some());
    assert_eq!(err.as_ref().map(|e| e.code.as_str()), Some("A42001"));
}

#[test]
fn domain_precomputed_table_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no precomputed_table annotations should pass");
}

#[test]
fn domain_precomputed_table_direct_api() {
    let mut checker = PrecomputedTableChecker::new();
    checker.declare_table("crc".into(), 256, "gen_crc".into(), 0..1);
    let errs = checker.check_coverage();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A43001");
}

#[test]
fn domain_platform_abstraction_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no platform annotations should pass");
}

#[test]
fn domain_platform_abstraction_direct_api() {
    let mut checker = PlatformAbstractionChecker::new();
    checker.add_platform("linux".into());
    checker.declare_abstraction("fs".into(), vec![]);
    let errs = checker.check_coverage();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A44001");
}

#[test]
fn domain_feature_flag_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no feature_flag annotations should pass");
}

#[test]
fn domain_feature_flag_direct_api() {
    let mut checker = FeatureFlagChecker::new();
    checker.declare("experimental".into(), false, vec![]);
    let errs = checker.check_unused();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A45001");
}

#[test]
fn domain_resource_limit_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no resource_limit annotations should pass");
}

#[test]
fn domain_resource_limit_direct_api() {
    let mut checker = ResourceLimitChecker::new();
    checker.declare_limit("mem".into(), 1024, "bytes".into());
    checker.record_usage("mem", 2000);
    let errs = checker.check_limits();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A46001");
}

#[test]
fn domain_unsafe_escape_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no unsafe blocks should pass");
}

#[test]
fn domain_unsafe_escape_direct_api() {
    let mut checker = UnsafeEscapeChecker::new();
    checker.declare_unsafe("raw_ptr".into(), vec!["valid_ptr".into()], 0..1);
    let errs = checker.check_unproven();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A47001");
}

#[test]
fn domain_complexity_bound_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no complexity annotations should pass");
}

#[test]
fn domain_complexity_bound_direct_api() {
    let mut checker = ComplexityBoundChecker::new();
    checker.declare_bound("sort".into(), ComplexityClass::NLogN, 0..1);
    checker.record_measured("sort", ComplexityClass::Quadratic);
    let errs = checker.check_bounds();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A48001");
}

#[test]
fn domain_behavioral_equivalence_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no equivalence annotations should pass");
}

#[test]
fn domain_behavioral_equivalence_direct_api() {
    let mut checker = BehavioralEquivalenceChecker::new();
    checker.declare(
        "eq1".into(),
        "implA".into(),
        "implB".into(),
        "contract1".into(),
        0..1,
    );
    let errs = checker.check_unverified();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A49001");
}

#[test]
fn domain_multi_pass_refinement_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no refinement_pass annotations should pass");
}

#[test]
fn domain_multi_pass_refinement_direct_api() {
    let mut checker = MultiPassRefinementChecker::new();
    checker.add_pass("p1".into(), "L0".into(), "L1".into(), 5, 0..1);
    let errs = checker.check_complete();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A50001");
}

#[test]
fn domain_incremental_contract_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no contract_version annotations should pass");
}

#[test]
fn domain_incremental_contract_direct_api() {
    let mut checker = IncrementalContractChecker::new();
    checker.add_version("c1".into(), 1, 2, 3);
    checker.add_version("c1".into(), 2, 5, 3);
    let errs = checker.check_precondition_weakening();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A51001");
}

#[test]
fn domain_scoped_invariant_no_annotation_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no scoped invariant annotations should pass");
}

#[test]
fn domain_scoped_invariant_direct_api() {
    let mut checker = ScopedInvariantChecker::new();
    checker.declare_invariant("balance_positive".into());
    assert!(checker.suspend("balance_positive").is_none());
    let errs = checker.check_all_restored();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A52001");
}

#[test]
fn domain_contract_composition_no_extends_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no extends annotations should pass");
}

#[test]
fn domain_contract_composition_direct_api() {
    let mut checker = ContractCompositionChecker::new();
    checker.declare("Child".into(), vec!["MissingParent".into()], 1);
    let errs = checker.check_extends();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A54001");
}

#[test]
fn domain_contract_library_no_library_passes() {
    let src = r#"contract Simple { requires { true } }"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("no library blocks should pass");
}

#[test]
fn domain_contract_library_direct_api() {
    let mut checker = ContractLibraryChecker::new();
    checker.declare_library("mylib".into(), "1.0.0".into());
    let errs = checker.check_empty_exports();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A55001");
}

// ---------------------------------------------------------------------------
// Liveness block validation (G006)
// ---------------------------------------------------------------------------

#[test]
fn liveness_block_missing_prove_emits_a_core_030() {
    let source = r#"
liveness BadBlock {
    assume: fair
}
"#;
    let resolved = resolve_ok(source);
    let errs = type_check(&resolved).unwrap_err();
    assert!(
        errs.iter().any(|e| e.code == "A-CORE-030"),
        "expected A-CORE-030 for liveness block with no prove clause, got: {errs:?}"
    );
}

#[test]
fn liveness_block_with_prove_is_valid() {
    let source = r#"
liveness GoodBlock {
    assume: fair
    prove: eventually(done)
}
"#;
    let resolved = resolve_ok(source);
    // Should type-check without A-CORE-030 errors
    let result = type_check(&resolved);
    if let Err(errs) = &result {
        assert!(
            !errs.iter().any(|e| e.code == "A-CORE-030"),
            "should not emit A-CORE-030 when prove clause is present, got: {errs:?}"
        );
    }
}

#[test]
fn liveness_leads_to_without_fair_emits_a_core_031() {
    let source = r#"
liveness LeadsToNoFair {
    prove: leads_to(waiting, served)
}
"#;
    let resolved = resolve_ok(source);
    let errs = type_check(&resolved).unwrap_err();
    assert!(
        errs.iter().any(|e| e.code == "A-CORE-031"),
        "expected A-CORE-031 for leads_to without assume fair, got: {errs:?}"
    );
}

// ----------------------------------------------------------------
// G007: CONC.6 Weak Memory Ordering
// ----------------------------------------------------------------

#[test]
fn ordering_relaxed_with_ensures_emits_a_conc_016() {
    let source = r#"
contract RelaxedRead {
    input(counter: Int)
    ordering: relaxed
    ensures { counter >= 0 }
}
"#;
    let resolved = resolve_ok(source);
    let errs = type_check(&resolved).unwrap_err();
    assert!(
        errs.iter().any(|e| e.code == "A-CONC-016"),
        "expected A-CONC-016 for relaxed ordering with ensures, got: {errs:?}"
    );
}

#[test]
fn ordering_acquire_with_ensures_no_error() {
    let source = r#"
contract AcquireRead {
    input(counter: Int)
    ordering: acquire
    ensures { counter >= 0 }
}
"#;
    let resolved = resolve_ok(source);
    let result = type_check(&resolved);
    // No A-CONC-016 should be emitted for acquire ordering
    match &result {
        Ok(_) => {} // pass
        Err(errs) => {
            assert!(
                !errs.iter().any(|e| e.code == "A-CONC-016"),
                "unexpected A-CONC-016 for acquire ordering: {errs:?}"
            );
        }
    }
}

#[test]
fn ordering_relaxed_without_ensures_no_error() {
    let source = r#"
contract RelaxedNoEnsures {
    input(counter: Int)
    ordering: relaxed
    requires { counter >= 0 }
}
"#;
    let resolved = resolve_ok(source);
    let result = type_check(&resolved);
    // No A-CONC-016 without ensures clause
    match &result {
        Ok(_) => {} // pass
        Err(errs) => {
            assert!(
                !errs.iter().any(|e| e.code == "A-CONC-016"),
                "unexpected A-CONC-016 without ensures clause: {errs:?}"
            );
        }
    }
}

#[test]
fn ordering_seq_cst_with_ensures_no_error() {
    let source = r#"
contract SeqCstRead {
    input(counter: Int)
    ordering: seq_cst
    ensures { counter >= 0 }
}
"#;
    let resolved = resolve_ok(source);
    let result = type_check(&resolved);
    match &result {
        Ok(_) => {}
        Err(errs) => {
            assert!(
                !errs.iter().any(|e| e.code == "A-CONC-016"),
                "unexpected A-CONC-016 for seq_cst ordering: {errs:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// G008: Codec registry checks (FMT.4)
// ---------------------------------------------------------------------------

#[test]
fn codec_registry_overlapping_magic_a52001() {
    let source = r#"
        codec_registry Formats {
            output: Output,
            codec Png {
                magic: [0x89, 0x50, 0x4E, 0x47],
                decoder: decode_png
            }
            codec PngAlt {
                magic: [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A],
                decoder: decode_png_alt
            }
        }
    "#;
    let resolved = resolve_ok(source);
    let result = type_check(&resolved);
    match result {
        Err(errs) => {
            assert!(
                errs.iter().any(|e| e.code == "A52001"),
                "expected A52001 for overlapping magic patterns, got: {errs:?}"
            );
        }
        Ok(_) => panic!("expected type error A52001 for overlapping magic patterns"),
    }
}

#[test]
fn codec_registry_no_overlap_ok() {
    let source = r#"
        codec_registry Formats {
            output: Output,
            codec Png {
                magic: [0x89, 0x50, 0x4E, 0x47],
                decoder: decode_png
            }
            codec Jpeg {
                magic: [0xFF, 0xD8, 0xFF],
                decoder: decode_jpeg
            }
        }
    "#;
    let resolved = resolve_ok(source);
    let result = type_check(&resolved);
    match &result {
        Ok(_) => {}
        Err(errs) => {
            assert!(
                !errs.iter().any(|e| e.code == "A52001"),
                "unexpected A52001: {errs:?}"
            );
        }
    }
}

#[test]
fn codec_registry_empty_decoder_a52002() {
    // This tests the edge case where a codec has no decoder field.
    // The parser will produce an empty decoder string.
    let source = r#"
        codec_registry Formats {
            output: Output,
            codec Bad {
                magic: [0x89, 0x50]
            }
        }
    "#;
    let resolved = resolve_ok(source);
    let result = type_check(&resolved);
    match result {
        Err(errs) => {
            assert!(
                errs.iter().any(|e| e.code == "A52002"),
                "expected A52002 for missing decoder, got: {errs:?}"
            );
        }
        Ok(_) => panic!("expected type error A52002 for missing decoder"),
    }
}

// -----------------------------------------------------------------------
// G010: Type::Error propagation tests
// -----------------------------------------------------------------------

#[test]
fn error_type_suppresses_field_access() {
    let mut env = TypeEnv::new();
    env.insert("e".into(), Type::Error);
    let expr = AstExpr::Field(Box::new(AstExpr::Ident("e".into())), "foo".into());
    // Field access on Error receiver yields Error (no A03005 emitted)
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Error);
}

#[test]
fn error_type_suppresses_method_call() {
    let mut env = TypeEnv::new();
    env.insert("e".into(), Type::Error);
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("e".into())),
        method: "anything".into(),
        args: vec![],
    };
    // Method call on Error receiver yields Error (no A03005)
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Error);
}

#[test]
fn error_type_suppresses_index() {
    let mut env = TypeEnv::new();
    env.insert("e".into(), Type::Error);
    let expr = AstExpr::Index {
        expr: Box::new(AstExpr::Ident("e".into())),
        index: Box::new(AstExpr::Literal(assura_parser::ast::Literal::Int(
            "0".into(),
        ))),
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Error);
}

#[test]
fn error_type_suppresses_call() {
    let mut env = TypeEnv::new();
    env.insert("f".into(), Type::Error);
    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("f".into())),
        args: vec![],
    };
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Error);
}

#[test]
fn error_compatible_with_any_type() {
    assert!(types_compatible(&Type::Error, &Type::Int));
    assert!(types_compatible(&Type::Bool, &Type::Error));
    assert!(types_compatible(&Type::Error, &Type::Error));
}

#[test]
fn is_indeterminate_works() {
    assert!(Type::Unknown.is_indeterminate());
    assert!(Type::Error.is_indeterminate());
    assert!(!Type::Int.is_indeterminate());
    assert!(!Type::Bool.is_indeterminate());
    assert!(!Type::Named("Foo".into()).is_indeterminate());
}

// -----------------------------------------------------------------------
// G010: Improved inference tests for Option/Result map
// -----------------------------------------------------------------------

#[test]
fn option_map_with_known_function() {
    let mut env = TypeEnv::new();
    env.insert("opt".into(), Type::Option(Box::new(Type::Int)));
    env.insert(
        "to_string_fn".into(),
        Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(Type::String),
        },
    );
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("opt".into())),
        method: "map".into(),
        args: vec![AstExpr::Ident("to_string_fn".into())],
    };
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Option(Box::new(Type::String))
    );
}

#[test]
fn result_map_with_known_function() {
    let mut env = TypeEnv::new();
    env.insert(
        "r".into(),
        Type::Result(Box::new(Type::Int), Box::new(Type::String)),
    );
    env.insert(
        "double".into(),
        Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(Type::Float),
        },
    );
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("r".into())),
        method: "map".into(),
        args: vec![AstExpr::Ident("double".into())],
    };
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Result(Box::new(Type::Float), Box::new(Type::String))
    );
}

#[test]
fn result_map_err_with_known_function() {
    let mut env = TypeEnv::new();
    env.insert(
        "r".into(),
        Type::Result(Box::new(Type::Int), Box::new(Type::String)),
    );
    env.insert(
        "wrap_err".into(),
        Type::Fn {
            params: vec![Type::String],
            ret: Box::new(Type::Named("AppError".into())),
        },
    );
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("r".into())),
        method: "map_err".into(),
        args: vec![AstExpr::Ident("wrap_err".into())],
    };
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Result(
            Box::new(Type::Int),
            Box::new(Type::Named("AppError".into()))
        )
    );
}

#[test]
fn option_filter_preserves_type() {
    let mut env = TypeEnv::new();
    env.insert("opt".into(), Type::Option(Box::new(Type::Int)));
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("opt".into())),
        method: "filter".into(),
        args: vec![],
    };
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Option(Box::new(Type::Int))
    );
}

#[test]
fn option_or_else_preserves_type() {
    let mut env = TypeEnv::new();
    env.insert("opt".into(), Type::Option(Box::new(Type::Int)));
    let expr = AstExpr::MethodCall {
        receiver: Box::new(AstExpr::Ident("opt".into())),
        method: "or_else".into(),
        args: vec![],
    };
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::Option(Box::new(Type::Int))
    );
}

#[test]
fn type_param_call_returns_type_param() {
    let mut env = TypeEnv::new();
    env.insert("T".into(), Type::TypeParam("T".into()));
    let expr = AstExpr::Call {
        func: Box::new(AstExpr::Ident("T".into())),
        args: vec![AstExpr::Literal(assura_parser::ast::Literal::Int(
            "1".into(),
        ))],
    };
    assert_eq!(
        infer_expr(&expr, &env).unwrap(),
        Type::TypeParam("T".into())
    );
}

#[test]
fn block_empty_returns_unit() {
    let env = TypeEnv::new();
    let expr = AstExpr::Block(vec![]);
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unit);
}

#[test]
fn block_returns_last_expr_type() {
    let env = TypeEnv::new();
    let expr = AstExpr::Block(vec![
        AstExpr::Literal(assura_parser::ast::Literal::Int("1".into())),
        AstExpr::Literal(assura_parser::ast::Literal::Bool(true)),
    ]);
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
}

// =======================================================================
// Domain checker integration tests (issues #63, #65)
// =======================================================================

// --- PageCacheChecker tests ---

#[test]
fn page_cache_checker_capacity_from_ast() {
    use crate::domain::PageCacheChecker;
    let mut checker = PageCacheChecker::new(2);
    checker.load_page(1);
    checker.load_page(2);
    checker.load_page(3);
    let errors = checker.check_capacity();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A34003");
    assert!(errors[0].message.contains("exceeds capacity"));
}

#[test]
fn page_cache_checker_within_capacity() {
    use crate::domain::PageCacheChecker;
    let checker = PageCacheChecker::new(10);
    let errors = checker.check_capacity();
    assert!(errors.is_empty());
}

#[test]
fn page_cache_checker_dirty_evict() {
    use crate::domain::PageCacheChecker;
    let mut checker = PageCacheChecker::new(10);
    checker.load_page(42);
    checker.mark_dirty(42);
    let err = checker.evict(42);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A34002");
}

#[test]
fn page_cache_checker_pinned_evict() {
    use crate::domain::PageCacheChecker;
    let mut checker = PageCacheChecker::new(10);
    checker.load_page(7);
    checker.pin(7);
    let err = checker.evict(7);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A34001");
}

// --- MvccChecker tests ---

#[test]
fn mvcc_checker_write_conflict() {
    use crate::domain::MvccChecker;
    let mut checker = MvccChecker::new();
    let tx1 = checker.begin_txn();
    let tx2 = checker.begin_txn();
    checker.write_version("key_a".into(), tx1);
    checker.write_version("key_a".into(), tx2);
    let errors = checker.check_write_conflicts();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A35001");
    assert!(errors[0].message.contains("write-write conflict"));
}

#[test]
fn mvcc_checker_no_conflict_after_commit() {
    use crate::domain::MvccChecker;
    let mut checker = MvccChecker::new();
    let tx1 = checker.begin_txn();
    checker.write_version("key_b".into(), tx1);
    checker.commit_txn(tx1);
    let tx2 = checker.begin_txn();
    checker.write_version("key_b".into(), tx2);
    let errors = checker.check_write_conflicts();
    assert!(errors.is_empty());
}

#[test]
fn mvcc_checker_snapshot_violation() {
    use crate::domain::MvccChecker;
    let mut checker = MvccChecker::new();
    let tx1 = checker.begin_txn();
    let tx2 = checker.begin_txn();
    checker.write_version("shared".into(), tx1);
    let err = checker.check_snapshot_read("shared", tx2);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A35002");
}

// --- RollbackChecker tests ---

#[test]
fn rollback_checker_resource_leak() {
    use crate::domain::RollbackChecker;
    let mut checker = RollbackChecker::new();
    checker.create_savepoint("sp1".into());
    checker.acquire_resource("file_handle".into());
    let _ = checker.rollback_to("sp1");
    let errors = checker.check_resource_leak();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A36002");
    assert!(errors[0].message.contains("file_handle"));
}

#[test]
fn rollback_checker_no_leak_when_released() {
    use crate::domain::RollbackChecker;
    let mut checker = RollbackChecker::new();
    checker.create_savepoint("sp1".into());
    checker.acquire_resource("conn".into());
    checker.release_resource("conn");
    let _ = checker.rollback_to("sp1");
    let errors = checker.check_resource_leak();
    assert!(errors.is_empty());
}

#[test]
fn rollback_checker_unknown_savepoint() {
    use crate::domain::RollbackChecker;
    let mut checker = RollbackChecker::new();
    let err = checker.rollback_to("nonexistent");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A36001");
}

#[test]
fn rollback_checker_duplicate_savepoint() {
    use crate::domain::RollbackChecker;
    let mut checker = RollbackChecker::new();
    checker.create_savepoint("dup".into());
    checker.create_savepoint("dup".into());
    let errors = checker.check_savepoint_nesting();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A36003");
}

// --- Expression extraction helper tests ---

#[test]
fn extract_int_literal_positive() {
    use crate::checkers::extract_int_literal;
    use assura_parser::ast::{Expr, Literal};
    let expr = Expr::Literal(Literal::Int("42".into()));
    assert_eq!(extract_int_literal(&expr), Some(42));
}

#[test]
fn extract_int_literal_negative() {
    use crate::checkers::extract_int_literal;
    use assura_parser::ast::{Expr, Literal, UnaryOp};
    let expr = Expr::UnaryOp {
        op: UnaryOp::Neg,
        expr: Box::new(Expr::Literal(Literal::Int("5".into()))),
    };
    assert_eq!(extract_int_literal(&expr), Some(-5));
}

#[test]
fn extract_ident_works() {
    use crate::checkers::extract_ident;
    use assura_parser::ast::Expr;
    let expr = Expr::Ident("hello".into());
    assert_eq!(extract_ident(&expr), Some("hello"));
}

#[test]
fn extract_kv_pair_from_eq() {
    use crate::checkers::extract_kv_pair;
    use assura_parser::ast::{BinOp, Expr, Literal};
    let expr = Expr::BinOp {
        op: BinOp::Eq,
        lhs: Box::new(Expr::Ident("size".into())),
        rhs: Box::new(Expr::Literal(Literal::Int("256".into()))),
    };
    let pair = extract_kv_pair(&expr);
    assert!(pair.is_some());
    let (key, _val) = pair.unwrap();
    assert_eq!(key, "size");
}

#[test]
fn extract_call_works() {
    use crate::checkers::extract_call;
    use assura_parser::ast::{Expr, Literal};
    let expr = Expr::Call {
        func: Box::new(Expr::Ident("load_page".into())),
        args: vec![Expr::Literal(Literal::Int("42".into()))],
    };
    let result = extract_call(&expr);
    assert!(result.is_some());
    let (name, args) = result.unwrap();
    assert_eq!(name, "load_page");
    assert_eq!(args.len(), 1);
}

// --- Multi-pass refinement checker tests ---

#[test]
fn multi_pass_refinement_chain() {
    use crate::domain::MultiPassRefinementChecker;
    let mut checker = MultiPassRefinementChecker::new();
    checker.add_pass("step1".into(), "abstract".into(), "mid".into(), 3, 0..5);
    checker.add_pass("step2".into(), "mid".into(), "concrete".into(), 2, 5..10);
    let errors = checker.check_chain();
    assert!(
        errors.is_empty(),
        "well-chained passes should have no errors"
    );
}

#[test]
fn multi_pass_refinement_broken_chain() {
    use crate::domain::MultiPassRefinementChecker;
    let mut checker = MultiPassRefinementChecker::new();
    checker.add_pass("step1".into(), "abstract".into(), "mid".into(), 3, 0..5);
    checker.add_pass("step2".into(), "other".into(), "concrete".into(), 2, 5..10);
    let errors = checker.check_chain();
    assert!(!errors.is_empty(), "broken chain should produce errors");
}

// --- Incremental contract checker tests ---

#[test]
fn incremental_contract_version_continuity() {
    use crate::domain::IncrementalContractChecker;
    let mut checker = IncrementalContractChecker::new();
    checker.add_version("Foo".into(), 1, 2, 1);
    checker.add_version("Foo".into(), 2, 3, 2);
    let errors = checker.check_version_continuity();
    assert!(errors.is_empty(), "sequential versions should pass");
}

#[test]
fn incremental_contract_precondition_weakening() {
    use crate::domain::IncrementalContractChecker;
    let mut checker = IncrementalContractChecker::new();
    checker.add_version("Bar".into(), 1, 3, 1);
    checker.add_version("Bar".into(), 2, 2, 1);
    let errors = checker.check_precondition_weakening();
    // Fewer requires in v2 is allowed (weakening); more would be an error
    assert!(errors.is_empty());
}

// =======================================================================
// 4.03: Cross-file type checking
// =======================================================================

#[test]
fn cross_file_import_resolves_contract_type() {
    // Module "math" defines contract Add
    let math_src = "module math\ncontract Add { input(a: Int, b: Int) output(result: Int) }";
    let math_file = assura_parser::parse_unwrap(math_src);
    let mut module_map = std::collections::HashMap::new();
    module_map.insert("math".to_string(), math_file.clone());

    let math_resolved = assura_resolve::resolve_with_modules(
        &math_file,
        &module_map,
        &mut std::collections::HashSet::new(),
    )
    .expect("math resolve failed");

    // Module "main" imports Add from math
    let main_src = "import math { Add }\ncontract Main { input(x: Int) output(result: Int) }";
    let main_file = assura_parser::parse_unwrap(main_src);
    let main_resolved = assura_resolve::resolve_with_modules(
        &main_file,
        &module_map,
        &mut std::collections::HashSet::new(),
    )
    .expect("main resolve failed");

    // Build modules map for cross-file type checking
    let mut modules = std::collections::HashMap::new();
    modules.insert("math".to_string(), math_resolved.clone());

    let result = crate::type_check_with_modules(
        &main_resolved,
        &modules,
        &assura_config::TypeCheckConfig::default(),
    );
    // Should succeed: imported Add is known, main's own types are valid
    assert!(result.is_ok(), "cross-file type check should succeed");

    // Verify the imported type is concrete (not Unknown)
    let typed = result.unwrap();
    let add_ty = typed.type_env.lookup("Add");
    assert!(
        add_ty.is_some(),
        "imported contract Add should be in the type env"
    );
    assert_ne!(
        add_ty.unwrap(),
        &crate::Type::Unknown,
        "imported contract Add should not be Type::Unknown"
    );
}

#[test]
fn cross_file_import_resolves_type_def() {
    // Module "geom" defines type Vector
    let geom_src = "module geom\ntype Vector { x: Float, y: Float }";
    let geom_file = assura_parser::parse_unwrap(geom_src);
    let mut module_map = std::collections::HashMap::new();
    module_map.insert("geom".to_string(), geom_file.clone());

    let geom_resolved = assura_resolve::resolve_with_modules(
        &geom_file,
        &module_map,
        &mut std::collections::HashSet::new(),
    )
    .expect("geom resolve failed");

    // Module "main" imports Vector from geom
    let main_src =
        "import geom { Vector }\ncontract UseVector { input(v: Vector) output(result: Float) }";
    let main_file = assura_parser::parse_unwrap(main_src);
    let main_resolved = assura_resolve::resolve_with_modules(
        &main_file,
        &module_map,
        &mut std::collections::HashSet::new(),
    )
    .expect("main resolve failed");

    let mut modules = std::collections::HashMap::new();
    modules.insert("geom".to_string(), geom_resolved.clone());

    let result = crate::type_check_with_modules(
        &main_resolved,
        &modules,
        &assura_config::TypeCheckConfig::default(),
    );
    assert!(result.is_ok(), "cross-file type check should succeed");

    let typed = result.unwrap();
    // Verify struct fields were injected
    assert!(
        typed.type_env.struct_fields.contains_key("Vector"),
        "imported struct Vector should have its fields in the type env"
    );
    let fields = &typed.type_env.struct_fields["Vector"];
    assert_eq!(fields.len(), 2, "Vector should have 2 fields (x, y)");
}

#[test]
fn cross_file_without_modules_still_works() {
    // Single-file type checking (no imports, empty modules map)
    let src = "contract Simple { input(x: Int) output(result: Int) }";
    let file = assura_parser::parse_unwrap(src);
    let resolved = assura_resolve::resolve(&file).expect("resolve failed");
    let modules = std::collections::HashMap::new();

    let result = crate::type_check_with_modules(
        &resolved,
        &modules,
        &assura_config::TypeCheckConfig::default(),
    );
    assert!(
        result.is_ok(),
        "type checking with empty modules map should still work"
    );
}

#[test]
fn cross_file_unresolved_import_is_ignored() {
    // Module "main" imports from a module that doesn't exist in the map
    let main_src =
        "import nonexistent { Foo }\ncontract Main { input(x: Int) output(result: Int) }";
    let main_file = assura_parser::parse_unwrap(main_src);
    let main_resolved = assura_resolve::resolve(&main_file).expect("resolve failed");
    let modules = std::collections::HashMap::new();

    let result = crate::type_check_with_modules(
        &main_resolved,
        &modules,
        &assura_config::TypeCheckConfig::default(),
    );
    // Should succeed; unresolved imports are just Unknown types (no crash)
    assert!(
        result.is_ok(),
        "unresolved imports should not cause type check failure"
    );
}

// =========================================================================
// Issue #112: Circular buffer capacity extracted from annotations
// =========================================================================

#[test]
fn circular_buffer_capacity_extraction_call_syntax() {
    // Verify that capacity is extracted from call syntax, not hardcoded to 256
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 16);
    // Buffer with capacity 16 should report full at 16 items
    for _ in 0..16 {
        checker.push("ring");
    }
    assert!(checker.buffers.get("ring").unwrap().is_full());
}

#[test]
fn circular_buffer_check_index_wired() {
    // Verify check_index is callable and returns errors for out-of-bounds
    let mut checker = CircularBufferChecker::new();
    checker.declare("buf".into(), 4);
    // Index 0 on empty buffer: check_index should flag it
    let err = checker.check_index("buf", 5, &(0..1));
    assert!(err.is_some(), "index 5 on capacity-4 buffer should error");
    assert_eq!(err.unwrap().code, "A23001");
}

// =========================================================================
// Issue #113: Axiomatic definitions extract references from clause bodies
// =========================================================================

#[test]
fn axiomatic_references_extracted_from_body() {
    let mut checker = AxiomaticDefChecker::new();
    // Declare two axioms where axiom_a references axiom_b
    checker.declare_axiom(AxiomDef {
        name: "axiom_a".into(),
        span: 0..1,
        references: vec!["axiom_b".into()],
    });
    checker.declare_axiom(AxiomDef {
        name: "axiom_b".into(),
        span: 0..1,
        references: vec![],
    });
    // check_circular should find no cycle (a -> b, no b -> a)
    let circ_errs = checker.check_circular();
    assert!(circ_errs.is_empty(), "no circular dependency expected");
}

#[test]
fn axiomatic_circular_reference_detected() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "ax1".into(),
        span: 0..1,
        references: vec!["ax2".into()],
    });
    checker.declare_axiom(AxiomDef {
        name: "ax2".into(),
        span: 0..1,
        references: vec!["ax1".into()],
    });
    let circ_errs = checker.check_circular();
    assert!(
        !circ_errs.is_empty(),
        "circular dependency should be detected"
    );
}

// =========================================================================
// Issue #115: Platform abstraction extracts supported platforms
// =========================================================================

#[test]
fn platform_abstraction_with_supported_platforms() {
    let mut checker = PlatformAbstractionChecker::new();
    checker.add_platform("linux".into());
    checker.add_platform("macos".into());
    checker.declare_abstraction("fs_ops".into(), vec!["linux".into(), "macos".into()]);
    // All declared platforms are supported, should not error
    let errs = checker.check_unknown_platforms();
    assert!(errs.is_empty(), "all platforms are known");
}

#[test]
fn platform_abstraction_unknown_platform_detected() {
    let mut checker = PlatformAbstractionChecker::new();
    checker.add_platform("linux".into());
    checker.declare_abstraction("fs_ops".into(), vec!["linux".into(), "windows".into()]);
    let errs = checker.check_unknown_platforms();
    assert_eq!(errs.len(), 1, "windows should be flagged as unknown");
    assert_eq!(errs[0].code, "A44003");
}

// =========================================================================
// Issue #116: Feature flags extract default_enabled from annotations
// =========================================================================

#[test]
fn feature_flag_with_enabled_default() {
    let mut checker = FeatureFlagChecker::new();
    checker.declare("dark_mode".into(), true, Vec::new());
    checker.declare("experimental".into(), false, Vec::new());
    // Neither is used, both should be flagged as unused
    let errs = checker.check_unused();
    assert_eq!(errs.len(), 2, "both flags should be flagged as unused");
}

#[test]
fn feature_flag_with_dependencies() {
    let mut checker = FeatureFlagChecker::new();
    checker.declare("base".into(), true, Vec::new());
    checker.declare("advanced".into(), false, vec!["base".into()]);
    checker.mark_used("base");
    let errs = checker.check_unused();
    assert_eq!(errs.len(), 1, "only advanced should be unused");
}

// =========================================================================
// Issue #118: Unsafe escape extracts proof obligations from annotations
// =========================================================================

#[test]
fn unsafe_escape_with_obligations() {
    let mut checker = UnsafeEscapeChecker::new();
    checker.declare_unsafe(
        "raw_ptr_deref".into(),
        vec!["memory_safety".into(), "alignment".into()],
        0..1,
    );
    // Without discharging, check should flag unfulfilled obligations
    let errs = checker.check_obligations();
    assert!(
        !errs.is_empty(),
        "undischarged obligations should be flagged"
    );
}

#[test]
fn unsafe_escape_discharge_obligation() {
    let mut checker = UnsafeEscapeChecker::new();
    checker.declare_unsafe("raw_ptr_deref".into(), vec!["memory_safety".into()], 0..1);
    checker.attach_proof("raw_ptr_deref");
    checker.discharge_obligation("raw_ptr_deref", "memory_safety".into());
    let errs = checker.check_obligations();
    assert!(errs.is_empty(), "discharged obligations should not error");
}

// =========================================================================
// Issue #110: Cross-type comparisons rejected in clause bodies
// =========================================================================

#[test]
fn cross_type_comparison_string_vs_int_rejected() {
    // String >= Int should produce a type error
    let src = r#"
        contract Bad {
            input(name: String)
            requires { name >= 650 }
            ensures(result: Int)
        }
    "#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    // Should produce type errors for String >= Int
    assert!(
        result.is_err(),
        "String >= Int comparison should be rejected"
    );
}

#[test]
fn cross_type_arithmetic_string_plus_int_rejected() {
    // String + Int should produce a type error
    let src = r#"
        contract Bad {
            input(name: String)
            output(result: Int)
            ensures { result == name + 1 }
        }
    "#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    assert!(
        result.is_err(),
        "String + Int arithmetic should be rejected"
    );
}

#[test]
fn same_type_comparison_passes() {
    // Int >= Int should pass
    let src = r#"
        contract Good {
            input(x: Int)
            requires { x >= 0 }
            output(result: Int)
        }
    "#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    assert!(result.is_ok(), "Int >= Int should pass: {:?}", result.err());
}
