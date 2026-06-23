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
// Issue #96: explain command integration tests
// =======================================================================

#[test]
fn explain_valid_code_exits_zero() {
    let out = Command::new(assura_bin())
        .args(["explain", "A01001"])
        .output()
        .expect("failed to run assura explain");
    assert!(
        out.status.success(),
        "explain A01001 should exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn explain_valid_code_shows_info() {
    let out = Command::new(assura_bin())
        .args(["explain", "A01001"])
        .output()
        .expect("failed to run assura explain");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("A01001"),
        "explain output should contain the error code: {stdout}"
    );
    assert!(
        stdout.contains("Example"),
        "explain output should contain an example: {stdout}"
    );
    assert!(
        stdout.contains("How to fix"),
        "explain output should contain fix guidance: {stdout}"
    );
}

#[test]
fn explain_invalid_code_exits_nonzero() {
    let out = Command::new(assura_bin())
        .args(["explain", "XXXXX"])
        .output()
        .expect("failed to run assura explain XXXXX");
    assert!(!out.status.success(), "explain XXXXX should exit non-zero");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Unknown error code"),
        "should say unknown code: {stderr}"
    );
}

#[test]
fn explain_lists_known_codes_on_failure() {
    let out = Command::new(assura_bin())
        .args(["explain", "XXXXX"])
        .output()
        .expect("failed to run assura explain XXXXX");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Known error codes"),
        "should list known codes on failure: {stderr}"
    );
    assert!(
        stderr.contains("A01"),
        "known codes should include A01 range: {stderr}"
    );
}
