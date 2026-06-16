use super::*;
// T055 MEM.2: FixedWidthChecker tests
// -----------------------------------------------------------------------

#[test]
fn fixed_width_range_u8() {
    let r = FixedWidthChecker::range_for_type(&Type::U8).unwrap();
    assert_eq!(r, (0, 255));
}

#[test]
fn fixed_width_range_i8() {
    let r = FixedWidthChecker::range_for_type(&Type::I8).unwrap();
    assert_eq!(r, (-128, 127));
}

#[test]
fn fixed_width_range_u16() {
    let r = FixedWidthChecker::range_for_type(&Type::U16).unwrap();
    assert_eq!(r, (0, 65535));
}

#[test]
fn fixed_width_range_i16() {
    let r = FixedWidthChecker::range_for_type(&Type::I16).unwrap();
    assert_eq!(r, (-32768, 32767));
}

#[test]
fn fixed_width_range_u32() {
    let r = FixedWidthChecker::range_for_type(&Type::U32).unwrap();
    assert_eq!(r, (0, u32::MAX as i128));
}

#[test]
fn fixed_width_range_i32() {
    let r = FixedWidthChecker::range_for_type(&Type::I32).unwrap();
    assert_eq!(r, (i32::MIN as i128, i32::MAX as i128));
}

#[test]
fn fixed_width_range_u64() {
    let r = FixedWidthChecker::range_for_type(&Type::U64).unwrap();
    assert_eq!(r, (0, u64::MAX as i128));
}

#[test]
fn fixed_width_range_i64() {
    let r = FixedWidthChecker::range_for_type(&Type::I64).unwrap();
    assert_eq!(r, (i64::MIN as i128, i64::MAX as i128));
}

#[test]
fn fixed_width_range_non_fixed() {
    // Non-fixed-width types return None
    assert!(FixedWidthChecker::range_for_type(&Type::Int).is_none());
    assert!(FixedWidthChecker::range_for_type(&Type::Bool).is_none());
    assert!(FixedWidthChecker::range_for_type(&Type::Float).is_none());
}

#[test]
fn fixed_width_u8_overflow_add() {
    // U8 + U8: 255 + 255 = 510 > 255 -> overflow
    let checker = FixedWidthChecker::new();
    let err = checker.check_arithmetic_overflow(&AstBinOp::Add, &Type::U8, &Type::U8, &(0..1));
    assert!(err.is_some(), "U8 + U8 should detect potential overflow");
    let e = err.unwrap();
    assert_eq!(e.code, "A10101");
    assert!(e.message.contains("checked_add"));
}

#[test]
fn fixed_width_i8_overflow_add() {
    // I8 + I8: 127 + 127 = 254 > 127 -> overflow
    let checker = FixedWidthChecker::new();
    let err = checker.check_arithmetic_overflow(&AstBinOp::Add, &Type::I8, &Type::I8, &(0..1));
    assert!(err.is_some(), "I8 + I8 should detect potential overflow");
    assert_eq!(err.unwrap().code, "A10101");
}

#[test]
fn fixed_width_safe_arithmetic_no_error() {
    // This tests that overflow check only fires on arithmetic ops.
    // For comparison operators, no overflow check applies.
    let checker = FixedWidthChecker::new();
    let err = checker.check_arithmetic_overflow(&AstBinOp::Lt, &Type::U8, &Type::U8, &(0..1));
    assert!(err.is_none(), "comparison should not trigger overflow");
}

#[test]
fn fixed_width_mul_overflow() {
    // U8 * U8: 255 * 255 = 65025 > 255 -> overflow
    let checker = FixedWidthChecker::new();
    let err = checker.check_arithmetic_overflow(&AstBinOp::Mul, &Type::U8, &Type::U8, &(0..1));
    assert!(err.is_some(), "U8 * U8 should detect potential overflow");
    let e = err.unwrap();
    assert!(e.message.contains("checked_mul"));
}

#[test]
fn fixed_width_narrowing_cast_u32_to_u16() {
    // U32 -> U16: max 4294967295 > 65535 -> unsafe
    let err = FixedWidthChecker::check_cast_safety(&Type::U32, &Type::U16, &(0..1));
    assert!(err.is_some(), "U32 -> U16 should be unsafe narrowing");
    assert_eq!(err.unwrap().code, "A10102");
}

#[test]
fn fixed_width_widening_cast_u16_to_u32() {
    // U16 -> U32: always safe (widening)
    let err = FixedWidthChecker::check_cast_safety(&Type::U16, &Type::U32, &(0..1));
    assert!(err.is_none(), "U16 -> U32 should be safe widening cast");
}

#[test]
fn fixed_width_signed_unsigned_comparison() {
    // I32 == U32 -> signedness mismatch
    let err = FixedWidthChecker::check_signedness_mismatch(
        &AstBinOp::Eq,
        &Type::I32,
        &Type::U32,
        &(0..1),
    );
    assert!(err.is_some(), "I32 vs U32 comparison should warn");
    assert_eq!(err.unwrap().code, "A10103");
}

#[test]
fn fixed_width_same_signedness_ok() {
    // U32 == U32 -> no mismatch
    let err = FixedWidthChecker::check_signedness_mismatch(
        &AstBinOp::Lt,
        &Type::U32,
        &Type::U32,
        &(0..1),
    );
    assert!(err.is_none(), "same signedness should not warn");
}

#[test]
fn fixed_width_division_by_zero() {
    let rhs = AstExpr::Literal(AstLit::Int("0".into()));
    let err = FixedWidthChecker::check_division_by_zero(&AstBinOp::Div, &rhs, &Type::U32, &(0..1));
    assert!(err.is_some(), "division by literal 0 should be flagged");
    assert_eq!(err.unwrap().code, "A10104");
}

#[test]
fn fixed_width_division_nonzero_ok() {
    let rhs = AstExpr::Literal(AstLit::Int("5".into()));
    let err = FixedWidthChecker::check_division_by_zero(&AstBinOp::Div, &rhs, &Type::U32, &(0..1));
    assert!(err.is_none(), "division by non-zero should pass");
}

#[test]
fn fixed_width_suggest_checked_add() {
    assert_eq!(
        FixedWidthChecker::suggest_checked_alternative(&AstBinOp::Add),
        "checked_add"
    );
}

#[test]
fn fixed_width_suggest_checked_sub() {
    assert_eq!(
        FixedWidthChecker::suggest_checked_alternative(&AstBinOp::Sub),
        "checked_sub"
    );
}

#[test]
fn fixed_width_suggest_checked_mul() {
    assert_eq!(
        FixedWidthChecker::suggest_checked_alternative(&AstBinOp::Mul),
        "checked_mul"
    );
}

#[test]
fn fixed_width_cast_i32_to_u32() {
    // I32 -> U32: signed-to-unsigned, range [-2^31, 2^31-1] does not
    // fit in [0, 2^32-1] because of negative values -> unsafe
    let err = FixedWidthChecker::check_cast_safety(&Type::I32, &Type::U32, &(0..1));
    assert!(err.is_some(), "I32 -> U32 cast should be unsafe");
    assert_eq!(err.unwrap().code, "A10102");
}

#[test]
fn fixed_width_is_unsigned() {
    assert!(FixedWidthChecker::is_unsigned(&Type::U8));
    assert!(FixedWidthChecker::is_unsigned(&Type::U16));
    assert!(FixedWidthChecker::is_unsigned(&Type::U32));
    assert!(FixedWidthChecker::is_unsigned(&Type::U64));
    assert!(!FixedWidthChecker::is_unsigned(&Type::I8));
    assert!(!FixedWidthChecker::is_unsigned(&Type::Int));
}

#[test]
fn fixed_width_is_signed() {
    assert!(FixedWidthChecker::is_signed(&Type::I8));
    assert!(FixedWidthChecker::is_signed(&Type::I16));
    assert!(FixedWidthChecker::is_signed(&Type::I32));
    assert!(FixedWidthChecker::is_signed(&Type::I64));
    assert!(!FixedWidthChecker::is_signed(&Type::U8));
    assert!(!FixedWidthChecker::is_signed(&Type::Float));
}

