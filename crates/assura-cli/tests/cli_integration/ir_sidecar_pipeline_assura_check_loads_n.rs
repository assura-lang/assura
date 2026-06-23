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
// IR sidecar pipeline: assura check loads {Name}.ir from disk
// =======================================================================

#[test]
fn check_loads_ir_sidecar_and_verifies_ensures() {
    let tmp = unique_temp("assura_ir_e2e");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let assura_path = tmp.join("CopyBytes.assura");
    std::fs::write(
        &assura_path,
        r#"
contract CopyBytes {
  input(raw: Bytes)
  output(result: Bytes)
  requires { raw.length() > 0 }
  ensures  { result.length() <= raw.length() }
}
"#,
    )
    .unwrap();
    std::fs::write(
        tmp.join("CopyBytes.ir"),
        r#"
module copy {
  fn #0 : ($0: Bytes) -> Bytes ! pure
  {
    $result = load $0 : Bytes
  }
}
"#,
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .arg("check")
        .arg(assura_path.to_str().unwrap())
        .current_dir(&tmp)
        .output()
        .expect("failed to run assura check");

    assert!(
        out.status.success(),
        "check should succeed with IR sidecar: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("verified") || combined.contains("Verified"),
        "expected verified ensures, got: {combined}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ir_branch_sidecar_changes_verification_outcome() {
    let tmp = unique_temp("assura_ir_branch");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let assura_path = tmp.join("BranchMax.assura");
    std::fs::write(
        &assura_path,
        r#"
contract BranchMax {
  input(x: Int)
  output(result: Int)
  requires { x >= 0 }
  ensures  { result >= 0 }
}
"#,
    )
    .unwrap();
    std::fs::write(
        tmp.join("BranchMax.ir"),
        r#"
module branch {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $1 = if $0 then #1 else #2 : Int
    $result = load $1 : Int
  }
  fn #1 : ($0: Int) -> Int ! pure
  {
    $result = load $0 : Int
  }
  fn #2 : ($0: Int) -> Int ! pure
  {
    $result = const 0 : Int
  }
}
"#,
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .arg("check")
        .arg(assura_path.to_str().unwrap())
        .current_dir(&tmp)
        .output()
        .expect("failed to run assura check");

    assert!(
        out.status.success(),
        "check with branch IR sidecar should verify ensures: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("verified") || combined.contains("Verified"),
        "expected verified ensures with branch IR, got: {combined}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ir_branch_sidecar_broken_else_yields_counterexample() {
    let tmp = unique_temp("assura_ir_branch_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let assura_path = tmp.join("BranchMax.assura");
    std::fs::write(
        &assura_path,
        r#"
contract BranchMax {
  input(x: Int)
  output(result: Int)
  requires { x >= 0 }
  ensures  { result >= 0 }
}
"#,
    )
    .unwrap();
    // Broken #2 body: sets result to -1, violating ensures { result >= 0 }
    std::fs::write(
        tmp.join("BranchMax.ir"),
        r#"
module branch {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $1 = if $0 then #1 else #2 : Int
    $result = load $1 : Int
  }
  fn #1 : ($0: Int) -> Int ! pure
  {
    $result = load $0 : Int
  }
  fn #2 : ($0: Int) -> Int ! pure
  {
    $result = const -1 : Int
  }
}
"#,
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .arg("check")
        .arg(assura_path.to_str().unwrap())
        .current_dir(&tmp)
        .output()
        .expect("failed to run assura check");

    assert!(
        !out.status.success(),
        "check with broken branch IR should fail: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("counterexample") || combined.contains("Counterexample"),
        "expected counterexample from broken else branch, got: {combined}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_writes_stub_ir_sidecars_to_generated() {
    let tmp = unique_temp("assura_ir_build");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let assura_path = tmp.join("StubContract.assura");
    std::fs::write(
        &assura_path,
        r#"
contract StubContract {
  input(x: Int)
  output(result: Int)
  requires { x >= 0 }
  ensures  { result >= 0 }
}
"#,
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["build", assura_path.to_str().unwrap()])
        .current_dir(&tmp)
        .output()
        .expect("failed to run assura build");

    assert!(
        out.status.success(),
        "build should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ir_path = tmp.join("generated/StubContract.ir");
    assert!(
        ir_path.exists(),
        "build should write stub IR sidecar to generated/StubContract.ir"
    );
    let ir_text = std::fs::read_to_string(&ir_path).unwrap();
    assert!(
        ir_text.contains("$result = load $0"),
        "stub IR should identity-load first param"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
