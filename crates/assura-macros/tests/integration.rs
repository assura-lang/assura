//! Integration tests for assura-macros proc macros.

use assura_macros::{contract, trust};

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
