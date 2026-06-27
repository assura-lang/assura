use super::*;

// ── Bitvector parity tests (#453) ───────────────────────────────────────

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_bv_const_sort() {
    use crate::cvc5_bitvector_encode::*;
    let tm = cvc5::TermManager::new();
    let v = bv_const(&tm, "x", 8);
    assert!(v.sort().is_bv());
    assert_eq!(v.sort().bv_size(), 8);
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_bv_from_u64() {
    use crate::cvc5_bitvector_encode::*;
    let tm = cvc5::TermManager::new();
    for width in [8, 16, 32, 64] {
        let v = bv_from_u64(&tm, 42, width);
        assert!(v.sort().is_bv());
        assert_eq!(v.sort().bv_size(), width);
    }
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_bv_from_i64() {
    use crate::cvc5_bitvector_encode::*;
    let tm = cvc5::TermManager::new();
    let v = bv_from_i64(&tm, -1, 8);
    assert!(v.sort().is_bv());
    assert_eq!(v.sort().bv_size(), 8);
    // -1 as u8 = 255 = 0xFF
    assert_eq!(v.bv_value(10), "255");
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_bv_arithmetic() {
    use crate::cvc5_bitvector_encode::*;
    let tm = cvc5::TermManager::new();
    let a = bv_const(&tm, "a", 32);
    let b = bv_const(&tm, "b", 32);

    let sum = bvadd(&tm, &a, &b);
    assert!(sum.sort().is_bv());
    assert_eq!(sum.sort().bv_size(), 32);

    let diff = bvsub(&tm, &a, &b);
    assert!(diff.sort().is_bv());

    let prod = bvmul(&tm, &a, &b);
    assert!(prod.sort().is_bv());
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_bv_comparisons() {
    use crate::cvc5_bitvector_encode::*;
    let tm = cvc5::TermManager::new();
    let a = bv_const(&tm, "a", 16);
    let b = bv_const(&tm, "b", 16);

    let slt = bvslt(&tm, &a, &b);
    assert!(slt.sort().is_boolean());

    let sle = bvsle(&tm, &a, &b);
    assert!(sle.sort().is_boolean());

    let ult = bvult(&tm, &a, &b);
    assert!(ult.sort().is_boolean());

    let ule = bvule(&tm, &a, &b);
    assert!(ule.sort().is_boolean());
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_bv_bitwise() {
    use crate::cvc5_bitvector_encode::*;
    let tm = cvc5::TermManager::new();
    let a = bv_const(&tm, "a", 8);
    let b = bv_const(&tm, "b", 8);

    let and_val = bvand(&tm, &a, &b);
    assert!(and_val.sort().is_bv());

    let or_val = bvor(&tm, &a, &b);
    assert!(or_val.sort().is_bv());

    let xor_val = bvxor(&tm, &a, &b);
    assert!(xor_val.sort().is_bv());
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_bv_shifts() {
    use crate::cvc5_bitvector_encode::*;
    let tm = cvc5::TermManager::new();
    let a = bv_const(&tm, "a", 32);
    let b = bv_const(&tm, "b", 32);

    let shl = bvshl(&tm, &a, &b);
    assert!(shl.sort().is_bv());

    let lshr = bvlshr(&tm, &a, &b);
    assert!(lshr.sort().is_bv());

    let ashr = bvashr(&tm, &a, &b);
    assert!(ashr.sort().is_bv());
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_bv_overflow_detection() {
    use crate::cvc5_bitvector_encode::*;
    let tm = cvc5::TermManager::new();
    let a = bv_const(&tm, "a", 8);
    let b = bv_const(&tm, "b", 8);

    let uaddo = bvadd_overflow_unsigned(&tm, &a, &b);
    assert!(uaddo.sort().is_boolean());

    let saddo = bvadd_overflow_signed(&tm, &a, &b);
    assert!(saddo.sort().is_boolean());
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_bv_extension_extraction() {
    use crate::cvc5_bitvector_encode::*;
    let tm = cvc5::TermManager::new();
    let a = bv_const(&tm, "a", 8);

    let zext = bv_zero_extend(&tm, &a, 8);
    assert!(zext.sort().is_bv());
    assert_eq!(zext.sort().bv_size(), 16);

    let sext = bv_sign_extend(&tm, &a, 8);
    assert!(sext.sort().is_bv());
    assert_eq!(sext.sort().bv_size(), 16);

    let extr = bv_extract(&tm, &a, 7, 4);
    assert!(extr.sort().is_bv());
    assert_eq!(extr.sort().bv_size(), 4);
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_bv_is_bv_and_width() {
    use crate::cvc5_bitvector_encode::*;
    let tm = cvc5::TermManager::new();
    let bv_term = bv_const(&tm, "x", 32);
    let int_term = tm.mk_const(tm.integer_sort(), "y");

    assert!(is_bv(&bv_term));
    assert!(!is_bv(&int_term));
    assert_eq!(bv_width(&bv_term), 32);
    assert_eq!(bv_width(&int_term), 32); // fallback
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_bv_overflow_sat_check() {
    // Verify overflow detection semantics: u8 250 + 10 overflows.
    use crate::cvc5_bitvector_encode::*;
    let tm = cvc5::TermManager::new();
    let mut solver = cvc5::Solver::new(&tm);
    solver.set_logic("ALL");
    solver.set_option("produce-models", "true");

    let a = bv_from_u64(&tm, 250, 8);
    let b = bv_from_u64(&tm, 10, 8);
    let overflow = bvadd_overflow_unsigned(&tm, &a, &b);
    solver.assert_formula(overflow);
    let result = solver.check_sat();
    assert!(result.is_sat(), "250u8 + 10u8 should overflow");
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_bv_no_overflow_sat_check() {
    // Verify no overflow: u8 100 + 100 = 200 (no overflow).
    use crate::cvc5_bitvector_encode::*;
    let tm = cvc5::TermManager::new();
    let mut solver = cvc5::Solver::new(&tm);
    solver.set_logic("ALL");
    solver.set_option("produce-models", "true");

    let a = bv_from_u64(&tm, 100, 8);
    let b = bv_from_u64(&tm, 100, 8);
    let overflow = bvadd_overflow_unsigned(&tm, &a, &b);
    // Assert NOT overflow.
    let no_overflow = tm.mk_term(cvc5::Kind::Not, &[overflow]);
    solver.assert_formula(no_overflow);
    let result = solver.check_sat();
    assert!(result.is_sat(), "100u8 + 100u8 should not overflow");
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_bv_signed_overflow_sat_check() {
    // i8 120 + 20 = 140 > 127 overflows signed.
    use crate::cvc5_bitvector_encode::*;
    let tm = cvc5::TermManager::new();
    let mut solver = cvc5::Solver::new(&tm);
    solver.set_logic("ALL");
    solver.set_option("produce-models", "true");

    let a = bv_from_i64(&tm, 120, 8);
    let b = bv_from_i64(&tm, 20, 8);
    let overflow = bvadd_overflow_signed(&tm, &a, &b);
    solver.assert_formula(overflow);
    let result = solver.check_sat();
    assert!(result.is_sat(), "120i8 + 20i8 should overflow signed");
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_bv_param_registration() {
    // Verify register_cvc5_fixed_width_params creates BV-sorted vars.
    use crate::cvc5_encoder_state::default_cvc5_encoder_state;
    use crate::cvc5_verify_native_solver::register_cvc5_fixed_width_params;
    use std::collections::HashMap;

    let tm = cvc5::TermManager::new();
    let mut var_map: HashMap<String, cvc5::Term> = HashMap::new();
    var_map.insert("x".to_string(), tm.mk_const(tm.integer_sort(), "x"));
    var_map.insert("y".to_string(), tm.mk_const(tm.integer_sort(), "y"));

    let mut enc_state = default_cvc5_encoder_state();

    let params = vec![
        assura_ast::Param {
            name: "x".to_string(),
            ty: Some(assura_ast::TypeExpr::Named("u32".to_string())),
        },
        assura_ast::Param {
            name: "y".to_string(),
            ty: Some(assura_ast::TypeExpr::Named("Int".to_string())),
        },
    ];

    register_cvc5_fixed_width_params(&tm, &params, &mut var_map, &mut enc_state);

    // x should now be BV-sorted.
    let x = var_map.get("x").unwrap();
    assert!(x.sort().is_bv(), "u32 param should be BV-sorted");
    assert_eq!(x.sort().bv_size(), 32);

    // y should remain integer-sorted.
    let y = var_map.get("y").unwrap();
    assert!(
        y.sort().is_integer(),
        "Int param should remain integer-sorted"
    );

    // bv_signed should have x as unsigned.
    assert_eq!(enc_state.bv_signed.get("x"), Some(&false));
    assert!(enc_state.bv_signed.get("y").is_none());
}

#[cfg(feature = "cvc5-verify")]
#[test]
fn test_cvc5_fixed_width_bits_shared() {
    // Verify the shared fixed_width_bits function (moved from Z3 Encoder).
    use crate::prelude_policy::fixed_width_bits;
    assert_eq!(fixed_width_bits(&["u8".into()]), Some((8, false)));
    assert_eq!(fixed_width_bits(&["i64".into()]), Some((64, true)));
    assert_eq!(fixed_width_bits(&["Int".into()]), None);
    assert_eq!(fixed_width_bits(&["u8".into(), "extra".into()]), None);
}
