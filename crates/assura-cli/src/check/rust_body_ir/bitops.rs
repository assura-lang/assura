//! Bit products, bitops, bit counts, ilog, next_power_of_two IR helpers.
use super::emit_pow2_factor;

/// Const mask as unsigned bit pattern in `bits` width (two's complement for negatives).
pub(super) fn mask_bits_u64(m: i64, bits: u32) -> u64 {
    let width_mask = if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    (m as u64) & width_mask
}

/// Map signed Int slot to unsigned bit pattern in `[0, m)`.
pub(super) fn emit_to_unsigned_bits(
    a: usize,
    mslot: usize,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> usize {
    let t1 = *next;
    *next += 1;
    lines.push(format!("${t1} = arith mod ${a} ${mslot} : Int"));
    let t2 = *next;
    *next += 1;
    lines.push(format!("${t2} = arith add ${t1} ${mslot} : Int"));
    let u = *next;
    *next += 1;
    lines.push(format!("${u} = arith mod ${t2} ${mslot} : Int"));
    u
}

/// Reinterpret unsigned bit pattern in `[0, m)` as signed Int using `hi` max.
pub(super) fn emit_from_unsigned_bits(
    u: usize,
    mslot: usize,
    hi: i64,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> usize {
    let his = *next;
    *next += 1;
    lines.push(format!("${his} = const {hi} : Int"));
    let gt = *next;
    *next += 1;
    lines.push(format!("${gt} = cmp gt ${u} ${his} : Bool"));
    let adj = *next;
    *next += 1;
    lines.push(format!("${adj} = arith mul ${gt} ${mslot} : Int"));
    let slot = *next;
    *next += 1;
    lines.push(format!("${slot} = arith sub ${u} ${adj} : Int"));
    slot
}

#[derive(Clone, Copy)]
pub(super) enum BitOpKind {
    And,
    Or,
    Xor,
}

/// Variable unsigned `a &/|/^ mask` with const non-neg `mask` (bits ≤64).
/// Bit product encode: extract bit_i of `a`, combine with known mask bit, sum * 2^i.
pub(super) fn encode_unsigned_bitop_var_const(
    a: usize,
    mask: u64,
    op: BitOpKind,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    if bits == 0 || bits > 64 {
        return None;
    }
    let width_mask = if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let mask = mask & width_mask;
    // Identity / zero peeps
    if mask == 0 {
        match op {
            BitOpKind::And => {
                let slot = *next;
                *next += 1;
                lines.push(format!("${slot} = const 0 : Int"));
                return Some(slot);
            }
            BitOpKind::Or | BitOpKind::Xor => return Some(a),
        }
    }
    if mask == width_mask {
        match op {
            BitOpKind::And => return Some(a),
            BitOpKind::Or => {
                // all-ones: 2^bits - 1 (synthetic when bits==64)
                if bits < 64 {
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = const {mask} : Int"));
                    return Some(slot);
                }
                let half = *next;
                *next += 1;
                lines.push(format!("${half} = const 4294967296 : Int"));
                let m = *next;
                *next += 1;
                lines.push(format!("${m} = arith mul ${half} ${half} : Int"));
                let one = *next;
                *next += 1;
                lines.push(format!("${one} = const 1 : Int"));
                let slot = *next;
                *next += 1;
                lines.push(format!("${slot} = arith sub ${m} ${one} : Int"));
                return Some(slot);
            }
            BitOpKind::Xor => { /* fall through: bitwise not */ }
        }
    }
    let two = *next;
    *next += 1;
    lines.push(format!("${two} = const 2 : Int"));
    let one = *next;
    *next += 1;
    lines.push(format!("${one} = const 1 : Int"));
    let zero = *next;
    *next += 1;
    lines.push(format!("${zero} = const 0 : Int"));
    let mut acc = zero;
    for i in 0..bits {
        let m_bit = ((mask >> i) & 1) as i64;
        match (op, m_bit) {
            (BitOpKind::And, 0) => continue,
            (BitOpKind::Or, 1) => {
                // result bit is 1
                let f = emit_pow2_factor(i, lines, next)?;
                let sum = *next;
                *next += 1;
                lines.push(format!("${sum} = arith add ${acc} ${f} : Int"));
                acc = sum;
                continue;
            }
            _ => {}
        }
        let f = emit_pow2_factor(i, lines, next)?;
        let shifted = *next;
        *next += 1;
        lines.push(format!("${shifted} = arith div ${a} ${f} : Int"));
        let bit = *next;
        *next += 1;
        lines.push(format!("${bit} = arith mod ${shifted} ${two} : Int"));
        // result_bit in {0,1}
        let rbit = match (op, m_bit) {
            (BitOpKind::And, 1) | (BitOpKind::Or, 0) | (BitOpKind::Xor, 0) => bit,
            (BitOpKind::Xor, 1) => {
                // 1 - bit
                let inv = *next;
                *next += 1;
                lines.push(format!("${inv} = arith sub ${one} ${bit} : Int"));
                inv
            }
            _ => unreachable!("handled by match above"),
        };
        let term = *next;
        *next += 1;
        lines.push(format!("${term} = arith mul ${rbit} ${f} : Int"));
        let sum = *next;
        *next += 1;
        lines.push(format!("${sum} = arith add ${acc} ${term} : Int"));
        acc = sum;
    }
    Some(acc)
}

/// Both-variable unsigned `a &/|/^ b` (bits ≤32) via bit products.
pub(super) fn encode_unsigned_bitop_var_var(
    a: usize,
    b: usize,
    op: BitOpKind,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    if bits == 0 || bits > 64 {
        return None;
    }
    let two = *next;
    *next += 1;
    lines.push(format!("${two} = const 2 : Int"));
    let one = *next;
    *next += 1;
    lines.push(format!("${one} = const 1 : Int"));
    let zero = *next;
    *next += 1;
    lines.push(format!("${zero} = const 0 : Int"));
    let mut acc = zero;
    for i in 0..bits {
        let f = emit_pow2_factor(i, lines, next)?;
        let sa = *next;
        *next += 1;
        lines.push(format!("${sa} = arith div ${a} ${f} : Int"));
        let ba = *next;
        *next += 1;
        lines.push(format!("${ba} = arith mod ${sa} ${two} : Int"));
        let sb = *next;
        *next += 1;
        lines.push(format!("${sb} = arith div ${b} ${f} : Int"));
        let bb = *next;
        *next += 1;
        lines.push(format!("${bb} = arith mod ${sb} ${two} : Int"));
        let rbit = match op {
            BitOpKind::And => {
                let p = *next;
                *next += 1;
                lines.push(format!("${p} = arith mul ${ba} ${bb} : Int"));
                p
            }
            BitOpKind::Or => {
                // ba + bb - ba*bb
                let s = *next;
                *next += 1;
                lines.push(format!("${s} = arith add ${ba} ${bb} : Int"));
                let p = *next;
                *next += 1;
                lines.push(format!("${p} = arith mul ${ba} ${bb} : Int"));
                let o = *next;
                *next += 1;
                lines.push(format!("${o} = arith sub ${s} ${p} : Int"));
                o
            }
            BitOpKind::Xor => {
                // ba + bb - 2*ba*bb
                let s = *next;
                *next += 1;
                lines.push(format!("${s} = arith add ${ba} ${bb} : Int"));
                let p = *next;
                *next += 1;
                lines.push(format!("${p} = arith mul ${ba} ${bb} : Int"));
                let two_p = *next;
                *next += 1;
                lines.push(format!("${two_p} = arith mul ${two} ${p} : Int"));
                let x = *next;
                *next += 1;
                lines.push(format!("${x} = arith sub ${s} ${two_p} : Int"));
                x
            }
        };
        let term = *next;
        *next += 1;
        lines.push(format!("${term} = arith mul ${rbit} ${f} : Int"));
        let sum = *next;
        *next += 1;
        lines.push(format!("${sum} = arith add ${acc} ${term} : Int"));
        acc = sum;
    }
    // Defense: keep result in [0, 2^bits) even if bit extraction is off for
    // unconstrained Int models (helps range ensures like result <= 255).
    let mslot = if bits == 64 {
        // synthetic 2^64 = 2^32 * 2^32
        let half = *next;
        *next += 1;
        lines.push(format!("${half} = const 4294967296 : Int"));
        let m = *next;
        *next += 1;
        lines.push(format!("${m} = arith mul ${half} ${half} : Int"));
        m
    } else {
        let modulus = 1i64 << bits;
        let m = *next;
        *next += 1;
        lines.push(format!("${m} = const {modulus} : Int"));
        m
    };
    let t1 = *next;
    *next += 1;
    lines.push(format!("${t1} = arith mod ${acc} ${mslot} : Int"));
    let t2 = *next;
    *next += 1;
    lines.push(format!("${t2} = arith add ${t1} ${mslot} : Int"));
    let u = *next;
    *next += 1;
    lines.push(format!("${u} = arith mod ${t2} ${mslot} : Int"));
    Some(u)
}

/// Integer square root for unsigned `a` with width `bits`.
/// bits ≤16: dense ladder over `r` in `0..=floor(sqrt(2^bits-1))`.
/// bits 17..=64: unrolled binary search (SMT Int mul; 32 iters for roots ≤ 2^32-1).
pub(super) fn encode_unsigned_isqrt(
    a: usize,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    if bits == 0 || bits > 64 {
        return None;
    }
    if bits > 16 {
        return encode_unsigned_isqrt_binsearch(a, bits, lines, next);
    }
    let max_val = (1u64 << bits) - 1;
    let max_root = (max_val as f64).sqrt().floor() as u32;
    let zero = *next;
    *next += 1;
    lines.push(format!("${zero} = const 0 : Int"));
    let mut acc = zero;
    for r in 0..=max_root {
        let lo_sq = (r as u64) * (r as u64);
        let hi_sq = ((r as u64) + 1) * ((r as u64) + 1);
        let r_c = *next;
        *next += 1;
        lines.push(format!("${r_c} = const {r} : Int"));
        let lo = *next;
        *next += 1;
        lines.push(format!("${lo} = const {lo_sq} : Int"));
        let ge = *next;
        *next += 1;
        lines.push(format!("${ge} = cmp ge ${a} ${lo} : Bool"));
        let sel = if hi_sq > max_val {
            ge
        } else {
            let hi = *next;
            *next += 1;
            lines.push(format!("${hi} = const {hi_sq} : Int"));
            let lt = *next;
            *next += 1;
            lines.push(format!("${lt} = cmp lt ${a} ${hi} : Bool"));
            let s = *next;
            *next += 1;
            lines.push(format!("${s} = arith mul ${ge} ${lt} : Bool"));
            s
        };
        let term = *next;
        *next += 1;
        lines.push(format!("${term} = arith mul ${sel} ${r_c} : Int"));
        let sum = *next;
        *next += 1;
        lines.push(format!("${sum} = arith add ${acc} ${term} : Int"));
        acc = sum;
    }
    Some(acc)
}

/// Unrolled binary search isqrt for bits in 17..=64.
/// mid² uses unbounded SMT Int (not i64); max_root for u64 is 2^32-1.
fn encode_unsigned_isqrt_binsearch(
    a: usize,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    if !(17..=64).contains(&bits) {
        return None;
    }
    let max_val = if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let max_root = max_val.isqrt() as i64;
    // log2(max_root)+1 iters; 32 covers roots through 2^32-1 (u64 isqrt)
    let iters = if bits > 32 { 32 } else { 16 };
    let zero = *next;
    *next += 1;
    lines.push(format!("${zero} = const 0 : Int"));
    let one = *next;
    *next += 1;
    lines.push(format!("${one} = const 1 : Int"));
    let two = *next;
    *next += 1;
    lines.push(format!("${two} = const 2 : Int"));
    let hi0 = *next;
    *next += 1;
    lines.push(format!("${hi0} = const {max_root} : Int"));
    let mut lo = zero;
    let mut hi = hi0;
    for _ in 0..iters {
        // mid = lo + (hi - lo) / 2
        let diff = *next;
        *next += 1;
        lines.push(format!("${diff} = arith sub ${hi} ${lo} : Int"));
        let half = *next;
        *next += 1;
        lines.push(format!("${half} = arith div ${diff} ${two} : Int"));
        let mid = *next;
        *next += 1;
        lines.push(format!("${mid} = arith add ${lo} ${half} : Int"));
        // mid2 = mid * mid
        let mid2 = *next;
        *next += 1;
        lines.push(format!("${mid2} = arith mul ${mid} ${mid} : Int"));
        // le = mid2 <= a
        let le = *next;
        *next += 1;
        lines.push(format!("${le} = cmp le ${mid2} ${a} : Bool"));
        // lo' = lo + (mid - lo) * le
        let dlo = *next;
        *next += 1;
        lines.push(format!("${dlo} = arith sub ${mid} ${lo} : Int"));
        let dlo_s = *next;
        *next += 1;
        lines.push(format!("${dlo_s} = arith mul ${dlo} ${le} : Int"));
        let lo_new = *next;
        *next += 1;
        lines.push(format!("${lo_new} = arith add ${lo} ${dlo_s} : Int"));
        // hi' = (mid - 1) + (hi - (mid - 1)) * le
        // when le: hi' = hi; when !le: hi' = mid - 1
        let mid_m1 = *next;
        *next += 1;
        lines.push(format!("${mid_m1} = arith sub ${mid} ${one} : Int"));
        let dhi = *next;
        *next += 1;
        lines.push(format!("${dhi} = arith sub ${hi} ${mid_m1} : Int"));
        let dhi_s = *next;
        *next += 1;
        lines.push(format!("${dhi_s} = arith mul ${dhi} ${le} : Int"));
        let hi_new = *next;
        *next += 1;
        lines.push(format!("${hi_new} = arith add ${mid_m1} ${dhi_s} : Int"));
        lo = lo_new;
        hi = hi_new;
    }
    let _ = hi;
    Some(lo)
}

/// `next_power_of_two` for unsigned `a` with width `bits` (≤64).
/// Ladder: for pot `2^k` (k=0..bits-1), select when `a` is in `(prev, pot]`.
/// When `a > 2^(bits-1)`, result is 0 (Rust non-wrapping panics; wrapping wraps).
pub(super) fn encode_unsigned_next_power_of_two(
    a: usize,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    if bits == 0 || bits > 64 {
        return None;
    }
    let zero = *next;
    *next += 1;
    lines.push(format!("${zero} = const 0 : Int"));
    let mut acc = zero;
    let mut prev: Option<usize> = None;
    for k in 0..bits {
        // 2^k via emit_pow2_factor (handles 2^63; no 2^64 in ladder).
        let pot = emit_pow2_factor(k, lines, next)?;
        let le = *next;
        *next += 1;
        lines.push(format!("${le} = cmp le ${a} ${pot} : Bool"));
        let sel = if let Some(p) = prev {
            let gt = *next;
            *next += 1;
            lines.push(format!("${gt} = cmp gt ${a} ${p} : Bool"));
            let s = *next;
            *next += 1;
            lines.push(format!("${s} = arith mul ${gt} ${le} : Bool"));
            s
        } else {
            // k==0: pot=1 covers a==0 and a==1
            le
        };
        let term = *next;
        *next += 1;
        lines.push(format!("${term} = arith mul ${sel} ${pot} : Int"));
        let sum = *next;
        *next += 1;
        lines.push(format!("${sum} = arith add ${acc} ${term} : Int"));
        acc = sum;
        prev = Some(pot);
    }
    Some(acc)
}

/// `ilog2` for unsigned `a` with width `bits`: highest set bit index.
/// `sum_i i * bit_i * prod_{j>i}(1-bit_j)`. When `a==0`, result is 0 (Rust panics;
/// documented honesty: not a panic model; range ensures still CE if they require
/// a nonzero log for all inputs).
pub(super) fn encode_unsigned_ilog2(
    a: usize,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    if bits == 0 || bits > 64 {
        return None;
    }
    let two = *next;
    *next += 1;
    lines.push(format!("${two} = const 2 : Int"));
    let one = *next;
    *next += 1;
    lines.push(format!("${one} = const 1 : Int"));
    let zero = *next;
    *next += 1;
    lines.push(format!("${zero} = const 0 : Int"));
    // higher bits still zero (start from MSB side)
    let mut higher_zero = one;
    let mut acc = zero;
    for i in (0..bits).rev() {
        let f = emit_pow2_factor(i, lines, next)?;
        let shifted = *next;
        *next += 1;
        lines.push(format!("${shifted} = arith div ${a} ${f} : Int"));
        let bit = *next;
        *next += 1;
        lines.push(format!("${bit} = arith mod ${shifted} ${two} : Int"));
        // term = i * bit * higher_zero
        let i_c = *next;
        *next += 1;
        lines.push(format!("${i_c} = const {i} : Int"));
        let ib = *next;
        *next += 1;
        lines.push(format!("${ib} = arith mul ${i_c} ${bit} : Int"));
        let term = *next;
        *next += 1;
        lines.push(format!("${term} = arith mul ${ib} ${higher_zero} : Int"));
        let new_acc = *next;
        *next += 1;
        lines.push(format!("${new_acc} = arith add ${acc} ${term} : Int"));
        acc = new_acc;
        // higher_zero *= (1 - bit)
        let one_m = *next;
        *next += 1;
        lines.push(format!("${one_m} = arith sub ${one} ${bit} : Int"));
        let new_hz = *next;
        *next += 1;
        lines.push(format!(
            "${new_hz} = arith mul ${higher_zero} ${one_m} : Int"
        ));
        higher_zero = new_hz;
    }
    Some(acc)
}

/// `ilog10` for unsigned `a` with max value `hi` (path-param bound).
/// `sum_{k=1..floor(log10(hi))} (a >= 10^k)`. When `a==0`, result is 0.
/// For full u64 domain pass `hi = -1` (sentinel): thresholds through 10^19.
pub(super) fn encode_unsigned_ilog10(
    a: usize,
    hi: i64,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    let max_k = if hi == -1 {
        // u64 path-param domain
        19u32
    } else if hi <= 0 {
        return None;
    } else {
        (hi as u64).ilog10()
    };
    if max_k == 0 {
        // hi < 10: always 0
        let slot = *next;
        *next += 1;
        lines.push(format!("${slot} = const 0 : Int"));
        return Some(slot);
    }
    let zero = *next;
    *next += 1;
    lines.push(format!("${zero} = const 0 : Int"));
    let ten = *next;
    *next += 1;
    lines.push(format!("${ten} = const 10 : Int"));
    let mut acc = zero;
    // thr starts at 10^1 = 10
    let mut thr_slot = ten;
    for k in 1..=max_k {
        if k > 1 {
            // thr *= 10 (works past i64::MAX via successive mul)
            let next_thr = *next;
            *next += 1;
            lines.push(format!("${next_thr} = arith mul ${thr_slot} ${ten} : Int"));
            thr_slot = next_thr;
        }
        let ge = *next;
        *next += 1;
        lines.push(format!("${ge} = cmp ge ${a} ${thr_slot} : Bool"));
        let sum = *next;
        *next += 1;
        lines.push(format!("${sum} = arith add ${acc} ${ge} : Int"));
        acc = sum;
    }
    Some(acc)
}

/// Popcount bit-sum for an unsigned value already in slot `a` with width `bits`.
/// Emits `sum_i (a / 2^i) mod 2` into IR; returns the accumulator slot.
pub(super) fn encode_bit_sum_count_ones(
    a: usize,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    if bits == 0 || bits > 64 {
        return None;
    }
    let two = *next;
    *next += 1;
    lines.push(format!("${two} = const 2 : Int"));
    let mut acc: Option<usize> = None;
    for i in 0..bits {
        let f = emit_pow2_factor(i, lines, next)?;
        let shifted = *next;
        *next += 1;
        lines.push(format!("${shifted} = arith div ${a} ${f} : Int"));
        let bit = *next;
        *next += 1;
        lines.push(format!("${bit} = arith mod ${shifted} ${two} : Int"));
        acc = Some(match acc {
            None => bit,
            Some(prev) => {
                let sum = *next;
                *next += 1;
                lines.push(format!("${sum} = arith add ${prev} ${bit} : Int"));
                sum
            }
        });
    }
    acc
}

/// Bitwise NOT of unsigned `a` in width `bits`: `(2^bits - 1) - a`.
pub(super) fn encode_unsigned_bitnot(
    a: usize,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    if bits == 0 || bits > 64 {
        return None;
    }
    // mask = 2^bits - 1 (synthetic 2^64 - 1 for u64)
    let m = if bits == 64 {
        let half = *next;
        *next += 1;
        lines.push(format!("${half} = const 4294967296 : Int"));
        let two64 = *next;
        *next += 1;
        lines.push(format!("${two64} = arith mul ${half} ${half} : Int"));
        let one = *next;
        *next += 1;
        lines.push(format!("${one} = const 1 : Int"));
        let mask = *next;
        *next += 1;
        lines.push(format!("${mask} = arith sub ${two64} ${one} : Int"));
        mask
    } else {
        let mask_v = (1i64 << bits) - 1;
        let mask = *next;
        *next += 1;
        lines.push(format!("${mask} = const {mask_v} : Int"));
        mask
    };
    let slot = *next;
    *next += 1;
    lines.push(format!("${slot} = arith sub ${m} ${a} : Int"));
    Some(slot)
}

/// trailing_ones: trailing_zeros of bitwise NOT (same width).
pub(super) fn encode_unsigned_trailing_ones(
    a: usize,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    let not_a = encode_unsigned_bitnot(a, bits, lines, next)?;
    encode_unsigned_trailing_zeros(not_a, bits, lines, next)
}

/// leading_ones: leading_zeros of bitwise NOT (same width).
pub(super) fn encode_unsigned_leading_ones(
    a: usize,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    let not_a = encode_unsigned_bitnot(a, bits, lines, next)?;
    encode_unsigned_leading_zeros(not_a, bits, lines, next)
}

/// trailing_zeros for unsigned `a` with width `bits`.
/// `sum_i i * bit_i * prod_{j<i}(1-bit_j) + bits * prod_all(1-bit)`.
pub(super) fn encode_unsigned_trailing_zeros(
    a: usize,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    if bits == 0 || bits > 64 {
        return None;
    }
    let two = *next;
    *next += 1;
    lines.push(format!("${two} = const 2 : Int"));
    let one = *next;
    *next += 1;
    lines.push(format!("${one} = const 1 : Int"));
    let zero = *next;
    *next += 1;
    lines.push(format!("${zero} = const 0 : Int"));
    // prod starts at 1 (all lower bits zero so far)
    let mut prod = one;
    let mut acc = zero;
    for i in 0..bits {
        let f = emit_pow2_factor(i, lines, next)?;
        let shifted = *next;
        *next += 1;
        lines.push(format!("${shifted} = arith div ${a} ${f} : Int"));
        let bit = *next;
        *next += 1;
        lines.push(format!("${bit} = arith mod ${shifted} ${two} : Int"));
        // term = i * bit * prod
        let i_c = *next;
        *next += 1;
        lines.push(format!("${i_c} = const {i} : Int"));
        let ib = *next;
        *next += 1;
        lines.push(format!("${ib} = arith mul ${i_c} ${bit} : Int"));
        let term = *next;
        *next += 1;
        lines.push(format!("${term} = arith mul ${ib} ${prod} : Int"));
        let new_acc = *next;
        *next += 1;
        lines.push(format!("${new_acc} = arith add ${acc} ${term} : Int"));
        acc = new_acc;
        // prod *= (1 - bit)
        let one_m_bit = *next;
        *next += 1;
        lines.push(format!("${one_m_bit} = arith sub ${one} ${bit} : Int"));
        let new_prod = *next;
        *next += 1;
        lines.push(format!(
            "${new_prod} = arith mul ${prod} ${one_m_bit} : Int"
        ));
        prod = new_prod;
    }
    // + bits when all zero (prod still 1)
    let bits_c = *next;
    *next += 1;
    lines.push(format!("${bits_c} = const {bits} : Int"));
    let all_zero = *next;
    *next += 1;
    lines.push(format!("${all_zero} = arith mul ${bits_c} ${prod} : Int"));
    let slot = *next;
    *next += 1;
    lines.push(format!("${slot} = arith add ${acc} ${all_zero} : Int"));
    Some(slot)
}

/// leading_zeros for unsigned `a` with width `bits`.
/// Scan high→low: count consecutive zero bits while still in prefix.
pub(super) fn encode_unsigned_leading_zeros(
    a: usize,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    if bits == 0 || bits > 64 {
        return None;
    }
    let two = *next;
    *next += 1;
    lines.push(format!("${two} = const 2 : Int"));
    let one = *next;
    *next += 1;
    lines.push(format!("${one} = const 1 : Int"));
    let zero = *next;
    *next += 1;
    lines.push(format!("${zero} = const 0 : Int"));
    let mut still = one;
    let mut acc = zero;
    for i in (0..bits).rev() {
        let f = emit_pow2_factor(i, lines, next)?;
        let shifted = *next;
        *next += 1;
        lines.push(format!("${shifted} = arith div ${a} ${f} : Int"));
        let bit = *next;
        *next += 1;
        lines.push(format!("${bit} = arith mod ${shifted} ${two} : Int"));
        let zbit = *next;
        *next += 1;
        lines.push(format!("${zbit} = arith sub ${one} ${bit} : Int"));
        let term = *next;
        *next += 1;
        lines.push(format!("${term} = arith mul ${still} ${zbit} : Int"));
        let new_acc = *next;
        *next += 1;
        lines.push(format!("${new_acc} = arith add ${acc} ${term} : Int"));
        acc = new_acc;
        let new_still = *next;
        *next += 1;
        lines.push(format!("${new_still} = arith mul ${still} ${zbit} : Int"));
        still = new_still;
    }
    Some(acc)
}

/// reverse_bits for unsigned `a` with width `bits`: sum_i bit_i * 2^(bits-1-i).
pub(super) fn encode_unsigned_reverse_bits(
    a: usize,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    if bits == 0 || bits > 64 {
        return None;
    }
    let two = *next;
    *next += 1;
    lines.push(format!("${two} = const 2 : Int"));
    let zero = *next;
    *next += 1;
    lines.push(format!("${zero} = const 0 : Int"));
    let mut acc = zero;
    for i in 0..bits {
        let f = emit_pow2_factor(i, lines, next)?;
        let shifted = *next;
        *next += 1;
        lines.push(format!("${shifted} = arith div ${a} ${f} : Int"));
        let bit = *next;
        *next += 1;
        lines.push(format!("${bit} = arith mod ${shifted} ${two} : Int"));
        let rf = emit_pow2_factor(bits - 1 - i, lines, next)?;
        let term = *next;
        *next += 1;
        lines.push(format!("${term} = arith mul ${bit} ${rf} : Int"));
        let new_acc = *next;
        *next += 1;
        lines.push(format!("${new_acc} = arith add ${acc} ${term} : Int"));
        acc = new_acc;
    }
    Some(acc)
}
