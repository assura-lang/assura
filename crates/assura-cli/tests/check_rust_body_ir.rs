//! Integration tests for `assura check-rust` body IR encode / CE pairs.
//!
//! Split from `cli_integration.rs` (#1352) so encode surface tests do not
//! dominate the general CLI integration binary compile.

mod common;

use common::{assura_bin, unique_temp};
use std::process::Command;

/// Const bitwise NOT for typed lit.
#[test]
fn check_rust_encodes_const_bitnot() {
    let tmp = unique_temp("assura_check_rust_bitnot");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 250
fn n(x: u8) -> u8 { !5u8 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong const bitnot ensures must CE.
#[test]
fn check_rust_const_bitnot_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_bitnot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn n(x: u8) -> u8 { !5u8 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

#[test]
fn check_rust_finds_annotations() {
    let tmp = unique_temp("assura_check_rust_test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    std::fs::write(
        tmp.join("src/lib.rs"),
        r#"
/// @requires x > 0
/// @ensures result > 0
fn positive(x: i32) -> i32 {
    x + 1
}
"#,
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["check-rust", tmp.join("src/lib.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("check-rust:") && stdout.contains("1 file(s)"),
        "should report file count, got: {stdout}"
    );
    assert!(
        stdout.contains("annotated item"),
        "should report annotated items, got: {stdout}"
    );
}

#[test]
fn check_rust_json_output() {
    let tmp = unique_temp("assura_check_rust_json");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("test.rs"),
        r#"
/// @requires a > 0
fn only_positive(a: i32) -> i32 {
    a
}
"#,
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args([
            "check-rust",
            "--json",
            tmp.join("test.rs").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{stdout}"));
    assert_eq!(parsed["files"], 1);
    assert_eq!(parsed["items"], 1);
    assert!(parsed["clauses"].as_u64().unwrap() >= 1);
}

#[test]
fn check_rust_no_annotations() {
    let tmp = unique_temp("assura_check_rust_none");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("plain.rs"),
        r#"
/// Regular doc comment, no annotations.
fn regular(x: i32) -> i32 {
    x
}
"#,
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["check-rust", tmp.join("plain.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("no inline contract annotations found"),
        "should report no annotations, got: {stdout}"
    );
}

/// Ensures without co-located IR must not print "check passed" / "ensures …
/// verified" before body_not_modeled (MPI End User / Observability).
#[test]
fn check_rust_body_not_modeled_human_message() {
    let tmp = unique_temp("assura_check_rust_bnm");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @requires x > 0
/// @ensures result == x + 1
fn bad(x: i64) -> i64 { x }
"#,
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["check-rust", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        !out.status.success(),
        "body_not_modeled should be non-zero exit, got stdout={stdout} stderr={stderr}"
    );
    assert!(
        combined.contains("body_not_modeled"),
        "expected body_not_modeled status, got: {combined}"
    );
    assert!(
        !combined.contains("check passed"),
        "must not claim check passed when body is not modeled: {combined}"
    );
    // Grouped SMT table uses "ensures ... verified"; must stay silent for BNM.
    assert!(
        !combined.contains("... verified"),
        "must not print SMT 'ensures ... verified' before body_not_modeled: {combined}"
    );
}

#[test]
fn check_rust_directory_scan() {
    let tmp = unique_temp("assura_check_rust_dir");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    std::fs::write(
        tmp.join("src/a.rs"),
        r#"
/// @requires n > 0
fn f(n: i32) -> i32 { n }
"#,
    )
    .unwrap();
    std::fs::write(
        tmp.join("src/b.rs"),
        r#"
/// @invariant self.x >= 0
struct Foo { x: i32 }
"#,
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["check-rust", tmp.join("src").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("2 file(s)"),
        "should find 2 files, got: {stdout}"
    );
}

// =======================================================================
// infer: heuristic-based contract inference for Rust files
// =======================================================================

/// Simple if body encodes (Clamp.ir-style multi-block) (#986).
#[test]
fn check_rust_encodes_if_body() {
    let tmp = unique_temp("assura_check_rust_body_if");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn clamp0(x: i64) -> i64 { if x > 0 { x } else { 0 } }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "if body should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["verified"].as_u64().unwrap_or(0) >= 1);

    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result >= 0
fn clamp0(x: i64) -> i64 { if x > 0 { x } else { -1 } }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "wrong else branch should CE");
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Single `let y = e; y` inlines to encode `e` (#986).
#[test]
fn check_rust_encodes_let_inline_body() {
    let tmp = unique_temp("assura_check_rust_body_let");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x + 1
fn multi(x: i64) -> i64 { let y = x + 1; y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "let-inline body should pass: {stdout}"
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["verified"].as_u64().unwrap_or(0) >= 1);

    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x + 1
fn multi(x: i64) -> i64 { let y = x; y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// abs/min method bodies encode and verify simple ensures.
#[test]
fn check_rust_encodes_abs_min_bodies() {
    let tmp = unique_temp("assura_check_rust_body_abs");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("lib.rs"),
        r#"
/// @ensures result >= 0
fn abs_like(x: i64) -> i64 { x.abs() }

/// @ensures result <= x
/// @ensures result <= y
fn mymin(x: i64, y: i64) -> i64 { x.min(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("lib.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "abs/min bodies should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 3, "{stdout}");
}

/// Nested / mul body encoding: correct body verifies; wrong body CEs.
#[test]
fn check_rust_encodes_nested_and_mul_bodies() {
    let tmp = unique_temp("assura_check_rust_body_nested");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x + y + 1
fn nest(x: i64, y: i64) -> i64 { x + y + 1 }

/// @ensures result == x * 2
fn mul(x: i64) -> i64 { x * 2 }

/// @ensures result == -x
fn neg(x: i64) -> i64 { -x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(
        v["body_not_modeled"], 0,
        "all three bodies should encode: {stdout}"
    );
    assert!(out.status.success(), "correct bodies should pass: {stdout}");
    assert!(
        v["verified"].as_u64().unwrap_or(0) >= 3,
        "expected >=3 verified clauses: {stdout}"
    );

    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x * 2
fn mul(x: i64) -> i64 { x + 2 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "wrong mul body should fail");
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Multi-let bodies fold into a single expression and verify.
#[test]
fn check_rust_encodes_multi_let_body() {
    let tmp = unique_temp("assura_check_rust_body_multilet");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x + 2
fn f(x: i64) -> i64 { let a = x + 1; let b = a + 1; b }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "multi-let should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 1, "{stdout}");

    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x + 2
fn f(x: i64) -> i64 { let a = x + 1; let b = a; b }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "wrong multi-let should CE");
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Simple match (literal + wildcard) encodes multi-block IR (#993).
#[test]
fn check_rust_encodes_match_body() {
    let tmp = unique_temp("assura_check_rust_body_match");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn sign(x: i64) -> i64 {
    match x {
        0 => 0,
        _ => 1,
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "match body should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 1, "{stdout}");

    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result >= 0
fn sign(x: i64) -> i64 {
    match x {
        0 => 0,
        _ => -1,
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "wrong match arm should CE");
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// If branches with simple `let y = e; y` fold and verify.
#[test]
fn check_rust_encodes_if_let_branch() {
    let tmp = unique_temp("assura_check_rust_body_if_let");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: i64) -> i64 {
    if x > 0 {
        let y = x;
        y
    } else {
        0
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "if-let branch should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 1, "{stdout}");
}

/// Identity match guards rewrite to if-tree (#999).
#[test]
fn check_rust_encodes_match_guard() {
    let tmp = unique_temp("assura_check_rust_body_match_guard");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: i64) -> i64 {
    match x {
        n if n > 0 => n,
        _ => 0,
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "match guard should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 1, "{stdout}");

    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result >= 0
fn f(x: i64) -> i64 {
    match x {
        n if n > 0 => n,
        _ => -1,
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "wrong default should CE");
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Guarded match with non-identity wrong arm must CE (not BNM).
#[test]
fn check_rust_match_guard_wrong_arm_ce() {
    let tmp = unique_temp("assura_check_rust_match_guard_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result >= 0
fn f(x: i64) -> i64 {
    match x {
        n if n > 10 => n,
        n if n > 0 => -1,
        _ => 0,
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "wrong guarded arm should CE");
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{v}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Bool not and bool match encode.
#[test]
fn check_rust_encodes_bool_not_and_match() {
    let tmp = unique_temp("assura_check_rust_bool_not");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn notb(b: bool) -> bool { !b }

/// @ensures result == true || result == false
fn m(b: bool) -> bool {
    match b {
        true => true,
        false => false,
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "bool bodies should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 2, "{stdout}");

    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == b
fn notb(b: bool) -> bool { !b }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// x.clamp(lo, hi) encodes as min(max(x, lo), hi).
#[test]
fn check_rust_encodes_clamp() {
    let tmp = unique_temp("assura_check_rust_clamp");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 10
fn f(x: i64) -> i64 { x.clamp(0, 10) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "clamp should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 2, "{stdout}");
}

/// clamp(x, y, y) peeps to y.
#[test]
fn check_rust_encodes_clamp_same_bounds() {
    let tmp = unique_temp("assura_check_rust_clamp_same");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == y
fn f(x: i64, y: i64) -> i64 { x.clamp(y, y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Parametric clamp needs lo<=hi requires for range ensures.
#[test]
fn check_rust_encodes_clamp_params() {
    let tmp = unique_temp("assura_check_rust_clamp_params");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @requires lo <= hi
/// @ensures result >= lo
/// @ensures result <= hi
fn f(x: i64, lo: i64, hi: i64) -> i64 { x.clamp(lo, hi) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "parametric clamp should pass: {stdout}"
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 2, "{stdout}");

    // Without lo<=hi, range ensures can fail (sound CE or error).
    std::fs::write(
        tmp.join("no_req.rs"),
        r#"
/// @ensures result >= lo
/// @ensures result <= hi
fn f(x: i64, lo: i64, hi: i64) -> i64 { x.clamp(lo, hi) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args([
            "check-rust",
            "--json",
            tmp.join("no_req.rs").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "clamp without lo<=hi should not soft-pass: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// Mid-width clamp for path params.
#[test]
fn check_rust_encodes_mid_width_clamp() {
    let tmp = unique_temp("assura_check_rust_mid_clamp");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn c(x: u16) -> u16 { x.clamp(1, 100) }

/// @ensures result == 0 || result != 0
fn s(x: i16) -> i16 { x.clamp(-10, 10) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width clamp ensures must CE.
#[test]
fn check_rust_mid_width_clamp_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_clamp_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn c(x: u16) -> u16 { x.clamp(1, 100) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// saturating_add encodes with i64 range requires (Closes #1007).
#[test]
fn check_rust_encodes_saturating_add() {
    let tmp = unique_temp("assura_check_rust_sat_add");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= x
fn f(x: i64) -> i64 { x.saturating_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "saturating_add should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 1, "{stdout}");
}

/// i32 saturating_add clamps to i32 range (not i64).
#[test]
fn check_rust_i32_saturating_add() {
    let tmp = unique_temp("assura_check_rust_i32_sat");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result <= 2147483647
fn f(x: i32) -> i32 { x.saturating_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "i32 sat should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Mid-width saturating_mul for path params.
#[test]
fn check_rust_encodes_mid_width_saturating_mul() {
    let tmp = unique_temp("assura_check_rust_mid_sat_mul");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn s16(x: i16) -> i16 { x.saturating_mul(2) }

/// @ensures result == 0 || result != 0
fn u16s(x: u16) -> u16 { x.saturating_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i16 saturating_mul ensures must CE.
#[test]
fn check_rust_i16_saturating_mul_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i16_sat_mul_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: i16) -> i16 { x.saturating_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width saturating_sub for path params.
#[test]
fn check_rust_encodes_mid_width_saturating_sub() {
    let tmp = unique_temp("assura_check_rust_mid_sat_sub");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn s16(x: i16) -> i16 { x.saturating_sub(1) }

/// @ensures result == 0 || result != 0
fn u16s(x: u16) -> u16 { x.saturating_sub(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i16 saturating_sub ensures must CE.
#[test]
fn check_rust_i16_saturating_sub_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i16_sat_sub_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: i16) -> i16 { x.saturating_sub(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width saturating_add for path params.
#[test]
fn check_rust_encodes_mid_width_saturating_add() {
    let tmp = unique_temp("assura_check_rust_mid_sat_add");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn s16(x: i16) -> i16 { x.saturating_add(1) }

/// @ensures result == 0 || result != 0
fn u16s(x: u16) -> u16 { x.saturating_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i16 saturating_add ensures must CE.
#[test]
fn check_rust_i16_saturating_add_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i16_sat_add_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: i16) -> i16 { x.saturating_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 if/else and multi-let encode.
#[test]
fn check_rust_encodes_u64_if_else() {
    let tmp = unique_temp("assura_check_rust_u64_if");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: u64, y: u64) -> u64 { if x > y { x } else { y } }

/// @ensures result >= 0
fn l(x: u64) -> u64 { let a = x + 1; a }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 if/else ensures must CE.
#[test]
fn check_rust_u64_if_else_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_if_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn f(x: u64, y: u64) -> u64 { if x > y { x } else { y } }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// if with && condition encodes.
#[test]
fn check_rust_encodes_if_and_cond() {
    let tmp = unique_temp("assura_check_rust_if_and");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: u64, y: u64) -> u64 {
    if x > 0 && y > 0 { x + y } else { 0 }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong if-and ensures must CE.
#[test]
fn check_rust_if_and_cond_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_if_and_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn f(x: u64, y: u64) -> u64 {
    if x > 0 && y > 0 { x + y } else { 0 }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// if with || condition encodes.
#[test]
fn check_rust_encodes_if_or_cond() {
    let tmp = unique_temp("assura_check_rust_if_or");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: u64, y: u64) -> u64 {
    if x == 0 || y == 0 { 0 } else { x + y }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong if-or ensures must CE.
#[test]
fn check_rust_if_or_cond_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_if_or_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn f(x: u64, y: u64) -> u64 {
    if x == 0 || y == 0 { 0 } else { x + y }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// let y = if …; y + 1 folds and encodes (distribute if out of binary).
#[test]
fn check_rust_encodes_let_if_fold() {
    let tmp = unique_temp("assura_check_rust_let_if_fold");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 1
fn f(x: i64) -> i64 {
    let y = if x > 5 { x } else { 5 };
    y + 1
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong let-if-fold ensures must CE.
#[test]
fn check_rust_let_if_fold_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_let_if_fold_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn f(x: i64) -> i64 {
    let y = if x > 5 { x } else { 5 };
    y + 1
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// if-on-right of binary encodes after distribute.
#[test]
fn check_rust_encodes_if_on_right_binary() {
    let tmp = unique_temp("assura_check_rust_if_on_right");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 1
fn f(x: i64) -> i64 {
    1 + (if x > 5 { x } else { 5 })
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong if-on-right ensures must CE.
#[test]
fn check_rust_if_on_right_binary_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_if_on_right_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn f(x: i64) -> i64 {
    1 + (if x > 5 { x } else { 5 })
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// let y = match …; y + 1 folds and encodes.
#[test]
fn check_rust_encodes_let_match_fold() {
    let tmp = unique_temp("assura_check_rust_let_match_fold");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result > x
fn f(x: i64) -> i64 {
    let y = match x { 0 => 1, _ => x };
    y + 1
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong let-match-fold ensures must CE.
#[test]
fn check_rust_let_match_fold_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_let_match_fold_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn f(x: i64) -> i64 {
    let y = match x { 0 => 1, _ => x };
    y + 1
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Unary-neg of if encodes after distribute.
#[test]
fn check_rust_encodes_unary_neg_if() {
    let tmp = unique_temp("assura_check_rust_unary_neg_if");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result <= 0
fn f(x: i64) -> i64 {
    -(if x > 0 { x } else { 1 })
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong unary-neg-if ensures must CE.
#[test]
fn check_rust_unary_neg_if_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_unary_neg_if_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn f(x: i64) -> i64 {
    -(if x > 0 { x } else { 1 })
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Method on if-receiver encodes after distribute.
#[test]
fn check_rust_encodes_method_on_if() {
    let tmp = unique_temp("assura_check_rust_method_on_if");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: i64) -> i64 {
    (if x > 0 { x } else { -x }).abs()
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong method-on-if ensures must CE.
#[test]
fn check_rust_method_on_if_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_method_on_if_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn f(x: i64) -> i64 {
    (if x > 0 { x } else { -x }).abs()
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Cast of if encodes after distribute.
#[test]
fn check_rust_encodes_cast_of_if() {
    let tmp = unique_temp("assura_check_rust_cast_of_if");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: i64) -> i64 {
    (if x > 0 { x } else { 0 }) as i64
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong cast-of-if ensures must CE.
#[test]
fn check_rust_cast_of_if_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_cast_of_if_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == -1
fn f(x: i64) -> i64 {
    (if x > 0 { x } else { 0 }) as i64
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// if as method arg encodes after distribute.
#[test]
fn check_rust_encodes_if_as_method_arg() {
    let tmp = unique_temp("assura_check_rust_if_method_arg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= x
fn f(x: i64, y: i64) -> i64 {
    x.saturating_add(if y > 0 { 1 } else { 0 })
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong if-as-method-arg ensures must CE.
#[test]
fn check_rust_if_as_method_arg_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_if_method_arg_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn f(x: i64, y: i64) -> i64 {
    x.saturating_add(if y > 0 { 1 } else { 0 })
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// `*& (if …)` peels to multi-block if encode.
#[test]
fn check_rust_encodes_ref_deref_if() {
    let tmp = unique_temp("assura_check_rust_ref_deref_if");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: i64) -> i64 {
    *& (if x > 0 { x } else { 0 })
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong ref-deref-if ensures must CE.
#[test]
fn check_rust_ref_deref_if_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_ref_deref_if_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == -1
fn f(x: i64) -> i64 {
    *& (if x > 0 { x } else { 0 })
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// checked_add(const).unwrap_or encodes via overflow if-tree.
#[test]
fn check_rust_encodes_checked_add_unwrap() {
    let tmp = unique_temp("assura_check_rust_checked_add");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= x
fn f(x: i64) -> i64 {
    x.checked_add(1).unwrap_or(x)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong checked_add ensures must CE.
#[test]
fn check_rust_checked_add_unwrap_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_checked_add_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x + 2
fn f(x: i64) -> i64 {
    x.checked_add(1).unwrap_or(x)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// overflowing_add(c).0 encodes as wrapping_add.
#[test]
fn check_rust_encodes_overflowing_add() {
    let tmp = unique_temp("assura_check_rust_overflowing_add");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x + 1 || result < x
fn f(x: i64) -> i64 {
    x.overflowing_add(1).0
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong overflowing_add ensures must CE.
#[test]
fn check_rust_overflowing_add_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_overflowing_add_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn f(x: i64) -> i64 {
    x.overflowing_add(1).0
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// checked_mul(2).unwrap_or encodes.
#[test]
fn check_rust_encodes_checked_mul_unwrap() {
    let tmp = unique_temp("assura_check_rust_checked_mul");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0 || result < 0
fn f(x: i64) -> i64 {
    x.checked_mul(2).unwrap_or(0)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong checked_mul ensures must CE.
#[test]
fn check_rust_checked_mul_unwrap_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_checked_mul_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x * 3
fn f(x: i64) -> i64 {
    x.checked_mul(2).unwrap_or(0)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// checked_div(const).unwrap_or encodes.
#[test]
fn check_rust_encodes_checked_div_unwrap() {
    let tmp = unique_temp("assura_check_rust_checked_div");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result * 2 <= x + 1
fn f(x: i64) -> i64 {
    x.checked_div(2).unwrap_or(0)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong checked_div ensures must CE.
#[test]
fn check_rust_checked_div_unwrap_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_checked_div_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn f(x: i64) -> i64 {
    x.checked_div(2).unwrap_or(0)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// let mut without reassignment folds like let.
#[test]
fn check_rust_encodes_let_mut_no_reassign() {
    let tmp = unique_temp("assura_check_rust_let_mut");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= x
fn f(x: i64) -> i64 {
    let mut y = x;
    y + 1
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// checked_neg().unwrap_or encodes (MIN → alt).
#[test]
fn check_rust_encodes_checked_neg_unwrap() {
    let tmp = unique_temp("assura_check_rust_checked_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result <= 0 || result > 0
fn f(x: i64) -> i64 {
    x.checked_neg().unwrap_or(0)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong checked_neg ensures must CE.
#[test]
fn check_rust_checked_neg_unwrap_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_checked_neg_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn f(x: i64) -> i64 {
    x.checked_neg().unwrap_or(0)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// checked_pow(const).unwrap_or encodes for small exp.
#[test]
fn check_rust_encodes_checked_pow_unwrap() {
    let tmp = unique_temp("assura_check_rust_checked_pow");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: i64) -> i64 {
    x.checked_pow(2).unwrap_or(0)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong checked_pow ensures must CE.
#[test]
fn check_rust_checked_pow_unwrap_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_checked_pow_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn f(x: i64) -> i64 {
    x.checked_pow(2).unwrap_or(0)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Multi-let with if RHS folds and encodes.
#[test]
fn check_rust_encodes_multi_let_if() {
    let tmp = unique_temp("assura_check_rust_multi_let_if");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: i64) -> i64 {
    let a = if x > 0 { x } else { 0 };
    let b = a + 1;
    b * 2
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// checked_abs().unwrap_or encodes (MIN → alt).
#[test]
fn check_rust_encodes_checked_abs_unwrap() {
    let tmp = unique_temp("assura_check_rust_checked_abs");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: i64) -> i64 {
    x.checked_abs().unwrap_or(0)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong checked_abs ensures must CE.
#[test]
fn check_rust_checked_abs_unwrap_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_checked_abs_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn f(x: i64) -> i64 {
    x.checked_abs().unwrap_or(0)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// checked_ilog2().unwrap_or encodes.
#[test]
fn check_rust_encodes_checked_ilog2_unwrap() {
    let tmp = unique_temp("assura_check_rust_checked_ilog2");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: u32) -> u32 {
    x.checked_ilog2().unwrap_or(0)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong checked_ilog2 ensures must CE.
#[test]
fn check_rust_checked_ilog2_unwrap_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_checked_ilog2_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn f(x: u32) -> u32 {
    x.checked_ilog2().unwrap_or(0)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// checked_next_power_of_two().unwrap_or encodes (unsigned).
#[test]
fn check_rust_encodes_checked_next_power_of_two() {
    let tmp = unique_temp("assura_check_rust_checked_npot");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 1
fn f(x: u8) -> u8 {
    x.checked_next_power_of_two().unwrap_or(1)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong checked_next_power_of_two ensures must CE.
#[test]
fn check_rust_checked_next_power_of_two_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_checked_npot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn f(x: u8) -> u8 {
    x.checked_next_power_of_two().unwrap_or(1)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// checked_shl(const).unwrap_or encodes.
#[test]
fn check_rust_encodes_checked_shl_unwrap() {
    let tmp = unique_temp("assura_check_rust_checked_shl");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: u8) -> u8 {
    x.checked_shl(1).unwrap_or(0)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong checked_shl ensures must CE.
#[test]
fn check_rust_checked_shl_unwrap_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_checked_shl_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn f(x: u8) -> u8 {
    x.checked_shl(1).unwrap_or(0)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// checked_shr(const).unwrap_or encodes.
#[test]
fn check_rust_encodes_checked_shr_unwrap() {
    let tmp = unique_temp("assura_check_rust_checked_shr");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: u8) -> u8 {
    x.checked_shr(1).unwrap_or(0)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong checked_shr ensures must CE.
#[test]
fn check_rust_checked_shr_unwrap_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_checked_shr_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn f(x: u8) -> u8 {
    x.checked_shr(1).unwrap_or(0)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// overflowing_pow(const).0 encodes as wrapping_pow.
#[test]
fn check_rust_encodes_overflowing_pow() {
    let tmp = unique_temp("assura_check_rust_overflowing_pow");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: u8) -> u8 {
    x.overflowing_pow(2).0
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong overflowing_pow ensures must CE.
#[test]
fn check_rust_overflowing_pow_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_overflowing_pow_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn f(x: u8) -> u8 {
    x.overflowing_pow(2).0
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Full i64 reverse_bits via synthetic 2^64 bit-pattern map.
#[test]
fn check_rust_encodes_i64_reverse_bits() {
    let tmp = unique_temp("assura_check_rust_i64_reverse_bits");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 1 || result != 1
fn r(x: i64) -> i64 {
    x.reverse_bits()
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i64 reverse_bits ensures must CE.
#[test]
fn check_rust_i64_reverse_bits_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i64_reverse_bits_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn r(x: i64) -> i64 {
    x.reverse_bits()
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Full i64 both-variable bitops via synthetic 2^64 bit-pattern map.
#[test]
fn check_rust_encodes_i64_both_var_bitops() {
    let tmp = unique_temp("assura_check_rust_i64_both_var");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn band(x: i64, y: i64) -> i64 { x & y }

/// @ensures result == 0 || result != 0
fn bor(x: i64, y: i64) -> i64 { x | y }

/// @ensures result == 0 || result != 0
fn bxor(x: i64, y: i64) -> i64 { x ^ y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// overflowing_shl/shr(const).0 encodes as wrapping_*.
#[test]
fn check_rust_encodes_overflowing_shl_shr() {
    let tmp = unique_temp("assura_check_rust_overflowing_shl");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn s(x: u8) -> u8 { x.overflowing_shl(1).0 }

/// @ensures result >= 0
fn r(x: u8) -> u8 { x.overflowing_shr(1).0 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong overflowing_shl ensures must CE.
#[test]
fn check_rust_overflowing_shl_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_overflowing_shl_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: u8) -> u8 { x.overflowing_shl(1).0 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// overflowing_*(…).1 overflow flag encodes (dual of checked_*.is_none()).
#[test]
fn check_rust_encodes_overflowing_flag() {
    let tmp = unique_temp("assura_check_rust_overflowing_flag");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn a(x: u8) -> bool { x.overflowing_add(1).1 }

/// @ensures result == true
fn oob(x: u8) -> bool { x.overflowing_shl(8).1 }

/// @ensures result == true || result == false
fn n(x: i64) -> bool { x.overflowing_neg().1 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong overflowing_add.1 ensures must CE (x=255 overflows).
#[test]
fn check_rust_overflowing_flag_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_overflowing_flag_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == false
fn a(x: u8) -> bool { x.overflowing_add(1).1 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE (x=255): {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// checked_shl(n) with n >= width always uses unwrap_or alt.
#[test]
fn check_rust_encodes_checked_shl_oob_alt() {
    let tmp = unique_temp("assura_check_rust_checked_shl_oob");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 7
fn f(x: u8) -> u8 {
    x.checked_shl(8).unwrap_or(7)
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// wrapping_add_signed / wrapping_add_unsigned encode as modular wrap.
#[test]
fn check_rust_encodes_wrapping_add_signed_unsigned() {
    let tmp = unique_temp("assura_check_rust_wrap_signed");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn u(x: u8) -> u8 { x.wrapping_add_signed(1) }

/// @ensures result >= -128 && result <= 127
fn s(x: i8) -> i8 { x.wrapping_add_unsigned(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong wrapping_add_signed ensures must CE.
#[test]
fn check_rust_wrapping_add_signed_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_wrap_signed_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn u(x: u8) -> u8 { x.wrapping_add_signed(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// checked_add(c).is_some() / is_none() encode as overflow bounds.
#[test]
fn check_rust_encodes_checked_is_some_none() {
    let tmp = unique_temp("assura_check_rust_checked_is_some");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn s(x: u8) -> bool { x.checked_add(1).is_some() }

/// @ensures result == true || result == false
fn n(x: u8) -> bool { x.checked_sub(1).is_none() }

/// @ensures result == false
fn oob(x: u8) -> bool { x.checked_shl(8).is_some() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong checked_add.is_some ensures must CE.
#[test]
fn check_rust_checked_is_some_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_checked_is_some_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == true
fn s(x: u8) -> bool { x.checked_add(1).is_some() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE (x=255): {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// checked_mul/div/ilog/npot is_some family encodes.
#[test]
fn check_rust_encodes_checked_is_some_family() {
    let tmp = unique_temp("assura_check_rust_checked_is_some_family");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn m(x: u8) -> bool { x.checked_mul(2).is_some() }

/// @ensures result == false
fn d0(x: i64) -> bool { x.checked_div(0).is_some() }

/// @ensures result == true || result == false
fn l(x: u32) -> bool { x.checked_ilog2().is_some() }

/// @ensures result == true || result == false
fn p(x: u8) -> bool { x.checked_next_power_of_two().is_some() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong checked_mul.is_some ensures must CE.
#[test]
fn check_rust_checked_mul_is_some_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_checked_mul_is_some_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == true
fn m(x: u8) -> bool { x.checked_mul(2).is_some() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE (x>127): {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong i64 both-var bitop ensures must CE.
#[test]
fn check_rust_i64_both_var_bitops_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i64_both_var_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn a(x: i64, y: i64) -> i64 {
    x & y
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Multi-let chain encodes.
#[test]
fn check_rust_encodes_u64_multi_let() {
    let tmp = unique_temp("assura_check_rust_u64_multi_let");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn l(x: u64) -> u64 {
    let a = x + 1;
    let b = a * 2;
    b
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong multi-let ensures must CE.
#[test]
fn check_rust_u64_multi_let_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_multi_let_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn l(x: u64) -> u64 {
    let a = x + 1;
    let b = a * 2;
    b
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 match with guards encodes.
#[test]
fn check_rust_encodes_u64_match_guard() {
    let tmp = unique_temp("assura_check_rust_u64_match");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn m(x: u64) -> u64 { match x { n if n > 0 => n, _ => 0 } }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 match-guard ensures must CE.
#[test]
fn check_rust_u64_match_guard_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_match_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn m(x: u64) -> u64 { match x { n if n > 0 => n, _ => 0 } }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 match with lit + identity bind arm encodes.
#[test]
fn check_rust_encodes_u64_match_literal_bind() {
    let tmp = unique_temp("assura_check_rust_u64_match_bind");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn m(x: u64) -> u64 { match x { 0 => 1, n => n } }

/// @ensures result >= 0
fn m2(x: u64) -> u64 { match x { 0 => 1, n => n + 1 } }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 match-bind ensures must CE.
#[test]
fn check_rust_u64_match_literal_bind_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_match_bind_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn m(x: u64) -> u64 { match x { 0 => 1, n => n } }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Multi-literal match + bind arm encodes.
#[test]
fn check_rust_encodes_u64_match_multi_lit() {
    let tmp = unique_temp("assura_check_rust_u64_match_multi");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn m(x: u64) -> u64 {
    match x {
        0 => 10,
        1 => 20,
        n => n,
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong multi-lit match ensures must CE.
#[test]
fn check_rust_u64_match_multi_lit_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_match_multi_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn m(x: u64) -> u64 {
    match x {
        0 => 10,
        1 => 20,
        n => n,
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Multi-guard match rewrites to if-tree.
#[test]
fn check_rust_encodes_u64_match_multi_guard() {
    let tmp = unique_temp("assura_check_rust_u64_match_mguard");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn m(x: u64) -> u64 {
    match x {
        n if n > 10 => n,
        n if n > 0 => 1,
        _ => 0,
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong multi-guard match ensures must CE.
#[test]
fn check_rust_u64_match_multi_guard_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_match_mguard_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn m(x: u64) -> u64 {
    match x {
        n if n > 10 => n,
        n if n > 0 => 1,
        _ => 0,
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 path-param + * encode (Nat-range nonneg).
#[test]
fn check_rust_encodes_u64_arith() {
    let tmp = unique_temp("assura_check_rust_u64_arith");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn a(x: u64, y: u64) -> u64 { x + y }

/// @ensures result == result
fn s(x: u64, y: u64) -> u64 { x - y }

/// @ensures result >= 0
fn m(x: u64, y: u64) -> u64 { x * y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 add ensures must CE.
#[test]
fn check_rust_u64_arith_add_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_arith_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn a(x: u64, y: u64) -> u64 { x + y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 sub ensures must CE.
#[test]
fn check_rust_u64_arith_sub_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_sub_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn s(x: u64, y: u64) -> u64 { x - y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Parenthesized u64 arith product encodes.
#[test]
fn check_rust_encodes_u64_paren_arith() {
    let tmp = unique_temp("assura_check_rust_u64_paren");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: u64) -> u64 { (x + 1) * (x + 2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong paren arith ensures must CE.
#[test]
fn check_rust_u64_paren_arith_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_paren_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn f(x: u64) -> u64 { (x + 1) * (x + 2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 saturating_add/sub clamp via synthetic 2^64-1.
#[test]
fn check_rust_encodes_u64_saturating() {
    let tmp = unique_temp("assura_check_rust_u64_sat");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn a(x: u64, y: u64) -> u64 { x.saturating_add(y) }

/// @ensures result >= 0
fn s(x: u64, y: u64) -> u64 { x.saturating_sub(y) }

/// @ensures result >= 0
fn m(x: u64, y: u64) -> u64 { x.saturating_mul(y) }

/// @ensures result == 0
fn n(x: u64) -> u64 { x.saturating_neg() }

/// @ensures result >= 0
fn ab(x: u64) -> u64 { x.saturating_abs() }

/// @ensures result >= 0
fn c(x: u64) -> u64 { x.clamp(0, 10) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 saturating_add ensures must CE.
#[test]
fn check_rust_u64_saturating_add_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_sat_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn a(x: u64, y: u64) -> u64 { x.saturating_add(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Nested u64 saturating chain encodes.
#[test]
fn check_rust_encodes_u64_nested_saturating() {
    let tmp = unique_temp("assura_check_rust_u64_nested_sat");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn n(x: u64, y: u64) -> u64 { x.saturating_add(y).saturating_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong nested saturating ensures must CE.
#[test]
fn check_rust_u64_nested_saturating_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_nested_sat_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn n(x: u64, y: u64) -> u64 { x.saturating_add(y).saturating_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 clamp ensures must CE.
#[test]
fn check_rust_u64_clamp_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_clamp_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn c(x: u64) -> u64 { x.clamp(1, 10) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64::MAX / u64::MIN associated consts encode.
#[test]
fn check_rust_encodes_u64_associated_max_min() {
    let tmp = unique_temp("assura_check_rust_u64_assoc_max");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn m(x: u64) -> u64 { u64::MAX }

/// @ensures result == 0
fn n(x: u64) -> u64 { u64::MIN }

/// @ensures result == 0
fn d(x: u64) -> u64 { u64::default() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64::MAX ensures must CE.
#[test]
fn check_rust_u64_associated_max_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_assoc_max_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn m(x: u64) -> u64 { u64::MAX }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Default::default() path form encodes as 0.
#[test]
fn check_rust_encodes_default_trait() {
    let tmp = unique_temp("assura_check_rust_default_trait");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0
fn d(x: u64) -> u64 { Default::default() }

/// @ensures result == 0
fn d2(x: i64) -> i64 { <i64 as Default>::default() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong Default::default ensures must CE.
#[test]
fn check_rust_default_trait_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_default_trait_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 1
fn d(x: u64) -> u64 { Default::default() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// usize isqrt and usize::MAX encode (same width path as u64).
#[test]
fn check_rust_encodes_usize_isqrt_max() {
    let tmp = unique_temp("assura_check_rust_usize_isqrt");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn s(x: usize) -> usize { x.isqrt() }

/// @ensures result >= 0
fn m(x: usize) -> usize { usize::MAX }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// usize div_ceil / rem_euclid encode (u64 width alias).
#[test]
fn check_rust_encodes_usize_div_ceil_rem() {
    let tmp = unique_temp("assura_check_rust_usize_div");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn d(x: usize) -> usize { x.div_ceil(3) }

/// @ensures result >= 0
fn r(x: usize) -> usize { x.rem_euclid(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong usize div_ceil ensures must CE.
#[test]
fn check_rust_usize_div_ceil_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_usize_dc_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn d(x: usize) -> usize { x.div_ceil(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong usize isqrt ensures must CE.
#[test]
fn check_rust_usize_isqrt_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_usize_isqrt_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn s(x: usize) -> usize { x.isqrt() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 NonZeroU64 div_euclid ensures must CE.
#[test]
fn check_rust_u64_div_euclid_nonzero_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_de_nz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
use std::num::NonZeroU64;

/// @ensures result == 0
fn d(x: u64, n: NonZeroU64) -> u64 { x.div_euclid(n.get()) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 min/max/signum/pow path params encode.
#[test]
fn check_rust_encodes_u64_min_max_signum_pow() {
    let tmp = unique_temp("assura_check_rust_u64_min_max_pow");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn mn(x: u64, y: u64) -> u64 { x.min(y) }

/// @ensures result >= 0
fn mx(x: u64, y: u64) -> u64 { x.max(y) }

/// @ensures result >= 0
/// @ensures result <= 1
fn sg(x: u64) -> u64 { x.signum() }

/// @ensures result >= 0
fn p(x: u64) -> u64 { x.pow(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 min ensures must CE.
#[test]
fn check_rust_u64_min_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_min_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn m(x: u64, y: u64) -> u64 { x.min(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 max ensures must CE.
#[test]
fn check_rust_u64_max_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_max_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn m(x: u64, y: u64) -> u64 { x.max(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 pow ensures must CE.
#[test]
fn check_rust_u64_pow_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_pow_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn p(x: u64) -> u64 { x.pow(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Nested max/min chain encodes (clamp-like).
#[test]
fn check_rust_encodes_nested_min_max() {
    let tmp = unique_temp("assura_check_rust_nested_min_max");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn c(x: u64) -> u64 { x.max(1).min(10) }

/// @ensures result >= 0
fn a(x: i64) -> i64 { x.abs().min(10) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong nested min/max ensures must CE.
#[test]
fn check_rust_nested_min_max_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_nested_min_max_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn c(x: u64) -> u64 { x.max(1).min(10) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// wrapping_pow with small const exp encodes (mod 2^w).
#[test]
fn check_rust_encodes_wrapping_pow() {
    let tmp = unique_temp("assura_check_rust_wrapping_pow");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn p8(x: u8) -> u8 { x.wrapping_pow(2) }

/// @ensures result >= 0
fn p64(x: u64) -> u64 { x.wrapping_pow(3) }

/// @ensures result == 16
fn c(x: u8) -> u8 { 2u8.wrapping_pow(4) }

/// @ensures result == 1
fn z(x: u32) -> u32 { x.wrapping_pow(0) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong wrapping_pow ensures must CE.
#[test]
fn check_rust_wrapping_pow_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_wrapping_pow_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn p(x: u8) -> u8 { x.wrapping_pow(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width wrapping_pow for path params.
#[test]
fn check_rust_encodes_mid_width_wrapping_pow() {
    let tmp = unique_temp("assura_check_rust_mid_wp");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn p16(x: u16) -> u16 { x.wrapping_pow(3) }

/// @ensures result >= 0
fn p32(x: u32) -> u32 { x.wrapping_pow(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width wrapping_pow ensures must CE.
#[test]
fn check_rust_mid_width_wrapping_pow_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_wp_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn p(x: u16) -> u16 { x.wrapping_pow(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed wrapping_pow encodes (mod 2^w + reinterpret).
#[test]
fn check_rust_encodes_signed_wrapping_pow() {
    let tmp = unique_temp("assura_check_rust_signed_wp");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == result
fn p(x: i32) -> i32 { x.wrapping_pow(2) }

/// @ensures result == result
fn p64(x: i64) -> i64 { x.wrapping_pow(2) }

/// @ensures result == 1
fn z(x: i16) -> i16 { x.wrapping_pow(0) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong signed wrapping_pow ensures must CE.
#[test]
fn check_rust_signed_wrapping_pow_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_signed_wp_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn p(x: i32) -> i32 { x.wrapping_pow(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong i64 wrapping_pow ensures must CE.
#[test]
fn check_rust_i64_wrapping_pow_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i64_wp_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn p(x: i64) -> i64 { x.wrapping_pow(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u32 saturating_add with unsigned range requires.
#[test]
fn check_rust_u32_saturating_add() {
    let tmp = unique_temp("assura_check_rust_u32_sat");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= x
/// @ensures result <= 4294967295
fn f(x: u32) -> u32 { x.saturating_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "u32 sat should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// saturating_mul encodes with type-width clamp.
#[test]
fn check_rust_encodes_saturating_mul() {
    let tmp = unique_temp("assura_check_rust_sat_mul");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @requires x >= 0
/// @ensures result >= x
fn f(x: i64) -> i64 { x.saturating_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "sat mul: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// is_positive / is_negative method bodies encode as Bool cmp.
#[test]
fn check_rust_encodes_is_positive() {
    let tmp = unique_temp("assura_check_rust_is_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn f(x: i64) -> bool { x.is_positive() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// is_zero encodes as cmp eq 0.
#[test]
fn check_rust_encodes_is_zero() {
    let tmp = unique_temp("assura_check_rust_is_zero");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn f(x: i64) -> bool { x.is_zero() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Mid-width is_positive/is_negative/is_zero for path params.
#[test]
fn check_rust_encodes_mid_width_sign_predicates() {
    let tmp = unique_temp("assura_check_rust_mid_sign_pred");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn p(x: i16) -> bool { x.is_positive() }

/// @ensures result == true || result == false
fn n(x: i16) -> bool { x.is_negative() }

/// @ensures result == true || result == false
fn z(x: i16) -> bool { x.is_zero() }

/// @ensures result == true || result == false
fn p32(x: i32) -> bool { x.is_positive() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width is_positive ensures must CE.
#[test]
fn check_rust_mid_width_is_positive_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_is_pos_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == true
fn p(x: i16) -> bool { x.is_positive() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// clone/to_owned on ints encode as identity.
#[test]
fn check_rust_encodes_clone() {
    let tmp = unique_temp("assura_check_rust_clone");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x
fn f(x: i64) -> i64 { x.clone() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// signum encodes as clamp to [-1, 1] (single-block) and verifies range ensures.
#[test]
fn check_rust_encodes_signum() {
    let tmp = unique_temp("assura_check_rust_signum");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == -1 || result == 0 || result == 1
fn f(x: i64) -> i64 { x.signum() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Mid-width signum for path params.
#[test]
fn check_rust_encodes_mid_width_signum() {
    let tmp = unique_temp("assura_check_rust_mid_signum");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -1
/// @ensures result <= 1
fn s8(x: i8) -> i8 { x.signum() }

/// @ensures result >= -1
/// @ensures result <= 1
fn s16(x: i16) -> i16 { x.signum() }

/// @ensures result >= -1
/// @ensures result <= 1
fn s32(x: i32) -> i32 { x.signum() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i16 signum ensures must CE.
#[test]
fn check_rust_i16_signum_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i16_signum_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn s(x: i16) -> i16 { x.signum() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong i64 signum ensures must CE.
#[test]
fn check_rust_i64_signum_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i64_signum_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn s(x: i64) -> i64 { x.signum() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Nested signum in arith encodes (#1032); proves result in {-1,0,1,2}.
#[test]
fn check_rust_encodes_nested_signum() {
    let tmp = unique_temp("assura_check_rust_nested_signum");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 2
fn s(x: i64) -> i64 { x.signum() + 1 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// signum chains: abs, sum receiver, product with x (#1032 follow-through).
#[test]
fn check_rust_encodes_signum_chains() {
    let tmp = unique_temp("assura_check_rust_signum_chains");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 1
fn a(x: i64) -> i64 { x.signum().abs() }

/// @ensures result >= -1
/// @ensures result <= 1
fn t(x: i64, y: i64) -> i64 { (x + y).signum() }

/// @ensures result == x || result == -x || result == 0
fn m(x: i64) -> i64 { x.signum() * x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Associated i64::max / i64::from encode.
#[test]
fn check_rust_encodes_assoc_max_from() {
    let tmp = unique_temp("assura_check_rust_assoc");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x
fn m(x: i64) -> i64 { i64::max(x, x) }

/// @ensures result == x
fn f(x: i32) -> i64 { i64::from(x) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Logical && / || on bools encode and verify (0/1 mul / or-ne0).
#[test]
fn check_rust_encodes_bool_logic() {
    let tmp = unique_temp("assura_check_rust_bool_logic");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == (a && b)
fn both(a: bool, b: bool) -> bool { a && b }

/// @ensures result == (a || b)
fn either(a: bool, b: bool) -> bool { a || b }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "bool logic should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 2, "{stdout}");
}

/// Wrong bool || ensures must CE.
#[test]
fn check_rust_bool_logic_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_bool_logic_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == false
fn either(a: bool, b: bool) -> bool { a || b }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// is_multiple_of encodes mod/eq; into/as are identity on i64.
#[test]
fn check_rust_encodes_multiple_into_as() {
    let tmp = unique_temp("assura_check_rust_multiple_into");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == (x % 2 == 0)
fn even(x: i64) -> bool { x.is_multiple_of(2) }

/// @ensures result == true
fn by_one(x: i64) -> bool { x.is_multiple_of(1) }

/// @ensures result == true
fn by_neg_one(x: i64) -> bool { x.is_multiple_of(-1) }

/// @ensures result == x
fn id_into(x: i64) -> i64 { x.into() }

/// @ensures result == x
fn id_as(x: i64) -> i64 { x as i64 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "multiple/into/as should pass: {stdout}"
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 5, "{stdout}");
}

/// Mid-width is_multiple_of for path params.
#[test]
fn check_rust_encodes_mid_width_is_multiple_of() {
    let tmp = unique_temp("assura_check_rust_mid_imo");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn m16(x: u16) -> bool { x.is_multiple_of(2) }

/// @ensures result == true || result == false
fn s16(x: i16) -> bool { x.is_multiple_of(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width is_multiple_of ensures must CE.
#[test]
fn check_rust_mid_width_is_multiple_of_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_imo_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == true
fn m(x: u16) -> bool { x.is_multiple_of(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 is_multiple_of path params encode.
#[test]
fn check_rust_encodes_u64_is_multiple_of() {
    let tmp = unique_temp("assura_check_rust_u64_imult");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn m(x: u64) -> bool { x.is_multiple_of(3) }

/// @ensures result == true
fn one(x: u64) -> bool { x.is_multiple_of(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 is_multiple_of ensures must CE.
#[test]
fn check_rust_u64_is_multiple_of_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_imult_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == true
fn m(x: u64) -> bool { x.is_multiple_of(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 is_multiple_of with NonZeroU64 divisor.
#[test]
fn check_rust_encodes_u64_is_multiple_of_nonzero() {
    let tmp = unique_temp("assura_check_rust_u64_imo_nz");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
use std::num::NonZeroU64;

/// @ensures result == true || result == false
fn m(x: u64, d: NonZeroU64) -> bool { x.is_multiple_of(d.get()) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong NonZeroU64 is_multiple_of ensures must CE.
#[test]
fn check_rust_u64_is_multiple_of_nonzero_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_imo_nz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
use std::num::NonZeroU64;

/// @ensures result == true
fn m(x: u64, d: NonZeroU64) -> bool { x.is_multiple_of(d.get()) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// is_multiple_of with NonZeroU32 divisor (zero-including paths stay BNM).
#[test]
fn check_rust_encodes_is_multiple_of_nonzero() {
    let tmp = unique_temp("assura_check_rust_imo_nz");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
use std::num::NonZeroU32;

/// @ensures result == true || result == false
fn m(x: u32, d: NonZeroU32) -> bool { x.is_multiple_of(d.get()) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong is_multiple_of with NonZero divisor must CE (#1204 path live).
#[test]
fn check_rust_is_multiple_of_nonzero_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_imo_nz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
use std::num::NonZeroU32;

/// @ensures result == true
fn m(x: u32, d: NonZeroU32) -> bool { x.is_multiple_of(d.get()) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// abs_diff and ref/deref encode and verify.
#[test]
fn check_rust_encodes_abs_diff_ref() {
    let tmp = unique_temp("assura_check_rust_abs_diff");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn d(x: i64, y: i64) -> i64 { x.abs_diff(y) }

/// @ensures result == x
fn r(x: i64) -> i64 { *&x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "abs_diff/ref should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 2, "{stdout}");
}

/// Same-path peeps: abs_diff/min/max identity.
#[test]
fn check_rust_encodes_same_path_peeps() {
    let tmp = unique_temp("assura_check_rust_same_path");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0
fn d(x: i64) -> i64 { x.abs_diff(x) }

/// @ensures result == x
fn mn(x: i64) -> i64 { x.min(x) }

/// @ensures result == x
fn mx(x: i64) -> i64 { x.max(x) }

/// @ensures result == x
fn free(x: i64) -> i64 { min(x, x) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Mid-width abs for path params.
#[test]
fn check_rust_encodes_mid_width_abs() {
    let tmp = unique_temp("assura_check_rust_mid_abs");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn a8(x: i8) -> i8 { x.abs() }

/// @ensures result >= 0
fn a16(x: i16) -> i16 { x.abs() }

/// @ensures result >= 0
fn a32(x: i32) -> i32 { x.abs() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i16 abs ensures must CE.
#[test]
fn check_rust_i16_abs_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i16_abs_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn a(x: i16) -> i16 { x.abs() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// abs/saturating_abs().is_negative() peeps to false.
#[test]
fn check_rust_encodes_abs_never_negative() {
    let tmp = unique_temp("assura_check_rust_abs_nn");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == false
fn a(x: i64) -> bool { x.abs().is_negative() }

/// @ensures result == false
fn s(x: i64) -> bool { x.saturating_abs().is_negative() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// abs_diff(x,x).is_zero / is_positive peeps.
#[test]
fn check_rust_encodes_abs_diff_self_bool_peeps() {
    let tmp = unique_temp("assura_check_rust_ad_self");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true
fn z(x: i64) -> bool { x.abs_diff(x).is_zero() }

/// @ensures result == false
fn p(x: i64) -> bool { x.abs_diff(x).is_positive() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// PartialOrd methods (x.gt(&0)) encode via cmp + ref strip.
#[test]
fn check_rust_encodes_partial_ord() {
    let tmp = unique_temp("assura_check_rust_partial_ord");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == (x > 0)
fn pos(x: i64) -> bool { x.gt(&0) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "partial ord should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Mid-width min/max for path params.
#[test]
fn check_rust_encodes_mid_width_min_max() {
    let tmp = unique_temp("assura_check_rust_mid_minmax");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn mn(x: u16, y: u16) -> u16 { x.min(y) }

/// @ensures result == 0 || result != 0
fn mx(x: u16, y: u16) -> u16 { x.max(y) }

/// @ensures result == 0 || result != 0
fn smn(x: i16, y: i16) -> i16 { x.min(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width min ensures must CE.
#[test]
fn check_rust_mid_width_min_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_min_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn mn(x: u16, y: u16) -> u16 { x.min(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// i64::default / i64::MAX encode as const bodies.
#[test]
fn check_rust_encodes_default_minmax() {
    let tmp = unique_temp("assura_check_rust_default_minmax");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0
fn z(x: i64) -> i64 { i64::default() }

/// @ensures result == 9223372036854775807
fn mx(x: i64) -> i64 { i64::MAX }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "default/minmax should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// x.pow(2) encodes as mul and verifies square ensures.
#[test]
fn check_rust_encodes_pow() {
    let tmp = unique_temp("assura_check_rust_pow");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x * x
fn sq(x: i64) -> i64 { x.pow(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "pow should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// bool.not() and as_ref identity encode.
#[test]
fn check_rust_encodes_not_method() {
    let tmp = unique_temp("assura_check_rust_not");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == !a
fn n(a: bool) -> bool { a.not() }

/// @ensures result == x
fn r(x: i64) -> i64 { x.as_ref() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "not/as_ref should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Multi-let folds through &x / *y ref patterns.
#[test]
fn check_rust_encodes_multi_let_ref() {
    let tmp = unique_temp("assura_check_rust_multilet_ref");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x
fn f(x: i64) -> i64 { let y = &x; *y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "multi-let ref should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Narrowing `as i32` must not pretend to model the body (BNM).
#[test]
fn check_rust_narrowing_cast_body_not_modeled() {
    let tmp = unique_temp("assura_check_rust_narrow_cast");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x
fn f(x: i64) -> i32 { x as i32 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    // Should not claim verified body model; BNM or type issues
    assert!(
        v["body_not_modeled"].as_u64().unwrap_or(0) >= 1 || !out.status.success(),
        "narrowing cast must not soft-pass as verified body: {stdout}"
    );
    // specifically no false success with body_not_modeled=0 and verified>0 without model
    if out.status.success() {
        assert_ne!(v["body_not_modeled"], 0, "must BNM: {stdout}");
    }
}

/// Nested methods (abs then is_positive) encode and verify for non-min.
#[test]
fn check_rust_encodes_nested_methods() {
    let tmp = unique_temp("assura_check_rust_nested_methods");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    // Note: i64::MIN.abs() is not positive in Rust (overflow); range requires
    // include MIN, so avoid ensures that claim abs().is_positive() <=> x != 0.
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == (x > 0)
fn f(x: i64) -> bool { x.is_positive() }

/// @ensures result >= 0
fn g(x: i64) -> i64 { x.abs().max(0) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "nested/pos should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 2, "{stdout}");
}

/// Wrong pow body must counterexample, not BNM.
#[test]
fn check_rust_pow_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_pow_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x * x
fn sq(x: i64) -> i64 { x.pow(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "wrong pow should fail");
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{v}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width pow for path params.
#[test]
fn check_rust_encodes_mid_width_pow() {
    let tmp = unique_temp("assura_check_rust_mid_pow");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn p8(x: u8) -> u8 { x.pow(2) }

/// @ensures result == 0 || result != 0
fn p16(x: u16) -> u16 { x.pow(2) }

/// @ensures result == 0 || result != 0
fn s16(x: i16) -> i16 { x.pow(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width pow ensures must CE.
#[test]
fn check_rust_mid_width_pow_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_pow_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn p(x: u16) -> u16 { x.pow(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong && body must CE.
#[test]
fn check_rust_bool_logic_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_bool_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == (a && b)
fn both(a: bool, b: bool) -> bool { a || b }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Wrong is_multiple_of body must CE.
#[test]
fn check_rust_is_multiple_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_imo_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == (x % 2 == 0)
fn even(x: i64) -> bool { x.is_multiple_of(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// borrow/deref identity encode and verify.
#[test]
fn check_rust_encodes_borrow_deref() {
    let tmp = unique_temp("assura_check_rust_borrow_deref");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x
fn b(x: i64) -> i64 { x.borrow() }

/// @ensures result == x
fn d(x: i64) -> i64 { x.deref() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong PartialOrd method body must CE.
#[test]
fn check_rust_partial_ord_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_po_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == (x > 0)
fn pos(x: i64) -> bool { x.lt(&0) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Mid-width PartialOrd methods for path params.
#[test]
fn check_rust_encodes_mid_width_partial_ord() {
    let tmp = unique_temp("assura_check_rust_mid_partial_ord");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn g(x: u16, y: u16) -> bool { x.gt(&y) }

/// @ensures result == true || result == false
fn l(x: i16, y: i16) -> bool { x.lt(&y) }

/// @ensures result == true || result == false
fn e(x: u16, y: u16) -> bool { x.eq(&y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width PartialOrd ensures must CE.
#[test]
fn check_rust_mid_width_partial_ord_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_partial_ord_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == true
fn g(x: u16, y: u16) -> bool { x.gt(&y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 PartialOrd methods encode.
#[test]
fn check_rust_encodes_u64_partial_ord() {
    let tmp = unique_temp("assura_check_rust_u64_partial");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn lt(x: u64, y: u64) -> bool { x.lt(&y) }

/// @ensures result == true || result == false
fn ge(x: u64, y: u64) -> bool { x.ge(&y) }

/// @ensures result == true
fn eq(x: u64) -> bool { x.eq(&x) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 PartialOrd ensures must CE.
#[test]
fn check_rust_u64_partial_ord_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_partial_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == true
fn lt(x: u64, y: u64) -> bool { x.lt(&y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 binary comparisons encode.
#[test]
fn check_rust_encodes_u64_cmp() {
    let tmp = unique_temp("assura_check_rust_u64_cmp");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn lt(x: u64, y: u64) -> bool { x < y }

/// @ensures result == true || result == false
fn eq(x: u64, y: u64) -> bool { x == y }

/// @ensures result == true || result == false
fn ge(x: u64, y: u64) -> bool { x >= y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 comparison ensures must CE.
#[test]
fn check_rust_u64_cmp_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_cmp_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == true
fn lt(x: u64, y: u64) -> bool { x < y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 is_negative / is_positive / is_zero encode.
#[test]
fn check_rust_encodes_u64_is_sign() {
    let tmp = unique_temp("assura_check_rust_u64_is_sign");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == false
fn n(x: u64) -> bool { x.is_negative() }

/// @ensures result == true || result == false
fn p(x: u64) -> bool { x.is_positive() }

/// @ensures result == true || result == false
fn z(x: u64) -> bool { x.is_zero() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 is_zero ensures must CE.
#[test]
fn check_rust_u64_is_zero_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_is_zero_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == true
fn z(x: u64) -> bool { x.is_zero() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 abs / unsigned_abs encode (identity for non-neg).
#[test]
fn check_rust_encodes_u64_abs() {
    let tmp = unique_temp("assura_check_rust_u64_abs");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn a(x: u64) -> u64 { x.abs() }

/// @ensures result >= 0
fn ua(x: u64) -> u64 { x.unsigned_abs() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 abs ensures must CE.
#[test]
fn check_rust_u64_abs_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_abs_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn a(x: u64) -> u64 { x.abs() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong abs body must CE.
#[test]
fn check_rust_abs_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_abs_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result >= 0
fn a(x: i64) -> i64 { -x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Wrong is_zero body must CE.
#[test]
fn check_rust_is_zero_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_iz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == (x == 0)
fn z(x: i64) -> bool { x.is_positive() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Wrong default body must CE.
#[test]
fn check_rust_default_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_def_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn z(x: i64) -> i64 { 1 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Wrong clamp body must CE.
#[test]
fn check_rust_clamp_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_clamp_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 10
fn c(x: i64) -> i64 { x.clamp(-5, 5) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Wrong signum body must CE.
#[test]
fn check_rust_signum_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_signum_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == -1 || result == 0 || result == 1
fn s(x: i64) -> i64 { 2 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Multi-let through `as i64` encode (lossless cast).
#[test]
fn check_rust_encodes_multi_let_cast() {
    let tmp = unique_temp("assura_check_rust_multilet_cast");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x
fn f(x: i64) -> i64 { let y = x as i64; y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong abs_diff body must CE.
#[test]
fn check_rust_abs_diff_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_ad_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result >= 0
fn d(x: i64, y: i64) -> i64 { x - y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Nested wrapping_neg encodes via modular 2^w (no longer BNM).
#[test]
fn check_rust_encodes_nested_wrapping_neg() {
    let tmp = unique_temp("assura_check_rust_wrap_nest");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn w(x: i64) -> i64 { x.wrapping_neg() + 1 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong nested wrapping_neg ensures must CE.
#[test]
fn check_rust_nested_wrapping_neg_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_wrap_nest_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn w(x: i64) -> i64 { x.wrapping_neg() + 1 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// i64 wrapping_add encodes via synthetic 2^64 modulus (#1010).
#[test]
fn check_rust_encodes_i64_wrapping_add() {
    let tmp = unique_temp("assura_check_rust_i64_wrap");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -9223372036854775808
/// @ensures result <= 9223372036854775807
fn w(x: i64) -> i64 { x.wrapping_add(1) }

/// @ensures result >= -9223372036854775808
/// @ensures result <= 9223372036854775807
fn s(x: i64) -> i64 { x.wrapping_sub(1) }

/// @ensures result >= -9223372036854775808
/// @ensures result <= 9223372036854775807
fn m(x: i64) -> i64 { x.wrapping_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i64 wrapping_add ensures must CE (proves wrap of MAX is live).
#[test]
fn check_rust_i64_wrapping_add_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i64_wrap_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x + 1
fn w(x: i64) -> i64 { x.wrapping_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE on wrap of MAX: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong signed div_euclid ensures must CE.
#[test]
fn check_rust_signed_div_euclid_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_signed_div_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn d(x: i64) -> i64 { x.div_euclid(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong signed rem_euclid ensures must CE.
#[test]
fn check_rust_signed_rem_euclid_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_signed_rem_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result < 0
fn r(x: i64) -> i64 { x.rem_euclid(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !out.status.success(),
        "must CE (rem_euclid always >=0): {stdout}"
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// f32 bodies stay body_not_modeled (not false verified).
#[test]
fn check_rust_f32_body_not_modeled() {
    let tmp = unique_temp("assura_check_rust_f32_bnm");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0.0
fn f(x: f32) -> f32 { x.abs() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert!(
        v["body_not_modeled"].as_u64().unwrap_or(0) >= 1,
        "f32 must BNM: {stdout}"
    );
    assert!(!out.status.success());
}

/// String bodies stay body_not_modeled (not false verified).
#[test]
fn check_rust_string_body_not_modeled() {
    let tmp = unique_temp("assura_check_rust_string_bnm");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result.len() >= 0
fn f(x: &str) -> usize { x.len() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert!(
        v["body_not_modeled"].as_u64().unwrap_or(0) >= 1,
        "string must BNM: {stdout}"
    );
    assert!(!out.status.success());
}

/// to_be/to_le stay body_not_modeled (host-endian; not encoded).
#[test]
fn check_rust_to_be_body_not_modeled() {
    let tmp = unique_temp("assura_check_rust_to_be_bnm");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn t(x: u32) -> u32 { x.to_be() }

/// @ensures result >= 0
fn l(x: u32) -> u32 { x.to_le() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert!(
        v["body_not_modeled"].as_u64().unwrap_or(0) >= 1,
        "to_be/to_le must BNM: {stdout}"
    );
    assert!(!out.status.success());
}

/// checked_/overflowing_* stay body_not_modeled (Option/tuple returns unencoded).
#[test]
fn check_rust_checked_overflowing_body_not_modeled() {
    let tmp = unique_temp("assura_check_rust_checked_bnm");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result.is_some()
fn c(x: i64) -> Option<i64> { x.checked_add(1) }

/// @ensures result.0 >= x
fn o(x: i64) -> (i64, bool) { x.overflowing_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert!(
        v["body_not_modeled"].as_u64().unwrap_or(0) >= 1,
        "checked/overflowing must BNM not soft-pass: {stdout}"
    );
    assert!(!out.status.success());
}

/// Unsigned wrapping_add encodes via mod 2^w (#1010 partial).
#[test]
fn check_rust_encodes_u8_wrapping_add() {
    let tmp = unique_temp("assura_check_rust_u8_wrap");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 255
fn w(x: u8) -> u8 { x.wrapping_add(1) }

/// @ensures result >= 0
/// @ensures result <= 255
fn s(x: u8) -> u8 { x.wrapping_sub(1) }

/// @ensures result >= 0
/// @ensures result <= 255
fn m(x: u8) -> u8 { x.wrapping_mul(3) }

/// @ensures result >= 0
/// @ensures result <= 255
fn n(x: u8) -> u8 { x.wrapping_neg() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// u16 wrapping_add encodes via mod 65536.
#[test]
fn check_rust_encodes_u16_wrapping_add() {
    let tmp = unique_temp("assura_check_rust_u16_wrap");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 65535
fn w(x: u16) -> u16 { x.wrapping_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u8 wrapping_add ensures must CE.
#[test]
fn check_rust_u8_wrapping_add_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u8_wrap_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x + 1
fn w(x: u8) -> u8 { x.wrapping_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE on wrap of 255: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// i8 wrapping_add encodes via mod 256 + signed reinterpret (#1010 partial).
#[test]
fn check_rust_encodes_i8_wrapping_add() {
    let tmp = unique_temp("assura_check_rust_i8_wrap");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -128
/// @ensures result <= 127
fn w(x: i8) -> i8 { x.wrapping_add(1) }

/// @ensures result >= -128
/// @ensures result <= 127
fn s(x: i8) -> i8 { x.wrapping_sub(1) }

/// @ensures result >= -128
/// @ensures result <= 127
fn m(x: i8) -> i8 { x.wrapping_mul(2) }

/// @ensures result >= -2147483648
/// @ensures result <= 2147483647
fn w32(x: i32) -> i32 { x.wrapping_add(1) }

/// @ensures result >= -32768
/// @ensures result <= 32767
fn w16s(x: i16) -> i16 { x.wrapping_add(1) }

/// @ensures result >= -2147483648
/// @ensures result <= 2147483647
fn m32(x: i32) -> i32 { x.wrapping_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i8 wrapping_add ensures must CE (proves wrap of 127 is live).
#[test]
fn check_rust_i8_wrapping_add_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i8_wrap_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x + 1
fn w(x: i8) -> i8 { x.wrapping_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE on wrap of 127: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width wrapping_sub for path params.
#[test]
fn check_rust_encodes_mid_width_wrapping_sub() {
    let tmp = unique_temp("assura_check_rust_mid_wsub");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn w16(x: u16) -> u16 { x.wrapping_sub(1) }

/// @ensures result == 0 || result != 0
fn w32(x: u32) -> u32 { x.wrapping_sub(1) }

/// @ensures result == 0 || result != 0
fn s16(x: i16) -> i16 { x.wrapping_sub(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u16 wrapping_sub ensures must CE.
#[test]
fn check_rust_u16_wrapping_sub_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u16_wsub_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn w(x: u16) -> u16 { x.wrapping_sub(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width wrapping_mul for path params.
#[test]
fn check_rust_encodes_mid_width_wrapping_mul() {
    let tmp = unique_temp("assura_check_rust_mid_wmul");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn w16(x: u16) -> u16 { x.wrapping_mul(2) }

/// @ensures result == 0 || result != 0
fn w32(x: u32) -> u32 { x.wrapping_mul(2) }

/// @ensures result == 0 || result != 0
fn s16(x: i16) -> i16 { x.wrapping_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u16 wrapping_mul ensures must CE.
#[test]
fn check_rust_u16_wrapping_mul_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u16_wmul_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn w(x: u16) -> u16 { x.wrapping_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong i32 wrapping_mul ensures must CE (proves double-mod mul is live).
#[test]
fn check_rust_i32_wrapping_mul_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i32_mul_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x * 2
fn m(x: i32) -> i32 { x.wrapping_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE on overflow mul: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong i64 wrapping_mul ensures must CE (synthetic 2^64 modulus live).
#[test]
fn check_rust_i64_wrapping_mul_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i64_mul_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x * 2
fn m(x: i64) -> i64 { x.wrapping_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !out.status.success(),
        "must CE on i64 overflow mul: {stdout}"
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u16/u32 wrapping_add encode via mod 2^w (#1010 partial).
#[test]
fn check_rust_encodes_u16_u32_wrapping_add() {
    let tmp = unique_temp("assura_check_rust_u16u32_wrap");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 65535
fn w16(x: u16) -> u16 { x.wrapping_add(1) }

/// @ensures result >= 0
/// @ensures result <= 4294967295
fn w32(x: u32) -> u32 { x.wrapping_add(1) }

/// @ensures result >= 0
/// @ensures result <= 4294967295
fn m32(x: u32) -> u32 { x.wrapping_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Mid-width both-variable wrapping_add for path params.
#[test]
fn check_rust_encodes_mid_width_wrapping_add_both_var() {
    let tmp = unique_temp("assura_check_rust_mid_wadd_both");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn w(x: u16, y: u16) -> u16 { x.wrapping_add(y) }

/// @ensures result == 0 || result != 0
fn s(x: i16, y: i16) -> i16 { x.wrapping_add(y) }

/// @ensures result == 0 || result != 0
fn w32(x: u32, y: u32) -> u32 { x.wrapping_add(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width both-var wrapping_add ensures must CE.
#[test]
fn check_rust_mid_width_wrapping_add_both_var_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_wadd_both_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn w(x: u16, y: u16) -> u16 { x.wrapping_add(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width both-variable wrapping_sub for path params.
#[test]
fn check_rust_encodes_mid_width_wrapping_sub_both_var() {
    let tmp = unique_temp("assura_check_rust_mid_wsub_both");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn w(x: u16, y: u16) -> u16 { x.wrapping_sub(y) }

/// @ensures result == 0 || result != 0
fn s(x: i16, y: i16) -> i16 { x.wrapping_sub(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width both-var wrapping_sub ensures must CE.
#[test]
fn check_rust_mid_width_wrapping_sub_both_var_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_wsub_both_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn w(x: u16, y: u16) -> u16 { x.wrapping_sub(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width both-variable wrapping_mul for path params.
#[test]
fn check_rust_encodes_mid_width_wrapping_mul_both_var() {
    let tmp = unique_temp("assura_check_rust_mid_wmul_both");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn w(x: u16, y: u16) -> u16 { x.wrapping_mul(y) }

/// @ensures result == 0 || result != 0
fn s(x: i16, y: i16) -> i16 { x.wrapping_mul(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width both-var wrapping_mul ensures must CE.
#[test]
fn check_rust_mid_width_wrapping_mul_both_var_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_wmul_both_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn w(x: u16, y: u16) -> u16 { x.wrapping_mul(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed wrapping_shl by const encodes via mul+double-mod+reinterpret.
#[test]
fn check_rust_encodes_signed_wrapping_shl() {
    let tmp = unique_temp("assura_check_rust_signed_shl");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -128
/// @ensures result <= 127
fn s(x: i8) -> i8 { x.wrapping_shl(1) }

/// @ensures result >= -9223372036854775808
/// @ensures result <= 9223372036854775807
fn l(x: i64) -> i64 { x.wrapping_shl(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u8 rotate_left/right encode via case-sum.
#[test]
fn check_rust_encodes_variable_u8_rotate() {
    let tmp = unique_temp("assura_check_rust_var_rot");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 255
fn r(x: u8, n: u32) -> u8 { x.rotate_left(n) }

/// @ensures result >= 0
/// @ensures result <= 255
fn rr(x: u8, n: u32) -> u8 { x.rotate_right(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong variable rotate_left ensures must CE.
#[test]
fn check_rust_variable_u8_rotate_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_rot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn r(x: u8, n: u32) -> u8 { x.rotate_left(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Variable bitops with const mask (x & 1) encode via bit products.
#[test]
fn check_rust_encodes_variable_bitop_const_mask() {
    let tmp = unique_temp("assura_check_rust_var_bitop");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result <= 1
/// @ensures result >= 0
fn low(x: u8) -> u8 { x & 1 }

/// @ensures result >= 0
/// @ensures result <= 255
fn set_hi(x: u8) -> u8 { x | 0x80 }

/// @ensures result >= 0
/// @ensures result <= 255
fn flip(x: u8) -> u8 { x ^ 0xFF }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong ensures on variable & const mask must CE (bit model is live).
#[test]
fn check_rust_variable_bitop_const_mask_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_bitop_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn low(x: u8) -> u8 { x & 1 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width unsigned bitops with const mask.
#[test]
fn check_rust_encodes_mid_width_bitop_const_mask() {
    let tmp = unique_temp("assura_check_rust_mid_bitop");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn low16(x: u16) -> u16 { x & 0x00ff }

/// @ensures result >= 0
fn set16(x: u16) -> u16 { x | 1 }

/// @ensures result >= 0
fn flip16(x: u16) -> u16 { x ^ 0xffff }

/// @ensures result >= 0
fn low32(x: u32) -> u32 { x & 0xff }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width bitop ensures must CE.
#[test]
fn check_rust_mid_width_bitop_const_mask_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_bitop_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn low(x: u16) -> u16 { x & 0x00ff }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed mid-width bitops with const mask.
#[test]
fn check_rust_encodes_signed_mid_width_bitop_const_mask() {
    let tmp = unique_temp("assura_check_rust_signed_mid_bitop");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn a16(x: i16) -> i16 { x & 0x00ff }

/// @ensures result == 0 || result != 0
fn o32(x: i32) -> i32 { x | 1 }

/// @ensures result == 0 || result != 0
fn x16(x: i16) -> i16 { x ^ 0x00ff }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong signed mid-width bitop ensures must CE.
#[test]
fn check_rust_signed_mid_width_bitop_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_signed_mid_bitop_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn a(x: i16) -> i16 { x & 0x00ff }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed i8 bitops with const mask encode via unsigned bit-pattern map.
#[test]
fn check_rust_encodes_signed_bitop_const_mask() {
    let tmp = unique_temp("assura_check_rust_signed_bitop");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -128
/// @ensures result <= 127
fn low(x: i8) -> i8 { x & 1 }

/// @ensures result >= -128
/// @ensures result <= 127
fn flip(x: i8) -> i8 { x ^ -1 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong signed bitop ensures must CE.
#[test]
fn check_rust_signed_bitop_const_mask_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_signed_bitop_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn low(x: i8) -> i8 { x & 1 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Both-variable unsigned bitops encode via bit products.
#[test]
fn check_rust_encodes_both_variable_bitops() {
    let tmp = unique_temp("assura_check_rust_both_var_bitop");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 255
fn band(x: u8, y: u8) -> u8 { x & y }

/// @ensures result >= 0
/// @ensures result <= 255
fn bor(x: u8, y: u8) -> u8 { x | y }

/// @ensures result >= 0
/// @ensures result <= 255
fn bxor(x: u8, y: u8) -> u8 { x ^ y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// u64 both-variable bitops for path params.
#[test]
fn check_rust_encodes_u64_both_variable_bitops() {
    let tmp = unique_temp("assura_check_rust_u64_both_var_bitop");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn band(x: u64, y: u64) -> u64 { x & y }

/// @ensures result == 0 || result != 0
fn bor(x: u64, y: u64) -> u64 { x | y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 both-variable bitop ensures must CE.
#[test]
fn check_rust_u64_both_variable_bitops_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_both_var_bitop_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn band(x: u64, y: u64) -> u64 { x & y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong both-variable bitop ensures must CE.
#[test]
fn check_rust_both_variable_bitops_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_both_var_bitop_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn band(x: u8, y: u8) -> u8 { x & y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed both-variable bitops encode via unsigned bit-pattern map (#1171).
#[test]
fn check_rust_encodes_signed_both_variable_bitops() {
    let tmp = unique_temp("assura_check_rust_signed_both_var_bitop");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -128
/// @ensures result <= 127
fn band(x: i8, y: i8) -> i8 { x & y }

/// @ensures result >= -128
/// @ensures result <= 127
fn bor(x: i8, y: i8) -> i8 { x | y }

/// @ensures result >= -128
/// @ensures result <= 127
fn bxor(x: i8, y: i8) -> i8 { x ^ y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong signed both-variable bitop ensures must CE (#1171).
#[test]
fn check_rust_signed_both_variable_bitops_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_signed_both_var_bitop_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn band(x: i8, y: i8) -> i8 { x & y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width both-variable bitops for path params.
#[test]
fn check_rust_encodes_mid_width_both_variable_bitops() {
    let tmp = unique_temp("assura_check_rust_mid_both_var_bitop");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn band(x: u16, y: u16) -> u16 { x & y }

/// @ensures result >= 0
fn bor(x: u32, y: u32) -> u32 { x | y }

/// @ensures result == 0 || result != 0
fn band_i(x: i16, y: i16) -> i16 { x & y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width both-variable bitop ensures must CE.
#[test]
fn check_rust_mid_width_both_variable_bitops_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_both_var_bitop_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn band(x: u16, y: u16) -> u16 { x & y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Variable u16/u32 rotate_left for path params.
#[test]
fn check_rust_encodes_u16_u32_rotate() {
    let tmp = unique_temp("assura_check_rust_u16_u32_rot");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn r16(x: u16) -> u16 { x.rotate_left(1) }

/// @ensures result == 0 || result != 0
fn r32(x: u32) -> u32 { x.rotate_left(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Signed i16/i32 rotate_left for path params.
#[test]
fn check_rust_encodes_i16_i32_rotate() {
    let tmp = unique_temp("assura_check_rust_i16_i32_rot");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn r16(x: i16) -> i16 { x.rotate_left(1) }

/// @ensures result == 0 || result != 0
fn r32(x: i32) -> i32 { x.rotate_left(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u16 rotate_left ensures must CE.
#[test]
fn check_rust_u16_rotate_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u16_rot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn r(x: u16) -> u16 { x.rotate_left(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong i32 rotate_left ensures must CE.
#[test]
fn check_rust_i32_rotate_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i32_rot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn r(x: i32) -> i32 { x.rotate_left(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width rotate with variable shift for path params.
#[test]
fn check_rust_encodes_mid_width_rotate_var_k() {
    let tmp = unique_temp("assura_check_rust_mid_rot_var_k");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn rl(x: u16, k: u32) -> u16 { x.rotate_left(k) }

/// @ensures result == 0 || result != 0
fn rr(x: u16, k: u32) -> u16 { x.rotate_right(k) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width var-k rotate_left ensures must CE.
#[test]
fn check_rust_mid_width_rotate_var_k_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_rot_var_k_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn rl(x: u16, k: u32) -> u16 { x.rotate_left(k) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width rotate_right for path params.
#[test]
fn check_rust_encodes_mid_width_rotate_right() {
    let tmp = unique_temp("assura_check_rust_mid_rotr");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn r16(x: u16) -> u16 { x.rotate_right(1) }

/// @ensures result == 0 || result != 0
fn r32(x: u32) -> u32 { x.rotate_right(1) }

/// @ensures result == 0 || result != 0
fn s16(x: i16) -> i16 { x.rotate_right(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u32 rotate_right ensures must CE.
#[test]
fn check_rust_u32_rotate_right_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u32_rotr_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn r(x: u32) -> u32 { x.rotate_right(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Variable u64 rotate_left encodes via 64-case-sum (#1172).
/// Range uses `result >= 0` (u64::MAX does not fit as an i64 ensures lit).
/// Full i64 signed range can cancel under default SMT timeout; soundness is
/// covered by the CE test and unit IR parse (same pattern as wrapping_shl).
#[test]
fn check_rust_encodes_variable_u64_rotate() {
    let tmp = unique_temp("assura_check_rust_u64_rotate");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn rot(x: u64, n: u32) -> u64 { x.rotate_left(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 rotate_left ensures must CE.
#[test]
fn check_rust_u64_rotate_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_rotate_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn rot(x: u64, n: u32) -> u64 { x.rotate_left(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 rotate_right path params encode.
#[test]
fn check_rust_encodes_u64_rotate_right() {
    let tmp = unique_temp("assura_check_rust_u64_ror");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn rot(x: u64, n: u32) -> u64 { x.rotate_right(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong variable i64 rotate ensures must CE (#1172).
#[test]
fn check_rust_variable_i64_rotate_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i64_rotate_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn rot(x: i64, n: u32) -> i64 { x.rotate_left(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 is_power_of_two encodes via 64-pot enum (#1173).
#[test]
fn check_rust_encodes_u64_is_power_of_two() {
    let tmp = unique_temp("assura_check_rust_u64_pot");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn p(x: u64) -> bool { x.is_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 is_power_of_two ensures must CE (#1173).
#[test]
fn check_rust_u64_is_power_of_two_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_pot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == true
fn p(x: u64) -> bool { x.is_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Variable unsigned ilog2/ilog10 encode for path params (#1174).
#[test]
fn check_rust_encodes_variable_ilog() {
    let tmp = unique_temp("assura_check_rust_var_ilog");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 7
fn ig(x: u8) -> u32 { x.ilog2() }

/// @ensures result >= 0
/// @ensures result <= 2
fn l10(x: u8) -> u32 { x.ilog10() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong variable ilog2 ensures must CE (#1174).
#[test]
fn check_rust_variable_ilog_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_ilog_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn ig(x: u8) -> u32 { x.ilog2() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong variable ilog10 ensures must CE.
#[test]
fn check_rust_variable_ilog10_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_ilog10_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn l10(x: u8) -> u32 { x.ilog10() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Variable u16 ilog2/ilog10 encode for path params.
/// ilog2 upper bound can cancel under CI solver budget (16-step ladder).
#[test]
fn check_rust_encodes_variable_ilog_u16() {
    let tmp = unique_temp("assura_check_rust_var_ilog_u16");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn ig(x: u16) -> u32 { x.ilog2() }

/// @ensures result >= 0
/// @ensures result <= 4
fn l10(x: u16) -> u32 { x.ilog10() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u16 ilog2 ensures must CE.
#[test]
fn check_rust_variable_ilog_u16_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_ilog_u16_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn ig(x: u16) -> u32 { x.ilog2() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Variable u32 ilog2/ilog10 encode for path params.
/// ilog2 upper bound (result <= 31) times out on the 32-bit ladder; keep
/// non-timeout-safe bounds only (same pattern as large isqrt).
#[test]
fn check_rust_encodes_variable_ilog_u32() {
    let tmp = unique_temp("assura_check_rust_var_ilog_u32");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn ig(x: u32) -> u32 { x.ilog2() }

/// @ensures result >= 0
/// @ensures result <= 9
fn l10(x: u32) -> u32 { x.ilog10() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u32 ilog2 ensures must CE.
#[test]
fn check_rust_variable_ilog_u32_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_ilog_u32_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn ig(x: u32) -> u32 { x.ilog2() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed path-param ilog2 (positivity gate; a<=0 models as 0).
#[test]
fn check_rust_encodes_signed_ilog2() {
    let tmp = unique_temp("assura_check_rust_signed_ilog");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn ig8(x: i8) -> u32 { x.ilog2() }

/// @ensures result >= 0
fn ig16(x: i16) -> u32 { x.ilog2() }

/// @ensures result >= 0
fn ig32(x: i32) -> u32 { x.ilog2() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u64 ilog2 for path params.
#[test]
fn check_rust_encodes_variable_ilog2_u64() {
    let tmp = unique_temp("assura_check_rust_var_ilog2_u64");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn ig(x: u64) -> u32 { x.ilog2() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u64 ilog10 for path params.
#[test]
fn check_rust_encodes_variable_ilog10_u64() {
    let tmp = unique_temp("assura_check_rust_var_ilog10_u64");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn l(x: u64) -> u32 { x.ilog10() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 ilog10 ensures must CE.
#[test]
fn check_rust_variable_ilog10_u64_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_ilog10_u64_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn l(x: u64) -> u32 { x.ilog10() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 ilog2 ensures must CE.
#[test]
fn check_rust_variable_ilog2_u64_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_ilog2_u64_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn ig(x: u64) -> u32 { x.ilog2() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong signed ilog2 ensures must CE.
#[test]
fn check_rust_signed_ilog2_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_signed_ilog_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn ig(x: i8) -> u32 { x.ilog2() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed path-param ilog10 (positivity gate; a<=0 models as 0).
#[test]
fn check_rust_encodes_signed_ilog10() {
    let tmp = unique_temp("assura_check_rust_signed_ilog10");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn l8(x: i8) -> u32 { x.ilog10() }

/// @ensures result >= 0
fn l16(x: i16) -> u32 { x.ilog10() }

/// @ensures result >= 0
/// @ensures result <= 9
fn l32(x: i32) -> u32 { x.ilog10() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong signed ilog10 ensures must CE.
#[test]
fn check_rust_signed_ilog10_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_signed_ilog10_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn l(x: i8) -> u32 { x.ilog10() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Variable next_power_of_two for unsigned path params (#1185).
#[test]
fn check_rust_encodes_variable_next_power_of_two() {
    let tmp = unique_temp("assura_check_rust_var_npot");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 255
fn np(x: u8) -> u8 { x.next_power_of_two() }

/// @ensures result >= 0
/// @ensures result <= 255
fn wnp(x: u8) -> u8 { x.wrapping_next_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable next_power_of_two for u16 path params.
#[test]
fn check_rust_encodes_variable_next_power_of_two_u16() {
    let tmp = unique_temp("assura_check_rust_var_npot_u16");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn np(x: u16) -> u16 { x.next_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable next_power_of_two for u32 path params (encode surface ≤32).
#[test]
fn check_rust_encodes_variable_next_power_of_two_u32() {
    let tmp = unique_temp("assura_check_rust_var_npot_u32");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn np(x: u32) -> u32 { x.next_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u64 next_power_of_two / wrapping_next_power_of_two for path params.
#[test]
fn check_rust_encodes_variable_next_power_of_two_u64() {
    let tmp = unique_temp("assura_check_rust_var_npot_u64");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn np(x: u64) -> u64 { x.next_power_of_two() }

/// @ensures result >= 0
fn wnp(x: u64) -> u64 { x.wrapping_next_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 next_power_of_two ensures must CE.
#[test]
fn check_rust_variable_next_power_of_two_u64_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_npot_u64_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn np(x: u64) -> u64 { x.next_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u32 next_power_of_two ensures must CE.
#[test]
fn check_rust_variable_next_power_of_two_u32_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_npot_u32_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn np(x: u32) -> u32 { x.next_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u16 next_power_of_two ensures must CE.
#[test]
fn check_rust_variable_next_power_of_two_u16_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_npot_u16_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn np(x: u16) -> u16 { x.next_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong wrapping_next_power_of_two ensures must CE.
#[test]
fn check_rust_wrapping_next_power_of_two_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_wnp_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn wnp(x: u8) -> u8 { x.wrapping_next_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Variable wrapping_next_power_of_two for u16 path params.
#[test]
fn check_rust_encodes_wrapping_next_power_of_two_u16() {
    let tmp = unique_temp("assura_check_rust_wnp_u16");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn wnp(x: u16) -> u16 { x.wrapping_next_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u16 wrapping_next_power_of_two ensures must CE.
#[test]
fn check_rust_wrapping_next_power_of_two_u16_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_wnp_u16_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn wnp(x: u16) -> u16 { x.wrapping_next_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Variable wrapping_next_power_of_two for u32 path params.
#[test]
fn check_rust_encodes_wrapping_next_power_of_two_u32() {
    let tmp = unique_temp("assura_check_rust_wnp_u32");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn wnp(x: u32) -> u32 { x.wrapping_next_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u32 wrapping_next_power_of_two ensures must CE.
#[test]
fn check_rust_wrapping_next_power_of_two_u32_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_wnp_u32_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn wnp(x: u32) -> u32 { x.wrapping_next_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong variable next_power_of_two ensures must CE (#1185).
#[test]
fn check_rust_variable_next_power_of_two_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_npot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn np(x: u8) -> u8 { x.next_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 const-mask bitops and !x encode (#1186).
#[test]
fn check_rust_encodes_u64_bitops_and_bitnot() {
    let tmp = unique_temp("assura_check_rust_u64_bit");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn low(x: u64) -> u64 { x & 1 }

/// @ensures result >= 0
fn flip(x: u64) -> u64 { !x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 bitop ensures must CE (#1186).
#[test]
fn check_rust_u64_bitops_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_bit_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn low(x: u64) -> u64 { x & 1 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Variable isqrt for u8 path params (ladder encode).
#[test]
fn check_rust_encodes_variable_isqrt() {
    let tmp = unique_temp("assura_check_rust_var_isqrt");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 15
fn sq(x: u8) -> u8 { x.isqrt() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable isqrt for u16 path params (≤16-bit ladder).
#[test]
fn check_rust_encodes_variable_isqrt_u16() {
    let tmp = unique_temp("assura_check_rust_var_isqrt_u16");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn sq(x: u16) -> u16 { x.isqrt() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u32 isqrt for path params (binary-search encode).
#[test]
fn check_rust_encodes_variable_isqrt_u32() {
    let tmp = unique_temp("assura_check_rust_var_isqrt_u32");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn sq(x: u32) -> u32 { x.isqrt() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u64 isqrt for path params (32-iter binary-search encode).
#[test]
fn check_rust_encodes_variable_isqrt_u64() {
    let tmp = unique_temp("assura_check_rust_var_isqrt_u64");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn sq(x: u64) -> u64 { x.isqrt() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 isqrt ensures must CE.
#[test]
fn check_rust_variable_isqrt_u64_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_isqrt_u64_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn sq(x: u64) -> u64 { x.isqrt() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u32 isqrt ensures must CE.
#[test]
fn check_rust_variable_isqrt_u32_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_isqrt_u32_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn sq(x: u32) -> u32 { x.isqrt() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u16 isqrt ensures must CE (ladder encode live for 16-bit).
#[test]
fn check_rust_variable_isqrt_u16_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_isqrt_u16_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn sq(x: u16) -> u16 { x.isqrt() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong variable isqrt ensures must CE.
#[test]
fn check_rust_variable_isqrt_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_isqrt_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn sq(x: u8) -> u8 { x.isqrt() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed path-param count_ones via bit-pattern map.
#[test]
fn check_rust_encodes_signed_count_ones() {
    let tmp = unique_temp("assura_check_rust_signed_count_ones");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 8
fn c(x: i8) -> u32 { x.count_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Signed path-param count_zeros via bits - count_ones.
#[test]
fn check_rust_encodes_signed_count_zeros() {
    let tmp = unique_temp("assura_check_rust_signed_count_zeros");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 8
fn c(x: i8) -> u32 { x.count_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Signed i16/i32 count_ones/count_zeros for path params.
#[test]
fn check_rust_encodes_i16_i32_count_ones() {
    let tmp = unique_temp("assura_check_rust_i16_i32_count");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 16
fn c16(x: i16) -> u32 { x.count_ones() }

/// @ensures result >= 0
/// @ensures result <= 16
fn z16(x: i16) -> u32 { x.count_zeros() }

/// @ensures result >= 0
/// @ensures result <= 32
fn c32(x: i32) -> u32 { x.count_ones() }

/// @ensures result >= 0
/// @ensures result <= 32
fn z32(x: i32) -> u32 { x.count_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i16 count_ones ensures must CE.
#[test]
fn check_rust_i16_count_ones_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i16_count_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn c(x: i16) -> u32 { x.count_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong i32 count_zeros ensures must CE.
#[test]
fn check_rust_i32_count_zeros_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i32_zeros_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn z(x: i32) -> u32 { x.count_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed path-param trailing_zeros / leading_zeros via bit-pattern map.
#[test]
fn check_rust_encodes_signed_trailing_leading_zeros() {
    let tmp = unique_temp("assura_check_rust_signed_tz_lz");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 8
fn t(x: i8) -> u32 { x.trailing_zeros() }

/// @ensures result >= 0
/// @ensures result <= 8
fn l(x: i8) -> u32 { x.leading_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Signed i16/i32 trailing_zeros/leading_zeros for path params.
#[test]
fn check_rust_encodes_i16_i32_trailing_leading_zeros() {
    let tmp = unique_temp("assura_check_rust_i16_i32_tz_lz");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn t16(x: i16) -> u32 { x.trailing_zeros() }

/// @ensures result >= 0
fn l16(x: i16) -> u32 { x.leading_zeros() }

/// @ensures result >= 0
fn t32(x: i32) -> u32 { x.trailing_zeros() }

/// @ensures result >= 0
fn l32(x: i32) -> u32 { x.leading_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i16 trailing_zeros ensures must CE.
#[test]
fn check_rust_i16_trailing_zeros_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i16_tz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn t(x: i16) -> u32 { x.trailing_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong i32 leading_zeros ensures must CE.
#[test]
fn check_rust_i32_leading_zeros_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i32_lz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn l(x: i32) -> u32 { x.leading_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Path-param trailing_ones / leading_ones via NOT + zeros product.
#[test]
fn check_rust_encodes_trailing_leading_ones() {
    let tmp = unique_temp("assura_check_rust_tlo");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 8
fn t(x: u8) -> u32 { x.trailing_ones() }

/// @ensures result >= 0
/// @ensures result <= 8
fn l(x: u8) -> u32 { x.leading_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u16 trailing_ones/leading_ones for path params.
#[test]
fn check_rust_encodes_u16_trailing_leading_ones() {
    let tmp = unique_temp("assura_check_rust_u16_tlo");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn t(x: u16) -> u32 { x.trailing_ones() }

/// @ensures result >= 0
fn l(x: u16) -> u32 { x.leading_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u32 trailing_ones/leading_ones for path params.
#[test]
fn check_rust_encodes_u32_trailing_leading_ones() {
    let tmp = unique_temp("assura_check_rust_u32_tlo");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn t(x: u32) -> u32 { x.trailing_ones() }

/// @ensures result >= 0
fn l(x: u32) -> u32 { x.leading_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Signed i16/i32 trailing_ones/leading_ones for path params.
#[test]
fn check_rust_encodes_i16_i32_trailing_leading_ones() {
    let tmp = unique_temp("assura_check_rust_i16_i32_tlo");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn t16(x: i16) -> u32 { x.trailing_ones() }

/// @ensures result >= 0
fn l16(x: i16) -> u32 { x.leading_ones() }

/// @ensures result >= 0
fn t32(x: i32) -> u32 { x.trailing_ones() }

/// @ensures result >= 0
fn l32(x: i32) -> u32 { x.leading_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i16 trailing_ones ensures must CE.
#[test]
fn check_rust_i16_trailing_ones_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i16_to_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn t(x: i16) -> u32 { x.trailing_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong i32 leading_ones ensures must CE.
#[test]
fn check_rust_i32_leading_ones_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i32_lo_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn l(x: i32) -> u32 { x.leading_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u16 trailing_ones ensures must CE.
#[test]
fn check_rust_u16_trailing_ones_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u16_to_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn t(x: u16) -> u32 { x.trailing_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u32 leading_ones ensures must CE.
#[test]
fn check_rust_u32_leading_ones_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u32_lo_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn l(x: u32) -> u32 { x.leading_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed path-param reverse_bits via bit-pattern map + reinterpret.
#[test]
fn check_rust_encodes_signed_reverse_bits() {
    let tmp = unique_temp("assura_check_rust_signed_rev");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 1 || result != 1
fn r(x: i8) -> i8 { x.reverse_bits() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Signed path-param swap_bytes via bit-pattern map + reinterpret.
#[test]
fn check_rust_encodes_signed_swap_bytes() {
    let tmp = unique_temp("assura_check_rust_signed_swap");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 1 || result != 1
fn s(x: i16) -> i16 { x.swap_bytes() }

/// @ensures result == 0 || result != 0
fn s32(x: i32) -> i32 { x.swap_bytes() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Nested is_power_of_two inherits path-param pot width (#1034).
#[test]
fn check_rust_encodes_nested_is_power_of_two() {
    let tmp = unique_temp("assura_check_rust_nested_pot");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn n(x: u8) -> bool { (x + 1).is_power_of_two() }

/// @ensures result == true || result == false
fn m(x: u8) -> bool { (x * 2).is_power_of_two() }

/// @ensures result == true || result == false
fn w(x: u8) -> bool { x.wrapping_add(1).is_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong nested pot ensures must CE.
#[test]
fn check_rust_nested_is_power_of_two_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_nested_pot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == true
fn n(x: u8) -> bool { (x + 1).is_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Variable bitwise NOT for fixed-width ints.
#[test]
fn check_rust_encodes_variable_bitnot() {
    let tmp = unique_temp("assura_check_rust_var_bitnot");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 255
fn n(x: u8) -> u8 { !x }

/// @ensures result >= -128
/// @ensures result <= 127
fn ns(x: i8) -> i8 { !x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong variable bitnot ensures must CE.
#[test]
fn check_rust_variable_bitnot_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_bitnot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn n(x: u8) -> u8 { !x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width variable bitwise not for path params.
#[test]
fn check_rust_encodes_mid_width_bitnot() {
    let tmp = unique_temp("assura_check_rust_mid_bitnot");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn n16(x: u16) -> u16 { !x }

/// @ensures result == 0 || result != 0
fn n32(x: u32) -> u32 { !x }

/// @ensures result == 0 || result != 0
fn s16(x: i16) -> i16 { !x }

/// @ensures result == 0 || result != 0
fn s32(x: i32) -> i32 { !x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width bitnot ensures must CE.
#[test]
fn check_rust_mid_width_bitnot_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_bitnot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn n(x: u16) -> u16 { !x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Variable u64 wrapping_shl/shr encode via synthetic 2^64 (#1160).
/// u64 both-variable wrapping_add/mul for path params.
#[test]
fn check_rust_encodes_u64_wrapping_add_mul_both_var() {
    let tmp = unique_temp("assura_check_rust_u64_wadd_wmul");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn w(x: u64, y: u64) -> u64 { x.wrapping_add(y) }

/// @ensures result == 0 || result != 0
fn m(x: u64, y: u64) -> u64 { x.wrapping_mul(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 both-var wrapping_add ensures must CE.
#[test]
fn check_rust_u64_wrapping_add_both_var_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_wadd_both_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn w(x: u64, y: u64) -> u64 { x.wrapping_add(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Nested u64 wrapping_add/mul chain encodes.
#[test]
fn check_rust_encodes_u64_nested_wrapping() {
    let tmp = unique_temp("assura_check_rust_u64_nested_wrap");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn n(x: u64, y: u64) -> u64 { x.wrapping_add(y).wrapping_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong nested wrapping chain ensures must CE.
#[test]
fn check_rust_u64_nested_wrapping_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_nested_wrap_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn n(x: u64, y: u64) -> u64 { x.wrapping_add(y).wrapping_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 both-variable wrapping_sub for path params.
#[test]
fn check_rust_encodes_u64_wrapping_sub_both_var() {
    let tmp = unique_temp("assura_check_rust_u64_wsub_both");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn w(x: u64, y: u64) -> u64 { x.wrapping_sub(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 both-var wrapping_sub ensures must CE.
#[test]
fn check_rust_u64_wrapping_sub_both_var_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_wsub_both_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn w(x: u64, y: u64) -> u64 { x.wrapping_sub(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Range uses `result >= 0` (u64::MAX does not fit as an i64 ensures lit).
#[test]
fn check_rust_encodes_variable_u64_wrapping_shl() {
    let tmp = unique_temp("assura_check_rust_var_u64_shl");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn s(x: u64, n: u32) -> u64 { x.wrapping_shl(n) }

/// @ensures result >= 0
fn r(x: u64, n: u32) -> u64 { x.wrapping_shr(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong variable u64 wrapping_shl ensures must CE (not identity).
#[test]
fn check_rust_variable_u64_wrapping_shl_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_u64_shl_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: u64, n: u32) -> u64 { x.wrapping_shl(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong variable u64 wrapping_shr ensures must CE (not identity).
#[test]
fn check_rust_variable_u64_wrapping_shr_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_u64_shr_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn r(x: u64, n: u32) -> u64 { x.wrapping_shr(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Variable i64 wrapping_shr range verifies via case-sum over k%64 (#1151).
/// (Full-range shl can cancel under default SMT timeout; soundness is
/// covered by the CE tests below and unit IR parse.)
#[test]
fn check_rust_encodes_variable_i64_wrapping_shr() {
    let tmp = unique_temp("assura_check_rust_var_i64_shr");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -9223372036854775808
/// @ensures result <= 9223372036854775807
fn r(x: i64, n: u32) -> i64 { x.wrapping_shr(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong variable i64 wrapping_shl ensures must CE (not identity).
#[test]
fn check_rust_variable_i64_wrapping_shl_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_i64_shl_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: i64, n: u32) -> i64 { x.wrapping_shl(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong variable i64 wrapping_shr ensures must CE (not identity).
#[test]
fn check_rust_variable_i64_wrapping_shr_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_i64_shr_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn r(x: i64, n: u32) -> i64 { x.wrapping_shr(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong signed wrapping_shl ensures must CE (proves wrap is live).
#[test]
fn check_rust_signed_wrapping_shl_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_signed_shl_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x * 2
fn s(x: i8) -> i8 { x.wrapping_shl(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE on i8 shl wrap: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed variable wrapping_shl/shr encode via case-sum (#1145).
#[test]
fn check_rust_encodes_i8_variable_wrapping_shl() {
    let tmp = unique_temp("assura_check_rust_i8_var_shl");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -128
/// @ensures result <= 127
fn s(x: i8, n: u32) -> i8 { x.wrapping_shl(n) }

/// @ensures result >= -128
/// @ensures result <= 127
fn r(x: i8, n: u32) -> i8 { x.wrapping_shr(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong signed variable wrapping_shl ensures must CE.
#[test]
fn check_rust_i8_variable_wrapping_shl_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i8_var_shl_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: i8, n: u32) -> i8 { x.wrapping_shl(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Variable wrapping_shl/shr for u8 encodes via case-sum (#1145).
#[test]
fn check_rust_encodes_u8_variable_wrapping_shl() {
    let tmp = unique_temp("assura_check_rust_var_shl");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 255
fn s(x: u8, n: u32) -> u8 { x.wrapping_shl(n) }

/// @ensures result >= 0
/// @ensures result <= 255
fn r(x: u8, n: u32) -> u8 { x.wrapping_shr(n) }

/// @ensures result >= 0
/// @ensures result <= 4294967295
fn s32(x: u32, n: u32) -> u32 { x.wrapping_shl(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong variable wrapping_shl ensures must CE.
#[test]
fn check_rust_u8_variable_wrapping_shl_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_shl_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: u8, n: u32) -> u8 { x.wrapping_shl(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed wrapping_shr encodes via floor div by 2^k.
#[test]
fn check_rust_encodes_signed_wrapping_shr() {
    let tmp = unique_temp("assura_check_rust_signed_shr");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -128
/// @ensures result <= 127
fn s(x: i8) -> i8 { x.wrapping_shr(1) }

/// @ensures result >= -9223372036854775808
/// @ensures result <= 9223372036854775807
fn l(x: i64) -> i64 { x.wrapping_shr(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong signed wrapping_shr ensures must CE (proves floor-div is live).
#[test]
fn check_rust_signed_wrapping_shr_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_signed_shr_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: i8) -> i8 { x.wrapping_shr(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width wrapping_shr for path params.
#[test]
fn check_rust_encodes_mid_width_wrapping_shr() {
    let tmp = unique_temp("assura_check_rust_mid_wshr");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn w16(x: u16) -> u16 { x.wrapping_shr(1) }

/// @ensures result == 0 || result != 0
fn w32(x: u32) -> u32 { x.wrapping_shr(1) }

/// @ensures result == 0 || result != 0
fn s16(x: i16) -> i16 { x.wrapping_shr(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u16 wrapping_shr ensures must CE.
#[test]
fn check_rust_u16_wrapping_shr_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u16_wshr_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn w(x: u16) -> u16 { x.wrapping_shr(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed rotate_left encodes via bit-pattern map + reinterpret.
#[test]
fn check_rust_encodes_signed_rotate() {
    let tmp = unique_temp("assura_check_rust_signed_rot");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -128
/// @ensures result <= 127
fn r(x: i8) -> i8 { x.rotate_left(1) }

/// @ensures result >= -128
/// @ensures result <= 127
fn rr(x: i8) -> i8 { x.rotate_right(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong signed rotate_left ensures must CE (bit rotate is live).
#[test]
fn check_rust_signed_rotate_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_signed_rot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x * 2
fn r(x: i8) -> i8 { x.rotate_left(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE on rotate: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width wrapping_shl for path params.
#[test]
fn check_rust_encodes_mid_width_wrapping_shl() {
    let tmp = unique_temp("assura_check_rust_mid_wshl");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn w16(x: u16) -> u16 { x.wrapping_shl(1) }

/// @ensures result == 0 || result != 0
fn w32(x: u32) -> u32 { x.wrapping_shl(1) }

/// @ensures result == 0 || result != 0
fn s16(x: i16) -> i16 { x.wrapping_shl(1) }

/// @ensures result == 0 || result != 0
fn s32(x: i32) -> i32 { x.wrapping_shl(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Mid-width wrapping_shl with variable shift for path params.
#[test]
fn check_rust_encodes_mid_width_wrapping_shl_var_k() {
    let tmp = unique_temp("assura_check_rust_mid_wshl_var_k");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn w16(x: u16, k: u32) -> u16 { x.wrapping_shl(k) }

/// @ensures result == 0 || result != 0
fn s16(x: i16, k: u32) -> i16 { x.wrapping_shl(k) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width var-k wrapping_shl ensures must CE.
#[test]
fn check_rust_mid_width_wrapping_shl_var_k_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_wshl_var_k_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn w(x: u16, k: u32) -> u16 { x.wrapping_shl(k) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width wrapping_shr with variable shift for path params.
#[test]
fn check_rust_encodes_mid_width_wrapping_shr_var_k() {
    let tmp = unique_temp("assura_check_rust_mid_wshr_var_k");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn w16(x: u16, k: u32) -> u16 { x.wrapping_shr(k) }

/// @ensures result == 0 || result != 0
fn s16(x: i16, k: u32) -> i16 { x.wrapping_shr(k) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width var-k wrapping_shr ensures must CE.
#[test]
fn check_rust_mid_width_wrapping_shr_var_k_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_wshr_var_k_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn w(x: u16, k: u32) -> u16 { x.wrapping_shr(k) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u16 wrapping_shl ensures must CE.
#[test]
fn check_rust_u16_wrapping_shl_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u16_wshl_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn w(x: u16) -> u16 { x.wrapping_shl(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Unsigned wrapping_shl by const encodes via mul+mod (#1010 partial).
#[test]
fn check_rust_encodes_u8_wrapping_shl() {
    let tmp = unique_temp("assura_check_rust_u8_shl");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 255
fn s(x: u8) -> u8 { x.wrapping_shl(1) }

/// @ensures result == x
fn id(x: u8) -> u8 { x.wrapping_shl(8) }

/// @ensures result >= 0
/// @ensures result <= 255
fn r(x: u8) -> u8 { x.wrapping_shr(1) }

/// @ensures result >= 0
/// @ensures result <= 255
fn rot(x: u8) -> u8 { x.rotate_left(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Const bit-count peeps (partial #1034 family) + shift/rotate-by-0 identity.
#[test]
fn check_rust_encodes_const_bit_peeps() {
    let tmp = unique_temp("assura_check_rust_bit_peeps");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 2
fn ones(x: u32) -> u32 { 12u32.count_ones() }

/// @ensures result == 3
fn to(x: u8) -> u32 { 7u8.trailing_ones() }

/// @ensures result == 4
fn lo(x: u32) -> u32 { 0xF000_0000u32.leading_ones() }

/// @ensures result == 30
fn zeros(x: u32) -> u32 { 12u32.count_zeros() }

/// @ensures result == 2
fn tz(x: u32) -> u32 { 12u32.trailing_zeros() }

/// @ensures result == 28
fn lz(x: u32) -> u32 { 8u32.leading_zeros() }

/// @ensures result == x
fn id_shl(x: i64) -> i64 { x.wrapping_shl(0) }

/// @ensures result == x
fn id_rot(x: i64) -> i64 { x.rotate_left(0) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Const bitops fold and verify.
#[test]
fn check_rust_encodes_const_bitops() {
    let tmp = unique_temp("assura_check_rust_bitops");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 8
fn a(x: u32) -> u32 { 12u32 & 10u32 }

/// @ensures result == 12
fn s(x: u32) -> u32 { 3u32 << 2 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong const bitops ensures must CE.
#[test]
fn check_rust_const_bitops_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_bitops_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn a(x: u32) -> u32 { 12u32 & 10u32 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Const reverse_bits / swap_bytes / zero trailing_zeros (typed).
#[test]
fn check_rust_encodes_const_reverse_swap() {
    let tmp = unique_temp("assura_check_rust_rev_swap");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 128
fn rev(x: u8) -> u8 { 1u8.reverse_bits() }

/// @ensures result == 13330
fn sw(x: u16) -> u16 { 0x1234u16.swap_bytes() }

/// @ensures result == 8
fn ztz(x: u8) -> u32 { 0u8.trailing_zeros() }

/// @ensures result == 3
fn ig(x: u32) -> u32 { 8u32.ilog2() }

/// @ensures result == 4
fn np(x: u32) -> u32 { 3u32.next_power_of_two() }

/// @ensures result == 0
fn wnp(x: u8) -> u8 { 200u8.wrapping_next_power_of_two() }

/// @ensures result == 3
fn sq(x: u32) -> u32 { 10u32.isqrt() }

/// @ensures result == 2
fn l10(x: u32) -> u32 { 100u32.ilog10() }

/// @ensures result >= 0
fn ua(x: i64) -> u64 { x.unsigned_abs() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong isqrt ensures must CE.
#[test]
fn check_rust_isqrt_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_isqrt_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn s(x: u32) -> u32 { 10u32.isqrt() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// div_ceil with positive const divisor for unsigned params.
#[test]
fn check_rust_encodes_div_ceil() {
    let tmp = unique_temp("assura_check_rust_div_ceil");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 255
fn d(x: u8) -> u8 { x.div_ceil(3) }

/// @ensures result == 4
fn c(x: u32) -> u32 { 10u32.div_ceil(3) }

/// @ensures result >= 0
/// @ensures result < 3
fn r(x: u8) -> u8 { x.rem_euclid(3) }

/// @ensures result >= 0
/// @ensures result <= 255
fn de(x: u8) -> u8 { x.div_euclid(3) }

/// @ensures result == 12
fn nmo(x: u8) -> u8 { 10u8.next_multiple_of(4) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Mid-width div_ceil for path params.
#[test]
fn check_rust_encodes_mid_width_div_ceil() {
    let tmp = unique_temp("assura_check_rust_mid_div_ceil");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn d16(x: u16) -> u16 { x.div_ceil(3) }

/// @ensures result >= 0
fn d32(x: u32) -> u32 { x.div_ceil(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width div_ceil ensures must CE.
#[test]
fn check_rust_mid_width_div_ceil_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_div_ceil_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn d(x: u16) -> u16 { x.div_ceil(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong div_ceil ensures must CE.
#[test]
fn check_rust_div_ceil_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_div_ceil_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn c(x: u32) -> u32 { 10u32.div_ceil(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed next_multiple_of with positive const encodes.
#[test]
fn check_rust_encodes_signed_next_multiple_of() {
    let tmp = unique_temp("assura_check_rust_signed_nmo");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= x
fn n(x: i64) -> i64 { x.next_multiple_of(4) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Mid-width next_multiple_of for unsigned path params.
#[test]
fn check_rust_encodes_mid_width_next_multiple_of() {
    let tmp = unique_temp("assura_check_rust_mid_nmo");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn n16(x: u16) -> u16 { x.next_multiple_of(4) }

/// @ensures result >= 0
fn n32(x: u32) -> u32 { x.next_multiple_of(4) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width next_multiple_of ensures must CE.
#[test]
fn check_rust_mid_width_next_multiple_of_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_nmo_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn n(x: u16) -> u16 { x.next_multiple_of(4) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// next_multiple_of with NonZeroU32 divisor.
#[test]
fn check_rust_encodes_next_multiple_of_nonzero() {
    let tmp = unique_temp("assura_check_rust_nmo_nz");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
use std::num::NonZeroU32;

/// @ensures result >= x
fn n(x: u32, m: NonZeroU32) -> u32 { x.next_multiple_of(m.get()) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong next_multiple_of with NonZero divisor must CE.
#[test]
fn check_rust_next_multiple_of_nonzero_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_nmo_nz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
use std::num::NonZeroU32;

/// @ensures result == x
fn n(x: u32, m: NonZeroU32) -> u32 { x.next_multiple_of(m.get()) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed rem_euclid/div_euclid with positive const encode.
#[test]
fn check_rust_encodes_signed_rem_euclid() {
    let tmp = unique_temp("assura_check_rust_signed_rem");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result < 3
fn r(x: i64) -> i64 { x.rem_euclid(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Mid-width div_euclid for path params.
#[test]
fn check_rust_encodes_mid_width_div_euclid() {
    let tmp = unique_temp("assura_check_rust_mid_div_euclid");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn d16(x: u16) -> u16 { x.div_euclid(3) }

/// @ensures result == 0 || result != 0
fn s16(x: i16) -> i16 { x.div_euclid(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width div_euclid ensures must CE.
#[test]
fn check_rust_mid_width_div_euclid_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_div_euclid_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn d(x: u16) -> u16 { x.div_euclid(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width rem_euclid for path params.
#[test]
fn check_rust_encodes_mid_width_rem_euclid() {
    let tmp = unique_temp("assura_check_rust_mid_rem");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn r16(x: u16) -> u16 { x.rem_euclid(3) }

/// @ensures result >= 0
fn s16(x: i16) -> i16 { x.rem_euclid(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// u64 rem_euclid/div_euclid for path params.
#[test]
fn check_rust_encodes_u64_rem_div_euclid() {
    let tmp = unique_temp("assura_check_rust_u64_rem_div");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn r(x: u64) -> u64 { x.rem_euclid(3) }

/// @ensures result == 0 || result != 0
fn d(x: u64) -> u64 { x.div_euclid(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 rem_euclid ensures must CE.
#[test]
fn check_rust_u64_rem_euclid_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_rem_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn r(x: u64) -> u64 { x.rem_euclid(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 path-param div_ceil / next_multiple_of with positive const divisor.
#[test]
fn check_rust_encodes_u64_div_ceil_next_multiple_of() {
    let tmp = unique_temp("assura_check_rust_u64_dc_nmo");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn d(x: u64) -> u64 { x.div_ceil(3) }

/// @ensures result >= 0
fn n(x: u64) -> u64 { x.next_multiple_of(4) }

/// @ensures result == 4
fn c(x: u64) -> u64 { 10u64.div_ceil(3) }

/// @ensures result == 12
fn nmoc(x: u64) -> u64 { 10u64.next_multiple_of(4) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 div_ceil ensures must CE.
#[test]
fn check_rust_u64_div_ceil_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_dc_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn d(x: u64) -> u64 { x.div_ceil(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 next_multiple_of ensures must CE.
#[test]
fn check_rust_u64_next_multiple_of_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_nmo_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn n(x: u64) -> u64 { x.next_multiple_of(4) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong mid-width rem_euclid ensures must CE.
#[test]
fn check_rust_mid_width_rem_euclid_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_rem_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn r(x: u16) -> u16 { x.rem_euclid(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Variable rem_euclid with NonZeroU32 divisor (lo >= 1) via `.get()`.
#[test]
fn check_rust_encodes_var_rem_euclid_nonzero() {
    let tmp = unique_temp("assura_check_rust_var_rem_nz");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
use std::num::NonZeroU32;

/// @ensures result >= 0
/// @ensures result <= x
fn r(x: u32, d: NonZeroU32) -> u32 { x.rem_euclid(d.get()) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// div_ceil with NonZeroU32 divisor (lo >= 1).
#[test]
fn check_rust_encodes_div_ceil_nonzero() {
    let tmp = unique_temp("assura_check_rust_div_ceil_nz");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
use std::num::NonZeroU32;

/// @ensures result >= 0
/// @ensures result <= x
fn d(x: u32, n: NonZeroU32) -> u32 { x.div_ceil(n.get()) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong div_ceil with NonZero divisor must CE (#1201 path live).
#[test]
fn check_rust_div_ceil_nonzero_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_div_ceil_nz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
use std::num::NonZeroU32;

/// @ensures result == 0
fn d(x: u32, n: NonZeroU32) -> u32 { x.div_ceil(n.get()) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 path with NonZeroU64 divisor for rem_euclid / div_ceil / next_multiple_of.
#[test]
fn check_rust_encodes_u64_nonzero_divisors() {
    let tmp = unique_temp("assura_check_rust_u64_nz_div");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
use std::num::NonZeroU64;

/// @ensures result >= 0
fn r(x: u64, n: NonZeroU64) -> u64 { x.rem_euclid(n.get()) }

/// @ensures result >= 0
fn d(x: u64, n: NonZeroU64) -> u64 { x.div_ceil(n.get()) }

/// @ensures result >= 0
fn n(x: u64, m: NonZeroU64) -> u64 { x.next_multiple_of(m.get()) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 NonZeroU64 div_ceil ensures must CE.
#[test]
fn check_rust_u64_div_ceil_nonzero_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_dc_nz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
use std::num::NonZeroU64;

/// @ensures result == 0
fn d(x: u64, n: NonZeroU64) -> u64 { x.div_ceil(n.get()) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 NonZeroU64 next_multiple_of ensures must CE.
#[test]
fn check_rust_u64_nmo_nonzero_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_nmo_nz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
use std::num::NonZeroU64;

/// @ensures result == x
fn n(x: u64, m: NonZeroU64) -> u64 { x.next_multiple_of(m.get()) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 NonZeroU64 rem_euclid ensures must CE.
#[test]
fn check_rust_u64_rem_euclid_nonzero_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_rem_nz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
use std::num::NonZeroU64;

/// @ensures result == 0
fn r(x: u64, n: NonZeroU64) -> u64 { x.rem_euclid(n.get()) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u64 midpoint / wrapping_neg / abs_diff path params encode.
#[test]
fn check_rust_encodes_u64_midpoint_wrapping_neg_abs_diff() {
    let tmp = unique_temp("assura_check_rust_u64_mid_neg_diff");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn m(x: u64, y: u64) -> u64 { x.midpoint(y) }

/// @ensures result >= 0
fn n(x: u64) -> u64 { x.wrapping_neg() }

/// @ensures result >= 0
fn d(x: u64, y: u64) -> u64 { x.abs_diff(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 wrapping_neg ensures must CE.
#[test]
fn check_rust_u64_wrapping_neg_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_wneg_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn n(x: u64) -> u64 { x.wrapping_neg() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u128 path-param add encodes (Nat map).
#[test]
fn check_rust_encodes_u128_add() {
    let tmp = unique_temp("assura_check_rust_u128_add");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn a(x: u128, y: u128) -> u128 { x + y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// u128 / with NonZeroU128 divisor encodes.
#[test]
fn check_rust_encodes_u128_div_nonzero() {
    let tmp = unique_temp("assura_check_rust_u128_div_nz");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
use std::num::NonZeroU128;

/// @ensures result >= 0
fn d(x: u128, n: NonZeroU128) -> u128 { x / n.get() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u128 NonZeroU128 div ensures must CE.
#[test]
fn check_rust_u128_div_nonzero_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u128_div_nz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
use std::num::NonZeroU128;

/// @ensures result == 0
fn d(x: u128, n: NonZeroU128) -> u128 { x / n.get() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// i128 abs encodes (Int map).
#[test]
fn check_rust_encodes_i128_abs() {
    let tmp = unique_temp("assura_check_rust_i128_abs");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn a(x: i128) -> i128 { x.abs() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i128 abs ensures must CE.
#[test]
fn check_rust_i128_abs_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i128_abs_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn a(x: i128) -> i128 { x.abs() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// i128 signum encodes.
#[test]
fn check_rust_encodes_i128_signum() {
    let tmp = unique_temp("assura_check_rust_i128_signum");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -1
/// @ensures result <= 1
fn s(x: i128) -> i128 { x.signum() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i128 signum ensures must CE.
#[test]
fn check_rust_i128_signum_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i128_signum_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn s(x: i128) -> i128 { x.signum() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 midpoint ensures must CE.
#[test]
fn check_rust_u64_midpoint_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_mid_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn m(x: u64, y: u64) -> u64 { x.midpoint(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 abs_diff ensures must CE.
#[test]
fn check_rust_u64_abs_diff_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_abs_diff_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn d(x: u64, y: u64) -> u64 { x.abs_diff(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Binary / and % with NonZeroU32 divisor (lo >= 1).
#[test]
fn check_rust_encodes_div_rem_nonzero() {
    let tmp = unique_temp("assura_check_rust_div_rem_nz");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
use std::num::NonZeroU32;

/// @ensures result <= x
fn d(x: u32, n: NonZeroU32) -> u32 { x / n.get() }

/// @ensures result <= x
fn r(x: u32, n: NonZeroU32) -> u32 { x % n.get() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// u64 / and % with NonZeroU64 divisor.
#[test]
fn check_rust_encodes_u64_div_rem_nonzero() {
    let tmp = unique_temp("assura_check_rust_u64_div_rem_nz");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
use std::num::NonZeroU64;

/// @ensures result >= 0
fn d(x: u64, n: NonZeroU64) -> u64 { x / n.get() }

/// @ensures result >= 0
fn r(x: u64, n: NonZeroU64) -> u64 { x % n.get() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 / NonZeroU64 ensures must CE.
#[test]
fn check_rust_u64_div_nonzero_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_div_nz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
use std::num::NonZeroU64;

/// @ensures result == 0
fn d(x: u64, n: NonZeroU64) -> u64 { x / n.get() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong / with NonZero divisor must CE (#1207 path live).
#[test]
fn check_rust_div_nonzero_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_div_nz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
use std::num::NonZeroU32;

/// @ensures result == 0
fn d(x: u32, n: NonZeroU32) -> u32 { x / n.get() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong % with NonZero divisor must CE (#1207 path live).
#[test]
fn check_rust_rem_nonzero_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_rem_nz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
use std::num::NonZeroU32;

/// @ensures result == 0
fn r(x: u32, n: NonZeroU32) -> u32 { x % n.get() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong rem_euclid ensures must CE.
#[test]
fn check_rust_rem_euclid_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_rem_euclid_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn r(x: u8) -> u8 { 10u8.rem_euclid(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong next_multiple_of ensures must CE.
#[test]
fn check_rust_next_multiple_of_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_nmo_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 10
fn nmo(x: u8) -> u8 { 10u8.next_multiple_of(4) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// midpoint encodes as floor((a+b)/2).
#[test]
fn check_rust_encodes_midpoint() {
    let tmp = unique_temp("assura_check_rust_midpoint");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 5
fn m(x: i64) -> i64 { 4i64.midpoint(6) }

/// @ensures result == x
fn s(x: i64) -> i64 { x.midpoint(x) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong midpoint ensures must CE.
#[test]
fn check_rust_midpoint_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn m(x: i64) -> i64 { 4i64.midpoint(6) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Top-level and nested wrapping_neg both encode.
#[test]
fn check_rust_encodes_wrapping_neg() {
    let tmp = unique_temp("assura_check_rust_wneg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn n(x: i64) -> i64 { x.wrapping_neg() }

/// @ensures result == 0 || result != 0
fn nest(x: i64) -> i64 { x.wrapping_neg() + x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Mid-width wrapping_neg for path params.
#[test]
fn check_rust_encodes_mid_width_wrapping_neg() {
    let tmp = unique_temp("assura_check_rust_mid_wneg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn n8(x: i8) -> i8 { x.wrapping_neg() }

/// @ensures result == 0 || result != 0
fn n16(x: i16) -> i16 { x.wrapping_neg() }

/// @ensures result == 0 || result != 0
fn n32(x: i32) -> i32 { x.wrapping_neg() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i16 wrapping_neg ensures must CE.
#[test]
fn check_rust_i16_wrapping_neg_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i16_wneg_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn n(x: i16) -> i16 { x.wrapping_neg() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// wrapping_add(0) / wrapping_mul(1) identity peeps encode.
#[test]
fn check_rust_encodes_wrapping_identity_peeps() {
    let tmp = unique_temp("assura_check_rust_wpeep");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x
fn a(x: i64) -> i64 { x.wrapping_add(0) }

/// @ensures result == x
fn m(x: i64) -> i64 { x.wrapping_mul(1) }

/// @ensures result == 0
fn z(x: i64) -> i64 { x.wrapping_mul(0) }

/// @ensures result == 0
fn s(x: i64) -> i64 { x.wrapping_sub(x) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong wrapping_neg ensures must CE (proves multi-block encode is live).
#[test]
fn check_rust_wrapping_neg_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_wneg_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn n(x: i64) -> i64 { x.wrapping_neg() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must fail: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode, not BNM: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Literal /0 and is_multiple_of(0) must be body_not_modeled (panic paths).
#[test]
fn check_rust_div_zero_body_not_modeled() {
    let tmp = unique_temp("assura_check_rust_div0_bnm");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0
fn d(x: i64) -> i64 { x / 0 }

/// @ensures result == true
fn m(x: i64) -> bool { x.is_multiple_of(0) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert!(
        v["body_not_modeled"].as_u64().unwrap_or(0) >= 2,
        "div0 / is_multiple_of(0) must BNM: {stdout}"
    );
    assert!(!out.status.success());
}

/// Wrong pot ensures with clone peel must CE (bounds peel live).
#[test]
fn check_rust_pot_clone_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_pot_clone_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == true
fn p(x: u8) -> bool { x.clone().is_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Nested non-path is_power_of_two stays body_not_modeled (no param bounds).
#[test]
fn check_rust_is_power_of_two_nested_i64_encodes() {
    // Nested pot inherits i64 pot-enum width (#1034 / #1169); was BNM before expr_int_bounds.
    let tmp = unique_temp("assura_check_rust_pot_nested_i64");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn p(x: i64) -> bool { (x + 1).is_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(
        v["body_not_modeled"], 0,
        "nested i64 pot must encode: {stdout}"
    );
}

/// u8/u32/i64 is_power_of_two encodes via pot enum (partial #1034).
/// Identity peels keep path-param bounds (clone/into).
#[test]
fn check_rust_encodes_u8_is_power_of_two() {
    let tmp = unique_temp("assura_check_rust_pot_u8");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn p(x: u8) -> bool { x.is_power_of_two() }

/// @ensures result == true || result == false
fn q(x: u32) -> bool { x.is_power_of_two() }

/// @ensures result == true || result == false
fn r(x: i64) -> bool { x.is_power_of_two() }

/// @ensures result == true || result == false
fn c(x: i64) -> bool { x.clone().is_power_of_two() }

/// @ensures result == true || result == false
fn i(x: i64) -> bool { x.into().is_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong pot ensures must CE (proves enum encode is live).
#[test]
fn check_rust_u8_is_power_of_two_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_pot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == true
fn p(x: u8) -> bool { x.is_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE when x=3: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width is_power_of_two for path params.
#[test]
fn check_rust_encodes_mid_width_is_power_of_two() {
    let tmp = unique_temp("assura_check_rust_mid_pot");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn p16(x: u16) -> bool { x.is_power_of_two() }

/// @ensures result == true || result == false
fn p32(x: u32) -> bool { x.is_power_of_two() }

/// @ensures result == true || result == false
fn s16(x: i16) -> bool { x.is_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u16 is_power_of_two ensures must CE.
#[test]
fn check_rust_u16_is_power_of_two_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u16_pot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == true
fn p(x: u16) -> bool { x.is_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Const is_power_of_two peeps (partial #1034 / #1089).
#[test]
fn check_rust_encodes_const_is_power_of_two() {
    let tmp = unique_temp("assura_check_rust_pot_const");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true
fn t(x: i64) -> bool { 8i64.is_power_of_two() }

/// @ensures result == false
fn f(x: i64) -> bool { 3i64.is_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Unsigned path-param count_ones/count_zeros encode via bit-sum (div/mod).
#[test]
fn check_rust_encodes_u8_count_ones() {
    let tmp = unique_temp("assura_check_rust_count_ones");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 8
fn c(x: u8) -> u32 { x.count_ones() }

/// @ensures result >= 0
/// @ensures result <= 8
fn z(x: u8) -> u32 { x.count_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u16 count_ones/count_zeros for path params.
#[test]
fn check_rust_encodes_u16_count_ones() {
    let tmp = unique_temp("assura_check_rust_u16_count_ones");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 16
fn c(x: u16) -> u32 { x.count_ones() }

/// @ensures result >= 0
/// @ensures result <= 16
fn z(x: u16) -> u32 { x.count_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u32 count_ones/count_zeros for path params.
#[test]
fn check_rust_encodes_u32_count_ones() {
    let tmp = unique_temp("assura_check_rust_u32_count_ones");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 32
fn c(x: u32) -> u32 { x.count_ones() }

/// @ensures result >= 0
/// @ensures result <= 32
fn z(x: u32) -> u32 { x.count_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u64 count_ones/count_zeros for path params.
#[test]
fn check_rust_encodes_u64_count_ones() {
    let tmp = unique_temp("assura_check_rust_u64_count_ones");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 64
fn c(x: u64) -> u32 { x.count_ones() }

/// @ensures result >= 0
/// @ensures result <= 64
fn z(x: u64) -> u32 { x.count_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u64 trailing/leading zeros, reverse_bits, and bitnot.
#[test]
fn check_rust_encodes_u64_bit_counts_and_reverse() {
    let tmp = unique_temp("assura_check_rust_u64_bit_counts");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn t(x: u64) -> u32 { x.trailing_zeros() }

/// @ensures result >= 0
fn l(x: u64) -> u32 { x.leading_zeros() }

/// @ensures result == 0 || result != 0
fn r(x: u64) -> u64 { x.reverse_bits() }

/// @ensures result == 0 || result != 0
fn n(x: u64) -> u64 { !x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u64 trailing/leading ones and swap_bytes.
#[test]
fn check_rust_encodes_u64_ones_and_swap() {
    let tmp = unique_temp("assura_check_rust_u64_ones_swap");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn t(x: u64) -> u32 { x.trailing_ones() }

/// @ensures result >= 0
fn l(x: u64) -> u32 { x.leading_ones() }

/// @ensures result == 0 || result != 0
fn s(x: u64) -> u64 { x.swap_bytes() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u64 swap_bytes ensures must CE.
#[test]
fn check_rust_u64_swap_bytes_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_swap_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: u64) -> u64 { x.swap_bytes() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 trailing_zeros ensures must CE.
#[test]
fn check_rust_u64_trailing_zeros_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_tz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn t(x: u64) -> u32 { x.trailing_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 leading_zeros ensures must CE.
#[test]
fn check_rust_u64_leading_zeros_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_lz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn l(x: u64) -> u32 { x.leading_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 bitnot ensures must CE.
#[test]
fn check_rust_u64_bitnot_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_bitnot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn n(x: u64) -> u64 { !x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 trailing_ones ensures must CE.
#[test]
fn check_rust_u64_trailing_ones_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_to_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn t(x: u64) -> u32 { x.trailing_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 leading_ones ensures must CE.
#[test]
fn check_rust_u64_leading_ones_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_lo_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn l(x: u64) -> u32 { x.leading_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 reverse_bits ensures must CE.
#[test]
fn check_rust_u64_reverse_bits_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_rev_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn r(x: u64) -> u64 { x.reverse_bits() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 count_ones ensures must CE.
#[test]
fn check_rust_u64_count_ones_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_count_ones_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn c(x: u64) -> u32 { x.count_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u64 count_zeros ensures must CE.
#[test]
fn check_rust_u64_count_zeros_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u64_cz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn z(x: u64) -> u32 { x.count_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u16 count_ones ensures must CE.
#[test]
fn check_rust_u16_count_ones_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u16_count_ones_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn c(x: u16) -> u32 { x.count_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u32 count_ones ensures must CE.
#[test]
fn check_rust_u32_count_ones_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u32_count_ones_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn c(x: u32) -> u32 { x.count_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Unsigned path-param trailing_zeros/leading_zeros encode via bit products.
#[test]
fn check_rust_encodes_u8_trailing_zeros() {
    let tmp = unique_temp("assura_check_rust_tz");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 8
fn t(x: u8) -> u32 { x.trailing_zeros() }

/// @ensures result >= 0
/// @ensures result <= 8
fn l(x: u8) -> u32 { x.leading_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u16 trailing_zeros/leading_zeros (weak bounds; tight upper can cancel).
#[test]
fn check_rust_encodes_u16_trailing_leading_zeros() {
    let tmp = unique_temp("assura_check_rust_u16_tz_lz");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn t(x: u16) -> u32 { x.trailing_zeros() }

/// @ensures result >= 0
fn l(x: u16) -> u32 { x.leading_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u32 trailing_zeros/leading_zeros with non-timeout-safe bounds.
#[test]
fn check_rust_encodes_u32_trailing_leading_zeros() {
    let tmp = unique_temp("assura_check_rust_u32_tz_lz");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn t(x: u32) -> u32 { x.trailing_zeros() }

/// @ensures result >= 0
fn l(x: u32) -> u32 { x.leading_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u16 trailing_zeros ensures must CE.
#[test]
fn check_rust_u16_trailing_zeros_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u16_tz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn t(x: u16) -> u32 { x.trailing_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u32 leading_zeros ensures must CE.
#[test]
fn check_rust_u32_leading_zeros_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u32_lz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn l(x: u32) -> u32 { x.leading_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong trailing_zeros ensures must CE (proves first-set-bit encode is live).
#[test]
fn check_rust_u8_trailing_zeros_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_tz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn t(x: u8) -> u32 { x.trailing_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Unsigned path-param reverse_bits encodes via bit reverse sum.
#[test]
fn check_rust_encodes_u8_reverse_bits() {
    let tmp = unique_temp("assura_check_rust_rev");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 255
fn r(x: u8) -> u8 { x.reverse_bits() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u16/u32 reverse_bits for path params.
#[test]
fn check_rust_encodes_u16_u32_reverse_bits() {
    let tmp = unique_temp("assura_check_rust_rev_u16_u32");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn r16(x: u16) -> u16 { x.reverse_bits() }

/// @ensures result == 0 || result != 0
fn r32(x: u32) -> u32 { x.reverse_bits() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Signed i16/i32 reverse_bits via bit-pattern map.
#[test]
fn check_rust_encodes_i16_i32_reverse_bits() {
    let tmp = unique_temp("assura_check_rust_rev_i16_i32");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn r16(x: i16) -> i16 { x.reverse_bits() }

/// @ensures result == 0 || result != 0
fn r32(x: i32) -> i32 { x.reverse_bits() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Variable u32 swap_bytes for path params.
#[test]
fn check_rust_encodes_u32_swap_bytes() {
    let tmp = unique_temp("assura_check_rust_sw_u32");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn s(x: u32) -> u32 { x.swap_bytes() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u16 reverse_bits ensures must CE.
#[test]
fn check_rust_u16_reverse_bits_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_rev_u16_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn r(x: u16) -> u16 { x.reverse_bits() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong u32 swap_bytes ensures must CE.
#[test]
fn check_rust_u32_swap_bytes_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_sw_u32_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn s(x: u32) -> u32 { x.swap_bytes() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Unsigned path-param swap_bytes encodes via byte reverse.
#[test]
fn check_rust_encodes_u16_swap_bytes() {
    let tmp = unique_temp("assura_check_rust_sw");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 65535
fn s(x: u16) -> u16 { x.swap_bytes() }

/// @ensures result >= 0
/// @ensures result <= 255
fn u(x: u8) -> u8 { x.swap_bytes() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong swap_bytes ensures must CE (proves byte reverse is live).
#[test]
fn check_rust_u16_swap_bytes_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_sw_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: u16) -> u16 { x.swap_bytes() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong signed swap_bytes ensures must CE (bit-pattern map is live).
#[test]
fn check_rust_i16_swap_bytes_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i16_swap_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn s(x: i16) -> i16 { x.swap_bytes() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong i32 swap_bytes ensures must CE.
#[test]
fn check_rust_i32_swap_bytes_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i32_swap_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn s(x: i32) -> i32 { x.swap_bytes() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong rem_euclid with NonZero divisor must CE (variable positive path live).
#[test]
fn check_rust_rem_euclid_nonzero_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_rem_nz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
use std::num::NonZeroU32;

/// @ensures result == 0
fn r(x: u32, d: NonZeroU32) -> u32 { x.rem_euclid(d.get()) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong reverse_bits ensures must CE.
#[test]
fn check_rust_u8_reverse_bits_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_rev_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn r(x: u8) -> u8 { x.reverse_bits() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong leading_zeros ensures must CE.
#[test]
fn check_rust_u8_leading_zeros_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_lz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn l(x: u8) -> u32 { x.leading_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong count_zeros ensures must CE.
#[test]
fn check_rust_u8_count_zeros_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_cz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn z(x: u8) -> u32 { x.count_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong signed count_zeros ensures must CE (bit-pattern path is live).
#[test]
fn check_rust_i8_count_zeros_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i8_cz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn z(x: i8) -> u32 { x.count_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong trailing_ones ensures must CE (NOT+zeros encode is live).
#[test]
fn check_rust_u8_trailing_ones_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_to_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn t(x: u8) -> u32 { x.trailing_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong leading_ones ensures must CE.
#[test]
fn check_rust_u8_leading_ones_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_lo_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn l(x: u8) -> u32 { x.leading_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong signed reverse_bits ensures must CE.
#[test]
fn check_rust_i8_reverse_bits_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i8_rev_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn r(x: i8) -> i8 { x.reverse_bits() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong count_ones ensures must CE (proves bit-sum is live).
#[test]
fn check_rust_u8_count_ones_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_count_ones_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn c(x: u8) -> u32 { x.count_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong nested signum ensures must CE (proves encode is live, #1032).
#[test]
fn check_rust_nested_signum_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_ns_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn s(x: i64) -> i64 { x.signum() + 1 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must fail: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode, not BNM: {stdout}");
    assert!(
        v["errors"].as_u64().unwrap_or(0) >= 1,
        "expected counterexample/errors: {v}"
    );
    let status = v["results"][0]["status"].as_str().unwrap_or("");
    assert_eq!(status, "error", "expected error status from CE: {v}");
}

/// into() identity and bool true path encode.
#[test]
fn check_rust_encodes_into_true() {
    let tmp = unique_temp("assura_check_rust_into_true");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x
fn i(x: i64) -> i64 { x.into() }

/// @ensures result == true
fn t(a: bool) -> bool { true }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i64::MAX body must CE.
#[test]
fn check_rust_max_const_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_max_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 9223372036854775807
fn m(x: i64) -> i64 { i64::MIN }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// saturating_neg encodes (body model present).
#[test]
fn check_rust_encodes_saturating_neg() {
    let tmp = unique_temp("assura_check_rust_sat_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn n(x: i64) -> i64 { x.saturating_neg() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Mid-width saturating_neg for path params.
#[test]
fn check_rust_encodes_mid_width_saturating_neg() {
    let tmp = unique_temp("assura_check_rust_mid_sat_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn s8(x: i8) -> i8 { x.saturating_neg() }

/// @ensures result == 0 || result != 0
fn s16(x: i16) -> i16 { x.saturating_neg() }

/// @ensures result == 0 || result != 0
fn s32(x: i32) -> i32 { x.saturating_neg() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i16 saturating_neg ensures must CE.
#[test]
fn check_rust_i16_saturating_neg_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i16_sat_neg_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: i16) -> i16 { x.saturating_neg() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width saturating_abs for path params.
#[test]
fn check_rust_encodes_mid_width_saturating_abs() {
    let tmp = unique_temp("assura_check_rust_mid_sat_abs");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn s8(x: i8) -> i8 { x.saturating_abs() }

/// @ensures result >= 0
fn s16(x: i16) -> i16 { x.saturating_abs() }

/// @ensures result >= 0
fn s32(x: i32) -> i32 { x.saturating_abs() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i16 saturating_abs ensures must CE.
#[test]
fn check_rust_i16_saturating_abs_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i16_sat_abs_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: i16) -> i16 { x.saturating_abs() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// saturating_abs encodes (MIN → MAX via abs then min with MAX).
#[test]
fn check_rust_encodes_saturating_abs() {
    let tmp = unique_temp("assura_check_rust_sat_abs");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn a(x: i64) -> i64 { x.saturating_abs() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong saturating_abs ensures must CE (proves encode is live).
#[test]
fn check_rust_saturating_abs_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_sat_abs_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result < 0
fn a(x: i64) -> i64 { x.saturating_abs() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must fail: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode, not BNM: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Mid-width abs_diff for path params.
#[test]
fn check_rust_encodes_mid_width_abs_diff() {
    let tmp = unique_temp("assura_check_rust_mid_abs_diff");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn a16(x: u16, y: u16) -> u16 { x.abs_diff(y) }

/// @ensures result >= 0
fn s16(x: i16, y: i16) -> u16 { x.abs_diff(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong mid-width abs_diff ensures must CE.
#[test]
fn check_rust_mid_width_abs_diff_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_abs_diff_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn a(x: u16, y: u16) -> u16 { x.abs_diff(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// abs_diff().is_positive() encodes and verifies for unequal params.
#[test]
fn check_rust_encodes_abs_diff_positive() {
    let tmp = unique_temp("assura_check_rust_ad_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == (x != y)
fn d(x: i64, y: i64) -> bool { x.abs_diff(y).is_positive() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    // abs_diff never overflows; is_positive iff x != y for all i64
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Nested if/else-if encodes multi-block IR and can CE wrong branches.
#[test]
fn check_rust_encodes_nested_if_body() {
    let tmp = unique_temp("assura_check_rust_body_nested_if");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn nest(x: i64) -> i64 {
    if x > 10 { x } else { if x > 0 { x } else { 0 } }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "nested if should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 1, "{stdout}");

    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result >= 0
fn nest(x: i64) -> i64 {
    if x > 10 { x } else { if x > 0 { x } else { -1 } }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "wrong nested else should CE");
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Bool comparison bodies encode; wrong wrapping ensures CEs (encode live).
#[test]
fn check_rust_bool_body_and_bnm_unmodeled() {
    let tmp = unique_temp("assura_check_rust_bool_bnm");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bool.rs"),
        r#"
/// @ensures result == true || result == false
fn is_pos(x: i64) -> bool { x > 0 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args([
            "check-rust",
            "--json",
            tmp.join("bool.rs").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Bool cmp encode is enough to avoid BNM; ensures may verify or be soft.
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(
        v["body_not_modeled"], 0,
        "bool comparison should encode body: {stdout}"
    );

    // wrapping_add encodes; wrong ensures (result >= x fails at MAX wrap) must CE.
    std::fs::write(
        tmp.join("wrap.rs"),
        r#"
/// @ensures result >= x
fn add1(x: i64) -> i64 { x.wrapping_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args([
            "check-rust",
            "--json",
            tmp.join("wrap.rs").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "wrong wrapping ensures must fail exit: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(
        v["body_not_modeled"], 0,
        "wrapping_add must encode (not BNM): {v}"
    );
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// #975: wrong identity body vs ensures x+1 must CE (not silent verified / BNM).
#[test]
fn check_rust_encodes_identity_body_counterexample() {
    let tmp = unique_temp("assura_check_rust_body_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @requires x > 0
/// @ensures result == x + 1
fn bad(x: i64) -> i64 { x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "wrong body should fail: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("check-rust --json");
    assert_eq!(v["body_not_modeled"], 0, "body was encoded, not BNM: {v}");
    assert!(
        v["errors"].as_u64().unwrap_or(0) >= 1,
        "expected counterexample/errors: {v}"
    );
    let status = v["results"][0]["status"].as_str().unwrap_or("");
    assert_eq!(status, "error", "expected error status from CE: {v}");
}
