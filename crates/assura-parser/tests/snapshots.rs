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

#[test]
fn snapshot_match_expr() {
    let ast = parse_file("../../tests/fixtures/match_expr.assura");
    insta::assert_debug_snapshot!(ast);
}

// --- Error recovery tests (T006) ---
//
// These tests verify that `parse_recovery()` produces errors (not panics)
// with meaningful messages and, where possible, returns a partial AST.

/// Helper: parse an error fixture, returning (Option<AST>, errors).
fn parse_error_file(
    path: &str,
) -> (
    Option<assura_parser::ast::SourceFile>,
    Vec<chumsky::error::Simple<assura_parser::lexer::Token>>,
) {
    let source =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
    parse(&source)
}

#[test]
fn error_recovery_missing_brace() {
    let (ast, errors) = parse_error_file("../../tests/fixtures/errors/missing_brace.assura");

    // Must produce at least one error for the unclosed brace
    assert!(
        !errors.is_empty(),
        "expected parse errors for missing_brace.assura but got none"
    );

    // Snapshot the errors for future regression tracking
    let error_messages: Vec<String> = errors.iter().map(|e| format!("{e}")).collect();
    insta::assert_debug_snapshot!("missing_brace_errors", &error_messages);

    // parse_recovery() may or may not return a partial AST; snapshot either way
    insta::assert_debug_snapshot!("missing_brace_ast", &ast);
}

#[test]
fn error_recovery_bad_token() {
    let (ast, errors) = parse_error_file("../../tests/fixtures/errors/bad_token.assura");

    // The lexer silently drops unrecognized characters (logos returns Err),
    // so the parser sees a token stream with gaps. This may or may not
    // produce parse errors depending on how the remaining tokens align.
    // We snapshot both outcomes for regression tracking.
    let error_messages: Vec<String> = errors.iter().map(|e| format!("{e}")).collect();
    insta::assert_debug_snapshot!("bad_token_errors", &error_messages);
    insta::assert_debug_snapshot!("bad_token_ast", &ast);
}

#[test]
fn error_recovery_duplicate_clause() {
    let (ast, errors) = parse_error_file("../../tests/fixtures/errors/duplicate_clause.assura");

    // The parser collects all clauses without deduplication, so duplicate
    // requires clauses should parse successfully. Detecting duplicates
    // is a concern for the resolver/type checker, not the parser.
    let error_messages: Vec<String> = errors.iter().map(|e| format!("{e}")).collect();
    insta::assert_debug_snapshot!("duplicate_clause_errors", &error_messages);

    // The AST should exist and contain both requires clauses
    let source_file = ast
        .as_ref()
        .expect("duplicate_clause.assura should parse successfully");
    let contract = source_file
        .decls
        .iter()
        .find_map(|d| match &d.node {
            assura_parser::ast::Decl::Contract(c) => Some(c),
            _ => None,
        })
        .expect("should contain a contract declaration");

    let requires_count = contract
        .clauses
        .iter()
        .filter(|c| c.kind == assura_parser::ast::ClauseKind::Requires)
        .count();
    assert_eq!(
        requires_count, 2,
        "parser should preserve both requires clauses"
    );

    insta::assert_debug_snapshot!("duplicate_clause_ast", &ast);
}
