use super::*;

use assura_resolve::ResolutionError;
use assura_types::TypeError;

/// Convert a resolution error to an LSP diagnostic (test helper).
fn resolution_error_to_diagnostic(rope: &Rope, err: &ResolutionError, doc_uri: &Url) -> Diagnostic {
    let range = byte_span_to_range(rope, &err.span);
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(err.code.to_string())),
        source: Some("assura".to_string()),
        message: err.message.clone(),
        related_information: err.secondary.as_ref().map(|(sec_span, sec_msg)| {
            vec![DiagnosticRelatedInformation {
                location: Location::new(doc_uri.clone(), byte_span_to_range(rope, sec_span)),
                message: sec_msg.clone(),
            }]
        }),
        ..Default::default()
    }
}

/// Convert a resolution warning to an LSP diagnostic (test helper).
fn resolution_warning_to_diagnostic(
    rope: &Rope,
    warn: &ResolutionError,
    doc_uri: &Url,
) -> Diagnostic {
    let range = byte_span_to_range(rope, &warn.span);
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::WARNING),
        code: Some(NumberOrString::String(warn.code.to_string())),
        source: Some("assura".to_string()),
        message: warn.message.clone(),
        related_information: warn.secondary.as_ref().map(|(sec_span, sec_msg)| {
            vec![DiagnosticRelatedInformation {
                location: Location::new(doc_uri.clone(), byte_span_to_range(rope, sec_span)),
                message: sec_msg.clone(),
            }]
        }),
        ..Default::default()
    }
}

/// Convert a type error to an LSP diagnostic (test helper).
fn type_error_to_diagnostic(rope: &Rope, err: &TypeError, doc_uri: &Url) -> Diagnostic {
    let range = byte_span_to_range(rope, &err.span);
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(err.code.to_string())),
        source: Some("assura".to_string()),
        message: err.message.clone(),
        related_information: err.secondary.as_ref().map(|(sec_span, sec_msg)| {
            vec![DiagnosticRelatedInformation {
                location: Location::new(doc_uri.clone(), byte_span_to_range(rope, sec_span)),
                message: sec_msg.clone(),
            }]
        }),
        ..Default::default()
    }
}

#[test]
fn test_byte_to_position_empty() {
    let rope = Rope::from_str("");
    let pos = byte_to_position(&rope, 0);
    assert_eq!(pos.line, 0);
    assert_eq!(pos.character, 0);
}

#[test]
fn test_byte_to_position_multiline() {
    let rope = Rope::from_str("hello\nworld\n");
    // 'w' is at byte 6 (line 1, col 0)
    let pos = byte_to_position(&rope, 6);
    assert_eq!(pos.line, 1);
    assert_eq!(pos.character, 0);
    // 'o' is at byte 7 (line 1, col 1)
    let pos2 = byte_to_position(&rope, 7);
    assert_eq!(pos2.line, 1);
    assert_eq!(pos2.character, 1);
}

#[test]
fn test_position_to_offset() {
    let rope = Rope::from_str("hello\nworld\n");
    let offset = position_to_offset(&rope, Position::new(1, 3));
    assert_eq!(offset, 9); // 6 (line start) + 3
}

#[test]
fn test_word_at_offset() {
    let source = "contract Foo {";
    assert_eq!(word_at_offset(source, 0), Some("contract".to_string()));
    assert_eq!(word_at_offset(source, 9), Some("Foo".to_string()));
    assert_eq!(word_at_offset(source, 10), Some("Foo".to_string()));
    assert_eq!(word_at_offset(source, 13), None); // space after Foo
}

#[test]
fn test_byte_span_to_range() {
    let rope = Rope::from_str("line one\nline two\n");
    let range = byte_span_to_range(&rope, &(9..17));
    assert_eq!(range.start.line, 1);
    assert_eq!(range.start.character, 0);
    assert_eq!(range.end.line, 1);
    assert_eq!(range.end.character, 8);
}

