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
// Issue #96: coverage command integration tests
// =======================================================================

/// Create a Rust crate with public functions and matching .assura contracts.
fn create_coverage_test_crate(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("contracts")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"cov-test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub fn add(a: i64, b: i64) -> i64 { a + b }\npub fn sub(a: i64, b: i64) -> i64 { a - b }\npub fn mul(a: i64, b: i64) -> i64 { a * b }\n",
    )
    .unwrap();
    // Contract covering only `add`
    std::fs::write(
        dir.join("contracts/math.assura"),
        "contract add {\n    input(a: Int, b: Int)\n    output(result: Int)\n    ensures { result == a + b }\n}\n",
    )
    .unwrap();
}

#[test]
fn coverage_human_output() {
    let tmp = unique_temp("assura_cov_human");
    let _ = std::fs::remove_dir_all(&tmp);
    create_coverage_test_crate(&tmp);

    let out = Command::new(assura_bin())
        .args(["coverage", tmp.to_str().unwrap()])
        .output()
        .expect("failed to run assura coverage");
    assert!(
        out.status.success(),
        "coverage should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Contract Coverage"),
        "should show coverage header: {stdout}"
    );
    assert!(
        stdout.contains("With contracts"),
        "should show covered count: {stdout}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn coverage_json_output_structure() {
    let tmp = unique_temp("assura_cov_json");
    let _ = std::fs::remove_dir_all(&tmp);
    create_coverage_test_crate(&tmp);

    let out = Command::new(assura_bin())
        .args(["coverage", tmp.to_str().unwrap(), "--format", "json"])
        .output()
        .expect("failed to run assura coverage --format json");
    assert!(
        out.status.success(),
        "coverage json should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{stdout}"));
    assert!(
        parsed.get("total_functions").is_some(),
        "JSON should have total_functions"
    );
    assert!(parsed.get("covered").is_some(), "JSON should have covered");
    assert!(
        parsed.get("coverage_percent").is_some(),
        "JSON should have coverage_percent"
    );
    assert!(
        parsed.get("covered_functions").is_some(),
        "JSON should have covered_functions"
    );
    assert!(
        parsed.get("uncovered_functions").is_some(),
        "JSON should have uncovered_functions"
    );
    // Verify counts: 1 covered (add), 2 uncovered (sub, mul)
    assert_eq!(parsed["covered"].as_u64().unwrap(), 1);
    assert_eq!(parsed["total_functions"].as_u64().unwrap(), 3);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn coverage_min_coverage_fails_when_below() {
    let tmp = unique_temp("assura_cov_min_fail");
    let _ = std::fs::remove_dir_all(&tmp);
    create_coverage_test_crate(&tmp);

    // 1 out of 3 = 33.3%, requiring 90% should fail
    let out = Command::new(assura_bin())
        .args(["coverage", tmp.to_str().unwrap(), "--min-coverage", "90"])
        .output()
        .expect("failed to run assura coverage --min-coverage");
    assert!(
        !out.status.success(),
        "coverage should fail when below min threshold"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn coverage_min_coverage_passes_when_above() {
    let tmp = unique_temp("assura_cov_min_pass");
    let _ = std::fs::remove_dir_all(&tmp);
    create_coverage_test_crate(&tmp);

    // 1 out of 3 = 33.3%, requiring 10% should pass
    let out = Command::new(assura_bin())
        .args(["coverage", tmp.to_str().unwrap(), "--min-coverage", "10"])
        .output()
        .expect("failed to run assura coverage --min-coverage");
    assert!(
        out.status.success(),
        "coverage should pass when above min threshold: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn coverage_no_src_dir_fails() {
    let tmp = unique_temp("assura_cov_no_src");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let out = Command::new(assura_bin())
        .args(["coverage", tmp.to_str().unwrap()])
        .output()
        .expect("failed to run assura coverage");
    assert!(!out.status.success(), "coverage should fail without src/");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("src/"),
        "should mention missing src/: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