#[test]
fn fixed_width_check_binop_combined() {
    // I8 + U8 -> both overflow (A10101) and signedness mismatch (A10103)
    let checker = FixedWidthChecker::new();
    let rhs_expr = AstExpr::Ident("y".into());
    let errors = checker.check_binop(&AstBinOp::Add, &Type::I8, &Type::U8, &rhs_expr, &(0..1));
    // Should have both an overflow error and a signedness mismatch
    let codes: Vec<&str> = errors.iter().map(|e| e.code.as_str()).collect();
    assert!(codes.contains(&"A10101"), "should flag overflow");
    // Signedness mismatch only fires for comparison ops, not arithmetic
    // (by design: check_signedness_mismatch only checks comparison ops)
}

#[test]
fn fixed_width_modulo_by_zero() {
    let rhs = AstExpr::Literal(AstLit::Int("0".into()));
    let err = FixedWidthChecker::check_division_by_zero(&AstBinOp::Mod, &rhs, &Type::I32, &(0..1));
    assert!(err.is_some(), "modulo by zero should be flagged");
    let e = err.unwrap();
    assert_eq!(e.code, "A10104");
    assert!(e.message.contains("modulo"));
}

#[test]
fn fixed_width_sub_overflow_unsigned() {
    // U8 - U8: 0 - 255 = -255 < 0 -> overflow (underflow)
    let checker = FixedWidthChecker::new();
    let err = checker.check_arithmetic_overflow(&AstBinOp::Sub, &Type::U8, &Type::U8, &(0..1));
    assert!(err.is_some(), "U8 - U8 should detect potential underflow");
    assert_eq!(err.unwrap().code, "A10101");
}

#[test]
fn fixed_width_declare_and_lookup() {
    let mut checker = FixedWidthChecker::new();
    checker.declare("counter".into(), Type::U32);
    assert_eq!(checker.get_type("counter"), Some(&Type::U32));
    assert_eq!(checker.get_type("unknown"), None);
}

#[test]
fn fixed_width_default_trait() {
    let checker = FixedWidthChecker::default();
    assert!(checker.get_type("x").is_none());
}

#[test]
fn fixed_width_safe_cast_same_type() {
    // U32 -> U32: always safe
    assert!(FixedWidthChecker::is_safe_cast(&Type::U32, &Type::U32));
}

#[test]
fn fixed_width_cast_non_fixed_width() {
    // Non-fixed-width types are outside scope -> treated as safe
    let err = FixedWidthChecker::check_cast_safety(&Type::Int, &Type::U32, &(0..1));
    assert!(err.is_none(), "non-fixed-width cast should be out of scope");
}

// =======================================================================
// T056: AllocatorChecker tests
// =======================================================================

#[test]
fn allocator_unpaired_alloc() {
    let mut checker = AllocatorChecker::new();
    checker.record_alloc("buf".into(), None, 0..4);
    let errors = checker.check_unpaired();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A22001");
}

#[test]
fn allocator_paired_ok() {
    let mut checker = AllocatorChecker::new();
    checker.record_alloc("buf".into(), None, 0..4);
    assert!(checker.record_free("buf", 10..14).is_none());
    let errors = checker.check_unpaired();
    assert!(errors.is_empty());
}

#[test]
fn allocator_double_free() {
    let mut checker = AllocatorChecker::new();
    checker.record_alloc("buf".into(), None, 0..4);
    assert!(checker.record_free("buf", 10..14).is_none());
    let err = checker.record_free("buf", 20..24);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A22002");
}

#[test]
fn allocator_arena_ok() {
    let mut checker = AllocatorChecker::new();
    checker.declare_arena("arena1".into());
    checker.record_alloc("obj".into(), Some("arena1".into()), 0..4);
    // Arena-managed allocations are not required to have explicit free
    let errors = checker.check_unpaired();
    assert!(errors.is_empty());
}

#[test]
fn allocator_arena_use_after_drop() {
    let mut checker = AllocatorChecker::new();
    checker.declare_arena("arena1".into());
    checker.record_alloc("obj".into(), Some("arena1".into()), 0..4);
    checker.drop_arena("arena1", 10..14);
    let err = checker.check_arena_use("obj", &(20..24));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A22004");
}

#[test]
fn allocator_arena_use_before_drop_ok() {
    let mut checker = AllocatorChecker::new();
    checker.declare_arena("arena1".into());
    checker.record_alloc("obj".into(), Some("arena1".into()), 0..4);
    let err = checker.check_arena_use("obj", &(5..8));
    assert!(err.is_none());
}

#[test]
fn allocator_default() {
    let checker = AllocatorChecker::default();
    assert!(checker.check_unpaired().is_empty());
}

// =======================================================================
// T057: CircularBufferChecker tests
// =======================================================================

#[test]
fn circ_buf_read_empty() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 8);
    let err = checker.check_read("ring", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A23003");
}

#[test]
fn circ_buf_read_nonempty() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 8);
    checker.push("ring");
    assert!(checker.check_read("ring", &(0..1)).is_none());
}

#[test]
fn circ_buf_index_out_of_bounds() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 4);
    let err = checker.check_index("ring", 5, &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A23001");
}

#[test]
fn circ_buf_index_ok() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 4);
    assert!(checker.check_index("ring", 3, &(0..1)).is_none());
}

#[test]
fn circ_buf_zero_capacity() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 0);
    let err = checker.check_physical_wrap("ring", 0, &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A23002");
}

#[test]
fn circ_buf_push_pop() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 2);
    checker.push("ring");
    checker.push("ring");
    // Full, push should not increase count
    checker.push("ring");
    let info = checker.buffers.get("ring").unwrap();
    assert_eq!(info.count, 2);
    assert!(info.is_full());
    checker.pop("ring");
    let info = checker.buffers.get("ring").unwrap();
    assert_eq!(info.count, 1);
}

#[test]
fn circ_buf_default() {
    let checker = CircularBufferChecker::default();
    assert!(checker.check_read("x", &(0..1)).is_none());
}

// =======================================================================
// T066: CallbackReentrancyChecker tests
// =======================================================================

#[test]
fn callback_reentrant_call() {
    let mut checker = CallbackReentrancyChecker::new();
    checker.mark_non_reentrant("handle_event".into(), 0..10);
    assert!(checker.enter_call("handle_event", &(0..1)).is_empty());
    // Re-entrant call
    let errors = checker.enter_call("handle_event", &(5..6));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A24001");
}

#[test]
fn callback_reentrant_allowed() {
    let mut checker = CallbackReentrancyChecker::new();
    // Not marked non-reentrant
    assert!(checker.enter_call("handle_event", &(0..1)).is_empty());
    assert!(checker.enter_call("handle_event", &(5..6)).is_empty());
}