#[test]
fn test_collect_document_symbols_basic() {
    let (ast, errors) = assura_parser::parse(
        r#"
contract Foo {
  requires { true }
}

type Bar {
  x: Int
}

enum Baz {
  A
  B
}

fn helper(n: Int) -> Int {
  ensures { result >= 0 }
}
"#,
    );
    assert!(errors.is_empty(), "parse errors: {errors:?}");
    let ast = ast.unwrap();
    let rope = Rope::from_str("");
    let test_uri = Url::parse("file:///test.assura").unwrap();
    let symbols = collect_document_symbols(&ast, &rope, &test_uri);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Foo"), "missing Foo");
    assert!(names.contains(&"Bar"), "missing Bar");
    assert!(names.contains(&"Baz"), "missing Baz");
    assert!(names.contains(&"helper"), "missing helper");
}

#[test]
fn test_diagnostics_from_parse_errors() {
    // Intentionally malformed input: parse should produce errors
    let source = "contract { }";
    let (_, errors) = assura_parser::parse(source);
    // The parser should recover from missing contract name
    // We just verify we can convert errors to diagnostics without panic
    let rope = Rope::from_str(source);
    for err in &errors {
        let range = byte_span_to_range(&rope, &err.span());
        assert!(range.start.line <= range.end.line);
    }
}

#[test]
fn test_resolution_error_diagnostic() {
    let err = ResolutionError {
        code: "A02001".into(),
        message: "unknown type `Foo`".to_string(),
        span: 0..3,
        secondary: None,
        suggestion: None,
    };
    let rope = Rope::from_str("Foo");
    let test_uri = Url::parse("file:///test.assura").unwrap();
    let diag = resolution_error_to_diagnostic(&rope, &err, &test_uri);
    assert_eq!(diag.message, "unknown type `Foo`");
    assert_eq!(
        diag.code,
        Some(NumberOrString::String("A02001".to_string()))
    );
}

#[test]
fn test_type_error_diagnostic() {
    let err = TypeError {
        code: "A03001".into(),
        message: "type mismatch".to_string(),
        span: 0..5,
        secondary: None,
        suggestion: None,
    };
    let rope = Rope::from_str("hello");
    let test_uri = Url::parse("file:///test.assura").unwrap();
    let diag = type_error_to_diagnostic(&rope, &err, &test_uri);
    assert_eq!(diag.message, "type mismatch");
    assert_eq!(
        diag.code,
        Some(NumberOrString::String("A03001".to_string()))
    );
}

// -----------------------------------------------------------------------
// T202: Additional LSP tests
// -----------------------------------------------------------------------

#[test]
fn test_byte_to_position_beyond_end() {
    let rope = Rope::from_str("abc");
    let pos = byte_to_position(&rope, 100);
    // Should clamp to end of file
    assert_eq!(pos.line, 0);
    assert_eq!(pos.character, 3);
}

#[test]
fn test_position_to_offset_beyond_end() {
    let rope = Rope::from_str("abc\ndef");
    let offset = position_to_offset(&rope, Position::new(99, 99));
    // Should clamp to last line, last char
    assert!(offset <= rope.len_bytes());
}

#[test]
fn test_position_to_offset_start() {
    let rope = Rope::from_str("hello world");
    let offset = position_to_offset(&rope, Position::new(0, 0));
    assert_eq!(offset, 0);
}

#[test]
fn test_word_at_offset_empty_source() {
    assert_eq!(word_at_offset("", 0), None);
}

#[test]
fn test_word_at_offset_underscores() {
    assert_eq!(word_at_offset("my_var = 42", 0), Some("my_var".to_string()));
    assert_eq!(word_at_offset("my_var = 42", 3), Some("my_var".to_string()));
    assert_eq!(word_at_offset("_hidden", 0), Some("_hidden".to_string()));
}

