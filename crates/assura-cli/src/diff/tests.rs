/// Run the full pipeline: parse -> resolve -> type-check -> codegen
fn full_pipeline(source: &str) -> Result<assura_codegen::GeneratedProject, String> {
    let (file, errs) = assura_parser::parse(source);
    if !errs.is_empty() {
        return Err(format!("parse errors: {errs:?}"));
    }
    let file = file.ok_or("parse returned None")?;
    let resolved = assura_resolve::resolve(&file).map_err(|e| format!("resolve errors: {e:?}"))?;
    let typed = assura_types::type_check(resolved).map_err(|e| format!("type errors: {e:?}"))?;
    Ok(assura_codegen::codegen(&typed))
}

/// Verify that a source string successfully passes all pipeline stages.
fn assert_pipeline_ok(source: &str) {
    let project = full_pipeline(source).expect("pipeline failed");
    assert!(!project.cargo_toml.is_empty(), "empty Cargo.toml");
    assert!(!project.files.is_empty(), "no generated files");
    // Validate generated Rust is syntactically valid
    let lib = &project.files[0].1;
    syn::parse_file(lib).unwrap_or_else(|e| {
        panic!("generated Rust is not valid:\n{lib}\n\nerror: {e}");
    });
}

/// Load a monorepo fixture when present (workspace checkout). Returns `None`
/// when packaging/crates.io verify has no demos/ tree next to the crate.
fn load_monorepo_fixture(rel: &str) -> Option<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(path).ok()
}

#[test]
fn pipeline_contract() {
    assert_pipeline_ok(
        r#"
contract SafeDivision {
  input(a: Int, b: Int)
  output(result: Int)
  requires { b != 0 }
  ensures { result * b == a }
  effects { pure }
}
"#,
    );
}

#[test]
fn pipeline_fn_with_clauses() {
    assert_pipeline_ok(
        r#"
fn clamp(x: Int, lo: Int, hi: Int) -> Int
  requires { lo <= hi }
  ensures { result >= lo && result <= hi }
{
  if x < lo then lo else if x > hi then hi else x
}
"#,
    );
}

#[test]
fn pipeline_type_def() {
    assert_pipeline_ok(
        r#"
type Point {
  x: Int,
  y: Int
}

contract UsePoint {
  input(p: Point)
  output(result: Int)
  ensures { result >= 0 }
}
"#,
    );
}

#[test]
fn pipeline_demo_libwebp() {
    let Some(source) = load_monorepo_fixture("demos/libwebp-huffman.assura") else {
        return;
    };
    assert_pipeline_ok(&source);
}

#[test]
fn pipeline_demo_zlib() {
    let Some(source) = load_monorepo_fixture("demos/zlib-inflate.assura") else {
        return;
    };
    assert_pipeline_ok(&source);
}

#[test]
fn pipeline_demo_mbedtls() {
    let Some(source) = load_monorepo_fixture("demos/mbedtls-x509.assura") else {
        return;
    };
    assert_pipeline_ok(&source);
}

#[test]
fn pipeline_test_basic() {
    let Some(source) = load_monorepo_fixture("tests/fixtures/test_basic.assura") else {
        return;
    };
    assert_pipeline_ok(&source);
}

#[test]
fn pipeline_advanced_patterns() {
    let Some(source) = load_monorepo_fixture("tests/fixtures/advanced_patterns.assura") else {
        return;
    };
    assert_pipeline_ok(&source);
}

#[test]
fn test_diagnostics_from_parse_errors() {
    // Deliberately invalid syntax should produce parse errors
    let (file, errors) = assura_parser::parse("contract { invalid }");
    // At least some errors expected
    assert!(
        !errors.is_empty() || file.is_none(),
        "expected parse errors for invalid syntax"
    );
}

#[test]
fn test_parse_error_includes_message() {
    // Syntax error should produce an error with a meaningful message
    let (_file, errors) = assura_parser::parse("contract 123");
    assert!(!errors.is_empty(), "expected at least one parse error");
    let e = &errors[0];
    assert!(
        !e.message.is_empty(),
        "parse error should have a non-empty message, got: {e:?}"
    );
    // The error span should point to a valid location
    assert!(
        e.span.start <= e.span.end,
        "error span should be valid: {:?}",
        e.span
    );
}

#[test]
fn test_resolution_error_diagnostic() {
    // Duplicate contract names should produce a resolution error (A02003)
    let source = r#"
contract Foo {
  requires { true }
}
contract Foo {
  requires { false }
}
"#;
    let file = assura_parser::parse_unwrap(source);
    let resolved = assura_resolve::resolve(&file);
    assert!(
        resolved.is_err(),
        "duplicate contract names should produce resolution errors"
    );
    let errors = resolved.unwrap_err();
    assert!(
        !errors.is_empty(),
        "should have at least one resolution error for duplicate Foo"
    );
}

#[test]
fn test_type_error_diagnostic() {
    // Type checking should detect the type mismatch (requires needs Bool)
    let source = r#"
contract Typed {
  input(x: Int)
  requires { x + 1 }
}
"#;
    let file = assura_parser::parse_unwrap(source);
    let resolved = assura_resolve::resolve(&file).unwrap();
    let typed = assura_types::type_check(resolved);
    // `requires { x + 1 }` is an Int expression where Bool is expected,
    // so type checking should report at least one error.
    assert!(
        typed.is_err(),
        "expected type error for non-Bool requires clause"
    );
}

