//! Integration tests for the `assura` CLI binary.
//!
//! These tests invoke the compiled `assura` binary via `std::process::Command`.
//! Using `env!("CARGO_BIN_EXE_assura")` guarantees Cargo builds the binary
//! before running these tests, so they work in clean environments (issue #47).

use std::path::PathBuf;
use std::process::Command;

/// Const bitwise NOT for typed lit.
#[test]
fn check_rust_encodes_const_bitnot() {
    let tmp = unique_temp("assura_check_rust_bitnot");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 250
fn n(x: u8) -> u8 { !5u8 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong const bitnot ensures must CE.
#[test]
fn check_rust_const_bitnot_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_bitnot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn n(x: u8) -> u8 { !5u8 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

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
// R007: Build CLI integration tests
// =======================================================================

#[test]
fn build_cli_output_creates_custom_dir() {
    let tmp = unique_temp("assura_r007_custom_output");
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
    let workspace = tempfile::tempdir().expect("failed to create temp dir");
    let workspace_path = workspace.path();
    let demo_src = std::path::Path::new(&workspace_root()).join("demos/libwebp-huffman.assura");
    let demo_dest = workspace_path.join("input.assura");
    std::fs::copy(&demo_src, &demo_dest).unwrap();
    let out = Command::new(assura_bin())
        .args(["build", "input.assura"])
        .current_dir(workspace_path)
        .output()
        .expect("failed to run assura build");
    assert!(
        out.status.success(),
        "build should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        workspace_path.join("generated/Cargo.toml").exists(),
        "default generated/Cargo.toml should exist"
    );
    assert!(
        workspace_path.join("generated/src/lib.rs").exists(),
        "default generated/src/lib.rs should exist"
    );
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
    let tmp = unique_temp("assura_p001_verbose_build");
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
    let tmp = unique_temp("assura_p001_quiet_build");
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
    let tmp = unique_temp("assura_i003_wasm");
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
    let tmp = unique_temp("assura_i003_native");
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

// =======================================================================
// Issue #96: doctor command integration tests
// =======================================================================

#[test]
fn doctor_exits_zero() {
    let out = Command::new(assura_bin())
        .arg("doctor")
        .output()
        .expect("failed to run assura doctor");
    assert!(
        out.status.success(),
        "doctor should exit 0 when deps are present: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn doctor_output_contains_rustc() {
    let out = Command::new(assura_bin())
        .arg("doctor")
        .output()
        .expect("failed to run assura doctor");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("rustc"),
        "doctor output should mention rustc: {stdout}"
    );
}

#[test]
fn doctor_output_contains_z3() {
    let out = Command::new(assura_bin())
        .arg("doctor")
        .output()
        .expect("failed to run assura doctor");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("z3"),
        "doctor output should mention z3: {stdout}"
    );
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

// =======================================================================
// Issue #96: completions command integration tests
// =======================================================================

#[test]
fn completions_zsh_exits_zero() {
    let out = Command::new(assura_bin())
        .args(["completions", "zsh"])
        .output()
        .expect("failed to run assura completions zsh");
    assert!(
        out.status.success(),
        "completions zsh should exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn completions_zsh_output_is_valid() {
    let out = Command::new(assura_bin())
        .args(["completions", "zsh"])
        .output()
        .expect("failed to run assura completions zsh");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("compdef") || stdout.contains("_assura"),
        "zsh completions should contain compdef or _assura: {}",
        &stdout[..stdout.len().min(200)]
    );
}

#[test]
fn completions_bash_exits_zero() {
    let out = Command::new(assura_bin())
        .args(["completions", "bash"])
        .output()
        .expect("failed to run assura completions bash");
    assert!(
        out.status.success(),
        "completions bash should exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn completions_fish_exits_zero() {
    let out = Command::new(assura_bin())
        .args(["completions", "fish"])
        .output()
        .expect("failed to run assura completions fish");
    assert!(
        out.status.success(),
        "completions fish should exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Issue #974: `completions --json` must emit JSON, not a bare shell script.
#[test]
fn completions_bash_json_is_parseable() {
    let out = Command::new(assura_bin())
        .args(["completions", "bash", "--json"])
        .output()
        .expect("failed to run assura completions bash --json");
    assert!(
        out.status.success(),
        "completions bash --json should exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("completions --json stdout must be JSON");
    assert_eq!(v["command"], "completions");
    assert_eq!(v["shell"], "bash");
    let script = v["script"].as_str().expect("script field must be a string");
    assert!(
        script.contains("_assura") || script.contains("assura"),
        "script should contain completion body: {}",
        &script[..script.len().min(120)]
    );
    // Must not look like a bare bash function at the start of stdout
    assert!(
        stdout.trim_start().starts_with('{'),
        "JSON mode must start with object, got: {}",
        &stdout[..stdout.len().min(80)]
    );
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

/// Bare `quit` / `help` under --json must not be parsed as contract source.
#[test]
fn repl_json_bare_help_and_quit() {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new(assura_bin())
        .args(["repl", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start assura repl --json");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"help\nquit\n")
        .unwrap();

    let out = child.wait_with_output().expect("failed to wait on repl");
    assert!(out.status.success(), "repl json help/quit should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut saw_help = false;
    let mut saw_quit = false;
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("NDJSON line: {e}\n{line}"));
        if v["command"] == "help" {
            saw_help = true;
            assert_eq!(v["ok"], true);
            assert!(v["commands"].is_array());
        }
        if v["command"] == "quit" {
            saw_quit = true;
        }
        // Must not treat "help" as a contract declaration
        assert!(
            v.get("declarations").is_none()
                || !v["declarations"]
                    .as_array()
                    .map(|a| a.iter().any(|x| x.as_str() == Some("help ")))
                    .unwrap_or(false),
            "must not parse help as contract: {line}"
        );
    }
    assert!(saw_help, "expected help NDJSON object, got: {stdout}");
    assert!(saw_quit, "expected quit NDJSON object, got: {stdout}");
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

// =======================================================================
// MCP server (#89)
// =======================================================================

fn mcp_call(messages: &[&str]) -> Vec<String> {
    use std::io::Write;
    use std::process::Stdio;

    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#;
    let notif = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

    let mut child = Command::new(assura_bin())
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start assura mcp");

    let stdin = child.stdin.as_mut().unwrap();
    stdin.write_all(init.as_bytes()).unwrap();
    stdin.write_all(b"\n").unwrap();
    stdin.write_all(notif.as_bytes()).unwrap();
    stdin.write_all(b"\n").unwrap();
    for msg in messages {
        stdin.write_all(msg.as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
    }
    drop(child.stdin.take());

    let out = child.wait_with_output().expect("failed to wait on mcp");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(String::from)
        .collect()
}

#[test]
fn mcp_tools_list_returns_all_tools() {
    let lines = mcp_call(&[r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#]);
    assert!(lines.len() >= 2, "expected at least 2 response lines");
    let tools_line = &lines[1];
    let parsed: serde_json::Value = serde_json::from_str(tools_line)
        .unwrap_or_else(|e| panic!("invalid JSON: {e}\n{tools_line}"));
    let tools = parsed["result"]["tools"]
        .as_array()
        .expect("tools should be an array");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"assura_check"), "missing assura_check");
    assert!(names.contains(&"assura_explain"), "missing assura_explain");
    assert!(
        names.contains(&"assura_type_map"),
        "missing assura_type_map"
    );
    assert!(names.contains(&"assura_infer"), "missing assura_infer");
    assert!(
        names.contains(&"assura_ir_prompt"),
        "missing assura_ir_prompt"
    );
}

#[test]
fn ir_prompt_command_lists_decls() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/test_basic.assura"
    );
    let out = Command::new(assura_bin())
        .args(["ir-prompt", fixture, "--list"])
        .output()
        .expect("spawn assura ir-prompt --list");
    assert!(
        out.status.success(),
        "ir-prompt --list failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.trim().is_empty(),
        "expected at least one declaration name"
    );
}

#[test]
fn ir_prompt_command_requires_decl_when_multiple_jobs() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/test_basic.assura"
    );
    let list_out = Command::new(assura_bin())
        .args(["ir-prompt", fixture, "--list"])
        .output()
        .expect("spawn assura ir-prompt --list");
    let decl_count = String::from_utf8_lossy(&list_out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count();
    if decl_count <= 1 {
        return;
    }

    let out = Command::new(assura_bin())
        .args(["ir-prompt", fixture])
        .output()
        .expect("spawn assura ir-prompt without --decl");
    assert!(
        !out.status.success(),
        "expected failure when multiple decls and no --decl"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--decl") || stderr.contains("--list"),
        "stderr should mention --decl or --list, got: {stderr}"
    );
}

#[test]
fn ir_prompt_command_emits_prompt_for_named_decl() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/test_basic.assura"
    );
    let list_out = Command::new(assura_bin())
        .args(["ir-prompt", fixture, "--list"])
        .output()
        .expect("spawn assura ir-prompt --list");
    let first_decl = String::from_utf8_lossy(&list_out.stdout)
        .lines()
        .next()
        .expect("fixture should have a decl")
        .trim()
        .to_string();

    let out = Command::new(assura_bin())
        .args(["ir-prompt", fixture, "--decl", &first_decl])
        .output()
        .expect("spawn assura ir-prompt --decl");
    assert!(
        out.status.success(),
        "ir-prompt --decl failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Instruction reference"),
        "prompt should include IR syntax reference"
    );
    assert!(
        stdout.contains(&first_decl),
        "prompt should mention declaration {first_decl}"
    );
    assert!(
        !stdout.contains("```\n// Generated IR"),
        "heuristic starter must not be wrapped in markdown fences"
    );
}

#[test]
fn mcp_ir_prompt_tool_returns_json() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/test_basic.assura"
    );
    let list_out = Command::new(assura_bin())
        .args(["ir-prompt", fixture, "--list"])
        .output()
        .expect("spawn assura ir-prompt --list");
    let first_decl = String::from_utf8_lossy(&list_out.stdout)
        .lines()
        .next()
        .expect("fixture should have a decl")
        .trim()
        .to_string();

    let call = format!(
        r#"{{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{{"name":"assura_ir_prompt","arguments":{{"file":"{fixture}","decl":"{first_decl}"}}}}}}"#
    );
    let lines = mcp_call(&[&call]);
    let response = lines.last().expect("should have response");
    let parsed: serde_json::Value =
        serde_json::from_str(response).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{response}"));
    let text = parsed["result"]["content"][0]["text"]
        .as_str()
        .expect("should have text content");
    let json: serde_json::Value =
        serde_json::from_str(text).unwrap_or_else(|e| panic!("invalid tool JSON: {e}\n{text}"));
    assert!(json["prompts"].is_array(), "expected prompts array");
    assert!(
        json["prompts"][0]["prompt"]
            .as_str()
            .is_some_and(|p| p.contains("Instruction reference")),
        "prompt should include IR reference"
    );
}

#[test]
fn mcp_type_map_tool_returns_mapping() {
    let lines = mcp_call(&[
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"assura_type_map","arguments":{"rust_type":"Vec<i64>"}}}"#,
    ]);
    let response = lines.last().expect("should have response");
    let parsed: serde_json::Value =
        serde_json::from_str(response).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{response}"));
    let text = parsed["result"]["content"][0]["text"]
        .as_str()
        .expect("should have text content");
    assert!(
        text.contains("List<Int>"),
        "should map Vec<i64> to List<Int>, got: {text}"
    );
}

#[test]
fn mcp_explain_tool_returns_error_info() {
    let lines = mcp_call(&[
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"assura_explain","arguments":{"code":"A03001"}}}"#,
    ]);
    let response = lines.last().expect("should have response");
    let parsed: serde_json::Value =
        serde_json::from_str(response).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{response}"));
    let text = parsed["result"]["content"][0]["text"]
        .as_str()
        .expect("should have text content");
    assert!(
        text.contains("A03001") && text.contains("Type mismatch"),
        "should contain error info, got: {text}"
    );
}