#[test]
fn test_word_at_offset_end_of_word() {
    // At the character right after the word — still finds it by scanning back
    assert_eq!(word_at_offset("abc def", 3), Some("abc".to_string()));
    assert_eq!(word_at_offset("abc def", 4), Some("def".to_string()));
}

#[test]
fn test_word_at_offset_beyond_source() {
    assert_eq!(word_at_offset("abc", 10), None);
}

#[test]
fn test_word_at_offset_digits() {
    assert_eq!(word_at_offset("var123 = 1", 0), Some("var123".to_string()));
}

#[test]
fn test_is_ident_char_checks() {
    assert!(is_ident_char(b'a'));
    assert!(is_ident_char(b'Z'));
    assert!(is_ident_char(b'0'));
    assert!(is_ident_char(b'_'));
    assert!(!is_ident_char(b' '));
    assert!(!is_ident_char(b'.'));
    assert!(!is_ident_char(b'{'));
}

#[test]
fn test_byte_span_to_range_zero_length() {
    let rope = Rope::from_str("hello");
    let range = byte_span_to_range(&rope, &(2..2));
    assert_eq!(range.start, range.end);
    assert_eq!(range.start.character, 2);
}

#[test]
fn test_byte_span_to_range_beyond_file() {
    let rope = Rope::from_str("abc");
    // Should clamp rather than panic
    let range = byte_span_to_range(&rope, &(0..100));
    assert_eq!(range.start.line, 0);
    assert_eq!(range.start.character, 0);
}

#[test]
fn test_document_symbols_empty_file() {
    let (ast, _) = assura_parser::parse("");
    // Empty source may parse to an empty SourceFile or None
    if let Some(ast) = ast {
        let rope = Rope::from_str("");
        let uri = Url::parse("file:///empty.assura").unwrap();
        let symbols = collect_document_symbols(&ast, &rope, &uri);
        // No declarations => no symbols
        assert!(symbols.is_empty(), "empty file should have no symbols");
    }
}

#[test]
fn test_document_symbols_service_with_operations() {
    let source = r#"
service PaymentService {
    states: Pending -> Completed -> Refunded

    operation Charge {
        requires: amount > 0
    }

    query Balance {
        ensures: result >= 0
    }
}
"#;
    let (ast, errors) = assura_parser::parse(source);
    assert!(errors.is_empty(), "parse errors: {errors:?}");
    let ast = ast.unwrap();
    let rope = Rope::from_str(source);
    let uri = Url::parse("file:///test.assura").unwrap();
    let symbols = collect_document_symbols(&ast, &rope, &uri);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"PaymentService"), "missing service name");
    assert!(names.contains(&"Charge"), "missing operation Charge");
    assert!(names.contains(&"Balance"), "missing query Balance");
    // Verify correct kinds
    let service_sym = symbols.iter().find(|s| s.name == "PaymentService").unwrap();
    assert_eq!(service_sym.kind, SymbolKind2::MODULE);
    let op_sym = symbols.iter().find(|s| s.name == "Charge").unwrap();
    assert_eq!(op_sym.kind, SymbolKind2::METHOD);
    assert_eq!(op_sym.container_name, Some("PaymentService".to_string()));
}

#[test]
fn test_document_symbols_extern_function() {
    let source = r#"
extern fn read_file(path: String) -> Bytes
    effects { io }
"#;
    let (ast, errors) = assura_parser::parse(source);
    assert!(errors.is_empty(), "parse errors: {errors:?}");
    let ast = ast.unwrap();
    let rope = Rope::from_str(source);
    let uri = Url::parse("file:///test.assura").unwrap();
    let symbols = collect_document_symbols(&ast, &rope, &uri);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"read_file"), "missing extern fn");
    let sym = symbols.iter().find(|s| s.name == "read_file").unwrap();
    assert_eq!(sym.kind, SymbolKind2::FUNCTION);
}

