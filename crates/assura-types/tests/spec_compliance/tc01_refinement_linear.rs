//! Test Case 1: Refinement + Linear (Ghost Use Problem)
//! Refinement predicates are ghost (logical, not computational).
//! They do NOT count as a linear use.

use super::must_compile;

#[test]
fn ghost_use_compiles() {
    // Refinement use of x is ghost, not computational.
    must_compile(
        r#"
contract RefinementLinearGhost {
    requires(x: Int, y: Int)
    requires(y < x)
    ensures(result: Int)
    ensures(result == x + y)
}
"#,
    );
}

#[test]
fn double_refinement_use() {
    // A variable used twice in refinement predicates is fine (ghost uses).
    must_compile(
        r#"
contract RefinementLinearDoubleUse {
    requires(x: Int, y: Int)
    requires(y < x)
    requires(x > 0)
    ensures(result: Int)
}
"#,
    );
}

#[test]
fn refinement_with_linear_grade() {
    // Contract using linear-grade annotation with refinement.
    must_compile(
        r#"
contract LinearGrade {
    requires(buf: Bytes, len: Nat)
    requires(len > 0)
    ensures(result: Bytes)
}
"#,
    );
}
