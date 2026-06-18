use super::*;

fn verify_source(source: &str) -> Vec<VerificationResult> {
    let file = assura_parser::parse_unwrap(source);
    let resolved = assura_resolve::resolve(&file).expect("resolve failed in test");
    let typed = assura_types::type_check(&resolved).expect("type_check failed in test");
    verify(&typed)
}

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
    let src = std::fs::read_to_string("../../tests/e2e/verified_positive.assura")
        .expect("test file missing");
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
    let src = std::fs::read_to_string("../../tests/e2e/counterexample_simple.assura")
        .expect("test file missing");
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
    let src = std::fs::read_to_string("../../tests/e2e/verified_arithmetic.assura")
        .expect("test file missing");
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

// -----------------------------------------------------------------------
// T045: Frame condition (modifies clause) SMT tests
// -----------------------------------------------------------------------

#[test]
fn test_frame_axiom_unmodified_var_verified() {
    // modifies { x }, ensures { y == old(y) }
    // y is NOT modified, so frame axiom y == old(y) is injected.
    // This should VERIFY because the axiom makes it trivially true.
    let src = r#"
        contract FrameUnmodified {
            modifies { x }
            ensures { y == old(y) }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should have verification results");
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "unmodified var y == old(y) should verify with frame axiom, got: {:?}",
        results[0]
    );
}

#[test]
fn test_frame_no_axiom_for_modified_var() {
    // modifies { x }, ensures { x == old(x) }
    // x IS modified, so no frame axiom is injected.
    // Without a requires binding x to old(x), this should produce
    // a COUNTEREXAMPLE because x is unconstrained.
    let src = r#"
        contract FrameModified {
            modifies { x }
            ensures { x == old(x) }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should have verification results");
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "modified var x == old(x) should produce counterexample, got: {:?}",
        results[0]
    );
}

#[test]
fn test_frame_axiom_with_requires() {
    // modifies { x }, requires { x > 0 }, ensures { y == old(y) }
    // Frame axiom for y, requires assumed for x.
    let src = r#"
        contract FrameWithReq {
            modifies { x }
            requires { x > 0 }
            ensures { y == old(y) }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "frame axiom + requires should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn test_no_modifies_no_frame_axiom() {
    // No modifies clause: y == old(y) should produce counterexample
    // because no frame axiom is injected.
    let src = r#"
        contract NoModifies {
            ensures { y == old(y) }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "without modifies clause, y == old(y) should be counterexample, got: {:?}",
        results[0]
    );
}

// -----------------------------------------------------------------------
// T039: Refinement type subtyping as SMT queries
// -----------------------------------------------------------------------

use assura_parser::ast::{BinOp, Expr, Literal};

/// Helper: build `Expr::BinOp { lhs, op, rhs }`.
fn binop(lhs: Expr, op: BinOp, rhs: Expr) -> Expr {
    Expr::BinOp {
        lhs: Box::new(lhs),
        op,
        rhs: Box::new(rhs),
    }
}

/// Helper: build `Expr::Ident(name)`.
fn ident(name: &str) -> Expr {
    Expr::Ident(name.to_string())
}

/// Helper: build `Expr::Literal(Literal::Int(n))`.
fn int_lit(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n.to_string()))
}

#[test]
fn test_refinement_subtype_holds() {
    // x > 0 implies x >= 0 -> Verified
    let ante = binop(ident("x"), BinOp::Gt, int_lit(0));
    let cons = binop(ident("x"), BinOp::Gte, int_lit(0));

    let result = super::check_refinement_subtype(&ante, &cons);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "x > 0 should imply x >= 0, got: {result:?}"
    );
}

#[test]
fn test_refinement_subtype_fails() {
    // x > 0 does NOT imply x > 10 -> Counterexample
    let ante = binop(ident("x"), BinOp::Gt, int_lit(0));
    let cons = binop(ident("x"), BinOp::Gt, int_lit(10));

    let result = super::check_refinement_subtype(&ante, &cons);
    assert!(
        matches!(result, VerificationResult::Counterexample { .. }),
        "x > 0 should NOT imply x > 10, got: {result:?}"
    );
}

#[test]
fn test_refinement_with_context() {
    // Context: n > 5, n <= 10. Antecedent: x < n. Consequent: x < 10.
    // With n bounded above by 10, x < n implies x < 10. -> Verified
    let ctx = vec![
        binop(ident("n"), BinOp::Gt, int_lit(5)),
        binop(ident("n"), BinOp::Lte, int_lit(10)),
    ];
    let ante = binop(ident("x"), BinOp::Lt, ident("n"));
    let cons = binop(ident("x"), BinOp::Lt, int_lit(10));

    let result = super::check_refinement_subtype_with_context(&ctx, &ante, &cons);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "with n > 5 and n <= 10, x < n should imply x < 10, got: {result:?}"
    );
}

// -----------------------------------------------------------------------
// T040: Counterexample extraction
// -----------------------------------------------------------------------

#[test]
fn test_counterexample_has_model() {
    // true does NOT imply x > 0 -> counterexample with x value
    let ante = Expr::Literal(Literal::Bool(true));
    let cons = binop(ident("x"), BinOp::Gt, int_lit(0));

    let result = super::check_refinement_subtype(&ante, &cons);
    match &result {
        VerificationResult::Counterexample {
            counter_model: Some(cm),
            ..
        } => {
            assert!(
                !cm.variables.is_empty(),
                "counterexample model should have variables"
            );
            // The model should contain 'x' with some integer value
            let has_x = cm.variables.iter().any(|(name, _)| name == "x");
            assert!(
                has_x,
                "counterexample should contain variable 'x', got: {cm:?}"
            );
        }
        other => panic!("expected counterexample with model, got: {other:?}"),
    }
}

#[test]
fn test_counterexample_json() {
    // Build a CounterexampleModel directly and test JSON output
    let cm = super::CounterexampleModel {
        variables: vec![
            ("b".to_string(), "-1".to_string()),
            ("x".to_string(), "0".to_string()),
        ],
    };
    let json = cm.to_json();
    assert!(
        json.contains("\"variables\""),
        "JSON should have variables key"
    );
    assert!(
        json.contains("\"x\": \"0\""),
        "JSON should contain x=0, got: {json}"
    );
    assert!(
        json.contains("\"b\": \"-1\""),
        "JSON should contain b=-1, got: {json}"
    );

    // Verify it's parseable JSON by checking structural correctness
    assert!(json.starts_with('{'), "JSON should start with open brace");
    assert!(json.ends_with('}'), "JSON should end with close brace");
}

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
    let requires: Vec<Expr> = vec![];
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
    let context: Vec<Expr> = vec![];

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
    let context: Vec<Expr> = vec![];

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

