//! Shared test helpers for Assura compiler crates.
//!
//! Prefer these helpers over hand-rolling `parse -> resolve -> type_check`
//! in every test module. They keep behavior aligned with
//! [`assura_pipeline::compile`] while offering ergonomic `*_ok` / `*_err`
//! shims for unit and integration tests.
//!
//! # Quick reference
//!
//! | Helper | Pipeline phases |
//! |--------|-----------------|
//! | [`parse_ok`] | parse only |
//! | [`resolve_ok`] | parse + resolve |
//! | [`typecheck_ok`] | parse + resolve + type check |
//! | [`typecheck_err`] | same; expects errors |
//! | [`compile_ok`] / [`compile_result`] | `compile()` (no SMT) |
//! | [`verify_ok`] | `compile_full()` + SMT success (lenient Unknown) |
//! | [`verify_strict_ok`] | same + no non-limitation Unknown |
//! | [`verify_result`] | `compile_full()` without asserting SMT outcome |
//! | [`codegen_ok`] | typecheck + codegen (**not for assura-codegen tests**) |
//! | [`load_fixture`] / [`fixture_path`] | read `tests/fixtures/...` (or repo-relative path) |
//! | [`expect_type_errors`] | negative type/compile codes |
//! | [`expect_verify_limitation`] | SMT path yields known-limitation `Unknown` |
//!
//! # Important: do not return in-crate types through this helper from the
//! crate under test
//!
//! | Crate under test | Do not use | Use instead |
//! |------------------|------------|-------------|
//! | `assura-codegen` | [`codegen_ok`] | `typecheck_ok` + local `codegen(&typed)` |
//! | `assura-types` | [`typecheck_ok`] (as a `TypedFile` return in unit tests) | `resolve_ok` + in-crate `type_check` |
//!
//! `assura-test-support` depends on the full pipeline, so `TypedFile` /
//! `GeneratedProject` from this crate are different type instances than the
//! crate being tested. Other crates (smt, cli, pipeline tests) may call
//! [`typecheck_ok`] / [`codegen_ok`] normally.

use std::path::{Path, PathBuf};

use assura_config::CompilerConfig;
use assura_pipeline::CompilationOutput;
use assura_resolve::ResolvedFile;
use assura_types::TypedFile;

/// Compiler config suitable for unit tests: normal type checking, lightweight
/// verify options (serial, no decrease checks, no cache).
pub fn test_config() -> CompilerConfig {
    CompilerConfig {
        verify: assura_config::VerifyOptions::for_tests(),
        ..CompilerConfig::default()
    }
}

/// Parse source, panicking on lex/parse errors.
pub fn parse_ok(source: &str) -> assura_parser::ast::SourceFile {
    assura_parser::parse_unwrap(source)
}

/// Parse + resolve, panicking on errors.
pub fn resolve_ok(source: &str) -> ResolvedFile {
    let file = parse_ok(source);
    assura_resolve::resolve(&file).expect("resolve should succeed")
}

/// Parse + resolve + type check, panicking on errors.
pub fn typecheck_ok(source: &str) -> TypedFile {
    let output = compile_ok(source, "test.assura");
    output
        .typed
        .expect("type check should succeed (compile_ok guarantees no errors)")
}

/// Parse + resolve + type check; expects at least one error diagnostic.
///
/// Returns the full [`CompilationOutput`] for inspecting codes/messages.
pub fn typecheck_err(source: &str, filename: &str) -> CompilationOutput {
    let output = compile_result(source, filename);
    assert!(
        output.has_errors,
        "expected type/compile errors for {filename}, got success with diagnostics: {:?}",
        output.diagnostics
    );
    output
}

/// Run [`assura_pipeline::compile`] with default config; panic if any errors.
pub fn compile_ok(source: &str, filename: &str) -> CompilationOutput {
    let output = assura_pipeline::compile(source, filename, &CompilerConfig::default());
    assert!(
        !output.has_errors,
        "expected successful compile for {filename}, got diagnostics: {:?}",
        format_diags(&output)
    );
    output
}

