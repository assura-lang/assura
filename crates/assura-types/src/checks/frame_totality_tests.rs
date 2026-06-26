use super::*;

fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
    let (sf, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    sf.unwrap()
}

// --- Frame checks ---

#[test]
fn frame_no_modifies_no_errors() {
    let sf = parse_source("contract Simple { requires { true } }");
    let env = TypeEnv::new();
    let r = assura_resolve::resolve(&sf).unwrap();
    assert!(run_frame_checks(&r.source, &env, &r.symbols).is_empty());
}

#[test]
fn frame_modifies_with_variable_no_errors() {
    let sf = parse_source("contract C { modifies { x } }");
    let env = TypeEnv::new();
    let r = assura_resolve::resolve(&sf).unwrap();
    assert!(run_frame_checks(&r.source, &env, &r.symbols).is_empty());
}

// --- A14002: ensures modification detection ---

#[test]
fn frame_a14002_frame_assertion_no_error() {
    // ensures { y == old(y) } with modifies { x } is a frame assertion, NOT A14002
    let sf = parse_source("contract C {\n    modifies { x }\n    ensures { y == old(y) }\n}");
    let env = TypeEnv::new();
    let r = assura_resolve::resolve(&sf).unwrap();
    let errs = run_frame_checks(&r.source, &env, &r.symbols);
    assert!(
        !errs.iter().any(|e| e.code.as_ref() == "A14002"),
        "frame assertion y == old(y) should not trigger A14002: {errs:?}"
    );
}

#[test]
fn frame_a14002_modification_detected() {
    // ensures { y > old(y) } with modifies { x } implies y is modified
    // but y is not in modifies set => A14002
    let sf = parse_source("contract C {\n    modifies { x }\n    ensures { y > old(y) }\n}");
    let env = TypeEnv::new();
    let r = assura_resolve::resolve(&sf).unwrap();
    let errs = run_frame_checks(&r.source, &env, &r.symbols);
    assert!(
        errs.iter().any(|e| e.code.as_ref() == "A14002"),
        "y > old(y) with modifies {{ x }} should trigger A14002: {errs:?}"
    );
}

#[test]
fn frame_a14002_modified_var_no_error() {
    // ensures { x > old(x) } with modifies { x } is fine: x IS modified
    let sf = parse_source("contract C {\n    modifies { x }\n    ensures { x > old(x) }\n}");
    let env = TypeEnv::new();
    let r = assura_resolve::resolve(&sf).unwrap();
    let errs = run_frame_checks(&r.source, &env, &r.symbols);
    assert!(
        !errs.iter().any(|e| e.code.as_ref() == "A14002"),
        "x > old(x) with modifies {{ x }} should not trigger A14002: {errs:?}"
    );
}

// --- Totality checks ---

#[test]
fn totality_non_recursive_no_errors() {
    let sf = parse_source("fn add(a: Int, b: Int) -> Int\n    requires { a > 0 }");
    let (errs, _pending) = run_totality_checks(&sf);
    assert!(
        errs.is_empty(),
        "non-recursive fn should have no totality errors: {errs:?}"
    );
}

#[test]
fn totality_partial_function_no_errors() {
    let src = "fn diverge(x: Int) -> Int\n    partial\n    requires { x > 0 }";
    let sf = parse_source(src);
    let (errs, _pending) = run_totality_checks(&sf);
    assert!(
        errs.is_empty(),
        "partial fn should not trigger totality error: {errs:?}"
    );
}