#[test]
fn callback_max_depth() {
    let mut checker = CallbackReentrancyChecker::new().with_max_depth(2);
    assert!(checker.enter_call("a", &(0..1)).is_empty());
    assert!(checker.enter_call("b", &(0..1)).is_empty());
    let errors = checker.enter_call("c", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A24003");
}

#[test]
fn callback_register_in_context() {
    let mut checker = CallbackReentrancyChecker::new();
    checker.mark_non_reentrant("handler".into(), 0..10);
    assert!(checker.enter_call("handler", &(0..1)).is_empty());
    let err = checker.check_register_callback("handler", &(5..6));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A24002");
}

#[test]
fn callback_exit_resets() {
    let mut checker = CallbackReentrancyChecker::new();
    checker.mark_non_reentrant("f".into(), 0..10);
    assert!(checker.enter_call("f", &(0..1)).is_empty());
    checker.exit_call();
    // After exit, re-entry is allowed
    assert!(checker.enter_call("f", &(5..6)).is_empty());
}

#[test]
fn callback_depth_tracking() {
    let mut checker = CallbackReentrancyChecker::new();
    assert_eq!(checker.current_depth(), 0);
    checker.enter_call("a", &(0..1));
    assert_eq!(checker.current_depth(), 1);
    checker.enter_call("b", &(0..1));
    assert_eq!(checker.current_depth(), 2);
    checker.exit_call();
    assert_eq!(checker.current_depth(), 1);
}

#[test]
fn callback_default() {
    let checker = CallbackReentrancyChecker::default();
    assert_eq!(checker.current_depth(), 0);
}

// =======================================================================
// T069: TemporalDeadlineChecker tests
// =======================================================================

#[test]
fn deadline_operation_exceeds() {
    let mut checker = TemporalDeadlineChecker::new();
    checker.register_bound("heavy_compute".into(), 500);
    assert!(
        checker
            .enter_deadline("fast".into(), 100, &(0..1))
            .is_none()
    );
    let err = checker.check_operation("heavy_compute", &(5..6));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A25001");
}

#[test]
fn deadline_operation_ok() {
    let mut checker = TemporalDeadlineChecker::new();
    checker.register_bound("quick".into(), 10);
    assert!(
        checker
            .enter_deadline("normal".into(), 100, &(0..1))
            .is_none()
    );
    assert!(checker.check_operation("quick", &(5..6)).is_none());
}

#[test]
fn deadline_unbounded_operation() {
    let mut checker = TemporalDeadlineChecker::new();
    assert!(
        checker
            .enter_deadline("strict".into(), 50, &(0..1))
            .is_none()
    );
    let err = checker.check_operation("unknown_op", &(5..6));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A25003");
}

#[test]
fn deadline_nested_violation() {
    let mut checker = TemporalDeadlineChecker::new();
    assert!(
        checker
            .enter_deadline("outer".into(), 100, &(0..1))
            .is_none()
    );
    let err = checker.enter_deadline("inner".into(), 200, &(5..6));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A25002");
}

#[test]
fn deadline_nested_ok() {
    let mut checker = TemporalDeadlineChecker::new();
    assert!(
        checker
            .enter_deadline("outer".into(), 100, &(0..1))
            .is_none()
    );
    assert!(
        checker
            .enter_deadline("inner".into(), 50, &(5..6))
            .is_none()
    );
}

#[test]
fn deadline_no_context_ok() {
    let checker = TemporalDeadlineChecker::new();
    // No deadline context, any operation is fine
    assert!(checker.check_operation("anything", &(0..1)).is_none());
}

#[test]
fn deadline_current() {
    let mut checker = TemporalDeadlineChecker::new();
    assert!(checker.current_deadline().is_none());
    checker.enter_deadline("d".into(), 42, &(0..1));
    assert_eq!(checker.current_deadline(), Some(("d", 42)));
    checker.exit_deadline();
    assert!(checker.current_deadline().is_none());
}

#[test]
fn deadline_default() {
    let checker = TemporalDeadlineChecker::default();
    assert!(checker.current_deadline().is_none());
}

// =======================================================================
// T070: BinaryFormatChecker tests
// =======================================================================

#[test]
fn binary_fmt_bounds_ok() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "magic".into(),
        offset: 0,
        size: 4,
        endianness: Some(Endianness::Big),
        span: 0..1,
    });
    assert!(checker.check_bounds(100).is_empty());
}

#[test]
fn binary_fmt_bounds_overflow() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "data".into(),
        offset: 96,
        size: 8,
        endianness: Some(Endianness::Little),
        span: 0..1,
    });
    let errors = checker.check_bounds(100);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A26001");
}

#[test]
fn binary_fmt_no_endianness() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "len".into(),
        offset: 0,
        size: 4,
        endianness: None,
        span: 0..1,
    });
    let errors = checker.check_endianness();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A26003");
}

#[test]
fn binary_fmt_single_byte_no_endianness_ok() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "flags".into(),
        offset: 0,
        size: 1,
        endianness: None,
        span: 0..1,
    });
    assert!(checker.check_endianness().is_empty());
}

#[test]
fn binary_fmt_overlap() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "a".into(),
        offset: 0,
        size: 4,
        endianness: Some(Endianness::Big),
        span: 0..1,
    });
    checker.add_field(BinaryField {
        name: "b".into(),
        offset: 2,
        size: 4,
        endianness: Some(Endianness::Big),
        span: 0..1,
    });
    let errors = checker.check_overlaps();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A26004");
}

#[test]
fn binary_fmt_no_overlap() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "a".into(),
        offset: 0,
        size: 4,
        endianness: Some(Endianness::Big),
        span: 0..1,
    });
    checker.add_field(BinaryField {
        name: "b".into(),
        offset: 4,
        size: 4,
        endianness: Some(Endianness::Big),
        span: 0..1,
    });
    assert!(checker.check_overlaps().is_empty());
}

#[test]
fn binary_fmt_check_all() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "header".into(),
        offset: 0,
        size: 4,
        endianness: None,
        span: 0..1, // missing endianness
    });
    let errors = checker.check_all(100);
    assert_eq!(errors.len(), 1); // endianness only
}

#[test]
fn binary_fmt_default() {
    let checker = BinaryFormatChecker::default();
    assert!(checker.check_all(0).is_empty());
}

// =======================================================================
// T071: BitLevelChecker tests
// =======================================================================

#[test]
fn bit_level_bounds_ok() {
    let mut checker = BitLevelChecker::new(32);
    checker.add_field(BitField {
        name: "version".into(),
        bit_offset: 0,
        bit_width: 4,
        span: 0..1,
        cross_byte_ok: false,
    });
    assert!(checker.check_bounds().is_empty());
}

#[test]
fn bit_level_bounds_overflow() {
    let mut checker = BitLevelChecker::new(8);
    checker.add_field(BitField {
        name: "big".into(),
        bit_offset: 4,
        bit_width: 8,
        span: 0..1,
        cross_byte_ok: true,
    });
    let errors = checker.check_bounds();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A27001");
}

#[test]
fn bit_level_byte_crossing() {
    let mut checker = BitLevelChecker::new(16);
    checker.add_field(BitField {
        name: "cross".into(),
        bit_offset: 6,
        bit_width: 4,
        span: 0..1,
        cross_byte_ok: false,
    });
    let errors = checker.check_byte_crossing();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A27002");
}

#[test]
fn bit_level_byte_crossing_allowed() {
    let mut checker = BitLevelChecker::new(16);
    checker.add_field(BitField {
        name: "cross".into(),
        bit_offset: 6,
        bit_width: 4,
        span: 0..1,
        cross_byte_ok: true,
    });
    assert!(checker.check_byte_crossing().is_empty());
}

#[test]
fn bit_level_total_width_match() {
    let mut checker = BitLevelChecker::new(8);
    checker.add_field(BitField {
        name: "a".into(),
        bit_offset: 0,
        bit_width: 4,
        span: 0..1,
        cross_byte_ok: false,
    });
    checker.add_field(BitField {
        name: "b".into(),
        bit_offset: 4,
        bit_width: 4,
        span: 0..1,
        cross_byte_ok: false,
    });
    assert!(checker.check_total_width(8).is_none());
}

#[test]
fn bit_level_total_width_mismatch() {
    let mut checker = BitLevelChecker::new(8);
    checker.add_field(BitField {
        name: "a".into(),
        bit_offset: 0,
        bit_width: 3,
        span: 0..1,
        cross_byte_ok: false,
    });
    let err = checker.check_total_width(8);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A27003");
}

#[test]
fn bit_level_check_all() {
    let mut checker = BitLevelChecker::new(16);
    checker.add_field(BitField {
        name: "a".into(),
        bit_offset: 0,
        bit_width: 8,
        span: 0..1,
        cross_byte_ok: false,
    });
    checker.add_field(BitField {
        name: "b".into(),
        bit_offset: 8,
        bit_width: 8,
        span: 0..1,
        cross_byte_ok: false,
    });
    assert!(checker.check_all(16).is_empty());
}

// =======================================================================
// T072: StringEncodingChecker tests
// =======================================================================

