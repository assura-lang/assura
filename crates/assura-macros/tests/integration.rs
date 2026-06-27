//! Integration tests for assura-macros proc macros.

use assura_macros::{
    contract, ensures, ensures_err, ensures_ok, invariant, requires, taint, trust,
};

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
#[should_panic(expected = "assura: invariant (entry) failed")]
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

// -- #[taint] attribute tests --

#[taint(secret)]
fn tainted_api_key(key: String) -> String {
    // `key` is now Tainted<String>; must declassify to use
    let raw = key.declassify();
    format!("processed:{}", raw)
}

#[test]
fn taint_declassify_works() {
    let result = tainted_api_key("sk-abc123".to_string());
    assert_eq!(result, "processed:sk-abc123");
}

#[taint(pii)]
fn tainted_validate(email: String) -> bool {
    // Use .validate() to check the tainted value
    email.validate(|e| e.contains('@')).is_some()
}

#[test]
fn taint_validate_pass() {
    assert!(tainted_validate("user@example.com".to_string()));
}

#[test]
fn taint_validate_fail() {
    assert!(!tainted_validate("not-an-email".to_string()));
}

#[taint(api_key)]
fn tainted_debug_format(token: String) -> String {
    // Debug format should show [REDACTED], not the actual value
    format!("{:?}", token)
}

#[test]
fn taint_debug_redacts() {
    let debug_output = tainted_debug_format("super-secret-key".to_string());
    assert!(debug_output.contains("REDACTED"));
    assert!(!debug_output.contains("super-secret-key"));
}

#[taint(secret)]
fn tainted_multiple_params(key: String, token: String) -> String {
    // Both params are tainted
    let k = key.declassify();
    let t = token.declassify();
    format!("{}:{}", k, t)
}

#[test]
fn taint_multiple_params() {
    let result = tainted_multiple_params("key1".to_string(), "tok2".to_string());
    assert_eq!(result, "key1:tok2");
}

#[taint(secret)]
fn tainted_with_map(key: String) -> String {
    // Use .map() to transform while keeping taint
    let upper = key.map(|s| s.to_uppercase());
    upper.declassify()
}

#[test]
fn taint_map_preserves_taint() {
    let result = tainted_with_map("hello".to_string());
    assert_eq!(result, "HELLO");
}

// -- #[contract] with runtime-checks feature (default mode = debug_assert) --
// These tests verify the default mode works. To test runtime-checks mode,
// rebuild with: cargo test -p assura-macros --features runtime-checks

#[contract]
/// @requires x > 0
/// @ensures result > 0
fn contract_runtime_mode(x: i32) -> i32 {
    x * 2
}

#[test]
fn contract_default_mode_works() {
    assert_eq!(contract_runtime_mode(5), 10);
}

#[test]
#[should_panic(expected = "assura: precondition failed")]
fn contract_default_mode_panics() {
    contract_runtime_mode(0);
}

// -- #[ensures_ok] tests --

#[ensures_ok(result > 0)]
fn ok_positive(x: i32) -> Result<i32, String> {
    if x > 0 {
        Ok(x * 2)
    } else {
        Err("non-positive".to_string())
    }
}

#[test]
fn ensures_ok_passes_on_ok() {
    assert_eq!(ok_positive(5).unwrap(), 10);
}

#[test]
fn ensures_ok_skips_on_err() {
    // Err path should NOT trigger the postcondition check
    assert!(ok_positive(-1).is_err());
}

#[test]
#[should_panic(expected = "assura: ensures_ok failed")]
fn ensures_ok_fails_when_violated() {
    // This function always returns Ok(0), which violates result > 0
    #[ensures_ok(result > 0)]
    fn always_zero() -> Result<i32, String> {
        Ok(0)
    }
    let _ = always_zero();
}

// -- #[ensures_err] tests --

#[ensures_err(!result.is_empty())]
fn err_non_empty(x: i32) -> Result<i32, String> {
    if x > 0 {
        Ok(x)
    } else {
        Err("bad value".to_string())
    }
}

