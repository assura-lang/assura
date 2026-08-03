//! Gate demos/check-rust (issue #1458): prove ok tree; fail demo must CE.
mod common;

use common::{assura_bin, workspace_root};
use std::process::Command;

#[test]
fn check_rust_demos_ok_tree_proves() {
    let root = workspace_root();
    let path = format!("{root}/demos/check-rust/ok");
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", &path])
        .output()
        .expect("run check-rust");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "ok demos should prove: status={:?} stdout={stdout}",
        out.status
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["verified"].as_u64().unwrap_or(0) >= 3, "{stdout}");
    assert_eq!(v["errors"], 0, "{stdout}");
}

#[test]
fn check_rust_demos_fail_clamp_counterexample() {
    let root = workspace_root();
    let path = format!("{root}/demos/check-rust/fail/clamp_wrong.rs");
    let out = Command::new(assura_bin())
        .args(["check-rust", "--json", &path])
        .output()
        .expect("run check-rust");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "wrong ensures should fail: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["body_not_modeled"], 0, "{stdout}");
    assert!(v["errors"].as_u64().unwrap_or(0) >= 1, "{stdout}");
}