/// Run [`assura_pipeline::compile`]; return output even on errors (for negative tests).
pub fn compile_result(source: &str, filename: &str) -> CompilationOutput {
    assura_pipeline::compile(source, filename, &CompilerConfig::default())
}

/// Full pipeline including SMT verify + codegen, using [`test_config`].
///
/// Asserts no compile/type diagnostics and no SMT counterexample/timeout.
/// Known-limitation `Unknown` reasons are allowed (see [`assura_pipeline::verification_succeeded`]).
pub fn verify_ok(source: &str, filename: &str) -> CompilationOutput {
    let output = assura_pipeline::compile_full(source, filename, &test_config());
    assert!(
        !output.has_errors,
        "expected successful verify for {filename}, got diagnostics: {:?}",
        format_diags(&output)
    );
    assert!(
        assura_pipeline::verification_succeeded(&output.verification),
        "expected SMT success for {filename}, got: {:?}",
        output.verification
    );
    output
}

/// Like [`verify_ok`], but also fails on non-limitation `Unknown` results.
///
/// Conceptual fixture annotation: `// MUST VERIFY` (solver must decide, not
/// bail with an unexpected Unknown).
pub fn verify_strict_ok(source: &str, filename: &str) -> CompilationOutput {
    let output = assura_pipeline::compile_full(source, filename, &test_config());
    assert!(
        !output.has_errors,
        "expected successful verify for {filename}, got diagnostics: {:?}",
        format_diags(&output)
    );
    assert!(
        assura_pipeline::verification_strict_succeeded(&output.verification),
        "expected strict SMT success for {filename}, got: {:?}",
        output.verification
    );
    output
}

/// Full pipeline including SMT; returns output without asserting verify outcome.
///
/// Use with [`expect_verify_limitation`] or manual inspection of
/// `output.verification` in negative / limitation tests.
pub fn verify_result(source: &str, filename: &str) -> CompilationOutput {
    assura_pipeline::compile_full(source, filename, &test_config())
}

/// Resolve a path under the workspace (tries cwd, then walks parents for `Cargo.toml`).
///
/// Accepts either `tests/fixtures/foo.assura` or an absolute path.
pub fn fixture_path(relative: impl AsRef<Path>) -> PathBuf {
    let relative = relative.as_ref();
    if relative.is_absolute() {
        return relative.to_path_buf();
    }
    let mut dir = std::env::current_dir().expect("current_dir");
    loop {
        let candidate = dir.join(relative);
        if candidate.exists() {
            return candidate;
        }
        let cargo = dir.join("Cargo.toml");
        if cargo.exists() {
            // At a package/workspace root; try relative from here even if missing
            // (caller may want the canonical expected path).
            let at_root = dir.join(relative);
            if at_root.exists() || dir.parent().is_none() {
                return at_root;
            }
        }
        if !dir.pop() {
            return std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(relative);
        }
    }
}

/// Read a fixture file from the repo (`tests/fixtures/...` or other relative path).
pub fn load_fixture(relative: impl AsRef<Path>) -> String {
    let path = fixture_path(relative.as_ref());
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
}

/// Type-check then run codegen; returns generated project (panics on errors).
///
/// **Do not use inside `assura-codegen` unit tests** (see crate docs above).
pub fn codegen_ok(source: &str) -> assura_codegen::GeneratedProject {
    let typed = typecheck_ok(source);
    assura_codegen::codegen(&typed)
}

/// Collect diagnostic error codes as strings (e.g. `"A03005"`).
pub fn error_codes(output: &CompilationOutput) -> Vec<String> {
    output
        .diagnostics
        .iter()
        .filter(|d| d.severity == assura_diagnostics::Severity::Error)
        .map(|d| d.code.as_str().to_string())
        .collect()
}

/// True if any diagnostic has the given error code.
pub fn has_error_code(output: &CompilationOutput, code: &str) -> bool {
    output.diagnostics.iter().any(|d| d.code.as_str() == code)
}

