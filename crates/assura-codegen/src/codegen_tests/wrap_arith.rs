use super::*;

/// Unbounded `i128` add makes `a + b >= a` a tautology. SMT wrap (#1584)
/// is `u64`/`i64`, so the generated assert must wrap too (#1590).
#[test]
fn same_kind_nat_add_uses_wrapping_not_i128() {
    assert!(
        i128::from(u64::MAX) + i128::from(1u64) >= i128::from(u64::MAX),
        "i128 add of MAX+1 is a tautology"
    );
    assert!(
        !(u64::MAX.wrapping_add(1) >= u64::MAX),
        "u64 wrap of MAX+1 must fail a+b >= a"
    );

    let project = codegen_ok(
        r#"
contract SumNoOverflow {
    input(a: Nat, b: Nat)
    requires { true }
    ensures { a + b >= a }
}
"#,
    );
    let rust = &project.files[0].1;
    assert!(
        rust.contains("wrapping_add"),
        "same-kind Nat + must wrap at 64-bit, got: {rust}"
    );
    assert!(
        !rust.contains("i128::from(a) + i128::from(b)"),
        "same-kind Nat + must not widen add to i128, got: {rust}"
    );
}

#[test]
fn same_kind_int_add_uses_wrapping_not_i128() {
    assert!(
        !(i64::MAX.wrapping_add(1) >= i64::MAX),
        "i64 wrap of MAX+1 must fail a+b >= a"
    );

    let project = codegen_ok(
        r#"
contract IntSumNoOverflow {
    input(a: Int, b: Int)
    requires { true }
    ensures { a + b >= a }
}
"#,
    );
    let rust = &project.files[0].1;
    assert!(
        rust.contains("wrapping_add"),
        "same-kind Int + must wrap at 64-bit, got: {rust}"
    );
    assert!(
        !rust.contains("i128::from(a) + i128::from(b)"),
        "same-kind Int + must not widen add to i128, got: {rust}"
    );
}

#[test]
fn mixed_int_nat_add_still_widens_i128() {
    let project = codegen_ok(
        r#"
contract MixedAdd {
    input(a: Int, b: Nat)
    requires { true }
    ensures { a + b >= a }
}
"#,
    );
    let rust = &project.files[0].1;
    assert!(
        rust.contains("i128::from(a)") && rust.contains("i128::from(b)"),
        "mixed Int/Nat must still widen to i128, got: {rust}"
    );
    assert!(
        !rust.contains("wrapping_add"),
        "mixed Int/Nat must not emit wrapping_add, got: {rust}"
    );
}

#[test]
fn same_kind_nat_sub_mul_use_wrapping() {
    let project = codegen_ok(
        r#"
contract NatSubMul {
    input(a: Nat, b: Nat)
    requires { true }
    ensures { a - b <= a }
    ensures { a * b >= a }
}
"#,
    );
    let rust = &project.files[0].1;
    assert!(
        rust.contains("wrapping_sub"),
        "same-kind Nat - must wrap, got: {rust}"
    );
    assert!(
        rust.contains("wrapping_mul"),
        "same-kind Nat * must wrap, got: {rust}"
    );
    assert!(
        !rust.contains("i128::from(a) - i128::from(b)")
            && !rust.contains("i128::from(a) * i128::from(b)"),
        "same-kind Nat -/* must not widen, got: {rust}"
    );
}

/// `3.wrapping_add(a)` is E0689 (`{integer}` method). Prefer the named Nat.
#[test]
fn wrap_literal_plus_nat_does_not_use_untyped_receiver() {
    let project = codegen_ok(
        r#"
contract LitPlusNat {
    input(a: Nat)
    requires { true }
    ensures { 3 + a >= a }
}
"#,
    );
    let rust = &project.files[0].1;
    assert!(
        !rust.contains("3.wrapping_add"),
        "literal wrapping receiver is E0689, got: {rust}"
    );
    assert!(
        rust.contains("wrapping_add"),
        "3 + a (Nat) must still wrap, got: {rust}"
    );
}

/// Heartbleed-style `3 + payload + padding` must not emit `3.wrapping_add`.
#[test]
fn wrap_heartbleed_like_header_plus_lengths_typed_receiver() {
    let project = codegen_ok(
        r#"
contract HeartbleedLike {
    input(record_length: Nat, payload_length: Nat, padding_length: Nat)
    requires { true }
    ensures { 3 + payload_length + padding_length <= record_length }
}
"#,
    );
    let rust = &project.files[0].1;
    assert!(
        !rust.contains("3.wrapping_add"),
        "header literal must not be wrapping receiver, got: {rust}"
    );
    assert!(
        rust.contains("wrapping_add"),
        "same-kind Nat + must wrap, got: {rust}"
    );
}

/// `record_data.length() - 3 - 16` must not emit `as u64.wrapping_sub`.
#[test]
fn wrap_bytes_length_minus_lits_parenthesizes_as_cast() {
    let project = codegen_ok(
        r#"
contract HeartbleedLen {
    input(record_data: Bytes, declared_payload_len: Nat)
    requires { true }
    ensures { declared_payload_len <= record_data.length() - 3 - 16 }
}
"#,
    );
    let rust = &project.files[0].1;
    assert!(
        !rust.contains("as u64.wrapping_"),
        "unparenthesized as-cast wrapping is a parse error, got: {rust}"
    );
    assert!(
        rust.contains("(record_data.len() as u64).wrapping_sub"),
        "as-cast must be parenthesized before wrapping_sub, got: {rust}"
    );
}