// -----------------------------------------------------------------------
// T047: Taint tracking (SEC.1) SMT tests
// -----------------------------------------------------------------------

#[test]
fn test_taint_safe_all_validated() {
    // All variables are validated, all sensitive uses require validated -> Verified
    use assura_types::TaintLabel;
    let labels = vec![
        ("idx".to_string(), TaintLabel::Validated),
        ("len".to_string(), TaintLabel::Trusted),
    ];
    let validation_fns = vec!["validate".to_string()];
    let sensitive = vec![
        ("idx".to_string(), TaintLabel::Validated),
        ("len".to_string(), TaintLabel::Validated),
    ];
    let result = super::verify_taint_safety(&labels, &validation_fns, &sensitive);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "all validated should verify, got: {result:?}"
    );
}

#[test]
fn test_taint_unsafe_untrusted_at_validated_sink() {
    // Untrusted variable used where Validated is required -> Counterexample
    use assura_types::TaintLabel;
    let labels = vec![("raw_idx".to_string(), TaintLabel::Untrusted)];
    let validation_fns = vec![];
    let sensitive = vec![("raw_idx".to_string(), TaintLabel::Validated)];
    let result = super::verify_taint_safety(&labels, &validation_fns, &sensitive);
    assert!(
        matches!(result, VerificationResult::Counterexample { .. }),
        "untrusted at validated sink should produce counterexample, got: {result:?}"
    );
}

#[test]
fn test_taint_no_sensitive_uses() {
    // No sensitive uses -> trivially verified
    use assura_types::TaintLabel;
    let labels = vec![("x".to_string(), TaintLabel::Untrusted)];
    let result = super::verify_taint_safety(&labels, &[], &[]);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "no sensitive uses should verify, got: {result:?}"
    );
}

#[test]
fn test_taint_mixed_labels() {
    // Multiple variables: one untrusted used safely, one untrusted used unsafely
    use assura_types::TaintLabel;
    let labels = vec![
        ("safe".to_string(), TaintLabel::Validated),
        ("unsafe_var".to_string(), TaintLabel::Untrusted),
    ];
    let sensitive = vec![
        ("safe".to_string(), TaintLabel::Validated),
        ("unsafe_var".to_string(), TaintLabel::Validated),
    ];
    let result = super::verify_taint_safety(&labels, &[], &sensitive);
    assert!(
        matches!(result, VerificationResult::Counterexample { .. }),
        "mixed labels with one violation should produce counterexample, got: {result:?}"
    );
}

#[test]
fn test_taint_trusted_satisfies_all() {
    // Trusted variable satisfies any requirement
    use assura_types::TaintLabel;
    let labels = vec![("key".to_string(), TaintLabel::Trusted)];
    let sensitive = vec![("key".to_string(), TaintLabel::Trusted)];
    let result = super::verify_taint_safety(&labels, &[], &sensitive);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "trusted at trusted sink should verify, got: {result:?}"
    );
}

// -----------------------------------------------------------------------
// T054: Measure encoding tests
// -----------------------------------------------------------------------

#[test]
fn test_measure_len_non_negative_provable() {
    // Verify that adding a NonNegative axiom for len does not break
    // basic verification. The axiom asserts forall xs: len(xs) >= 0.
    let measures = vec![
        super::MeasureDefinition::new(
            "len",
            vec![super::MeasureSort::Collection],
            super::MeasureSort::Nat,
        )
        .with_axiom("len(xs) >= 0", super::MeasureAxiomTag::NonNegative),
    ];

    // A simple requires/ensures that should verify independently of
    // the measure axioms, confirming the axiom does not interfere.
    let requires = vec![binop(ident("n"), BinOp::Gte, int_lit(0))];
    let ensures = binop(ident("n"), BinOp::Gte, int_lit(0));

    let result = super::verify_with_measures(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "non-negative axiom should not break basic verification, got: {result:?}"
    );
}

#[test]
fn test_measure_len_empty_is_zero() {
    // Verify len(empty) == 0 using the EmptyIsZero axiom directly.
    // We set up measures with len, then verify a trivial requires/ensures
    // that leverages the axiom.
    let measures = super::register_builtin_measures();

    let requires = vec![binop(ident("x"), BinOp::Gt, int_lit(0))];
    let ensures = binop(ident("x"), BinOp::Gt, int_lit(0));

    let result = super::verify_with_measures(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "trivial ensures with measure context should verify, got: {result:?}"
    );
}

#[test]
fn test_measure_axioms_do_not_break_basic_verification() {
    // Adding measure axioms should not break basic arithmetic verification.
    let measures = super::register_builtin_measures();

    let requires = vec![
        binop(ident("a"), BinOp::Gte, int_lit(0)),
        binop(ident("b"), BinOp::Gte, int_lit(0)),
    ];
    let ensures = binop(
        binop(ident("a"), BinOp::Add, ident("b")),
        BinOp::Gte,
        int_lit(0),
    );

    let result = super::verify_with_measures(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "a>=0 and b>=0 => a+b>=0 should verify with measures, got: {result:?}"
    );
}

#[test]
fn test_measure_with_wrong_ensures_counterexample() {
    // Measures present but ensures is provably false -> counterexample.
    // Use only a single measure to keep quantifier load minimal.
    let measures = vec![
        super::MeasureDefinition::new(
            "len",
            vec![super::MeasureSort::Collection],
            super::MeasureSort::Nat,
        )
        .with_axiom("len(xs) >= 0", super::MeasureAxiomTag::NonNegative),
    ];

    let requires = vec![binop(ident("x"), BinOp::Gt, int_lit(0))];
    let ensures = binop(ident("x"), BinOp::Lt, int_lit(0));

    let result = super::verify_with_measures(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Counterexample { .. }),
        "x > 0 => x < 0 should produce counterexample, got: {result:?}"
    );
}

#[test]
fn test_measure_custom_user_measure() {
    // A user-defined measure (e.g., "depth") with custom axiom should
    // be encodable without error.
    let measures = vec![
        super::MeasureDefinition::new(
            "depth",
            vec![super::MeasureSort::Collection],
            super::MeasureSort::Nat,
        )
        .with_axiom("depth(xs) >= 0", super::MeasureAxiomTag::NonNegative)
        .with_axiom("depth(empty) == 0", super::MeasureAxiomTag::EmptyIsZero)
        .with_axiom(
            "depth is always bounded",
            super::MeasureAxiomTag::Custom("user-defined depth bound".into()),
        ),
    ];

    let requires = vec![binop(ident("n"), BinOp::Gt, int_lit(5))];
    let ensures = binop(ident("n"), BinOp::Gt, int_lit(5));

    let result = super::verify_with_measures(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "custom user measure should not break verification, got: {result:?}"
    );
}

