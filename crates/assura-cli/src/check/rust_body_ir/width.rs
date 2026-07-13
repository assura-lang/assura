//! Integer width/bounds helpers for body IR encode.
use super::{PARAM_BOUNDS, SAT_BOUNDS, is_identity_peel_method};

/// Full-width unsigned 64-bit (`u64`/`usize`): max does not fit in i64.
/// Sentinel: `(0, -1)` → modulus `2^64` via synthetic `(2^32)*(2^32)`.
pub(super) fn is_u64_width_bounds(lo: i64, hi: i64) -> bool {
    lo == 0 && hi == -1
}

/// Integer range for a Rust primitive type name, or `None` if unsupported.
pub(super) fn rust_int_bounds(ty: &str) -> Option<(i64, i64)> {
    // Strip path prefixes: `std::num::NonZeroU8` → `NonZeroU8`
    let base = ty.rsplit("::").next().unwrap_or(ty).trim();
    match base {
        "i8" => Some((i8::MIN as i64, i8::MAX as i64)),
        "i16" => Some((i16::MIN as i64, i16::MAX as i64)),
        "i32" => Some((i32::MIN as i64, i32::MAX as i64)),
        "i64" | "isize" => Some((i64::MIN, i64::MAX)),
        "u8" => Some((0, u8::MAX as i64)),
        "u16" => Some((0, u16::MAX as i64)),
        "u32" => Some((0, u32::MAX as i64)),
        // u64 max exceeds i64; use sentinel for synthetic 2^64 (#1160)
        "u64" | "usize" => Some((0, -1)),
        // u128/i128 map to Nat/Int without fitting machine bounds; use nonneg / full i64
        // sentinel for divisor peels and nonneg path-param encoding only.
        "u128" => Some((0, -1)),
        "i128" => Some((i64::MIN, i64::MAX)),
        // Positive-only divisors for rem_euclid / div_euclid / next_multiple_of
        "NonZeroU8" => Some((1, u8::MAX as i64)),
        "NonZeroU16" => Some((1, u16::MAX as i64)),
        "NonZeroU32" => Some((1, u32::MAX as i64)),
        // hi unused for divisor path (only lo>=1 gates encode_positive_divisor)
        "NonZeroU64" | "NonZeroUsize" | "NonZeroU128" => Some((1, i64::MAX)),
        _ => None,
    }
}

/// Resolve wrap/shift width: `(bits, Some(modulus)|None for synthetic 2^64, signed)`.
pub(super) fn wrap_width(lo: i64, hi: i64) -> Option<(u32, Option<i64>, bool)> {
    if is_u64_width_bounds(lo, hi) {
        return Some((64, None, false));
    }
    let signed = lo != 0;
    if signed && lo == i64::MIN && hi == i64::MAX {
        return Some((64, None, true));
    }
    let modulus = if signed {
        hi.checked_sub(lo).and_then(|d| d.checked_add(1))?
    } else {
        hi.checked_add(1)?
    };
    if modulus <= 0 || !(modulus as u64).is_power_of_two() {
        return None;
    }
    Some(((modulus as u64).trailing_zeros(), Some(modulus), signed))
}

/// Shared wrap-width slots for rotate emit (keeps `emit_rotl_bits` under clippy's arg cap).
pub(super) struct RotWrap {
    pub(super) bits: u32,
    pub(super) mslot: usize,
    pub(super) signed: bool,
    pub(super) hi: i64,
}

