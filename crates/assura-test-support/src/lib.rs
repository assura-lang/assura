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
//! | [`compile_ok`] | full `compile()` (no SMT) |
//! | [`verify_ok`] | `compile_full()` with test verify options |
//! | [`codegen_ok`] | typecheck + codegen |

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

/// Run [`assura_pipeline::compile`] with default config; panic if any errors.
pub fn compile_ok(source: &str, filename: &str) -> CompilationOutput {
    let output = assura_pipeline::compile(source, filename, &CompilerConfig::default());
    assert!(
        !output.has_errors,
        "expected successful compile for {filename}, got diagnostics: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| format!("{}: {}", d.code.as_str(), d.message))
            .collect::<Vec<_>>()
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
pub fn verify_ok(source: &str, filename: &str) -> CompilationOutput {
    let output = assura_pipeline::compile_full(source, filename, &test_config());
    assert!(
        !output.has_errors,
        "expected successful verify for {filename}, got diagnostics: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| format!("{}: {}", d.code.as_str(), d.message))
            .collect::<Vec<_>>()
    );
    assert!(
        assura_pipeline::verification_succeeded(&output.verification),
        "expected SMT success for {filename}, got: {:?}",
        output.verification
    );
    output
}

/// Type-check then run codegen; returns generated project (panics on errors).
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
}