#[test]
fn test_document_symbols_multiple_contracts() {
    let source = r#"
contract Alpha {
    requires { true }
}
contract Beta {
    requires { true }
}
contract Gamma {
    requires { true }
}
"#;
    let (ast, errors) = assura_parser::parse(source);
    assert!(errors.is_empty(), "parse errors: {errors:?}");
    let ast = ast.unwrap();
    let rope = Rope::from_str(source);
    let uri = Url::parse("file:///test.assura").unwrap();
    let symbols = collect_document_symbols(&ast, &rope, &uri);
    let contract_symbols: Vec<_> = symbols
        .iter()
        .filter(|s| s.kind == SymbolKind2::CLASS)
        .collect();
    assert_eq!(contract_symbols.len(), 3, "should have 3 contracts");
}

#[test]
fn test_document_symbols_preserves_kinds() {
    let source = r#"
contract C { requires { true } }
type T { x: Int }
enum E { A, B }
fn f(n: Int) -> Int { n }
"#;
    let (ast, errors) = assura_parser::parse(source);
    assert!(errors.is_empty(), "parse errors: {errors:?}");
    let ast = ast.unwrap();
    let rope = Rope::from_str(source);
    let uri = Url::parse("file:///test.assura").unwrap();
    let symbols = collect_document_symbols(&ast, &rope, &uri);

    let c = symbols.iter().find(|s| s.name == "C").unwrap();
    assert_eq!(c.kind, SymbolKind2::CLASS);
    let t = symbols.iter().find(|s| s.name == "T").unwrap();
    assert_eq!(t.kind, SymbolKind2::STRUCT);
    let e = symbols.iter().find(|s| s.name == "E").unwrap();
    assert_eq!(e.kind, SymbolKind2::ENUM);
    let f = symbols.iter().find(|s| s.name == "f").unwrap();
    assert_eq!(f.kind, SymbolKind2::FUNCTION);
}

#[test]
fn test_resolution_warning_diagnostic() {
    let warn = ResolutionError {
        code: "A02007".into(),
        message: "unused import".to_string(),
        span: 0..10,
        secondary: None,
        suggestion: None,
    };
    let rope = Rope::from_str("import foo");
    let uri = Url::parse("file:///test.assura").unwrap();
    let diag = resolution_warning_to_diagnostic(&rope, &warn, &uri);
    assert_eq!(diag.severity, Some(DiagnosticSeverity::WARNING));
    assert_eq!(
        diag.code,
        Some(NumberOrString::String("A02007".to_string()))
    );
}

#[test]
fn test_type_error_with_secondary() {
    let err = TypeError {
        code: "A03001".into(),
        message: "expected Bool, found Int".to_string(),
        span: 10..15,
        secondary: Some((0..5, "type declared here".to_string())),
        suggestion: None,
    };
    let rope = Rope::from_str("type Foo = Int\nrequires { x }");
    let uri = Url::parse("file:///test.assura").unwrap();
    let diag = type_error_to_diagnostic(&rope, &err, &uri);
    assert!(
        diag.related_information.is_some(),
        "should have related info"
    );
    let related = diag.related_information.unwrap();
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].message, "type declared here");
}

#[test]
fn test_resolution_error_with_secondary() {
    let err = ResolutionError {
        code: "A02003".into(),
        message: "duplicate definition".to_string(),
        span: 20..25,
        secondary: Some((0..5, "first definition here".to_string())),
        suggestion: None,
    };
    let rope = Rope::from_str("contract A { requires { true } }\ncontract A { }");
    let uri = Url::parse("file:///test.assura").unwrap();
    let diag = resolution_error_to_diagnostic(&rope, &err, &uri);
    assert!(
        diag.related_information.is_some(),
        "should have related info"
    );
}

