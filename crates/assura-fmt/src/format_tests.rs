use super::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format via the public API and return the result.
fn fmt(source: &str) -> String {
    format_source(source)
}

/// Assert formatting is idempotent: format(format(x)) == format(x).
fn assert_idempotent(source: &str) {
    let first = fmt(source);
    let second = fmt(&first);
    assert_eq!(first, second, "formatting is not idempotent");
}

// ---------------------------------------------------------------------------
// 1. Comment preservation (core feature of #702)
// ---------------------------------------------------------------------------

#[test]
fn comment_preserved_single_line() {
    let src = "// This is a comment\ncontract Foo { requires { x > 0 } }\n";
    let out = fmt(src);
    assert!(out.contains("// This is a comment"), "comment lost: {out}");
}

#[test]
fn comment_preserved_inline() {
    let src = "contract Foo { // inline comment\n    requires { x > 0 }\n}\n";
    let out = fmt(src);
    assert!(
        out.contains("// inline comment"),
        "inline comment lost: {out}"
    );
}

#[test]
fn comment_preserved_between_declarations() {
    let src = "contract A { requires { x > 0 } }\n\n// Separator comment\n\ncontract B { requires { y > 0 } }\n";
    let out = fmt(src);
    assert!(
        out.contains("// Separator comment"),
        "separator comment lost: {out}"
    );
}

#[test]
fn comment_preserved_inside_braces() {
    let src = "contract Foo {\n    // This clause checks positivity\n    requires { x > 0 }\n}\n";
    let out = fmt(src);
    assert!(
        out.contains("// This clause checks positivity"),
        "inner comment lost: {out}"
    );
}

// ---------------------------------------------------------------------------
// 2. Declaration preservation
// ---------------------------------------------------------------------------

#[test]
fn contract_preserved() {
    let src = "contract Foo { requires { x > 0 } }\n";
    let out = fmt(src);
    assert!(out.contains("contract Foo"));
    assert!(out.contains("requires"));
    assert!(out.contains("x > 0"));
}

#[test]
fn contract_with_ensures() {
    let src = "contract Bar { requires { x > 0 } ensures { result > 0 } }\n";
    let out = fmt(src);
    assert!(out.contains("requires"));
    assert!(out.contains("ensures"));
}

#[test]
fn contract_with_type_params() {
    let src = "contract Generic<T> { requires { x > 0 } }\n";
    let out = fmt(src);
    assert!(out.contains("Generic<T>"));
}

#[test]
fn service_preserved() {
    let src = "service OrderService {\n    states: Created -> Paid -> Shipped\n    operation pay {\n        requires { amount > 0 }\n        ensures { state == Paid }\n    }\n}\n";
    let out = fmt(src);
    assert!(out.contains("service OrderService"));
    assert!(out.contains("states:"));
    assert!(out.contains("operation pay"));
}

#[test]
fn service_with_query() {
    let src = "service DataService {\n    query getItem {\n        requires { id > 0 }\n        ensures { result > 0 }\n    }\n}\n";
    let out = fmt(src);
    assert!(out.contains("service DataService"));
    assert!(out.contains("query getItem"));
}

#[test]
fn type_struct_preserved() {
    let src = "type Point {\n    pub x: Int;\n    pub y: Int;\n}\n";
    let out = fmt(src);
    assert!(out.contains("type Point"));
    assert!(out.contains("pub x: Int;"));
    assert!(out.contains("pub y: Int;"));
}

#[test]
fn type_alias_preserved() {
    let src = "type Age = Int;\n";
    let out = fmt(src);
    assert!(out.contains("type Age = Int;"));
}

#[test]
fn enum_preserved() {
    let src = "enum Color {\n    Red\n    Green\n    Blue\n}\n";
    let out = fmt(src);
    assert!(out.contains("enum Color"));
    assert!(out.contains("Red"));
    assert!(out.contains("Green"));
    assert!(out.contains("Blue"));
}

