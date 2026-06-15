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
    let tmp = std::env::temp_dir().join("assura_audit_human");
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
    let tmp = std::env::temp_dir().join("assura_audit_json");
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
    let tmp = std::env::temp_dir().join("assura_audit_no_cargo");
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
    let tmp = std::env::temp_dir().join("assura_audit_empty_src");
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
    let tmp = std::env::temp_dir().join("assura_audit_max_fn");
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
    let tmp = std::env::temp_dir().join("assura_audit_medium");
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
// Issue #96: New CLI command integration tests
// =======================================================================

#[test]
fn doctor_exits_zero_and_shows_deps() {
    let out = Command::new(assura_bin())
        .arg("doctor")
        .output()
        .expect("failed to run assura doctor");
    assert!(
        out.status.success(),
        "doctor should exit 0 when rustc/z3 present: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("rustc"), "doctor should check rustc");
    assert!(stdout.contains("z3"), "doctor should check z3");
    assert!(stdout.contains("cargo"), "doctor should check cargo");
}

#[test]
fn coverage_json_output_is_valid() {
    let tmp = std::env::temp_dir().join("assura_coverage_json");
    let _ = std::fs::remove_dir_all(&tmp);
    create_test_crate(&tmp);
    std::fs::create_dir_all(tmp.join("contracts")).unwrap();

    let out = Command::new(assura_bin())
        .args(["coverage", tmp.to_str().unwrap(), "--format", "json"])
        .output()
        .expect("failed to run assura coverage");
    assert!(
        out.status.success(),
        "coverage should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{stdout}"));
    assert!(
        parsed.get("coverage_percent").is_some(),
        "JSON should have coverage_percent"
    );
    assert!(
        parsed.get("total_functions").is_some(),
        "JSON should have total_functions"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn coverage_min_coverage_fails_when_below() {
    let tmp = std::env::temp_dir().join("assura_coverage_min");
    let _ = std::fs::remove_dir_all(&tmp);
    create_test_crate(&tmp);
    std::fs::create_dir_all(tmp.join("contracts")).unwrap();

    let out = Command::new(assura_bin())
        .args(["coverage", tmp.to_str().unwrap(), "--min-coverage", "100.0"])
        .output()
        .expect("failed to run assura coverage");
    // With no .assura contracts, coverage is 0%, so --min-coverage 100 should fail
    assert!(
        !out.status.success(),
        "coverage should fail when below --min-coverage"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn completions_zsh_outputs_valid_script() {
    let out = Command::new(assura_bin())
        .args(["completions", "zsh"])
        .output()
        .expect("failed to run assura completions zsh");
    assert!(
        out.status.success(),
        "completions zsh should exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("compdef") || stdout.contains("_assura"),
        "zsh completions should contain compdef or _assura"
    );
}

#[test]
fn completions_bash_outputs_valid_script() {
    let out = Command::new(assura_bin())
        .args(["completions", "bash"])
        .output()
        .expect("failed to run assura completions bash");
    assert!(
        out.status.success(),
        "completions bash should exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("complete") || stdout.contains("_assura"),
        "bash completions should contain complete"
    );
}

#[test]
fn explain_known_code_exits_zero() {
    let out = Command::new(assura_bin())
        .args(["explain", "A01001"])
        .output()
        .expect("failed to run assura explain");
    assert!(
        out.status.success(),
        "explain A01001 should exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("A01001"),
        "explain output should contain the error code: {stdout}"
    );
}

#[test]
fn explain_unknown_code_exits_nonzero() {
    let out = Command::new(assura_bin())
        .args(["explain", "XXXXX"])
        .output()
        .expect("failed to run assura explain");
    assert!(!out.status.success(), "explain XXXXX should exit non-zero");
}

#[test]
fn agent_instructions_outputs_reference() {
    let out = Command::new(assura_bin())
        .arg("agent-instructions")
        .output()
        .expect("failed to run assura agent-instructions");
    assert!(
        out.status.success(),
        "agent-instructions should exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Type Mapping") || stdout.contains("Assura"),
        "should contain reference content: {stdout}"
    );
    assert!(
        stdout.contains("CLI Commands"),
        "should contain CLI commands section: {stdout}"
    );
}

#[test]
fn diff_identical_files_exits_zero() {
    let out = Command::new(assura_bin())
        .args([
            "diff",
            "demos/libwebp-huffman.assura",
            "demos/libwebp-huffman.assura",
        ])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura diff");
    assert!(
        out.status.success(),
        "diff of identical files should exit 0"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("No structural differences"),
        "should report no differences: {stdout}"
    );
}

#[test]
fn diff_different_files_exits_one() {
    let out = Command::new(assura_bin())
        .args([
            "diff",
            "demos/libwebp-huffman.assura",
            "demos/zlib-inflate.assura",
        ])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura diff");
    assert!(
        !out.status.success(),
        "diff of different files should exit 1"
    );
}

#[test]
fn diff_json_output_is_valid() {
    let out = Command::new(assura_bin())
        .args([
            "diff",
            "demos/libwebp-huffman.assura",
            "demos/zlib-inflate.assura",
            "--format",
            "json",
        ])
        .current_dir(workspace_root())
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

#[test]
fn fmt_check_mode_exits_cleanly() {
    let out = Command::new(assura_bin())
        .args(["fmt", "tests/fixtures/test_basic.assura", "--check"])
        .current_dir(workspace_root())
        .output()
        .expect("failed to run assura fmt --check");
    // May exit 0 (already formatted) or 1 (needs formatting); must not crash
    assert!(
        out.status.code().is_some(),
        "fmt --check should exit with a code, not crash"
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

/// Helper: send MCP JSON-RPC messages and return stdout lines.
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
    let tools_line = &lines[1]; // second line is tools/list response
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