/// Walk tests/fixtures/errors/*.assura looking for `// MUST REJECT Axxxxx`
/// annotations. Each annotated file must produce a type error with the
/// specified code. This validates the error detection pipeline.
/// Scans both `tests/fixtures/errors/` and `tests/fixtures/must_reject/`.
#[test]
fn test_must_reject_fixtures() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let dirs = [
        root.join("tests/fixtures/errors"),
        root.join("tests/fixtures/must_reject"),
    ];

    let mut tested = 0;
    let mut blocked_paths: Vec<String> = Vec::new();
    for dir in &dirs {
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(dir).expect("cannot read fixtures dir") {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("assura") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();

            // Look for // MUST REJECT Axxxxx
            let expected_code = source.lines().find_map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("// MUST REJECT ") {
                    Some(trimmed.strip_prefix("// MUST REJECT ")?.trim().to_string())
                } else {
                    None
                }
            });
            let Some(code) = expected_code else {
                continue; // No annotation, skip
            };

            // Skip BLOCKED fixtures (known wiring gaps). Track paths so
            // silent debt cannot accumulate without failing the harness (#349).
            let is_blocked = source
                .lines()
                .any(|line| line.trim().starts_with("// BLOCKED:"));
            if is_blocked {
                blocked_paths.push(path.display().to_string());
                continue;
            }

            let (file, _parse_errors) = assura_parser::parse(&source);
            let Some(file) = file else {
                continue; // Parse failed entirely, not a type check test
            };
            let resolved = match assura_resolve::resolve(&file) {
                Ok(r) => r,
                Err(res_errors) => {
                    let found = res_errors.iter().any(|e| e.code == code);
                    assert!(
                        found,
                        "{}: expected resolution error {code}, got: {:?}",
                        path.display(),
                        res_errors
                    );
                    tested += 1;
                    continue;
                }
            };
            let type_result = assura_types::type_check(resolved);
            match type_result {
                Err(type_errors) => {
                    let found = type_errors.iter().any(|e| e.code == code);
                    assert!(
                        found,
                        "{}: expected type error {code}, got: {:?}",
                        path.display(),
                        type_errors
                    );
                }
                Ok(typed) => {
                    // Type check passed; try SMT verification for
                    // error codes in the A05xxx range (prophecy,
                    // verification failures).
                    let config = assura_config::CompilerConfig {
                        verify: assura_config::VerifyOptions::for_tests(),
                        ..Default::default()
                    };
                    let vr = assura_pipeline::verify_typed(
                        &typed,
                        path.to_str().unwrap_or("test.assura"),
                        &config,
                    );
                    let found = vr.iter().any(|r| match r {
                        assura_smt::VerificationResult::Unknown { clause_desc, .. } => {
                            clause_desc.contains(&code)
                        }
                        assura_smt::VerificationResult::Counterexample { clause_desc, .. } => {
                            clause_desc.contains(&code)
                        }
                        _ => false,
                    });
                    assert!(
                        found,
                        "{}: expected error {code} but type checking succeeded \
                             and SMT verification did not produce it. \
                             Verification results: {:?}",
                        path.display(),
                        vr
                    );
                }
            }
            tested += 1;
        }
    }
    if !blocked_paths.is_empty() {
        eprintln!(
            "test_must_reject_fixtures: skipped {} BLOCKED fixture(s): {}",
            blocked_paths.len(),
            blocked_paths.join(", ")
        );
    }
    // Zero is the healthy default; any BLOCKED fixture must have a tracking
    // GitHub issue referenced in the `// BLOCKED:` line. Raising this limit
    // is allowed only when adding a justified temporary gap (prefer fixing).
    const MAX_BLOCKED_MUST_REJECT: usize = 0;
    assert_eq!(
        blocked_paths.len(),
        MAX_BLOCKED_MUST_REJECT,
        "must_reject has {} BLOCKED fixture(s) (max allowed {MAX_BLOCKED_MUST_REJECT}). \
             Fix the wiring or add a tracking issue and temporarily raise MAX_BLOCKED_MUST_REJECT. \
             Blocked: {blocked_paths:?}",
        blocked_paths.len()
    );
    assert!(
        tested >= 92,
        "expected at least 92 MUST REJECT fixtures, found {tested}"
    );
}

/// T204: Positive test suite. Files annotated with `// MUST COMPILE` must
/// parse, resolve, type-check, and produce valid generated Rust (verified
/// via `syn::parse_file`).
#[test]
fn test_must_compile_fixtures() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/must_compile");

    let mut tested = 0;
    for entry in std::fs::read_dir(&dir).expect("cannot read must_compile fixtures dir") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("assura") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();

        // Verify annotation
        let has_annotation = source.lines().any(|l| l.trim() == "// MUST COMPILE");
        assert!(
            has_annotation,
            "{}: missing // MUST COMPILE annotation",
            path.display()
        );

        // Parse
        let (file, parse_errors) = assura_parser::parse(&source);
        assert!(
            parse_errors.is_empty(),
            "{}: unexpected parse errors: {:?}",
            path.display(),
            parse_errors
        );
        let file = file.unwrap_or_else(|| {
            panic!("{}: parse returned None", path.display());
        });

        // Resolve
        let resolved = assura_resolve::resolve(&file).unwrap_or_else(|errs| {
            panic!("{}: resolution errors: {:?}", path.display(), errs);
        });

        // Type check
        let typed = assura_types::type_check(resolved).unwrap_or_else(|errs| {
            panic!("{}: type errors: {:?}", path.display(), errs);
        });

        // Codegen
        let project = assura_codegen::codegen(&typed);

        // Verify generated Rust is syntactically valid
        for (file_path, rust_source) in &project.files {
            syn::parse_file(rust_source).unwrap_or_else(|err| {
                panic!(
                    "{}: generated {} is not valid Rust: {}\n--- source ---\n{}",
                    path.display(),
                    file_path,
                    err,
                    rust_source
                );
            });
        }

        tested += 1;
    }
    assert!(
        tested >= 25,
        "expected at least 25 MUST COMPILE fixtures, found {tested}"
    );
}

