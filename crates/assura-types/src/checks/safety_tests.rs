use super::*;

fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
    let (sf, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    sf.unwrap()
}

#[test]
fn constant_time_no_annotation_no_errors() {
    let sf = parse_source(r#"contract Simple { requires { true } }"#);
    assert!(run_constant_time_checks(&sf).is_empty());
}

#[test]
fn secure_erasure_no_annotation_no_errors() {
    let sf = parse_source(r#"contract Simple { requires { true } }"#);
    assert!(run_secure_erasure_checks(&sf).is_empty());
}

#[test]
fn unsafe_escape_no_annotation_no_errors() {
    let sf = parse_source(r#"contract Simple { requires { true } }"#);
    assert!(run_unsafe_escape_checks(&sf).is_empty());
}

#[test]
fn unsafe_escape_fn_without_proof_emits_a47001() {
    let src = "fn risky(p: Int) -> Int\n    unsafe_escape marker\n    requires { p > 0 }";
    let sf = parse_source(src);
    let errs = run_unsafe_escape_checks(&sf);
    assert!(errs.iter().any(|e| e.code == "A47001"), "got: {errs:?}");
}
