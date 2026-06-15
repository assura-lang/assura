//! Test Case 3: Refinement + Dependent (Index Arithmetic)
//! Functions with refined indices and dependent type arithmetic.

use super::must_compile;

#[test]
fn split_at_refined_index() {
    must_compile(
        r#"
contract SplitAt {
    requires(n: Nat, i: Nat)
    requires(i <= n)
    ensures(result: Nat)
    ensures(result == n)
}
"#,
    );
}

#[test]
fn index_arithmetic_with_bounds() {
    must_compile(
        r#"
contract IndexArithmetic {
    requires(total: Nat, offset: Nat)
    requires(offset < total)
    ensures(remaining: Nat)
    ensures(remaining == total - offset)
}
"#,
    );
}

#[test]
fn refined_nat_operations() {
    must_compile(
        r#"
contract RefinedNat {
    requires(a: Nat, b: Nat)
    requires(a > 0)
    requires(b > 0)
    ensures(result: Nat)
    ensures(result == a * b)
}
"#,
    );
}
