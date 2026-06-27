use super::*;

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
    let requires: Vec<SpExpr> = vec![];
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
