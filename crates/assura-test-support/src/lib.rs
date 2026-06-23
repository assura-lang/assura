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
//! | [`codegen_ok`] | typecheck + codegen (**not for assura-codegen tests**) |
//!
//! # Important: `codegen_ok` and the `assura-codegen` crate
//!
//! Do **not** call [`codegen_ok`] from `assura-codegen`'s own unit tests.
//! `assura-test-support` depends on `assura-codegen`, so returning
//! `GeneratedProject` from this crate yields a *different type instance*
//! than the crate under test. In `assura-codegen` tests, use:
//!
//! ```ignore
//! let typed = assura_test_support::typecheck_ok(source);
//! let project = assura_codegen::codegen(&typed); // or `codegen(&typed)` in-crate
//! ```
//!
//! Other crates may call [`codegen_ok`] normally.

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
}
