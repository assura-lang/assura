use super::*;

#[test]
fn negative_div_and_mod_match_rust_toward_zero() {
    let src = r#"
        contract NegDiv {
            input(n: Int)
            requires { n == -5 }
            ensures { n / 2 == -2 }
            ensures { n % 2 == -1 }
        }
    "#;
    let results = verify_source(src);
    assert!(results.len() >= 2, "expected both clauses, got {results:?}");
    assert!(
        results
            .iter()
            .all(|r| matches!(r, VerificationResult::Verified { .. })),
        "toward-zero div and rem should verify, got {results:?}"
    );
}

#[test]
fn negative_floor_div_is_rejected() {
    let src = r#"
        contract FloorDiv {
            input(n: Int)
            requires { n == -5 }
            ensures { n / 2 == -3 }
        }
    "#;
    let results = verify_source(src);
    assert!(
        matches!(
            results.first(),
            Some(VerificationResult::Counterexample { .. })
        ),
        "floor quotient must not verify, got {results:?}"
    );
}

#[test]
fn negative_divisor_matches_rust() {
    let src = r#"
        contract NegDenom {
            input(n: Int)
            requires { n == 5 }
            ensures { n / -2 == -2 }
            ensures { n % -2 == 1 }
        }
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .all(|r| matches!(r, VerificationResult::Verified { .. })),
        "negative divisor should truncate toward zero, got {results:?}"
    );
}

#[test]
fn both_negative_matches_rust() {
    let src = r#"
        contract BothNeg {
            input(n: Int)
            requires { n == -5 }
            ensures { n / -2 == 2 }
            ensures { n % -2 == -1 }
        }
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .all(|r| matches!(r, VerificationResult::Verified { .. })),
        "both-negative div and rem should match Rust, got {results:?}"
    );
}

#[test]
fn exact_negative_quotient_is_not_adjusted() {
    let src = r#"
        contract Exact {
            input(n: Int)
            requires { n == -4 }
            ensures { n / 2 == -2 }
        }
    "#;
    let results = verify_source(src);
    assert!(
        matches!(results.first(), Some(VerificationResult::Verified { .. })),
        "exact negative division must not add one, got {results:?}"
    );
}

#[test]
fn positive_div_still_verifies() {
    let src = r#"
        contract PosDiv {
            input(n: Int)
            requires { n == 5 }
            ensures { n / 2 == 2 }
            ensures { n % 2 == 1 }
        }
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .all(|r| matches!(r, VerificationResult::Verified { .. })),
        "positive div and rem should verify, got {results:?}"
    );
}