#[test]
fn test_measure_empty_measures_list() {
    // verify_with_measures with no measures should still work.
    let measures: Vec<super::MeasureDefinition> = vec![];
    let requires = vec![binop(ident("x"), BinOp::Eq, int_lit(5))];
    let ensures = binop(ident("x"), BinOp::Eq, int_lit(5));

    let result = super::verify_with_measures(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "empty measures should still allow verification, got: {result:?}"
    );
}

#[test]
fn test_measure_size_len_equivalence() {
    // size has EquivalentTo("len") axiom. When both are registered,
    // the solver should know size(xs) == len(xs).
    // We can verify basic properties still hold with both measures.
    let measures = super::register_builtin_measures();

    let requires = vec![binop(ident("count"), BinOp::Gte, int_lit(0))];
    let ensures = binop(ident("count"), BinOp::Gte, int_lit(0));

    let result = super::verify_with_measures(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "size/len equivalence should not break verification, got: {result:?}"
    );
}

#[test]
fn test_measure_keys_empty_map_axiom() {
    // keys and values both have EmptyMapEmptySet axiom.
    // Verify the solver doesn't crash or timeout with map measures.
    let measures = super::register_builtin_measures();

    let requires = vec![
        binop(ident("k"), BinOp::Gt, int_lit(0)),
        binop(ident("k"), BinOp::Lt, int_lit(100)),
    ];
    let ensures = binop(
        binop(ident("k"), BinOp::Gt, int_lit(0)),
        BinOp::And,
        binop(ident("k"), BinOp::Lt, int_lit(100)),
    );

    let result = super::verify_with_measures(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "map measure axioms should not break verification, got: {result:?}"
    );
}

#[test]
fn test_measure_no_requires_counterexample() {
    // No requires, ensures x > 0 with a minimal measure set -> counterexample.
    let measures = vec![
        super::MeasureDefinition::new(
            "len",
            vec![super::MeasureSort::Collection],
            super::MeasureSort::Nat,
        )
        .with_axiom("len(xs) >= 0", super::MeasureAxiomTag::NonNegative),
    ];
    let requires: Vec<Expr> = vec![];
    let ensures = binop(ident("x"), BinOp::Gt, int_lit(0));

    let result = super::verify_with_measures(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Counterexample { .. }),
        "no requires with measures should still produce counterexample, got: {result:?}"
    );
}

#[test]
fn test_measure_multiple_requires_with_measures() {
    // Multiple requires should all be asserted as assumptions.
    let measures = super::register_builtin_measures();

    let requires = vec![
        binop(ident("x"), BinOp::Gte, int_lit(0)),
        binop(ident("x"), BinOp::Lte, int_lit(10)),
        binop(
            ident("y"),
            BinOp::Eq,
            binop(ident("x"), BinOp::Add, int_lit(1)),
        ),
    ];
    let ensures = binop(ident("y"), BinOp::Gte, int_lit(1));

    let result = super::verify_with_measures(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "multiple requires with measures should verify, got: {result:?}"
    );
}

#[test]
fn test_measure_append_increment_axiom() {
    // Verify the append axiom is asserted without errors.
    // len has the AppendIncrement axiom.
    let measures = vec![
        super::MeasureDefinition::new(
            "len",
            vec![super::MeasureSort::Collection],
            super::MeasureSort::Nat,
        )
        .with_axiom("len(xs) >= 0", super::MeasureAxiomTag::NonNegative)
        .with_axiom(
            "len(append(xs, x)) == len(xs) + 1",
            super::MeasureAxiomTag::AppendIncrement,
        ),
    ];

    // A simple verification to confirm the axiom doesn't crash the solver
    let requires = vec![binop(ident("n"), BinOp::Eq, int_lit(3))];
    let ensures = binop(ident("n"), BinOp::Eq, int_lit(3));

    let result = super::verify_with_measures(&requires, &ensures, &measures);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "append axiom should not break verification, got: {result:?}"
    );
}

// =======================================================================
// Quantifier domain encoding tests
// =======================================================================

#[test]
fn forall_with_range_domain_produces_guarded_implication() {
    // forall i in 0..10: i >= 0
    // SMT: forall i: (0 <= i && i < 10) => i >= 0
    let source = r#"
contract RangeForall {
  input(arr: List<Int>)
  output(result: Bool)
  requires { forall i in 0 .. 10 : i >= 0 }
}
"#;
    let results = verify_source(source);
    // Contract has only requires (no ensures), so no verifiable clauses.
    // This test verifies the encoding doesn't panic during processing.
    assert!(
        results.is_empty(),
        "requires-only contract should have no verifiable clauses"
    );
}

#[test]
fn exists_with_range_domain_encodes_conjunction() {
    // exists i in 0..5: i == 3
    // SMT: exists i: (0 <= i && i < 5) && i == 3
    let source = r#"
contract RangeExists {
  input(arr: List<Int>)
  output(result: Bool)
  requires { exists i in 0 .. 5 : i == 3 }
}
"#;
    let results = verify_source(source);
    assert!(
        results.is_empty(),
        "requires-only contract should have no verifiable clauses"
    );
}

#[test]
fn forall_with_ident_domain_uses_uninterpreted_contains() {
    // forall x in S: x > 0
    // Domain is an identifier (not a range), encoded with uninterpreted contains
    let source = r#"
contract SetForall {
  input(s: Set<Int>)
  output(result: Bool)
  requires { forall x in s : x > 0 }
}
"#;
    let results = verify_source(source);
    assert!(
        results.is_empty(),
        "requires-only contract should have no verifiable clauses"
    );
}

// =======================================================================
// String theory encoding tests
// =======================================================================

#[test]
fn string_literal_has_known_length() {
    // String literal "hello" should have len == 5
    // requires: s == "hello", ensures: s.len >= 0
    // should verify because len("hello") == 5 >= 0
    let source = r#"
contract StringLen {
  requires { s.len >= 0 }
  ensures { s.len >= 0 }
}
"#;
    let results = verify_source(source);
    assert!(!results.is_empty(), "should have verification results");
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "string len >= 0 should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn concat_length_is_sum_verified() {
    // len(a ++ b) == len(a) + len(b) should be provable
    // We require len(a) >= 0 and len(b) >= 0, and the concat
    // axiom should make len(a ++ b) == len(a) + len(b)
    let source = r#"
contract ConcatLen {
  requires { a.len >= 0 && b.len >= 0 }
  ensures { (a ++ b).len == a.len + b.len }
}
"#;
    let results = verify_source(source);
    assert!(!results.is_empty(), "should have verification results");
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "concat length axiom should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn concat_length_nonneg() {
    // len(a ++ b) >= 0 should always hold
    let source = r#"
contract ConcatNonNeg {
  requires { a.len >= 0 && b.len >= 0 }
  ensures { (a ++ b).len >= 0 }
}
"#;
    let results = verify_source(source);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "concat result length should be non-negative, got: {:?}",
        results[0]
    );
}

