use super::*;

// -----------------------------------------------------------------------
// Field access len >= 0 axiom
// -----------------------------------------------------------------------

#[test]
fn test_field_len_nonneg_axiom() {
    // The encoder should inject `buf.len >= 0` as a background axiom
    // when encoding `.len` field access. This test verifies that
    // a contract using buf.len >= 0 in ensures is verified.
    let src = r#"
        contract LenNonNeg {
            input { buf: List<Int> }
            requires { buf.len > 0 }
            ensures { buf.len >= 0 }
        }
    "#;
    let results = verify_source(src);
    assert!(
        !results.is_empty(),
        "should have at least one verification result"
    );
    assert!(
        matches!(results[0], VerificationResult::Verified { .. }),
        "buf.len >= 0 should verify with non-negativity axiom, got: {:?}",
        results[0]
    );
}

#[test]
fn test_abs_encoding() {
    // abs(x) >= 0 should always verify
    let src = r#"
        contract AbsNonNeg {
            input { x: Int }
            ensures { abs(x) >= 0 }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should produce verification results");
    assert!(
        matches!(results[0], VerificationResult::Verified { .. }),
        "abs(x) >= 0 should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn test_min_max_encoding() {
    // min(a, b) <= max(a, b) should always verify
    let src = r#"
        contract MinLtMax {
            input { a: Int, b: Int }
            ensures { min(a, b) <= max(a, b) }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should produce verification results");
    assert!(
        matches!(results[0], VerificationResult::Verified { .. }),
        "min(a,b) <= max(a,b) should verify, got: {:?}",
        results[0]
    );
}

// -----------------------------------------------------------------------
// Raw token operator aliases and keyword tests
// -----------------------------------------------------------------------

#[test]
fn test_raw_implies_operator() {
    // x > 0 implies x >= 1 should verify (integer domain)
    let src = r#"
        contract ImpliesTest {
            input { x: Int }
            requires { x > 0 }
            ensures { x >= 1 }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(results[0], VerificationResult::Verified { .. }),
        "x > 0 => x >= 1 should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn test_raw_modulo_operator() {
    // x % 2 can be 0 or 1 for non-negative x, so x mod 2 >= 0 should verify
    let src = r#"
        contract ModTest {
            input { x: Int }
            requires { x >= 0 }
            ensures { x mod 2 >= 0 }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(results[0], VerificationResult::Verified { .. }),
        "non-negative modulo should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn test_raw_result_keyword() {
    // result should be accessible in ensures clauses
    let src = r#"
        contract ResultTest {
            input { x: Int }
            output { Int }
            ensures { result >= 0 || result < 0 }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    // result >= 0 || result < 0 is a tautology
    assert!(
        matches!(results[0], VerificationResult::Verified { .. }),
        "result >= 0 || result < 0 should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn test_raw_old_ident() {
    // old(x) in ensures with modifies should be accessible
    let src = r#"
        contract OldRawTest {
            input { x: Int, y: Int }
            modifies { x }
            ensures { old(y) >= 0 || old(y) < 0 }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    // old(y) >= 0 || old(y) < 0 is a tautology
    assert!(
        matches!(results[0], VerificationResult::Verified { .. }),
        "old(y) tautology should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn test_raw_boolean_method_returns_bool() {
    // is_empty() => true or false (tautology), raw tokens should encode as Bool
    let src = r#"
        contract IsEmptyTest {
            input { buf: List<Int> }
            ensures { buf.is_empty() || not buf.is_empty() }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(results[0], VerificationResult::Verified { .. }),
        "is_empty tautology should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn test_raw_contains_returns_bool() {
    // contains(x) => true or false (tautology)
    let src = r#"
        contract ContainsTest {
            input { items: List<Int>, x: Int }
            ensures { items.contains(x) || not items.contains(x) }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(results[0], VerificationResult::Verified { .. }),
        "contains tautology should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn test_index_bounds_axiom() {
    // When we index into an array, the index should have bounds axioms.
    // buf[i] with requires { i >= 0 and i < buf.len() } should be consistent.
    let src = r#"
        contract IndexBounds {
            input { buf: List<Int>, i: Int }
            requires { i >= 0 }
            requires { i < buf.len() }
            ensures { buf[i] >= 0 || buf[i] < 0 }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(results[0], VerificationResult::Verified { .. }),
        "index access tautology should verify, got: {:?}",
        results[0]
    );
}
