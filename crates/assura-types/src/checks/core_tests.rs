use super::*;

fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
    let (sf, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    sf.unwrap()
}

// --- prophecy resolution checks ---

#[test]
fn prophecy_referenced_but_unresolved() {
    let src = r#"
module test;
prophecy future_val: Int
contract Use {
    input(x: Int)
    requires { x > 0 }
    ensures { result > future_val }
}
"#;
    let sf = parse_source(src);
    let errors = run_prophecy_resolution_checks(&sf);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05025");
    assert!(errors[0].message.contains("future_val"));
}

#[test]
fn prophecy_referenced_and_resolved() {
    let src = r#"
module test;
prophecy future_val: Int
contract Use {
    input(x: Int)
    requires { x > 0 }
    ensures { result > future_val }
    ensures { resolve(future_val) }
}
"#;
    let sf = parse_source(src);
    let errors = run_prophecy_resolution_checks(&sf);
    assert!(errors.is_empty(), "expected no errors: {errors:?}");
}

#[test]
fn prophecy_declared_but_unused() {
    // Declared but never referenced in any clause: not an error.
    let src = r#"
module test;
prophecy unused_val: Int
contract Unrelated {
    input(x: Int)
    requires { x > 0 }
    ensures { result >= 0 }
}
"#;
    let sf = parse_source(src);
    let errors = run_prophecy_resolution_checks(&sf);
    assert!(
        errors.is_empty(),
        "unused prophecy should not error: {errors:?}"
    );
}

#[test]
fn multiple_prophecies_mixed() {
    // Two prophecies in separate contracts to avoid parser merging
    // of consecutive prophecy declarations (known parser limitation).
    let src = r#"
module test;
prophecy alpha: Int

contract UseAlpha {
    input(x: Int)
    ensures { result > alpha }
    ensures { resolve(alpha) }
}

prophecy beta: Int

contract UseBeta {
    input(x: Int)
    ensures { result > beta }
}
"#;
    let sf = parse_source(src);
    let errors = run_prophecy_resolution_checks(&sf);
    assert_eq!(errors.len(), 1, "only beta should error: {errors:?}");
    assert!(errors[0].message.contains("beta"));
}

#[test]
fn prophecy_resolved_via_resolve_prophecy() {
    let src = r#"
module test;
prophecy pv: Int
contract Use {
    input(x: Int)
    ensures { result > pv }
    ensures { resolve_prophecy(pv) }
}
"#;
    let sf = parse_source(src);
    let errors = run_prophecy_resolution_checks(&sf);
    assert!(
        errors.is_empty(),
        "resolve_prophecy should count: {errors:?}"
    );
}

#[test]
fn no_prophecies_no_errors() {
    let src = r#"
module test;
contract Simple {
    input(x: Int)
    requires { x > 0 }
    ensures { result >= 0 }
}
"#;
    let sf = parse_source(src);
    let errors = run_prophecy_resolution_checks(&sf);
    assert!(errors.is_empty());
}

// --- liveness checks ---

#[test]
fn liveness_block_missing_prove() {
    let src = r#"
module test;
liveness EventualResponse {
    assume { fair }
}
"#;
    let sf = parse_source(src);
    let errors = run_liveness_checks(&sf);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A31006");
}

#[test]
fn liveness_block_with_prove_ok() {
    let src = r#"
module test;
liveness EventualResponse {
    prove { leads_to(request, response) }
    assume { fair }
}
"#;
    let sf = parse_source(src);
    let errors = run_liveness_checks(&sf);
    assert!(
        errors.is_empty(),
        "valid liveness block should pass: {errors:?}"
    );
}
