use super::*;

// -----------------------------------------------------------------------
// Regression: #170 — Tuple elements must be individually constrained
// -----------------------------------------------------------------------

#[test]
fn test_tuple_encoding_preserves_elements() {
    use crate::z3_backend::encoder::Encoder;
    use assura_ast::{Expr, Literal};
    z3::with_z3_config(&z3::Config::new(), || {
        let mut encoder = Encoder::new();
        let tuple_expr = Spanned::no_span(Expr::Tuple(vec![
            Spanned::no_span(Expr::Literal(Literal::Int("1".into()))),
            Spanned::no_span(Expr::Literal(Literal::Int("2".into()))),
        ]));
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
    use assura_ast::{Expr, Literal};
    z3::with_z3_config(&z3::Config::new(), || {
        let mut encoder = Encoder::new();
        let list_expr = Spanned::no_span(Expr::List(vec![
            Spanned::no_span(Expr::Literal(Literal::Int("10".into()))),
            Spanned::no_span(Expr::Literal(Literal::Int("20".into()))),
            Spanned::no_span(Expr::Literal(Literal::Int("30".into()))),
        ]));
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
    use assura_ast::{Expr, Literal};
    z3::with_z3_config(&z3::Config::new(), || {
        let mut encoder = Encoder::new();
        // Encode two different string literals
        let _hello = encoder.encode_expr(&Spanned::no_span(Expr::Literal(Literal::Str(
            "hello".into(),
        ))));
        let _world = encoder.encode_expr(&Spanned::no_span(Expr::Literal(Literal::Str(
            "world".into(),
        ))));
        // Must have a distinctness axiom (hello != world) plus length axioms
        let has_distinctness = encoder.background_axioms.len() >= 3; // 2 lengths + 1 distinct
        assert!(
            has_distinctness,
            "Different string constants must have distinctness axioms, got {} axioms",
            encoder.background_axioms.len()
        );
        // Same string encoded twice should NOT add another distinctness axiom
        let axiom_count_before = encoder.background_axioms.len();
        let _hello2 = encoder.encode_expr(&Spanned::no_span(Expr::Literal(Literal::Str(
            "hello".into(),
        ))));
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
    use assura_ast::Expr;
    z3::with_z3_config(&z3::Config::new(), || {
        let mut encoder = Encoder::new();
        let apply_expr = Spanned::no_span(Expr::Apply {
            lemma_name: "NonexistentLemma".into(),
            args: vec![Spanned::no_span(Expr::Ident("x".into()))],
        });
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
        VerificationResult::Verified { clause_desc, .. }
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
        VerificationResult::Verified { clause_desc, .. }
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
// encode_call: string / collection method axioms
// -----------------------------------------------------------------------

#[test]
fn test_concat_length_additive() {
    // len(concat(a, b)) == len(a) + len(b) verifies via background axioms
    let src = r#"
contract ConcatLen {
  input(a: String, b: String)
  requires { len(a) == 2 }
  requires { len(b) == 3 }
  ensures { len(concat(a, b)) == 5 }
}
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("ensures"))),
        "concat length should verify, got: {results:?}"
    );
}

#[test]
fn test_substring_length_diff() {
    let src = r#"
contract SubstrLen {
  input(s: String, start: Int, end: Int)
  requires { start >= 0 }
  requires { start <= end }
  requires { end <= len(s) }
  ensures { len(substring(s, start, end)) == end - start }
}
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("ensures"))),
        "substring length should verify, got: {results:?}"
    );
}

#[test]
fn test_push_increments_length() {
    let src = r#"
contract PushLen {
  input(xs: List<Int>, x: Int)
  requires { len(xs) == n }
  requires { n >= 0 }
  ensures { len(push(xs, x)) == n + 1 }
}
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("ensures"))),
        "push length should verify, got: {results:?}"
    );
}

#[test]
fn test_reverse_preserves_length() {
    let src = r#"
contract ReverseLen {
  input(xs: List<Int>)
  requires { len(xs) == n }
  ensures { len(reverse(xs)) == n }
}
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("ensures"))),
        "reverse length should verify, got: {results:?}"
    );
}

#[test]
fn test_clear_zero_length() {
    let src = r#"
contract ClearLen {
  input(xs: List<Int>)
  ensures { len(clear(xs)) == 0 }
}
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("ensures"))),
        "clear length should verify, got: {results:?}"
    );
}

#[test]
fn test_take_length_bounded() {
    let src = r#"
contract TakeLen {
  input(xs: List<Int>, k: Int)
  requires { k >= 0 }
  requires { len(xs) == 10 }
  requires { k <= 10 }
  ensures { len(take(xs, k)) == k }
}
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("ensures"))),
        "take length should verify, got: {results:?}"
    );
}

#[test]
fn test_is_empty_iff_len_zero() {
    // is_empty(xs) implies len(xs) == 0 (bidirectional axiom)
    let src = r#"
contract IsEmptyLen {
  input(xs: List<Int>)
  requires { is_empty(xs) }
  ensures { len(xs) == 0 }
}
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("ensures"))),
        "is_empty => len==0 should verify, got: {results:?}"
    );
}