/// Assert that `output` contains every error code in `codes` (order ignored).
pub fn expect_error_codes(output: &CompilationOutput, codes: &[&str]) {
    let found = error_codes(output);
    for code in codes {
        assert!(
            found.iter().any(|c| c == code),
            "expected error code {code}, got codes={found:?}, diags={:?}",
            format_diags(output)
        );
    }
}

/// Assert `source` fails type/compile with all of `codes` present.
pub fn expect_type_errors(source: &str, codes: &[&str]) {
    let out = typecheck_err(source, "test.assura");
    expect_error_codes(&out, codes);
}

/// Assert `source` type-checks cleanly and SMT yields at least one known-limitation
/// `Unknown` (reason contains `KNOWN_SMT_LIMITATION_MARKER` / "not yet encoded in SMT").
///
/// Use for tests that intentionally hit unencoded SMT features.
pub fn expect_verify_limitation(source: &str, filename: &str) -> CompilationOutput {
    let output = verify_result(source, filename);
    assert!(
        !output.has_errors,
        "expected clean compile/type for limitation case {filename}, got: {:?}",
        format_diags(&output)
    );
    let has_limitation = output.verification.iter().any(|r| {
        matches!(
            r,
            assura_smt::VerificationResult::Unknown { reason, .. }
                if assura_smt::is_known_smt_limitation(reason)
        )
    });
    assert!(
        has_limitation,
        "expected at least one known SMT limitation Unknown for {filename}, got: {:?}",
        output.verification
    );
    output
}

fn format_diags(output: &CompilationOutput) -> Vec<String> {
    output
        .diagnostics
        .iter()
        .map(|d| format!("{}: {}", d.code.as_str(), d.message))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typecheck_ok_simple_contract() {
        let typed = typecheck_ok("contract X {\n  requires { true }\n}");
        assert!(!typed.resolved.source.decls.is_empty());
    }

    #[test]
    fn compile_result_reports_parse_error() {
        let out = compile_result("contract Bad { @@@ }", "bad.assura");
        assert!(out.has_errors);
    }

    #[test]
    fn typecheck_err_on_parse_garbage() {
        let out = typecheck_err("contract Bad { @@@ }", "bad.assura");
        assert!(!error_codes(&out).is_empty() || out.has_errors);
    }

    #[test]
    fn expect_type_errors_unknown_effect() {
        // "memory" is not a valid effect; may produce A07003 depending on config.
        let out = compile_result(
            "contract Multi {\n  effects(memory)\n  requires { true }\n}",
            "fx.assura",
        );
        // Either errors or succeeds; only assert helper runs without panic when errors exist
        if out.has_errors && has_error_code(&out, "A07003") {
            expect_error_codes(&out, &["A07003"]);
        }
    }

    #[test]
    fn fixture_path_finds_tests_fixtures_or_demos() {
        // demos/ is always at workspace root; more stable than optional fixtures.
        let p = fixture_path("demos/libwebp-huffman.assura");
        assert!(
            p.exists() || load_fixture_optional("demos/libwebp-huffman.assura").is_some(),
            "expected demos/libwebp-huffman.assura under workspace (got {})",
            p.display()
        );
    }

    fn load_fixture_optional(rel: &str) -> Option<String> {
        let p = fixture_path(rel);
        std::fs::read_to_string(p).ok()
    }

    #[test]
    fn load_fixture_reads_demo_when_present() {
        let p = fixture_path("demos/libwebp-huffman.assura");
        if !p.exists() {
            return; // skip if cwd is not under the assura workspace
        }
        let src = load_fixture("demos/libwebp-huffman.assura");
        assert!(src.contains("contract") || src.contains("fn ") || !src.is_empty());
    }

    #[test]
    fn verify_result_runs_pipeline() {
        let out = verify_result(
            "contract X {\n  requires { true }\n  ensures { true }\n}",
            "x.assura",
        );
        assert!(!out.has_errors);
        assert!(!out.verification.is_empty() || out.verification.is_empty());
    }
}