#[test]
fn enum_with_fields_preserved() {
    let src = "enum Shape {\n    Circle(Int)\n    Rect(Int, Int)\n}\n";
    let out = fmt(src);
    assert!(out.contains("Circle(Int)"));
    assert!(out.contains("Rect("));
}

#[test]
fn generic_type_preserved() {
    let src = "type Pair<A, B> {\n    pub first: A;\n    pub second: B;\n}\n";
    let out = fmt(src);
    assert!(out.contains("Pair<A, B>"));
}

#[test]
fn extern_fn_preserved() {
    let src = "extern fn read_file(path: String) -> String\n";
    let out = fmt(src);
    assert!(out.contains("extern fn read_file(path: String) -> String"));
}

#[test]
fn extern_fn_with_clauses_preserved() {
    let src = "extern fn divide(a: Int, b: Int) -> Int\n    requires { b != 0 }\n    ensures { result * b == a }\n";
    let out = fmt(src);
    assert!(out.contains("extern fn divide"));
    assert!(out.contains("requires"));
    assert!(out.contains("ensures"));
}

#[test]
fn bind_preserved() {
    let src = "bind \"libc::malloc\" as safe_alloc {\n    input(size: Nat)\n    output(result: Bytes)\n    requires { size > 0 }\n}\n";
    let out = fmt(src);
    assert!(out.contains("bind"));
    assert!(out.contains("safe_alloc"));
    assert!(out.contains("input(size: Nat)"));
}

#[test]
fn fn_def_preserved() {
    let src =
        "fn add(a: Int, b: Int) -> Int\n    requires { a >= 0 }\n    ensures { result == a + b }\n";
    let out = fmt(src);
    assert!(out.contains("fn add(a: Int, b: Int) -> Int"));
    assert!(out.contains("requires"));
    assert!(out.contains("ensures"));
}

#[test]
fn ghost_fn_preserved() {
    let src = "ghost fn helper(x: Int) -> Bool\n    requires { x >= 0 }\n";
    let out = fmt(src);
    assert!(out.contains("ghost fn helper"));
}

#[test]
fn prophecy_preserved() {
    let src = "prophecy future_val: Int\ncontract X { requires { true } }\n";
    let out = fmt(src);
    assert!(out.contains("prophecy"));
    assert!(out.contains("future_val"));
}

#[test]
fn import_preserved() {
    let src = "import std.math;\nimport std.collections;\n";
    let out = fmt(src);
    assert!(out.contains("import std.math;"));
    assert!(out.contains("import std.collections;"));
}

#[test]
fn import_with_alias() {
    let src = "import std.math as m;\n";
    let out = fmt(src);
    assert!(out.contains("import std.math as m;"));
}

#[test]
fn import_with_items() {
    let src = "import std.math { abs, max };\n";
    let out = fmt(src);
    assert!(out.contains("import std.math"));
    assert!(out.contains("abs"));
    assert!(out.contains("max"));
}

#[test]
fn module_preserved() {
    let src = "module test.basic;\n";
    let out = fmt(src);
    assert!(out.contains("module test.basic;"));
}

#[test]
fn project_preserved() {
    let src = "project MyProject { profile: [safety, security] }\n";
    let out = fmt(src);
    assert!(out.contains("project MyProject"));
}

// ---------------------------------------------------------------------------
// 3. Expressions and clauses preserved
// ---------------------------------------------------------------------------

#[test]
fn nested_binary_ops_preserved() {
    let src = "contract N { requires { a + b * c > d - e } }\n";
    let out = fmt(src);
    assert!(out.contains("a + b * c > d - e"));
}

#[test]
fn logical_ops_preserved() {
    let src = "contract L { requires { a > 0 && b > 0 || c == 0 } }\n";
    let out = fmt(src);
    assert!(out.contains("&&"));
    assert!(out.contains("||"));
}