#[test]
fn string_encoding_raw_bytes_error() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("data".into(), StringEncoding::RawBytes);
    let err = checker.check_use_as_string("data", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A28001");
}

#[test]
fn string_encoding_utf8_ok() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("text".into(), StringEncoding::Utf8);
    assert!(checker.check_use_as_string("text", &(0..1)).is_none());
}

#[test]
fn string_encoding_mismatch() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("wide".into(), StringEncoding::Utf16Le);
    let err = checker.check_encoding_compat("wide", &StringEncoding::Utf8, &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A28002");
}

#[test]
fn string_encoding_ascii_compat() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("ascii_str".into(), StringEncoding::Ascii);
    // ASCII is compatible with everything
    assert!(
        checker
            .check_encoding_compat("ascii_str", &StringEncoding::Utf8, &(0..1))
            .is_none()
    );
}

#[test]
fn string_encoding_truncation_utf16() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("wide".into(), StringEncoding::Utf16Le);
    let err = checker.check_truncation("wide", 5, &(0..1)); // 5 bytes, not aligned to 2
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A28003");
}

#[test]
fn string_encoding_truncation_ok() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("wide".into(), StringEncoding::Utf16Be);
    assert!(checker.check_truncation("wide", 4, &(0..1)).is_none()); // 4 bytes, aligned
}

#[test]
fn string_encoding_unknown_var() {
    let checker = StringEncodingChecker::new();
    let err = checker.check_use_as_string("unknown", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A28001");
}

#[test]
fn string_encoding_default() {
    let checker = StringEncodingChecker::default();
    assert!(checker.check_use_as_string("x", &(0..1)).is_some());
}

// =======================================================================
// T074: ChecksumChecker tests
// =======================================================================

#[test]
fn checksum_use_before_verify() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("payload".into(), ChecksumAlgorithm::Crc32, 0, 100);
    let err = checker.check_use_before_verify("payload", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A29001");
}

#[test]
fn checksum_use_after_verify_ok() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("payload".into(), ChecksumAlgorithm::Crc32, 0, 100);
    checker.mark_verified("payload");
    assert!(
        checker
            .check_use_before_verify("payload", &(0..1))
            .is_none()
    );
}

#[test]
fn checksum_algorithm_mismatch() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("data".into(), ChecksumAlgorithm::Sha256, 0, 100);
    let err = checker.check_algorithm_match("data", &ChecksumAlgorithm::Crc32, &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A29002");
}

#[test]
fn checksum_algorithm_match_ok() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("data".into(), ChecksumAlgorithm::Sha256, 0, 100);
    assert!(
        checker
            .check_algorithm_match("data", &ChecksumAlgorithm::Sha256, &(0..1))
            .is_none()
    );
}

#[test]
fn checksum_range_coverage() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("data".into(), ChecksumAlgorithm::Adler32, 10, 50);
    let err = checker.check_range_coverage("data", 0, 60, &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A29003");
}

#[test]
fn checksum_range_covered_ok() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("data".into(), ChecksumAlgorithm::Adler32, 0, 100);
    assert!(
        checker
            .check_range_coverage("data", 10, 50, &(0..1))
            .is_none()
    );
}

#[test]
fn checksum_default() {
    let checker = ChecksumChecker::default();
    assert!(checker.check_use_before_verify("x", &(0..1)).is_none());
}

// =======================================================================
// T075: ProtocolGrammarChecker tests
// =======================================================================

#[test]
fn protocol_valid_transition() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_state("connected".into());
    checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
    assert!(checker.check_send("CONNECT", &(0..1)).is_none());
    assert!(checker.transition("CONNECT", &(0..1)).is_none());
}

#[test]
fn protocol_invalid_send() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
    let err = checker.check_send("DISCONNECT", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A30002");
}

#[test]
fn protocol_invalid_transition() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
    let err = checker.transition("DATA", &(0..1));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A30001");
}

#[test]
fn protocol_required_fields() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_required_fields("CONNECT".into(), vec!["host".into(), "port".into()]);
    let errors = checker.check_required_fields("CONNECT", &["host"], &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A30003");
    assert!(errors[0].message.contains("port"));
}

#[test]
fn protocol_required_fields_ok() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_required_fields("CONNECT".into(), vec!["host".into()]);
    let errors = checker.check_required_fields("CONNECT", &["host", "port"], &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn protocol_multi_state() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_state("connected".into());
    checker.add_state("ready".into());
    checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
    checker.add_transition("connected".into(), "ready".into(), "AUTH".into());
    checker.add_transition("ready".into(), "idle".into(), "CLOSE".into());

    assert!(checker.transition("CONNECT", &(0..1)).is_none());
    assert!(checker.transition("AUTH", &(0..1)).is_none());
    assert!(checker.transition("CLOSE", &(0..1)).is_none());
}

// =======================================================================
// T077: AxiomaticDefChecker tests
// =======================================================================

#[test]
fn axiom_undefined_reference() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "ax1".into(),
        span: 0..1,
        references: vec!["foo".into()],
    });
    let errors = checker.check_references(&[]);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A31001");
}

#[test]
fn axiom_known_reference_ok() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "ax1".into(),
        span: 0..1,
        references: vec!["foo".into()],
    });
    assert!(checker.check_references(&["foo"]).is_empty());
}

#[test]
fn axiom_unused() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "unused_ax".into(),
        span: 0..1,
        references: vec![],
    });
    let errors = checker.check_unused();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A31003");
}

#[test]
fn axiom_used_ok() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "ax1".into(),
        span: 0..1,
        references: vec![],
    });
    checker.mark_used("ax1");
    assert!(checker.check_unused().is_empty());
}

#[test]
fn axiom_circular() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "a".into(),
        span: 0..1,
        references: vec!["b".into()],
    });
    checker.declare_axiom(AxiomDef {
        name: "b".into(),
        span: 0..1,
        references: vec!["a".into()],
    });
    let errors = checker.check_circular();
    assert!(!errors.is_empty());
    assert!(errors.iter().any(|e| e.code == "A31002"));
}

#[test]
fn axiom_default() {
    let checker = AxiomaticDefChecker::default();
    assert!(checker.check_unused().is_empty());
}

// =======================================================================
// T079: OpaqueFunctionChecker tests
// =======================================================================

#[test]
fn opaque_call_without_contract() {
    let mut checker = OpaqueFunctionChecker::new();
    checker.declare_opaque("secret_fn".into(), false, 0..1);
    let err = checker.check_call("secret_fn", &(5..6));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A32001");
}

#[test]
fn opaque_call_with_contract_ok() {
    let mut checker = OpaqueFunctionChecker::new();
    checker.declare_opaque("secret_fn".into(), true, 0..1);
    assert!(checker.check_call("secret_fn", &(5..6)).is_none());
}

#[test]
fn opaque_body_access_without_reveal() {
    let mut checker = OpaqueFunctionChecker::new();
    checker.declare_opaque("hidden".into(), true, 0..1);
    let err = checker.check_body_access("hidden", &(5..6));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A32002");
}

#[test]
fn opaque_reveal_outside_proof() {
    let mut checker = OpaqueFunctionChecker::new();
    checker.declare_opaque("hidden".into(), true, 0..1);
    let err = checker.reveal("hidden", &(5..6));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A32003");
}

#[test]
fn opaque_reveal_in_proof_ok() {
    let mut checker = OpaqueFunctionChecker::new();
    checker.declare_opaque("hidden".into(), true, 0..1);
    checker.enter_proof();
    assert!(checker.reveal("hidden", &(5..6)).is_none());
    // After reveal, body access is allowed
    assert!(checker.check_body_access("hidden", &(10..11)).is_none());
}

#[test]
fn opaque_is_opaque() {
    let mut checker = OpaqueFunctionChecker::new();
    assert!(!checker.is_opaque("f"));
    checker.declare_opaque("f".into(), true, 0..1);
    assert!(checker.is_opaque("f"));
}

#[test]
fn opaque_non_opaque_call_ok() {
    let checker = OpaqueFunctionChecker::new();
    assert!(checker.check_call("regular_fn", &(0..1)).is_none());
}

