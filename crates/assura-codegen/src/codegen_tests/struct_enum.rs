use super::*;

// -----------------------------------------------------------------------
// T023: Struct and enum codegen tests
// -----------------------------------------------------------------------

#[test]
fn struct_has_derive_debug_clone_partialeq() {
    let project = codegen_ok(
        r#"
type Pair {
    a: Int
    b: Int
}
"#,
    );
    let lib = &project.files[0].1;
    assert!(lib.contains("Debug"), "struct should derive Debug");
    assert!(lib.contains("Clone"), "struct should derive Clone");
    assert!(lib.contains("PartialEq"), "struct should derive PartialEq");
}

#[test]
fn struct_field_types_are_mapped() {
    let project = codegen_ok(
        r#"
type Config {
    name: String
    count: Nat
    enabled: Bool
    ratio: Float
}
"#,
    );
    let lib = &project.files[0].1;
    assert!(lib.contains("String"), "String should map to String");
    assert!(lib.contains("u64"), "Nat should map to u64");
    assert!(lib.contains("bool"), "Bool should map to bool");
    assert!(lib.contains("f64"), "Float should map to f64");
}

#[test]
fn struct_pub_field_visibility() {
    let project = codegen_ok(
        r#"
type Visible {
    pub x: Int
    y: Int
}
"#,
    );
    let lib = &project.files[0].1;
    assert!(
        lib.contains("pub x"),
        "pub field should have pub visibility in generated code"
    );
}

#[test]
fn enum_has_derive_debug_clone_partialeq() {
    let project = codegen_ok(
        r#"
enum Direction {
    North,
    South,
    East,
    West
}
"#,
    );
    let lib = &project.files[0].1;
    assert!(lib.contains("Debug"), "enum should derive Debug");
    assert!(lib.contains("Clone"), "enum should derive Clone");
    assert!(lib.contains("PartialEq"), "enum should derive PartialEq");
}

#[test]
fn enum_variant_with_data() {
    let project = codegen_ok(
        r#"
enum Value {
    Num(Int),
    Text(String),
    Nothing
}
"#,
    );
    let lib = &project.files[0].1;
    assert!(lib.contains("Num"), "should contain Num variant");
    assert!(lib.contains("i64"), "Int should map to i64 in variant");
    assert!(lib.contains("Text"), "should contain Text variant");
    assert!(
        lib.contains("Nothing"),
        "should contain unit variant Nothing"
    );
}

#[test]
fn empty_struct_codegen() {
    let project = codegen_ok(
        r#"
type Marker {
}
"#,
    );
    let lib = &project.files[0].1;
    assert!(lib.contains("Marker"), "should contain empty struct");
}
