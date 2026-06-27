use super::*;

// T020: Type mapping tests
// -----------------------------------------------------------------------

#[test]
fn type_mapping_int_to_i64() {
    assert_eq!(map_type_token("Int"), "i64");
}

#[test]
fn type_mapping_nat_to_u64() {
    assert_eq!(map_type_token("Nat"), "u64");
}

#[test]
fn type_mapping_float_to_f64() {
    assert_eq!(map_type_token("Float"), "f64");
}

#[test]
fn type_mapping_bool() {
    assert_eq!(map_type_token("Bool"), "bool");
}

#[test]
fn type_mapping_string() {
    assert_eq!(map_type_token("String"), "String");
}

#[test]
fn type_mapping_bytes_to_vec_u8() {
    assert_eq!(map_type_token("Bytes"), "Vec<u8>");
}

#[test]
fn type_mapping_unit() {
    assert_eq!(map_type_token("Unit"), "()");
}

#[test]
fn type_mapping_never() {
    assert_eq!(map_type_token("Never"), "!");
}

#[test]
fn type_mapping_list_to_vec() {
    assert_eq!(map_type_token("List"), "Vec");
}

#[test]
fn type_mapping_map_to_btreemap() {
    assert_eq!(map_type_token("Map"), "std::collections::BTreeMap");
}

#[test]
fn type_mapping_set_to_btreeset() {
    assert_eq!(map_type_token("Set"), "std::collections::BTreeSet");
}

#[test]
fn type_mapping_option_passthrough() {
    assert_eq!(map_type_token("Option"), "Option");
}

#[test]
fn type_mapping_result_passthrough() {
    assert_eq!(map_type_token("Result"), "Result");
}

#[test]
fn type_mapping_fixed_width() {
    assert_eq!(map_type_token("U8"), "u8");
    assert_eq!(map_type_token("U16"), "u16");
    assert_eq!(map_type_token("U32"), "u32");
    assert_eq!(map_type_token("U64"), "u64");
    assert_eq!(map_type_token("I8"), "i8");
    assert_eq!(map_type_token("I16"), "i16");
    assert_eq!(map_type_token("I32"), "i32");
    assert_eq!(map_type_token("I64"), "i64");
    assert_eq!(map_type_token("F32"), "f32");
    assert_eq!(map_type_token("F64"), "f64");
}

#[test]
fn refined_type_generates_newtype() {
    let project = codegen_ok(
        r#"
type Pos = { n: Int | n > 0 }
"#,
    );
    let lib = &project.files[0].1;
    assert!(
        lib.contains("struct Pos"),
        "refined type should generate a newtype struct"
    );
    assert!(lib.contains("i64"), "refined type base should be i64");
}

#[test]
fn type_alias_codegen() {
    let project = codegen_ok(
        r#"
type UserId = Int
"#,
    );
    let lib = &project.files[0].1;
    assert!(lib.contains("UserId"), "should contain type alias name");
}