#[test]
fn opaque_default() {
    let checker = OpaqueFunctionChecker::default();
    assert!(!checker.is_opaque("x"));
}

// =======================================================================
// T083: TestGenerator tests
// =======================================================================

#[test]
fn test_gen_property_test() {
    let tgen = TestGenerator::new();
    let contract = TestableContract {
        name: "safe_div".into(),
        params: vec![("a".into(), Type::Int), ("b".into(), Type::Int)],
        requires: vec!["b != 0".into()],
        ensures: vec!["result * b + (a % b) == a".into()],
    };
    let test = tgen.generate_property_test(&contract);
    assert_eq!(test.kind, TestKind::Property);
    assert!(test.body.contains("proptest!"));
    assert!(test.body.contains("prop_assume!"));
    assert!(test.body.contains("b != 0"));
}

#[test]
fn test_gen_boundary_values() {
    let tgen = TestGenerator::new();
    let contract = TestableContract {
        name: "check".into(),
        params: vec![("x".into(), Type::U8)],
        requires: vec![],
        ensures: vec![],
    };
    let tests = tgen.generate_boundary_tests(&contract);
    assert_eq!(tests.len(), 3); // 0, 1, 255
    assert!(tests.iter().all(|t| t.kind == TestKind::Boundary));
}

#[test]
fn test_gen_smoke_test() {
    let tgen = TestGenerator::new();
    let contract = TestableContract {
        name: "foo".into(),
        params: vec![],
        requires: vec![],
        ensures: vec![],
    };
    let test = tgen.generate_smoke_test(&contract);
    assert_eq!(test.kind, TestKind::Smoke);
    assert!(test.body.contains("smoke_foo"));
}

#[test]
fn test_gen_generate_all() {
    let mut tgen = TestGenerator::new();
    tgen.add_contract(TestableContract {
        name: "add".into(),
        params: vec![("a".into(), Type::I32), ("b".into(), Type::I32)],
        requires: vec![],
        ensures: vec!["result == a + b".into()],
    });
    let all = tgen.generate_all();
    // 1 property + 10 boundary (5 per I32 param * 2) + 1 smoke
    assert_eq!(all.len(), 12);
}

#[test]
fn test_gen_no_requires() {
    let tgen = TestGenerator::new();
    let contract = TestableContract {
        name: "no_pre".into(),
        params: vec![("x".into(), Type::Bool)],
        requires: vec![],
        ensures: vec!["result".into()],
    };
    let test = tgen.generate_property_test(&contract);
    assert!(!test.body.contains("prop_assume!"));
}

#[test]
fn test_gen_default() {
    let tgen = TestGenerator::default();
    assert!(tgen.generate_all().is_empty());
}

// =======================================================================
// T086: CrashRecoveryChecker tests
// =======================================================================

#[test]
fn crash_recovery_write_ahead_violation() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("txn1".into());
    cr.write_data("txn1");
    let errs = cr.check_write_ahead();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A33001");
}

#[test]
fn crash_recovery_write_ahead_ok() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("txn1".into());
    cr.write_wal("txn1");
    cr.write_data("txn1");
    assert!(cr.check_write_ahead().is_empty());
}

#[test]
fn crash_recovery_commit_without_fsync() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("txn1".into());
    cr.commit("txn1");
    let errs = cr.check_commit_durability();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A33002");
}

#[test]
fn crash_recovery_fsync_before_data() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("txn1".into());
    cr.write_wal("txn1");
    cr.fsync("txn1");
    let errs = cr.check_ordering();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A33003");
}

#[test]
fn crash_recovery_full_sequence_ok() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("txn1".into());
    cr.write_wal("txn1");
    cr.write_data("txn1");
    cr.fsync("txn1");
    cr.commit("txn1");
    assert!(cr.check_all().is_empty());
}

#[test]
fn crash_recovery_default() {
    let cr = CrashRecoveryChecker::default();
    assert!(cr.check_all().is_empty());
}

// =======================================================================
// T087: PageCacheChecker tests
// =======================================================================

#[test]
fn page_cache_evict_pinned() {
    let mut pc = PageCacheChecker::new(10);
    pc.load_page(1);
    pc.pin(1);
    let err = pc.evict(1);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A34001");
}

#[test]
fn page_cache_evict_dirty() {
    let mut pc = PageCacheChecker::new(10);
    pc.load_page(1);
    pc.mark_dirty(1);
    let err = pc.evict(1);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A34002");
}

#[test]
fn page_cache_evict_clean_ok() {
    let mut pc = PageCacheChecker::new(10);
    pc.load_page(1);
    assert!(pc.evict(1).is_none());
}

#[test]
fn page_cache_capacity_exceeded() {
    let mut pc = PageCacheChecker::new(2);
    pc.load_page(1);
    pc.load_page(2);
    pc.load_page(3);
    let errs = pc.check_capacity();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A34003");
}

#[test]
fn page_cache_flush_then_evict() {
    let mut pc = PageCacheChecker::new(10);
    pc.load_page(1);
    pc.mark_dirty(1);
    pc.flush(1);
    assert!(pc.evict(1).is_none());
}

#[test]
fn page_cache_unpin_then_evict() {
    let mut pc = PageCacheChecker::new(10);
    pc.load_page(1);
    pc.pin(1);
    pc.unpin(1);
    assert!(pc.evict(1).is_none());
}

#[test]
fn page_cache_default() {
    let pc = PageCacheChecker::default();
    assert!(pc.check_capacity().is_empty());
}

// =======================================================================
// T088: MvccChecker tests
// =======================================================================

#[test]
fn mvcc_write_conflict() {
    let mut mv = MvccChecker::new();
    let t1 = mv.begin_txn();
    let t2 = mv.begin_txn();
    mv.write_version("key1".into(), t1);
    mv.write_version("key1".into(), t2);
    let errs = mv.check_write_conflicts();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A35001");
}

#[test]
fn mvcc_no_conflict_after_commit() {
    let mut mv = MvccChecker::new();
    let t1 = mv.begin_txn();
    mv.write_version("key1".into(), t1);
    mv.commit_txn(t1);
    let t2 = mv.begin_txn();
    mv.write_version("key1".into(), t2);
    assert!(mv.check_write_conflicts().is_empty());
}

#[test]
fn mvcc_snapshot_violation() {
    let mut mv = MvccChecker::new();
    let t1 = mv.begin_txn();
    let t2 = mv.begin_txn();
    mv.write_version("key1".into(), t1);
    let err = mv.check_snapshot_read("key1", t2);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A35002");
}

#[test]
fn mvcc_phantom_read() {
    let mut mv = MvccChecker::new();
    let t1 = mv.begin_txn();
    let t2 = mv.begin_txn();
    mv.write_version("key1".into(), t2);
    mv.commit_txn(t2);
    let errs = mv.check_phantom(t1);
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A35003");
}

#[test]
fn mvcc_default() {
    let mv = MvccChecker::default();
    assert!(mv.check_write_conflicts().is_empty());
}

// =======================================================================
// T089: RollbackChecker tests
// =======================================================================

#[test]
fn rollback_unknown_savepoint() {
    let mut rb = RollbackChecker::new();
    let err = rb.rollback_to("sp1");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A36001");
}

#[test]
fn rollback_resource_leak() {
    let mut rb = RollbackChecker::new();
    rb.create_savepoint("sp1".into());
    rb.acquire_resource("lock1".into());
    rb.rollback_to("sp1");
    let errs = rb.check_resource_leak();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A36002");
}

#[test]
fn rollback_resource_released_ok() {
    let mut rb = RollbackChecker::new();
    rb.create_savepoint("sp1".into());
    rb.acquire_resource("lock1".into());
    rb.release_resource("lock1");
    rb.rollback_to("sp1");
    assert!(rb.check_resource_leak().is_empty());
}

#[test]
fn rollback_duplicate_savepoint() {
    let mut rb = RollbackChecker::new();
    rb.create_savepoint("sp1".into());
    rb.create_savepoint("sp1".into());
    let errs = rb.check_savepoint_nesting();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A36003");
}

