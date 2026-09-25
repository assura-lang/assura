//! Test Case 3: Refinement + Dependent (Index Arithmetic)
//! Functions with refined indices and dependent type arithmetic.

use super::must_compile;

#[test]
fn split_at_refined_index() {
    must_compile(
        r#"
contract SplitAt {
    input(n: Nat, i: Nat)
    requires(i <= n)
    output(result: Nat)
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
    input(total: Nat, offset: Nat)
    requires(offset < total)
    output(remaining: Nat)
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
    input(a: Nat, b: Nat)
    requires(a > 0)
    requires(b > 0)
    output(result: Nat)
    ensures(result == a * b)
}
"#,
    );
}
