// Demo: straight-line let mut + += (linear SSA fold).
// Run: assura check-rust demos/check-rust/ok/inc_mut.rs

/// @ensures result == x + 1
pub fn inc_mut(x: i64) -> i64 {
    let mut y = x;
    y += 1;
    y
}
