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
