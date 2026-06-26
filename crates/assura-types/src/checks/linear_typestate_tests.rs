use super::*;

fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
    let (sf, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    sf.unwrap()
}

#[test]
fn linearity_no_annotation_no_errors() {
    let sf = parse_source(r#"contract Simple { requires { true } }"#);
    assert!(run_linearity_checks(&sf).is_empty());
}

#[test]
fn typestate_no_annotation_no_errors() {
    let sf = parse_source(r#"contract Simple { requires { true } }"#);
    assert!(run_typestate_checks(&sf).is_empty());
}