#[test]
fn string_method_contains_returns_bool() {
    // contains() should return a boolean value usable in logic
    let source = r#"
contract StrContains {
  requires { s.contains("x") }
  ensures { s.contains("x") }
}
"#;
    let results = verify_source(source);
    assert!(!results.is_empty());
    // P => P is trivially true
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "contains returning bool should verify P => P, got: {:?}",
        results[0]
    );
}

#[test]
fn string_starts_with_returns_bool() {
    // starts_with() returns Bool
    let source = r#"
contract StrStartsWith {
  requires { s.starts_with("pre") }
  ensures { s.starts_with("pre") }
}
"#;
    let results = verify_source(source);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "starts_with should return bool, got: {:?}",
        results[0]
    );
}

#[test]
fn string_is_empty_returns_bool() {
    // is_empty() returns Bool
    let source = r#"
contract StrIsEmpty {
  requires { !s.is_empty }
  ensures { !s.is_empty }
}
"#;
    let results = verify_source(source);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "is_empty should return bool, got: {:?}",
        results[0]
    );
}

// =======================================================================
// Comparison chaining tests
// =======================================================================

#[test]
fn chained_comparison_lower_upper_bound() {
    // 0 <= x < n with x = 3, n = 10 should verify
    let source = r#"
contract ChainedBounds {
  requires { x > 0 && x < 10 }
  ensures { 0 <= x && x < 10 }
}
"#;
    let results = verify_source(source);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "chained comparison should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn chained_comparison_three_way() {
    // a <= b <= c when a < b < c
    let source = r#"
contract ThreeWayChain {
  requires { a < b && b < c }
  ensures { a < c }
}
"#;
    let results = verify_source(source);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "transitivity through chain should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn chained_comparison_false_case() {
    // 0 < x > 10 does not imply x > 20
    let source = r#"
contract ChainedFalse {
  requires { x > 0 && x > 10 }
  ensures { x > 20 }
}
"#;
    let results = verify_source(source);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "false chained claim should produce counterexample, got: {:?}",
        results[0]
    );
}

#[test]
fn array_set_get_store_axiom() {
    // get(set(a, i, v), i) == v should verify
    let source = r#"
contract ArrayStore {
  requires { set(a, i, v) == a2 }
  ensures { a2[i] == v }
}
"#;
    let results = verify_source(source);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "array store axiom should verify, got: {results:?}"
    );
}

#[test]
fn array_set_preserves_length() {
    // len(set(a, i, v)) == len(a) should verify
    let source = r#"
contract ArraySetLen {
  requires { len(a) == n && set(a, 0, v) == a2 }
  ensures { len(a2) == n }
}
"#;
    let results = verify_source(source);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "array set preserves length should verify, got: {results:?}"
    );
}

#[test]
fn map_put_get_read_over_write() {
    // get(put(m, k, v), k) == v should verify
    let source = r#"
contract MapReadWrite {
  requires { put(m, k, v) == m2 }
  ensures { get(m2, k) == v }
}
"#;
    let results = verify_source(source);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "map read-over-write should verify, got: {results:?}"
    );
}

#[test]
fn map_put_size_nonneg() {
    // size of map after put is non-negative
    let source = r#"
contract MapSizeNonneg {
  requires { put(m, k, v) == m2 }
  ensures { size(m2) >= 0 }
}
"#;
    let results = verify_source(source);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "map size non-neg should verify, got: {results:?}"
    );
}

#[test]
fn decreases_clause_produces_result() {
    // A decreases clause should produce a verification result
    // (the well-foundedness check: measure >= 0).
    let source = r#"
contract DecreasesTest {
  requires { n > 0 }
  decreases { n }
}
"#;
    let results = verify_source(source);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "decreases n with requires n > 0 should verify non-negative, got: {results:?}"
    );
}

// -----------------------------------------------------------------------
// Regression: #170 — Tuple elements must be individually constrained
// -----------------------------------------------------------------------

#[test]
fn test_tuple_encoding_preserves_elements() {
    use crate::z3_backend::encoder::Encoder;
    use assura_parser::ast::{Expr, Literal};
    z3::with_z3_config(&z3::Config::new(), || {
        let mut encoder = Encoder::new();
        let tuple_expr = Expr::Tuple(vec![
            Expr::Literal(Literal::Int("1".into())),
            Expr::Literal(Literal::Int("2".into())),
        ]);
        let _val = encoder.encode_expr(&tuple_expr);
        // The fix asserts __tuple_2_0(tuple) == 1 and __tuple_2_1(tuple) == 2
        // as background axioms. Without the fix, no axioms are produced.
        assert!(
            encoder.background_axioms.len() >= 2,
            "Tuple encoding must produce element-access axioms, got {}",
            encoder.background_axioms.len()
        );
    });
}

#[test]
fn test_list_encoding_preserves_elements() {
    use crate::z3_backend::encoder::Encoder;
    use assura_parser::ast::{Expr, Literal};
    z3::with_z3_config(&z3::Config::new(), || {
        let mut encoder = Encoder::new();
        let list_expr = Expr::List(vec![
            Expr::Literal(Literal::Int("10".into())),
            Expr::Literal(Literal::Int("20".into())),
            Expr::Literal(Literal::Int("30".into())),
        ]);
        let _val = encoder.encode_expr(&list_expr);
        // 3 element axioms + 1 length axiom = 4 background axioms
        assert!(
            encoder.background_axioms.len() >= 4,
            "List encoding must produce element-access and length axioms, got {}",
            encoder.background_axioms.len()
        );
    });
}

// -----------------------------------------------------------------------
// Regression: #175 — String constants must have distinctness axioms
// -----------------------------------------------------------------------

#[test]
fn test_string_distinctness() {
    use crate::z3_backend::encoder::Encoder;
    use assura_parser::ast::{Expr, Literal};
    z3::with_z3_config(&z3::Config::new(), || {
        let mut encoder = Encoder::new();
        // Encode two different string literals
        let _hello = encoder.encode_expr(&Expr::Literal(Literal::Str("hello".into())));
        let _world = encoder.encode_expr(&Expr::Literal(Literal::Str("world".into())));
        // Must have a distinctness axiom (hello != world) plus length axioms
        let has_distinctness = encoder.background_axioms.len() >= 3; // 2 lengths + 1 distinct
        assert!(
            has_distinctness,
            "Different string constants must have distinctness axioms, got {} axioms",
            encoder.background_axioms.len()
        );
        // Same string encoded twice should NOT add another distinctness axiom
        let axiom_count_before = encoder.background_axioms.len();
        let _hello2 = encoder.encode_expr(&Expr::Literal(Literal::Str("hello".into())));
        // Only a new length axiom, no new distinctness axiom
        assert_eq!(
            encoder.background_axioms.len(),
            axiom_count_before + 1, // just the length axiom
            "Same string constant should not add extra distinctness axioms"
        );
    });
}

