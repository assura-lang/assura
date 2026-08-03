// Demo: pure let-binding arithmetic (modeled let-inline).
// Run: assura check-rust demos/check-rust/ok/inc.rs

/// @ensures result == x + 1
pub fn inc(x: i64) -> i64 {
    let y = x + 1;
    y
}
