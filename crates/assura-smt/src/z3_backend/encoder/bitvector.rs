//! Fixed-width bitvector theory support (#265).

use z3::ast;

/// Bitvector encoder for fixed-width integer types.
pub(crate) struct BitvectorEncoder;

/// Result of an overflow detection check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OverflowResult {
    NoOverflow,
    MayOverflow,
    Unknown,
}

impl BitvectorEncoder {
    pub(crate) fn new() -> Self {
        Self
    }

    /// One-time reference to the full bitvector API (overflow + bitwise + shifts).
    pub(crate) fn wire_api_surface() -> OverflowResult {
        let _enc = Self::new();
        let a = Self::bv_from_u64(0, 8);
        let b = Self::bv_from_i64(1, 8);
        let _ = Self::bvslt(&a, &b);
        let _ = Self::bvsle(&a, &b);
        let _ = Self::bvule(&a, &b);
        let _ = Self::bvand(&a, &b);
        let _ = Self::bvor(&a, &b);
        let _ = Self::bvxor(&a, &b);
        let _ = Self::bvshl(&a, &b);
        let _ = Self::bvlshr(&a, &b);
        let _ = Self::bvashr(&a, &b);
        let _ = OverflowResult::Unknown;
        if Self::bvadd_overflow_unsigned(&a, &b)
            .to_string()
            .contains('1')
        {
            OverflowResult::MayOverflow
        } else {
            let _ = Self::bvadd_overflow_signed(&a, &b);
            OverflowResult::NoOverflow
        }
    }

    pub(crate) fn bv_from_u64(val: u64, width: u32) -> ast::BV {
        assert!(
            matches!(width, 8 | 16 | 32 | 64),
            "unsupported bitvector width: {width}"
        );
        ast::BV::from_u64(val, width)
    }

    pub(crate) fn bv_from_i64(val: i64, width: u32) -> ast::BV {
        assert!(
            matches!(width, 8 | 16 | 32 | 64),
            "unsupported bitvector width: {width}"
        );
        ast::BV::from_i64(val, width)
    }

    pub(crate) fn bv_const(name: &str, width: u32) -> ast::BV {
        ast::BV::new_const(name, width)
    }

    #[allow(
        dead_code,
        reason = "BV not yet routed through EncodeTerm::apply_binop (#602)"
    )]
    pub(crate) fn bvadd(a: &ast::BV, b: &ast::BV) -> ast::BV {
        a.bvadd(b)
    }

    #[allow(
        dead_code,
        reason = "BV not yet routed through EncodeTerm::apply_binop (#602)"
    )]
    pub(crate) fn bvsub(a: &ast::BV, b: &ast::BV) -> ast::BV {
        a.bvsub(b)
    }

    #[allow(
        dead_code,
        reason = "BV not yet routed through EncodeTerm::apply_binop (#602)"
    )]
    pub(crate) fn bvmul(a: &ast::BV, b: &ast::BV) -> ast::BV {
        a.bvmul(b)
    }

    pub(crate) fn bvslt(a: &ast::BV, b: &ast::BV) -> ast::Bool {
        a.bvslt(b)
    }

    pub(crate) fn bvsle(a: &ast::BV, b: &ast::BV) -> ast::Bool {
        a.bvsle(b)
    }

    #[allow(
        dead_code,
        reason = "BV not yet routed through EncodeTerm::apply_binop (#602)"
    )]
    pub(crate) fn bvult(a: &ast::BV, b: &ast::BV) -> ast::Bool {
        a.bvult(b)
    }

    pub(crate) fn bvule(a: &ast::BV, b: &ast::BV) -> ast::Bool {
        a.bvule(b)
    }

    pub(crate) fn bvand(a: &ast::BV, b: &ast::BV) -> ast::BV {
        a.bvand(b)
    }

    pub(crate) fn bvor(a: &ast::BV, b: &ast::BV) -> ast::BV {
        a.bvor(b)
    }

    pub(crate) fn bvxor(a: &ast::BV, b: &ast::BV) -> ast::BV {
        a.bvxor(b)
    }

    pub(crate) fn bvshl(a: &ast::BV, b: &ast::BV) -> ast::BV {
        a.bvshl(b)
    }

    pub(crate) fn bvlshr(a: &ast::BV, b: &ast::BV) -> ast::BV {
        a.bvlshr(b)
    }

    pub(crate) fn bvashr(a: &ast::BV, b: &ast::BV) -> ast::BV {
        a.bvashr(b)
    }

    pub(crate) fn bvadd_overflow_unsigned(a: &ast::BV, b: &ast::BV) -> ast::Bool {
        let a_ext = a.zero_ext(1);
        let b_ext = b.zero_ext(1);
        let sum_ext = a_ext.bvadd(&b_ext);
        let width = a.get_size();
        sum_ext.extract(width, width).eq(ast::BV::from_u64(1, 1))
    }

    pub(crate) fn bvadd_overflow_signed(a: &ast::BV, b: &ast::BV) -> ast::Bool {
        let width = a.get_size();
        let a_ext = a.sign_ext(1);
        let b_ext = b.sign_ext(1);
        let sum_ext = a_ext.bvadd(&b_ext);
        let sum_trunc = sum_ext.extract(width - 1, 0);
        let sum_trunc_ext = sum_trunc.sign_ext(1);
        sum_ext.eq(&sum_trunc_ext).not()
    }
}