#[test]
fn rollback_default() {
    let rb = RollbackChecker::default();
    assert!(rb.check_resource_leak().is_empty());
}

// =======================================================================
// T090: MonotonicStateChecker tests
// =======================================================================

#[test]
fn monotonic_increasing_violation() {
    let mut mc = MonotonicStateChecker::new();
    mc.declare("seq".into(), MonotonicDirection::Increasing, 10, 0..1);
    let err = mc.update("seq", 5);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A37001");
}

#[test]
fn monotonic_increasing_ok() {
    let mut mc = MonotonicStateChecker::new();
    mc.declare("seq".into(), MonotonicDirection::Increasing, 10, 0..1);
    assert!(mc.update("seq", 10).is_none()); // equal allowed for Increasing
    assert!(mc.update("seq", 15).is_none());
}

#[test]
fn monotonic_strictly_increasing() {
    let mut mc = MonotonicStateChecker::new();
    mc.declare(
        "ts".into(),
        MonotonicDirection::StrictlyIncreasing,
        10,
        0..1,
    );
    let err = mc.update("ts", 10); // equal not allowed
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A37001");
}

#[test]
fn monotonic_reset_blocked() {
    let mc = MonotonicStateChecker::new();
    assert!(mc.check_reset("seq").is_none()); // not declared = no error
}

#[test]
fn monotonic_reset_declared() {
    let mut mc = MonotonicStateChecker::new();
    mc.declare("seq".into(), MonotonicDirection::Increasing, 0, 0..1);
    let err = mc.check_reset("seq");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A37002");
}

#[test]
fn monotonic_undeclared_access() {
    let mc = MonotonicStateChecker::new();
    let err = mc.check_access("unknown");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A37003");
}

#[test]
fn monotonic_current_value() {
    let mut mc = MonotonicStateChecker::new();
    mc.declare("seq".into(), MonotonicDirection::Increasing, 42, 0..1);
    assert_eq!(mc.current_value("seq"), Some(42));
    mc.update("seq", 100);
    assert_eq!(mc.current_value("seq"), Some(100));
}

#[test]
fn monotonic_default() {
    let mc = MonotonicStateChecker::default();
    assert!(mc.check_access("x").is_some());
}

// =======================================================================
// T091: StorageFailureChecker tests
// =======================================================================

#[test]
fn storage_failure_unhandled() {
    let mut sf = StorageFailureChecker::new();
    sf.declare_failure_mode(FailureMode::PartialWrite);
    let errs = sf.check_unhandled();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A38001");
}

#[test]
fn storage_failure_handled_ok() {
    let mut sf = StorageFailureChecker::new();
    sf.declare_failure_mode(FailureMode::BitRot);
    sf.mark_handled("bit_rot");
    assert!(sf.check_unhandled().is_empty());
}

#[test]
fn storage_failure_spurious_handler() {
    let mut sf = StorageFailureChecker::new();
    sf.mark_handled("nonexistent");
    let errs = sf.check_spurious_handlers();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A38002");
}

#[test]
fn storage_failure_critical_coverage() {
    let mut sf = StorageFailureChecker::new();
    sf.declare_failure_mode(FailureMode::PartialWrite);
    sf.declare_failure_mode(FailureMode::TornPage);
    let errs = sf.check_critical_coverage();
    assert_eq!(errs.len(), 2);
    assert!(errs.iter().all(|e| e.code == "A38003"));
}

#[test]
fn storage_failure_default() {
    let sf = StorageFailureChecker::default();
    assert!(sf.check_critical_coverage().is_empty());
}

// =======================================================================
// T095: NumericalPrecisionChecker tests
// =======================================================================

#[test]
fn num_precision_loss() {
    let mut np = NumericalPrecisionChecker::new();
    np.declare("x".into(), 64, 1e-15, 0..1);
    let err = np.check_precision_loss("x", 32);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A42001");
}

#[test]
fn num_precision_ok() {
    let mut np = NumericalPrecisionChecker::new();
    np.declare("x".into(), 32, 1e-7, 0..1);
    assert!(np.check_precision_loss("x", 64).is_none());
}

#[test]
fn num_ulp_violation() {
    let mut np = NumericalPrecisionChecker::new();
    np.declare("x".into(), 64, 1e-15, 0..1);
    let err = np.check_ulp_bound("x", 1e-10);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A42002");
}

#[test]
fn num_cancellation() {
    let mut np = NumericalPrecisionChecker::new();
    np.declare("x".into(), 64, 1e-15, 0..1);
    let err = np.check_cancellation("x", 0.9999);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A42003");
}

#[test]
fn num_precision_default() {
    let np = NumericalPrecisionChecker::default();
    assert!(np.check_precision_loss("x", 32).is_none());
}

// =======================================================================
// T096: PrecomputedTableChecker tests
// =======================================================================

#[test]
fn table_incomplete_coverage() {
    let mut tc = PrecomputedTableChecker::new();
    tc.declare_table("crc32".into(), 256, "gen_crc32".into(), 0..1);
    tc.mark_entries_verified("crc32", 100);
    let errs = tc.check_coverage();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A43001");
}

#[test]
fn table_full_coverage_ok() {
    let mut tc = PrecomputedTableChecker::new();
    tc.declare_table("crc32".into(), 256, "gen_crc32".into(), 0..1);
    tc.mark_entries_verified("crc32", 256);
    assert!(tc.check_coverage().is_empty());
}

#[test]
fn table_no_generator() {
    let mut tc = PrecomputedTableChecker::new();
    tc.declare_table("lut".into(), 16, "".into(), 0..1);
    let errs = tc.check_generator();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A43002");
}

#[test]
fn table_zero_size() {
    let mut tc = PrecomputedTableChecker::new();
    tc.declare_table("empty".into(), 0, "gen".into(), 0..1);
    let errs = tc.check_non_empty();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A43003");
}

#[test]
fn table_default() {
    let tc = PrecomputedTableChecker::default();
    assert!(tc.check_non_empty().is_empty());
}

// =======================================================================
// T097: PlatformAbstractionChecker tests
// =======================================================================

#[test]
fn platform_missing_impl() {
    let mut pa = PlatformAbstractionChecker::new();
    pa.add_platform("linux".into());
    pa.add_platform("windows".into());
    pa.declare_abstraction("fs_ops".into(), vec!["linux".into()]);
    let errs = pa.check_coverage();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A44001");
}

#[test]
fn platform_full_coverage_ok() {
    let mut pa = PlatformAbstractionChecker::new();
    pa.add_platform("linux".into());
    pa.declare_abstraction("fs_ops".into(), vec!["linux".into()]);
    assert!(pa.check_coverage().is_empty());
}

#[test]
fn platform_direct_use() {
    let mut pa = PlatformAbstractionChecker::new();
    pa.add_platform("linux".into());
    let err = pa.check_direct_platform_use("linux");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A44002");
}

#[test]
fn platform_unknown() {
    let mut pa = PlatformAbstractionChecker::new();
    pa.add_platform("linux".into());
    pa.declare_abstraction("net".into(), vec!["freebsd".into()]);
    let errs = pa.check_unknown_platforms();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A44003");
}

#[test]
fn platform_default() {
    let pa = PlatformAbstractionChecker::default();
    assert!(pa.check_coverage().is_empty());
}

// =======================================================================
// T098: FeatureFlagChecker tests
// =======================================================================

#[test]
fn feature_flag_unused() {
    let mut ff = FeatureFlagChecker::new();
    ff.declare("debug_mode".into(), false, vec![]);
    let errs = ff.check_unused();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A45001");
}

#[test]
fn feature_flag_used_ok() {
    let mut ff = FeatureFlagChecker::new();
    ff.declare("debug_mode".into(), false, vec![]);
    ff.mark_used("debug_mode");
    assert!(ff.check_unused().is_empty());
}