/// Rotate-left `u_in` (unsigned bit pattern in `[0, m)`) by `k_left` bits.
/// Returns the unsigned result in `[0, m)`, or signed reinterpret when `signed`.
pub(super) fn emit_rotl_bits(
    u_in: usize,
    k_left: u32,
    w: &RotWrap,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    if k_left == 0 || k_left >= w.bits {
        // identity (caller should short-circuit 0)
        if !w.signed {
            return Some(u_in);
        }
        let his = *next;
        *next += 1;
        lines.push(format!("${his} = const {} : Int", w.hi));
        let gt = *next;
        *next += 1;
        lines.push(format!("${gt} = cmp gt ${u_in} ${his} : Bool"));
        let adj = *next;
        *next += 1;
        lines.push(format!("${adj} = arith mul ${gt} ${} : Int", w.mslot));
        let slot = *next;
        *next += 1;
        lines.push(format!("${slot} = arith sub ${u_in} ${adj} : Int"));
        return Some(slot);
    }
    let lo_shift = w.bits - k_left;
    let hf = emit_pow2_factor(k_left, lines, next)?;
    let lf = emit_pow2_factor(lo_shift, lines, next)?;
    let hi_part = *next;
    *next += 1;
    lines.push(format!("${hi_part} = arith mul ${u_in} ${hf} : Int"));
    let lo_part = *next;
    *next += 1;
    lines.push(format!("${lo_part} = arith div ${u_in} ${lf} : Int"));
    let raw = *next;
    *next += 1;
    lines.push(format!("${raw} = arith add ${hi_part} ${lo_part} : Int"));
    let t3 = *next;
    *next += 1;
    lines.push(format!("${t3} = arith mod ${raw} ${} : Int", w.mslot));
    let t4 = *next;
    *next += 1;
    lines.push(format!("${t4} = arith add ${t3} ${} : Int", w.mslot));
    let u = *next;
    *next += 1;
    lines.push(format!("${u} = arith mod ${t4} ${} : Int", w.mslot));
    if !w.signed {
        return Some(u);
    }
    let his = *next;
    *next += 1;
    lines.push(format!("${his} = const {} : Int", w.hi));
    let gt = *next;
    *next += 1;
    lines.push(format!("${gt} = cmp gt ${u} ${his} : Bool"));
    let adj = *next;
    *next += 1;
    lines.push(format!("${adj} = arith mul ${gt} ${} : Int", w.mslot));
    let slot = *next;
    *next += 1;
    lines.push(format!("${slot} = arith sub ${u} ${adj} : Int"));
    Some(slot)
}

/// Emit IR for synthetic `2^64 = (2^32)*(2^32)` (does not fit in i64 const).
pub(super) fn emit_synthetic_2_64(lines: &mut Vec<String>, next: &mut usize) -> usize {
    let half = *next;
    *next += 1;
    lines.push(format!("${half} = const 4294967296 : Int"));
    let two64 = *next;
    *next += 1;
    lines.push(format!("${two64} = arith mul ${half} ${half} : Int"));
    two64
}

/// Emit IR for `u64::MAX = 2^64 - 1`.
pub(super) fn emit_u64_max(lines: &mut Vec<String>, next: &mut usize) -> usize {
    let two64 = emit_synthetic_2_64(lines, next);
    let one = *next;
    *next += 1;
    lines.push(format!("${one} = const 1 : Int"));
    let slot = *next;
    *next += 1;
    lines.push(format!("${slot} = arith sub ${two64} ${one} : Int"));
    slot
}

/// Emit IR for `2^e` as an Int slot (`e` in 0..=63).
/// `2^63` does not fit in a positive i64 const; build it as `2^32 * 2^31`.
pub(super) fn emit_pow2_factor(e: u32, lines: &mut Vec<String>, next: &mut usize) -> Option<usize> {
    if e > 63 {
        return None;
    }
    if e < 63 {
        let factor = 1i64 << e;
        let f = *next;
        *next += 1;
        lines.push(format!("${f} = const {factor} : Int"));
        return Some(f);
    }
    let half = *next;
    *next += 1;
    lines.push(format!("${half} = const 4294967296 : Int"));
    let q = *next;
    *next += 1;
    lines.push(format!("${q} = const 2147483648 : Int"));
    let f = *next;
    *next += 1;
    lines.push(format!("${f} = arith mul ${half} ${q} : Int"));
    Some(f)
}

/// Positive power-of-two exponents for a param with known integer bounds.
/// Unsigned: 0..bits (pots 1..2^(bits-1)). Signed: 0..(bits-1) (1..2^(bits-2)).
/// u64/usize: 64 (pots 1..2^63; 2^63 via synthetic product).
pub(super) fn pot_exponents(lo: i64, hi: i64) -> Option<u32> {
    if is_u64_width_bounds(lo, hi) {
        // 64 pot ORs; 2^63 uses emit_pow2_factor (#1173)
        return Some(64);
    }
    if lo == 0 {
        // unsigned: hi+1 must be power of two
        let m = (hi as u64).checked_add(1)?;
        if !m.is_power_of_two() {
            return None;
        }
        Some(m.trailing_zeros())
    } else if lo < 0 && hi > 0 {
        // signed two's complement: hi == 2^(n-1)-1
        let half = (hi as u64).checked_add(1)?;
        if !half.is_power_of_two() {
            return None;
        }
        // positive pots: 1 << e for e in 0..(bits-1)
        Some(half.trailing_zeros())
    } else {
        None
    }
}

