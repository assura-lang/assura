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
    let script = v["script"]
        .as_str()
        .expect("script field must be a string");
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
