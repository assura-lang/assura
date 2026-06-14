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

// =======================================================================
// R007: Build CLI integration tests
// =======================================================================

#[test]
fn build_cli_output_creates_custom_dir() {
    let tmp = std::env::temp_dir().join("assura_r007_custom_output");
    let _ = std::fs::remove_dir_all(&tmp);
    let out = Command::new(assura_bin())
        .args([
            "build",
            "demos/libwebp-huffman.assura",
            "--output",
            tmp.to_str().unwrap(),
        ])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura build");
    assert!(
        out.status.success(),
        "build should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(tmp.join("Cargo.toml").exists(), "Cargo.toml should exist");
    assert!(tmp.join("src/lib.rs").exists(), "src/lib.rs should exist");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_cli_default_output_is_generated() {
    let workspace = std::env::temp_dir().join("assura_r007_default");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).unwrap();
    let demo_src = std::path::Path::new(&workspace_root()).join("demos/libwebp-huffman.assura");
    let demo_dest = workspace.join("input.assura");
    std::fs::copy(&demo_src, &demo_dest).unwrap();
    let out = Command::new(assura_bin())
        .args(["build", "input.assura"])
        .current_dir(&workspace)
        .output()
        .expect("failed to run assura build");
    assert!(
        out.status.success(),
        "build should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        workspace.join("generated/Cargo.toml").exists(),
        "default generated/Cargo.toml should exist"
    );
    assert!(
        workspace.join("generated/src/lib.rs").exists(),
        "default generated/src/lib.rs should exist"
    );
    let _ = std::fs::remove_dir_all(&workspace);
}

#[test]
fn build_cli_error_on_missing_file() {
    let out = Command::new(assura_bin())
        .args(["build", "nonexistent_file_r007.assura"])
        .output()
        .expect("failed to run assura build");
    assert!(!out.status.success(), "build should fail for missing file");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Error") || stderr.contains("error") || stderr.contains("No such file"),
        "stderr should mention error: {stderr}"
    );
}

// =======================================================================
// P001: Verbose and quiet mode tests
// =======================================================================

#[test]
fn verbose_check_shows_timing() {
    let out = Command::new(assura_bin())
        .args(["check", "--verbose", "demos/libwebp-huffman.assura"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura check --verbose");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Pipeline timing"),
        "should show pipeline timing header: {stderr}"
    );
    assert!(stderr.contains("lex:"), "should show lex timing: {stderr}");
    assert!(
        stderr.contains("parse:"),
        "should show parse timing: {stderr}"
    );
    assert!(
        stderr.contains("resolve:"),
        "should show resolve timing: {stderr}"
    );
    assert!(
        stderr.contains("typecheck:"),
        "should show typecheck timing: {stderr}"
    );
    assert!(
        stderr.contains("ms"),
        "should show millisecond units: {stderr}"
    );
    assert!(
        stderr.contains("total:"),
        "should show total timing: {stderr}"
    );
}

#[test]
fn quiet_check_suppresses_summary() {
    let out = Command::new(assura_bin())
        .args(["check", "--quiet", "demos/libwebp-huffman.assura"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura check --quiet");
    assert!(out.status.success(), "check should succeed");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("check passed"),
        "quiet mode should not show 'check passed': {stderr}"
    );
    assert!(
        !stderr.contains("Verification"),
        "quiet mode should not show verification summary: {stderr}"
    );
}

#[test]
fn quiet_check_shows_errors() {
    let out = Command::new(assura_bin())
        .args([
            "check",
            "--quiet",
            "tests/fixtures/must_reject/clause_type_error.assura",
        ])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura check --quiet on invalid file");
    assert!(!out.status.success(), "check should fail on invalid input");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("error"),
        "quiet mode should still show errors: {stderr}"
    );
}

#[test]
fn verbose_short_flag_works() {
    let out = Command::new(assura_bin())
        .args(["check", "-v", "demos/libwebp-huffman.assura"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura check -v");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Pipeline timing"),
        "-v should work like --verbose: {stderr}"
    );
}

#[test]
fn quiet_short_flag_works() {
    let out = Command::new(assura_bin())
        .args(["check", "-q", "demos/libwebp-huffman.assura"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura check -q");
    assert!(out.status.success(), "check should succeed");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("check passed"),
        "-q should work like --quiet: {stderr}"
    );
}

#[test]
fn verbose_build_shows_codegen_timing() {
    let tmp = std::env::temp_dir().join("assura_p001_verbose_build");
    let _ = std::fs::remove_dir_all(&tmp);
    let out = Command::new(assura_bin())
        .args([
            "build",
            "--verbose",
            "demos/libwebp-huffman.assura",
            "--output",
            tmp.to_str().unwrap(),
        ])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura build --verbose");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Pipeline timing"),
        "build --verbose should show timing: {stderr}"
    );
    assert!(
        stderr.contains("codegen:"),
        "build --verbose should show codegen timing: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn quiet_build_suppresses_file_listing() {
    let tmp = std::env::temp_dir().join("assura_p001_quiet_build");
    let _ = std::fs::remove_dir_all(&tmp);
    let out = Command::new(assura_bin())
        .args([
            "build",
            "--quiet",
            "demos/libwebp-huffman.assura",
            "--output",
            tmp.to_str().unwrap(),
            "--no-check",
        ])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura build --quiet");
    assert!(
        out.status.success(),
        "build should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("wrote"),
        "quiet mode should not list files: {stdout}"
    );
    assert!(
        !stdout.contains("OK"),
        "quiet mode should not show OK: {stdout}"
    );
    assert!(
        tmp.join("Cargo.toml").exists(),
        "files should still be generated in quiet mode"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

// =======================================================================
// I003: WASM target tests
// =======================================================================

#[test]
fn build_cli_wasm_target_generates_config() {
    let tmp = std::env::temp_dir().join("assura_i003_wasm");
    let _ = std::fs::remove_dir_all(&tmp);
    let out = Command::new(assura_bin())
        .args([
            "build",
            "demos/libwebp-huffman.assura",
            "--output",
            tmp.to_str().unwrap(),
            "--target",
            "wasm",
            "--no-check",
        ])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura build");
    assert!(
        out.status.success(),
        "build --target wasm should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cargo_config = tmp.join(".cargo/config.toml");
    assert!(
        cargo_config.exists(),
        ".cargo/config.toml should exist for WASM target"
    );
    let content = std::fs::read_to_string(&cargo_config).unwrap();
    assert!(
        content.contains("wasm32-wasip1"),
        ".cargo/config.toml should set wasm32-wasip1 target"
    );
    let cargo_toml = std::fs::read_to_string(tmp.join("Cargo.toml")).unwrap();
    assert!(
        cargo_toml.contains("wasm32-wasip1"),
        "Cargo.toml should mention WASM target"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_cli_native_target_no_cargo_config() {
    let tmp = std::env::temp_dir().join("assura_i003_native");
    let _ = std::fs::remove_dir_all(&tmp);
    let out = Command::new(assura_bin())
        .args([
            "build",
            "demos/libwebp-huffman.assura",
            "--output",
            tmp.to_str().unwrap(),
            "--no-check",
        ])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura build");
    assert!(
        out.status.success(),
        "build should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cargo_config = tmp.join(".cargo/config.toml");
    assert!(
        !cargo_config.exists(),
        ".cargo/config.toml should NOT exist for native target"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