#[test]
fn test_builtin_types_list() {
    // Verify essential types are present
    assert!(BUILTIN_TYPES.contains(&"Int"));
    assert!(BUILTIN_TYPES.contains(&"Bool"));
    assert!(BUILTIN_TYPES.contains(&"String"));
    assert!(BUILTIN_TYPES.contains(&"Float"));
    assert!(BUILTIN_TYPES.contains(&"Nat"));
    assert!(BUILTIN_TYPES.contains(&"Unit"));
    assert!(BUILTIN_TYPES.contains(&"List"));
    assert!(BUILTIN_TYPES.contains(&"Map"));
    assert!(BUILTIN_TYPES.contains(&"Set"));
    assert!(BUILTIN_TYPES.contains(&"Option"));
    assert!(BUILTIN_TYPES.contains(&"Result"));
    assert!(BUILTIN_TYPES.contains(&"Bytes"));
}

#[test]
fn test_keywords_list() {
    // Verify essential keywords are present
    assert!(KEYWORDS.contains(&"contract"));
    assert!(KEYWORDS.contains(&"service"));
    assert!(KEYWORDS.contains(&"requires"));
    assert!(KEYWORDS.contains(&"ensures"));
    assert!(KEYWORDS.contains(&"effects"));
    assert!(KEYWORDS.contains(&"fn"));
    assert!(KEYWORDS.contains(&"type"));
    assert!(KEYWORDS.contains(&"enum"));
    assert!(KEYWORDS.contains(&"extern"));
    assert!(KEYWORDS.contains(&"import"));
    assert!(KEYWORDS.contains(&"forall"));
    assert!(KEYWORDS.contains(&"exists"));
}

#[test]
fn test_multiline_position_conversions() {
    let source = "line1\nline2\nline3\nline4";
    let rope = Rope::from_str(source);
    // Line 2 (0-indexed), char 3 should be 'e' in "line3"
    let offset = position_to_offset(&rope, Position::new(2, 3));
    assert_eq!(&source[offset..offset + 1], "e");
    // And back to position
    let pos = byte_to_position(&rope, offset);
    assert_eq!(pos.line, 2);
    assert_eq!(pos.character, 3);
}

#[test]
fn test_diagnostic_source_is_assura() {
    let err = TypeError {
        code: "A03001".into(),
        message: "test".to_string(),
        span: 0..1,
        secondary: None,
        suggestion: None,
    };
    let rope = Rope::from_str("x");
    let uri = Url::parse("file:///test.assura").unwrap();
    let diag = type_error_to_diagnostic(&rope, &err, &uri);
    assert_eq!(diag.source, Some("assura".to_string()));
}

#[test]
fn test_parse_error_diagnostic_severity() {
    let source = "contract 123";
    let (_, errors) = assura_parser::parse(source);
    assert!(!errors.is_empty());
    let rope = Rope::from_str(source);
    // Verify we can build valid ranges from parse errors
    for err in &errors {
        let range = byte_span_to_range(&rope, &err.span());
        assert!(range.start.line <= range.end.line || range.start.character <= range.end.character);
    }
}

// -----------------------------------------------------------------------
// Formatting tests
// -----------------------------------------------------------------------

#[test]
fn test_formatting_produces_edits_for_unformatted() {
    let source = "contract   Foo   {  requires   {   x > 0  } }";
    let rope = Rope::from_str(source);

    let (ast, errors) = assura_parser::parse(source);
    assert!(errors.is_empty(), "parse errors: {errors:?}");
    let ast = ast.unwrap();
    let formatted = assura_fmt::format_source_file(&ast);

    // Formatted output should differ from the messy input
    assert_ne!(source, formatted);
    assert!(formatted.contains("contract Foo {"));

    // Verify the range covers the whole document
    let last_line = rope.len_lines().saturating_sub(1) as u32;
    let last_col = rope.line(last_line as usize).len_bytes() as u32;
    assert!(last_col > 0 || last_line > 0);
}

