use super::*;

// -----------------------------------------------------------------------
// T028: End-to-end SafeDivision contract test
// -----------------------------------------------------------------------

#[test]
fn e2e_safe_division_check_passes() {
    // Parse the e2e test file through the full pipeline
    let source = std::fs::read_to_string("../../tests/e2e/safe_division.assura")
        .expect("failed to read safe_division.assura");
    let file = assura_parser::parse_unwrap(&source);
    let resolved = assura_resolve::resolve(&file).expect("resolution should succeed");
    let typed = assura_types::type_check(resolved).expect("type check should succeed");

    // Codegen should succeed
    let project = codegen(&typed);
    assert!(
        !project.cargo_toml.is_empty(),
        "Cargo.toml should not be empty"
    );
    assert_eq!(
        project.files.len(),
        1,
        "should produce exactly one source file"
    );
    assert_eq!(project.files[0].0, "src/lib.rs");
}

#[test]
fn e2e_safe_division_generates_debug_assert_for_requires() {
    let source = std::fs::read_to_string("../../tests/e2e/safe_division.assura")
        .expect("failed to read safe_division.assura");
    let project = codegen_ok(&source);
    let lib = &project.files[0].1;

    // The requires clause `b != 0` must produce a debug_assert
    assert!(
        lib.contains("debug_assert!"),
        "generated code must contain debug_assert!"
    );
    assert!(
        lib.contains("b != 0"),
        "generated code must contain the requires predicate 'b != 0'"
    );
}

#[test]
fn e2e_safe_division_generates_ensures_assertion() {
    let source = std::fs::read_to_string("../../tests/e2e/safe_division.assura")
        .expect("failed to read safe_division.assura");
    let project = codegen_ok(&source);
    let lib = &project.files[0].1;

    // The ensures clause should produce a debug_assert with the postcondition
    assert!(
        lib.contains("debug_assert!"),
        "generated code must contain debug_assert from requires/ensures"
    );
    // At least two debug_assert! calls: for requires and ensures
    let assert_count = lib.matches("debug_assert!").count();
    assert!(
        assert_count >= 2,
        "should have debug_assert for both requires and ensures, got {assert_count}"
    );
}

#[test]
fn e2e_safe_division_has_correct_signature() {
    let source = std::fs::read_to_string("../../tests/e2e/safe_division.assura")
        .expect("failed to read safe_division.assura");
    let project = codegen_ok(&source);
    let lib = &project.files[0].1;

    // Should have input params mapped to i64
    assert!(lib.contains("a: i64"), "input param 'a' should map to i64");
    assert!(lib.contains("b: i64"), "input param 'b' should map to i64");
    // Should have the contract module
    assert!(
        lib.contains("contract_safedivision"),
        "should contain the SafeDivision contract module"
    );
}

#[test]
fn e2e_safe_division_generated_rust_is_valid() {
    let source = std::fs::read_to_string("../../tests/e2e/safe_division.assura")
        .expect("failed to read safe_division.assura");
    let project = codegen_ok(&source);
    let lib = &project.files[0].1;

    // Verify the generated Rust parses as valid syntax via syn
    syn::parse_file(lib).expect("generated Rust should be valid syntax");
}