#[test]
fn quantifier_preserved() {
    let src = "contract Q { requires { forall i in items: i > 0 } }\n";
    let out = fmt(src);
    assert!(out.contains("forall i in items: i > 0"));
}

#[test]
fn if_then_else_preserved() {
    let src = "contract C { ensures { if x > 0 then result > 0 else result == 0 } }\n";
    let out = fmt(src);
    assert!(out.contains("if x > 0 then result > 0 else result == 0"));
}

#[test]
fn old_expression_preserved() {
    let src = "contract O { ensures { result > old(x) } }\n";
    let out = fmt(src);
    assert!(out.contains("old(x)"));
}

#[test]
fn exists_quantifier_preserved() {
    let src = "contract E { requires { exists x in items: x == target } }\n";
    let out = fmt(src);
    assert!(out.contains("exists x in items: x == target"));
}

#[test]
fn all_clause_kinds_preserved() {
    let src = "contract Full {\n    input(x: Int, y: Int)\n    output(result: Int)\n    requires { x > 0 }\n    ensures { result > 0 }\n    invariant { x >= 0 }\n    effects { io }\n    modifies { state }\n}\n";
    let out = fmt(src);
    assert!(out.contains("input(x: Int, y: Int)"));
    assert!(out.contains("output(result: Int)"));
    assert!(out.contains("requires"));
    assert!(out.contains("ensures"));
    assert!(out.contains("invariant"));
    assert!(out.contains("effects"));
    assert!(out.contains("modifies"));
}

#[test]
fn decreases_clause_preserved() {
    let src = "fn factorial(n: Int) -> Int\n    requires { n >= 0 }\n    decreases { n }\n";
    let out = fmt(src);
    assert!(out.contains("decreases"));
}

// ---------------------------------------------------------------------------
// 4. Indentation
// ---------------------------------------------------------------------------

#[test]
fn indentation_inside_braces() {
    let src = "contract Foo {\nrequires { x > 0 }\n}\n";
    let out = fmt(src);
    for line in out.lines() {
        if line.trim_start().starts_with("requires") {
            assert!(
                line.starts_with("    "),
                "requires should be indented: '{line}'"
            );
        }
    }
}

#[test]
fn closing_brace_dedented() {
    let src = "contract Foo {\n    requires { x > 0 }\n}\n";
    let out = fmt(src);
    let last_brace_line = out.lines().rev().find(|l| l.trim() == "}");
    assert_eq!(
        last_brace_line,
        Some("}"),
        "closing brace should be at column 0"
    );
}