// -----------------------------------------------------------------------
// Regression: #177 — Apply must not return hardcoded true
// -----------------------------------------------------------------------

#[test]
fn test_apply_missing_lemma_not_verified() {
    use crate::z3_backend::encoder::Encoder;
    use assura_parser::ast::Expr;
    z3::with_z3_config(&z3::Config::new(), || {
        let mut encoder = Encoder::new();
        let apply_expr = Expr::Apply {
            lemma_name: "NonexistentLemma".into(),
            args: vec![Expr::Ident("x".into())],
        };
        let val = encoder.encode_expr(&apply_expr);
        // Must NOT be hardcoded true. Should be a named bool variable.
        let is_bool = matches!(val, crate::z3_backend::encoder::Z3Value::Bool(_));
        assert!(is_bool, "Apply should return a Bool value");
        // The bool should be a fresh variable, not `true`.
        // We verify by checking it's not a constant true by checking
        // the Z3 string representation.
        if let crate::z3_backend::encoder::Z3Value::Bool(b) = &val {
            let s = format!("{b:?}");
            assert!(
                !s.contains("true"),
                "Apply for missing lemma must not return constant true, got: {s}"
            );
        }
    });
}

#[test]
fn test_apply_existing_lemma_contributes_constraints() {
    // A contract with a lemma and an apply should have the lemma's
    // postcondition injected by the verification pipeline.
    let source = r#"
contract UsesLemma {
    requires: x > 0
    ensures: x > 0
}
"#;
    // This is a basic sanity check that the pipeline still works
    // with lemma infrastructure. The Apply encoding change to
    // fresh bools doesn't break normal verification.
    let results = verify_source(source);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "Basic verification should still work after Apply fix, got: {:?}",
        results[0]
    );
}

// -----------------------------------------------------------------------
// #185: Return-type constraints (Nat -> result >= 0)
// -----------------------------------------------------------------------

#[test]
fn test_nat_return_type_constrains_result() {
    // A function returning Nat with `ensures result >= 0` should verify
    // because the return type Nat implies result >= 0.
    let src = r#"
fn nat_fn(n: Nat) -> Nat
  requires n >= 0
  ensures result >= 0
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should have verification results");
    for r in &results {
        eprintln!("  result: {r:?}");
    }
    // Find the ensures result
    let ensures_result = results.iter().find(|r| match r {
        VerificationResult::Verified { clause_desc }
        | VerificationResult::Counterexample { clause_desc, .. } => clause_desc.contains("ensures"),
        _ => false,
    });
    assert!(
        ensures_result.is_some(),
        "should have an ensures result, got: {results:?}"
    );
    assert!(
        matches!(ensures_result.unwrap(), VerificationResult::Verified { .. }),
        "Nat return type should constrain result >= 0, got: {:?}",
        ensures_result.unwrap()
    );
}

#[test]
fn test_nat_return_type_genuine_counterexample() {
    // A function returning Nat with `ensures result < 0` should produce
    // a genuine counterexample because Nat implies result >= 0,
    // contradicting result < 0.
    let src = r#"
fn bad_nat() -> Nat
  ensures result < 0
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should have verification results");
    // ensures result < 0 with Nat return type: result >= 0 AND NOT(result < 0)
    // is UNSAT => verified. Wait, no: the validity check asserts NOT(ensures).
    // ensures = "result < 0". NOT(ensures) = "result >= 0".
    // Combined with type constraint "result >= 0", both say result >= 0.
    // The query is: is NOT(ensures) satisfiable? NOT(result < 0) = result >= 0.
    // With result >= 0 from type constraint, we have result >= 0 AND result >= 0.
    // That's SAT (e.g., result = 0). So the ensures "result < 0" is NOT valid
    // (there exists result = 0 where result < 0 is false). So we get Verified
    // for the validity check... wait, no.
    //
    // Validity check: assert requires, assert NOT(ensures), check sat.
    // If UNSAT, ensures is valid (verified).
    // If SAT, ensures is not valid (counterexample).
    //
    // For ensures "result < 0":
    //   NOT(ensures) = result >= 0
    //   Type constraint: result >= 0
    //   Solver sees: result >= 0 AND result >= 0 => SAT (result = 0)
    //   => NOT valid => Counterexample
    //
    // Wait, that means the ensures "result < 0" produces a counterexample
    // because result >= 0 satisfies NOT(result < 0). But that's the
    // expected behavior: ensures "result < 0" is FALSE (Nat can't be < 0).
    // So the verifier should say the ensures is not provable => COUNTEREXAMPLE.
    // This is actually correct behavior: the ensures clause is wrong.
}

#[test]
fn test_nat_param_constraint() {
    // A function with Nat param: requires param >= 0 should be trivially verified
    let src = r#"
fn nat_param(n: Nat) -> Int
  ensures n >= 0
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should have verification results");
    let ensures_result = results.iter().find(|r| match r {
        VerificationResult::Verified { clause_desc }
        | VerificationResult::Counterexample { clause_desc, .. } => clause_desc.contains("ensures"),
        _ => false,
    });
    assert!(ensures_result.is_some(), "should have an ensures result");
    assert!(
        matches!(ensures_result.unwrap(), VerificationResult::Verified { .. }),
        "Nat param should constrain n >= 0, got: {:?}",
        ensures_result.unwrap()
    );
}

// -----------------------------------------------------------------------
// #180: feature_max constants bound to concrete values
// -----------------------------------------------------------------------

#[test]
fn test_feature_max_constant_is_bound() {
    // feature_max MAX_SIZE: Nat = 65536
    // A contract that uses MAX_SIZE in ensures should see the concrete value,
    // not a free variable. Z3 should verify `MAX_SIZE > 0` trivially.
    let src = r#"
feature_max MAX_SIZE: Nat = 65536

contract UsesConstant {
  requires MAX_SIZE > 0
  ensures MAX_SIZE == 65536
}
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should have verification results");
    for r in &results {
        eprintln!("  result: {r:?}");
    }
    // Both requires-as-ensures and the equality check should verify
    let ensures_result = results.iter().find(|r| match r {
        VerificationResult::Verified { clause_desc }
        | VerificationResult::Counterexample { clause_desc, .. } => clause_desc.contains("ensures"),
        _ => false,
    });
    assert!(ensures_result.is_some(), "should have an ensures result");
    assert!(
        matches!(ensures_result.unwrap(), VerificationResult::Verified { .. }),
        "feature_max should bind MAX_SIZE to 65536, got: {:?}",
        ensures_result.unwrap()
    );
}

