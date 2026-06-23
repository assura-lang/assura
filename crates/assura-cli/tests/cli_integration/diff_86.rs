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
// diff (#86)
// =======================================================================

#[test]
fn diff_identical_files_exits_zero() {
    let root = workspace_root();
    let demo = format!("{root}/demos/libwebp-huffman.assura");
    let out = Command::new(assura_bin())
        .args(["diff", &demo, &demo])
        .output()
        .expect("failed to run assura diff");
    assert!(out.status.success(), "diff of same file should exit 0");
}

#[test]
fn diff_different_files_exits_one() {
    let root = workspace_root();
    let out = Command::new(assura_bin())
        .args([
            "diff",
            &format!("{root}/demos/libwebp-huffman.assura"),
            &format!("{root}/demos/zlib-inflate.assura"),
        ])
        .output()
        .expect("failed to run assura diff");
    assert_eq!(
        out.status.code(),
        Some(1),
        "diff of different files should exit 1"
    );
}

#[test]
fn diff_json_output_is_valid() {
    let root = workspace_root();
    let out = Command::new(assura_bin())
        .args([
            "diff",
            &format!("{root}/demos/libwebp-huffman.assura"),
            &format!("{root}/demos/zlib-inflate.assura"),
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run assura diff --format json");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{stdout}"));
    assert!(
        parsed.get("changes").is_some(),
        "JSON should have changes array"
    );
    assert_eq!(
        parsed["identical"].as_bool(),
        Some(false),
        "should report not identical"
    );
}