// =======================================================================
// check-rust: inline contract annotation verification
// =======================================================================

#[test]
fn check_rust_finds_annotations() {
    let tmp = unique_temp("assura_check_rust_test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    std::fs::write(
        tmp.join("src/lib.rs"),
        r#"
/// @requires x > 0
/// @ensures result > 0
fn positive(x: i32) -> i32 {
    x + 1
}
"#,
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["check-rust", tmp.join("src/lib.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("check-rust:") && stdout.contains("1 file(s)"),
        "should report file count, got: {stdout}"
    );
    assert!(
        stdout.contains("annotated item"),
        "should report annotated items, got: {stdout}"
    );
}

#[test]
fn check_rust_json_output() {
    let tmp = unique_temp("assura_check_rust_json");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("test.rs"),
        r#"
/// @requires a > 0
fn only_positive(a: i32) -> i32 {
    a
}
"#,
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args([
            "check-rust",
            "--json",
            tmp.join("test.rs").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{stdout}"));
    assert_eq!(parsed["files"], 1);
    assert_eq!(parsed["items"], 1);
    assert!(parsed["clauses"].as_u64().unwrap() >= 1);
}

#[test]
fn check_rust_no_annotations() {
    let tmp = unique_temp("assura_check_rust_none");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("plain.rs"),
        r#"
/// Regular doc comment, no annotations.
fn regular(x: i32) -> i32 {
    x
}
"#,
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["check-rust", tmp.join("plain.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("no inline contract annotations found"),
        "should report no annotations, got: {stdout}"
    );
}

/// Ensures without co-located IR must not print "check passed" / "ensures …
/// verified" before body_not_modeled (MPI End User / Observability).
#[test]
fn check_rust_body_not_modeled_human_is_honest() {
    let tmp = unique_temp("assura_check_rust_bnm");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @requires x > 0
/// @ensures result == x + 1
fn bad(x: i64) -> i64 { x }
"#,
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["check-rust", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        !out.status.success(),
        "body_not_modeled should be non-zero exit, got stdout={stdout} stderr={stderr}"
    );
    assert!(
        combined.contains("body_not_modeled"),
        "expected body_not_modeled status, got: {combined}"
    );
    assert!(
        !combined.contains("check passed"),
        "must not claim check passed when body is not modeled: {combined}"
    );
    // Grouped SMT table uses "ensures ... verified"; must stay silent for BNM.
    assert!(
        !combined.contains("... verified"),
        "must not print SMT 'ensures ... verified' before body_not_modeled: {combined}"
    );
}

#[test]
fn check_rust_directory_scan() {
    let tmp = unique_temp("assura_check_rust_dir");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    std::fs::write(
        tmp.join("src/a.rs"),
        r#"
/// @requires n > 0
fn f(n: i32) -> i32 { n }
"#,
    )
    .unwrap();
    std::fs::write(
        tmp.join("src/b.rs"),
        r#"
/// @invariant self.x >= 0
struct Foo { x: i32 }
"#,
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["check-rust", tmp.join("src").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("2 file(s)"),
        "should find 2 files, got: {stdout}"
    );
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
fn build_does_not_write_identity_stub_ir_for_unanalyzable_ensures() {
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
    // Identity "Stub IR" must not be persisted (would poison co-located load/codegen).
    let ir_path = tmp.join("generated/StubContract.ir");
    assert!(
        !ir_path.exists(),
        "build must not write identity stub IR for unanalyzable ensures"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_write_ir_writes_analyzable_colocated_sidecar() {
    let tmp = unique_temp("assura_write_ir_ok");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let assura_path = tmp.join("Echo.assura");
    std::fs::write(
        &assura_path,
        r#"
contract Echo {
  input(x: Int)
  output(result: Int)
  ensures { result == x }
}
"#,
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args([
            "build",
            assura_path.to_str().unwrap(),
            "--write-ir",
            "--output",
            tmp.join("out").to_str().unwrap(),
        ])
        .current_dir(&tmp)
        .output()
        .expect("failed to run assura build --write-ir");

    assert!(
        out.status.success(),
        "build --write-ir should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ir_path = tmp.join("Echo.ir");
    assert!(
        ir_path.exists(),
        "analyzable ensures should get co-located Echo.ir"
    );
    let ir_text = std::fs::read_to_string(&ir_path).unwrap();
    assert!(!ir_text.contains("Stub IR"), "must not be a labeled stub");
    assert!(
        ir_text.contains("load $0") || ir_text.contains("$result"),
        "expected identity-style body, got: {ir_text}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

// =======================================================================
// Build command: additional edge-case tests
// =======================================================================

#[test]
fn build_no_check_shows_check_skipped() {
    let tmp = unique_temp("assura_build_nocheck");
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
        .expect("failed to run assura build --no-check");
    assert!(
        out.status.success(),
        "build --no-check should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("check skipped"),
        "should mention 'check skipped' in stdout: {stdout}"
    );
    // Files should still be generated
    assert!(
        tmp.join("Cargo.toml").exists(),
        "Cargo.toml should still be generated with --no-check"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_with_type_errors_fails() {
    let tmp = unique_temp("assura_build_type_err");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let src = tmp.join("bad.assura");
    // requires clause body is a String, not Bool => type error
    std::fs::write(
        &src,
        r#"
contract Bad {
  input(x: Int)
  output(result: Int)
  requires { "not a bool" }
  ensures { result >= 0 }
}
"#,
    )
    .unwrap();

    let out_dir = tmp.join("out");
    let out = Command::new(assura_bin())
        .args([
            "build",
            src.to_str().unwrap(),
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run assura build with type errors");
    assert!(
        !out.status.success(),
        "build should fail on type-error source"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("error") || stderr.contains("Error"),
        "stderr should mention error: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_generates_debug_assert_for_requires() {
    let tmp = unique_temp("assura_build_debug_assert");
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

    // The generated lib.rs should contain debug_assert! from requires clauses
    let lib_path = tmp.join("src/lib.rs");
    assert!(lib_path.exists(), "src/lib.rs should exist");
    let lib_content = std::fs::read_to_string(&lib_path).unwrap();
    assert!(
        lib_content.contains("debug_assert!"),
        "generated Rust should contain debug_assert! from requires clauses"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_solver_flag_z3_accepted() {
    let tmp = unique_temp("assura_build_solver_z3");
    let _ = std::fs::remove_dir_all(&tmp);
    let out = Command::new(assura_bin())
        .args([
            "build",
            "demos/libwebp-huffman.assura",
            "--output",
            tmp.to_str().unwrap(),
            "--solver",
            "z3",
            "--no-check",
        ])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura build --solver z3");
    assert!(
        out.status.success(),
        "build --solver z3 should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        tmp.join("Cargo.toml").exists(),
        "Cargo.toml should exist after build with --solver z3"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_multiple_demos_succeed() {
    // Verify that several demo files all build successfully.
    let demos = [
        "demos/zlib-inflate.assura",
        "demos/taint-tracking.assura",
        "demos/heartbleed.assura",
    ];
    for demo in &demos {
        let tmp = unique_temp(&format!(
            "assura_build_multi_{}",
            demo.replace(['/', '.'], "_")
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let out = Command::new(assura_bin())
            .args([
                "build",
                demo,
                "--output",
                tmp.to_str().unwrap(),
                "--no-check",
            ])
            .current_dir(workspace_root())
            .output()
            .unwrap_or_else(|e| panic!("failed to run assura build {demo}: {e}"));
        assert!(
            out.status.success(),
            "build {demo} should succeed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            tmp.join("Cargo.toml").exists(),
            "Cargo.toml should exist for {demo}"
        );
        assert!(
            tmp.join("src/lib.rs").exists(),
            "src/lib.rs should exist for {demo}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

// =======================================================================
// Init / Check / Build workflow integration tests
// =======================================================================

#[test]
fn init_creates_project_structure() {
    let tmp = unique_temp("assura_init_structure");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let out = Command::new(assura_bin())
        .args(["init", "test-project"])
        .current_dir(&tmp)
        .output()
        .expect("failed to run assura init");
    assert!(
        out.status.success(),
        "init should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let project = tmp.join("test-project");
    assert!(
        project.join("assura.toml").exists(),
        "assura.toml should exist"
    );
    assert!(
        project.join("contracts/lib.assura").exists(),
        "contracts/lib.assura should exist"
    );

    let toml_content = std::fs::read_to_string(project.join("assura.toml")).unwrap();
    assert!(
        toml_content.contains("[package]"),
        "assura.toml should contain [package]: {toml_content}"
    );

    let lib_content = std::fs::read_to_string(project.join("contracts/lib.assura")).unwrap();
    assert!(
        lib_content.contains("SafeDivision"),
        "lib.assura should contain SafeDivision: {lib_content}"
    );
    assert!(
        lib_content.contains("result == a / b"),
        "template ensures should mention result (not vacuous requires copy): {lib_content}"
    );
    assert!(
        project.join("contracts/SafeDivision.ir").exists(),
        "co-located SafeDivision.ir should exist for result ensures"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn init_fails_on_existing_directory() {
    let tmp = unique_temp("assura_init_existing");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // Pre-create the project directory so init should fail
    std::fs::create_dir_all(tmp.join("test-project")).unwrap();

    let out = Command::new(assura_bin())
        .args(["init", "test-project"])
        .current_dir(&tmp)
        .output()
        .expect("failed to run assura init");
    assert!(
        !out.status.success(),
        "init should fail when directory already exists"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("already exists"),
        "stderr should mention directory already exists: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn init_rejects_empty_and_invalid_names() {
    let tmp = unique_temp("assura_init_invalid_names");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    for bad in ["", "bad name", "1lead", "a/b", ".", ".."] {
        let out = Command::new(assura_bin())
            .args(["init", bad])
            .current_dir(&tmp)
            .output()
            .expect("failed to run assura init");
        assert_eq!(
            out.status.code(),
            Some(2),
            "init {bad:?} should exit 2: stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        // Empty name must not write into the working directory.
        assert!(
            !tmp.join("assura.toml").exists(),
            "init {bad:?} must not create assura.toml in cwd"
        );
        assert!(
            !tmp.join("contracts").exists(),
            "init {bad:?} must not create contracts/ in cwd"
        );
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn check_rejects_invalid_layer() {
    let tmp = unique_temp("assura_layer_invalid");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let src = tmp.join("ok.assura");
    std::fs::write(
        &src,
        "contract T { input(x: Int) requires { x >= 0 } ensures { x >= 0 } }\n",
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["check", src.to_str().unwrap(), "--layer", "99"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run check --layer 99");
    assert_eq!(
        out.status.code(),
        Some(2),
        "invalid layer should exit 2: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("invalid --layer") || stderr.contains("expected 0"),
        "stderr should explain layer range: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn init_then_check_contracts() {
    let tmp = unique_temp("assura_init_check");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // Step 1: init
    let init_out = Command::new(assura_bin())
        .args(["init", "test-project"])
        .current_dir(&tmp)
        .output()
        .expect("failed to run assura init");
    assert!(
        init_out.status.success(),
        "init should succeed: {}",
        String::from_utf8_lossy(&init_out.stderr)
    );

    // Step 2: check the generated contract file
    let contract = tmp.join("test-project/contracts/lib.assura");
    let check_out = Command::new(assura_bin())
        .args(["check", contract.to_str().unwrap()])
        .output()
        .expect("failed to run assura check on init'd contract");
    assert!(
        check_out.status.success(),
        "check on init'd contract should succeed: {}",
        String::from_utf8_lossy(&check_out.stderr)
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn check_project_directory_mode() {
    let tmp = unique_temp("assura_check_project");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("contracts")).unwrap();

    // Create assura.toml
    std::fs::write(
        tmp.join("assura.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    // Create a valid contract
    std::fs::write(
        tmp.join("contracts/lib.assura"),
        "contract Simple {\n    input(x: Int)\n    output(result: Int)\n    requires { x >= 0 }\n    ensures { x >= 0 }\n}\n",
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["check", tmp.to_str().unwrap()])
        .output()
        .expect("failed to run assura check on project directory");
    assert!(
        out.status.success(),
        "check on project directory should succeed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn init_then_build_generates_rust() {
    let tmp = unique_temp("assura_init_build");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // Step 1: init
    let init_out = Command::new(assura_bin())
        .args(["init", "test-project"])
        .current_dir(&tmp)
        .output()
        .expect("failed to run assura init");
    assert!(
        init_out.status.success(),
        "init should succeed: {}",
        String::from_utf8_lossy(&init_out.stderr)
    );

    // Step 2: build the generated contract
    let contract = tmp.join("test-project/contracts/lib.assura");
    let gen_dir = tmp.join("generated");
    let build_out = Command::new(assura_bin())
        .args([
            "build",
            contract.to_str().unwrap(),
            "--output",
            gen_dir.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run assura build on init'd contract");
    assert!(
        build_out.status.success(),
        "build on init'd contract should succeed: {}",
        String::from_utf8_lossy(&build_out.stderr)
    );
    assert!(
        gen_dir.join("Cargo.toml").exists(),
        "generated/Cargo.toml should exist"
    );
    assert!(
        gen_dir.join("src/lib.rs").exists(),
        "generated/src/lib.rs should exist"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

// =======================================================================
// #679: Build produces compiled artifact (not just codegen)
// =======================================================================

#[test]
fn build_produces_native_artifact() {
    let tmp = unique_temp("assura_build_artifact");
    let _ = std::fs::remove_dir_all(&tmp);
    // Run without --no-check so cargo build actually runs
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

    // The generated project should have been compiled by cargo
    let target_dir = tmp.join("target/debug/deps");
    assert!(
        target_dir.exists(),
        "target/debug/deps should exist after cargo build"
    );

    // Check that an .rlib artifact was produced
    let has_rlib = std::fs::read_dir(&target_dir)
        .expect("should be able to read deps dir")
        .flatten()
        .any(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "rlib" || ext == "rmeta")
        });
    assert!(has_rlib, "should produce an rlib or rmeta artifact");

    // CLI output should mention the artifact path and size
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("OK") && stdout.contains("bytes"),
        "stdout should report artifact path and size: {stdout}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_output_includes_artifact_size() {
    let tmp = unique_temp("assura_build_artifact_size");
    let _ = std::fs::remove_dir_all(&tmp);
    let out = Command::new(assura_bin())
        .args([
            "build",
            "demos/heartbleed.assura",
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

    let stdout = String::from_utf8_lossy(&out.stdout);
    // The output should report the artifact with a byte count (e.g. "1234 bytes")
    // or at minimum the OK status line
    assert!(
        stdout.contains("OK"),
        "stdout should contain OK status line: {stdout}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

// =======================================================================
// CLI subcommand smoke tests (fmt, doc, test-gen, agent-instructions, doctor)
// =======================================================================

#[test]
fn fmt_formats_valid_file() {
    let tmp = unique_temp("assura_fmt_ok");
    let file = tmp.join("test.assura");
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::copy(
        format!("{}/demos/libwebp-huffman.assura", workspace_root()),
        &file,
    )
    .unwrap();
    // Format in-place first
    let out = Command::new(assura_bin())
        .args(["fmt", file.to_str().unwrap()])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura fmt");
    assert!(
        out.status.success(),
        "fmt should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // --check on the already-formatted file should succeed
    let out2 = Command::new(assura_bin())
        .args(["fmt", file.to_str().unwrap(), "--check"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura fmt --check");
    assert!(
        out2.status.success(),
        "fmt --check should succeed on already-formatted file: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn fmt_rejects_unparseable_input() {
    let tmp = unique_temp("assura_fmt_bad");
    let bad_file = tmp.join("bad.assura");
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(&bad_file, "@@@ not valid syntax").unwrap();
    let out = Command::new(assura_bin())
        .args(["fmt", bad_file.to_str().unwrap(), "--check"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura fmt");
    assert!(
        !out.status.success(),
        "fmt --check should fail on unparseable input"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("parse error"),
        "stderr should mention parse error: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn doc_generates_markdown_for_demo() {
    let tmp = unique_temp("assura_doc_output");
    let _ = std::fs::remove_dir_all(&tmp);
    let out = Command::new(assura_bin())
        .args([
            "doc",
            "demos/libwebp-huffman.assura",
            "--output",
            tmp.to_str().unwrap(),
        ])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura doc");
    assert!(
        out.status.success(),
        "doc should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Should have created at least one .md file in the output dir
    let has_md = tmp.exists()
        && std::fs::read_dir(&tmp)
            .map(|entries| {
                entries
                    .flatten()
                    .any(|e| e.path().extension().is_some_and(|x| x == "md"))
            })
            .unwrap_or(false);
    assert!(has_md, "doc should produce .md files in output dir");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_gen_produces_output_for_demo() {
    let out = Command::new(assura_bin())
        .args(["test-gen", "demos/heartbleed.assura"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura test-gen");
    assert!(
        out.status.success(),
        "test-gen should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Generated tests should contain Rust test markers
    assert!(
        stdout.contains("#[test]") && stdout.contains("fn "),
        "test-gen should produce Rust test code: {stdout}"
    );
}

#[test]
fn agent_instructions_prints_reference() {
    let out = Command::new(assura_bin())
        .args(["agent-instructions"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura agent-instructions");
    assert!(
        out.status.success(),
        "agent-instructions should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Should contain the type mapping table and basic reference info
    assert!(
        stdout.contains("Int") && stdout.contains("Bool"),
        "agent-instructions should print type mappings: {stdout}"
    );
}

#[test]
fn doctor_checks_installation() {
    let out = Command::new(assura_bin())
        .args(["doctor"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura doctor");
    // doctor should succeed (rustc and cargo are available in test env)
    assert!(
        out.status.success(),
        "doctor should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Assura Doctor"),
        "doctor should print header: {stdout}"
    );
    assert!(
        stdout.contains("rustc:") && stdout.contains("cargo:"),
        "doctor should check rustc and cargo: {stdout}"
    );
}

// =======================================================================
// fixrealloop: stdin (`-`) for assura check
// =======================================================================

#[test]
fn check_reads_source_from_stdin_dash() {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new(assura_bin())
        .args(["check", "-", "--json"])
        .current_dir(workspace_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn assura check -");

    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(
                b"contract T {\n  input(x: Int)\n  requires { x >= 0 }\n  ensures { x >= 0 }\n}\n",
            )
            .expect("write stdin");
    }

    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "check - should succeed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim_start().starts_with('{'),
        "JSON output expected: {stdout}"
    );
    assert!(
        stdout.contains("<stdin>") || stdout.contains("\"success\""),
        "stdin check should produce file_info: {stdout}"
    );
}

#[test]
fn check_watch_rejects_stdin() {
    let out = Command::new(assura_bin())
        .args(["check", "-", "--watch"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura check - --watch");
    assert_eq!(
        out.status.code(),
        Some(2),
        "watch+stdin should exit 2: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("stdin") || stderr.contains("watch"),
        "error should mention watch/stdin: {stderr}"
    );
}

#[test]
fn check_watch_rejects_stdin_json() {
    let out = Command::new(assura_bin())
        .args(["check", "-", "--watch", "--json"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura check - --watch --json");
    assert_eq!(
        out.status.code(),
        Some(2),
        "watch+stdin --json should exit 2: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("watch+stdin --json must be JSON");
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"], "watch_stdin_unsupported");
}

/// Simple if body encodes (Clamp.ir-style multi-block) (#986).
#[test]
fn check_rust_encodes_if_body() {
    let tmp = unique_temp("assura_check_rust_body_if");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn clamp0(x: i64) -> i64 { if x > 0 { x } else { 0 } }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "if body should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["verified"].as_u64().unwrap_or(0) >= 1);

    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result >= 0
fn clamp0(x: i64) -> i64 { if x > 0 { x } else { -1 } }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "wrong else branch should CE");
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Single `let y = e; y` inlines to encode `e` (#986).
#[test]
fn check_rust_encodes_let_inline_body() {
    let tmp = unique_temp("assura_check_rust_body_let");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x + 1
fn multi(x: i64) -> i64 { let y = x + 1; y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "let-inline body should pass: {stdout}"
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["verified"].as_u64().unwrap_or(0) >= 1);

    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x + 1
fn multi(x: i64) -> i64 { let y = x; y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// abs/min method bodies encode and verify simple ensures.
#[test]
fn check_rust_encodes_abs_min_bodies() {
    let tmp = unique_temp("assura_check_rust_body_abs");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("lib.rs"),
        r#"
/// @ensures result >= 0
fn abs_like(x: i64) -> i64 { x.abs() }

/// @ensures result <= x
/// @ensures result <= y
fn mymin(x: i64, y: i64) -> i64 { x.min(y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("lib.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "abs/min bodies should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 3, "{stdout}");
}

/// Nested / mul body encoding: correct body verifies; wrong body CEs.
#[test]
fn check_rust_encodes_nested_and_mul_bodies() {
    let tmp = unique_temp("assura_check_rust_body_nested");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x + y + 1
fn nest(x: i64, y: i64) -> i64 { x + y + 1 }

/// @ensures result == x * 2
fn mul(x: i64) -> i64 { x * 2 }

/// @ensures result == -x
fn neg(x: i64) -> i64 { -x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(
        v["body_not_modeled"], 0,
        "all three bodies should encode: {stdout}"
    );
    assert!(out.status.success(), "correct bodies should pass: {stdout}");
    assert!(
        v["verified"].as_u64().unwrap_or(0) >= 3,
        "expected >=3 verified clauses: {stdout}"
    );

    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x * 2
fn mul(x: i64) -> i64 { x + 2 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "wrong mul body should fail");
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Multi-let bodies fold into a single expression and verify.
#[test]
fn check_rust_encodes_multi_let_body() {
    let tmp = unique_temp("assura_check_rust_body_multilet");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x + 2
fn f(x: i64) -> i64 { let a = x + 1; let b = a + 1; b }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "multi-let should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 1, "{stdout}");

    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x + 2
fn f(x: i64) -> i64 { let a = x + 1; let b = a; b }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "wrong multi-let should CE");
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Simple match (literal + wildcard) encodes multi-block IR (#993).
#[test]
fn check_rust_encodes_match_body() {
    let tmp = unique_temp("assura_check_rust_body_match");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn sign(x: i64) -> i64 {
    match x {
        0 => 0,
        _ => 1,
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "match body should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 1, "{stdout}");

    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result >= 0
fn sign(x: i64) -> i64 {
    match x {
        0 => 0,
        _ => -1,
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "wrong match arm should CE");
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// If branches with simple `let y = e; y` fold and verify.
#[test]
fn check_rust_encodes_if_let_branch() {
    let tmp = unique_temp("assura_check_rust_body_if_let");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: i64) -> i64 {
    if x > 0 {
        let y = x;
        y
    } else {
        0
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "if-let branch should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 1, "{stdout}");
}

/// Identity match guards rewrite to if-tree (#999).
#[test]
fn check_rust_encodes_match_guard() {
    let tmp = unique_temp("assura_check_rust_body_match_guard");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn f(x: i64) -> i64 {
    match x {
        n if n > 0 => n,
        _ => 0,
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "match guard should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 1, "{stdout}");

    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result >= 0
fn f(x: i64) -> i64 {
    match x {
        n if n > 0 => n,
        _ => -1,
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "wrong default should CE");
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Guarded match with non-identity wrong arm must CE (not BNM).
#[test]
fn check_rust_match_guard_wrong_arm_ce() {
    let tmp = unique_temp("assura_check_rust_match_guard_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result >= 0
fn f(x: i64) -> i64 {
    match x {
        n if n > 10 => n,
        n if n > 0 => -1,
        _ => 0,
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "wrong guarded arm should CE");
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{v}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Bool not and bool match encode.
#[test]
fn check_rust_encodes_bool_not_and_match() {
    let tmp = unique_temp("assura_check_rust_bool_not");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn notb(b: bool) -> bool { !b }

/// @ensures result == true || result == false
fn m(b: bool) -> bool {
    match b {
        true => true,
        false => false,
    }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "bool bodies should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 2, "{stdout}");

    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == b
fn notb(b: bool) -> bool { !b }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// x.clamp(lo, hi) encodes as min(max(x, lo), hi).
#[test]
fn check_rust_encodes_clamp() {
    let tmp = unique_temp("assura_check_rust_clamp");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 10
fn f(x: i64) -> i64 { x.clamp(0, 10) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "clamp should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 2, "{stdout}");
}

/// clamp(x, y, y) peeps to y.
#[test]
fn check_rust_encodes_clamp_same_bounds() {
    let tmp = unique_temp("assura_check_rust_clamp_same");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == y
fn f(x: i64, y: i64) -> i64 { x.clamp(y, y) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Parametric clamp needs lo<=hi requires for range ensures.
#[test]
fn check_rust_encodes_clamp_params() {
    let tmp = unique_temp("assura_check_rust_clamp_params");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @requires lo <= hi
/// @ensures result >= lo
/// @ensures result <= hi
fn f(x: i64, lo: i64, hi: i64) -> i64 { x.clamp(lo, hi) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "parametric clamp should pass: {stdout}"
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 2, "{stdout}");

    // Without lo<=hi, range ensures can fail (sound CE or error).
    std::fs::write(
        tmp.join("no_req.rs"),
        r#"
/// @ensures result >= lo
/// @ensures result <= hi
fn f(x: i64, lo: i64, hi: i64) -> i64 { x.clamp(lo, hi) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args([
            "check-rust",
            "--json",
            tmp.join("no_req.rs").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "clamp without lo<=hi should not soft-pass: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// saturating_add encodes with i64 range requires (Closes #1007).
#[test]
fn check_rust_encodes_saturating_add() {
    let tmp = unique_temp("assura_check_rust_sat_add");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= x
fn f(x: i64) -> i64 { x.saturating_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "saturating_add should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 1, "{stdout}");
}

/// i32 saturating_add clamps to i32 range (not i64).
#[test]
fn check_rust_i32_saturating_add() {
    let tmp = unique_temp("assura_check_rust_i32_sat");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result <= 2147483647
fn f(x: i32) -> i32 { x.saturating_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "i32 sat should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// u32 saturating_add with unsigned range requires.
#[test]
fn check_rust_u32_saturating_add() {
    let tmp = unique_temp("assura_check_rust_u32_sat");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= x
/// @ensures result <= 4294967295
fn f(x: u32) -> u32 { x.saturating_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "u32 sat should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// saturating_mul encodes with type-width clamp.
#[test]
fn check_rust_encodes_saturating_mul() {
    let tmp = unique_temp("assura_check_rust_sat_mul");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @requires x >= 0
/// @ensures result >= x
fn f(x: i64) -> i64 { x.saturating_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "sat mul: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// is_positive / is_negative method bodies encode as Bool cmp.
#[test]
fn check_rust_encodes_is_positive() {
    let tmp = unique_temp("assura_check_rust_is_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn f(x: i64) -> bool { x.is_positive() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// is_zero encodes as cmp eq 0.
#[test]
fn check_rust_encodes_is_zero() {
    let tmp = unique_temp("assura_check_rust_is_zero");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn f(x: i64) -> bool { x.is_zero() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// clone/to_owned on ints encode as identity.
#[test]
fn check_rust_encodes_clone() {
    let tmp = unique_temp("assura_check_rust_clone");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x
fn f(x: i64) -> i64 { x.clone() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// signum encodes as clamp to [-1, 1] (single-block) and verifies range ensures.
#[test]
fn check_rust_encodes_signum() {
    let tmp = unique_temp("assura_check_rust_signum");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == -1 || result == 0 || result == 1
fn f(x: i64) -> i64 { x.signum() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Nested signum in arith encodes (#1032); proves result in {-1,0,1,2}.
#[test]
fn check_rust_encodes_nested_signum() {
    let tmp = unique_temp("assura_check_rust_nested_signum");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 2
fn s(x: i64) -> i64 { x.signum() + 1 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// signum chains: abs, sum receiver, product with x (#1032 follow-through).
#[test]
fn check_rust_encodes_signum_chains() {
    let tmp = unique_temp("assura_check_rust_signum_chains");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 1
fn a(x: i64) -> i64 { x.signum().abs() }

/// @ensures result >= -1
/// @ensures result <= 1
fn t(x: i64, y: i64) -> i64 { (x + y).signum() }

/// @ensures result == x || result == -x || result == 0
fn m(x: i64) -> i64 { x.signum() * x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Associated i64::max / i64::from encode.
#[test]
fn check_rust_encodes_assoc_max_from() {
    let tmp = unique_temp("assura_check_rust_assoc");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x
fn m(x: i64) -> i64 { i64::max(x, x) }

/// @ensures result == x
fn f(x: i32) -> i64 { i64::from(x) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Logical && / || on bools encode and verify (0/1 mul / or-ne0).
#[test]
fn check_rust_encodes_bool_logic() {
    let tmp = unique_temp("assura_check_rust_bool_logic");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == (a && b)
fn both(a: bool, b: bool) -> bool { a && b }

/// @ensures result == (a || b)
fn either(a: bool, b: bool) -> bool { a || b }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "bool logic should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 2, "{stdout}");
}

/// is_multiple_of encodes mod/eq; into/as are identity on i64.
#[test]
fn check_rust_encodes_multiple_into_as() {
    let tmp = unique_temp("assura_check_rust_multiple_into");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == (x % 2 == 0)
fn even(x: i64) -> bool { x.is_multiple_of(2) }

/// @ensures result == true
fn by_one(x: i64) -> bool { x.is_multiple_of(1) }

/// @ensures result == true
fn by_neg_one(x: i64) -> bool { x.is_multiple_of(-1) }

/// @ensures result == x
fn id_into(x: i64) -> i64 { x.into() }

/// @ensures result == x
fn id_as(x: i64) -> i64 { x as i64 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "multiple/into/as should pass: {stdout}"
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 5, "{stdout}");
}

/// abs_diff and ref/deref encode and verify.
#[test]
fn check_rust_encodes_abs_diff_ref() {
    let tmp = unique_temp("assura_check_rust_abs_diff");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn d(x: i64, y: i64) -> i64 { x.abs_diff(y) }

/// @ensures result == x
fn r(x: i64) -> i64 { *&x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "abs_diff/ref should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 2, "{stdout}");
}

/// Same-path peeps: abs_diff/min/max identity.
#[test]
fn check_rust_encodes_same_path_peeps() {
    let tmp = unique_temp("assura_check_rust_same_path");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0
fn d(x: i64) -> i64 { x.abs_diff(x) }

/// @ensures result == x
fn mn(x: i64) -> i64 { x.min(x) }

/// @ensures result == x
fn mx(x: i64) -> i64 { x.max(x) }

/// @ensures result == x
fn free(x: i64) -> i64 { min(x, x) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// abs/saturating_abs().is_negative() peeps to false.
#[test]
fn check_rust_encodes_abs_never_negative() {
    let tmp = unique_temp("assura_check_rust_abs_nn");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == false
fn a(x: i64) -> bool { x.abs().is_negative() }

/// @ensures result == false
fn s(x: i64) -> bool { x.saturating_abs().is_negative() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// abs_diff(x,x).is_zero / is_positive peeps.
#[test]
fn check_rust_encodes_abs_diff_self_bool_peeps() {
    let tmp = unique_temp("assura_check_rust_ad_self");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true
fn z(x: i64) -> bool { x.abs_diff(x).is_zero() }

/// @ensures result == false
fn p(x: i64) -> bool { x.abs_diff(x).is_positive() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// PartialOrd methods (x.gt(&0)) encode via cmp + ref strip.
#[test]
fn check_rust_encodes_partial_ord() {
    let tmp = unique_temp("assura_check_rust_partial_ord");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == (x > 0)
fn pos(x: i64) -> bool { x.gt(&0) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "partial ord should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// i64::default / i64::MAX encode as const bodies.
#[test]
fn check_rust_encodes_default_minmax() {
    let tmp = unique_temp("assura_check_rust_default_minmax");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0
fn z(x: i64) -> i64 { i64::default() }

/// @ensures result == 9223372036854775807
fn mx(x: i64) -> i64 { i64::MAX }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "default/minmax should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// x.pow(2) encodes as mul and verifies square ensures.
#[test]
fn check_rust_encodes_pow() {
    let tmp = unique_temp("assura_check_rust_pow");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x * x
fn sq(x: i64) -> i64 { x.pow(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "pow should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// bool.not() and as_ref identity encode.
#[test]
fn check_rust_encodes_not_method() {
    let tmp = unique_temp("assura_check_rust_not");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == !a
fn n(a: bool) -> bool { a.not() }

/// @ensures result == x
fn r(x: i64) -> i64 { x.as_ref() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "not/as_ref should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Multi-let folds through &x / *y ref patterns.
#[test]
fn check_rust_encodes_multi_let_ref() {
    let tmp = unique_temp("assura_check_rust_multilet_ref");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x
fn f(x: i64) -> i64 { let y = &x; *y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "multi-let ref should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Narrowing `as i32` must not pretend to model the body (BNM).
#[test]
fn check_rust_narrowing_cast_body_not_modeled() {
    let tmp = unique_temp("assura_check_rust_narrow_cast");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x
fn f(x: i64) -> i32 { x as i32 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    // Should not claim verified body model; BNM or type issues
    assert!(
        v["body_not_modeled"].as_u64().unwrap_or(0) >= 1 || !out.status.success(),
        "narrowing cast must not soft-pass as verified body: {stdout}"
    );
    // specifically no false success with body_not_modeled=0 and verified>0 without model
    if out.status.success() {
        assert_ne!(v["body_not_modeled"], 0, "must BNM: {stdout}");
    }
}

/// Nested methods (abs then is_positive) encode and verify for non-min.
#[test]
fn check_rust_encodes_nested_methods() {
    let tmp = unique_temp("assura_check_rust_nested_methods");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    // Note: i64::MIN.abs() is not positive in Rust (overflow); range requires
    // include MIN, so avoid ensures that claim abs().is_positive() <=> x != 0.
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == (x > 0)
fn f(x: i64) -> bool { x.is_positive() }

/// @ensures result >= 0
fn g(x: i64) -> i64 { x.abs().max(0) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "nested/pos should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 2, "{stdout}");
}

/// Wrong pow body must counterexample, not BNM.
#[test]
fn check_rust_pow_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_pow_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x * x
fn sq(x: i64) -> i64 { x.pow(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "wrong pow should fail");
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{v}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong && body must CE.
#[test]
fn check_rust_bool_logic_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_bool_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == (a && b)
fn both(a: bool, b: bool) -> bool { a || b }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Wrong is_multiple_of body must CE.
#[test]
fn check_rust_is_multiple_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_imo_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == (x % 2 == 0)
fn even(x: i64) -> bool { x.is_multiple_of(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// borrow/deref identity encode and verify.
#[test]
fn check_rust_encodes_borrow_deref() {
    let tmp = unique_temp("assura_check_rust_borrow_deref");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x
fn b(x: i64) -> i64 { x.borrow() }

/// @ensures result == x
fn d(x: i64) -> i64 { x.deref() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong PartialOrd method body must CE.
#[test]
fn check_rust_partial_ord_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_po_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == (x > 0)
fn pos(x: i64) -> bool { x.lt(&0) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Wrong abs body must CE.
#[test]
fn check_rust_abs_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_abs_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result >= 0
fn a(x: i64) -> i64 { -x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Wrong is_zero body must CE.
#[test]
fn check_rust_is_zero_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_iz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == (x == 0)
fn z(x: i64) -> bool { x.is_positive() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Wrong default body must CE.
#[test]
fn check_rust_default_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_def_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn z(x: i64) -> i64 { 1 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Wrong clamp body must CE.
#[test]
fn check_rust_clamp_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_clamp_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 10
fn c(x: i64) -> i64 { x.clamp(-5, 5) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Wrong signum body must CE.
#[test]
fn check_rust_signum_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_signum_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == -1 || result == 0 || result == 1
fn s(x: i64) -> i64 { 2 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Multi-let through `as i64` encode (lossless cast).
#[test]
fn check_rust_encodes_multi_let_cast() {
    let tmp = unique_temp("assura_check_rust_multilet_cast");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x
fn f(x: i64) -> i64 { let y = x as i64; y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong abs_diff body must CE.
#[test]
fn check_rust_abs_diff_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_ad_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result >= 0
fn d(x: i64, y: i64) -> i64 { x - y }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Nested wrapping_neg stays body_not_modeled (top-level alone encodes).
#[test]
fn check_rust_nested_wrapping_neg_body_not_modeled() {
    let tmp = unique_temp("assura_check_rust_wrap_bnm");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn w(x: i64) -> i64 { x.wrapping_neg() + 1 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert!(
        v["body_not_modeled"].as_u64().unwrap_or(0) >= 1,
        "nested wrapping_neg must BNM not soft-pass: {stdout}"
    );
    assert!(!out.status.success());
}

/// i64 wrapping_add encodes via synthetic 2^64 modulus (#1010).
#[test]
fn check_rust_encodes_i64_wrapping_add() {
    let tmp = unique_temp("assura_check_rust_i64_wrap");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -9223372036854775808
/// @ensures result <= 9223372036854775807
fn w(x: i64) -> i64 { x.wrapping_add(1) }

/// @ensures result >= -9223372036854775808
/// @ensures result <= 9223372036854775807
fn s(x: i64) -> i64 { x.wrapping_sub(1) }

/// @ensures result >= -9223372036854775808
/// @ensures result <= 9223372036854775807
fn m(x: i64) -> i64 { x.wrapping_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i64 wrapping_add ensures must CE (proves wrap of MAX is live).
#[test]
fn check_rust_i64_wrapping_add_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i64_wrap_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x + 1
fn w(x: i64) -> i64 { x.wrapping_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE on wrap of MAX: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong signed rem_euclid ensures must CE.
#[test]
fn check_rust_signed_rem_euclid_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_signed_rem_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result < 0
fn r(x: i64) -> i64 { x.rem_euclid(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE (rem_euclid always >=0): {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// f32 bodies stay body_not_modeled (not false verified).
#[test]
fn check_rust_f32_body_not_modeled() {
    let tmp = unique_temp("assura_check_rust_f32_bnm");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0.0
fn f(x: f32) -> f32 { x.abs() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert!(
        v["body_not_modeled"].as_u64().unwrap_or(0) >= 1,
        "f32 must BNM: {stdout}"
    );
    assert!(!out.status.success());
}

/// String bodies stay body_not_modeled (not false verified).
#[test]
fn check_rust_string_body_not_modeled() {
    let tmp = unique_temp("assura_check_rust_string_bnm");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result.len() >= 0
fn f(x: &str) -> usize { x.len() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert!(
        v["body_not_modeled"].as_u64().unwrap_or(0) >= 1,
        "string must BNM: {stdout}"
    );
    assert!(!out.status.success());
}

/// to_be/to_le stay body_not_modeled (host-endian; not encoded).
#[test]
fn check_rust_to_be_body_not_modeled() {
    let tmp = unique_temp("assura_check_rust_to_be_bnm");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn t(x: u32) -> u32 { x.to_be() }

/// @ensures result >= 0
fn l(x: u32) -> u32 { x.to_le() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert!(
        v["body_not_modeled"].as_u64().unwrap_or(0) >= 1,
        "to_be/to_le must BNM: {stdout}"
    );
    assert!(!out.status.success());
}

/// checked_/overflowing_* stay body_not_modeled (Option/tuple returns unencoded).
#[test]
fn check_rust_checked_overflowing_body_not_modeled() {
    let tmp = unique_temp("assura_check_rust_checked_bnm");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result.is_some()
fn c(x: i64) -> Option<i64> { x.checked_add(1) }

/// @ensures result.0 >= x
fn o(x: i64) -> (i64, bool) { x.overflowing_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert!(
        v["body_not_modeled"].as_u64().unwrap_or(0) >= 1,
        "checked/overflowing must BNM not soft-pass: {stdout}"
    );
    assert!(!out.status.success());
}

/// Unsigned wrapping_add encodes via mod 2^w (#1010 partial).
#[test]
fn check_rust_encodes_u8_wrapping_add() {
    let tmp = unique_temp("assura_check_rust_u8_wrap");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 255
fn w(x: u8) -> u8 { x.wrapping_add(1) }

/// @ensures result >= 0
/// @ensures result <= 255
fn s(x: u8) -> u8 { x.wrapping_sub(1) }

/// @ensures result >= 0
/// @ensures result <= 255
fn m(x: u8) -> u8 { x.wrapping_mul(3) }

/// @ensures result >= 0
/// @ensures result <= 255
fn n(x: u8) -> u8 { x.wrapping_neg() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// u16 wrapping_add encodes via mod 65536.
#[test]
fn check_rust_encodes_u16_wrapping_add() {
    let tmp = unique_temp("assura_check_rust_u16_wrap");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 65535
fn w(x: u16) -> u16 { x.wrapping_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong u8 wrapping_add ensures must CE.
#[test]
fn check_rust_u8_wrapping_add_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_u8_wrap_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x + 1
fn w(x: u8) -> u8 { x.wrapping_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE on wrap of 255: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// i8 wrapping_add encodes via mod 256 + signed reinterpret (#1010 partial).
#[test]
fn check_rust_encodes_i8_wrapping_add() {
    let tmp = unique_temp("assura_check_rust_i8_wrap");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -128
/// @ensures result <= 127
fn w(x: i8) -> i8 { x.wrapping_add(1) }

/// @ensures result >= -128
/// @ensures result <= 127
fn s(x: i8) -> i8 { x.wrapping_sub(1) }

/// @ensures result >= -128
/// @ensures result <= 127
fn m(x: i8) -> i8 { x.wrapping_mul(2) }

/// @ensures result >= -2147483648
/// @ensures result <= 2147483647
fn w32(x: i32) -> i32 { x.wrapping_add(1) }

/// @ensures result >= -32768
/// @ensures result <= 32767
fn w16s(x: i16) -> i16 { x.wrapping_add(1) }

/// @ensures result >= -2147483648
/// @ensures result <= 2147483647
fn m32(x: i32) -> i32 { x.wrapping_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i8 wrapping_add ensures must CE (proves wrap of 127 is live).
#[test]
fn check_rust_i8_wrapping_add_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i8_wrap_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x + 1
fn w(x: i8) -> i8 { x.wrapping_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE on wrap of 127: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong i32 wrapping_mul ensures must CE (proves double-mod mul is live).
#[test]
fn check_rust_i32_wrapping_mul_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i32_mul_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x * 2
fn m(x: i32) -> i32 { x.wrapping_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE on overflow mul: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong i64 wrapping_mul ensures must CE (synthetic 2^64 modulus live).
#[test]
fn check_rust_i64_wrapping_mul_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i64_mul_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x * 2
fn m(x: i64) -> i64 { x.wrapping_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !out.status.success(),
        "must CE on i64 overflow mul: {stdout}"
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// u16/u32 wrapping_add encode via mod 2^w (#1010 partial).
#[test]
fn check_rust_encodes_u16_u32_wrapping_add() {
    let tmp = unique_temp("assura_check_rust_u16u32_wrap");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 65535
fn w16(x: u16) -> u16 { x.wrapping_add(1) }

/// @ensures result >= 0
/// @ensures result <= 4294967295
fn w32(x: u32) -> u32 { x.wrapping_add(1) }

/// @ensures result >= 0
/// @ensures result <= 4294967295
fn m32(x: u32) -> u32 { x.wrapping_mul(2) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Signed wrapping_shl by const encodes via mul+double-mod+reinterpret.
#[test]
fn check_rust_encodes_signed_wrapping_shl() {
    let tmp = unique_temp("assura_check_rust_signed_shl");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -128
/// @ensures result <= 127
fn s(x: i8) -> i8 { x.wrapping_shl(1) }

/// @ensures result >= -9223372036854775808
/// @ensures result <= 9223372036854775807
fn l(x: i64) -> i64 { x.wrapping_shl(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong signed wrapping_shl ensures must CE (proves wrap is live).
#[test]
fn check_rust_signed_wrapping_shl_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_signed_shl_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x * 2
fn s(x: i8) -> i8 { x.wrapping_shl(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE on i8 shl wrap: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed variable wrapping_shl/shr encode via case-sum (#1145).
#[test]
fn check_rust_encodes_i8_variable_wrapping_shl() {
    let tmp = unique_temp("assura_check_rust_i8_var_shl");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -128
/// @ensures result <= 127
fn s(x: i8, n: u32) -> i8 { x.wrapping_shl(n) }

/// @ensures result >= -128
/// @ensures result <= 127
fn r(x: i8, n: u32) -> i8 { x.wrapping_shr(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong signed variable wrapping_shl ensures must CE.
#[test]
fn check_rust_i8_variable_wrapping_shl_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_i8_var_shl_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: i8, n: u32) -> i8 { x.wrapping_shl(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Variable wrapping_shl/shr for u8 encodes via case-sum (#1145).
#[test]
fn check_rust_encodes_u8_variable_wrapping_shl() {
    let tmp = unique_temp("assura_check_rust_var_shl");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 255
fn s(x: u8, n: u32) -> u8 { x.wrapping_shl(n) }

/// @ensures result >= 0
/// @ensures result <= 255
fn r(x: u8, n: u32) -> u8 { x.wrapping_shr(n) }

/// @ensures result >= 0
/// @ensures result <= 4294967295
fn s32(x: u32, n: u32) -> u32 { x.wrapping_shl(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong variable wrapping_shl ensures must CE.
#[test]
fn check_rust_u8_variable_wrapping_shl_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_var_shl_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: u8, n: u32) -> u8 { x.wrapping_shl(n) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed wrapping_shr encodes via floor div by 2^k.
#[test]
fn check_rust_encodes_signed_wrapping_shr() {
    let tmp = unique_temp("assura_check_rust_signed_shr");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -128
/// @ensures result <= 127
fn s(x: i8) -> i8 { x.wrapping_shr(1) }

/// @ensures result >= -9223372036854775808
/// @ensures result <= 9223372036854775807
fn l(x: i64) -> i64 { x.wrapping_shr(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong signed wrapping_shr ensures must CE (proves floor-div is live).
#[test]
fn check_rust_signed_wrapping_shr_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_signed_shr_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: i8) -> i8 { x.wrapping_shr(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed rotate_left encodes via bit-pattern map + reinterpret.
#[test]
fn check_rust_encodes_signed_rotate() {
    let tmp = unique_temp("assura_check_rust_signed_rot");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= -128
/// @ensures result <= 127
fn r(x: i8) -> i8 { x.rotate_left(1) }

/// @ensures result >= -128
/// @ensures result <= 127
fn rr(x: i8) -> i8 { x.rotate_right(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong signed rotate_left ensures must CE (bit rotate is live).
#[test]
fn check_rust_signed_rotate_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_signed_rot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x * 2
fn r(x: i8) -> i8 { x.rotate_left(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE on rotate: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Unsigned wrapping_shl by const encodes via mul+mod (#1010 partial).
#[test]
fn check_rust_encodes_u8_wrapping_shl() {
    let tmp = unique_temp("assura_check_rust_u8_shl");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 255
fn s(x: u8) -> u8 { x.wrapping_shl(1) }

/// @ensures result == x
fn id(x: u8) -> u8 { x.wrapping_shl(8) }

/// @ensures result >= 0
/// @ensures result <= 255
fn r(x: u8) -> u8 { x.wrapping_shr(1) }

/// @ensures result >= 0
/// @ensures result <= 255
fn rot(x: u8) -> u8 { x.rotate_left(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Const bit-count peeps (partial #1034 family) + shift/rotate-by-0 identity.
#[test]
fn check_rust_encodes_const_bit_peeps() {
    let tmp = unique_temp("assura_check_rust_bit_peeps");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 2
fn ones(x: u32) -> u32 { 12u32.count_ones() }

/// @ensures result == 3
fn to(x: u8) -> u32 { 7u8.trailing_ones() }

/// @ensures result == 4
fn lo(x: u32) -> u32 { 0xF000_0000u32.leading_ones() }

/// @ensures result == 30
fn zeros(x: u32) -> u32 { 12u32.count_zeros() }

/// @ensures result == 2
fn tz(x: u32) -> u32 { 12u32.trailing_zeros() }

/// @ensures result == 28
fn lz(x: u32) -> u32 { 8u32.leading_zeros() }

/// @ensures result == x
fn id_shl(x: i64) -> i64 { x.wrapping_shl(0) }

/// @ensures result == x
fn id_rot(x: i64) -> i64 { x.rotate_left(0) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Const bitops fold and verify.
#[test]
fn check_rust_encodes_const_bitops() {
    let tmp = unique_temp("assura_check_rust_bitops");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 8
fn a(x: u32) -> u32 { 12u32 & 10u32 }

/// @ensures result == 12
fn s(x: u32) -> u32 { 3u32 << 2 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong const bitops ensures must CE.
#[test]
fn check_rust_const_bitops_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_bitops_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn a(x: u32) -> u32 { 12u32 & 10u32 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Const reverse_bits / swap_bytes / zero trailing_zeros (typed).
#[test]
fn check_rust_encodes_const_reverse_swap() {
    let tmp = unique_temp("assura_check_rust_rev_swap");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 128
fn rev(x: u8) -> u8 { 1u8.reverse_bits() }

/// @ensures result == 13330
fn sw(x: u16) -> u16 { 0x1234u16.swap_bytes() }

/// @ensures result == 8
fn ztz(x: u8) -> u32 { 0u8.trailing_zeros() }

/// @ensures result == 3
fn ig(x: u32) -> u32 { 8u32.ilog2() }

/// @ensures result == 4
fn np(x: u32) -> u32 { 3u32.next_power_of_two() }

/// @ensures result == 0
fn wnp(x: u8) -> u8 { 200u8.wrapping_next_power_of_two() }

/// @ensures result == 3
fn sq(x: u32) -> u32 { 10u32.isqrt() }

/// @ensures result == 2
fn l10(x: u32) -> u32 { 100u32.ilog10() }

/// @ensures result >= 0
fn ua(x: i64) -> u64 { x.unsigned_abs() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong isqrt ensures must CE.
#[test]
fn check_rust_isqrt_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_isqrt_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn s(x: u32) -> u32 { 10u32.isqrt() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// div_ceil with positive const divisor for unsigned params.
#[test]
fn check_rust_encodes_div_ceil() {
    let tmp = unique_temp("assura_check_rust_div_ceil");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 255
fn d(x: u8) -> u8 { x.div_ceil(3) }

/// @ensures result == 4
fn c(x: u32) -> u32 { 10u32.div_ceil(3) }

/// @ensures result >= 0
/// @ensures result < 3
fn r(x: u8) -> u8 { x.rem_euclid(3) }

/// @ensures result >= 0
/// @ensures result <= 255
fn de(x: u8) -> u8 { x.div_euclid(3) }

/// @ensures result == 12
fn nmo(x: u8) -> u8 { 10u8.next_multiple_of(4) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong div_ceil ensures must CE.
#[test]
fn check_rust_div_ceil_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_div_ceil_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn c(x: u32) -> u32 { 10u32.div_ceil(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Signed rem_euclid/div_euclid with positive const encode.
#[test]
fn check_rust_encodes_signed_rem_euclid() {
    let tmp = unique_temp("assura_check_rust_signed_rem");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result < 3
fn r(x: i64) -> i64 { x.rem_euclid(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong rem_euclid ensures must CE.
#[test]
fn check_rust_rem_euclid_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_rem_euclid_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn r(x: u8) -> u8 { 10u8.rem_euclid(3) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong next_multiple_of ensures must CE.
#[test]
fn check_rust_next_multiple_of_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_nmo_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 10
fn nmo(x: u8) -> u8 { 10u8.next_multiple_of(4) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// midpoint encodes as floor((a+b)/2).
#[test]
fn check_rust_encodes_midpoint() {
    let tmp = unique_temp("assura_check_rust_midpoint");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 5
fn m(x: i64) -> i64 { 4i64.midpoint(6) }

/// @ensures result == x
fn s(x: i64) -> i64 { x.midpoint(x) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong midpoint ensures must CE.
#[test]
fn check_rust_midpoint_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_mid_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn m(x: i64) -> i64 { 4i64.midpoint(6) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Top-level wrapping_neg encodes (MIN stays MIN); nested still BNM.
#[test]
fn check_rust_encodes_wrapping_neg() {
    let tmp = unique_temp("assura_check_rust_wneg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn n(x: i64) -> i64 { x.wrapping_neg() }

/// @ensures result == x
fn nest(x: i64) -> i64 { x.wrapping_neg() + x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert!(
        v["body_not_modeled"].as_u64().unwrap_or(0) >= 1,
        "nested wrapping_neg must BNM: {stdout}"
    );
    let results = v["results"].as_array().expect("results");
    let statuses: Vec<_> = results
        .iter()
        .map(|r| {
            (
                r["item"].as_str().unwrap_or(""),
                r["status"].as_str().unwrap_or(""),
            )
        })
        .collect();
    assert!(
        statuses
            .iter()
            .any(|(i, s)| *i == "n" && *s != "body_not_modeled"),
        "top-level wrapping_neg should encode, got {statuses:?}: {stdout}"
    );
}

/// wrapping_add(0) / wrapping_mul(1) identity peeps encode.
#[test]
fn check_rust_encodes_wrapping_identity_peeps() {
    let tmp = unique_temp("assura_check_rust_wpeep");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x
fn a(x: i64) -> i64 { x.wrapping_add(0) }

/// @ensures result == x
fn m(x: i64) -> i64 { x.wrapping_mul(1) }

/// @ensures result == 0
fn z(x: i64) -> i64 { x.wrapping_mul(0) }

/// @ensures result == 0
fn s(x: i64) -> i64 { x.wrapping_sub(x) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong wrapping_neg ensures must CE (proves multi-block encode is live).
#[test]
fn check_rust_wrapping_neg_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_wneg_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn n(x: i64) -> i64 { x.wrapping_neg() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must fail: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode, not BNM: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Literal /0 and is_multiple_of(0) must be body_not_modeled (panic paths).
#[test]
fn check_rust_div_zero_body_not_modeled() {
    let tmp = unique_temp("assura_check_rust_div0_bnm");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0
fn d(x: i64) -> i64 { x / 0 }

/// @ensures result == true
fn m(x: i64) -> bool { x.is_multiple_of(0) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert!(
        v["body_not_modeled"].as_u64().unwrap_or(0) >= 2,
        "div0 / is_multiple_of(0) must BNM: {stdout}"
    );
    assert!(!out.status.success());
}

/// Wrong pot ensures with clone peel must CE (bounds peel live).
#[test]
fn check_rust_pot_clone_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_pot_clone_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == true
fn p(x: u8) -> bool { x.clone().is_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Nested non-path is_power_of_two stays body_not_modeled (no param bounds).
#[test]
fn check_rust_is_power_of_two_nested_bnm() {
    let tmp = unique_temp("assura_check_rust_pot_bnm");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn p(x: i64) -> bool { (x + 1).is_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert!(
        v["body_not_modeled"].as_u64().unwrap_or(0) >= 1,
        "nested pot must BNM: {stdout}"
    );
    assert!(!out.status.success());
}

/// u8/u32/i64 is_power_of_two encodes via pot enum (partial #1034).
/// Identity peels keep path-param bounds (clone/into).
#[test]
fn check_rust_encodes_u8_is_power_of_two() {
    let tmp = unique_temp("assura_check_rust_pot_u8");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true || result == false
fn p(x: u8) -> bool { x.is_power_of_two() }

/// @ensures result == true || result == false
fn q(x: u32) -> bool { x.is_power_of_two() }

/// @ensures result == true || result == false
fn r(x: i64) -> bool { x.is_power_of_two() }

/// @ensures result == true || result == false
fn c(x: i64) -> bool { x.clone().is_power_of_two() }

/// @ensures result == true || result == false
fn i(x: i64) -> bool { x.into().is_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong pot ensures must CE (proves enum encode is live).
#[test]
fn check_rust_u8_is_power_of_two_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_pot_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == true
fn p(x: u8) -> bool { x.is_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE when x=3: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Const is_power_of_two peeps (partial #1034 / #1089).
#[test]
fn check_rust_encodes_const_is_power_of_two() {
    let tmp = unique_temp("assura_check_rust_pot_const");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == true
fn t(x: i64) -> bool { 8i64.is_power_of_two() }

/// @ensures result == false
fn f(x: i64) -> bool { 3i64.is_power_of_two() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Unsigned path-param count_ones/count_zeros encode via bit-sum (div/mod).
#[test]
fn check_rust_encodes_u8_count_ones() {
    let tmp = unique_temp("assura_check_rust_count_ones");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 8
fn c(x: u8) -> u32 { x.count_ones() }

/// @ensures result >= 0
/// @ensures result <= 8
fn z(x: u8) -> u32 { x.count_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Unsigned path-param trailing_zeros/leading_zeros encode via bit products.
#[test]
fn check_rust_encodes_u8_trailing_zeros() {
    let tmp = unique_temp("assura_check_rust_tz");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 8
fn t(x: u8) -> u32 { x.trailing_zeros() }

/// @ensures result >= 0
/// @ensures result <= 8
fn l(x: u8) -> u32 { x.leading_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong trailing_zeros ensures must CE (proves first-set-bit encode is live).
#[test]
fn check_rust_u8_trailing_zeros_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_tz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn t(x: u8) -> u32 { x.trailing_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Unsigned path-param reverse_bits encodes via bit reverse sum.
#[test]
fn check_rust_encodes_u8_reverse_bits() {
    let tmp = unique_temp("assura_check_rust_rev");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 255
fn r(x: u8) -> u8 { x.reverse_bits() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Unsigned path-param swap_bytes encodes via byte reverse.
#[test]
fn check_rust_encodes_u16_swap_bytes() {
    let tmp = unique_temp("assura_check_rust_sw");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
/// @ensures result <= 65535
fn s(x: u16) -> u16 { x.swap_bytes() }

/// @ensures result >= 0
/// @ensures result <= 255
fn u(x: u8) -> u8 { x.swap_bytes() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong swap_bytes ensures must CE (proves byte reverse is live).
#[test]
fn check_rust_u16_swap_bytes_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_sw_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn s(x: u16) -> u16 { x.swap_bytes() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong reverse_bits ensures must CE.
#[test]
fn check_rust_u8_reverse_bits_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_rev_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == x
fn r(x: u8) -> u8 { x.reverse_bits() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong leading_zeros ensures must CE.
#[test]
fn check_rust_u8_leading_zeros_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_lz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn l(x: u8) -> u32 { x.leading_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong count_zeros ensures must CE.
#[test]
fn check_rust_u8_count_zeros_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_cz_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn z(x: u8) -> u32 { x.count_zeros() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong count_ones ensures must CE (proves bit-sum is live).
#[test]
fn check_rust_u8_count_ones_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_count_ones_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn c(x: u8) -> u32 { x.count_ones() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must CE: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// Wrong nested signum ensures must CE (proves encode is live, #1032).
#[test]
fn check_rust_nested_signum_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_ns_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 0
fn s(x: i64) -> i64 { x.signum() + 1 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must fail: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode, not BNM: {stdout}");
    assert!(
        v["errors"].as_u64().unwrap_or(0) >= 1,
        "expected counterexample/errors: {v}"
    );
    let status = v["results"][0]["status"].as_str().unwrap_or("");
    assert_eq!(status, "error", "expected error status from CE: {v}");
}

/// into() identity and bool true path encode.
#[test]
fn check_rust_encodes_into_true() {
    let tmp = unique_temp("assura_check_rust_into_true");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == x
fn i(x: i64) -> i64 { x.into() }

/// @ensures result == true
fn t(a: bool) -> bool { true }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong i64::MAX body must CE.
#[test]
fn check_rust_max_const_wrong_body_ce() {
    let tmp = unique_temp("assura_check_rust_max_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result == 9223372036854775807
fn m(x: i64) -> i64 { i64::MIN }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// saturating_neg encodes (body model present).
#[test]
fn check_rust_encodes_saturating_neg() {
    let tmp = unique_temp("assura_check_rust_sat_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == 0 || result != 0
fn n(x: i64) -> i64 { x.saturating_neg() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// saturating_abs encodes (MIN → MAX via abs then min with MAX).
#[test]
fn check_rust_encodes_saturating_abs() {
    let tmp = unique_temp("assura_check_rust_sat_abs");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn a(x: i64) -> i64 { x.saturating_abs() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Wrong saturating_abs ensures must CE (proves encode is live).
#[test]
fn check_rust_saturating_abs_wrong_ce() {
    let tmp = unique_temp("assura_check_rust_sat_abs_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result < 0
fn a(x: i64) -> i64 { x.saturating_abs() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "must fail: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "must encode, not BNM: {stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// abs_diff().is_positive() encodes and verifies for unequal params.
#[test]
fn check_rust_encodes_abs_diff_positive() {
    let tmp = unique_temp("assura_check_rust_ad_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result == (x != y)
fn d(x: i64, y: i64) -> bool { x.abs_diff(y).is_positive() }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    // abs_diff never overflows; is_positive iff x != y for all i64
    assert!(out.status.success(), "{stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
}

/// Nested if/else-if encodes multi-block IR and can CE wrong branches.
#[test]
fn check_rust_encodes_nested_if_body() {
    let tmp = unique_temp("assura_check_rust_body_nested_if");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("ok.rs"),
        r#"
/// @ensures result >= 0
fn nest(x: i64) -> i64 {
    if x > 10 { x } else { if x > 0 { x } else { 0 } }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("ok.rs").to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "nested if should pass: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 1, "{stdout}");

    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @ensures result >= 0
fn nest(x: i64) -> i64 {
    if x > 10 { x } else { if x > 0 { x } else { -1 } }
}
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "wrong nested else should CE");
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["body_not_modeled"], 0);
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1);
}

/// Bool comparison bodies encode; wrong wrapping ensures CEs (encode live).
#[test]
fn check_rust_bool_body_and_bnm_unmodeled() {
    let tmp = unique_temp("assura_check_rust_bool_bnm");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bool.rs"),
        r#"
/// @ensures result == true || result == false
fn is_pos(x: i64) -> bool { x > 0 }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args([
            "check-rust",
            "--json",
            tmp.join("bool.rs").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Bool cmp encode is enough to avoid BNM; ensures may verify or be soft.
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(
        v["body_not_modeled"], 0,
        "bool comparison should encode body: {stdout}"
    );

    // wrapping_add encodes; wrong ensures (result >= x fails at MAX wrap) must CE.
    std::fs::write(
        tmp.join("wrap.rs"),
        r#"
/// @ensures result >= x
fn add1(x: i64) -> i64 { x.wrapping_add(1) }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args([
            "check-rust",
            "--json",
            tmp.join("wrap.rs").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "wrong wrapping ensures must fail exit: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(
        v["body_not_modeled"], 0,
        "wrapping_add must encode (not BNM): {v}"
    );
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{v}");
}

/// #975: wrong identity body vs ensures x+1 must CE (not silent verified / BNM).
#[test]
fn check_rust_encodes_identity_body_counterexample() {
    let tmp = unique_temp("assura_check_rust_body_ce");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("bad.rs"),
        r#"
/// @requires x > 0
/// @ensures result == x + 1
fn bad(x: i64) -> i64 { x }
"#,
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", tmp.join("bad.rs").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "wrong body should fail: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("check-rust --json");
    assert_eq!(v["body_not_modeled"], 0, "body was encoded, not BNM: {v}");
    assert!(
        v["errors"].as_u64().unwrap_or(0) >= 1,
        "expected counterexample/errors: {v}"
    );
    let status = v["results"][0]["status"].as_str().unwrap_or("");
    assert_eq!(status, "error", "expected error status from CE: {v}");
}

/// test-gen -o write failure under --json must be parseable.
#[test]
fn test_gen_write_fail_json() {
    let tmp = unique_temp("assura_test_gen_write");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("c.assura"),
        "contract C { requires { true } ensures { true } fn f(x: Int) -> Int }\n",
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args([
            "test-gen",
            tmp.join("c.assura").to_str().unwrap(),
            "-o",
            "/no/write/out.rs",
            "--json",
        ])
        .output()
        .expect("failed to run test-gen -o --json");
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("test-gen write fail --json must be JSON");
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"], "write_failed");
}

/// #977: clap missing required args under global --json must be JSON.
#[test]
fn clap_missing_arg_json() {
    let out = Command::new(assura_bin())
        .args(["fmt", "--json"])
        .output()
        .expect("failed to run fmt --json without file");
    assert_eq!(
        out.status.code(),
        Some(2),
        "missing required arg should exit 2: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("clap missing arg under --json must be JSON");
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"], "cli_error");
    assert!(
        v["kind"].as_str().unwrap_or("").contains("Missing")
            || v["message"].as_str().unwrap_or("").contains("required"),
        "expected missing-arg kind/message, got {v}"
    );
}

/// Global `--json` with invalid `--format` must emit JSON (coverage/audit/diff).
#[test]
fn coverage_invalid_format_json() {
    let out = Command::new(assura_bin())
        .args(["coverage", ".", "--format", "xml", "--json"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run coverage --format xml --json");
    assert_eq!(out.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("invalid --format under --json must be JSON");
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"], "invalid_format");
    assert_eq!(v["format"], "xml");
}

/// `check --watch --json` on a missing path must emit JSON, not bare stderr.
#[test]
fn check_watch_missing_path_json() {
    let out = Command::new(assura_bin())
        .args(["check", "/no/such/watch/path.assura", "--watch", "--json"])
        .output()
        .expect("failed to run assura check --watch --json");
    assert_eq!(
        out.status.code(),
        Some(2),
        "missing path should exit 2: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("watch missing path --json must be JSON");
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"], "cannot_resolve_path");
    assert_eq!(v["watch"], true);
}

#[test]
fn check_showcase_only_filters_by_header() {
    let tmp = unique_temp("assura_showcase_only");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("showcase.assura"),
        "// SHOWCASE (must-pass)\ncontract A { input(x: Int) requires { x >= 0 } ensures { x >= 0 } }\n",
    )
    .unwrap();
    std::fs::write(
        tmp.join("other.assura"),
        "contract B { input(x: Int) requires { x >= 0 } ensures { x >= 0 } }\n",
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["check", tmp.to_str().unwrap(), "--showcase-only"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run showcase-only check");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let combined = format!("{stdout}{stderr}");
    assert!(
        out.status.success(),
        "showcase-only should succeed: {combined}"
    );
    assert!(
        combined.contains("1 module") || combined.contains("modules\": 1"),
        "should check only the SHOWCASE file: {combined}"
    );
    assert!(
        !combined.contains("other") || combined.contains("showcase"),
        "should include showcase module: {combined}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// --showcase-only with zero matches is vacuous (not silent "all green").
#[test]
fn check_showcase_only_vacuous_when_none_match() {
    let tmp = unique_temp("assura_showcase_vacuous");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("plain.assura"),
        "contract C { requires { true } ensures { true } fn f(x: Int) -> Int }\n",
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check", tmp.to_str().unwrap(), "--showcase-only", "--json"])
        .current_dir(workspace_root())
        .output()
        .expect("showcase vacuous");
    assert!(out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["modules"], 0);
    assert_eq!(v["vacuous"], true);
    assert_eq!(v["showcase_only"], true);
    assert!(
        v["vacuous_reason"]
            .as_str()
            .unwrap_or("")
            .contains("SHOWCASE"),
        "{v}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn diff_global_json_flag_emits_json() {
    let tmp = unique_temp("assura_diff_global_json");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let a = tmp.join("a.assura");
    let b = tmp.join("b.assura");
    std::fs::write(
        &a,
        "contract T { input(x: Int) requires { x >= 0 } ensures { x >= 0 } }\n",
    )
    .unwrap();
    std::fs::write(
        &b,
        "contract T { input(x: Int) requires { x > 0 } ensures { x >= 0 } }\n",
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["diff", a.to_str().unwrap(), b.to_str().unwrap(), "--json"])
        .current_dir(workspace_root())
        .output()
        .expect("diff --json");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim_start().starts_with('{'),
        "global --json should yield JSON, got: {stdout}"
    );
    assert!(
        stdout.contains("\"changes\"") || stdout.contains("identical"),
        "JSON should have changes/identical: {stdout}"
    );
    // Should not print human "Requires:" banner as sole format
    assert!(
        !stdout.starts_with("T:"),
        "must not be human-only format: {stdout}"
    );

    let id = Command::new(assura_bin())
        .args(["diff", a.to_str().unwrap(), a.to_str().unwrap(), "--json"])
        .current_dir(workspace_root())
        .output()
        .expect("diff identical --json");
    let id_out = String::from_utf8_lossy(&id.stdout);
    assert!(
        id_out.trim_start().starts_with('{'),
        "identical diff --json should be JSON: {id_out}"
    );
    assert!(
        !id_out.contains("No structural differences"),
        "must not print human identical message: {id_out}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn project_check_fails_on_missing_import() {
    let tmp = unique_temp("assura_missing_import_proj");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("contracts")).unwrap();
    std::fs::write(
        tmp.join("assura.toml"),
        "[package]\nname = \"t\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        tmp.join("contracts/lib.assura"),
        "module lib;\ncontract C { requires { true } ensures { true } }\n",
    )
    .unwrap();
    std::fs::write(
        tmp.join("contracts/use.assura"),
        "module use_mod;\nimport missing_mod;\ncontract U { requires { true } ensures { true } }\n",
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["check", tmp.to_str().unwrap()])
        .current_dir(workspace_root())
        .output()
        .expect("check project");
    assert!(
        !out.status.success(),
        "missing import must fail project check: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("missing_mod") || stderr.contains("resolution"),
        "stderr should mention missing import: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn single_file_missing_import_is_a02010_not_unused() {
    let tmp = unique_temp("assura_missing_import_single");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("x.assura");
    std::fs::write(
        &path,
        "module m;\nimport missing_mod;\ncontract C { requires { true } ensures { true } }\n",
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["check", path.to_str().unwrap(), "--json"])
        .current_dir(workspace_root())
        .output()
        .expect("check single");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("A02010"), "expected A02010, got {stdout}");
    assert!(
        !stdout.contains("A02007"),
        "must not mislabel as unused import: {stdout}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn check_rejects_empty_requires_body() {
    let tmp = unique_temp("assura_empty_requires");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("e.assura");
    std::fs::write(
        &path,
        "contract E {\n  input(x: Int)\n  requires { }\n  ensures { true }\n}\n",
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args(["check", path.to_str().unwrap(), "--json"])
        .current_dir(workspace_root())
        .output()
        .expect("check");
    assert!(!out.status.success(), "empty requires must fail");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("A03006"),
        "expected A03006 for empty requires: {stdout}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn diff_rejects_invalid_format() {
    let tmp = unique_temp("assura_diff_bad_format");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let a = tmp.join("a.assura");
    std::fs::write(
        &a,
        "contract T { input(x: Int) requires { x >= 0 } ensures { x >= 0 } }\n",
    )
    .unwrap();
    let out = Command::new(assura_bin())
        .args([
            "diff",
            a.to_str().unwrap(),
            a.to_str().unwrap(),
            "--format",
            "xml",
        ])
        .current_dir(workspace_root())
        .output()
        .expect("diff");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("invalid --format") || stderr.contains("expected human"),
        "stderr: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn explain_json_and_doctor_json() {
    let out = Command::new(assura_bin())
        .args(["explain", "A03001", "--json"])
        .current_dir(workspace_root())
        .output()
        .expect("explain");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim_start().starts_with('{'), "json: {stdout}");
    assert!(stdout.contains("\"code\"") && stdout.contains("A03001"));

    let doc = Command::new(assura_bin())
        .args(["doctor", "--json"])
        .current_dir(workspace_root())
        .output()
        .expect("doctor");
    // may fail if z3 missing in env; still expect JSON
    let dstdout = String::from_utf8_lossy(&doc.stdout);
    assert!(
        dstdout.trim_start().starts_with('{'),
        "doctor --json should emit JSON: {dstdout}"
    );
    assert!(dstdout.contains("\"checks\"") || dstdout.contains("assura"));
}

#[test]
fn fmt_accepts_directory() {
    let tmp = unique_temp("assura_fmt_dir");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    std::fs::write(
        tmp.join("src/a.assura"),
        "contract A{input(x:Int)requires{x>=0}ensures{x>=0}}\n",
    )
    .unwrap();
    std::fs::write(
        tmp.join("src/b.assura"),
        "contract B{input(y:Int)requires{y>0}ensures{y>0}}\n",
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["fmt", tmp.to_str().unwrap()])
        .current_dir(workspace_root())
        .output()
        .expect("fmt dir");
    assert!(
        out.status.success(),
        "fmt dir should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let a = std::fs::read_to_string(tmp.join("src/a.assura")).unwrap();
    assert!(
        a.contains('\n') && a.lines().count() > 1,
        "expected expanded a.assura: {a:?}"
    );

    let check = Command::new(assura_bin())
        .args(["fmt", tmp.to_str().unwrap(), "--check"])
        .current_dir(workspace_root())
        .output()
        .expect("fmt --check dir");
    assert!(
        check.status.success(),
        "fmt --check after fmt should pass: {}",
        String::from_utf8_lossy(&check.stderr)
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

/// --dump-smt mkdir failure under --json must be parseable.
#[test]
fn check_dump_smt_mkdir_fail_json() {
    let out = Command::new(assura_bin())
        .args([
            "check",
            "demos/heartbleed.assura",
            "--dump-smt",
            "/no/write/path",
            "--json",
        ])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run check --dump-smt --json");
    assert_eq!(out.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("dump-smt mkdir fail --json must be JSON");
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"], "dump_smt_mkdir_failed");
}
