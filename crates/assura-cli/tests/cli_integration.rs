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

mod check_rust_inline_contract_annotation_ve;
mod diff_86;
mod diff_verify_212;
mod i003_wasm_target_tests;
mod infer_heuristic_based_contract_inference;
mod ir_sidecar_pipeline_assura_check_loads_n;
mod issue_49_audit_command_integration_tests;
mod issue_96_agent_instructions_command_inte;
mod issue_96_completions_command_integration;
mod issue_96_coverage_command_integration_te;
mod issue_96_doctor_command_integration_test;
mod issue_96_explain_command_integration_tes;
mod mcp_server_89;
mod p001_verbose_and_quiet_mode_tests;
mod r007_build_cli_integration_tests;
mod repl_91;
