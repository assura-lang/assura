use super::*;

fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
    let (sf, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    sf.unwrap()
}

#[test]
fn callback_no_annotation_no_errors() {
    let sf = parse_source(r#"contract Simple { requires { true } }"#);
    assert!(run_callback_reentrancy_checks(&sf).is_empty());
}

#[test]
fn callback_reentrant_emits_a24001() {
    let sf = parse_source(r#"contract G { non_reentrant handler requires { handler > 0 } }"#);
    let errs = run_callback_reentrancy_checks(&sf);
    assert!(errs.iter().any(|e| e.code == "A24001"), "got: {errs:?}");
}

#[test]
fn deadline_no_annotation_no_errors() {
    let sf = parse_source(r#"contract Simple { requires { true } }"#);
    assert!(run_temporal_deadline_checks(&sf).is_empty());
}

#[test]
fn deadline_unbounded_op_emits_a25003() {
    let sf = parse_source(r#"contract T { deadline respond requires { compute > 0 } }"#);
    let errs = run_temporal_deadline_checks(&sf);
    assert!(errs.iter().any(|e| e.code == "A25003"), "got: {errs:?}");
}