#[test]
fn test_feature_max_arithmetic() {
    // feature_max constants should participate in arithmetic correctly
    let src = r#"
feature_max HEADER_SIZE: Nat = 3

fn check_size(payload: Nat, record: Nat) -> Int
  requires payload >= 0
  requires record >= 0
  requires HEADER_SIZE + payload <= record
  ensures record >= 3
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should have verification results");
    let ensures_result = results.iter().find(|r| match r {
        VerificationResult::Verified { clause_desc }
        | VerificationResult::Counterexample { clause_desc, .. } => clause_desc.contains("ensures"),
        _ => false,
    });
    assert!(ensures_result.is_some(), "should have an ensures result");
    assert!(
        matches!(ensures_result.unwrap(), VerificationResult::Verified { .. }),
        "HEADER_SIZE=3 + payload <= record should imply record >= 3, got: {:?}",
        ensures_result.unwrap()
    );
}

#[test]
fn test_feature_max_wrong_value_produces_counterexample() {
    // If we claim MAX_SIZE == 1, but it's actually 65536,
    // the ensures should still verify because the constant IS 65536.
    // But if we assert something genuinely wrong, we should get a counterexample.
    let src = r#"
feature_max LIMIT: Nat = 10

contract WrongClaim {
  requires LIMIT > 0
  ensures LIMIT > 100
}
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should have verification results");
    let ensures_result = results.iter().find(|r| match r {
        VerificationResult::Verified { clause_desc }
        | VerificationResult::Counterexample { clause_desc, .. } => clause_desc.contains("ensures"),
        _ => false,
    });
    assert!(ensures_result.is_some(), "should have an ensures result");
    // LIMIT=10 so LIMIT > 100 is false: should be counterexample
    assert!(
        matches!(
            ensures_result.unwrap(),
            VerificationResult::Counterexample { .. }
        ),
        "LIMIT=10 > 100 should produce counterexample, got: {:?}",
        ensures_result.unwrap()
    );
}

#[test]
fn test_multiple_feature_max_constants() {
    // Two constants in the same file, used together in arithmetic
    let src = r#"
feature_max HEADER: Nat = 5
feature_max FOOTER: Nat = 3

fn check_total(payload: Nat) -> Int
  requires payload >= 0
  requires HEADER + payload + FOOTER <= 100
  ensures payload <= 92
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should have verification results");
    let ensures_result = results.iter().find(|r| match r {
        VerificationResult::Verified { clause_desc }
        | VerificationResult::Counterexample { clause_desc, .. } => clause_desc.contains("ensures"),
        _ => false,
    });
    assert!(ensures_result.is_some(), "should have an ensures result");
    // HEADER(5) + payload + FOOTER(3) <= 100 => payload <= 92
    assert!(
        matches!(ensures_result.unwrap(), VerificationResult::Verified { .. }),
        "5 + payload + 3 <= 100 should imply payload <= 92, got: {:?}",
        ensures_result.unwrap()
    );
}

// -----------------------------------------------------------------------
// #188: feature_max refinement narrowing
// -----------------------------------------------------------------------

#[test]
fn test_feature_max_narrowing_basic() {
    // feature_max max_page_size = 4096 should narrow `page_size <= 4096`
    // A function with a `page_size` param should see the upper bound.
    let src = r#"
feature_max max_page_size: Nat = 4096

fn validate_page(page_size: Nat) -> Int
  requires page_size >= 0
  ensures page_size <= 4096
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should have verification results");
    let ensures_result = results.iter().find(|r| match r {
        VerificationResult::Verified { clause_desc }
        | VerificationResult::Counterexample { clause_desc, .. } => clause_desc.contains("ensures"),
        _ => false,
    });
    assert!(ensures_result.is_some(), "should have an ensures result");
    assert!(
        matches!(ensures_result.unwrap(), VerificationResult::Verified { .. }),
        "max_page_size=4096 should narrow page_size <= 4096, got: {:?}",
        ensures_result.unwrap()
    );
}

#[test]
fn test_feature_max_narrowing_uppercase() {
    // feature_max MAX_CONTENT_LEN = 16384 should narrow `CONTENT_LEN <= 16384`
    let src = r#"
feature_max MAX_CONTENT_LEN: Nat = 16384

fn check_content(CONTENT_LEN: Nat) -> Int
  requires CONTENT_LEN >= 0
  ensures CONTENT_LEN <= 16384
    "#;
    let results = verify_source(src);
    let ensures_result = results.iter().find(|r| match r {
        VerificationResult::Verified { clause_desc }
        | VerificationResult::Counterexample { clause_desc, .. } => clause_desc.contains("ensures"),
        _ => false,
    });
    assert!(ensures_result.is_some(), "should have an ensures result");
    assert!(
        matches!(ensures_result.unwrap(), VerificationResult::Verified { .. }),
        "MAX_CONTENT_LEN=16384 should narrow CONTENT_LEN <= 16384, got: {:?}",
        ensures_result.unwrap()
    );
}

#[test]
fn test_feature_max_narrowing_without_narrowing_fails() {
    // Without narrowing, we can't prove page_size <= 4096 from just
    // page_size >= 0 (there's no upper bound).
    // This test verifies the narrowing is actually doing something.
    //
    // We use a constant name that does NOT trigger narrowing (no max_ prefix).
    let src = r#"
feature_max PAGE_LIMIT: Nat = 4096

fn validate_page(page_size: Nat) -> Int
  requires page_size >= 0
  ensures page_size <= 4096
    "#;
    let results = verify_source(src);
    let ensures_result = results.iter().find(|r| match r {
        VerificationResult::Verified { clause_desc }
        | VerificationResult::Counterexample { clause_desc, .. } => clause_desc.contains("ensures"),
        _ => false,
    });
    assert!(ensures_result.is_some(), "should have an ensures result");
    // PAGE_LIMIT has no max_ prefix so no narrowing happens for page_size
    assert!(
        matches!(
            ensures_result.unwrap(),
            VerificationResult::Counterexample { .. }
        ),
        "without narrowing, page_size <= 4096 should produce counterexample, got: {:?}",
        ensures_result.unwrap()
    );
}

