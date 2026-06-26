use super::*;

fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
    let (sf, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    sf.unwrap()
}

#[test]
fn crash_recovery_no_annotation_no_errors() {
    let sf = parse_source(r#"contract Simple { requires { true } }"#);
    assert!(run_crash_recovery_checks(&sf).is_empty());
}

#[test]
fn crash_recovery_wal_without_write_wal_emits_a33001() {
    let sf = parse_source(r#"contract W { wal txn1 write_data txn1 }"#);
    let errs = run_crash_recovery_checks(&sf);
    assert!(errs.iter().any(|e| e.code == "A33001"), "got: {errs:?}");
}

#[test]
fn monotonic_no_annotation_no_errors() {
    let sf = parse_source(r#"contract Simple { requires { true } }"#);
    assert!(run_monotonic_state_checks(&sf).is_empty());
}

#[test]
fn monotonic_undeclared_access_emits_a37003() {
    let sf = parse_source(r#"contract C { monotonic seq ensures { other > 0 } }"#);
    let errs = run_monotonic_state_checks(&sf);
    assert!(errs.iter().any(|e| e.code == "A37003"), "got: {errs:?}");
}

#[test]
fn storage_failure_no_annotation_no_errors() {
    let sf = parse_source(r#"contract Simple { requires { true } }"#);
    assert!(run_storage_failure_checks(&sf).is_empty());
}

#[test]
fn storage_failure_unhandled_emits_a38001() {
    let sf = parse_source(r#"contract D { storage_failure partial_write }"#);
    let errs = run_storage_failure_checks(&sf);
    assert!(errs.iter().any(|e| e.code == "A38001"), "got: {errs:?}");
}
