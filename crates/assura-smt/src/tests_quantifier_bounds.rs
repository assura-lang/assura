use super::*;

fn type_check_source(source: &str) -> assura_types::TypedFile {
    crate::test_util::typecheck_ok(source)
}

#[test]
fn forall_over_int_is_unbounded() {
    let typed = type_check_source(
        r#"
contract Bad {
    input(x: Int)
    requires { forall n in Int: n >= 0 }
}
"#,
    );
    let warnings = validate_quantifier_bounds(&typed);
    assert!(
        !warnings.is_empty(),
        "forall over Int should produce a warning"
    );
    assert!(warnings[0].reason.contains("infinite domain"));
}

#[test]
fn exists_over_nat_is_unbounded() {
    let typed = type_check_source(
        r#"
contract Bad {
    input(x: Int)
    requires { exists n in Nat: n > x }
}
"#,
    );
    let warnings = validate_quantifier_bounds(&typed);
    assert!(
        !warnings.is_empty(),
        "exists over Nat should produce a warning"
    );
}

#[test]
fn forall_over_collection_is_bounded() {
    let typed = type_check_source(
        r#"
contract Good {
    input(items: List<Int>)
    requires { forall v in items: v > 0 }
}
"#,
    );
    let warnings = validate_quantifier_bounds(&typed);
    assert!(
        warnings.is_empty(),
        "forall over a collection variable should NOT warn: {warnings:?}"
    );
}

#[test]
fn forall_over_range_is_bounded() {
    let typed = type_check_source(
        r#"
contract Good {
    input(n: Nat)
    requires { forall i in 0 .. n: i >= 0 }
}
"#,
    );
    let warnings = validate_quantifier_bounds(&typed);
    assert!(
        warnings.is_empty(),
        "forall over a range should NOT warn: {warnings:?}"
    );
}
