use super::*;

fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
    let (sf, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    sf.unwrap()
}

#[test]
fn platform_no_annotation_no_errors() {
    let sf = parse_source(r#"contract Simple { requires { true } }"#);
    assert!(run_platform_abstraction_checks(&sf).is_empty());
}

#[test]
fn platform_missing_impl_emits_a44001() {
    let src = "contract P {\n    platform linux\n    abstraction fs\n    platform windows\n}";
    let sf = parse_source(src);
    let errs = run_platform_abstraction_checks(&sf);
    assert!(errs.iter().any(|e| e.code == "A44001"), "got: {errs:?}");
}

#[test]
fn feature_flag_no_annotation_no_errors() {
    let sf = parse_source(r#"contract Simple { requires { true } }"#);
    assert!(run_feature_flag_checks(&sf).is_empty());
}

#[test]
fn feature_flag_unused_emits_a45001() {
    let sf = parse_source(r#"contract F { feature_flag debug_mode }"#);
    let errs = run_feature_flag_checks(&sf);
    assert!(errs.iter().any(|e| e.code == "A45001"), "got: {errs:?}");
}

#[test]
fn resource_limit_no_annotation_no_errors() {
    let sf = parse_source(r#"contract Simple { requires { true } }"#);
    assert!(run_resource_limit_checks(&sf).is_empty());
}
