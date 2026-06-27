use super::*;

// `assura build <file.assura>` — codegen to generated/
// ---------------------------------------------------------------------------

pub(crate) fn run_build(
    filename: &str,
    _output_mode: OutputMode,
    verbosity: Verbosity,
    cli_output: &str,
    cli_target: Option<assura_codegen::CompileTarget>,
    no_check: bool,
    cli_solver: Option<assura_smt::SolverChoice>,
) {
    // Load project config (assura.toml) if available
    let project = load_project_config(Path::new(filename));
    let config_output = project
        .as_ref()
        .map(|(c, _)| c.build.output.clone())
        .unwrap_or_else(|| "generated".to_string());

    // Output directory: CLI flag (non-default) > config file > default "generated"
    let out_dir_str = resolve_output_dir(cli_output, config_output.as_str());

    // Solver choice: CLI flag > config file > default (Z3)
    let build_solver = resolve_solver(
        cli_solver,
        project.as_ref().map(|(c, _)| c.verify.smt_solver),
    );

    // Target: CLI flag > config file > default (native)
    let compile_target = resolve_target(
        cli_target,
        project.as_ref().map(|(c, _)| c.build.target.as_str()),
    );

    // Build unified compiler config
    let compiler_config = if let Some((ref proj, _)) = project {
        let mut cc = CompilerConfig::from_project(proj, _output_mode, verbosity);
        cc.verify.solver = build_solver;
        cc.codegen.output_dir = out_dir_str.to_string();
        cc
    } else {
        CompilerConfig {
            output_mode: _output_mode,
            verbosity,
            verify: assura_config::VerifyOptions {
                solver: build_solver,
                ..Default::default()
            },
            codegen: assura_config::CodegenConfig {
                output_dir: out_dir_str.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    };
    let config = project;

    // --- Project mode: detect directory ---
    let path = Path::new(filename);
    if path.is_dir() {
        run_build_project(path, verbosity, out_dir_str, compile_target, no_check);
        return;
    }

    let source = fs::read_to_string(filename).unwrap_or_else(|e| {
        eprintln!("Error: {filename}: {e}");
        process::exit(2);
    });

    // --- Run shared pipeline ---
    let output = compile_with_config(&source, filename, &compiler_config);
    crate::timing::print_pipeline_timing(
        &output,
        crate::timing::TimingOptions {
            filename,
            output_mode: OutputMode::Human,
            verbosity,
            project: config.as_ref().map(|(cfg, root)| {
                (
                    cfg.package.name.as_str(),
                    cfg.package.version.as_str(),
                    root.as_path(),
                )
            }),
            config_line: config.as_ref().map(|(cfg, _)| {
                format!(
                    "config: output={}, target={}, solver={}, timeout={}ms",
                    cfg.build.output, cfg.build.target, cfg.verify.smt_solver, cfg.verify.timeout
                )
            }),
            verify_ms: None,
            show_total: false,
            show_phase_failures: false,
        },
    );
    let CompilationResult {
        diagnostics,
        has_errors,
        typed,
        timing: phase_timing,
        file: parsed_file,
        ..
    } = output;

    // Report errors in human mode
    if has_errors {
        assura_diagnostics::report_diagnostics_human(&diagnostics, filename, &source);
        eprintln!("{filename}: {} error(s) found", diagnostics.len());
        process::exit(1);
    }

    let typed = typed.expect("type check should succeed if has_errors is false");

    // --- Quantifier bound validation ---
    let qwarnings = assura_smt::validate_quantifier_bounds(&typed);
    if verbosity != Verbosity::Quiet {
        for w in &qwarnings {
            eprintln!(
                "warning: unbounded quantifier in {}: {} ({})",
                w.context, w.domain_desc, w.reason
            );
        }
    }

    // --- Verify ---
    let verify_start = Instant::now();
    let verify_config = assura_config::CompilerConfig {
        verify: assura_config::VerifyOptions {
            solver: build_solver,
            ..Default::default()
        },
        ..Default::default()
    };
    let verification_results = assura_pipeline::verify_typed(&typed, filename, &verify_config);
    let verify_ms = verify_start.elapsed().as_secs_f64() * 1000.0;

    if verbosity == Verbosity::Verbose {
        eprintln!(
            "  verify:    {} clause(s) ({verify_ms:.2}ms)",
            verification_results.len()
        );
    }

    if verbosity != Verbosity::Quiet && !verification_results.is_empty() {
        eprintln!();
        eprintln!("Verification ({} clause(s)):", verification_results.len());
        let _ = assura_smt::display::write_grouped_verification(
            &mut std::io::stderr(),
            &verification_results,
            "  ",
        );
    }

    // --- Codegen ---
    let codegen_start = Instant::now();
    let backend_config = assura_codegen::BackendConfig {
        target: compile_target.clone(),
        ..assura_codegen::BackendConfig::default()
    };
    let project = assura_codegen::codegen_with_config(&typed, &backend_config);

    // --- Write to output directory ---
    let out_dir = Path::new(out_dir_str);
    fs::create_dir_all(out_dir).unwrap_or_else(|e| {
        eprintln!("Error: cannot create {out_dir_str}/ directory: {e}");
        process::exit(1);
    });

    let codegen_ms = codegen_start.elapsed().as_secs_f64() * 1000.0;
    if verbosity == Verbosity::Verbose {
        eprintln!(
            "  codegen:   {} file(s) ({codegen_ms:.2}ms)",
            project.files.len()
        );
        let total = phase_timing.parse_ms
            + phase_timing.resolve_ms.unwrap_or(0.0)
            + phase_timing.typecheck_ms.unwrap_or(0.0)
            + verify_ms
            + codegen_ms;
        eprintln!("  total:     {total:.2}ms");
        eprintln!();
    }

    // Write Cargo.toml
    let cargo_path = out_dir.join("Cargo.toml");
    fs::write(&cargo_path, &project.cargo_toml).unwrap_or_else(|e| {
        eprintln!("Error: cannot write {}: {e}", cargo_path.display());
        process::exit(1);
    });
    if verbosity != Verbosity::Quiet {
        println!("  wrote {}", cargo_path.display());
    }

    // Write stub IR sidecars next to source ({parent}/generated/{Name}.ir)
    let ir_dir = std::path::Path::new(filename)
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("generated");
    if fs::create_dir_all(&ir_dir).is_ok() {
        for (name, ir_text) in assura_smt::stub_ir_sidecars_for_typed(&typed) {
            let ir_path = ir_dir.join(format!("{name}.ir"));
            if fs::write(&ir_path, ir_text).is_ok() && verbosity != Verbosity::Quiet {
                println!("  wrote {}", ir_path.display());
            }
        }
    }

    // Write source files
    for (rel_path, content) in &project.files {
        let full_path = out_dir.join(rel_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|e| {
                eprintln!("Error: cannot create directory {}: {e}", parent.display());
                process::exit(1);
            });
        }
        fs::write(&full_path, content).unwrap_or_else(|e| {
            eprintln!("Error: cannot write {}: {e}", full_path.display());
            process::exit(1);
        });
        if verbosity != Verbosity::Quiet {
            println!("  wrote {}", full_path.display());
        }
    }

    // Write sidecar metadata JSON
    if let Some(ref meta) = project.metadata {
        let json_path = out_dir.join("assura-contracts.json");
        if let Ok(json) = serde_json::to_string_pretty(meta) {
            if let Err(e) = fs::write(&json_path, &json) {
                eprintln!("Warning: could not write {}: {e}", json_path.display());
            } else if verbosity != Verbosity::Quiet {
                println!("  wrote {}", json_path.display());
            }
        }
    }

    // --- Generate .cargo/config.toml for WASM target ---
    if matches!(compile_target, assura_codegen::CompileTarget::Wasm) {
        let cargo_dir = out_dir.join(".cargo");
        fs::create_dir_all(&cargo_dir).unwrap_or_else(|e| {
            eprintln!("Error: cannot create {}: {e}", cargo_dir.display());
            process::exit(1);
        });
        let config_toml = cargo_dir.join("config.toml");
        fs::write(&config_toml, "[build]\ntarget = \"wasm32-wasip1\"\n").unwrap_or_else(|e| {
            eprintln!("Error: cannot write {}: {e}", config_toml.display());
            process::exit(1);
        });
        if verbosity != Verbosity::Quiet {
            println!("  wrote {}", config_toml.display());
        }
    }

    // --- Auto-generate tests for timeout/unknown verification results ---
    let has_unresolved = verification_results.iter().any(|r| {
        matches!(
            r,
            assura_smt::VerificationResult::Timeout { .. }
                | assura_smt::VerificationResult::Unknown { .. }
        )
    });
    if has_unresolved && let Some(ref pf) = parsed_file {
        let mut test_gen = assura_types::TestGenerator::new();
        for spanned in &pf.decls {
            if let Decl::Contract(c) = &spanned.node {
                let mut params = Vec::new();
                let mut requires = Vec::new();
                let mut ensures = Vec::new();
                for clause in &c.clauses {
                    match &clause.kind {
                        ClauseKind::Input => {
                            let parsed = assura_parser::ast::extract_clause_params(&clause.body);
                            for p in parsed {
                                let ty = typed
                                    .type_env
                                    .lookup(&p.name)
                                    .cloned()
                                    .unwrap_or(assura_types::Type::Unknown);
                                params.push((p.name, ty));
                            }
                        }
                        ClauseKind::Requires => {
                            requires.push(assura_codegen::expr_to_rust_static(&clause.body));
                        }
                        ClauseKind::Ensures => {
                            ensures.push(assura_codegen::expr_to_rust_static(&clause.body));
                        }
                        _ => {}
                    }
                }
                if !params.is_empty() || !ensures.is_empty() {
                    test_gen.add_contract(assura_types::TestableContract {
                        name: c.name.clone(),
                        params,
                        requires,
                        ensures,
                    });
                }
            }
        }
        let tests = test_gen.generate_all();
        if !tests.is_empty() {
            let tests_dir = out_dir.join("tests");
            fs::create_dir_all(&tests_dir).ok();
            let test_file = tests_dir.join("generated_tests.rs");
            let mut content = String::from(
                "// Auto-generated tests (SMT verification returned timeout/unknown)\nuse proptest::prelude::*;\n\n",
            );
            for t in &tests {
                content.push_str(&t.body);
                content.push_str("\n\n");
            }
            if fs::write(&test_file, &content).is_ok() && verbosity != Verbosity::Quiet {
                println!(
                    "  wrote {} ({} tests for unresolved contracts)",
                    test_file.display(),
                    tests.len()
                );
            }
        }
    }

    // --- Build or check the generated Rust project ---
    let skip_check = no_check;
    if !skip_check {
        // WASM targets get `cargo build` to produce a .wasm file;
        // native targets get `cargo check` for fast validation.
        let is_wasm = matches!(compile_target, assura_codegen::CompileTarget::Wasm);
        let cargo_verb = if is_wasm { "build" } else { "check" };

        let mut cmd = process::Command::new("cargo");
        cmd.arg(cargo_verb).current_dir(out_dir);
        if let Some(triple) = compile_target.rust_target() {
            cmd.arg("--target").arg(triple);
        }
        let cargo_result = cmd
            .stdout(process::Stdio::piped())
            .stderr(process::Stdio::piped())
            .output();

        match cargo_result {
            Ok(output) if output.status.success() => {
                if is_wasm {
                    // Report the .wasm artifact path
                    let wasm_dir = out_dir.join("target/wasm32-wasip1/debug");
                    let wasm_file = find_wasm_artifact(&wasm_dir);
                    if let Some(ref wf) = wasm_file {
                        let size = fs::metadata(wf).map(|m| m.len()).unwrap_or(0);
                        if verbosity != Verbosity::Quiet {
                            println!("OK  {filename} -> {} ({} bytes)", wf.display(), size);
                        }
                    } else if verbosity != Verbosity::Quiet {
                        println!(
                            "OK  {filename} -> {out_dir_str}/ (WASM build succeeded, artifact in target/)"
                        );
                    }
                } else if verbosity != Verbosity::Quiet {
                    println!("OK  {filename} -> {out_dir_str}/ (generated Rust compiles)");
                }
            }
            Ok(output) => {
                if verbosity != Verbosity::Quiet {
                    println!("OK  {filename} -> {out_dir_str}/");
                }
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!();
                eprintln!("warning: generated Rust does not {cargo_verb}:");
                // Show only the error lines, not the full cargo output
                for line in stderr.lines() {
                    if line.starts_with("error") || line.contains("-->") {
                        eprintln!("  {line}");
                    }
                }
                eprintln!();
                eprintln!("  Run `cd {out_dir_str} && cargo {cargo_verb}` to see full errors.");
                eprintln!("  Use `--no-check` to skip this validation.");
            }
            Err(_) => {
                // cargo not found or other OS error; skip silently
                if verbosity != Verbosity::Quiet {
                    println!(
                        "OK  {filename} -> {out_dir_str}/ (cargo {cargo_verb} skipped: cargo not found)"
                    );
                }
            }
        }
    } else if verbosity != Verbosity::Quiet {
        println!("OK  {filename} -> {out_dir_str}/ (check skipped)");
    }
}

/// Find the first `.wasm` file in a directory (for WASM build output).
pub(crate) fn find_wasm_artifact(dir: &Path) -> Option<std::path::PathBuf> {
    let rd = fs::read_dir(dir).ok()?;
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "wasm") {
            return Some(path);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Config resolution helpers (CLI flag > config file > default)
// ---------------------------------------------------------------------------

/// Resolve the output directory.
///
/// Priority: CLI flag (when it differs from the `"generated"` default) >
/// config file value > `"generated"`.
pub(crate) fn resolve_output_dir<'a>(cli_output: &'a str, config_output: &'a str) -> &'a str {
    if cli_output != "generated" {
        cli_output
    } else {
        config_output
    }
}

/// Resolve the SMT solver.
///
/// Priority: CLI flag > config file > Z3 (default).
pub(crate) fn resolve_solver(
    cli_solver: Option<assura_smt::SolverChoice>,
    config_solver: Option<assura_smt::SolverChoice>,
) -> assura_smt::SolverChoice {
    cli_solver
        .or(config_solver)
        .unwrap_or(assura_smt::SolverChoice::Z3)
}

/// Resolve the compile target.
///
/// Priority: CLI flag > config file (parsed via `from_str_loose`) > Native (default).
pub(crate) fn resolve_target(
    cli_target: Option<assura_codegen::CompileTarget>,
    config_target: Option<&str>,
) -> assura_codegen::CompileTarget {
    cli_target
        .or_else(|| config_target.and_then(assura_codegen::CompileTarget::from_str_loose))
        .unwrap_or(assura_codegen::CompileTarget::Native)
}

// ---------------------------------------------------------------------------
// Project-mode build: resolve, type-check, and codegen all .assura files
// ---------------------------------------------------------------------------

pub(crate) fn run_build_project(
    project_dir: &Path,
    verbosity: Verbosity,
    output_dir: &str,
    target: assura_codegen::CompileTarget,
    no_check: bool,
) {
    let (project_root, dep_map, dep_warnings) = load_project_deps(project_dir);

    eprintln!("Building project at {}", project_root.display());
    for w in &dep_warnings {
        eprintln!("Warning: {w}");
    }

    let (resolved_files, warnings) =
        match assura_resolve::discover_and_resolve_project_with_deps(&project_root, &dep_map) {
            Ok(pair) => pair,
            Err(errors) => {
                for e in &errors {
                    eprintln!("Error: {e}");
                }
                process::exit(1);
            }
        };

    for w in &warnings {
        eprintln!("Warning: {w}");
    }

    let mut all_typed = Vec::new();
    let mut has_errors = false;

    for (module_path, resolved) in &resolved_files {
        match assura_types::type_check(resolved) {
            Ok(typed) => {
                all_typed.push((module_path.clone(), typed));
                if verbosity == Verbosity::Verbose {
                    eprintln!("OK  {module_path}");
                }
            }
            Err(errors) => {
                has_errors = true;
                eprintln!("ERR {module_path}: {} error(s)", errors.len());
                for err in &errors {
                    eprintln!("  {}: {}", err.code, err.message);
                }
            }
        }
    }

    if has_errors {
        eprintln!("Build failed: type errors in project");
        process::exit(1);
    }

    // Generate code for each module
    let out_dir = Path::new(output_dir);
    let mut generated_files = 0usize;
    let mut cargo_toml_written = false;
    let backend_config = assura_codegen::BackendConfig {
        target,
        ..Default::default()
    };
    for (_module_path, typed) in &all_typed {
        let project = assura_codegen::codegen_with_config(typed, &backend_config);
        // Write Cargo.toml once
        if !cargo_toml_written {
            let cargo_path = out_dir.join("Cargo.toml");
            if let Some(parent) = cargo_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Err(e) = fs::write(&cargo_path, &project.cargo_toml) {
                eprintln!("Error writing {}: {e}", cargo_path.display());
                process::exit(1);
            }
            cargo_toml_written = true;
        }
        for (rel_path, content) in &project.files {
            let file_out = out_dir.join(rel_path);
            if let Some(parent) = file_out.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Err(e) = fs::write(&file_out, content) {
                eprintln!("Error writing {}: {e}", file_out.display());
                process::exit(1);
            }
            generated_files += 1;
        }

        // Write sidecar metadata JSON
        if let Some(ref meta) = project.metadata {
            let json_path = out_dir.join("assura-contracts.json");
            if let Ok(json) = serde_json::to_string_pretty(meta)
                && let Err(e) = fs::write(&json_path, json)
            {
                eprintln!("Warning: could not write {}: {e}", json_path.display());
            }
        }
    }

    eprintln!(
        "Generated {generated_files} file(s) in {}",
        out_dir.display()
    );

    // Optionally run cargo check on generated code
    if !no_check && out_dir.join("Cargo.toml").exists() {
        eprintln!("Running cargo check on generated code...");
        let status = std::process::Command::new("cargo")
            .arg("check")
            .current_dir(out_dir)
            .status();
        match status {
            Ok(s) if s.success() => {
                eprintln!("Generated code compiles successfully");
            }
            Ok(s) => {
                eprintln!(
                    "Generated code failed to compile (exit {})",
                    s.code().unwrap_or(-1)
                );
                process::exit(1);
            }
            Err(e) => {
                eprintln!("Failed to run cargo check: {e}");
            }
        }
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // find_wasm_artifact
    // ---------------------------------------------------------------

    #[test]
    fn build_find_wasm_artifact_returns_none_for_nonexistent_dir() {
        let result = find_wasm_artifact(Path::new(
            "/tmp/__assura_nonexistent_dir_build_test_98231__",
        ));
        assert!(result.is_none());
    }

    #[test]
    fn build_find_wasm_artifact_returns_none_for_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = find_wasm_artifact(dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn build_find_wasm_artifact_finds_wasm_file() {
        let dir = tempfile::tempdir().unwrap();
        let wasm_path = dir.path().join("output.wasm");
        fs::write(&wasm_path, b"fake wasm").unwrap();
        let result = find_wasm_artifact(dir.path());
        assert_eq!(result.unwrap().extension().unwrap(), "wasm");
    }

    #[test]
    fn build_find_wasm_artifact_ignores_non_wasm_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("lib.rs"), b"fn main() {}").unwrap();
        fs::write(dir.path().join("data.json"), b"{}").unwrap();
        let result = find_wasm_artifact(dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn build_find_wasm_artifact_picks_first_wasm_among_many() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.wasm"), b"w1").unwrap();
        fs::write(dir.path().join("b.wasm"), b"w2").unwrap();
        fs::write(dir.path().join("c.txt"), b"not wasm").unwrap();
        let result = find_wasm_artifact(dir.path());
        assert_eq!(result.unwrap().extension().unwrap(), "wasm");
    }

    // ---------------------------------------------------------------
    // resolve_output_dir
    // ---------------------------------------------------------------

    #[test]
    fn build_resolve_output_dir_cli_overrides_config() {
        // CLI flag is not the default "generated", so CLI wins.
        assert_eq!(
            resolve_output_dir("custom_out", "from_config"),
            "custom_out"
        );
    }

    #[test]
    fn build_resolve_output_dir_config_used_when_cli_is_default() {
        // CLI is "generated" (sentinel for "not specified"), so config wins.
        assert_eq!(
            resolve_output_dir("generated", "from_config"),
            "from_config"
        );
    }

    #[test]
    fn build_resolve_output_dir_both_default() {
        assert_eq!(resolve_output_dir("generated", "generated"), "generated");
    }

    #[test]
    fn build_resolve_output_dir_cli_empty_string_overrides() {
        // Even an empty CLI flag overrides (it differs from "generated").
        assert_eq!(resolve_output_dir("", "from_config"), "");
    }

    // ---------------------------------------------------------------
    // resolve_solver
    // ---------------------------------------------------------------

    #[test]
    fn build_resolve_solver_cli_overrides_config() {
        let result = resolve_solver(
            Some(assura_smt::SolverChoice::Cvc5),
            Some(assura_smt::SolverChoice::Z3),
        );
        assert_eq!(result, assura_smt::SolverChoice::Cvc5);
    }

    #[test]
    fn build_resolve_solver_config_used_when_cli_is_none() {
        let result = resolve_solver(None, Some(assura_smt::SolverChoice::Cvc5));
        assert_eq!(result, assura_smt::SolverChoice::Cvc5);
    }

    #[test]
    fn build_resolve_solver_default_z3() {
        let result = resolve_solver(None, None);
        assert_eq!(result, assura_smt::SolverChoice::Z3);
    }

    #[test]
    fn build_resolve_solver_portfolio_from_cli() {
        let result = resolve_solver(
            Some(assura_smt::SolverChoice::Portfolio),
            Some(assura_smt::SolverChoice::Z3),
        );
        assert_eq!(result, assura_smt::SolverChoice::Portfolio);
    }

    // ---------------------------------------------------------------
    // resolve_target
    // ---------------------------------------------------------------

    #[test]
    fn build_resolve_target_cli_overrides_config() {
        let result = resolve_target(Some(assura_codegen::CompileTarget::Wasm), Some("native"));
        assert_eq!(result, assura_codegen::CompileTarget::Wasm);
    }

    #[test]
    fn build_resolve_target_config_used_when_cli_is_none() {
        let result = resolve_target(None, Some("wasm"));
        assert_eq!(result, assura_codegen::CompileTarget::Wasm);
    }

    #[test]
    fn build_resolve_target_default_native() {
        let result = resolve_target(None, None);
        assert_eq!(result, assura_codegen::CompileTarget::Native);
    }

    #[test]
    fn build_resolve_target_unknown_config_falls_back_to_native() {
        // Config has an unrecognized target string; falls back to Native.
        let result = resolve_target(None, Some("riscv64"));
        assert_eq!(result, assura_codegen::CompileTarget::Native);
    }

    #[test]
    fn build_resolve_target_wasm32_wasi_alias() {
        let result = resolve_target(None, Some("wasm32-wasi"));
        assert_eq!(result, assura_codegen::CompileTarget::Wasm);
    }

    #[test]
    fn build_resolve_target_wasm32_wasip1_alias() {
        let result = resolve_target(None, Some("wasm32-wasip1"));
        assert_eq!(result, assura_codegen::CompileTarget::Wasm);
    }
}
