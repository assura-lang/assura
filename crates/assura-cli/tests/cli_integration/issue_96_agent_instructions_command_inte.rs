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
// Issue #96: agent-instructions command integration tests
// =======================================================================

#[test]
fn agent_instructions_exits_zero() {
    let out = Command::new(assura_bin())
        .arg("agent-instructions")
        .output()
        .expect("failed to run assura agent-instructions");
    assert!(
        out.status.success(),
        "agent-instructions should exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn agent_instructions_contains_type_mapping() {
    let out = Command::new(assura_bin())
        .arg("agent-instructions")
        .output()
        .expect("failed to run assura agent-instructions");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Type Mapping"),
        "should contain 'Type Mapping': {stdout}"
    );
}

#[test]
fn agent_instructions_contains_cli_commands() {
    let out = Command::new(assura_bin())
        .arg("agent-instructions")
        .output()
        .expect("failed to run assura agent-instructions");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("CLI Commands"),
        "should contain 'CLI Commands': {stdout}"
    );
}

#[test]
fn agent_instructions_contains_contract_syntax() {
    let out = Command::new(assura_bin())
        .arg("agent-instructions")
        .output()
        .expect("failed to run assura agent-instructions");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Contract Syntax"),
        "should contain 'Contract Syntax': {stdout}"
    );
}
