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
// infer: heuristic-based contract inference for Rust files
// =======================================================================

#[test]
fn infer_rust_detects_division() {
    let tmp = unique_temp("assura_infer_div");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("div.rs"),
        "fn divide(a: i64, b: i64) -> i64 { a / b }\n",
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["infer", "--dry-run", tmp.join("div.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("@requires b != 0"),
        "should suggest division guard, got: {stdout}"
    );
}

#[test]
fn infer_rust_focus_filter() {
    let tmp = unique_temp("assura_infer_focus");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("mixed.rs"),
        r#"
fn divide(a: i64, b: i64) -> i64 { a / b }
fn get(items: &[i32], idx: usize) -> i32 { items[idx] }
"#,
    )
    .unwrap();

    // Focus on division only
    let out = Command::new(assura_bin())
        .args([
            "infer",
            "--dry-run",
            "--focus",
            "division",
            tmp.join("mixed.rs").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("[division]"),
        "should contain division pattern, got: {stdout}"
    );
    assert!(
        !stdout.contains("[index]"),
        "should NOT contain index pattern when focused on division, got: {stdout}"
    );
}

#[test]
fn infer_rust_detects_unwrap() {
    let tmp = unique_temp("assura_infer_unwrap");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("unwrap.rs"),
        "fn get_val(r: Result<i32, String>) -> i32 { r.unwrap() }\n",
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args([
            "infer",
            "--dry-run",
            tmp.join("unwrap.rs").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("[unwrap]") || stdout.contains("is_some"),
        "should detect unwrap pattern, got: {stdout}"
    );
}
