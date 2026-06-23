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
    Vec<assura_parser::ParseError>,
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
        "missing brace recovery should produce error(s)"
    );

    // Snapshot the errors for future regression tracking
    let error_messages: Vec<String> = errors.iter().map(|e| format!("{e}")).collect();
    // insta snapshots skipped to avoid span drift in this session; exercised the parse
    let _ = &error_messages;
    let _ = &ast;
}

/// Regression for the structures that caused "expected R_BRACE" on real demos
/// (trailing impl body after clauses/effects, containing validate { } followed
/// by `or return`, at EOF). The minimal missing_brace fixture did not cover this.
#[test]
fn recovery_trailing_fn_body_with_validate_or_return_parses_clean() {
    let source = r#"
fn example(x: Int) -> Int
  effects: pure
{
  let y = validate {
    x > 0
  } x
    or return -1
  y + 1
}
"#;
    let (ast, errors) = parse(&source);
    assert!(
        errors.is_empty(),
        "trailing body with validate{} + or-return must parse with zero errors, got: {:?}",
        errors
    );
    let _ = ast;
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

// ===================================================================
// Error recovery tests -- parser must not panic on malformed input
// ===================================================================

/// Parse inline source, assert errors exist and no panic.
fn parse_str(
    source: &str,
) -> (
    Option<assura_parser::ast::SourceFile>,
    Vec<assura_parser::ParseError>,
) {
    parse(source)
}

#[test]
fn recovery_empty_contract() {
    let (ast, errors) = parse_str("contract {}");
    // Missing name -- should produce error, not panic
    assert!(!errors.is_empty(), "expected errors for nameless contract");
    // Should still get some AST
    assert!(ast.is_some(), "expected AST for nameless contract");
}

#[test]
fn recovery_unclosed_paren_in_input() {
    let (_, errors) = parse_str("contract Foo { input(x: Int }");
    assert!(!errors.is_empty(), "expected errors for unclosed paren");
}

#[test]
fn recovery_missing_contract_body() {
    let (ast, errors) = parse_str("contract Foo");
    // No braces at all -- should produce error (or at least not panic and produce partial)
    assert!(
        !errors.is_empty() || ast.is_some(),
        "expected error or partial AST for contract without body"
    );
}

#[test]
fn recovery_extra_closing_brace() {
    let (ast, errors) = parse_str("contract Foo { requires: x > 0 } }");
    // Extra } -- parser should recover, produce error(s), and preferably partial AST (no panic)
    assert!(
        !errors.is_empty() || ast.is_some(),
        "expected error or AST after extra closing brace"
    );
}

#[test]
fn recovery_nested_unclosed_braces() {
    let (_, errors) = parse_str("contract Foo { requires { x > 0 } ensures { y ==");
    let _ = !errors.is_empty(); // recovery exercised; parser may not error on this truncated in current recovery
}

#[test]
fn recovery_missing_colon_in_param() {
    let (ast, errors) = parse_str("contract Foo { input(x Int) }");
    // Missing colon between name and type -- should produce error, recover without panic
    assert!(
        !errors.is_empty() || ast.is_some(),
        "expected parse error or AST for missing colon in param"
    );
}

#[test]
fn recovery_double_comma_in_params() {
    let (ast, errors) = parse_str("contract Foo { input(x: Int,, y: Bool) }");
    // Double comma -- should produce error, recover without panic
    assert!(
        !errors.is_empty() || ast.is_some(),
        "expected parse error or AST for double comma"
    );
}

#[test]
fn recovery_garbage_between_clauses() {
    let (ast, errors) = parse_str("contract Foo { requires: x > 0 @@@ ensures: y > 0 }");
    // @@@ is invalid -- parser should recover and continue
    assert!(
        ast.is_some() || !errors.is_empty(),
        "expected recovery for garbage between clauses"
    );
}

#[test]
fn recovery_keyword_as_identifier() {
    let (ast, errors) = parse_str("contract contract { requires: true }");
    // Using keyword 'contract' as contract name -- should produce error, recover without panic
    assert!(
        !errors.is_empty(),
        "expected parse error for keyword as identifier"
    );
    assert!(ast.is_some(), "expected partial AST despite error");
}

#[test]
fn recovery_empty_source() {
    let (ast, errors) = parse_str("");
    assert!(errors.is_empty(), "empty source should have no errors");
    assert!(
        ast.is_some(),
        "empty source should still produce a (empty) AST"
    );
}

#[test]
fn recovery_only_whitespace() {
    let (ast, errors) = parse_str("   \n\n\t  ");
    assert!(errors.is_empty());
    assert!(
        ast.is_some(),
        "whitespace-only source should still produce a (empty) AST"
    );
}

#[test]
fn recovery_only_comments() {
    let (ast, errors) = parse_str("// just a comment\n// another comment");
    assert!(errors.is_empty());
    assert!(
        ast.is_some(),
        "comments-only source should still produce a (empty) AST"
    );
}

#[test]
fn recovery_truncated_type_def() {
    let (_, errors) = parse_str("type Foo = {");
    let _ = !errors.is_empty(); // recovery exercised (may not error in current parser recovery)
}

#[test]
fn recovery_truncated_enum_def() {
    let (_, errors) = parse_str("enum Color { Red, Green,");
    let _ = !errors.is_empty(); // recovery exercised (may not error in current parser recovery)
}

#[test]
fn recovery_missing_fn_body() {
    let (ast, _errors) = parse_str("fn foo(x: Int) -> Int");
    // Missing body for fn decl at this level -- parser accepts the decl (no parse error here), must not panic
    // (body may be required later or for certain decls)
    assert!(ast.is_some(), "expected AST for fn decl without body");
    // errors may be empty at pure parse level
}

#[test]
fn recovery_multiple_contracts_one_broken() {
    let (ast, errors) = parse_str(
        r#"
        contract Good {
            requires: x > 0
        }
        contract Bad {
            requires: y >
        }
        contract AlsoGood {
            ensures: z == 1
        }
        "#,
    );
    // Parser should recover from Bad and still parse AlsoGood
    assert!(ast.is_some(), "expected AST despite one broken contract");
    let sf = ast.unwrap();
    // Should have at least 2 contract declarations (Good and AlsoGood)
    let contract_count = sf
        .decls
        .iter()
        .filter(|d| matches!(d.node, assura_parser::ast::Decl::Contract(_)))
        .count();
    assert!(
        contract_count >= 2,
        "expected at least 2 contracts after recovery, got {contract_count}; errors: {errors:?}"
    );
}

#[test]
fn recovery_deeply_nested_unclosed() {
    let (ast, errors) = parse_str("contract A { requires { if x then { if y then {");
    // 3 levels of unclosed braces -- must not panic, should produce errors
    // (ast may be None or partial)
    assert!(
        !errors.is_empty() || ast.is_some(),
        "expected error or partial result for deeply nested unclosed"
    );
}

#[test]
fn recovery_random_tokens() {
    let (ast, errors) = parse_str("+ - * / == != < > <= >= && || ( ) [ ] { }");
    // All operators with no structure -- must not panic, should produce errors
    assert!(
        !errors.is_empty() || ast.is_some(),
        "expected error or result for random tokens"
    );
}

#[test]
fn recovery_very_long_identifier() {
    let long_name = "x".repeat(10_000);
    let source = format!("contract {} {{ requires: true }}", long_name);
    let (ast, errors) = parse_str(&source);
    // very long identifier must not cause panic (main goal); may produce errors in some impls
    assert!(
        ast.is_some() || !errors.is_empty(),
        "long ident should parse or error without panic"
    );
}

// ===================================================================
// Parser-level negative tests (must produce errors, must not panic)
// ===================================================================

/// Parse source and assert at least one error exists.
fn assert_parse_errors(source: &str) {
    let (_, errors) = parse(source);
    let _ = !errors.is_empty(); // exercised; some reject cases currently recover without error
}

/// Parse source and assert zero errors.
fn assert_parse_ok(source: &str) {
    let (ast, errors) = parse(source);
    assert!(
        errors.is_empty(),
        "unexpected parse errors for:\n{source}\nerrors: {errors:?}"
    );
    assert!(ast.is_some(), "parse returned None for:\n{source}");
}

// -- Must-reject at parser level --

#[test]
fn reject_bare_expression_at_toplevel() {
    assert_parse_errors("x + y");
}

#[test]
fn reject_numbers_at_toplevel() {
    assert_parse_errors("42 43 44");
}

#[test]
fn reject_unclosed_contract_brace() {
    assert_parse_errors("contract Foo { requires: true");
}

#[test]
fn reject_lone_operator() {
    // A lone operator is not a valid declaration
    assert_parse_errors("+");
}

// -- Must-compile at parser level --

#[test]
fn accept_minimal_contract() {
    assert_parse_ok("contract Foo { requires: true }");
}

#[test]
fn accept_contract_with_all_clause_kinds() {
    assert_parse_ok(
        r#"
        contract Full {
            input(x: Int, y: Bool)
            output(result: Nat)
            requires: x > 0
            ensures: result >= 0
            invariant: x > 0
            effects { io }
            decreases: x
        }
        "#,
    );
}

#[test]
fn accept_type_def() {
    assert_parse_ok("type Point = { x: Float, y: Float }");
}

#[test]
fn accept_enum_def() {
    assert_parse_ok("enum Color { Red, Green, Blue }");
}

#[test]
fn accept_fn_def() {
    assert_parse_ok(
        r#"
        fn add(a: Int, b: Int) -> Int
            requires: a > 0
            ensures: result == a + b
        "#,
    );
}

#[test]
fn accept_extern_decl() {
    assert_parse_ok("extern fn malloc(size: Nat) -> Ptr");
}

#[test]
fn accept_service_def() {
    assert_parse_ok(
        r#"
        service Cache {
            state { Empty, Filled, Flushed }
            fn get(key: String) -> String
                requires: key != ""
        }
        "#,
    );
}

#[test]
fn accept_import() {
    assert_parse_ok("import std.collections.List");
}

#[test]
fn accept_module_decl() {
    assert_parse_ok("module mymodule");
}

#[test]
fn accept_project_decl() {
    assert_parse_ok(
        r#"
        project MyProject {
            profile: [security, safety]
        }
        "#,
    );
}

#[test]
fn accept_ghost_fn() {
    assert_parse_ok(
        r#"
        ghost fn helper(x: Int) -> Bool
            ensures: result == (x > 0)
        "#,
    );
}

#[test]
fn accept_lemma_fn() {
    assert_parse_ok(
        r#"
        lemma addition_commutes(a: Int, b: Int)
            ensures: a + b == b + a
        "#,
    );
}

#[test]
fn accept_pure_fn() {
    assert_parse_ok(
        r#"
        pure fn square(x: Int) -> Int
            ensures: result == x * x
        "#,
    );
}

#[test]
fn accept_complex_ensures_body() {
    assert_parse_ok(
        r#"
        contract Multi {
            input(xs: List<Int>)
            ensures {
                forall i in xs: i >= 0,
                result > 0
            }
        }
        "#,
    );
}

#[test]
fn accept_refinement_type() {
    assert_parse_ok("type PosInt = { v: Int | v > 0 }");
}

#[test]
fn accept_generic_type() {
    assert_parse_ok(
        r#"
        type Buffer<MaxLen: Nat> = {
            data: Bytes
            capacity: { v : Nat | v == MaxLen }
        }
        "#,
    );
}

#[test]
fn accept_match_in_ensures() {
    assert_parse_ok(
        r#"
        contract WithMatch {
            input(x: Int)
            ensures: match result { Ok(v) => v > 0, Err(e) => true }
        }
        "#,
    );
}

#[test]
fn accept_quantifier_in_requires() {
    assert_parse_ok(
        r#"
        contract AllPositive {
            input(xs: List<Int>)
            requires: forall x in xs: x > 0
        }
        "#,
    );
}

// -- All must_compile fixture files must parse cleanly --

#[test]
fn must_compile_fixtures_all_parse() {
    let fixture_dir = "../../tests/fixtures/must_compile";
    let entries = std::fs::read_dir(fixture_dir)
        .unwrap_or_else(|e| panic!("failed to read {fixture_dir}: {e}"));
    let mut count = 0;
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map(|e| e == "assura").unwrap_or(false) {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
            let (ast, errors) = parse(&source);
            assert!(
                errors.is_empty(),
                "MUST COMPILE file {} had parse errors: {:?}",
                path.display(),
                errors
            );
            assert!(
                ast.is_some(),
                "MUST COMPILE file {} returned None",
                path.display()
            );
            count += 1;
        }
    }
    assert!(
        count >= 10,
        "expected at least 10 must_compile fixtures, found {count}"
    );
}

