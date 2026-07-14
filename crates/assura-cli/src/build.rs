use super::*;
use assura_llm::LlmProvider;

// `assura build <file.assura>` — codegen to generated/
// ---------------------------------------------------------------------------

/// Resolved build configuration from CLI flags and assura.toml.
struct BuildConfig<'a> {
    out_dir_str: &'a str,
    solver: assura_smt::SolverChoice,
    compile_target: assura_codegen::CompileTarget,
    compiler_config: CompilerConfig,
    project: Option<(assura_config::ProjectConfig, std::path::PathBuf)>,
}

fn resolve_build_config<'a>(
    filename: &str,
    output_mode: OutputMode,
    verbosity: Verbosity,
    cli_output: &'a str,
    cli_target: Option<assura_codegen::CompileTarget>,
    cli_solver: Option<assura_smt::SolverChoice>,
    config_output_buf: &'a mut String,
) -> BuildConfig<'a> {
    let project = load_project_config(Path::new(filename));
    *config_output_buf = project
        .as_ref()
        .map(|(c, _)| c.build.output.clone())
        .unwrap_or_else(|| "generated".to_string());
    let out_dir_str = resolve_output_dir(cli_output, config_output_buf.as_str());
    let solver = resolve_solver(
        cli_solver,
        project.as_ref().map(|(c, _)| c.verify.smt_solver),
    );
    let compile_target = resolve_target(
        cli_target,
        project.as_ref().map(|(c, _)| c.build.target.as_str()),
    );
    let compiler_config = if let Some((ref proj, _)) = project {
        let mut cc = CompilerConfig::from_project(proj, output_mode, verbosity);
        cc.verify.solver = solver;
        cc.codegen.output_dir = out_dir_str.to_string();
        cc
    } else {
        CompilerConfig {
            output_mode,
            verbosity,
            verify: assura_config::VerifyOptions {
                solver,
                ..Default::default()
            },
            codegen: assura_config::CodegenConfig {
                output_dir: out_dir_str.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    };
    BuildConfig {
        out_dir_str,
        solver,
        compile_target,
        compiler_config,
        project,
    }
}

/// True when human-facing chatter (stdout/stderr progress) should print.
fn human_speak(output_mode: OutputMode, verbosity: Verbosity) -> bool {
    output_mode == OutputMode::Human && verbosity != Verbosity::Quiet
}

/// Run verification on a typed file and optionally print results.
/// Returns the verification results and elapsed time in milliseconds.
fn verify_and_print(
    typed: &assura_types::TypedFile,
    filename: &str,
    solver: assura_smt::SolverChoice,
    output_mode: OutputMode,
    verbosity: Verbosity,
) -> (Vec<assura_smt::VerificationResult>, f64) {
    let qwarnings = assura_smt::validate_quantifier_bounds(typed);
    if human_speak(output_mode, verbosity) {
        for w in &qwarnings {
            eprintln!(
                "warning: unbounded quantifier in {}: {} ({})",
                w.context, w.domain_desc, w.reason
            );
        }
    }

    let verify_start = Instant::now();
    let verify_config = assura_config::CompilerConfig {
        verify: assura_config::VerifyOptions {
            solver,
            ..Default::default()
        },
        ..Default::default()
    };
    let results = assura_pipeline::verify_typed(typed, filename, &verify_config);
    let verify_ms = verify_start.elapsed().as_secs_f64() * 1000.0;

    if output_mode == OutputMode::Human && verbosity == Verbosity::Verbose {
        eprintln!(
            "  verify:    {} clause(s) ({verify_ms:.2}ms)",
            results.len()
        );
    }
    if human_speak(output_mode, verbosity) && !results.is_empty() {
        eprintln!();
        eprintln!("Verification ({} clause(s)):", results.len());
        let _ =
            assura_smt::display::write_grouped_verification(&mut std::io::stderr(), &results, "  ");
    }
    (results, verify_ms)
}

/// Result of optional `cargo build` validation after codegen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CargoCheckStatus {
    Skipped,
    Ok,
    Failed,
    CargoNotFound,
}

impl CargoCheckStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Skipped => "skipped",
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::CargoNotFound => "cargo_not_found",
        }
    }
}

fn compile_target_label(target: &assura_codegen::CompileTarget) -> &'static str {
    match target {
        assura_codegen::CompileTarget::Native => "native",
        assura_codegen::CompileTarget::Wasm => "wasm",
    }
}

/// Options for `assura build`.
pub(crate) struct BuildOpts<'a> {
    pub filename: &'a str,
    pub output_mode: OutputMode,
    pub verbosity: Verbosity,
    pub cli_output: &'a str,
    pub cli_target: Option<assura_codegen::CompileTarget>,
    pub no_check: bool,
    pub cli_solver: Option<assura_smt::SolverChoice>,
    pub runtime_checks: bool,
    pub auto_implement: bool,
    pub write_ir: bool,
    pub bin: bool,
}

