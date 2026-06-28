use super::super::*;

// TEST.1: TestGenerator pipeline integration
// ==========================================================================

#[test]
fn test_generator_populates_typed_file() {
    let src = r#"
contract SafeDiv {
    input(a: Int, b: Int)
    requires { b > 0 }
    ensures { result >= 0 }
}
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(resolved).expect("should type check");
    assert!(
        !typed.generated_tests.is_empty(),
        "TypedFile should contain generated tests for a contract with requires/ensures"
    );
    // Should have property + boundary + smoke tests
    let has_property = typed
        .generated_tests
        .iter()
        .any(|t| t.kind == TestKind::Property);
    let has_boundary = typed
        .generated_tests
        .iter()
        .any(|t| t.kind == TestKind::Boundary);
    let has_smoke = typed
        .generated_tests
        .iter()
        .any(|t| t.kind == TestKind::Smoke);
    assert!(has_property, "should have property test");
    assert!(has_boundary, "should have boundary test");
    assert!(has_smoke, "should have smoke test");
}

#[test]
fn test_generator_empty_for_no_constraints() {
    let src = r#"
contract Empty {
    input(x: Int)
}
"#;
    let resolved = resolve_ok(src);
    let typed = type_check(resolved).expect("should type check");
    assert!(
        typed.generated_tests.is_empty(),
        "contract with no requires/ensures should produce no tests"
    );
}