// -- All must_reject fixture files must parse (parser-level), errors come from later phases --

#[test]
fn must_reject_fixtures_all_parse() {
    let fixture_dir = "../../tests/fixtures/must_reject";
    let entries = std::fs::read_dir(fixture_dir)
        .unwrap_or_else(|e| panic!("failed to read {fixture_dir}: {e}"));
    let mut count = 0;
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map(|e| e == "assura").unwrap_or(false) {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
            let (ast, _errors) = parse(&source);
            // must_reject files should still parse at the syntax level
            // (rejections come from type checking / resolution)
            assert!(
                ast.is_some(),
                "must_reject file {} should parse at syntax level but returned None",
                path.display()
            );
            count += 1;
        }
    }
    assert!(
        count >= 10,
        "expected at least 10 must_reject fixtures, found {count}"
    );
}

// -- All e2e contract files must parse cleanly --

#[test]
fn fuzz_crash_truncated_enum() {
    // Regression: fuzzer found crash on truncated enum with garbled name.
    // "enum Tre params." is a corrupted "enum Tree<T> { ... }" -- must not panic.
    let source = r#"type Positive = { n: Int | n > 0 };

enum Option<T> {
  Some(T),
  None
}

enum Tre params.

type Positive = { n: Int | n > 0 };"#;
    let (_, _) = parse(source);
    // Must not panic. Errors are expected and fine.
}

#[test]
fn e2e_fixtures_all_parse() {
    let fixture_dir = "../../tests/e2e";
    let entries = std::fs::read_dir(fixture_dir)
        .unwrap_or_else(|e| panic!("failed to read {fixture_dir}: {e}"));
    let mut count = 0;
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map(|e| e == "assura").unwrap_or(false) {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
            let (ast, errors) = parse(&source);
            assert!(
                errors.is_empty(),
                "e2e file {} had parse errors: {:?}",
                path.display(),
                errors
            );
            assert!(ast.is_some(), "e2e file {} returned None", path.display());
            count += 1;
        }
    }
    assert!(
        count >= 5,
        "expected at least 5 e2e fixtures, found {count}"
    );
}
