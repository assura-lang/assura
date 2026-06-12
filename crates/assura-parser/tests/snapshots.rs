//! Snapshot tests for the Assura parser.
//!
//! Each test parses an .assura file and snapshots the resulting AST via
//! `insta::assert_debug_snapshot!`. Run `cargo insta review` to accept
//! or update snapshots.

use assura_parser::parse;

fn parse_file(path: &str) -> assura_parser::ast::SourceFile {
    let source =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
    let (ast, errors) = parse(&source);
    assert!(errors.is_empty(), "parse errors in {path}: {errors:?}");
    ast.expect("parse returned None without errors")
}

// --- Fixture tests ---

#[test]
fn snapshot_empty() {
    let ast = parse_file("../../tests/fixtures/empty.assura");
    insta::assert_debug_snapshot!(ast);
}

#[test]
fn snapshot_imports_only() {
    let ast = parse_file("../../tests/fixtures/imports_only.assura");
    insta::assert_debug_snapshot!(ast);
}

#[test]
fn snapshot_contract_minimal() {
    let ast = parse_file("../../tests/fixtures/contract_minimal.assura");
    insta::assert_debug_snapshot!(ast);
}

#[test]
fn snapshot_all_clause_kinds() {
    let ast = parse_file("../../tests/fixtures/all_clause_kinds.assura");
    insta::assert_debug_snapshot!(ast);
}

#[test]
fn snapshot_nested_types() {
    let ast = parse_file("../../tests/fixtures/nested_types.assura");
    insta::assert_debug_snapshot!(ast);
}

#[test]
fn snapshot_service_full() {
    let ast = parse_file("../../tests/fixtures/service_full.assura");
    insta::assert_debug_snapshot!(ast);
}

#[test]
fn snapshot_test_basic() {
    let ast = parse_file("../../tests/fixtures/test_basic.assura");
    insta::assert_debug_snapshot!(ast);
}

// --- Demo file tests ---

#[test]
fn snapshot_demo_libwebp() {
    let ast = parse_file("../../demos/libwebp-huffman.assura");
    insta::assert_debug_snapshot!(ast);
}

#[test]
fn snapshot_demo_zlib() {
    let ast = parse_file("../../demos/zlib-inflate.assura");
    insta::assert_debug_snapshot!(ast);
}

#[test]
fn snapshot_demo_mbedtls() {
    let ast = parse_file("../../demos/mbedtls-x509.assura");
    insta::assert_debug_snapshot!(ast);
}