#[test]
fn feature_flag_conflict() {
    let mut ff = FeatureFlagChecker::new();
    ff.declare("a".into(), true, vec!["b".into()]);
    ff.declare("b".into(), true, vec![]);
    let errs = ff.check_conflicts();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A45002");
}

#[test]
fn feature_flag_undeclared() {
    let ff = FeatureFlagChecker::new();
    let err = ff.check_undeclared("unknown");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A45003");
}

#[test]
fn feature_flag_default() {
    let ff = FeatureFlagChecker::default();
    assert!(ff.check_unused().is_empty());
}

// =======================================================================
// T099: ResourceLimitChecker tests
// =======================================================================

#[test]
fn resource_limit_exceeded() {
    let mut rl = ResourceLimitChecker::new();
    rl.declare_limit("mem".into(), 1000, "bytes".into());
    rl.record_usage("mem", 1500);
    let errs = rl.check_limits();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A46001");
}

#[test]
fn resource_limit_ok() {
    let mut rl = ResourceLimitChecker::new();
    rl.declare_limit("mem".into(), 1000, "bytes".into());
    rl.record_usage("mem", 500);
    assert!(rl.check_limits().is_empty());
}

#[test]
fn resource_unbounded() {
    let rl = ResourceLimitChecker::new();
    let err = rl.check_unbounded("unknown");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A46002");
}

#[test]
fn resource_near_limit() {
    let mut rl = ResourceLimitChecker::new();
    rl.declare_limit("fds".into(), 100, "count".into());
    rl.record_usage("fds", 95);
    let errs = rl.check_near_limit();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A46003");
}

#[test]
fn resource_release() {
    let mut rl = ResourceLimitChecker::new();
    rl.declare_limit("mem".into(), 100, "bytes".into());
    rl.record_usage("mem", 80);
    rl.release_usage("mem", 50);
    // After releasing 50 from 80, usage is 30 which is under the 100 limit
    assert!(rl.check_limits().is_empty());
}

#[test]
fn resource_default() {
    let rl = ResourceLimitChecker::default();
    assert!(rl.check_limits().is_empty());
}

// =======================================================================
// T100: UnsafeEscapeChecker tests
// =======================================================================

#[test]
fn unsafe_no_proof() {
    let mut ue = UnsafeEscapeChecker::new();
    ue.declare_unsafe("ptr_deref".into(), vec!["aligned".into()], 0..1);
    let errs = ue.check_unproven();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A47001");
}

#[test]
fn unsafe_with_proof_ok() {
    let mut ue = UnsafeEscapeChecker::new();
    ue.declare_unsafe("ptr_deref".into(), vec!["aligned".into()], 0..1);
    ue.attach_proof("ptr_deref");
    assert!(ue.check_unproven().is_empty());
}

#[test]
fn unsafe_undischarged_obligation() {
    let mut ue = UnsafeEscapeChecker::new();
    ue.declare_unsafe(
        "cast".into(),
        vec!["valid_repr".into(), "aligned".into()],
        0..1,
    );
    ue.discharge_obligation("cast", "valid_repr".into());
    let errs = ue.check_obligations();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A47002");
}

#[test]
fn unsafe_empty_obligations() {
    let mut ue = UnsafeEscapeChecker::new();
    ue.declare_unsafe("noop".into(), vec![], 0..1);
    let errs = ue.check_empty_obligations();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A47003");
}

#[test]
fn unsafe_default() {
    let ue = UnsafeEscapeChecker::default();
    assert!(ue.check_unproven().is_empty());
}

// =======================================================================
// T101: ComplexityBoundChecker tests
// =======================================================================

#[test]
fn complexity_bound_violated() {
    let mut cb = ComplexityBoundChecker::new();
    cb.declare_bound("sort".into(), ComplexityClass::Linear, 0..1);
    cb.record_measured("sort", ComplexityClass::Quadratic);
    let errs = cb.check_bounds();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A48001");
}

#[test]
fn complexity_bound_ok() {
    let mut cb = ComplexityBoundChecker::new();
    cb.declare_bound("lookup".into(), ComplexityClass::Logarithmic, 0..1);
    cb.record_measured("lookup", ComplexityClass::Constant);
    assert!(cb.check_bounds().is_empty());
}

#[test]
fn complexity_unverified() {
    let mut cb = ComplexityBoundChecker::new();
    cb.declare_bound("search".into(), ComplexityClass::Linear, 0..1);
    let errs = cb.check_unverified();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A48002");
}

#[test]
fn complexity_exponential_warning() {
    let mut cb = ComplexityBoundChecker::new();
    cb.declare_bound("brute".into(), ComplexityClass::Exponential, 0..1);
    let errs = cb.check_expensive();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A48003");
}

#[test]
fn complexity_default() {
    let cb = ComplexityBoundChecker::default();
    assert!(cb.check_bounds().is_empty());
}

// =======================================================================
// T102: BehavioralEquivalenceChecker tests
// =======================================================================

#[test]
fn equiv_unverified() {
    let mut be = BehavioralEquivalenceChecker::new();
    be.declare(
        "eq1".into(),
        "sort_a".into(),
        "sort_b".into(),
        "Sortable".into(),
        0..1,
    );
    let errs = be.check_unverified();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A49001");
}

#[test]
fn equiv_verified_ok() {
    let mut be = BehavioralEquivalenceChecker::new();
    be.declare(
        "eq1".into(),
        "sort_a".into(),
        "sort_b".into(),
        "Sortable".into(),
        0..1,
    );
    be.mark_verified("eq1");
    assert!(be.check_unverified().is_empty());
}

#[test]
fn equiv_self_equivalence() {
    let mut be = BehavioralEquivalenceChecker::new();
    be.declare(
        "eq1".into(),
        "sort_a".into(),
        "sort_a".into(),
        "Sortable".into(),
        0..1,
    );
    let errs = be.check_self_equivalence();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A49002");
}

#[test]
fn equiv_no_contract() {
    let mut be = BehavioralEquivalenceChecker::new();
    be.declare("eq1".into(), "a".into(), "b".into(), "".into(), 0..1);
    let errs = be.check_contract_ref();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A49003");
}

#[test]
fn equiv_default() {
    let be = BehavioralEquivalenceChecker::default();
    assert!(be.check_unverified().is_empty());
}

// =======================================================================
// T103: MultiPassRefinementChecker tests
// =======================================================================

#[test]
fn refinement_incomplete() {
    let mut mp = MultiPassRefinementChecker::new();
    mp.add_pass("r1".into(), "spec".into(), "design".into(), 5, 0..1);
    mp.discharge("r1", 3);
    let errs = mp.check_complete();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A50001");
}

#[test]
fn refinement_complete_ok() {
    let mut mp = MultiPassRefinementChecker::new();
    mp.add_pass("r1".into(), "spec".into(), "design".into(), 5, 0..1);
    mp.discharge("r1", 5);
    assert!(mp.check_complete().is_empty());
}

#[test]
fn refinement_chain_gap() {
    let mut mp = MultiPassRefinementChecker::new();
    mp.add_pass("r1".into(), "spec".into(), "design".into(), 1, 0..1);
    mp.add_pass("r2".into(), "impl".into(), "code".into(), 1, 0..1);
    let errs = mp.check_chain();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A50002");
}

#[test]
fn refinement_zero_obligations() {
    let mut mp = MultiPassRefinementChecker::new();
    mp.add_pass("r1".into(), "spec".into(), "design".into(), 0, 0..1);
    let errs = mp.check_non_trivial();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A50003");
}

#[test]
fn refinement_default() {
    let mp = MultiPassRefinementChecker::default();
    assert!(mp.check_non_trivial().is_empty());
}

// =======================================================================
// T104: IncrementalContractChecker tests
// =======================================================================

#[test]
fn incremental_strengthens_precondition() {
    let mut ic = IncrementalContractChecker::new();
    ic.add_version("SafeDiv".into(), 1, 1, 1);
    ic.add_version("SafeDiv".into(), 2, 3, 1); // more requires = stronger
    let errs = ic.check_precondition_weakening();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A51001");
}