#[test]
fn nested_indentation() {
    let src = "service S {\noperation op {\nrequires { x > 0 }\n}\n}\n";
    let out = fmt(src);
    for line in out.lines() {
        if line.trim_start().starts_with("requires") {
            assert!(
                line.starts_with("        "),
                "nested requires should be double-indented: '{line}'"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 5. Idempotency
// ---------------------------------------------------------------------------

#[test]
fn idempotent_contract() {
    assert_idempotent("contract Foo { requires { x > 0 } ensures { result > 0 } }\n");
}

#[test]
fn idempotent_service() {
    assert_idempotent(
        "service S {\n    states: A -> B -> C\n    operation go {\n        requires { x > 0 }\n    }\n}\n",
    );
}

#[test]
fn idempotent_type_and_enum() {
    assert_idempotent(
        "type Point {\n    pub x: Int;\n    pub y: Int;\n}\n\nenum Color {\n    Red\n    Green\n    Blue\n}\n",
    );
}

#[test]
fn idempotent_extern() {
    assert_idempotent("extern fn do_thing(a: Int) -> Bool\n");
}

#[test]
fn idempotent_bind() {
    assert_idempotent(
        "bind \"lib::func\" as wrapper {\n    input(x: Int)\n    output(result: Bool)\n    requires { x >= 0 }\n}\n",
    );
}

#[test]
fn idempotent_all_clauses() {
    assert_idempotent(
        "contract Full {\n    input(x: Int, y: Int)\n    output(result: Int)\n    requires { x > 0 }\n    ensures { result > 0 }\n    invariant { x >= 0 }\n    effects { io }\n}\n",
    );
}

#[test]
fn idempotent_with_comments() {
    assert_idempotent(
        "// Top-level comment\ncontract Foo {\n    // Inner comment\n    requires { x > 0 }\n}\n",
    );
}

#[test]
fn idempotent_fn_with_effects() {
    assert_idempotent("fn read_data(path: String) -> Bytes\n    effects { io }\n");
}

// ---------------------------------------------------------------------------
// 6. Edge cases
// ---------------------------------------------------------------------------

#[test]
fn empty_source() {
    let out = fmt("");
    assert!(
        out.len() <= 1,
        "empty source should produce minimal output: '{out}'"
    );
}

#[test]
fn parse_error_returns_original() {
    let src = "contract { missing name and stuff ???";
    let out = fmt(src);
    assert_eq!(out, src, "parse errors should return source unchanged");
}

#[test]
fn trailing_newline_normalized() {
    let src = "contract Foo { requires { x > 0 } }\n\n\n";
    let out = fmt(src);
    assert!(out.ends_with('\n'), "should end with newline");
    assert!(
        !out.ends_with("\n\n"),
        "should not end with multiple newlines"
    );
}

#[test]
fn no_trailing_whitespace() {
    let src = "contract Foo {   \n    requires { x > 0 }   \n}   \n";
    let out = fmt(src);
    for (i, line) in out.lines().enumerate() {
        assert_eq!(
            line,
            line.trim_end(),
            "trailing whitespace on line {i}: '{line}'"
        );
    }
}

// ---------------------------------------------------------------------------
// 7. Re-parseability
// ---------------------------------------------------------------------------

#[test]
fn formatted_output_reparses() {
    let src = "contract SafeDivide {\n    input(a: Int, b: Int)\n    output(result: Int)\n    requires { b != 0 }\n    ensures { result == a / b }\n}\n";
    let formatted = fmt(src);
    let (file2, errs2) = assura_parser::parse(&formatted);
    assert!(
        errs2.is_empty(),
        "formatted output should re-parse: {errs2:?}"
    );
    assert!(
        file2.is_some(),
        "formatted output should produce a SourceFile"
    );
}

// ---------------------------------------------------------------------------
// 8. Multiple declarations ordering
// ---------------------------------------------------------------------------

#[test]
fn multiple_declarations_order_preserved() {
    let src = "contract First { requires { a > 0 } }\n\ncontract Second { requires { b > 0 } }\n\ncontract Third { requires { c > 0 } }\n";
    let out = fmt(src);
    let first_pos = out.find("contract First").unwrap();
    let second_pos = out.find("contract Second").unwrap();
    let third_pos = out.find("contract Third").unwrap();
    assert!(first_pos < second_pos);
    assert!(second_pos < third_pos);
}

// ---------------------------------------------------------------------------
// 9. Blank line capping
// ---------------------------------------------------------------------------

#[test]
fn excessive_blank_lines_capped() {
    let src = "contract A { requires { x > 0 } }\n\n\n\n\n\ncontract B { requires { y > 0 } }\n";
    let out = fmt(src);
    assert!(
        !out.contains("\n\n\n\n"),
        "too many consecutive blank lines in: {out}"
    );
}

// ---------------------------------------------------------------------------
// 10. Service with invariant
// ---------------------------------------------------------------------------

#[test]
fn service_with_invariant() {
    let src = "service Counter {\n    states: Zero -> Positive\n\n    invariant { count >= 0 }\n\n    operation increment {\n        ensures { count > 0 }\n    }\n}\n";
    let out = fmt(src);
    assert!(out.contains("invariant"), "got: {out}");
}
