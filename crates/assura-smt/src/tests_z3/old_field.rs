use super::*;

// -----------------------------------------------------------------------
// T042: Z3 integration tests with realistic contracts
// -----------------------------------------------------------------------

#[test]
fn test_safe_division_contract() {
    // SafeDivision: requires b != 0, ensures result * b + (a % b) == a
    // Without a body implementation binding result, the verifier treats
    // result as unconstrained, so it correctly finds a counterexample.
    let src = r#"
        contract SafeDivision {
            input(a: Int, b: Int)
            output(result: Int)
            requires: b != 0
            ensures: result * b + (a % b) == a
        }
    "#;
    let results = verify_source(src);
    assert!(
        !results.is_empty(),
        "SafeDivision should produce verification results"
    );
    // Without body binding, result is free -> counterexample expected
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "unbound result should produce counterexample, got: {:?}",
        results[0]
    );
}

#[test]
fn test_safe_division_requires_verified() {
    // With matching requires/ensures (both reference the same variable),
    // the implication holds trivially.
    let src = r#"
        contract DivNonZero {
            requires: b != 0
            ensures: b != 0
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "b != 0 requires should verify b != 0 ensures, got: {:?}",
        results[0]
    );
}

#[test]
fn test_increment_preserves_bound() {
    // If x > 5, then x + 1 > 5 (trivially true in integer arithmetic)
    let src = r#"
        contract IncrBound {
            requires: x > 5
            ensures: x + 1 > 5
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "x > 5 => x + 1 > 5 should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn test_sum_nonnegative() {
    // a >= 0 and b >= 0 implies a + b >= 0
    let src = r#"
        contract SumNonNeg {
            requires: a >= 0
            requires: b >= 0
            ensures: a + b >= 0
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "sum of non-negatives should be non-negative, got: {:?}",
        results[0]
    );
}

#[test]
fn test_counterexample_no_requires() {
    // No requires, ensures x > 0: should produce counterexample (x=0)
    let src = r#"
        contract NoGuard {
            ensures: x > 0
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    match &results[0] {
        VerificationResult::Counterexample { model, .. } => {
            assert!(
                !model.is_empty(),
                "counterexample should have non-empty model"
            );
        }
        other => panic!("expected counterexample, got: {other:?}"),
    }
}

#[test]
fn test_negation_ensures() {
    // requires: x < 0, ensures: -x > 0
    let src = r#"
        contract NegPositive {
            requires: x < 0
            ensures: 0 - x > 0
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "x < 0 => -x > 0 should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn test_invariant_always_true() {
    // invariant: x * x >= 0 -- always true for integers
    let src = r#"
        contract SquareNonNeg {
            invariant: x * x >= 0
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    // Invariant check = satisfiability check, x*x >= 0 is satisfiable
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "x^2 >= 0 invariant should be satisfiable, got: {:?}",
        results[0]
    );
}

#[test]
fn test_e2e_verified_positive_file() {
    let src = assura_test_support::load_fixture("tests/e2e/verified_positive.assura");
    let results = verify_source(&src);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "verified_positive.assura should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn test_e2e_counterexample_file() {
    let src = assura_test_support::load_fixture("tests/e2e/counterexample_simple.assura");
    let results = verify_source(&src);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "counterexample_simple.assura should produce counterexample, got: {:?}",
        results[0]
    );
}

#[test]
fn test_e2e_arithmetic_file() {
    let src = assura_test_support::load_fixture("tests/e2e/verified_arithmetic.assura");
    let results = verify_source(&src);
    // Should have results for both contracts
    assert!(
        results.len() >= 2,
        "should have results for both contracts, got {}",
        results.len()
    );
    for (i, r) in results.iter().enumerate() {
        assert!(
            matches!(r, VerificationResult::Verified { .. }),
            "contract {i} should verify, got: {r:?}"
        );
    }
}

// -----------------------------------------------------------------------
// old(expr.field) encoding
// -----------------------------------------------------------------------

#[test]
fn test_old_unmodified_var_verified() {
    // For an unmodified variable, old(y) == y via frame axiom.
    // requires { y > 0 } modifies { x } ensures { old(y) > 0 }
    // y is NOT modified, so frame axiom asserts y == y__old.
    // requires constrains y > 0, so old(y) > 0 holds.
    let src = r#"
        contract OldUnmod {
            input { x: Int, y: Int }
            modifies { x }
            requires { y > 0 }
            ensures { old(y) > 0 }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should produce verification results");
    assert!(
        matches!(results[0], VerificationResult::Verified { .. }),
        "old(y) > 0 should verify for unmodified y, got: {:?}",
        results[0]
    );
}

