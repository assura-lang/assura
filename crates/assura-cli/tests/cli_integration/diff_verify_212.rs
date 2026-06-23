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
// diff --verify (#212)
// =======================================================================

#[test]
fn diff_verify_identical_exits_zero() {
    let root = workspace_root();
    let demo = format!("{root}/demos/libwebp-huffman.assura");
    let out = Command::new(assura_bin())
        .args(["diff", "--verify", &demo, &demo])
        .output()
        .expect("failed to run assura diff --verify");
    assert!(
        out.status.success(),
        "diff --verify of same file should exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn diff_verify_compatible_evolution_exits_zero() {
    let root = workspace_root();
    let old_file = format!("{root}/tests/fixtures/diff_verify_old.assura");
    let new_file = format!("{root}/tests/fixtures/diff_verify_new_compat.assura");
    let out = Command::new(assura_bin())
        .args(["diff", "--verify", &old_file, &new_file])
        .output()
        .expect("failed to run assura diff --verify (compatible)");
    assert!(
        out.status.success(),
        "compatible evolution (weakened precondition) should exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn diff_verify_incompatible_evolution_exits_nonzero() {
    let root = workspace_root();
    let old_file = format!("{root}/tests/fixtures/diff_verify_old.assura");
    let new_file = format!("{root}/tests/fixtures/diff_verify_new_incompat.assura");
    let out = Command::new(assura_bin())
        .args(["diff", "--verify", &old_file, &new_file])
        .output()
        .expect("failed to run assura diff --verify (incompatible)");
    assert!(
        !out.status.success(),
        "incompatible evolution (strengthened precondition) should exit non-zero"
    );
}

#[test]
fn diff_verify_json_output_has_evolution() {
    let root = workspace_root();
    let old_file = format!("{root}/tests/fixtures/diff_verify_old.assura");
    let new_file = format!("{root}/tests/fixtures/diff_verify_new_compat.assura");
    let out = Command::new(assura_bin())
        .args(["diff", "--verify", "--format", "json", &old_file, &new_file])
        .output()
        .expect("failed to run assura diff --verify --format json");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // The output includes both the structural diff JSON and the evolution
    // verification JSON (two separate JSON documents on stdout).
    assert!(
        stdout.contains("evolution") && stdout.contains("compatible"),
        "JSON output should include evolution verification results: {stdout}"
    );
}