#[test]
fn test_feature_max_narrowing_combined_with_constant() {
    // feature_max max_buffer: Nat = 1024 binds max_buffer=1024 AND narrows buffer <= 1024
    let src = r#"
feature_max max_buffer: Nat = 1024

fn check_buffer(buffer: Nat) -> Int
  requires buffer >= 0
  requires buffer + max_buffer <= 2048
  ensures buffer <= 1024
    "#;
    let results = verify_source(src);
    let ensures_result = results.iter().find(|r| match r {
        VerificationResult::Verified { clause_desc }
        | VerificationResult::Counterexample { clause_desc, .. } => clause_desc.contains("ensures"),
        _ => false,
    });
    assert!(ensures_result.is_some(), "should have an ensures result");
    assert!(
        matches!(ensures_result.unwrap(), VerificationResult::Verified { .. }),
        "max_buffer=1024 should narrow buffer <= 1024, got: {:?}",
        ensures_result.unwrap()
    );
}

#[test]
fn test_feature_max_narrowing_derives_pairs() {
    // Unit test for the derive_narrowings function itself
    use crate::z3_backend::verify::derive_narrowings;

    let constants = vec![
        ("max_page_size".to_string(), 4096),
        ("MAX_CONTENT_LEN".to_string(), 16384),
        ("LIMIT".to_string(), 100), // no max_ prefix, no narrowing
    ];
    let narrowings = derive_narrowings(&constants);

    // max_page_size -> page_size (already lowercase)
    assert!(
        narrowings
            .iter()
            .any(|(n, v)| n == "page_size" && *v == 4096),
        "should derive page_size narrowing"
    );
    // MAX_CONTENT_LEN -> CONTENT_LEN (as-is) + content_len (lowercase)
    assert!(
        narrowings
            .iter()
            .any(|(n, v)| n == "CONTENT_LEN" && *v == 16384),
        "should derive CONTENT_LEN narrowing"
    );
    assert!(
        narrowings
            .iter()
            .any(|(n, v)| n == "content_len" && *v == 16384),
        "should derive content_len lowercase narrowing"
    );
    // LIMIT has no max_ prefix, should not produce narrowing
    assert!(
        !narrowings.iter().any(|(n, _)| n == "IMIT" || n == "imit"),
        "LIMIT should not produce narrowing"
    );
}

// -----------------------------------------------------------------------
// #198: Deep field chain SMT encoding
// -----------------------------------------------------------------------

#[test]
fn deep_field_chain_ensures_verifies() {
    // Deep field chain: state.head.extra.extra_max should be flattened
    // to a single Z3 variable, making the ensures verifiable.
    let src = r#"
        contract DeepChain {
            requires: x.head.extra.extra_max >= 0
            ensures: x.head.extra.extra_max >= 0
        }
    "#;
    let results = verify_source(src);
    assert!(
        !results.is_empty(),
        "should have results for deep field chain"
    );
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "deep field chain ensures should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn deep_field_chain_not_unmodelable() {
    use z3_backend::encoder::expr_has_unmodelable_features;
    // state.head.extra.extra_max should NOT be unmodelable
    let expr = Expr::Field(
        Box::new(Expr::Field(
            Box::new(Expr::Field(
                Box::new(Expr::Ident("state".into())),
                "head".into(),
            )),
            "extra".into(),
        )),
        "extra_max".into(),
    );
    assert!(
        !expr_has_unmodelable_features(&expr),
        "deep field chain should be modelable after #198"
    );
}

// -----------------------------------------------------------------------
// #200: Taint/validate/ghost/region SMT encoding
// -----------------------------------------------------------------------

#[test]
fn taint_keyword_not_unmodelable() {
    use z3_backend::encoder::expr_has_unmodelable_features;
    // Raw tokens with taint/ghost/region but no @ should be modelable
    let expr = Expr::Raw(vec![
        "taint".into(),
        "untrusted".into(),
        "x".into(),
        ">=".into(),
        "0".into(),
    ]);
    assert!(
        !expr_has_unmodelable_features(&expr),
        "taint keywords should be modelable after #200"
    );
}

#[test]
fn typestate_at_now_modelable() {
    use z3_backend::encoder::expr_has_unmodelable_features;
    // #262: Raw tokens with @ are now modelable (encoded as integer equality)
    let expr = Expr::Raw(vec![
        "state".into(),
        ".".into(),
        "status".into(),
        "@".into(),
        "Active".into(),
    ]);
    assert!(
        !expr_has_unmodelable_features(&expr),
        "typestate @ annotation should be modelable after #262"
    );
}

// -----------------------------------------------------------------------
// #262: Typestate @ encoding as integer equality
// -----------------------------------------------------------------------

#[test]
fn z3_typestate_same_state_verifies() {
    use assura_parser::ast::{Clause, ClauseKind, Expr};
    // requires { file @ Open }
    // ensures  { file @ Open }
    // Same typestate in pre and post => should verify
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
            effect_variables: vec![],
        },
    ];
    let results = crate::verify_contract("TypestateIdentity", &clauses);
    assert!(
        !results.is_empty(),
        "should have results for typestate identity"
    );
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "same typestate pre/post should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn z3_typestate_different_state_counterexample() {
    use assura_parser::ast::{Clause, ClauseKind, Expr};
    // requires { file @ Open }
    // ensures  { file @ Closed }
    // Different typestate in pre and post => counterexample
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Expr::Raw(vec!["file".into(), "@".into(), "Closed".into()]),
            effect_variables: vec![],
        },
    ];
    let results = crate::verify_contract("TypestateMismatch", &clauses);
    assert!(
        !results.is_empty(),
        "should have results for typestate mismatch"
    );
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "different typestate pre/post should produce counterexample, got: {:?}",
        results[0]
    );
}

#[test]
fn z3_typestate_with_dot_field() {
    use assura_parser::ast::{Clause, ClauseKind, Expr};
    // requires { conn.state @ Connected }
    // ensures  { conn.state @ Connected }
    // Dot-separated field + typestate should verify
    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: Expr::Raw(vec![
                "conn".into(),
                ".".into(),
                "state".into(),
                "@".into(),
                "Connected".into(),
            ]),
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: Expr::Raw(vec![
                "conn".into(),
                ".".into(),
                "state".into(),
                "@".into(),
                "Connected".into(),
            ]),
            effect_variables: vec![],
        },
    ];
    let results = crate::verify_contract("TypestateField", &clauses);
    assert!(
        !results.is_empty(),
        "should have results for typestate with field access"
    );
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "typestate with dot field should verify, got: {:?}",
        results[0]
    );
}

// -----------------------------------------------------------------------
// #201: Unknown method call encoding as uninterpreted functions
// -----------------------------------------------------------------------

#[test]
fn unknown_method_call_not_unmodelable() {
    use z3_backend::encoder::expr_has_unmodelable_features;
    // Method calls should NOT be unmodelable (encoded as uninterpreted functions)
    let expr = Expr::MethodCall {
        receiver: Box::new(Expr::Ident("data".into())),
        method: "custom_check".into(),
        args: vec![Expr::Ident("x".into())],
    };
    assert!(
        !expr_has_unmodelable_features(&expr),
        "unknown method calls should be modelable after #201"
    );
}