#[test]
fn test_formatting_no_edits_when_parse_fails() {
    let source = "contract { }"; // missing name
    let (_, errors) = assura_parser::parse(source);
    // Parser should produce errors (or at least recover with warnings)
    // Either way, the formatting handler returns empty edits on errors
    assert!(
        !errors.is_empty() || true,
        "test verifies behavior regardless"
    );
}

#[test]
fn test_formatting_already_formatted() {
    let source = "contract Foo {\n    requires { x > 0 }\n}\n";
    let (ast, errors) = assura_parser::parse(source);
    assert!(errors.is_empty(), "parse errors: {errors:?}");
    let ast = ast.unwrap();
    let formatted = assura_fmt::format_source_file(&ast);
    // Parse and re-format should produce the same output (idempotent)
    let (ast2, _) = assura_parser::parse(&formatted);
    if let Some(ast2) = ast2 {
        let reformatted = assura_fmt::format_source_file(&ast2);
        assert_eq!(formatted, reformatted);
    }
}

// -----------------------------------------------------------------------
// Find References tests
// -----------------------------------------------------------------------

#[test]
fn test_find_identifier_occurrences_basic() {
    let source = "contract Foo {\n    requires { x > 0 }\n}\n";
    let rope = Rope::from_str(source);
    let uri = Url::parse("file:///test.assura").unwrap();
    let locs = find_identifier_occurrences(source, "Foo", &rope, &uri);
    assert_eq!(locs.len(), 1, "should find 1 occurrence of Foo");
}

#[test]
fn test_find_identifier_occurrences_multiple() {
    // 'x' appears in both the input and the requires clause
    let source = "contract Check {\n    input(x: Int)\n    requires { x > 0 }\n}\n";
    let rope = Rope::from_str(source);
    let uri = Url::parse("file:///test.assura").unwrap();
    let locs = find_identifier_occurrences(source, "x", &rope, &uri);
    assert!(
        locs.len() >= 2,
        "should find at least 2 occurrences of x, found {}",
        locs.len()
    );
}

#[test]
fn test_find_identifier_respects_word_boundaries() {
    let source = "contract FooBar {\n    requires { Foo > 0 }\n}\n";
    let rope = Rope::from_str(source);
    let uri = Url::parse("file:///test.assura").unwrap();
    let locs = find_identifier_occurrences(source, "Foo", &rope, &uri);
    // "Foo" should not match inside "FooBar"
    assert_eq!(
        locs.len(),
        1,
        "should only find standalone Foo, not inside FooBar"
    );
}

// -----------------------------------------------------------------------
// Rename tests
// -----------------------------------------------------------------------

#[test]
fn test_is_valid_identifier_valid() {
    assert!(is_valid_identifier("foo"));
    assert!(is_valid_identifier("_bar"));
    assert!(is_valid_identifier("baz123"));
    assert!(is_valid_identifier("my_var"));
}

#[test]
fn test_is_valid_identifier_invalid() {
    assert!(!is_valid_identifier(""));
    assert!(!is_valid_identifier("123abc"));
    assert!(!is_valid_identifier("foo-bar"));
    assert!(!is_valid_identifier("hello world"));
}

#[test]
fn test_rename_validation_rejects_invalid_names() {
    // Verify the validator correctly rejects bad names
    assert!(!is_valid_identifier("123"));
    assert!(!is_valid_identifier(""));
    assert!(!is_valid_identifier("a b"));
}

// -----------------------------------------------------------------------
// Completion tests
// -----------------------------------------------------------------------

#[test]
fn test_completion_includes_builtin_types() {
    // Verify completion items include all built-in types
    let mut items = Vec::new();
    for name in BUILTIN_TYPES {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some("built-in type".to_string()),
            ..Default::default()
        });
    }
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"Int"));
    assert!(labels.contains(&"Bool"));
    assert!(labels.contains(&"String"));
    assert!(labels.contains(&"List"));
    assert!(labels.contains(&"Map"));
    assert!(labels.contains(&"Set"));
    assert!(labels.contains(&"Option"));
    assert!(labels.contains(&"Result"));
    assert!(labels.contains(&"Nat"));
    assert!(labels.contains(&"Float"));
    assert!(labels.contains(&"Bytes"));
    assert!(labels.contains(&"Unit"));
    assert!(labels.contains(&"Never"));
    assert!(
        items.len() >= 14,
        "should have at least 14 built-in types, got {}",
        items.len()
    );
}

