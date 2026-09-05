use super::*;

fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
    let (sf, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    sf.unwrap()
}

#[test]
fn no_effects_clause_no_errors() {
    let sf = parse_source(r#"contract Simple { requires { true } }"#);
    assert!(run_effect_checks(&sf).is_empty());
}

#[test]
fn declared_effects_with_io_no_error() {
    let sf = parse_source(r#"contract WithIo { effects { io } requires { true } }"#);
    let errs = run_effect_checks(&sf);
    assert!(
        !errs.iter().any(|e| e.code == "A07003"),
        "unexpected undeclared effect error: {errs:?}"
    );
}

#[test]
fn test_effect_polymorphism_basic() {
    // Effect row with a variable: `effects <io | E>`
    // The variable E should NOT produce A07003 (unknown effect)
    let sf = parse_source(
        r#"contract EffPoly {
            effects <io | E>
            fn map_with_effect(f: (Int) -> Int) -> List<Int>
        }"#,
    );
    let errs = run_effect_checks(&sf);
    let a07003_errors: Vec<_> = errs.iter().filter(|e| e.code == "A07003").collect();
    assert!(
        a07003_errors.is_empty(),
        "effect variable E should not produce A07003, got: {a07003_errors:?}"
    );
}

#[test]
fn must_not_io_with_effects_io_a07003() {
    let sf = parse_source(
        r#"contract Forbidden {
            effects { io }
            must-not { io }
            requires { true }
        }"#,
    );
    let errs = run_effect_checks(&sf);
    assert!(
        errs.iter()
            .any(|e| e.code == "A07003" && e.message.contains("must-not")),
        "effects(io) + must-not(io) must be A07003, got: {errs:?}"
    );
}

#[test]
fn must_not_database_allows_effects_io() {
    let sf = parse_source(
        r#"contract OkIo {
            effects { io }
            must-not { database }
            requires { true }
        }"#,
    );
    let errs = run_effect_checks(&sf);
    assert!(
        !errs.iter().any(|e| e.message.contains("must-not")),
        "io is not in must-not database, got: {errs:?}"
    );
}

#[test]
fn must_not_unknown_name_a07003() {
    let sf = parse_source(
        r#"contract BadForbid {
            must-not { teleport }
            requires { true }
        }"#,
    );
    let errs = run_effect_checks(&sf);
    assert!(
        errs.iter().any(|e| e.code == "A07003"),
        "unknown must-not name must be A07003, got: {errs:?}"
    );
}
