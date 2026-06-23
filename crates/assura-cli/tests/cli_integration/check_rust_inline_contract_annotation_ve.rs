//! Integration tests for the `assura` CLI binary.
//!
//! These tests invoke the compiled `assura` binary via `std::process::Command`.
//! Using `env!("CARGO_BIN_EXE_assura")` guarantees Cargo builds the binary
//! before running these tests, so they work in clean environments (issue #47).

use std::path::PathBuf;
use std::process::Command;

/// Path to the `assura` binary, guaranteed to exist by Cargo.
fn assura_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_assura"))
}

/// Workspace root (two levels up from crate manifest).
fn workspace_root() -> String {
    env!("CARGO_MANIFEST_DIR").replace("/crates/assura-cli", "")
}

/// Unique temp dir (using tempfile for strong uniqueness across parallel tests).
/// The TempDir guard is leaked so the directory persists for the duration of the
/// test (and is cleaned up by explicit remove_dir_all at end of each test or OS).
fn unique_temp(prefix: &str) -> std::path::PathBuf {
    let d = tempfile::Builder::new()
        .prefix(&format!("{}_", prefix))
        .tempdir()
        .expect("failed to create unique temp dir");
    let p = d.path().to_path_buf();
    std::mem::forget(d);
    p
}

// =======================================================================
// check-rust: inline contract annotation verification
// =======================================================================

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
