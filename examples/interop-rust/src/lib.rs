//! Ordinary Rust library that also carries Assura inline contracts.
//!
//! ```bash
//! cargo test --manifest-path examples/interop-rust/Cargo.toml
//! assura check-rust examples/interop-rust/src
//! ```
//!
//! Proof surface: see docs/CHECK-RUST-SURFACE.md. This is not Verus-depth
//! verification of arbitrary Rust.

/// Clamp to non-negative (proved with check-rust when body is modeled).
///
/// @ensures result >= 0
pub fn clamp0(x: i64) -> i64 {
    if x > 0 { x } else { 0 }
}

/// Increment (proved with check-rust).
///
/// @ensures result == x + 1
pub fn inc(x: i64) -> i64 {
    let y = x + 1;
    y
}

/// Call sites look like normal Rust.
pub fn pipeline(x: i64) -> i64 {
    inc(clamp0(x))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_pipeline() {
        assert_eq!(pipeline(-3), 1);
        assert_eq!(pipeline(4), 5);
    }
}
