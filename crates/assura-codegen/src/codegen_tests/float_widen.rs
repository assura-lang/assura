use super::*;

/// `let y = 1.0 in y >= x` must not wrap the let-bound float in `i128::from`.
#[test]
fn let_float_value_skips_i128_in_codegen() {
    let typed = typecheck_ok(
        r#"
contract FloatLet {
    input(x: Int)
    requires { true }
    ensures { (let y = 1.0 in y) >= x }
}
"#,
    );
    let project = codegen(&typed);
    let rust = &project.files[0].1;
    assert!(
        !rust.contains("i128::from"),
        "let-bound float must not use i128::from, got: {rust}"
    );
}

/// A u128-scale literal inside `if` must use `as u128`, not `i128::from`.
#[test]
fn u128_literal_in_if_skips_i128_from() {
    let typed = typecheck_ok(
        r#"
contract U128If {
    input(x: Int)
    requires { true }
    ensures { (if true then 340282366920938463463374607431768211455 else 0) >= 0 }
}
"#,
    );
    let project = codegen(&typed);
    let rust = &project.files[0].1;
    assert!(
        !rust.contains("i128::from"),
        "u128 literal in if must not use i128::from, got: {rust}"
    );
    assert!(
        rust.contains("as u128"),
        "u128 literal in if should widen via as u128, got: {rust}"
    );
}
