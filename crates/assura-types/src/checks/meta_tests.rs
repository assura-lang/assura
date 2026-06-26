use super::*;

/// Helper: parse source text into a SourceFile, panicking on errors.
fn parse_ok(src: &str) -> assura_parser::ast::SourceFile {
    assura_parser::parse_unwrap(src)
}

// -----------------------------------------------------------------------
// run_complexity_bound_checks
// -----------------------------------------------------------------------

#[test]
fn complexity_no_annotation_produces_no_errors() {
    let source = parse_ok("contract Plain { requires { true } }");
    let errs = run_complexity_bound_checks(&source);
    assert!(
        errs.is_empty(),
        "no complexity clause should yield no errors"
    );
}

#[test]
fn complexity_linear_unverified_produces_a48002() {
    let source =
        parse_ok(r#"contract Search { complexity linear requires { true } ensures { true } }"#);
    let errs = run_complexity_bound_checks(&source);
    assert!(
        errs.iter().any(|e| e.code == "A48002"),
        "expected A48002 for unverified complexity bound, got: {errs:?}"
    );
}

#[test]
fn complexity_verified_does_not_produce_a48002() {
    // Declare a bound AND supply a measured_complexity annotation to discharge it.
    let source = parse_ok(
        r#"contract Sort {
            complexity linear
            measured_complexity linear
            requires { true }
        }"#,
    );
    let errs = run_complexity_bound_checks(&source);
    assert!(
        !errs.iter().any(|e| e.code == "A48002"),
        "verified bound should not emit A48002, got: {errs:?}"
    );
}

// -----------------------------------------------------------------------
// run_contract_composition_checks
// -----------------------------------------------------------------------

#[test]
fn composition_no_extends_produces_no_errors() {
    let source = parse_ok("contract Standalone { requires { true } }");
    let errs = run_contract_composition_checks(&source);
    assert!(
        errs.is_empty(),
        "no extends clause should yield no errors, got: {errs:?}"
    );
}

#[test]
fn composition_extends_unknown_produces_a54001() {
    let source = parse_ok(r#"contract Child { extends NonExistent requires { true } }"#);
    let errs = run_contract_composition_checks(&source);
    assert!(
        errs.iter().any(|e| e.code == "A54001"),
        "expected A54001 for extends unknown contract, got: {errs:?}"
    );
}

#[test]
fn composition_extends_known_does_not_produce_a54001() {
    let source = parse_ok(
        r#"
        contract Parent { requires { true } }
        contract Child { extends Parent requires { true } }
        "#,
    );
    let errs = run_contract_composition_checks(&source);
    assert!(
        !errs.iter().any(|e| e.code == "A54001"),
        "extending a known contract should not emit A54001, got: {errs:?}"
    );
}

// -----------------------------------------------------------------------
// run_scoped_invariant_checks
// -----------------------------------------------------------------------

#[test]
fn scoped_invariant_no_annotation_produces_no_errors() {
    let source = parse_ok("contract Clean { requires { true } }");
    let errs = run_scoped_invariant_checks(&source);
    assert!(
        errs.is_empty(),
        "no suspend_invariant should yield no errors, got: {errs:?}"
    );
}

#[test]
fn scoped_invariant_suspended_ref_in_ensures_produces_a52001() {
    // suspend_invariant marks "sorted" as suspended, then ensures references it.
    let source =
        parse_ok(r#"contract Maintenance { suspend_invariant sorted requires { sorted > 0 } }"#);
    let errs = run_scoped_invariant_checks(&source);
    assert!(
        errs.iter().any(|e| e.code == "A52001"),
        "expected A52001 for suspended invariant referenced in clause, got: {errs:?}"
    );
}

#[test]
fn scoped_invariant_restored_does_not_produce_a52001() {
    // suspend_invariant then restore_invariant before the requires clause.
    let source = parse_ok(
        r#"contract Maintenance {
            suspend_invariant sorted
            restore_invariant sorted
            requires { sorted > 0 }
        }"#,
    );
    let errs = run_scoped_invariant_checks(&source);
    // After restore, references in requires should not emit A52001.
    assert!(
        !errs.iter().any(|e| e.code == "A52001"),
        "restored invariant should not emit A52001, got: {errs:?}"
    );
}

// -----------------------------------------------------------------------
// run_behavioral_equivalence_checks
// -----------------------------------------------------------------------

#[test]
fn behavioral_equivalence_no_annotation_produces_no_errors() {
    let source = parse_ok("contract Simple { requires { true } }");
    let errs = run_behavioral_equivalence_checks(&source);
    assert!(
        errs.is_empty(),
        "no equivalent clause should yield no errors, got: {errs:?}"
    );
}

#[test]
fn behavioral_equivalence_unverified_produces_a49001() {
    let source = parse_ok(r#"contract Equiv { equivalent impl_a == impl_b requires { true } }"#);
    let errs = run_behavioral_equivalence_checks(&source);
    assert!(
        errs.iter().any(|e| e.code == "A49001"),
        "expected A49001 for unverified behavioral equivalence, got: {errs:?}"
    );
}

#[test]
fn behavioral_equivalence_self_equiv_produces_a49002() {
    // Declaring equivalence where both sides are the same triggers A49002.
    let source = parse_ok(r#"contract Equiv { equivalent same == same requires { true } }"#);
    let errs = run_behavioral_equivalence_checks(&source);
    assert!(
        errs.iter().any(|e| e.code == "A49002"),
        "expected A49002 for trivial self-equivalence, got: {errs:?}"
    );
}
