// Demo: intentional Counterexample (else branch returns -1).
// Expect: assura check-rust exits non-zero (errors > 0).
// Run: assura check-rust demos/check-rust/fail/clamp_wrong.rs

/// @ensures result >= 0
pub fn clamp0(x: i64) -> i64 {
    if x > 0 {
        x
    } else {
        -1
    }
}
