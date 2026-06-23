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
// repl (#91)
// =======================================================================

#[test]
fn repl_quit_command_exits_zero() {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new(assura_bin())
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start assura repl");

    child.stdin.as_mut().unwrap().write_all(b":quit\n").unwrap();

    let out = child.wait_with_output().expect("failed to wait on repl");
    assert!(out.status.success(), "repl :quit should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Assura REPL"),
        "should show banner on stdout"
    );
}

#[test]
fn repl_type_command_maps_rust_types() {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new(assura_bin())
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start assura repl");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b":type Vec<i64>\n:quit\n")
        .unwrap();

    let out = child.wait_with_output().expect("failed to wait on repl");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("List<Int>"),
        "should map Vec<i64> to List<Int>, got: {stdout}"
    );
}

#[test]
fn repl_explain_command_shows_error_info() {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new(assura_bin())
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start assura repl");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b":explain A03001\n:quit\n")
        .unwrap();

    let out = child.wait_with_output().expect("failed to wait on repl");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("A03001"),
        "should show error code info, got: {stdout}"
    );
}

#[test]
fn repl_load_parses_file() {
    use std::io::Write;
    use std::process::Stdio;

    let root = workspace_root();
    let mut child = Command::new(assura_bin())
        .arg("repl")
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start assura repl");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b":load demos/libwebp-huffman.assura\n:quit\n")
        .unwrap();

    let out = child.wait_with_output().expect("failed to wait on repl");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("OK"),
        "should parse demo file successfully, got: {stdout}"
    );
}
