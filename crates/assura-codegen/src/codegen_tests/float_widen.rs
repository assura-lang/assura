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

/// Service query `input(x: Float)` clauses must not wrap `x` in `i128::from`.
/// `f64` does not implement `Into<i128>`, so those `debug_assert!`s fail to compile.
#[test]
fn service_float_param_skips_i128_in_codegen() {
    let typed = typecheck_ok(
        r#"
service FloatQuery {
    query scale {
        input(x: Float)
        output(result: Float)
        requires { x >= 0 }
        ensures { result >= x }
    }
}
"#,
    );
    let project = codegen(&typed);
    let rust = &project.files[0].1;
    // Doc comments are not compiled; only the method body feeds debug_assert!.
    let method = rust
        .split("pub fn scale")
        .nth(1)
        .expect("generated scale method");
    let body_start = method.find('{').expect("method body");
    let body = &method[body_start..];
    assert!(
        !body.contains("i128::from(x)") && !body.contains("i128::from"),
        "service Float param x must not use i128::from, got: {body}"
    );
    assert!(
        body.contains("x >= 0") || body.contains("x >= 0.0"),
        "service requires should compare x as f64, got: {body}"
    );
}