pub(crate) fn run_build(opts: BuildOpts<'_>) {
    let BuildOpts {
        filename,
        output_mode,
        verbosity,
        cli_output,
        cli_target,
        no_check,
        cli_solver,
        runtime_checks,
        auto_implement,
        write_ir,
        bin,
    } = opts;
    let mut config_output_buf = String::new();
    let bc = resolve_build_config(
        filename,
        output_mode,
        verbosity,
        cli_output,
        cli_target,
        cli_solver,
        &mut config_output_buf,
    );

    // Project mode: detect directory
    if Path::new(filename).is_dir() {
        run_build_project(
            Path::new(filename),
            output_mode,
            verbosity,
            bc.out_dir_str,
            bc.compile_target,
            no_check,
            runtime_checks,
        );
        return;
    }

    // Resolve the output directory relative to the input file's parent when
    // the output path is relative. This way `assura build /tmp/project/lib.assura`
    // writes to `/tmp/project/generated/` instead of `./generated/`.
    let resolved_out_dir = resolve_output_dir_for_file(bc.out_dir_str, filename);
    let out_dir_str_owned;
    let effective_out_dir_str = if let Some(ref resolved) = resolved_out_dir {
        out_dir_str_owned = resolved.to_string_lossy().to_string();
        out_dir_str_owned.as_str()
    } else {
        bc.out_dir_str
    };

    let source = fs::read_to_string(filename).unwrap_or_else(|e| {
        if output_mode == OutputMode::Json {
            let report = serde_json::json!({
                "ok": false,
                "file": filename,
                "error": format!("{e}"),
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!("Error: {filename}: {e}");
        }
        process::exit(2);
    });

    // Pipeline + timing
    let output = compile_with_config(&source, filename, &bc.compiler_config);
    crate::timing::print_pipeline_timing(
        &output,
        crate::timing::TimingOptions {
            filename,
            output_mode,
            verbosity,
            project: bc.project.as_ref().map(|(cfg, root)| {
                (
                    cfg.package.name.as_str(),
                    cfg.package.version.as_str(),
                    root.as_path(),
                )
            }),
            config_line: bc.project.as_ref().map(|(cfg, _)| {
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
    if has_errors {
        if output_mode == OutputMode::Json {
            let report = serde_json::json!({
                "ok": false,
                "file": filename,
                "diagnostics": diagnostics,
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            assura_diagnostics::report_diagnostics_human(&diagnostics, filename, &source);
            eprintln!("{filename}: {} error(s) found", diagnostics.len());
        }
        process::exit(1);
    }
    let typed = typed.expect("type check should succeed if has_errors is false");

    // Offline IR write (no LLM) before verify so co-located sidecars are loaded.
    if write_ir {
        write_colocated_ir_sidecars(&typed, filename, verbosity, output_mode);
    }

    // Verify
    let (verification_results, verify_ms) =
        verify_and_print(&typed, filename, bc.solver, output_mode, verbosity);

    // Prefer co-located IR sidecars for codegen bodies (#866).
    // `--write-ir` runs above, so sidecars are already on disk for this load.
    // `--auto-implement`: offline heuristics first (no API key), then LLM only
    // for remaining contracts so synthesizable shapes never need the model.
    let ir_bodies = if auto_implement {
        let mut bodies = rust_bodies_from_ir_sidecars(&typed, filename, verbosity, output_mode);
        let offline = offline_heuristic_rust_bodies(&typed, verbosity, output_mode);
        for (name, body) in offline {
            bodies.entry(name).or_insert(body);
        }
        let already: std::collections::HashSet<String> = bodies.keys().cloned().collect();
        let ai_config = bc
            .project
            .as_ref()
            .map(|(p, _)| p.ai.clone())
            .unwrap_or_default();
        let llm = auto_implement_contracts(
            &typed,
            filename,
            &bc.compiler_config,
            verbosity,
            output_mode,
            &ai_config,
            &already,
        );
        for (name, body) in llm {
            bodies.insert(name, body);
        }
        bodies
    } else {
        rust_bodies_from_ir_sidecars(&typed, filename, verbosity, output_mode)
    };

    // Codegen
    let codegen_start = Instant::now();
    let backend_config = assura_codegen::BackendConfig {
        target: bc.compile_target.clone(),
        runtime_checks,
        ir_bodies,
        ..assura_codegen::BackendConfig::default()
    };
    let mut project = assura_codegen::codegen_with_config(&typed, &backend_config);
    if bin {
        inject_bin_main(&mut project, &typed, verbosity, output_mode);
    }

    // Write output
    let out_dir = Path::new(effective_out_dir_str);
    fs::create_dir_all(out_dir).unwrap_or_else(|e| {
        if output_mode == OutputMode::Json {
            let report = serde_json::json!({
                "ok": false,
                "file": filename,
                "error": format!("cannot create {}/: {e}", effective_out_dir_str),
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!(
                "Error: cannot create {}/ directory: {e}",
                effective_out_dir_str
            );
        }
        process::exit(1);
    });
    let codegen_ms = codegen_start.elapsed().as_secs_f64() * 1000.0;
    if output_mode == OutputMode::Human && verbosity == Verbosity::Verbose {
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

    let mut written = write_generated_project(
        filename,
        out_dir,
        &project,
        &typed,
        &bc.compile_target,
        verbosity,
        output_mode,
    );
    written.extend(write_unresolved_tests(
        out_dir,
        &verification_results,
        parsed_file.as_ref(),
        &typed,
        verbosity,
        output_mode,
    ));
    let cargo_check = run_cargo_build(
        filename,
        effective_out_dir_str,
        out_dir,
        &bc.compile_target,
        no_check,
        verbosity,
        output_mode,
    );

    if output_mode == OutputMode::Json {
        let verification_json: Vec<serde_json::Value> = verification_results
            .iter()
            .map(assura_smt::VerificationResult::to_json_value)
            .collect();
        let report = serde_json::json!({
            "ok": true,
            "file": filename,
            "output_dir": effective_out_dir_str,
            "target": compile_target_label(&bc.compile_target),
            "files": written,
            "verification": verification_json,
            "cargo_check": cargo_check.as_str(),
            "no_check": no_check,
        });
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    }
}

// ---------------------------------------------------------------------------
// Extracted subfunctions for run_build
// ---------------------------------------------------------------------------

/// Write the generated Rust project to the output directory: Cargo.toml,
/// source files, IR sidecars, metadata JSON, and .cargo/config.toml for WASM.
/// Returns paths written (for JSON reporting).
fn write_generated_project(
    filename: &str,
    out_dir: &Path,
    project: &assura_codegen::GeneratedProject,
    typed: &assura_types::TypedFile,
    compile_target: &assura_codegen::CompileTarget,
    verbosity: Verbosity,
    output_mode: OutputMode,
) -> Vec<String> {
    let speak = human_speak(output_mode, verbosity);
    let mut written = Vec::new();

    // Cargo.toml
    let cargo_path = out_dir.join("Cargo.toml");
    fs::write(&cargo_path, &project.cargo_toml).unwrap_or_else(|e| {
        if output_mode == OutputMode::Json {
            let report = serde_json::json!({
                "ok": false,
                "file": filename,
                "error": format!("cannot write {}: {e}", cargo_path.display()),
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!("Error: cannot write {}: {e}", cargo_path.display());
        }
        process::exit(1);
    });
    written.push(cargo_path.display().to_string());
    if speak {
        println!("  wrote {}", cargo_path.display());
    }

    // Stub IR sidecars into a `generated/` directory next to the source file.
    // When the output dir was resolved relative to the input file, reuse that
    // parent so IR files land beside the generated Rust.
    let ir_dir = std::path::Path::new(filename)
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("generated");
    if fs::create_dir_all(&ir_dir).is_ok() {
        for (name, ir_text) in assura_smt::stub_ir_sidecars_for_typed(typed) {
            // Same policy as --write-ir co-located: never persist identity stubs.
            if ir_text.contains("Stub IR") {
                continue;
            }
            let ir_path = ir_dir.join(format!("{name}.ir"));
            if fs::write(&ir_path, &ir_text).is_ok() {
                written.push(ir_path.display().to_string());
                if speak {
                    println!("  wrote {}", ir_path.display());
                }
            }
        }
    }

    // Source files
    for (rel_path, content) in &project.files {
        let full_path = out_dir.join(rel_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|e| {
                if output_mode == OutputMode::Json {
                    let report = serde_json::json!({
                        "ok": false,
                        "file": filename,
                        "error": format!("cannot create directory {}: {e}", parent.display()),
                    });
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    eprintln!("Error: cannot create directory {}: {e}", parent.display());
                }
                process::exit(1);
            });
        }
        fs::write(&full_path, content).unwrap_or_else(|e| {
            if output_mode == OutputMode::Json {
                let report = serde_json::json!({
                    "ok": false,
                    "file": filename,
                    "error": format!("cannot write {}: {e}", full_path.display()),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                eprintln!("Error: cannot write {}: {e}", full_path.display());
            }
            process::exit(1);
        });
        written.push(full_path.display().to_string());
        if speak {
            println!("  wrote {}", full_path.display());
        }
    }

    // Sidecar metadata JSON
    if let Some(ref meta) = project.metadata {
        let json_path = out_dir.join("assura-contracts.json");
        if let Ok(json) = serde_json::to_string_pretty(meta) {
            if let Err(e) = fs::write(&json_path, &json) {
                if speak {
                    eprintln!("Warning: could not write {}: {e}", json_path.display());
                }
            } else {
                written.push(json_path.display().to_string());
                if speak {
                    println!("  wrote {}", json_path.display());
                }
            }
        }
    }

    // .cargo/config.toml for WASM target
    if matches!(compile_target, assura_codegen::CompileTarget::Wasm) {
        let cargo_dir = out_dir.join(".cargo");
        fs::create_dir_all(&cargo_dir).unwrap_or_else(|e| {
            if output_mode == OutputMode::Json {
                let report = serde_json::json!({
                    "ok": false,
                    "file": filename,
                    "error": format!("cannot create {}: {e}", cargo_dir.display()),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                eprintln!("Error: cannot create {}: {e}", cargo_dir.display());
            }
            process::exit(1);
        });
        let config_toml = cargo_dir.join("config.toml");
        fs::write(&config_toml, "[build]\ntarget = \"wasm32-wasip1\"\n").unwrap_or_else(|e| {
            if output_mode == OutputMode::Json {
                let report = serde_json::json!({
                    "ok": false,
                    "file": filename,
                    "error": format!("cannot write {}: {e}", config_toml.display()),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                eprintln!("Error: cannot write {}: {e}", config_toml.display());
            }
            process::exit(1);
        });
        written.push(config_toml.display().to_string());
        if speak {
            println!("  wrote {}", config_toml.display());
        }
    }
    written
}

/// Auto-generate proptest tests for contracts whose verification was
/// timeout or unknown. Returns paths written (for JSON reporting).
fn write_unresolved_tests(
    out_dir: &Path,
    verification_results: &[assura_smt::VerificationResult],
    parsed_file: Option<&assura_parser::ast::SourceFile>,
    typed: &assura_types::TypedFile,
    verbosity: Verbosity,
    output_mode: OutputMode,
) -> Vec<String> {
    let has_unresolved = verification_results.iter().any(|r| {
        matches!(
            r,
            assura_smt::VerificationResult::Timeout { .. }
                | assura_smt::VerificationResult::Unknown { .. }
        )
    });
    let Some(pf) = parsed_file else {
        return Vec::new();
    };
    if !has_unresolved {
        return Vec::new();
    }

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
    let mut written = Vec::new();
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
        if fs::write(&test_file, &content).is_ok() {
            written.push(test_file.display().to_string());
            if human_speak(output_mode, verbosity) {
                println!(
                    "  wrote {} ({} tests for unresolved contracts)",
                    test_file.display(),
                    tests.len()
                );
            }
        }
    }
    written
}

/// Run `cargo build` on the generated project and report results.
fn run_cargo_build(
    filename: &str,
    out_dir_str: &str,
    out_dir: &Path,
    compile_target: &assura_codegen::CompileTarget,
    no_check: bool,
    verbosity: Verbosity,
    output_mode: OutputMode,
) -> CargoCheckStatus {
    let speak = human_speak(output_mode, verbosity);
    if no_check {
        if speak {
            println!("OK  {filename} -> {out_dir_str}/ (check skipped)");
        }
        return CargoCheckStatus::Skipped;
    }

    let is_wasm = matches!(compile_target, assura_codegen::CompileTarget::Wasm);
    let mut cmd = process::Command::new("cargo");
    cmd.arg("build").current_dir(out_dir);
    cmd.env_remove("RUSTC_WRAPPER");
    cmd.env_remove("CARGO_TARGET_DIR");
    if let Some(triple) = compile_target.rust_target() {
        cmd.arg("--target").arg(triple);
    }
    let cargo_result = cmd
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .output();

    match cargo_result {
        Ok(output) if output.status.success() => {
            if speak {
                report_build_success(filename, out_dir, out_dir_str, is_wasm);
            }
            CargoCheckStatus::Ok
        }
        Ok(output) => {
            if speak {
                println!("OK  {filename} -> {out_dir_str}/");
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!();
                eprintln!("warning: generated Rust does not compile:");
                for line in stderr.lines() {
                    if line.starts_with("error") || line.contains("-->") {
                        eprintln!("  {line}");
                    }
                }
                eprintln!();
                eprintln!("  Run `cd {out_dir_str} && cargo build` to see full errors.");
                eprintln!("  Use `--no-check` to skip this validation.");
            }
            CargoCheckStatus::Failed
        }
        Err(_) => {
            if speak {
                println!("OK  {filename} -> {out_dir_str}/ (cargo build skipped: cargo not found)");
            }
            CargoCheckStatus::CargoNotFound
        }
    }
}

fn report_build_success(filename: &str, out_dir: &Path, out_dir_str: &str, is_wasm: bool) {
    if is_wasm {
        let wasm_dir = out_dir.join("target/wasm32-wasip1/debug");
        if let Some(ref wf) = find_wasm_artifact(&wasm_dir) {
            let size = fs::metadata(wf).map(|m| m.len()).unwrap_or(0);
            println!("OK  {filename} -> {} ({} bytes)", wf.display(), size);
        } else {
            println!(
                "OK  {filename} -> {out_dir_str}/ (WASM build succeeded, artifact in target/)"
            );
        }
    } else {
        let native_dir = out_dir.join("target/debug");
        if let Some(ref nf) = find_native_artifact(&native_dir) {
            let size = fs::metadata(nf).map(|m| m.len()).unwrap_or(0);
            println!("OK  {filename} -> {} ({} bytes)", nf.display(), size);
        } else {
            println!("OK  {filename} -> {out_dir_str}/ (native build succeeded)");
        }
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

/// Find a native build artifact (`.rlib`) in the `deps/` subdirectory.
///
/// Cargo places library artifacts as `lib{crate_name}-{hash}.rlib` in `deps/`.
/// Returns the first `.rlib` file found (the generated project is the only
/// crate built in that directory).
pub(crate) fn find_native_artifact(dir: &Path) -> Option<std::path::PathBuf> {
    let deps_dir = dir.join("deps");
    let rd = fs::read_dir(&deps_dir).ok()?;
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "rlib") {
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

/// When using the **default** output name (`generated`), resolve it next to
/// the input file so `assura build /tmp/project/lib.assura` writes to
/// `/tmp/project/generated/` instead of `./generated/`.
///
/// Any other relative `--output` (including single-segment names like
/// `covproj` or multi-segment `myproj/gen`) stays CWD-relative. Dogfood:
/// `build contracts/lib.assura --output covproj` must not write
/// `contracts/covproj/`.
///
/// Returns `None` if the path should not be rewritten.
fn resolve_output_dir_for_file(out_dir_str: &str, filename: &str) -> Option<std::path::PathBuf> {
    let out_path = Path::new(out_dir_str);
    if out_path.is_absolute() {
        return None;
    }
    // Only rewrite the CLI/config default directory name.
    if out_dir_str != "generated" {
        return None;
    }
    let input_parent = Path::new(filename).parent()?;
    if input_parent == Path::new("") || input_parent == Path::new(".") {
        return None;
    }
    Some(input_parent.join("generated"))
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
// Auto-implement: call an LLM to generate IR implementations for contracts
// ---------------------------------------------------------------------------

/// Use the configured LLM provider to generate IR implementations for
/// each verifiable declaration. Returns a map of declaration name to Rust
/// body code, suitable for `BackendConfig.ir_bodies`.
fn auto_implement_contracts(
    typed: &assura_types::TypedFile,
    source_path: &str,
    config: &CompilerConfig,
    verbosity: Verbosity,
    output_mode: OutputMode,
    ai_config: &assura_config::AiConfig,
    skip_names: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, String> {
    let mut ir_bodies = std::collections::HashMap::new();
    let speak = human_speak(output_mode, verbosity);

    let contexts = assura_smt::ir_prompt_contexts_for_typed(typed, Some(Path::new(source_path)));
    if contexts.is_empty() {
        return ir_bodies;
    }

    for ctx in &contexts {
        if skip_names.contains(&ctx.decl_name) {
            if speak && verbosity == Verbosity::Verbose {
                eprintln!(
                    "  auto-implement: skip `{}` (offline heuristic / co-located IR)",
                    ctx.decl_name
                );
            }
            continue;
        }
        if speak {
            eprint!("  auto-implement {}...", ctx.decl_name);
        }

        let prompt = assura_smt::render_ir_prompt(ctx, assura_smt::IrPromptPattern::Auto);
        // Build a single-contract source for verification.
        // verify_ir validates the IR against the first contract it finds,
        // so we must pass just this contract (not the full multi-contract file).
        let single_contract_source = build_single_contract_source(ctx);
        // Suppress LLM progress chatter under --json (agents parse stdout only).
        let llm_verbosity = if output_mode == OutputMode::Json {
            Verbosity::Quiet
        } else {
            verbosity
        };
        match call_llm_for_ir(
            &ctx.decl_name,
            &prompt,
            &single_contract_source,
            config,
            llm_verbosity,
            &ctx.params,
            ai_config,
        ) {
            Some(rust_body) => {
                if speak {
                    eprintln!(" ok");
                }
                ir_bodies.insert(ctx.decl_name.clone(), rust_body);
            }
            None => {
                if speak {
                    eprintln!(" failed (will use todo!())");
                }
            }
        }
    }

    if speak {
        let total = contexts.len();
        let ok = ir_bodies.len();
        eprintln!("  auto-implement: {ok}/{total} contracts implemented by LLM");
    }

    ir_bodies
}

/// Call the configured LLM with the IR prompt and validate the response.
/// Retries up to 3 times on parse/validation failure.
fn call_llm_for_ir(
    _decl_name: &str,
    base_prompt: &str,
    contract_source: &str,
    config: &CompilerConfig,
    verbosity: Verbosity,
    params: &[assura_parser::ast::Param],
    ai_config: &assura_config::AiConfig,
) -> Option<String> {
    const MAX_RETRIES: usize = 3;
    let mut prompt = base_prompt.to_string();

    for attempt in 0..MAX_RETRIES {
        let raw_response = if ai_config.mode == "cli" {
            // CLI mode: shell out to the configured command
            let cmd = ai_config.command.as_deref().unwrap_or("claude");
            let args: Vec<String> = if ai_config.args.is_empty() {
                vec!["-p".to_string(), prompt.clone()]
            } else {
                ai_config
                    .args
                    .iter()
                    .map(|a| a.replace("{prompt}", &prompt))
                    .collect()
            };
            let output = process::Command::new(cmd)
                .args(&args)
                .stdout(process::Stdio::piped())
                .stderr(process::Stdio::piped())
                .output();

            match output {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
                Ok(o) => {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    if verbosity == Verbosity::Verbose {
                        eprintln!(
                            "\n    attempt {}: {cmd} failed: {}",
                            attempt + 1,
                            stderr.lines().next().unwrap_or("unknown error")
                        );
                    }
                    continue;
                }
                Err(e) => {
                    if attempt == 0 {
                        eprintln!(
                            "\n    error: '{cmd}' not found ({e}). \
                             Set [ai] command in assura.toml or use mode = \"api\".",
                        );
                    }
                    return None;
                }
            }
        } else {
            // API mode: use assura-llm HttpProvider
            let llm_config =
                assura_llm::LlmConfig::from_provider(&ai_config.provider, Some(&ai_config.model));
            let provider = match assura_llm::HttpProvider::new(llm_config) {
                Ok(p) => p,
                Err(e) => {
                    if attempt == 0 {
                        eprintln!(
                            "\n    error: LLM provider setup failed: {e}\n    \
                             Set the API key env var or configure [ai] in assura.toml.",
                        );
                    }
                    return None;
                }
            };
            match provider.call_raw(
                "You are an Assura IR code generator. Output only the IR code, no explanation.",
                &prompt,
            ) {
                Ok(resp) => resp,
                Err(e) => {
                    if verbosity == Verbosity::Verbose {
                        eprintln!("\n    attempt {}: LLM call failed: {e}", attempt + 1);
                    }
                    continue;
                }
            }
        };
        let ir_text = strip_markdown_fences(&raw_response);

        // Try to parse the IR
        match assura_smt::parse_ir_module(&ir_text) {
            Ok(module) => {
                // Validate against contract
                let verify_result = assura_pipeline::verify_ir(contract_source, &ir_text, config);
                // Accept the IR if verification passed or all clauses are
                // verified/unknown (no counterexamples found). Unknown results
                // with known SMT limitations are acceptable since the
                // implementation isn't provably wrong.
                let has_counterexample = verify_result
                    .clauses
                    .iter()
                    .any(|c| c.status == "counterexample" || c.status == "failed");
                if !has_counterexample && verify_result.status != "error" {
                    // Convert IR to Rust body with parameter bindings
                    if let Some(func) = module.functions.first() {
                        let mut body = String::new();
                        // Map slot variables to actual parameter names
                        for (i, param) in params.iter().enumerate() {
                            body.push_str(&format!(
                                "    let slot_{i} = {name}.clone();\n",
                                name = param.name
                            ));
                        }
                        let ir_body = assura_smt::ir_function_body_to_rust(func);
                        // The IR codegen uses __result but assura-codegen uses
                        // __assura_result. Replace and strip the trailing bare
                        // return expression (codegen adds its own return after
                        // ensures checks).
                        let ir_body = ir_body.replace("__result", "__assura_result");
                        let ir_body = strip_trailing_return(&ir_body);
                        body.push_str(&ir_body);
                        return Some(body);
                    }
                }
                // Verification failed, retry with feedback
                let feedback = verify_result
                    .clauses
                    .iter()
                    .filter(|c| c.status != "verified")
                    .map(|c| {
                        if let Some(ref ce) = c.counterexample {
                            format!("COUNTEREXAMPLE for {}: {ce}", c.name)
                        } else if let Some(ref reason) = c.reason {
                            format!("UNKNOWN for {}: {reason}", c.name)
                        } else {
                            format!("{} for {}", c.status.to_uppercase(), c.name)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if verbosity == Verbosity::Verbose {
                    eprintln!(
                        "\n    attempt {}: verification {}, retrying",
                        attempt + 1,
                        verify_result.status
                    );
                    for c in &verify_result.clauses {
                        eprint!("      {} = {}", c.name, c.status);
                        if let Some(ref ce) = c.counterexample {
                            eprint!(" ({})", ce);
                        }
                        if let Some(ref reason) = c.reason {
                            eprint!(" ({})", reason);
                        }
                        eprintln!();
                    }
                    for e in &verify_result.ir_errors {
                        eprintln!("      IR error: {e}");
                    }
                    for e in &verify_result.validation_errors {
                        eprintln!("      validation error: {e}");
                    }
                }
                prompt = format!(
                    "{base_prompt}\n\n\
                     The previous IR was rejected by the SMT verifier:\n\
                     {feedback}\n\n\
                     Fix the IR body to satisfy all ensures clauses. \
                     Output ONLY the corrected .ir text, no markdown."
                );
            }
            Err(errors) => {
                let err_msg = errors.join("; ");
                if verbosity == Verbosity::Verbose {
                    eprintln!(
                        "\n    attempt {}: IR parse error: {}",
                        attempt + 1,
                        &err_msg[..err_msg.len().min(120)]
                    );
                }
                prompt = format!(
                    "{base_prompt}\n\n\
                     The previous response could not be parsed as valid IR:\n\
                     {err_msg}\n\n\
                     Output ONLY the .ir module text. No markdown fences, \
                     no commentary, no explanation. Just the IR starting with `module`."
                );
            }
        }
    }

    None
}

/// Build an Assura source string containing just one contract from a prompt context.
///
/// `verify_ir` validates IR against the first contract it finds in the source.
/// When the original file has multiple contracts, we need to extract just the
/// one being implemented so verification targets the right contract.
fn build_single_contract_source(ctx: &assura_smt::IrPromptContext) -> String {
    let mut src = format!("contract {} {{\n", ctx.decl_name);
    // Input clause with parameters
    if !ctx.params.is_empty() {
        let params: Vec<String> = ctx
            .params
            .iter()
            .map(|p| {
                let ty =
                    p.ty.as_ref()
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "Int".to_string());
                format!("{}: {}", p.name, ty)
            })
            .collect();
        src.push_str(&format!("    input({})\n", params.join(", ")));
    }
    // Output clause
    let ret = ctx.return_type_str();
    if ret != "Unit" {
        src.push_str(&format!("    output(result: {})\n", ret));
    }
    // Other clauses
    for clause in &ctx.clauses {
        let kind = match clause.kind {
            ClauseKind::Requires => "requires",
            ClauseKind::Ensures => "ensures",
            ClauseKind::Invariant => "invariant",
            _ => continue,
        };
        let body = expr_to_string(&clause.body);
        src.push_str(&format!("    {kind} {{ {body} }}\n"));
    }
    src.push_str("}\n");
    src
}

/// Write **analyzable** heuristic IR next to the source file (offline, no LLM).
///
/// Skips labeled stub placeholders (`Stub IR`). Writing identity stubs for
/// unanalyzable ensures used to poison co-located load/codegen (identity body
/// injected while ensures still demanded something else).
fn write_colocated_ir_sidecars(
    typed: &assura_types::TypedFile,
    source_path: &str,
    verbosity: Verbosity,
    output_mode: OutputMode,
) {
    let speak = human_speak(output_mode, verbosity);
    let parent = Path::new(source_path)
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let mut wrote = 0usize;
    let mut skipped_stub = 0usize;
    for (name, ir_text) in assura_smt::stub_ir_sidecars_for_typed(typed) {
        if ir_text.contains("Stub IR") {
            skipped_stub += 1;
            if output_mode == OutputMode::Human && verbosity == Verbosity::Verbose {
                eprintln!("  --write-ir: skip stub for `{name}` (ensures not auto-synthesizable)");
            }
            continue;
        }
        let path = parent.join(format!("{name}.ir"));
        if let Err(e) = fs::write(&path, &ir_text) {
            if speak {
                eprintln!("Warning: could not write {}: {e}", path.display());
            }
        } else {
            wrote += 1;
            if speak {
                println!("  wrote co-located IR {}", path.display());
            }
        }
    }
    if speak && wrote == 0 && skipped_stub > 0 {
        eprintln!(
            "  --write-ir: no analyzable ensures to materialize ({skipped_stub} stub(s) skipped); \
             try `assura build --auto-implement` (LLM) or hand-write {{Contract}}.ir"
        );
    }
}

/// In-memory IR→Rust bodies from ensures heuristics (no disk, no LLM).
///
/// Used by `--auto-implement` so synthesizable contracts never require an
/// API key. Same non-stub filter as `--write-ir`.
fn offline_heuristic_rust_bodies(
    typed: &assura_types::TypedFile,
    verbosity: Verbosity,
    output_mode: OutputMode,
) -> std::collections::HashMap<String, String> {
    let speak = human_speak(output_mode, verbosity);
    let contexts = assura_smt::ir_prompt_contexts_for_typed(typed, None);
    let mut out = std::collections::HashMap::new();
    for (name, ir_text) in assura_smt::stub_ir_sidecars_for_typed(typed) {
        if ir_text.contains("Stub IR") {
            continue;
        }
        let Ok(module) = assura_smt::parse_ir_module(&ir_text) else {
            continue;
        };
        if module.functions.is_empty() {
            continue;
        }
        let Some(ctx) = contexts.iter().find(|c| c.decl_name == name) else {
            continue;
        };
        let mut body = String::new();
        for (i, param) in ctx.params.iter().enumerate() {
            body.push_str(&format!(
                "    let slot_{i} = {name}.clone();\n",
                name = param.name
            ));
        }
        let ir_body = if module.functions.len() == 1 {
            assura_smt::ir_function_body_to_rust(&module.functions[0])
        } else {
            multi_block_ir_to_embedded_body(&module)
        };
        let ir_body = ir_body.replace("__result", "__assura_result");
        let ir_body = strip_trailing_return(&ir_body);
        body.push_str(&ir_body);
        out.insert(name, body);
    }
    if speak && !out.is_empty() {
        let names: Vec<&str> = out.keys().map(String::as_str).collect();
        eprintln!(
            "  codegen: offline heuristic IR for {} contract(s): {}",
            out.len(),
            names.join(", ")
        );
    }
    out
}

/// Add a `src/main.rs` binary entry that calls the primary contract/fn.
///
/// For `contract` declarations the codegen wraps them in `pub mod contract_<name>`
/// with `pub fn check(...)`. For `fn` declarations the function lives at crate
/// root as `pub fn <name>(...)`. This function inspects the AST to determine
/// the correct call path and generate default arguments matching the actual
/// parameter list.
fn inject_bin_main(
    project: &mut assura_codegen::GeneratedProject,
    typed: &assura_types::TypedFile,
    verbosity: Verbosity,
    output_mode: OutputMode,
) {
    // Find the first contract or fn declaration. Prefer contracts over fn
    // declarations because contracts always have defaultable params (either
    // from input() clauses or synthesized as i64 from free variables). Fn
    // declarations may have reference or custom types that can't be defaulted
    // in a standalone main.rs.
    let source = &typed.resolved.source;
    let mut primary_name: Option<String> = None;
    let mut is_contract = false;
    let mut param_types: Vec<(String, String)> = Vec::new();

    // Pass 1: look for contracts (use assura_ast only so crates.io package
    // verify still builds against published assura-codegen without new APIs).
    for decl in &source.decls {
        if let Decl::Contract(c) = &decl.node {
            primary_name = Some(c.name.clone());
            is_contract = true;
            param_types = bin_contract_params(c);
            break;
        }
    }

    // Pass 2: if no contract, look for a fn with defaultable params
    if primary_name.is_none() {
        for decl in &source.decls {
            if let Decl::FnDef(f) = &decl.node {
                if f.is_ghost || f.is_lemma {
                    continue;
                }
                let mut fn_params = Vec::new();
                let mut has_undefaultable = false;
                for p in &f.params {
                    let ty =
                        p.ty.as_ref()
                            .map(|t| bin_map_simple_type(&t.to_tokens().join(" ")))
                            .unwrap_or_else(|| "i64".to_string());
                    // Skip fn declarations with reference or custom types
                    if ty.contains('&') || ty == "()" {
                        has_undefaultable = true;
                        break;
                    }
                    fn_params.push((p.name.clone(), ty));
                }
                if !has_undefaultable {
                    primary_name = Some(f.name.clone());
                    is_contract = false;
                    param_types = fn_params;
                    break;
                }
            }
        }
    }

    let Some(primary) = primary_name else {
        if human_speak(output_mode, verbosity) {
            eprintln!("  --bin: no contracts found; skipping main.rs");
        }
        return;
    };

    let to_snake = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect()
    };

    // Package name from Cargo.toml (name = "...").
    let pkg = project
        .cargo_toml
        .lines()
        .find_map(|l| {
            let t = l.trim();
            t.strip_prefix("name = \"")
                .and_then(|r| r.strip_suffix('"'))
                .map(str::to_string)
        })
        .unwrap_or_else(|| "generated".into());

    let bin_name = to_snake(&primary);

    // Build default value declarations for each parameter.
    // First integer-like param may be overridden from argv (smoke: `cargo run -- 9`).
    let mut let_stmts = String::new();
    let mut call_args = String::new();
    let mut first_int_from_args = true;
    for (i, (name, ty)) in param_types.iter().enumerate() {
        let default = default_value_for_type(ty);
        let is_int = matches!(
            ty.as_str(),
            "i64" | "i32" | "i16" | "i8" | "u64" | "u32" | "u16" | "u8" | "i128" | "u128"
        );
        if first_int_from_args && is_int {
            let_stmts.push_str(&format!(
                "    let {name}: {ty} = std::env::args()\n        .nth(1)\n        .and_then(|s| s.parse().ok())\n        .unwrap_or({default});\n"
            ));
            first_int_from_args = false;
        } else {
            let_stmts.push_str(&format!("    let {name}: {ty} = {default};\n"));
        }
        if i > 0 {
            call_args.push_str(", ");
        }
        call_args.push_str(name);
    }

    // Build the function call expression.
    let call_expr = if is_contract {
        let mod_name = format!("contract_{}", to_snake(&primary));
        format!("{mod_name}::check({call_args})")
    } else {
        let fn_name = to_snake(&primary);
        format!("{fn_name}({call_args})")
    };

    // Build use statement.
    let use_stmt = if is_contract {
        let mod_name = format!("contract_{}", to_snake(&primary));
        format!("use {pkg}::{mod_name};\n")
    } else {
        format!("use {pkg}::{fn_name};\n", fn_name = to_snake(&primary))
    };

    let print_result = !param_types.is_empty() || is_contract;
    let main_rs = if print_result {
        format!(
            "// Generated by assura build --bin\n\
             {use_stmt}\n\
             fn main() {{\n\
             {let_stmts}\
                 let r = {call_expr};\n\
                 println!(\"{{r}}\");\n\
             }}\n"
        )
    } else {
        format!(
            "// Generated by assura build --bin\n\
             {use_stmt}\n\
             fn main() {{\n\
             {let_stmts}\
                 {call_expr};\n\
             }}\n"
        )
    };

    project.files.push(("src/main.rs".into(), main_rs));
    if !project.cargo_toml.contains("[[bin]]") {
        project.cargo_toml.push_str(&format!(
            "\n[[bin]]\nname = \"{bin_name}\"\npath = \"src/main.rs\"\n"
        ));
    }
    if human_speak(output_mode, verbosity) {
        eprintln!("  codegen: added binary entry for `{primary}`");
    }
}

/// Contract params for `--bin` (mirrors codegen input()/free-var synthesis).
/// Kept local so `cargo package` of `assura` verifies against crates.io codegen.
fn bin_contract_params(c: &assura_parser::ast::ContractDecl) -> Vec<(String, String)> {
    use assura_parser::ast::ClauseKind;
    let mut params = Vec::new();
    for clause in &c.clauses {
        if clause.kind == ClauseKind::Input {
            for p in assura_parser::ast::extract_clause_params(&clause.body) {
                let ty =
                    p.ty.as_ref()
                        .map(|t| bin_map_simple_type(&t.to_tokens().join(" ")))
                        .unwrap_or_else(|| "i64".into());
                params.push((p.name, ty));
            }
        }
    }
    if params.is_empty() {
        let mut free = std::collections::HashSet::new();
        for clause in &c.clauses {
            if matches!(
                clause.kind,
                ClauseKind::Requires | ClauseKind::Ensures | ClauseKind::Invariant
            ) {
                bin_collect_free_idents(&clause.body, &mut free);
            }
        }
        let mut sorted: Vec<String> = free.into_iter().collect();
        sorted.sort();
        for name in sorted {
            params.push((name, "i64".into()));
        }
    }
    params
}

fn bin_collect_free_idents(
    expr: &assura_parser::ast::SpExpr,
    idents: &mut std::collections::HashSet<String>,
) {
    use assura_parser::ast::Expr;
    match &expr.node {
        Expr::Ident(n) if n != "result" && n != "true" && n != "false" => {
            idents.insert(n.clone());
        }
        Expr::BinOp { lhs, rhs, .. } => {
            bin_collect_free_idents(lhs, idents);
            bin_collect_free_idents(rhs, idents);
        }
        Expr::UnaryOp { expr: e, .. } | Expr::Old(e) | Expr::Field(e, _) => {
            bin_collect_free_idents(e, idents);
        }
        Expr::Call { func, args } => {
            bin_collect_free_idents(func, idents);
            for a in args {
                bin_collect_free_idents(a, idents);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            bin_collect_free_idents(receiver, idents);
            for a in args {
                bin_collect_free_idents(a, idents);
            }
        }
        Expr::Index { expr: e, index } => {
            bin_collect_free_idents(e, idents);
            bin_collect_free_idents(index, idents);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            bin_collect_free_idents(cond, idents);
            bin_collect_free_idents(then_branch, idents);
            if let Some(e) = else_branch {
                bin_collect_free_idents(e, idents);
            }
        }
        _ => {}
    }
}

/// Minimal Assura→Rust type map for `--bin` defaults (no codegen dependency).
fn bin_map_simple_type(assura_ty: &str) -> String {
    let t = assura_ty.split_whitespace().next().unwrap_or("Int");
    match t {
        "Int" => "i64".into(),
        "Nat" => "u64".into(),
        "Bool" => "bool".into(),
        "Float" => "f64".into(),
        "String" => "String".into(),
        "Bytes" => "Vec<u8>".into(),
        "Unit" => "()".into(),
        other => other.to_string(),
    }
}

/// Return a sensible default value for a Rust type used in `--bin` scaffolding.
fn default_value_for_type(ty: &str) -> &'static str {
    match ty {
        "i64" | "i32" | "i16" | "i8" => "42",
        "u64" | "u32" | "u16" | "u8" => "42",
        "i128" => "42_i128",
        "u128" => "42_u128",
        "f64" | "f32" => "1.0",
        "bool" => "true",
        "String" => "String::from(\"test\")",
        "Vec<u8>" => "vec![0u8; 16]",
        _ if ty.starts_with("Vec<") => "Vec::new()",
        _ if ty.starts_with("BTreeMap<") => "std::collections::BTreeMap::new()",
        _ if ty.starts_with("Option<") => "None",
        _ if ty.starts_with("Result<") => "Ok(Default::default())",
        _ => "Default::default()",
    }
}

/// Local embed of multi-fn IR.
///
/// Sibling `fn #N` become capturing closures referenced from main as `block_N()`.
/// Kept in the CLI (instead of calling `assura_smt::ir_module_to_embedded_body`)
/// so `cargo package` co-publish against the last crates.io `assura-smt` still
/// compiles before the monorepo versions are published together.
fn multi_block_ir_to_embedded_body(module: &assura_smt::IrModule) -> String {
    let mut code = String::new();
    let ret_ty = module
        .functions
        .first()
        .map(|f| match f.return_type.as_str() {
            "Int" => "i64",
            "Nat" => "u64",
            "Bool" => "bool",
            "Float" => "f64",
            other if !other.is_empty() => other,
            _ => "i64",
        })
        .unwrap_or("i64");

    for func in module.functions.iter().skip(1) {
        let id = func.id.trim_start_matches('#');
        let body = assura_smt::ir_function_body_to_rust(func);
        let indented: String = body
            .lines()
            .map(|l| {
                if l.is_empty() {
                    String::new()
                } else {
                    format!("    {l}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        code.push_str(&format!(
            "    let block_{id} = || -> {ret_ty} {{\n{indented}\n    }};\n"
        ));
    }
    if let Some(main) = module.functions.first() {
        code.push_str(&assura_smt::ir_function_body_to_rust(main));
    }
    code
}

/// Convert co-located `{ContractName}.ir` sidecars into Rust bodies for codegen.
///
/// Used by the getting-started flow (#866): after `assura check` with
/// co-located IR proves the contract, `assura build` injects the same IR as a
/// real implementation instead of `todo!()`.
///
/// Only loads IR from the **same directory as the source file** (e.g.
/// `ShowcaseEcho.ir` next to `showcase-echo.assura`). Heuristic stubs under
/// `generated/` are intentionally ignored: they often lack real semantics and
/// would inject non-compiling Rust into multi-contract demos.
fn rust_bodies_from_ir_sidecars(
    typed: &assura_types::TypedFile,
    source_path: &str,
    verbosity: Verbosity,
    output_mode: OutputMode,
) -> std::collections::HashMap<String, String> {
    let path = Path::new(source_path);
    let parent = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let contexts = assura_smt::ir_prompt_contexts_for_typed(typed, Some(path));
    let mut out = std::collections::HashMap::new();
    for ctx in &contexts {
        let ir_path = parent.join(format!("{}.ir", ctx.decl_name));
        let Ok(ir_text) = fs::read_to_string(&ir_path) else {
            continue;
        };
        // Stubs must not become `todo!()` replacements (identity load != ensures).
        if ir_text.contains("Stub IR") {
            if output_mode == OutputMode::Human && verbosity == Verbosity::Verbose {
                eprintln!("  codegen: skip co-located stub IR for `{}`", ctx.decl_name);
            }
            continue;
        }
        let Ok(module) = assura_smt::parse_ir_module(&ir_text) else {
            continue;
        };
        if module.functions.is_empty() {
            continue;
        }
        let mut body = String::new();
        for (i, param) in ctx.params.iter().enumerate() {
            body.push_str(&format!(
                "    let slot_{i} = {name}.clone();\n",
                name = param.name
            ));
        }
        // Multi-block modules get sibling closures + main body (#882).
        let ir_body = if module.functions.len() == 1 {
            assura_smt::ir_function_body_to_rust(&module.functions[0])
        } else {
            multi_block_ir_to_embedded_body(&module)
        };
        let ir_body = ir_body.replace("__result", "__assura_result");
        let ir_body = strip_trailing_return(&ir_body);
        body.push_str(&ir_body);
        out.insert(ctx.decl_name.clone(), body);
    }

    if human_speak(output_mode, verbosity) && !out.is_empty() {
        let names: Vec<&str> = out.keys().map(String::as_str).collect();
        eprintln!(
            "  codegen: injected co-located IR for {} contract(s): {}",
            out.len(),
            names.join(", ")
        );
    }
    out
}

/// Remove the trailing bare return expression from an IR body.
///
/// `ir_function_body_to_rust` ends with `__assura_result\n` (no semicolon),
/// which would be a return expression. But the codegen framework adds ensures
/// checks and its own return after the IR body, so we need to remove or
/// semicolon-terminate the trailing return.
fn strip_trailing_return(body: &str) -> String {
    let trimmed = body.trim_end();
    // If the last line is just the result variable, remove it
    if let Some(last_newline) = trimmed.rfind('\n') {
        let last_line = trimmed[last_newline + 1..].trim();
        if last_line == "__assura_result" {
            return trimmed[..last_newline].to_string() + "\n";
        }
    } else if trimmed.trim() == "__assura_result" {
        return String::new();
    }
    body.to_string()
}

/// Strip markdown code fences from LLM output to extract raw IR text.
fn strip_markdown_fences(text: &str) -> String {
    let text = text.trim();
    // Try to extract content between ```...``` fences
    if let Some(start) = text.find("```") {
        let after_fence = &text[start + 3..];
        // Skip the language tag (e.g., ```ir or ```)
        let content_start = after_fence.find('\n').map(|i| i + 1).unwrap_or(0);
        let content = &after_fence[content_start..];
        if let Some(end) = content.find("```") {
            return content[..end].trim().to_string();
        }
        return content.trim().to_string();
    }
    // If the text starts with "module", it's already raw IR
    text.to_string()
}

// ---------------------------------------------------------------------------
// Project-mode build: resolve, type-check, and codegen all .assura files
// ---------------------------------------------------------------------------

pub(crate) fn run_build_project(
    project_dir: &Path,
    output_mode: OutputMode,
    verbosity: Verbosity,
    output_dir: &str,
    target: assura_codegen::CompileTarget,
    no_check: bool,
    runtime_checks: bool,
) {
    let speak = human_speak(output_mode, verbosity);
    let (project_root, dep_map, dep_warnings) = load_project_deps(project_dir);

    if speak {
        eprintln!("Building project at {}", project_root.display());
        for w in &dep_warnings {
            eprintln!("Warning: {w}");
        }
    }

    let (resolved_files, warnings) =
        match assura_resolve::discover_and_resolve_project_with_deps(&project_root, &dep_map) {
            Ok(pair) => pair,
            Err(errors) => {
                if output_mode == OutputMode::Json {
                    let report = serde_json::json!({
                        "ok": false,
                        "project": project_root.display().to_string(),
                        "errors": errors.iter().map(|e| e.to_string()).collect::<Vec<_>>(),
                    });
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    for e in &errors {
                        eprintln!("Error: {e}");
                    }
                }
                process::exit(1);
            }
        };

    if speak {
        for w in &warnings {
            eprintln!("Warning: {w}");
        }
    }

    let mut all_typed = Vec::new();
    let mut has_errors = false;
    let mut module_errors: Vec<serde_json::Value> = Vec::new();

    for (module_path, resolved) in resolved_files {
        match assura_types::type_check(resolved) {
            Ok(typed) => {
                all_typed.push((module_path.clone(), typed));
                if output_mode == OutputMode::Human && verbosity == Verbosity::Verbose {
                    eprintln!("OK  {module_path}");
                }
            }
            Err(errors) => {
                has_errors = true;
                if output_mode == OutputMode::Json {
                    module_errors.push(serde_json::json!({
                        "module": module_path,
                        "errors": errors.iter().map(|e| serde_json::json!({
                            "code": e.code.to_string(),
                            "message": e.message,
                        })).collect::<Vec<_>>(),
                    }));
                } else {
                    eprintln!("ERR {module_path}: {} error(s)", errors.len());
                    for err in &errors {
                        eprintln!("  {}: {}", err.code, err.message);
                    }
                }
            }
        }
    }

    if has_errors {
        if output_mode == OutputMode::Json {
            let report = serde_json::json!({
                "ok": false,
                "project": project_root.display().to_string(),
                "modules": module_errors,
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!("Build failed: type errors in project");
        }
        process::exit(1);
    }

    // Generate code for each module
    let out_dir = Path::new(output_dir);
    let mut generated_files = 0usize;
    let mut written_paths: Vec<String> = Vec::new();
    let mut cargo_toml_written = false;
    let backend_config = assura_codegen::BackendConfig {
        target: target.clone(),
        runtime_checks,
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
                if output_mode == OutputMode::Json {
                    let report = serde_json::json!({
                        "ok": false,
                        "project": project_root.display().to_string(),
                        "error": format!("cannot write {}: {e}", cargo_path.display()),
                    });
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    eprintln!("Error writing {}: {e}", cargo_path.display());
                }
                process::exit(1);
            }
            written_paths.push(cargo_path.display().to_string());
            cargo_toml_written = true;
        }
        for (rel_path, content) in &project.files {
            let file_out = out_dir.join(rel_path);
            if let Some(parent) = file_out.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Err(e) = fs::write(&file_out, content) {
                if output_mode == OutputMode::Json {
                    let report = serde_json::json!({
                        "ok": false,
                        "project": project_root.display().to_string(),
                        "error": format!("cannot write {}: {e}", file_out.display()),
                    });
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    eprintln!("Error writing {}: {e}", file_out.display());
                }
                process::exit(1);
            }
            written_paths.push(file_out.display().to_string());
            generated_files += 1;
        }

        // Write sidecar metadata JSON
        if let Some(ref meta) = project.metadata {
            let json_path = out_dir.join("assura-contracts.json");
            if let Ok(json) = serde_json::to_string_pretty(meta) {
                if let Err(e) = fs::write(&json_path, json) {
                    if speak {
                        eprintln!("Warning: could not write {}: {e}", json_path.display());
                    }
                } else {
                    written_paths.push(json_path.display().to_string());
                }
            }
        }
    }

    if speak {
        eprintln!(
            "Generated {generated_files} file(s) in {}",
            out_dir.display()
        );
    }

    let mut cargo_check = CargoCheckStatus::Skipped;

    // Build the generated code to produce artifacts
    if !no_check && out_dir.join("Cargo.toml").exists() {
        if speak {
            eprintln!("Running cargo build on generated code...");
        }
        let status = std::process::Command::new("cargo")
            .arg("build")
            .current_dir(out_dir)
            .env_remove("RUSTC_WRAPPER")
            .env_remove("CARGO_TARGET_DIR")
            .status();
        match status {
            Ok(s) if s.success() => {
                cargo_check = CargoCheckStatus::Ok;
                if speak {
                    eprintln!("Generated code compiled successfully");
                }
            }
            Ok(s) => {
                cargo_check = CargoCheckStatus::Failed;
                if output_mode == OutputMode::Json {
                    let report = serde_json::json!({
                        "ok": false,
                        "project": project_root.display().to_string(),
                        "output_dir": out_dir.display().to_string(),
                        "files": written_paths,
                        "cargo_check": cargo_check.as_str(),
                        "error": format!(
                            "generated code failed to compile (exit {})",
                            s.code().unwrap_or(-1)
                        ),
                    });
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    eprintln!(
                        "Generated code failed to compile (exit {})",
                        s.code().unwrap_or(-1)
                    );
                }
                process::exit(1);
            }
            Err(e) => {
                cargo_check = CargoCheckStatus::CargoNotFound;
                if speak {
                    eprintln!("Failed to run cargo build: {e}");
                }
            }
        }
    }

    if output_mode == OutputMode::Json {
        let report = serde_json::json!({
            "ok": true,
            "project": project_root.display().to_string(),
            "output_dir": out_dir.display().to_string(),
            "target": compile_target_label(&target),
            "files": written_paths,
            "generated_file_count": generated_files,
            "cargo_check": cargo_check.as_str(),
            "no_check": no_check,
        });
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // JSON purity helpers
    // ---------------------------------------------------------------

    #[test]
    fn human_speak_false_for_json_even_when_verbose() {
        assert!(!human_speak(OutputMode::Json, Verbosity::Verbose));
        assert!(!human_speak(OutputMode::Json, Verbosity::Normal));
        assert!(!human_speak(OutputMode::Json, Verbosity::Quiet));
    }

    #[test]
    fn human_speak_respects_quiet_and_human() {
        assert!(human_speak(OutputMode::Human, Verbosity::Normal));
        assert!(human_speak(OutputMode::Human, Verbosity::Verbose));
        assert!(!human_speak(OutputMode::Human, Verbosity::Quiet));
    }

    #[test]
    fn cargo_check_status_as_str_labels() {
        assert_eq!(CargoCheckStatus::Skipped.as_str(), "skipped");
        assert_eq!(CargoCheckStatus::Ok.as_str(), "ok");
        assert_eq!(CargoCheckStatus::Failed.as_str(), "failed");
        assert_eq!(CargoCheckStatus::CargoNotFound.as_str(), "cargo_not_found");
    }

    #[test]
    fn compile_target_label_native_and_wasm() {
        assert_eq!(
            compile_target_label(&assura_codegen::CompileTarget::Native),
            "native"
        );
        assert_eq!(
            compile_target_label(&assura_codegen::CompileTarget::Wasm),
            "wasm"
        );
    }

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
    // find_native_artifact
    // ---------------------------------------------------------------

    #[test]
    fn build_find_native_artifact_returns_none_for_nonexistent_dir() {
        let result = find_native_artifact(Path::new(
            "/tmp/__assura_nonexistent_dir_native_test_98231__",
        ));
        assert!(result.is_none());
    }

    #[test]
    fn build_find_native_artifact_returns_none_for_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = find_native_artifact(dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn build_find_native_artifact_finds_rlib_in_deps() {
        let dir = tempfile::tempdir().unwrap();
        let deps = dir.path().join("deps");
        fs::create_dir_all(&deps).unwrap();
        fs::write(deps.join("libmy_crate-abc123.rlib"), b"fake rlib").unwrap();
        let result = find_native_artifact(dir.path()).expect("should find native artifact");
        assert_eq!(result.extension().unwrap(), "rlib");
    }

    #[test]
    fn build_find_native_artifact_returns_none_without_deps() {
        let dir = tempfile::tempdir().unwrap();
        // rlib in the dir itself (not deps/) should not be found
        fs::write(dir.path().join("libfoo.rlib"), b"not in deps").unwrap();
        let result = find_native_artifact(dir.path());
        assert!(result.is_none());
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
    // resolve_output_dir_for_file
    // ---------------------------------------------------------------

    #[test]
    fn default_generated_resolves_next_to_source() {
        let p = resolve_output_dir_for_file("generated", "myproj/contracts/lib.assura");
        assert_eq!(
            p.as_ref().map(|p| p.to_string_lossy().into_owned()),
            Some("myproj/contracts/generated".into())
        );
    }

    #[test]
    fn explicit_multi_segment_output_stays_cwd_relative() {
        // Must not nest: myproj/contracts/myproj/gen
        let p = resolve_output_dir_for_file("myproj/gen", "myproj/contracts/lib.assura");
        assert!(
            p.is_none(),
            "explicit relative multi-segment --output should not join source parent"
        );
    }

    #[test]
    fn explicit_single_segment_output_stays_cwd_relative() {
        // --output covproj must not become contracts/covproj
        let p = resolve_output_dir_for_file("covproj", "covproj/contracts/lib.assura");
        assert!(
            p.is_none(),
            "explicit single-segment --output should stay CWD-relative"
        );
    }

    #[test]
    fn absolute_output_unchanged() {
        let p = resolve_output_dir_for_file("/tmp/out", "myproj/contracts/lib.assura");
        assert!(p.is_none());
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

    // ---------------------------------------------------------------
    // strip_trailing_return
    // ---------------------------------------------------------------

    #[test]
    fn strip_trailing_return_removes_bare_result() {
        let body = "    let x = 1;\n__assura_result\n";
        let result = strip_trailing_return(body);
        assert_eq!(result, "    let x = 1;\n");
    }

    #[test]
    fn strip_trailing_return_only_result() {
        let result = strip_trailing_return("__assura_result");
        assert_eq!(result, "");
    }

    #[test]
    fn strip_trailing_return_preserves_non_result_ending() {
        let body = "    let x = 1;\n    x + 2\n";
        let result = strip_trailing_return(body);
        assert_eq!(result, body);
    }

    #[test]
    fn strip_trailing_return_preserves_result_in_middle() {
        let body = "    __assura_result = 42;\n    do_stuff();\n";
        let result = strip_trailing_return(body);
        assert_eq!(result, body);
    }

    // ---------------------------------------------------------------
    // strip_markdown_fences
    // ---------------------------------------------------------------

    #[test]
    fn strip_markdown_fences_extracts_fenced_content() {
        let input = "```ir\nmodule Foo {\n}\n```";
        let result = strip_markdown_fences(input);
        assert_eq!(result, "module Foo {\n}");
    }

    #[test]
    fn strip_markdown_fences_plain_text_passthrough() {
        let input = "module Bar {\n}";
        let result = strip_markdown_fences(input);
        assert_eq!(result, "module Bar {\n}");
    }

    #[test]
    fn strip_markdown_fences_no_language_tag() {
        let input = "```\nmodule Baz {}\n```";
        let result = strip_markdown_fences(input);
        assert_eq!(result, "module Baz {}");
    }

    #[test]
    fn strip_markdown_fences_surrounding_text() {
        let input = "Here is the IR:\n```ir\nmodule X {}\n```\nDone.";
        let result = strip_markdown_fences(input);
        assert_eq!(result, "module X {}");
    }

    // ---------------------------------------------------------------
    // build_single_contract_source
    // ---------------------------------------------------------------

    #[test]
    fn build_single_contract_source_basic() {
        let ctx = assura_smt::IrPromptContext {
            decl_name: "SafeDiv".into(),
            params: vec![
                Param {
                    name: "a".into(),
                    ty: Some(TypeExpr::Named("Int".into())),
                },
                Param {
                    name: "b".into(),
                    ty: Some(TypeExpr::Named("Int".into())),
                },
            ],
            return_ty: vec!["Int".into()],
            clauses: vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Spanned::no_span(Expr::BinOp {
                        op: BinOp::Neq,
                        lhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
                        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                    }),
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Spanned::no_span(Expr::BinOp {
                        op: BinOp::Eq,
                        lhs: Box::new(Spanned::no_span(Expr::Ident("result".into()))),
                        rhs: Box::new(Spanned::no_span(Expr::BinOp {
                            op: BinOp::Div,
                            lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
                            rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
                        })),
                    }),
                    effect_variables: vec![],
                },
            ],
            source_file: None,
        };
        let src = build_single_contract_source(&ctx);
        assert!(src.contains("contract SafeDiv {"));
        assert!(src.contains("input(a: Int, b: Int)"));
        assert!(src.contains("output(result: Int)"));
        assert!(src.contains("requires {"));
        assert!(src.contains("ensures {"));
    }

    #[test]
    fn build_single_contract_source_no_params_unit_return() {
        let ctx = assura_smt::IrPromptContext {
            decl_name: "Ping".into(),
            params: vec![],
            return_ty: vec![],
            clauses: vec![],
            source_file: None,
        };
        let src = build_single_contract_source(&ctx);
        assert!(src.contains("contract Ping {"));
        assert!(!src.contains("input("));
        assert!(!src.contains("output("));
        assert!(src.ends_with("}\n"));
    }

    // ---------------------------------------------------------------
    // default_value_for_type
    // ---------------------------------------------------------------

    #[test]
    fn default_value_for_integer_types() {
        assert_eq!(default_value_for_type("i64"), "42");
        assert_eq!(default_value_for_type("u64"), "42");
        assert_eq!(default_value_for_type("i128"), "42_i128");
        assert_eq!(default_value_for_type("u128"), "42_u128");
    }

    #[test]
    fn default_value_for_float_types() {
        assert_eq!(default_value_for_type("f64"), "1.0");
        assert_eq!(default_value_for_type("f32"), "1.0");
    }

    #[test]
    fn default_value_for_string_and_bool() {
        assert_eq!(default_value_for_type("bool"), "true");
        assert!(default_value_for_type("String").contains("String::from"));
    }

    #[test]
    fn default_value_for_collection_types() {
        assert!(default_value_for_type("Vec<u8>").contains("vec!"));
        assert!(default_value_for_type("Vec<i64>").contains("Vec::new()"));
        assert!(default_value_for_type("BTreeMap<String, i64>").contains("BTreeMap::new()"));
        assert_eq!(default_value_for_type("Option<i64>"), "None");
    }

    #[test]
    fn default_value_for_unknown_type_falls_back() {
        assert_eq!(default_value_for_type("MyCustomType"), "Default::default()");
    }

    // ---------------------------------------------------------------
    // inject_bin_main (via pipeline)
    // ---------------------------------------------------------------

    /// Helper: compile source, codegen, inject --bin main, return the main.rs content.
    fn bin_main_for(source: &str) -> String {
        let config = assura_config::CompilerConfig::default();
        let output = assura_pipeline::compile(source, "test.assura", &config);
        assert!(!output.has_errors, "source should compile without errors");
        let typed = output.typed.as_ref().expect("typed should be present");
        let mut project = assura_codegen::codegen(typed);
        inject_bin_main(&mut project, typed, Verbosity::Quiet, OutputMode::Human);
        project
            .files
            .iter()
            .find(|(name, _)| name == "src/main.rs")
            .map(|(_, content)| content.clone())
            .expect("should have src/main.rs")
    }

    #[test]
    fn bin_main_contract_generates_correct_call_path() {
        let main_rs = bin_main_for(
            "contract SafeDivision {\n  input(a: Int, b: Int)\n  requires { b != 0 }\n  ensures { result == a / b }\n}",
        );
        assert!(
            main_rs.contains("contract_safedivision::check("),
            "got: {main_rs}"
        );
        assert!(
            main_rs.contains("std::env::args()") && main_rs.contains("unwrap_or(42)"),
            "first int param from argv, got: {main_rs}"
        );
        assert!(main_rs.contains("let b: i64 = 42;"), "got: {main_rs}");
        assert!(
            main_rs.contains("println!"),
            "must print result, got: {main_rs}"
        );
    }

    #[test]
    fn bin_main_fn_decl_generates_direct_call() {
        let main_rs = bin_main_for(
            "fn add(x: Int, y: Int) -> Int {\n  requires { x >= 0 }\n  ensures { result == x + y }\n}",
        );
        assert!(main_rs.contains("add("), "got: {main_rs}");
        assert!(
            !main_rs.contains("contract_"),
            "fn decl should not use contract_ module, got: {main_rs}"
        );
    }

    #[test]
    fn bin_main_prefers_contract_over_fn() {
        let main_rs = bin_main_for(
            "fn helper(x: Int) -> Int {\n  requires { x >= 0 }\n  ensures { result >= 0 }\n}\n\ncontract Verify {\n  input(n: Int)\n  requires { n > 0 }\n  ensures { result > 0 }\n}",
        );
        assert!(
            main_rs.contains("contract_verify::check("),
            "should prefer contract, got: {main_rs}"
        );
    }

    #[test]
    fn bin_main_float_param_uses_f64_default() {
        let main_rs = bin_main_for(
            "contract Temperature {\n  input(celsius: Float)\n  requires { celsius > -273.15 }\n  ensures { result > 0.0 }\n}",
        );
        assert!(
            main_rs.contains("f64"),
            "Float param should map to f64, got: {main_rs}"
        );
        assert!(
            main_rs.contains("1.0"),
            "Float default should be 1.0, got: {main_rs}"
        );
    }
}
