//! CVC5 bitvector theory support (#453).
//!
//! Mirrors Z3's `BitvectorEncoder` using native CVC5 `Kind::Bitvector*` operations.
//! All operations require `feature = "cvc5-verify"`.

#![cfg(feature = "cvc5-verify")]

/// Create a bitvector constant from a `u64` value.
pub(crate) fn bv_from_u64<'a>(tm: &'a cvc5::TermManager, val: u64, width: u32) -> cvc5::Term<'a> {
    assert!(
        matches!(width, 8 | 16 | 32 | 64),
        "unsupported bitvector width: {width}"
    );
    tm.mk_bv(width, val)
}

/// Create a bitvector constant from an `i64` value.
pub(crate) fn bv_from_i64<'a>(tm: &'a cvc5::TermManager, val: i64, width: u32) -> cvc5::Term<'a> {
    assert!(
        matches!(width, 8 | 16 | 32 | 64),
        "unsupported bitvector width: {width}"
    );
    // Reinterpret as unsigned bits of the same width.
    let unsigned = match width {
        8 => (val as i8) as u8 as u64,
        16 => (val as i16) as u16 as u64,
        32 => (val as i32) as u32 as u64,
        64 => val as u64,
        _ => unreachable!(),
    };
    tm.mk_bv(width, unsigned)
}

/// Create a named bitvector variable (uninterpreted constant of BV sort).
pub(crate) fn bv_const<'a>(tm: &'a cvc5::TermManager, name: &str, width: u32) -> cvc5::Term<'a> {
    let sort = tm.mk_bv_sort(width);
    tm.mk_const(sort, name)
}

// ── Arithmetic ──────────────────────────────────────────────────────────

pub(crate) fn bvadd<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorAdd, &[a.clone(), b.clone()])
}

pub(crate) fn bvsub<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorSub, &[a.clone(), b.clone()])
}

pub(crate) fn bvmul<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorMult, &[a.clone(), b.clone()])
}

// ── Signed comparisons ──────────────────────────────────────────────────

pub(crate) fn bvslt<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorSlt, &[a.clone(), b.clone()])
}

pub(crate) fn bvsle<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorSle, &[a.clone(), b.clone()])
}

pub(crate) fn bvsgt<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorSgt, &[a.clone(), b.clone()])
}

pub(crate) fn bvsge<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorSge, &[a.clone(), b.clone()])
}

// ── Unsigned comparisons ────────────────────────────────────────────────

pub(crate) fn bvult<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorUlt, &[a.clone(), b.clone()])
}

pub(crate) fn bvule<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorUle, &[a.clone(), b.clone()])
}

pub(crate) fn bvugt<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorUgt, &[a.clone(), b.clone()])
}

pub(crate) fn bvuge<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorUge, &[a.clone(), b.clone()])
}

// ── Bitwise ─────────────────────────────────────────────────────────────

pub(crate) fn bvand<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorAnd, &[a.clone(), b.clone()])
}

pub(crate) fn bvor<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorOr, &[a.clone(), b.clone()])
}

pub(crate) fn bvxor<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorXor, &[a.clone(), b.clone()])
}

// ── Shifts ──────────────────────────────────────────────────────────────

pub(crate) fn bvshl<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorShl, &[a.clone(), b.clone()])
}

pub(crate) fn bvlshr<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorLshr, &[a.clone(), b.clone()])
}

pub(crate) fn bvashr<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorAshr, &[a.clone(), b.clone()])
}

// ── Overflow detection ──────────────────────────────────────────────────

/// Unsigned addition overflow: CVC5 `BitvectorUaddo` returns Bool directly.
pub(crate) fn bvadd_overflow_unsigned<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorUaddo, &[a.clone(), b.clone()])
}

/// Signed addition overflow: CVC5 `BitvectorSaddo` returns Bool directly.
pub(crate) fn bvadd_overflow_signed<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    b: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    tm.mk_term(cvc5::Kind::BitvectorSaddo, &[a.clone(), b.clone()])
}

// ── Extension / extraction helpers ──────────────────────────────────────

/// Zero-extend a bitvector by `extra` bits.
pub(crate) fn bv_zero_extend<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    extra: u32,
) -> cvc5::Term<'a> {
    let op = tm.mk_op(cvc5::Kind::BitvectorZeroExtend, &[extra]);
    tm.mk_term_from_op(op, std::slice::from_ref(a))
}

/// Sign-extend a bitvector by `extra` bits.
pub(crate) fn bv_sign_extend<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    extra: u32,
) -> cvc5::Term<'a> {
    let op = tm.mk_op(cvc5::Kind::BitvectorSignExtend, &[extra]);
    tm.mk_term_from_op(op, std::slice::from_ref(a))
}

/// Extract bits `[high:low]` from a bitvector (inclusive, zero-indexed).
pub(crate) fn bv_extract<'a>(
    tm: &'a cvc5::TermManager,
    a: &cvc5::Term<'a>,
    high: u32,
    low: u32,
) -> cvc5::Term<'a> {
    let op = tm.mk_op(cvc5::Kind::BitvectorExtract, &[high, low]);
    tm.mk_term_from_op(op, std::slice::from_ref(a))
}

/// Check if a CVC5 term has bitvector sort.
pub(crate) fn is_bv(term: &cvc5::Term<'_>) -> bool {
    term.sort().is_bv()
}

/// Get the bit-width of a bitvector-sorted term (defaults to 32 for non-BV).
pub(crate) fn bv_width(term: &cvc5::Term<'_>) -> u32 {
    let sort = term.sort();
    if sort.is_bv() { sort.bv_size() } else { 32 }
}