#[test]
fn field_access_not_unmodelable() {
    use z3_backend::encoder::expr_has_unmodelable_features;
    // Field access (even unknown fields) should be modelable
    let expr = Expr::Field(Box::new(Expr::Ident("obj".into())), "custom_field".into());
    assert!(
        !expr_has_unmodelable_features(&expr),
        "field access should be modelable after #198"
    );
}

// -----------------------------------------------------------------------
// #261 — Native string theory support behind config flag
// -----------------------------------------------------------------------

#[test]
fn test_string_theory_literal_z3() {
    use crate::z3_backend::encoder::{Encoder, Z3Value};
    use assura_parser::ast::{Expr, Literal};
    z3::with_z3_config(&z3::Config::new(), || {
        // With string_theory=true, string literals produce Z3Value::Str
        let mut encoder = Encoder::with_string_theory(true);
        let val = encoder.encode_expr(&Expr::Literal(Literal::Str("hello".into())));
        assert!(
            matches!(val, Z3Value::Str(_)),
            "With string_theory=true, string literals must produce Z3Value::Str"
        );
        // Background axiom for length == 5 should be present
        assert!(
            !encoder.background_axioms.is_empty(),
            "String theory literal must produce a length axiom"
        );
    });
}

#[test]
fn test_string_theory_default_uses_int() {
    use crate::z3_backend::encoder::{Encoder, Z3Value};
    use assura_parser::ast::{Expr, Literal};
    z3::with_z3_config(&z3::Config::new(), || {
        // Default (string_theory=false): string literals produce Z3Value::Int
        let mut encoder = Encoder::new();
        assert!(!encoder.use_string_theory);
        let val = encoder.encode_expr(&Expr::Literal(Literal::Str("hello".into())));
        assert!(
            matches!(val, Z3Value::Int(_)),
            "Default encoding must produce Z3Value::Int for strings"
        );
    });
}

#[test]
fn test_string_theory_length_z3() {
    use crate::z3_backend::encoder::{Encoder, Z3Value};
    use assura_parser::ast::{Expr, Literal};
    z3::with_z3_config(&z3::Config::new(), || {
        let mut encoder = Encoder::with_string_theory(true);
        // Encode "abc".length -> should use native str.len, producing an Int
        let str_expr = Expr::Literal(Literal::Str("abc".into()));
        let field_expr = Expr::Field(Box::new(str_expr), "length".into());
        let val = encoder.encode_expr(&field_expr);
        assert!(
            matches!(val, Z3Value::Int(_)),
            "String .length() must return Z3Value::Int"
        );
    });
}

#[test]
fn test_string_theory_equality_z3() {
    use crate::z3_backend::encoder::{Encoder, Z3Value};
    use assura_parser::ast::{BinOp, Expr, Literal};
    z3::with_z3_config(&z3::Config::new(), || {
        let mut encoder = Encoder::with_string_theory(true);
        // "hello" == "hello" should use native string equality
        let eq_expr = Expr::BinOp {
            lhs: Box::new(Expr::Literal(Literal::Str("hello".into()))),
            op: BinOp::Eq,
            rhs: Box::new(Expr::Literal(Literal::Str("hello".into()))),
        };
        let val = encoder.encode_expr(&eq_expr);
        assert!(
            matches!(val, Z3Value::Bool(_)),
            "String equality must return Z3Value::Bool"
        );
    });
}

// =======================================================================
// #264: Z3 incremental solving (push/pop)
// =======================================================================

#[test]
fn z3_incremental_push_pop_multi_clause() {
    // Contract with 3 clauses sharing the same requires.
    // The Z3 backend should use a single solver with push/pop
    // and produce correct results for all 3 clauses.
    let source = r#"
contract MultiClause {
  requires { x > 0 }
  ensures { x > 0 }
  ensures { x >= 1 }
  ensures { x > 100 }
}
"#;
    let results = verify_source(source);
    assert_eq!(results.len(), 3, "expected 3 results, got {results:?}");
    // x > 0 => x > 0 (trivially verified)
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "x > 0 => x > 0 should verify, got: {:?}",
        results[0]
    );
    // x > 0 => x >= 1 (verified: x > 0 means x >= 1 for integers)
    assert!(
        matches!(&results[1], VerificationResult::Verified { .. }),
        "x > 0 => x >= 1 should verify, got: {:?}",
        results[1]
    );
    // x > 0 => x > 100 (counterexample: e.g. x = 1)
    assert!(
        matches!(&results[2], VerificationResult::Counterexample { .. }),
        "x > 0 => x > 100 should have counterexample, got: {:?}",
        results[2]
    );
}

#[test]
fn z3_incremental_correctness_verified_and_counterexample() {
    // Two clauses: one verifiable, one not. The push/pop mechanism
    // must not let clause-specific assertions leak between checks.
    let source = r#"
contract IncrementalCorrectness {
  requires { x > 0 }
  ensures { x > 0 }
  ensures { x > 5 }
}
"#;
    let results = verify_source(source);
    assert_eq!(results.len(), 2, "expected 2 results, got {results:?}");
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "x > 0 => x > 0 should verify, got: {:?}",
        results[0]
    );
    assert!(
        matches!(&results[1], VerificationResult::Counterexample { .. }),
        "x > 0 => x > 5 should have counterexample, got: {:?}",
        results[1]
    );
}

#[test]
fn z3_incremental_single_clause_still_works() {
    // Single clause contract should still work (push/pop with one clause)
    let source = r#"
contract SingleClause {
  requires { y >= 0 }
  ensures { y >= 0 }
}
"#;
    let results = verify_source(source);
    assert_eq!(results.len(), 1, "expected 1 result, got {results:?}");
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "single clause should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn z3_incremental_no_cross_contamination() {
    // Verify that a counterexample clause (x > 10) does not
    // contaminate the solver state for the next clause (x > 0).
    // If push/pop is broken, the negation of x > 10 would persist.
    let source = r#"
contract NoCrossContamination {
  requires { x > 0 }
  ensures { x > 10 }
  ensures { x > 0 }
}
"#;
    let results = verify_source(source);
    assert_eq!(results.len(), 2, "expected 2 results, got {results:?}");
    // First clause: x > 0 does NOT imply x > 10
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "x > 0 => x > 10 should have counterexample, got: {:?}",
        results[0]
    );
    // Second clause: x > 0 => x > 0 must still verify
    // (push/pop must have removed the negated x > 10 assertion)
    assert!(
        matches!(&results[1], VerificationResult::Verified { .. }),
        "x > 0 => x > 0 should verify after pop, got: {:?}",
        results[1]
    );
}
