use super::*;

// -----------------------------------------------------------------------
// T021: Contract codegen tests
// -----------------------------------------------------------------------

#[test]
fn contract_ensures_generates_debug_assert() {
    let project = codegen_ok(
        r#"
contract NonNeg {
    requires { true }
    ensures  { true }
}
"#,
    );
    let lib = &project.files[0].1;
    // Both requires and ensures should produce debug_assert!
    let assert_count = lib.matches("debug_assert!").count();
    assert!(
        assert_count >= 2,
        "should have debug_assert for both requires and ensures, got {assert_count}"
    );
}

#[test]
fn contract_has_result_variable() {
    let project = codegen_ok(
        r#"
contract Foo {
    requires { true }
    ensures  { true }
}
"#,
    );
    let lib = &project.files[0].1;
    assert!(
        lib.contains(RESULT_VAR),
        "contract should declare result variable"
    );
}

#[test]
fn fn_def_ensures_generates_debug_assert() {
    // Note: clauses must be outside the body block for the parser
    // to parse them as structured Clause objects.
    let project =
        codegen_ok("fn abs(n: Int) -> Int\n    requires { true }\n    ensures  { result >= 0 }\n");
    let lib = &project.files[0].1;
    // requires and ensures should both be debug_assert!
    let assert_count = lib.matches("debug_assert!").count();
    assert!(
        assert_count >= 2,
        "fn should have debug_assert for both requires and ensures, got {assert_count}"
    );
    assert!(
        lib.contains(RESULT_VAR),
        "fn should declare result variable"
    );
}

#[test]
fn fn_result_maps_to_dunder_result() {
    let project = codegen_ok("fn double(n: Int) -> Int\n    ensures { result == n + n }\n");
    let lib = &project.files[0].1;
    assert!(
        lib.contains(RESULT_VAR),
        "result keyword in ensures should map to result var"
    );
}

