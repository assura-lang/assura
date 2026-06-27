use super::*;

#[test]
fn test_trivially_true_ensures() {
    // requires: x > 0, ensures: x > 0 should be Verified
    let src = r#"
        contract TrueEnsures {
            requires: x > 0
            ensures: x > 0
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should have verification results");
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "trivially true ensures should be verified, got: {:?}",
        results[0]
    );
}

#[test]
fn test_false_ensures() {
    // requires: x > 0, ensures: x < 0 should produce a counterexample
    let src = r#"
        contract FalseEnsures {
            requires: x > 0
            ensures: x < 0
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should have verification results");
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "false ensures should produce counterexample, got: {:?}",
        results[0]
    );
}

#[test]
fn test_satisfiable_invariant() {
    // invariant: x > 0 is satisfiable (e.g., x=1)
    let src = r#"
        contract SatInvariant {
            invariant: x > 0
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should have verification results");
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "satisfiable invariant should be verified, got: {:?}",
        results[0]
    );
}

#[test]
fn test_unsatisfiable_invariant() {
    // invariant: x > 0 and x < 0 is unsatisfiable
    let src = r#"
        contract UnsatInvariant {
            invariant: x > 0 and x < 0
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should have verification results");
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "unsatisfiable invariant should produce counterexample, got: {:?}",
        results[0]
    );
}

#[test]
fn test_no_verifiable_clauses() {
    // Only requires, no ensures/invariant: nothing to verify
    let src = r#"
        contract OnlyRequires {
            requires: x > 0
        }
    "#;
    let results = verify_source(src);
    assert!(results.is_empty(), "should have no verification results");
}

#[test]
fn test_arithmetic_ensures() {
    // requires: a > 0 and b > 0, ensures: a + b > 0
    let src = r#"
        contract AddPositive {
            requires: a > 0 and b > 0
            ensures: a + b > 0
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "a>0 and b>0 implies a+b>0, got: {:?}",
        results[0]
    );
}

#[test]
fn test_equality_ensures() {
    // requires: x == 5, ensures: x == 5
    let src = r#"
        contract EqEnsures {
            requires: x == 5
            ensures: x == 5
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "x==5 requires should verify x==5 ensures, got: {:?}",
        results[0]
    );
}

#[test]
fn test_multiple_requires() {
    // Multiple requires act as conjunction
    let src = r#"
        contract MultiReq {
            requires: x >= 0
            requires: x <= 10
            ensures: x >= 0 and x <= 10
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "conjunction of requires should verify, got: {:?}",
        results[0]
    );
}

