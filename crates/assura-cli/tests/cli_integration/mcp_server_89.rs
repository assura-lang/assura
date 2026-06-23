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