#[test]
fn incremental_weakens_postcondition() {
    let mut ic = IncrementalContractChecker::new();
    ic.add_version("SafeDiv".into(), 1, 1, 3);
    ic.add_version("SafeDiv".into(), 2, 1, 1); // fewer ensures = weaker
    let errs = ic.check_postcondition_strengthening();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A51002");
}

#[test]
fn incremental_version_gap() {
    let mut ic = IncrementalContractChecker::new();
    ic.add_version("SafeDiv".into(), 1, 1, 1);
    ic.add_version("SafeDiv".into(), 5, 1, 1);
    let errs = ic.check_version_continuity();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A51003");
}

#[test]
fn incremental_ok() {
    let mut ic = IncrementalContractChecker::new();
    ic.add_version("SafeDiv".into(), 1, 3, 1);
    ic.add_version("SafeDiv".into(), 2, 2, 2); // weaker pre, stronger post
    assert!(ic.check_precondition_weakening().is_empty());
    assert!(ic.check_postcondition_strengthening().is_empty());
}

#[test]
fn incremental_default() {
    let ic = IncrementalContractChecker::default();
    assert!(ic.check_precondition_weakening().is_empty());
}

// =======================================================================
// T105: ScopedInvariantChecker tests
// =======================================================================

#[test]
fn invariant_double_suspend() {
    let mut si = ScopedInvariantChecker::new();
    si.declare_invariant("inv1".into());
    assert!(si.suspend("inv1").is_none());
    let err = si.suspend("inv1");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A52001");
}

#[test]
fn invariant_suspend_undeclared() {
    let mut si = ScopedInvariantChecker::new();
    let err = si.suspend("unknown");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A52002");
}

#[test]
fn invariant_restore_not_suspended() {
    let mut si = ScopedInvariantChecker::new();
    si.declare_invariant("inv1".into());
    let err = si.restore("inv1");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A52003");
}

#[test]
fn invariant_suspend_restore_ok() {
    let mut si = ScopedInvariantChecker::new();
    si.declare_invariant("inv1".into());
    si.suspend("inv1");
    assert!(si.is_suspended("inv1"));
    si.restore("inv1");
    assert!(!si.is_suspended("inv1"));
    assert!(si.check_all_restored().is_empty());
}

#[test]
fn invariant_still_suspended_at_exit() {
    let mut si = ScopedInvariantChecker::new();
    si.declare_invariant("inv1".into());
    si.suspend("inv1");
    let errs = si.check_all_restored();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A52001");
}

#[test]
fn invariant_default() {
    let si = ScopedInvariantChecker::default();
    assert!(si.check_all_restored().is_empty());
}

// =======================================================================
// T107: StdlibTypes tests
// =======================================================================

#[test]
fn stdlib_has_core_types() {
    let stdlib = StdlibTypes::new();
    let types = stdlib.all_types();
    let names: Vec<&str> = types.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"Pos"));
    assert!(names.contains(&"NonNeg"));
    assert!(names.contains(&"Email"));
    assert!(names.contains(&"Uuid"));
    assert!(!names.contains(&"Unknown"));
}

#[test]
fn stdlib_default() {
    let stdlib = StdlibTypes::default();
    assert!(stdlib.all_types().len() >= 6);
}

// =======================================================================
// T108: CollectionContracts tests
// =======================================================================

#[test]
fn collection_has_standard_ops() {
    let cc = CollectionContracts::new();
    assert!(cc.lookup("sort").is_some());
    assert!(cc.lookup("filter").is_some());
    assert!(cc.lookup("map").is_some());
    assert!(cc.lookup("reverse").is_some());
}

#[test]
fn collection_sort_preserves_length() {
    let cc = CollectionContracts::new();
    let sort = cc.lookup("sort").unwrap();
    assert!(sort.preserves_length);
}

#[test]
fn collection_filter_does_not_preserve_length() {
    let cc = CollectionContracts::new();
    let filter = cc.lookup("filter").unwrap();
    assert!(!filter.preserves_length);
}

#[test]
fn collection_default() {
    let cc = CollectionContracts::default();
    assert!(cc.lookup("sort").is_some());
}

// =======================================================================
// T109: CrudAuthContracts tests
// =======================================================================

#[test]
fn crud_auth_missing_policy() {
    let mut ca = CrudAuthContracts::new();
    ca.add_crud("create_user".into(), CrudType::Create, true);
    let errs = ca.check_auth_coverage();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A53001");
}

#[test]
fn crud_auth_with_policy_ok() {
    let mut ca = CrudAuthContracts::new();
    ca.add_crud("create_user".into(), CrudType::Create, true);
    ca.add_auth_policy("create_user".into(), "admin".into(), false);
    assert!(ca.check_auth_coverage().is_empty());
}

#[test]
fn crud_delete_without_auth() {
    let mut ca = CrudAuthContracts::new();
    ca.add_crud("delete_item".into(), CrudType::Delete, false);
    let errs = ca.check_delete_protection();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A53002");
}

#[test]
fn crud_default() {
    let ca = CrudAuthContracts::default();
    assert!(ca.check_delete_protection().is_empty());
}

// =======================================================================
// T110: ContractCompositionChecker tests
// =======================================================================

#[test]
fn composition_unknown_extends() {
    let mut cc = ContractCompositionChecker::new();
    cc.declare("Child".into(), vec!["Unknown".into()], 1);
    let errs = cc.check_extends();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A54001");
}

#[test]
fn composition_valid_extends() {
    let mut cc = ContractCompositionChecker::new();
    cc.declare("Base".into(), vec![], 2);
    cc.declare("Child".into(), vec!["Base".into()], 1);
    assert!(cc.check_extends().is_empty());
}

#[test]
fn composition_circular() {
    let mut cc = ContractCompositionChecker::new();
    cc.declare("A".into(), vec!["B".into()], 1);
    cc.declare("B".into(), vec!["A".into()], 1);
    let errs = cc.check_circular();
    assert!(!errs.is_empty());
    assert!(errs.iter().any(|e| e.code == "A54002"));
}

#[test]
fn composition_diamond() {
    let mut cc = ContractCompositionChecker::new();
    cc.declare("Base".into(), vec![], 1);
    cc.declare("Left".into(), vec!["Base".into()], 1);
    cc.declare("Right".into(), vec!["Base".into()], 1);
    cc.declare("Diamond".into(), vec!["Left".into(), "Right".into()], 1);
    let errs = cc.check_diamond();
    assert!(!errs.is_empty());
    assert!(errs.iter().any(|e| e.code == "A54003"));
}

#[test]
fn composition_default() {
    let cc = ContractCompositionChecker::default();
    assert!(cc.check_extends().is_empty());
}

// =======================================================================
// T111: ContractLibraryChecker tests
// =======================================================================

#[test]
fn library_empty_exports() {
    let mut lc = ContractLibraryChecker::new();
    lc.declare_library("mylib".into(), "1.0.0".into());
    let errs = lc.check_empty_exports();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A55001");
}

#[test]
fn library_with_exports_ok() {
    let mut lc = ContractLibraryChecker::new();
    lc.declare_library("mylib".into(), "1.0.0".into());
    lc.add_export("mylib", "SafeDiv".into());
    assert!(lc.check_empty_exports().is_empty());
}

#[test]
fn library_self_dependency() {
    let mut lc = ContractLibraryChecker::new();
    lc.declare_library("mylib".into(), "1.0.0".into());
    lc.add_dependency(
        "mylib",
        LibraryDep {
            name: "mylib".into(),
            version_req: ">=1.0".into(),
        },
    );
    let errs = lc.check_circular_deps();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A55002");
}

#[test]
fn library_duplicate() {
    let mut lc = ContractLibraryChecker::new();
    lc.declare_library("mylib".into(), "1.0.0".into());
    lc.declare_library("mylib".into(), "2.0.0".into());
    let errs = lc.check_duplicates();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A55003");
}

#[test]
fn library_default() {
    let lc = ContractLibraryChecker::default();
    assert!(lc.check_empty_exports().is_empty());
}

// -----------------------------------------------------------------------
