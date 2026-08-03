// Demo: method body encode (i64::abs).
// Run: assura check-rust demos/check-rust/ok/abs.rs

/// @ensures result >= 0
pub fn abs_i64(x: i64) -> i64 {
    x.abs()
}
