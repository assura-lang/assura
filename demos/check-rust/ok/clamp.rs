// Demo: branching body with non-neg ensures (modeled if/else).
// Run: assura check-rust demos/check-rust/ok/clamp.rs

/// @ensures result >= 0
pub fn clamp0(x: i64) -> i64 {
    if x > 0 {
        x
    } else {
        0
    }
}