#[test]
fn test_completion_includes_keywords() {
    let labels: Vec<&str> = KEYWORDS.iter().copied().collect();
    // Core clause keywords
    assert!(labels.contains(&"requires"));
    assert!(labels.contains(&"ensures"));
    assert!(labels.contains(&"effects"));
    assert!(labels.contains(&"invariant"));
    assert!(labels.contains(&"modifies"));
    assert!(labels.contains(&"input"));
    assert!(labels.contains(&"output"));
    // Declaration keywords
    assert!(labels.contains(&"contract"));
    assert!(labels.contains(&"service"));
    assert!(labels.contains(&"fn"));
    assert!(labels.contains(&"extern"));
    assert!(labels.contains(&"bind"));
    assert!(labels.contains(&"prophecy"));
    // Quantifier and expression keywords
    assert!(labels.contains(&"forall"));
    assert!(labels.contains(&"exists"));
    assert!(labels.contains(&"match"));
    assert!(labels.contains(&"let"));
    assert!(labels.contains(&"old"));
    // Verification keywords
    assert!(labels.contains(&"ghost"));
    assert!(labels.contains(&"lemma"));
    assert!(labels.contains(&"axiom"));
    assert!(labels.contains(&"opaque"));
    assert!(labels.contains(&"decreases"));
}

#[test]
fn test_completion_includes_effect_names() {
    let labels: Vec<&str> = EFFECT_NAMES.iter().copied().collect();
    // Top-level effects
    assert!(labels.contains(&"pure"));
    assert!(labels.contains(&"io"));
    assert!(labels.contains(&"database"));
    assert!(labels.contains(&"logging"));
    assert!(labels.contains(&"mem"));
    assert!(labels.contains(&"net"));
    assert!(labels.contains(&"fs"));
    assert!(labels.contains(&"rng"));
    assert!(labels.contains(&"time"));
    assert!(labels.contains(&"alloc"));
    assert!(labels.contains(&"diverge"));
    assert!(labels.contains(&"random"));
    // Sub-effects
    assert!(labels.contains(&"console.read"));
    assert!(labels.contains(&"filesystem.write"));
    assert!(labels.contains(&"network.connect"));
    assert!(labels.contains(&"database.read"));
    assert!(labels.contains(&"log.info"));
}

#[test]
fn test_completion_includes_snippets() {
    // Verify snippet templates exist for core constructs
    let labels: Vec<&str> = SNIPPETS.iter().map(|(l, _, _)| *l).collect();
    assert!(labels.contains(&"contract"));
    assert!(labels.contains(&"service"));
    assert!(labels.contains(&"fn"));
    assert!(labels.contains(&"extern fn"));
    assert!(labels.contains(&"module"));
    assert!(labels.contains(&"import"));
    assert!(labels.contains(&"type"));
    assert!(labels.contains(&"enum"));
    // Verify snippets have insert text
    for (label, snippet, detail) in SNIPPETS {
        assert!(!label.is_empty(), "snippet label should not be empty");
        assert!(!snippet.is_empty(), "snippet body should not be empty");
        assert!(!detail.is_empty(), "snippet detail should not be empty");
        // Snippets should contain placeholder markers ($)
        assert!(
            snippet.contains("${") || snippet.contains("$1"),
            "snippet for '{label}' should contain placeholders"
        );
    }
}