// =======================================================================
// Build --output flag tests
// =======================================================================

#[test]
fn build_output_generates_to_custom_dir() {
    // Verify codegen writes to the correct output directory
    let source = r#"
contract SimpleBuild {
  input(x: Int)
  output(result: Int)
  requires { x > 0 }
  ensures { result > 0 }
}
"#;
    let project = full_pipeline(source).expect("pipeline failed");
    // Verify the project has cargo toml and source files
    assert!(
        project.cargo_toml.contains("[package]"),
        "should have package section"
    );
    assert!(!project.files.is_empty(), "should have generated files");
    let (path, content) = &project.files[0];
    assert_eq!(path, "src/lib.rs");
    assert!(
        content.contains("fn check"),
        "should contain check function"
    );
}

#[test]
fn build_output_writes_files_to_disk() {
    let source = r#"
contract DiskWrite {
  input(n: Int)
  output(result: Bool)
  requires { n >= 0 }
  ensures { result }
}
"#;
    let project = full_pipeline(source).expect("pipeline failed");
    // Write to a temp directory and verify files exist
    let tmp = std::env::temp_dir().join("assura_test_output");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    std::fs::write(tmp.join("Cargo.toml"), &project.cargo_toml).unwrap();
    for (path, content) in &project.files {
        std::fs::write(tmp.join(path), content).unwrap();
    }
    // Verify files exist
    assert!(tmp.join("Cargo.toml").exists());
    assert!(tmp.join("src/lib.rs").exists());
    // Read back and verify content
    let cargo_content = std::fs::read_to_string(tmp.join("Cargo.toml")).unwrap();
    assert!(cargo_content.contains("[package]"));
    let lib_content = std::fs::read_to_string(tmp.join("src/lib.rs")).unwrap();
    assert!(lib_content.contains("Generated by the Assura compiler"));
    // Clean up
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_codegen_with_cranelift_backend() {
    let source = r#"
contract CraneliftTest {
  input(x: Int)
  output(result: Int)
  ensures { result == x }
}
"#;
    let file = assura_parser::parse_unwrap(source);
    let resolved = assura_resolve::resolve(&file).unwrap();
    let typed = assura_types::type_check(resolved).unwrap();
    let config = assura_codegen::BackendConfig {
        backend: assura_codegen::CodegenBackend::Cranelift,
        opt_level: 0,
        debug_info: true,
        target: assura_codegen::CompileTarget::Native,
        ..Default::default()
    };
    let project = assura_codegen::codegen_with_config(&typed, &config);
    assert!(
        project.cargo_toml.contains("Cranelift"),
        "should mention Cranelift backend"
    );
    assert!(
        project.cargo_toml.contains("debug = true"),
        "should have debug info"
    );
}

// =======================================================================
// T205: End-to-end round-trip tests
// =======================================================================

/// Helper: run the full pipeline on a demo file and return the generated project.
fn roundtrip_demo(demo_name: &str) -> Option<assura_codegen::GeneratedProject> {
    let source = load_monorepo_fixture(&format!("demos/{demo_name}"))?;
    Some(full_pipeline(&source).unwrap_or_else(|e| panic!("{demo_name}: pipeline failed: {e}")))
}

/// Helper: write a GeneratedProject to a temp dir and run cargo check on it.
fn cargo_check_project(project: &assura_codegen::GeneratedProject, label: &str) {
    let tmp = std::env::temp_dir().join(format!("assura_t205_{label}"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    std::fs::write(tmp.join("Cargo.toml"), &project.cargo_toml).unwrap();
    for (path, content) in &project.files {
        let full = tmp.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full, content).unwrap();
    }
    let output = std::process::Command::new("cargo")
        .arg("check")
        .current_dir(&tmp)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("cargo check failed to start");
    if !output.status.success() {
        // Keep temp dir on failure so the path in the panic is still valid.
        panic!(
            "{label}: generated Rust failed cargo check (temp: {}):\n{}",
            tmp.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn roundtrip_libwebp_generates_valid_rust() {
    let Some(project) = roundtrip_demo("libwebp-huffman.assura") else {
        return;
    };
    // Verify syntactically valid
    for (path, content) in &project.files {
        syn::parse_file(content).unwrap_or_else(|e| {
            panic!("libwebp {path}: invalid Rust: {e}");
        });
    }
    // Verify cargo check passes
    cargo_check_project(&project, "libwebp");
}

#[test]
fn roundtrip_zlib_generates_valid_rust() {
    let Some(project) = roundtrip_demo("zlib-inflate.assura") else {
        return;
    };
    for (path, content) in &project.files {
        syn::parse_file(content).unwrap_or_else(|e| {
            panic!("zlib {path}: invalid Rust: {e}");
        });
    }
    cargo_check_project(&project, "zlib");
}

#[test]
fn roundtrip_mbedtls_generates_valid_rust() {
    let Some(project) = roundtrip_demo("mbedtls-x509.assura") else {
        return;
    };
    for (path, content) in &project.files {
        syn::parse_file(content).unwrap_or_else(|e| {
            panic!("mbedtls {path}: invalid Rust: {e}");
        });
    }
    cargo_check_project(&project, "mbedtls");
}

#[test]
fn roundtrip_libwebp_has_debug_asserts() {
    let Some(project) = roundtrip_demo("libwebp-huffman.assura") else {
        return;
    };
    // Contracts with requires clauses should produce debug_assert! calls
    let all_source: String = project
        .files
        .iter()
        .map(|(_, content)| content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    // The libwebp demo has requires { alphabet_size <= MAX_ALPHABET_SIZE }
    // which should produce a debug_assert in the generated code
    assert!(
        all_source.contains("debug_assert!"),
        "generated code should contain debug_assert! from requires clauses"
    );
}

#[test]
fn roundtrip_zlib_has_function_stubs() {
    let Some(project) = roundtrip_demo("zlib-inflate.assura") else {
        return;
    };
    let all_source: String = project
        .files
        .iter()
        .map(|(_, content)| content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    // The zlib demo defines functions and contracts
    // The generated lib always has the allow header (proves codegen produced output for the demo)
    assert!(
        all_source.contains("#![allow("),
        "zlib generated code should contain the standard allow header from codegen"
    );
}

#[test]
fn roundtrip_libwebp_function_signatures_present() {
    let Some(project) = roundtrip_demo("libwebp-huffman.assura") else {
        return;
    };
    let all_source: String = project
        .files
        .iter()
        .map(|(_, content)| content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    // The libwebp demo defines functions like validate_code_lengths
    // and a contract BuildHuffmanTableContract with a check() method
    assert!(
        all_source.contains("fn validate_code_lengths"),
        "generated code should have validate_code_lengths function"
    );
    assert!(
        all_source.contains("fn check("),
        "generated code should have check function from contract"
    );
}

#[test]
fn roundtrip_contract_with_ensures_has_postcondition() {
    // A contract with ensures should generate postcondition checks
    let source = r#"
contract PostCheck {
    input(a: Int, b: Int)
    output(result: Int)
    requires { b != 0 }
    ensures { result * b == a }
}
"#;
    let project = full_pipeline(source).expect("pipeline failed");
    let all_source: String = project
        .files
        .iter()
        .map(|(_, content)| content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    // requires clause should become debug_assert
    assert!(
        all_source.contains("debug_assert!"),
        "should have debug_assert from requires clause"
    );
    // The function should have the right parameter types
    assert!(
        all_source.contains("i64") || all_source.contains("Int"),
        "should have integer types in generated code"
    );
}

#[test]
fn roundtrip_service_generates_typestate() {
    // A service with states should generate typestate markers
    // Colon-form typestate clauses must parse as Bool expressions (not empty Raw).
    let source = r#"
service Connection {
    states: Disconnected -> Connected -> Authenticated

    operation Connect {
        requires: state == Disconnected
        ensures: state == Connected
    }

    operation Authenticate {
        requires: state == Connected
        ensures: state == Authenticated
    }
}
"#;
    let project = full_pipeline(source).expect("pipeline failed");
    let all_source: String = project
        .files
        .iter()
        .map(|(_, content)| content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    // Should generate state marker structs
    assert!(
        all_source.contains("Disconnected") && all_source.contains("Connected"),
        "should generate state marker structs"
    );
    assert!(
        all_source.contains("PhantomData"),
        "should use PhantomData for typestate"
    );
}

#[test]
fn roundtrip_project_has_valid_cargo_toml() {
    let Some(project) = roundtrip_demo("libwebp-huffman.assura") else {
        return;
    };
    // Verify Cargo.toml has essential sections
    assert!(
        project.cargo_toml.contains("[package]"),
        "needs [package] section"
    );
    assert!(project.cargo_toml.contains("name ="), "needs package name");
    assert!(project.cargo_toml.contains("edition ="), "needs edition");
}

// ---------------------------------------------------------------------------
// Formatter tests
// ---------------------------------------------------------------------------

/// Format source, re-format, and assert idempotency.
fn assert_format_idempotent(source: &str) {
    let formatted1 = assura_fmt::format_source(source);

    let (_, errs2) = assura_parser::parse(&formatted1);
    assert!(
        errs2.is_empty(),
        "parse errors on formatted output: {errs2:?}\nformatted:\n{formatted1}"
    );

    let formatted2 = assura_fmt::format_source(&formatted1);
    assert_eq!(
        formatted1, formatted2,
        "formatter is not idempotent:\n--- pass 1 ---\n{formatted1}\n--- pass 2 ---\n{formatted2}"
    );
}

#[test]
fn fmt_contract_idempotent() {
    assert_format_idempotent(
        r#"
contract SafeDivision {
  input(a: Int, b: Int)
  output(result: Int)
  requires { b != 0 }
  ensures { result * b + (a mod b) == a }
  effects { pure }
}
"#,
    );
}

#[test]
fn fmt_type_and_enum_idempotent() {
    assert_format_idempotent(
        r#"
type PositiveInt = { n: Int | n > 0 };
enum Color { Red, Green, Blue }
enum Result<T> { Ok(T), Err(String) }
"#,
    );
}

#[test]
fn fmt_extern_fn_idempotent() {
    assert_format_idempotent(
        r#"
extern fn malloc(size: Nat) -> Bytes
  requires { size > 0 }
  ensures { result.length() == size }
  effects { mem.alloc };
"#,
    );
}

#[test]
fn fmt_fn_with_clauses_idempotent() {
    assert_format_idempotent(
        r#"
fn fibonacci(n: Nat) -> Nat
  requires n >= 0
  decreases n
  ensures result >= 0
"#,
    );
}

#[test]
fn fmt_service_idempotent() {
    assert_format_idempotent(
        r#"
service UserService {
  type User {
    id: Nat;
    name: String;
  }
  states: Created -> Active -> Deleted
  operation CreateUser {
    input(name: String)
    output(user: User)
    requires { name.length() > 0 }
    effects { database.write }
  }
  invariant { forall u in users: u.id > 0 }
}
"#,
    );
}

#[test]
fn fmt_project_and_module_idempotent() {
    assert_format_idempotent(
        r#"
project myapp {
  profile: [core, mem, sec]
}

module app.main;

import std.math { abs };

contract Foo {
  input(x: Int)
  requires { x > 0 }
}
"#,
    );
}

#[test]
fn fmt_feature_block_idempotent() {
    assert_format_idempotent(
        r#"
feature ecdsa = enabled
feature x509 = enabled
  requires: ecdsa
feature_max MAX_SIZE: Nat = 256
"#,
    );
}

#[test]
fn fmt_produces_parseable_output() {
    // Verify that formatting a contract produces valid parseable output
    let source = "contract Foo {\n  input(x: Int)\n  requires { x > 0 }\n}\n";
    let formatted = assura_fmt::format_source(source);
    // Must contain the contract name
    assert!(
        formatted.contains("contract Foo"),
        "formatted must contain contract name"
    );
    // Must re-parse without errors
    let (file2, errs) = assura_parser::parse(&formatted);
    assert!(
        errs.is_empty(),
        "formatted output has parse errors: {errs:?}"
    );
    assert!(file2.is_some(), "formatted output parsed to None");
}

#[test]
fn fmt_dotted_effects_idempotent() {
    assert_format_idempotent(
        r#"
fn read_data(conn: &mut Connection) -> Bytes
  effects { io.read }
"#,
    );
}

// -----------------------------------------------------------------------
// Config parsing tests
// -----------------------------------------------------------------------

#[test]
fn parse_full_config() {
    let toml_str = r#"
[package]
name = "my-project"
version = "1.2.3"

[build]
target = "wasm32-wasi"
output = "out"

[verify]
smt-solver = "cvc5"
layer = 0
timeout = 5000

[profile]
type = "database"
"#;
    let config: crate::ProjectConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.package.name, "my-project");
    assert_eq!(config.package.version, "1.2.3");
    assert_eq!(config.build.target, "wasm32-wasi");
    assert_eq!(config.build.output, "out");
    assert_eq!(config.verify.smt_solver, assura_config::SolverChoice::Cvc5);
    assert_eq!(config.verify.layer, 0);
    assert_eq!(config.verify.timeout, 5000);
    assert_eq!(config.profile.profile_type, "database");
}

#[test]
fn parse_minimal_config() {
    let toml_str = r#"
[package]
name = "test"
"#;
    let config: crate::ProjectConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.package.name, "test");
    assert_eq!(config.package.version, "0.1.0"); // default
    assert_eq!(config.build.target, "native"); // default
    assert_eq!(config.build.output, "generated"); // default
    assert_eq!(config.verify.smt_solver, assura_config::SolverChoice::Z3); // default
    assert_eq!(config.verify.layer, 1); // default
    assert_eq!(config.verify.timeout, 1000); // default
    assert_eq!(config.profile.profile_type, "minimal"); // default
}

#[test]
fn parse_empty_config() {
    let config: crate::ProjectConfig = toml::from_str("").unwrap();
    assert_eq!(config.package.name, ""); // default
    assert_eq!(config.verify.layer, 1);
}

#[test]
fn load_config_from_disk() {
    let dir = std::env::temp_dir().join("assura-config-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let config_content = r#"[package]
name = "disk-test"
version = "0.3.0"

[verify]
layer = 0
timeout = 2000
"#;
    std::fs::write(dir.join("assura.toml"), config_content).unwrap();

    // Create a subdir with a dummy file
    let sub = dir.join("src");
    std::fs::create_dir_all(&sub).unwrap();
    let file = sub.join("main.assura");
    std::fs::write(&file, "").unwrap();

    let result = crate::load_project_config(&file);
    let (cfg, root) = result.expect("should find config");
    assert_eq!(cfg.package.name, "disk-test");
    assert_eq!(cfg.package.version, "0.3.0");
    assert_eq!(cfg.verify.layer, 0);
    assert_eq!(cfg.verify.timeout, 2000);
    assert_eq!(root, dir);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_config_missing_returns_none() {
    let dir = std::env::temp_dir().join("assura-no-config-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test.assura");
    std::fs::write(&file, "").unwrap();

    // Temp dir with no assura.toml should return None.
    let result = crate::load_project_config(&file);
    assert!(
        result.is_none(),
        "expected None for directory without assura.toml, got {:?}",
        result
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// =======================================================================
// E2E expected outcomes test harness
// =======================================================================

/// Expected outcome parsed from an `// EXPECTED: <kind>` annotation.
#[derive(Debug, PartialEq)]
enum ExpectedOutcome {
    /// File should verify successfully (no errors).
    Verified,
    /// File should produce at least one counterexample.
    Counterexample,
}

/// Parse `// EXPECTED: verified` or `// EXPECTED: counterexample`
/// from the first lines of source text.
fn parse_expected(source: &str) -> Option<ExpectedOutcome> {
    for line in source.lines().take(5) {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("// EXPECTED:") {
            let kind = rest.trim().to_lowercase();
            return match kind.as_str() {
                "verified" => Some(ExpectedOutcome::Verified),
                "counterexample" => Some(ExpectedOutcome::Counterexample),
                _ => None,
            };
        }
    }
    None
}

/// Run the full pipeline (parse -> resolve -> type-check -> verify)
/// and return (has_errors, has_counterexample).
///
/// E2E tests use verify_parallel with caching (matches real CLI
/// behavior) rather than the shared pipeline's basic verify().
fn run_e2e_pipeline(source: &str, source_path: &std::path::Path) -> (bool, bool) {
    let (file, parse_errors) = assura_parser::parse(source);
    if !parse_errors.is_empty() {
        return (true, false);
    }
    let file = match file {
        Some(f) => f,
        None => return (true, false),
    };
    let resolved = match assura_resolve::resolve(&file) {
        Ok(r) => r,
        Err(_) => return (true, false),
    };
    let typed = match assura_types::TypeChecker::new().check(resolved) {
        Ok(t) => t,
        Err(_) => return (true, false),
    };
    let path_str = source_path.to_str().unwrap_or("test.assura");
    let config = assura_config::CompilerConfig::default();
    let results = assura_pipeline::verify_typed(&typed, path_str, &config);
    let has_counterexample = results
        .iter()
        .any(|r| matches!(r, assura_smt::VerificationResult::Counterexample { .. }));
    (false, has_counterexample)
}

#[test]
fn test_e2e_expected_outcomes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let e2e_dir = root.join("tests/e2e");

    let mut tested = 0;
    let mut failures: Vec<String> = Vec::new();

    for entry in std::fs::read_dir(&e2e_dir).expect("cannot read tests/e2e") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("assura") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        let filename = path.file_name().unwrap().to_str().unwrap();

        let expected = match parse_expected(&source) {
            Some(e) => e,
            None => {
                failures.push(format!("{filename}: missing // EXPECTED: annotation"));
                continue;
            }
        };

        let (has_errors, has_counterexample) = run_e2e_pipeline(&source, &path);

        match expected {
            ExpectedOutcome::Verified => {
                if has_errors || has_counterexample {
                    failures.push(format!(
                            "{filename}: expected verified, but got errors={has_errors} counterexample={has_counterexample}"
                        ));
                }
            }
            ExpectedOutcome::Counterexample => {
                if !has_counterexample {
                    failures.push(format!(
                        "{filename}: expected counterexample, but none found"
                    ));
                }
            }
        }

        tested += 1;
    }

    assert!(
        failures.is_empty(),
        "E2E test failures:\n{}",
        failures.join("\n")
    );
    assert!(
        tested >= 5,
        "expected at least 5 E2E test files, found {tested}"
    );
}

// =======================================================================
// discover_rs_files unit tests (issue #49)
// =======================================================================

#[test]
fn discover_rs_files_finds_nested_files() {
    let dir = std::env::temp_dir().join("assura_test_discover_nested");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub/deep")).unwrap();
    std::fs::write(dir.join("main.rs"), "fn main() {}").unwrap();
    std::fs::write(dir.join("sub/lib.rs"), "pub fn f() {}").unwrap();
    std::fs::write(dir.join("sub/deep/util.rs"), "pub fn g() {}").unwrap();
    // Non-Rust files should be skipped
    std::fs::write(dir.join("notes.txt"), "not rust").unwrap();
    std::fs::write(dir.join("sub/readme.md"), "docs").unwrap();

    let found = crate::discover_rs_files(&dir);
    assert_eq!(found.len(), 3, "should find exactly 3 .rs files");
    assert!(
        found.iter().any(|p| p.ends_with("main.rs")),
        "should find main.rs"
    );
    assert!(
        found.iter().any(|p| p.ends_with("lib.rs")),
        "should find sub/lib.rs"
    );
    assert!(
        found.iter().any(|p| p.ends_with("util.rs")),
        "should find sub/deep/util.rs"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discover_rs_files_empty_dir_returns_empty() {
    let dir = std::env::temp_dir().join("assura_test_discover_empty");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let found = crate::discover_rs_files(&dir);
    assert!(found.is_empty(), "empty dir should yield no files");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discover_rs_files_nonexistent_returns_empty() {
    let dir = std::env::temp_dir().join("assura_test_discover_nonexistent");
    let _ = std::fs::remove_dir_all(&dir);

    let found = crate::discover_rs_files(&dir);
    assert!(found.is_empty(), "nonexistent dir should yield no files");
}

// =======================================================================
// Infer helper tests (issue #50)
// =======================================================================

#[test]
fn extract_sigs_simple_pub_fn() {
    let source = "pub fn add(a: i64, b: i64) -> i64 { a + b }";
    let sigs = crate::extract_rust_fn_signatures(source);
    assert_eq!(sigs.len(), 1);
    assert_eq!(sigs[0].name, "add");
    assert!(sigs[0].is_pub);
    assert_eq!(sigs[0].params.len(), 2);
    assert_eq!(sigs[0].return_type, "i64");
}

#[test]
fn extract_sigs_skips_private_fn() {
    let source = "fn helper(x: i32) -> i32 { x }";
    let sigs = crate::extract_rust_fn_signatures(source);
    assert_eq!(sigs.len(), 1);
    assert!(!sigs[0].is_pub);
}

#[test]
fn extract_sigs_multiline() {
    let source = "pub fn long_name(\n    a: String,\n    b: Vec<u8>,\n) -> bool {\n    true\n}";
    let sigs = crate::extract_rust_fn_signatures(source);
    assert_eq!(sigs.len(), 1);
    assert_eq!(sigs[0].name, "long_name");
    assert_eq!(sigs[0].params.len(), 2);
    assert_eq!(sigs[0].return_type, "bool");
}

#[test]
fn extract_sigs_with_self_param() {
    let source = "pub fn get(&self, key: &str) -> Option<String> {";
    let sigs = crate::extract_rust_fn_signatures(source);
    assert_eq!(sigs.len(), 1);
    // &self should be skipped
    assert_eq!(sigs[0].params.len(), 1);
    assert_eq!(sigs[0].params[0].0, "key");
}

#[test]
fn extract_sigs_pub_crate() {
    let source = "pub(crate) fn internal(x: u32) -> u32 { x }";
    let sigs = crate::extract_rust_fn_signatures(source);
    assert_eq!(sigs.len(), 1);
    assert!(sigs[0].is_pub);
    assert_eq!(sigs[0].name, "internal");
}

#[test]
fn extract_sigs_no_return_type() {
    let source = "pub fn do_stuff(x: i32) { println!(\"{x}\"); }";
    let sigs = crate::extract_rust_fn_signatures(source);
    assert_eq!(sigs.len(), 1);
    assert_eq!(sigs[0].return_type, "()");
}

#[test]
fn parse_param_list_empty() {
    let result = crate::parse_param_list("");
    assert!(result.is_empty());
}

#[test]
fn parse_param_list_single() {
    let result = crate::parse_param_list("x: i64");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], ("x".to_string(), "i64".to_string()));
}

#[test]
fn parse_param_list_multiple() {
    let result = crate::parse_param_list("a: i32, b: String, c: bool");
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].0, "a");
    assert_eq!(result[1].0, "b");
    assert_eq!(result[2].0, "c");
}

#[test]
fn parse_param_list_nested_generics() {
    let result = crate::parse_param_list("data: HashMap<String, Vec<Option<i32>>>, count: usize");
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].0, "data");
    assert_eq!(result[0].1, "HashMap<String, Vec<Option<i32>>>");
    assert_eq!(result[1].0, "count");
}

#[test]
fn parse_param_list_skips_self() {
    let result = crate::parse_param_list("&self, x: i32");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "x");
}

#[test]
fn parse_param_list_mut_self() {
    let result = crate::parse_param_list("&mut self, key: String, val: i64");
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].0, "key");
    assert_eq!(result[1].0, "val");
}

#[test]
fn parse_fn_sig_basic() {
    let sig = crate::parse_fn_signature("add(a: i64, b: i64) -> i64 {", true).unwrap();
    assert_eq!(sig.name, "add");
    assert_eq!(sig.params.len(), 2);
    assert_eq!(sig.return_type, "i64");
    assert!(sig.is_pub);
}

#[test]
fn parse_fn_sig_with_where() {
    let sig = crate::parse_fn_signature("process(x: T) -> T where T: Clone {", true).unwrap();
    assert_eq!(sig.name, "process");
    assert_eq!(sig.return_type, "T");
}

#[test]
fn parse_fn_sig_no_return() {
    let sig = crate::parse_fn_signature("do_work(x: i32) {", false).unwrap();
    assert_eq!(sig.name, "do_work");
    assert_eq!(sig.return_type, "()");
    assert!(!sig.is_pub);
}

#[test]
fn generate_bind_skeleton_roundtrip() {
    let sig = crate::RustFnSig {
        name: "add".to_string(),
        params: vec![
            ("a".to_string(), "i64".to_string()),
            ("b".to_string(), "i64".to_string()),
        ],
        return_type: "i64".to_string(),
        is_pub: true,
    };
    let mut out = String::new();
    crate::generate_bind_skeleton("crate::math", &sig, &mut out);
    assert!(out.contains("bind \"crate::math::add\" as add"));
    assert!(out.contains("input(a: Int, b: Int)"));
    assert!(out.contains("output(result: Int)"));
    // Numeric params get heuristic requires clauses
    assert!(
        out.contains("requires { a >= 0 }"),
        "expected requires for numeric param a:\n{out}"
    );
    assert!(
        out.contains("requires { b >= 0 }"),
        "expected requires for numeric param b:\n{out}"
    );
    // Numeric return gets heuristic ensures clause
    assert!(
        out.contains("ensures { result >= 0 }"),
        "expected ensures for numeric return:\n{out}"
    );
    // Should NOT have TODO placeholders when heuristic clauses are generated
    assert!(
        !out.contains("// TODO:"),
        "should not have TODO when clauses generated:\n{out}"
    );
    // Should parse through our own parser
    let (parsed, errs) = assura_parser::parse(&out);
    assert!(
        errs.is_empty(),
        "generated bind should parse: {errs:?}\n{out}"
    );
    assert!(parsed.is_some(), "parsed to None:\n{out}");
}

#[test]
fn generate_bind_skeleton_no_return() {
    let sig = crate::RustFnSig {
        name: "log".to_string(),
        params: vec![("msg".to_string(), "&str".to_string())],
        return_type: "()".to_string(),
        is_pub: true,
    };
    let mut out = String::new();
    crate::generate_bind_skeleton("crate::util", &sig, &mut out);
    assert!(out.contains("bind \"crate::util::log\" as log"));
    assert!(out.contains("input(msg: String)"));
    // Unit return should not produce output line
    assert!(!out.contains("output(result:"));
    // No numeric params or return: should fall back to TODO comments
    assert!(
        out.contains("// TODO: add requires clauses"),
        "expected TODO fallback for non-numeric function:\n{out}"
    );
}

#[test]
fn generate_bind_skeleton_mixed_params() {
    // Mix of numeric and non-numeric params; only numeric ones get requires
    let sig = crate::RustFnSig {
        name: "process".to_string(),
        params: vec![
            ("label".to_string(), "String".to_string()),
            ("count".to_string(), "u32".to_string()),
        ],
        return_type: "bool".to_string(),
        is_pub: true,
    };
    let mut out = String::new();
    crate::generate_bind_skeleton("crate::ops", &sig, &mut out);
    assert!(
        out.contains("requires { count >= 0 }"),
        "expected requires for numeric param count:\n{out}"
    );
    // Non-numeric param should NOT get a requires
    assert!(
        !out.contains("requires { label"),
        "non-numeric param should not get requires:\n{out}"
    );
    // Bool return should NOT get ensures
    assert!(
        !out.contains("ensures"),
        "Bool return should not get ensures:\n{out}"
    );
    // Has at least one clause so no TODO
    assert!(
        !out.contains("// TODO:"),
        "should not have TODO when clauses generated:\n{out}"
    );
}

#[test]
fn discover_rs_files_results_are_sorted() {
    let dir = std::env::temp_dir().join("assura_test_discover_sorted");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("b")).unwrap();
    std::fs::create_dir_all(dir.join("a")).unwrap();
    std::fs::write(dir.join("b/z.rs"), "").unwrap();
    std::fs::write(dir.join("a/a.rs"), "").unwrap();
    std::fs::write(dir.join("c.rs"), "").unwrap();

    let found = crate::discover_rs_files(&dir);
    let mut sorted = found.clone();
    sorted.sort();
    assert_eq!(found, sorted, "results should be sorted");
    let _ = std::fs::remove_dir_all(&dir);
}

// --- Regression tests for #42: module path derivation ---

#[test]
fn derive_module_path_crate_with_hyphen() {
    // Simulates crates/assura-codegen/src/type_map.rs
    let dir = std::env::temp_dir().join("assura_test_modpath_42");
    let _ = std::fs::remove_dir_all(&dir);
    let crate_dir = dir.join("crates/my-crate");
    std::fs::create_dir_all(crate_dir.join("src")).unwrap();
    std::fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    let path = format!("{}/crates/my-crate/src/type_map.rs", dir.display());
    let module = crate::derive_rust_module_path(&path);
    assert_eq!(
        module, "my_crate::type_map",
        "hyphens must become underscores"
    );

    // lib.rs should resolve to just the crate name
    let lib_path = format!("{}/crates/my-crate/src/lib.rs", dir.display());
    let lib_module = crate::derive_rust_module_path(&lib_path);
    assert_eq!(lib_module, "my_crate");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn derive_module_path_nested_module() {
    let dir = std::env::temp_dir().join("assura_test_modpath_nested");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/foo")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"example\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    let path = format!("{}/src/foo/bar.rs", dir.display());
    let module = crate::derive_rust_module_path(&path);
    assert_eq!(module, "example::foo::bar");

    let _ = std::fs::remove_dir_all(&dir);
}

// --- Regression tests for #43: workspace discovery ---

#[test]
fn discover_workspace_src_dirs_single_crate() {
    let dir = std::env::temp_dir().join("assura_test_ws_single");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"single\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let dirs = crate::discover_workspace_src_dirs(&dir);
    assert_eq!(dirs.len(), 1);
    assert!(dirs[0].ends_with("src"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discover_workspace_src_dirs_workspace_glob() {
    let dir = std::env::temp_dir().join("assura_test_ws_glob");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("crates/alpha/src")).unwrap();
    std::fs::create_dir_all(dir.join("crates/beta/src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\n",
    )
    .unwrap();

    let dirs = crate::discover_workspace_src_dirs(&dir);
    assert_eq!(dirs.len(), 2, "should find both workspace members");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discover_workspace_src_dirs_explicit_members() {
    let dir = std::env::temp_dir().join("assura_test_ws_explicit");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lib/core/src")).unwrap();
    std::fs::create_dir_all(dir.join("tools/cli/src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[workspace]\nmembers = [\"lib/core\", \"tools/cli\"]\n",
    )
    .unwrap();

    let dirs = crate::discover_workspace_src_dirs(&dir);
    assert_eq!(dirs.len(), 2);

    let _ = std::fs::remove_dir_all(&dir);
}

// --- Regression tests for #44: function signature extraction ---

#[test]
fn extract_sigs_async_fn() {
    let source = "pub async fn fetch(url: &str) -> Result<String, Error> {";
    let sigs = crate::extract_rust_fn_signatures(source);
    assert_eq!(sigs.len(), 1, "should match pub async fn");
    assert_eq!(sigs[0].name, "fetch");
    assert!(sigs[0].is_pub);
}

#[test]
fn extract_sigs_const_fn() {
    let source = "pub const fn max_size() -> usize { 1024 }";
    let sigs = crate::extract_rust_fn_signatures(source);
    assert_eq!(sigs.len(), 1, "should match pub const fn");
    assert_eq!(sigs[0].name, "max_size");
}

#[test]
fn extract_sigs_unsafe_fn() {
    let source = "pub unsafe fn raw_ptr(p: *const u8) -> u8 {";
    let sigs = crate::extract_rust_fn_signatures(source);
    assert_eq!(sigs.len(), 1, "should match pub unsafe fn");
    assert_eq!(sigs[0].name, "raw_ptr");
}

#[test]
fn extract_sigs_pub_crate_async_fn() {
    let source = "pub(crate) async fn internal_fetch(url: &str) -> String {";
    let sigs = crate::extract_rust_fn_signatures(source);
    assert_eq!(sigs.len(), 1, "should match pub(crate) async fn");
    assert_eq!(sigs[0].name, "internal_fetch");
    assert!(sigs[0].is_pub);
}

#[test]
fn extract_sigs_generic_fn_name_stripped() {
    let source = "pub fn encode<T: Serialize>(value: &T) -> Vec<u8> {";
    let sigs = crate::extract_rust_fn_signatures(source);
    assert_eq!(sigs.len(), 1);
    assert_eq!(
        sigs[0].name, "encode",
        "generic params must be stripped from name"
    );
}

#[test]
fn extract_sigs_generic_with_where() {
    let source = "pub fn process<T>(items: Vec<T>) -> Vec<T> where T: Clone + Debug {";
    let sigs = crate::extract_rust_fn_signatures(source);
    assert_eq!(sigs.len(), 1);
    assert_eq!(sigs[0].name, "process");
    assert_eq!(sigs[0].return_type, "Vec<T>");
}
