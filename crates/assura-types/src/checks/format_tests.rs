use super::*;

fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
    let (sf, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    sf.unwrap()
}

// --- binary format checks ---

#[test]
fn binary_format_no_annotation_produces_no_errors() {
    let src = r#"contract Simple { input(x: Int) requires { x > 0 } ensures { x > 0 } }"#;
    let sf = parse_source(src);
    let errors = run_binary_format_checks(&sf);
    assert!(errors.is_empty(), "expected no errors: {errors:?}");
}

#[test]
fn binary_format_field_exceeds_buffer_length() {
    let src = r#"contract Header { binary_format buf field length }"#;
    let sf = parse_source(src);
    let errors = run_binary_format_checks(&sf);
    assert!(
        errors.iter().any(|e| e.code == "A26001"),
        "expected A26001 for field exceeding buffer length, got: {errors:?}"
    );
}

// --- bit level checks ---

#[test]
fn bit_level_no_annotation_produces_no_errors() {
    let src = r#"contract Simple { input(x: Int) requires { x > 0 } ensures { x > 0 } }"#;
    let sf = parse_source(src);
    let errors = run_bit_level_checks(&sf);
    assert!(errors.is_empty(), "expected no errors: {errors:?}");
}

#[test]
fn bit_level_width_mismatch() {
    let src = r#"contract Flags { bit_layout flags bit_field status }"#;
    let sf = parse_source(src);
    let errors = run_bit_level_checks(&sf);
    assert!(
        errors.iter().any(|e| e.code == "A27003"),
        "expected A27003 for bit width mismatch, got: {errors:?}"
    );
}

// --- string encoding checks ---

#[test]
fn string_encoding_no_annotation_produces_no_errors() {
    let src = r#"contract Simple { input(x: Int) requires { x > 0 } ensures { x > 0 } }"#;
    let sf = parse_source(src);
    let errors = run_string_encoding_checks(&sf);
    assert!(errors.is_empty(), "expected no errors: {errors:?}");
}

#[test]
fn string_encoding_raw_bytes_as_string() {
    let src = r#"contract Decode { encoding data ensures { data > 0 } }"#;
    let sf = parse_source(src);
    let errors = run_string_encoding_checks(&sf);
    assert!(
        errors.iter().any(|e| e.code == "A28001"),
        "expected A28001 for raw bytes used as string, got: {errors:?}"
    );
}

// --- opaque function checks ---

#[test]
fn opaque_function_no_annotation_produces_no_errors() {
    let src = r#"contract Simple { input(x: Int) requires { x > 0 } ensures { x > 0 } }"#;
    let sf = parse_source(src);
    let errors = run_opaque_function_checks(&sf);
    assert!(errors.is_empty(), "expected no errors: {errors:?}");
}

#[test]
fn opaque_function_body_access_without_reveal() {
    let src = "fn helper(x: Int) -> Int\n    opaque marker\n    ensures { helper > 0 }";
    let sf = parse_source(src);
    let errors = run_opaque_function_checks(&sf);
    assert!(
        errors.iter().any(|e| e.code == "A32002"),
        "expected A32002 for opaque function body access, got: {errors:?}"
    );
}
