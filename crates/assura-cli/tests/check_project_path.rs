//! Directory check must name the file that was read, not a path built
//! from the declared module. This runs the `assura` binary so a wrong
//! lookup inside `run_check_project` fails the test.

mod common;

use common::{assura_bin, unique_temp};
use std::process::Command;

#[test]
fn project_check_reports_hb_assura_not_the_module_path() {
    let dir = unique_temp("assura_project_path");
    let file = dir.join("hb.assura");
    std::fs::write(
        &file,
        "\
module tls.heartbeat;

type T {
  f: (,)
}

contract C {
  input(x: T)
  ensures { true }
}
",
    )
    .unwrap();

    let out = Command::new(assura_bin())
        .args(["check", dir.to_str().unwrap()])
        .output()
        .expect("failed to run assura check");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "type error must fail the check: {stderr}"
    );
    assert!(
        stderr.contains("hb.assura"),
        "stderr must name the file that was read: {stderr}"
    );
    assert!(
        !stderr.contains("tls/heartbeat.assura") && !stderr.contains("tls\\heartbeat.assura"),
        "stderr must not invent a path from the module name: {stderr}"
    );

    let json = Command::new(assura_bin())
        .args(["check", "--json", dir.to_str().unwrap()])
        .output()
        .expect("failed to run assura check --json");
    let stdout = String::from_utf8_lossy(&json.stdout);
    let report: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("json: {e}\n{stdout}"));
    let files: Vec<&str> = report["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("diagnostics: {stdout}"))
        .iter()
        .filter_map(|d| d["file"].as_str())
        .collect();
    assert!(
        files.iter().any(|f| f.ends_with("hb.assura")),
        "json diagnostics must use hb.assura: {files:?}"
    );
    assert!(
        files
            .iter()
            .all(|f| !f.contains("tls/heartbeat.assura") && !f.contains("tls\\heartbeat.assura")),
        "json diagnostics must not invent a module path: {files:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
