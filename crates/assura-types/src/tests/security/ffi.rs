use super::*;

// --- T058: FFI boundary contract tests ---

#[test]
fn ffi_extern_without_boundary_a11001() {
    let checker = FfiBoundaryChecker::new();
    let errors = checker.check_extern_decl("malloc", false, false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A11001");
}

#[test]
fn ffi_extern_with_boundary_ok() {
    let checker = FfiBoundaryChecker::new();
    let errors = checker.check_extern_decl("malloc", true, true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ffi_untrusted_without_contract_a11002() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("read_bytes".into(), TrustBoundary::Untrusted);
    let errors = checker.check_extern_decl("read_bytes", true, false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A11002");
}

#[test]
fn ffi_untrusted_with_contract_ok() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("read_bytes".into(), TrustBoundary::Untrusted);
    let errors = checker.check_extern_decl("read_bytes", true, true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ffi_trusted_no_contract_ok() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("internal_fn".into(), TrustBoundary::Trusted);
    let errors = checker.check_extern_decl("internal_fn", true, false, &(0..1));
    assert!(errors.is_empty(), "trusted extern doesn't need a contract");
}

#[test]
fn ffi_call_untrusted_unvalidated_a11003() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("read_raw".into(), TrustBoundary::Untrusted);
    let errors = checker.check_ffi_call("read_raw", false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A11003");
}

#[test]
fn ffi_call_untrusted_validated_ok() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("read_raw".into(), TrustBoundary::Untrusted);
    let errors = checker.check_ffi_call("read_raw", true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ffi_call_trusted_unvalidated_ok() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("safe_fn".into(), TrustBoundary::Trusted);
    let errors = checker.check_ffi_call("safe_fn", false, &(0..1));
    assert!(errors.is_empty(), "trusted calls don't need validation");
}

#[test]
fn ffi_unsafe_outside_wrapper_a11004() {
    let checker = FfiBoundaryChecker::new();
    let errors = checker.check_unsafe_confinement("compute", false, true, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A11004");
}

#[test]
fn ffi_unsafe_inside_wrapper_ok() {
    let checker = FfiBoundaryChecker::new();
    let errors = checker.check_unsafe_confinement("ffi_wrapper", true, true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ffi_boundary_display() {
    assert_eq!(TrustBoundary::Trusted.to_string(), "trusted");
    assert_eq!(TrustBoundary::Audited.to_string(), "audited");
    assert_eq!(TrustBoundary::Untrusted.to_string(), "untrusted");
}

#[test]
fn ffi_file_check_multiple_externs() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("read".into(), TrustBoundary::Untrusted);
    checker.register_extern("write".into(), TrustBoundary::Audited);
    let externs = vec![
        ("read".into(), true, false, 0..5), // untrusted, no contract -> A11002
        ("write".into(), true, true, 10..15), // audited, has contract -> ok
        ("unknown".into(), false, false, 20..25), // no boundary -> A11001
    ];
    let errors = checker.check_file(&externs);
    assert_eq!(errors.len(), 2); // A11002 for read, A11001 for unknown
}

