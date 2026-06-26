use super::*;

fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
    let (sf, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    sf.unwrap()
}

// --- FFI boundary checks ---

#[test]
fn ffi_no_externs_no_errors() {
    let sf = parse_source("contract Simple { requires { true } }");
    assert!(run_ffi_checks(&sf).is_empty());
}

#[test]
fn ffi_extern_without_boundary_no_errors() {
    let sf = parse_source("extern fn malloc(size: Nat) -> Nat");
    assert!(run_ffi_checks(&sf).is_empty());
}

#[test]
fn ffi_extern_boundary_without_contract_emits_a11005() {
    let sf = parse_source("extern fn malloc(size: Nat) -> Nat\n    boundary untrusted");
    let errs = run_ffi_checks(&sf);
    assert!(
        errs.iter().any(|e| e.code == "A11005"),
        "expected A11005: {errs:?}"
    );
}

#[test]
fn ffi_extern_with_boundary_and_requires_no_a11005() {
    let src = "extern fn malloc(size: Nat) -> Nat\n    \
               boundary untrusted\n    requires { size > 0 }";
    let sf = parse_source(src);
    let errs = run_ffi_checks(&sf);
    assert!(
        !errs.iter().any(|e| e.code == "A11005"),
        "should not emit A11005 when extern has requires: {errs:?}"
    );
}

// --- Error propagation checks ---

#[test]
fn error_propagation_no_annotations_no_errors() {
    let sf = parse_source("contract Simple { requires { true } }");
    assert!(run_error_propagation_checks(&sf).is_empty());
}

#[test]
fn error_propagation_fn_without_result_return_no_errors() {
    let src = "fn handler(x: Int) -> Int\n    requires { x > 0 }";
    let sf = parse_source(src);
    assert!(
        run_error_propagation_checks(&sf).is_empty(),
        "fn without Result return should not trigger error propagation checks"
    );
}

/// Pipeline-level A12002 for must_not_mask + catch translate_to (#345).
/// Depends on return-type slurp stopping at ident clause starters (`catch`).
#[test]
fn error_propagation_must_not_mask_catch_translate_a12002() {
    let src = r#"
contract FilePolicy {
    input(path: String)
    must_not_mask { IoError GenericError }
    requires { true }
    ensures { true }
}

fn read_file(path: String) -> Result
    catch IoError translate_to GenericError
    requires { true }
    ensures { true }
"#;
    let sf = parse_source(src);
    let errs = run_error_propagation_checks(&sf);
    assert!(
        errs.iter().any(|e| e.code == "A12002"),
        "expected A12002 for forbidden catch/translate, got: {errs:?}"
    );
}
