use super::*;

#[test]
fn extract_capacity_buffer() {
    assert_eq!(
        extract_capacity_annotation("Buffer<1024>"),
        Some("1024".into())
    );
}

#[test]
fn extract_capacity_region() {
    assert_eq!(
        extract_capacity_annotation("Region<MAX_SIZE>"),
        Some("MAX_SIZE".into())
    );
}

#[test]
fn extract_capacity_fixed_buffer() {
    assert_eq!(
        extract_capacity_annotation("FixedBuffer<256>"),
        Some("256".into())
    );
}

#[test]
fn extract_capacity_no_match() {
    assert_eq!(extract_capacity_annotation("Int"), None);
    assert_eq!(extract_capacity_annotation("String"), None);
    assert_eq!(extract_capacity_annotation("List<Int>"), None);
}

#[test]
fn extract_capacity_empty_angle() {
    assert_eq!(extract_capacity_annotation("Buffer<>"), Some("".into()));
}

fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
    let (sf, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    sf.unwrap()
}

#[test]
fn allocator_unbounded_via_source() {
    let src = r#"
module test;
contract Alloc {
    input(size: Nat)
    alloc buf
    requires { size > 0 }
    ensures { size > 0 }
}
"#;
    let sf = parse_source(src);
    let errors = run_allocator_checks(&sf);
    let has_a22003 = errors.iter().any(|e| e.code == "A22003");
    assert!(has_a22003, "expected A22003 unbounded alloc: {errors:?}");
}

#[test]
fn allocator_bounded_via_source() {
    let src = r#"
module test;
contract Alloc {
    input(size: Nat)
    alloc buf
    bounded buf
    requires { size > 0 }
    ensures { size > 0 }
}
"#;
    let sf = parse_source(src);
    let errors = run_allocator_checks(&sf);
    let has_a22003 = errors.iter().any(|e| e.code == "A22003");
    assert!(
        !has_a22003,
        "bounded alloc should not produce A22003: {errors:?}"
    );
}
