//! Integration tests for assura-macros proc macros.

use assura_macros::{contract, ensures, invariant, requires, trust};

// -- #[contract] tests --

#[contract]
/// @requires divisor != 0
/// @ensures result == dividend / divisor
fn safe_divide(dividend: i64, divisor: i64) -> i64 {
    dividend / divisor
}

#[test]
fn contract_debug_assert_precondition() {
    // Valid call should succeed
    assert_eq!(safe_divide(10, 2), 5);
    assert_eq!(safe_divide(100, 10), 10);
    assert_eq!(safe_divide(-6, 3), -2);
}

#[test]
#[should_panic(expected = "assura: precondition failed")]
fn contract_debug_assert_precondition_fails() {
    // divisor == 0 should trigger the precondition assert in debug mode
    safe_divide(10, 0);
}

#[contract]
/// @requires x >= 0
fn non_negative_only(x: i32) -> i32 {
    x + 1
}

#[test]
fn contract_no_ensures_works() {
    assert_eq!(non_negative_only(0), 1);
    assert_eq!(non_negative_only(5), 6);
}

#[test]
#[should_panic(expected = "assura: precondition failed")]
fn contract_no_ensures_fails_precondition() {
    non_negative_only(-1);
}

#[contract]
/// Regular documentation for this function.
///
/// @requires a > 0
/// @requires b > 0
fn both_positive(a: i32, b: i32) -> i32 {
    a + b
}

#[test]
fn contract_multiple_requires() {
    assert_eq!(both_positive(1, 2), 3);
}

#[test]
#[should_panic(expected = "assura: precondition failed")]
fn contract_multiple_requires_first_fails() {
    both_positive(0, 2);
}

#[contract]
/// No contract clauses, just regular docs.
fn no_clauses(x: i32) -> i32 {
    x * 2
}

#[test]
fn contract_no_clauses_passthrough() {
    assert_eq!(no_clauses(5), 10);
}

// -- #[trust] tests --

#[trust("Unit test: verified by manual inspection")]
fn trusted_function(x: i32) -> i32 {
    x * 3
}

#[test]
fn trust_attribute_passthrough() {
    assert_eq!(trusted_function(5), 15);
}

#[trust]
fn trusted_no_reason(x: i32) -> i32 {
    x + 10
}

#[test]
fn trust_no_reason_passthrough() {
    assert_eq!(trusted_no_reason(5), 15);
}

// -- Void function with contract --

#[contract]
/// @requires x > 0
fn void_with_precondition(x: i32) {
    _ = x;
}

#[test]
fn contract_void_function_works() {
    void_with_precondition(1);
}

#[test]
#[should_panic(expected = "assura: precondition failed")]
fn contract_void_function_fails() {
    void_with_precondition(0);
}

// -- Multi-line predicate --

#[contract]
/// @requires
///   x > 0 &&
///   x < 100
fn bounded(x: i32) -> i32 {
    x
}

#[test]
fn contract_multiline_predicate() {
    assert_eq!(bounded(50), 50);
}

#[test]
#[should_panic(expected = "assura: precondition failed")]
fn contract_multiline_predicate_fails() {
    bounded(0);
}

// -- #[requires] attribute syntax tests --

#[requires(divisor != 0)]
fn attr_safe_divide(dividend: i64, divisor: i64) -> i64 {
    dividend / divisor
}

#[test]
fn requires_attr_valid_call() {
    assert_eq!(attr_safe_divide(10, 2), 5);
    assert_eq!(attr_safe_divide(-9, 3), -3);
}

#[test]
#[should_panic(expected = "assura: precondition failed: divisor != 0")]
fn requires_attr_fails_on_zero() {
    attr_safe_divide(10, 0);
}

// Multiple #[requires] stacked on same function
#[requires(a > 0)]
#[requires(b > 0)]
fn attr_both_positive(a: i32, b: i32) -> i32 {
    a + b
}

#[test]
fn requires_attr_multiple_valid() {
    assert_eq!(attr_both_positive(3, 4), 7);
}

#[test]
#[should_panic(expected = "assura: precondition failed")]
fn requires_attr_multiple_first_fails() {
    attr_both_positive(0, 5);
}

#[test]
#[should_panic(expected = "assura: precondition failed")]
fn requires_attr_multiple_second_fails() {
    attr_both_positive(5, 0);
}

// -- #[ensures] attribute syntax tests --

#[ensures(result >= 0)]
fn attr_absolute(x: i32) -> i32 {
    if x < 0 { -x } else { x }
}

#[test]
fn ensures_attr_valid() {
    assert_eq!(attr_absolute(-5), 5);
    assert_eq!(attr_absolute(3), 3);
    assert_eq!(attr_absolute(0), 0);
}

// Combined #[requires] + #[ensures]
#[requires(x >= 0)]
#[requires(x <= max)]
#[ensures(result >= 0)]
#[ensures(result <= max)]
fn attr_clamp(x: i32, max: i32) -> i32 {
    x.min(max).max(0)
}

#[test]
fn requires_ensures_combined() {
    assert_eq!(attr_clamp(5, 10), 5);
    assert_eq!(attr_clamp(0, 10), 0);
    assert_eq!(attr_clamp(10, 10), 10);
}

#[test]
#[should_panic(expected = "assura: precondition failed")]
fn requires_ensures_precondition_fails() {
    attr_clamp(-1, 10);
}

// -- #[invariant] attribute syntax tests --

#[invariant(x > 0)]
fn attr_increment(x: i32) -> i32 {
    x + 1
}

#[test]
fn invariant_attr_checks_entry_and_exit() {
    assert_eq!(attr_increment(5), 6);
}

#[test]
#[should_panic(expected = "assura: invariant violated on entry")]
fn invariant_attr_fails_on_entry() {
    attr_increment(0);
}

// Invariant with `result` - only checks on exit
#[invariant(result >= 0)]
fn attr_abs_invariant(x: i32) -> i32 {
    if x < 0 { -x } else { x }
}

#[test]
fn invariant_attr_with_return() {
    assert_eq!(attr_abs_invariant(-7), 7);
    assert_eq!(attr_abs_invariant(3), 3);
}

// -- async function support --

#[requires(n > 0)]
#[ensures(result == n * 2)]
async fn attr_async_double(n: i32) -> i32 {
    n * 2
}

#[tokio::test]
async fn requires_ensures_async() {
    assert_eq!(attr_async_double(5).await, 10);
}