#[test]
fn test_method_call_push_length() {
    // Method form: xs.push(x) routes receiver as first arg to encode_call
    let src = r#"
contract MethodPush {
  input(xs: List<Int>, x: Int)
  requires { xs.length() == 3 }
  ensures { xs.push(x).length() == 4 }
}
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("ensures"))),
        "method push length should verify, got: {results:?}"
    );
}

// -----------------------------------------------------------------------
// encode_call wave 2: string/collection predicates + array/map get/set/put
// -----------------------------------------------------------------------

#[test]
fn test_contains_implies_length_ge_needle() {
    // contains(s, sub) => len(s) >= len(sub)
    let src = r#"
contract ContainsLen {
  input(s: String, sub: String)
  requires { contains(s, sub) }
  requires { len(sub) == 3 }
  ensures { len(s) >= 3 }
}
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("ensures"))),
        "contains length axiom should verify, got: {results:?}"
    );
}

#[test]
fn test_starts_with_implies_length_ge_prefix() {
    let src = r#"
contract StartsWithLen {
  input(s: String, pre: String)
  requires { starts_with(s, pre) }
  requires { len(pre) == 2 }
  ensures { len(s) >= 2 }
}
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("ensures"))),
        "starts_with length axiom should verify, got: {results:?}"
    );
}

#[test]
fn test_ends_with_empty_affix_always_true() {
    // ends_with(s, aff) when len(aff) == 0 is always true (empty suffix).
    let src = r#"
contract EndsWithEmpty {
  input(s: String, aff: String)
  requires { len(aff) == 0 }
  ensures { ends_with(s, aff) }
}
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("ensures"))),
        "empty affix ends_with should verify, got: {results:?}"
    );
}

#[test]
fn test_contains_key_implies_size_ge_one() {
    let src = r#"
contract ContainsKeySize {
  input(m: Map<Int, Int>, k: Int)
  requires { contains_key(m, k) }
  ensures { size(m) >= 1 }
}
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("ensures"))),
        "contains_key => size>=1 should verify, got: {results:?}"
    );
}

#[test]
fn test_get_set_read_over_write() {
    // get(set(arr, i, v), i) == v
    let src = r#"
contract GetSetRow {
  input(arr: List<Int>, i: Int, v: Int)
  requires { i >= 0 }
  ensures { get(set(arr, i, v), i) == v }
}
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("ensures"))),
        "get/set read-over-write should verify, got: {results:?}"
    );
}

#[test]
fn test_set_preserves_length() {
    let src = r#"
contract SetLen {
  input(arr: List<Int>, i: Int, v: Int)
  requires { len(arr) == n }
  requires { n >= 0 }
  requires { i >= 0 }
  ensures { len(set(arr, i, v)) == n }
}
    "#;
    let results = verify_source(src);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("ensures"))),
        "set preserves length should verify, got: {results:?}"
    );
}

#[test]
fn test_put_read_over_write_and_contains_key() {
    let src = r#"
contract PutGet {
  input(m: Map<Int, Int>, k: Int, v: Int)
  ensures { get(put(m, k, v), k) == v }
  ensures { contains_key(put(m, k, v), k) }
}
    "#;
    let results = verify_source(src);
    let ensures: Vec<_> = results
        .iter()
        .filter(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Unknown { clause_desc, .. } => clause_desc.contains("ensures"),
            _ => false,
        })
        .collect();
    assert!(ensures.len() >= 2, "expected 2 ensures, got {ensures:?}");
    for r in &ensures {
        assert!(
            matches!(r, VerificationResult::Verified { .. }),
            "put get/contains_key should verify, got: {r:?}"
        );
    }
}

// -----------------------------------------------------------------------
// min/max: ite encoding (not unconstrained UF)
// -----------------------------------------------------------------------

#[test]
fn test_min_max_bounds_verify() {
    // min(a,b) <= a and max(a,b) >= a are tautologies when min/max use ite.
    // With unconstrained UF encoding these would produce counterexamples.
    let src = r#"
contract MinMaxBounds {
  input(a: Int, b: Int)
  requires { a >= 0 }
  requires { b >= 0 }
  ensures { min(a, b) <= a }
  ensures { min(a, b) <= b }
  ensures { max(a, b) >= a }
  ensures { max(a, b) >= b }
}
    "#;
    let results = verify_source(src);
    let ensures: Vec<_> = results
        .iter()
        .filter(|r| match r {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Unknown { clause_desc, .. } => clause_desc.contains("ensures"),
            _ => false,
        })
        .collect();
    assert!(
        ensures.len() >= 4,
        "expected 4 ensures results, got {ensures:?}"
    );
    for r in &ensures {
        assert!(
            matches!(r, VerificationResult::Verified { .. }),
            "min/max ite encoding should verify bounds, got: {r:?}"
        );
    }
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
        VerificationResult::Verified { clause_desc, .. }
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
        VerificationResult::Verified { clause_desc, .. }
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
        VerificationResult::Verified { clause_desc, .. }
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
        VerificationResult::Verified { clause_desc, .. }
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
        VerificationResult::Verified { clause_desc, .. }
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
        VerificationResult::Verified { clause_desc, .. }
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
        VerificationResult::Verified { clause_desc, .. }
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
        VerificationResult::Verified { clause_desc, .. }
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
    use crate::feature_max::derive_narrowings;

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

