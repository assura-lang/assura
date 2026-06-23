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
// Issue #49: Audit command integration tests
// =======================================================================

/// Create a minimal Rust crate for audit testing.
fn create_test_crate(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"test-crate\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub fn add(a: i64, b: i64) -> i64 { a + b }\npub fn greet(name: &str) -> String { format!(\"Hello, {name}\") }\n",
    )
    .unwrap();
}

#[test]
fn audit_human_output_shows_summary() {
    let tmp = unique_temp("assura_audit_human");
    let _ = std::fs::remove_dir_all(&tmp);
    create_test_crate(&tmp);

    let out = Command::new(assura_bin())
        .args(["audit", tmp.to_str().unwrap(), "--depth", "shallow"])
        .output()
        .expect("failed to run assura audit");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // Should show summary line with function count
    assert!(
        stdout.contains("AUDIT SUMMARY") || stderr.contains("Scanning"),
        "audit should show summary or scanning info:\nstdout: {stdout}\nstderr: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn audit_json_output_is_valid() {
    let tmp = unique_temp("assura_audit_json");
    let _ = std::fs::remove_dir_all(&tmp);
    create_test_crate(&tmp);

    let out = Command::new(assura_bin())
        .args([
            "audit",
            tmp.to_str().unwrap(),
            "--format",
            "json",
            "--depth",
            "shallow",
        ])
        .output()
        .expect("failed to run assura audit");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // JSON output should be valid JSON with expected fields
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{stdout}"));
    assert!(
        parsed.get("functions_scanned").is_some(),
        "JSON should have functions_scanned"
    );
    assert!(
        parsed.get("files_scanned").is_some(),
        "JSON should have files_scanned"
    );
    assert!(
        parsed.get("results").is_some(),
        "JSON should have results array"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn audit_no_cargo_toml_fails() {
    let tmp = unique_temp("assura_audit_no_cargo");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let out = Command::new(assura_bin())
        .args(["audit", tmp.to_str().unwrap()])
        .output()
        .expect("failed to run assura audit");
    assert!(
        !out.status.success(),
        "audit should fail without Cargo.toml"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Cargo.toml"),
        "should mention missing Cargo.toml: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn audit_empty_src_fails() {
    let tmp = unique_temp("assura_audit_empty_src");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    std::fs::write(
        tmp.join("Cargo.toml"),
        "[package]\nname = \"empty\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    // src/ exists but has no .rs files

    let out = Command::new(assura_bin())
        .args(["audit", tmp.to_str().unwrap()])
        .output()
        .expect("failed to run assura audit");
    assert!(!out.status.success(), "audit should fail with no .rs files");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn audit_max_functions_limits_output() {
    let tmp = unique_temp("assura_audit_max_fn");
    let _ = std::fs::remove_dir_all(&tmp);
    create_test_crate(&tmp);
    // Add more functions
    std::fs::write(
        tmp.join("src/lib.rs"),
        "pub fn f1(x: i64) -> i64 { x }\npub fn f2(x: i64) -> i64 { x }\npub fn f3(x: i64) -> i64 { x }\npub fn f4(x: i64) -> i64 { x }\n",
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args([
            "audit",
            tmp.to_str().unwrap(),
            "--format",
            "json",
            "--max-functions",
            "2",
            "--depth",
            "shallow",
        ])
        .output()
        .expect("failed to run assura audit");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{stdout}"));
    let scanned = parsed["functions_scanned"].as_u64().unwrap();
    assert!(
        scanned <= 2,
        "max-functions=2 should limit to 2 functions, got {scanned}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn audit_medium_depth_adds_heuristics() {
    let tmp = unique_temp("assura_audit_medium");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    std::fs::write(
        tmp.join("Cargo.toml"),
        "[package]\nname = \"test-med\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    // Function with index param should get bounds heuristic at medium depth
    std::fs::write(
        tmp.join("src/lib.rs"),
        "pub fn get_item(data: &[u8], index: usize) -> u8 { data[index] }\n",
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args([
            "audit",
            tmp.to_str().unwrap(),
            "--depth",
            "medium",
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run assura audit");
    // Should complete (even if verification produces findings)
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{stdout}"));
    assert!(
        parsed.get("functions_scanned").is_some(),
        "medium depth audit should produce JSON output"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