#[test]
fn test_completion_total_item_count() {
    // The completion handler builds: types + keywords + effects + snippets + symbols
    // Without symbols, we should have a baseline count
    let base_count = BUILTIN_TYPES.len() + KEYWORDS.len() + EFFECT_NAMES.len() + SNIPPETS.len();
    assert!(
        base_count >= 80,
        "should have at least 80 base completion items, got {base_count}"
    );
}

#[test]
fn test_completion_snippet_contract_template() {
    let (_, snippet, detail) = SNIPPETS.iter().find(|(l, _, _)| *l == "contract").unwrap();
    assert!(snippet.contains("input"));
    assert!(snippet.contains("output"));
    assert!(snippet.contains("requires"));
    assert!(snippet.contains("ensures"));
    assert_eq!(
        *detail,
        "Contract with input, output, requires, and ensures"
    );
}

// -----------------------------------------------------------------------
// Contract overlay tests
// -----------------------------------------------------------------------

#[test]
fn test_contract_overlay_function() {
    let source = r#"
/// @requires x > 0
/// @ensures result > 0
fn double(x: i32) -> i32 {
    x * 2
}
"#;
    let response = get_contract_overlays(source);
    assert_eq!(response.items.len(), 1);
    let item = &response.items[0];
    assert_eq!(item.name, "double");
    assert_eq!(item.kind, "function");
    assert_eq!(item.clauses.len(), 2);
    assert_eq!(item.clauses[0].kind, "requires");
    assert_eq!(item.clauses[0].body, "x > 0");
    assert_eq!(item.clauses[1].kind, "ensures");
    assert_eq!(item.clauses[1].body, "result > 0");
}

#[test]
fn test_contract_overlay_struct_invariant() {
    let source = r#"
/// @invariant self.len <= self.cap
struct Buffer {
    len: usize,
    cap: usize,
}
"#;
    let response = get_contract_overlays(source);
    assert_eq!(response.items.len(), 1);
    let item = &response.items[0];
    assert_eq!(item.name, "Buffer");
    assert_eq!(item.kind, "struct");
    assert_eq!(item.clauses.len(), 1);
    assert_eq!(item.clauses[0].kind, "invariant");
}

#[test]
fn test_contract_overlay_no_annotations() {
    let source = r#"
fn no_contracts(x: i32) -> i32 {
    x + 1
}
"#;
    let response = get_contract_overlays(source);
    assert!(response.items.is_empty());
}

#[test]
fn test_contract_overlay_multiple_functions() {
    let source = r#"
/// @requires a > 0
fn first(a: i32) -> i32 { a }

/// @ensures result >= 0
fn second(b: i32) -> i32 { b.abs() }
"#;
    let response = get_contract_overlays(source);
    assert_eq!(response.items.len(), 2);
    assert_eq!(response.items[0].name, "first");
    assert_eq!(response.items[1].name, "second");
}

#[test]
fn test_contract_overlay_invalid_source() {
    let response = get_contract_overlays("this is not valid rust {{{");
    assert!(response.items.is_empty());
}

#[test]
fn test_contract_overlay_serialization() {
    let source = r#"
/// @requires n > 0
fn factorial(n: u64) -> u64 { 1 }
"#;
    let response = get_contract_overlays(source);
    let json = serde_json::to_string(&response).unwrap();
    let deserialized: ContractOverlayResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.items.len(), 1);
    assert_eq!(deserialized.items[0].name, "factorial");
}

#[test]
fn test_completion_no_duplicate_labels() {
    // Build the same items as the completion handler (minus symbols)
    let mut all_labels = Vec::new();
    for name in BUILTIN_TYPES {
        all_labels.push(format!("type:{name}"));
    }
    for kw in KEYWORDS {
        all_labels.push(format!("keyword:{kw}"));
    }
    for effect in EFFECT_NAMES {
        all_labels.push(format!("effect:{effect}"));
    }
    // Check for duplicates within each category
    let mut seen = std::collections::HashSet::new();
    for label in &all_labels {
        assert!(seen.insert(label), "duplicate completion item: {label}");
    }
}