/// Bounds for a simple path param (`x`), if registered.
/// Also peels paren/group/ref/deref and identity methods (`clone`, `into`, …)
/// so `x.clone().is_power_of_two()` shares the path-param pot enum path.
pub(super) fn path_param_bounds(expr: &syn::Expr) -> Option<(i64, i64)> {
    let name = match expr {
        syn::Expr::Paren(p) => return path_param_bounds(&p.expr),
        syn::Expr::Group(g) => return path_param_bounds(&g.expr),
        syn::Expr::Reference(r) => return path_param_bounds(&r.expr),
        syn::Expr::Unary(u) if matches!(u.op, syn::UnOp::Deref(_)) => {
            return path_param_bounds(&u.expr);
        }
        syn::Expr::MethodCall(m)
            if m.args.is_empty()
                && (is_identity_peel_method(&m.method.to_string())
                    // NonZero*::get() preserves the path-param integer bounds
                    || m.method == "get") =>
        {
            return path_param_bounds(&m.receiver);
        }
        syn::Expr::Path(p) if p.path.segments.len() == 1 => p.path.segments[0].ident.to_string(),
        _ => return None,
    };
    PARAM_BOUNDS.with(|c| c.borrow().get(&name).copied())
}

/// Width bounds for wrapping/rotate methods: return type first, else receiver.
/// Nested chains like `x.wrapping_shl(1).is_power_of_two()` have bool return
/// (no SAT_BOUNDS) but still need the receiver's fixed width.
pub(super) fn wrap_bounds_for(receiver: &syn::Expr) -> Option<(i64, i64)> {
    SAT_BOUNDS
        .get()
        .or_else(|| path_param_bounds(receiver))
        .or_else(|| expr_int_bounds(receiver))
}

/// Integer bounds for pot/bit-width: path param, or first path found under
/// arith/unary so `(x + 1).is_power_of_two()` inherits `x`'s width (#1034 nested).
pub(super) fn expr_int_bounds(expr: &syn::Expr) -> Option<(i64, i64)> {
    if let Some(b) = path_param_bounds(expr) {
        return Some(b);
    }
    match expr {
        syn::Expr::Paren(p) => expr_int_bounds(&p.expr),
        syn::Expr::Group(g) => expr_int_bounds(&g.expr),
        syn::Expr::Reference(r) => expr_int_bounds(&r.expr),
        syn::Expr::Unary(u) => expr_int_bounds(&u.expr),
        syn::Expr::Binary(b) => expr_int_bounds(&b.left).or_else(|| expr_int_bounds(&b.right)),
        syn::Expr::MethodCall(m) if m.args.is_empty() => expr_int_bounds(&m.receiver),
        syn::Expr::MethodCall(m) if m.args.len() == 1 => {
            expr_int_bounds(&m.receiver).or_else(|| expr_int_bounds(&m.args[0]))
        }
        _ => None,
    }
}

/// Literal integer with known bit width from a typed suffix (`8u32` → (8, 32)).
/// Bare unsuffixed lits return `None` (leading_zeros needs a width).
pub(super) fn lit_int_i64_bits(expr: &syn::Expr) -> Option<(i64, u32)> {
    lit_int_i64_bits_signed(expr).map(|(v, bits, _)| (v, bits))
}

/// Like [`lit_int_i64_bits`] but also reports whether the suffix is signed
/// (`i8`/`i16`/`i32`/`i64`/`isize` → true; `u*`/`usize` → false).
/// Needed for bit-pattern peeps (`reverse_bits`/`swap_bytes`) where
/// `1u8.reverse_bits() == 128` but `1i8.reverse_bits() == -128`.
pub(super) fn lit_int_i64_bits_signed(expr: &syn::Expr) -> Option<(i64, u32, bool)> {
    match expr {
        syn::Expr::Paren(p) => lit_int_i64_bits_signed(&p.expr),
        syn::Expr::Group(g) => lit_int_i64_bits_signed(&g.expr),
        syn::Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => {
            let (v, bits, signed) = lit_int_i64_bits_signed(&u.expr)?;
            Some((v.checked_neg()?, bits, signed))
        }
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(n),
            ..
        }) => {
            let (bits, signed) = match n.suffix() {
                "u8" => (8, false),
                "i8" => (8, true),
                "u16" => (16, false),
                "i16" => (16, true),
                "u32" => (32, false),
                "i32" => (32, true),
                "u64" | "usize" => (64, false),
                "i64" | "isize" => (64, true),
                _ => return None,
            };
            let v: i64 = n.base10_parse().ok()?;
            Some((v, bits, signed))
        }
        _ => None,
    }
}