#[test]
fn ensures_err_passes_on_err() {
    assert!(err_non_empty(-1).is_err());
}

#[test]
fn ensures_err_skips_on_ok() {
    // Ok path should NOT trigger the postcondition check
    assert_eq!(err_non_empty(5).unwrap(), 5);
}

#[test]
#[should_panic(expected = "assura: ensures_err failed")]
fn ensures_err_fails_when_violated() {
    #[ensures_err(!result.is_empty())]
    fn empty_error() -> Result<i32, String> {
        Err(String::new())
    }
    let _ = empty_error();
}

// Combined ensures_ok + requires
#[requires(x != 0)]
#[ensures_ok(result > 0)]
fn divide_ten(x: i32) -> Result<i32, String> {
    if x < 0 {
        return Err("negative divisor".to_string());
    }
    Ok(10 / x)
}

#[test]
fn ensures_ok_with_requires() {
    assert_eq!(divide_ten(2).unwrap(), 5);
    assert!(divide_ten(-1).is_err());
}

// -- old() expression tests --

#[ensures(result == old(x) + 1)]
fn increment(x: i32) -> i32 {
    x + 1
}

#[test]
fn old_captures_pre_state() {
    assert_eq!(increment(5), 6);
    assert_eq!(increment(0), 1);
}

#[ensures(result >= old(len))]
fn grow(len: usize) -> usize {
    len + 10
}

#[test]
fn old_with_different_types() {
    assert_eq!(grow(5), 15);
}

// old() with ensures_ok
#[ensures_ok(result >= old(min))]
fn parse_with_min(s: &str, min: i32) -> Result<i32, String> {
    let val: i32 = s
        .parse()
        .map_err(|e: std::num::ParseIntError| e.to_string())?;
    if val < min {
        return Err("too small".to_string());
    }
    Ok(val)
}

#[test]
fn old_in_ensures_ok() {
    assert_eq!(parse_with_min("42", 10).unwrap(), 42);
    assert!(parse_with_min("5", 10).is_err()); // Err path skips check
}

// -- #[invariant] on impl blocks --

struct BoundedVec {
    items: Vec<i32>,
    capacity: usize,
}

impl BoundedVec {
    fn new(capacity: usize) -> Self {
        BoundedVec {
            items: Vec::new(),
            capacity,
        }
    }
}

#[invariant(self.items.len() <= self.capacity)]
impl BoundedVec {
    fn push(&mut self, item: i32) {
        if self.items.len() < self.capacity {
            self.items.push(item);
        }
    }

    fn pop(&mut self) -> Option<i32> {
        self.items.pop()
    }

    // &self method should NOT get invariant checks (no mutation)
    fn len(&self) -> usize {
        self.items.len()
    }

    // Static method should NOT get invariant checks
    fn max_capacity() -> usize {
        1024
    }
}

#[test]
fn impl_invariant_passes() {
    let mut v = BoundedVec::new(3);
    v.push(1);
    v.push(2);
    v.push(3);
    assert_eq!(v.len(), 3);
    assert_eq!(v.pop(), Some(3));
}

#[test]
fn impl_invariant_static_method_unaffected() {
    assert_eq!(BoundedVec::max_capacity(), 1024);
}

#[test]
#[should_panic(expected = "assura: invariant (exit) failed")]
fn impl_invariant_detects_violation() {
    struct BadVec {
        items: Vec<i32>,
        capacity: usize,
    }

    impl BadVec {
        fn new(capacity: usize) -> Self {
            BadVec {
                items: Vec::new(),
                capacity,
            }
        }
    }

    #[invariant(self.items.len() <= self.capacity)]
    impl BadVec {
        fn force_push(&mut self, item: i32) {
            // Intentionally violates invariant: pushes past capacity
            self.items.push(item);
        }
    }

    let mut v = BadVec::new(1);
    v.force_push(1); // ok: len=1, cap=1
    v.force_push(2); // panic: len=2 > cap=1
}
