use super::*;

// -----------------------------------------------------------------------
// T046: MEM.1 Memory region contracts - buffer bounds SMT tests
// -----------------------------------------------------------------------

#[test]
fn test_buffer_bounds_with_requires_verified() {
    // Contract: requires { offset + len <= buf_len }, ensures { offset + len <= buf_len }
    // This should be verified (the requires directly implies the ensures).
    let requires = vec![binop(
        binop(ident("offset"), BinOp::Add, ident("len")),
        BinOp::Lte,
        ident("buf_len"),
    )];
    let ensures = binop(
        binop(ident("offset"), BinOp::Add, ident("len")),
        BinOp::Lte,
        ident("buf_len"),
    );

    let result = super::verify_buffer_bounds(&requires, &ensures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "buffer bounds with matching requires should verify, got: {result:?}"
    );
}

#[test]
fn test_buffer_bounds_without_requires_counterexample() {
    // Contract: no requires, ensures { offset + len <= buf_len }
    // Without bounds check, offset/len are unconstrained -> counterexample.
    let requires: Vec<SpExpr> = vec![];
    let ensures = binop(
        binop(ident("offset"), BinOp::Add, ident("len")),
        BinOp::Lte,
        ident("buf_len"),
    );

    let result = super::verify_buffer_bounds(&requires, &ensures);
    assert!(
        matches!(result, VerificationResult::Counterexample { .. }),
        "buffer bounds without requires should produce counterexample, got: {result:?}"
    );
}

#[test]
fn test_buffer_bounds_partial_requires_counterexample() {
    // requires { offset >= 0 }, ensures { offset + len <= buf_len }
    // offset is bounded below, but len and buf_len are unconstrained.
    let requires = vec![binop(ident("offset"), BinOp::Gte, int_lit(0))];
    let ensures = binop(
        binop(ident("offset"), BinOp::Add, ident("len")),
        BinOp::Lte,
        ident("buf_len"),
    );

    let result = super::verify_buffer_bounds(&requires, &ensures);
    assert!(
        matches!(result, VerificationResult::Counterexample { .. }),
        "partial requires should produce counterexample, got: {result:?}"
    );
}

#[test]
fn test_buffer_bounds_nonneg_offset_and_len() {
    // requires { offset >= 0 and len >= 0 and offset + len <= cap }
    // ensures { offset >= 0 }
    // Should verify: the requires directly constrains offset >= 0.
    let requires = vec![
        binop(ident("offset"), BinOp::Gte, int_lit(0)),
        binop(ident("len"), BinOp::Gte, int_lit(0)),
        binop(
            binop(ident("offset"), BinOp::Add, ident("len")),
            BinOp::Lte,
            ident("cap"),
        ),
    ];
    let ensures = binop(ident("offset"), BinOp::Gte, int_lit(0));

    let result = super::verify_buffer_bounds(&requires, &ensures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "non-negative offset should verify, got: {result:?}"
    );
}

#[test]
fn test_region_containment_sub_within_parent() {
    // Context: cap > 0
    // Sub-region: [2, 5), Parent-region: [0, cap)
    // With cap > 0, and since 2 >= 0 and 5 <= cap needs cap >= 5.
    // Let's use cap >= 5 in context.
    let context = vec![binop(ident("cap"), BinOp::Gte, int_lit(5))];

    let result = super::verify_region_containment(
        &context,
        &int_lit(2),
        &int_lit(5),
        &int_lit(0),
        &ident("cap"),
    );
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "[2,5) subset [0,cap) with cap>=5 should verify, got: {result:?}"
    );
}

#[test]
fn test_region_containment_sub_exceeds_parent() {
    // Sub-region: [0, 10), Parent-region: [0, 5)
    // 10 > 5, so containment fails.
    let context: Vec<SpExpr> = vec![];

    let result = super::verify_region_containment(
        &context,
        &int_lit(0),
        &int_lit(10),
        &int_lit(0),
        &int_lit(5),
    );
    assert!(
        matches!(result, VerificationResult::Counterexample { .. }),
        "[0,10) NOT subset [0,5) should produce counterexample, got: {result:?}"
    );
}

#[test]
fn test_region_containment_same_range() {
    // Sub-region == parent-region: [0, n) subset [0, n) -> Verified
    let context: Vec<SpExpr> = vec![];

    let result = super::verify_region_containment(
        &context,
        &int_lit(0),
        &ident("n"),
        &int_lit(0),
        &ident("n"),
    );
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "[0,n) subset [0,n) should verify, got: {result:?}"
    );
}

#[test]
fn test_region_containment_shifted_sub() {
    // Sub: [start, start+len), Parent: [0, cap)
    // Context: start >= 0 and len >= 0 and start + len <= cap
    // Should verify.
    let context = vec![
        binop(ident("start"), BinOp::Gte, int_lit(0)),
        binop(ident("len"), BinOp::Gte, int_lit(0)),
        binop(
            binop(ident("start"), BinOp::Add, ident("len")),
            BinOp::Lte,
            ident("cap"),
        ),
    ];

    let result = super::verify_region_containment(
        &context,
        &ident("start"),
        &binop(ident("start"), BinOp::Add, ident("len")),
        &int_lit(0),
        &ident("cap"),
    );
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "[start, start+len) subset [0,cap) with bounds should verify, got: {result:?}"
    );
}

#[test]
fn test_safe_buffer_read_contract_verified() {
    // SafeBufferRead: requires { offset + len <= buf_len }, ensures { data_len == len }
    // The ensures does not depend on buf_len, so with requires constraining
    // data_len == len, this verifies.
    let src = r#"
        contract SafeBufferRead {
            requires { offset + len <= buf_len }
            ensures { data_len == len }
        }
    "#;
    let results = verify_source(src);
    // The ensures data_len == len with unconstrained data_len should produce
    // counterexample (data_len is free). This is correct: the contract
    // specifies the property, but without a body binding data_len to len,
    // the verifier correctly reports it cannot prove it.
    assert!(!results.is_empty(), "should have results");
    // At least one result should be a Counterexample (data_len is unconstrained)
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
        "unconstrained data_len should produce counterexample, got: {results:?}"
    );
}

#[test]
fn test_buffer_bounds_contract_ensures_via_requires() {
    // requires { offset + len <= cap and offset >= 0 and len >= 0 }
    // ensures { offset + len <= cap }
    // The ensures is a subset of the requires -> Verified
    let src = r#"
        contract BoundsChecked {
            requires { offset + len <= cap and offset >= 0 and len >= 0 }
            ensures { offset + len <= cap }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "bounds from requires should verify ensures, got: {:?}",
        results[0]
    );
}

#[test]
fn test_unsafe_buffer_read_contract_counterexample() {
    // No requires clause, ensures { offset + len <= buf_len }
    // Without bounds check, this should produce counterexample.
    let src = r#"
        contract UnsafeRead {
            ensures { offset + len <= buf_len }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "missing bounds check should produce counterexample, got: {:?}",
        results[0]
    );
}

#[test]
fn test_nested_region_bounds() {
    // Nested bounds: requires { a >= 0 and b >= a and b <= cap }
    // ensures { a >= 0 and b <= cap }
    // The ensures is a subset of the requires -> Verified
    let src = r#"
        contract NestedBounds {
            requires { a >= 0 and b >= a and b <= cap }
            ensures { a >= 0 and b <= cap }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "nested bounds from requires should verify, got: {:?}",
        results[0]
    );
}

