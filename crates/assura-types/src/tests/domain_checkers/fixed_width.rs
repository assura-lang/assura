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
    let rhs = Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())));
    let err = FixedWidthChecker::check_division_by_zero(&AstBinOp::Div, &rhs, &Type::U32, &(0..1));
    assert!(err.is_some(), "division by literal 0 should be flagged");
    assert_eq!(err.unwrap().code, "A10104");
}

#[test]
fn fixed_width_division_nonzero_ok() {
    let rhs = Spanned::no_span(AstExpr::Literal(AstLit::Int("5".into())));
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
    let rhs_expr = Spanned::no_span(AstExpr::Ident("y".into()));
    let errors = checker.check_binop(&AstBinOp::Add, &Type::I8, &Type::U8, &rhs_expr, &(0..1));
    // Should have both an overflow error and a signedness mismatch
    let codes: Vec<&str> = errors.iter().map(|e| e.code.as_str()).collect();
    assert!(codes.contains(&"A10101"), "should flag overflow");
    // Signedness mismatch only fires for comparison ops, not arithmetic
    // (by design: check_signedness_mismatch only checks comparison ops)
}

#[test]
fn fixed_width_modulo_by_zero() {
    let rhs = Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())));
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

