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

/// Run verification on a typed file and print results. Returns the verification
/// results and elapsed time in milliseconds.
fn verify_and_print(
    typed: &assura_types::TypedFile,
    filename: &str,
    solver: assura_smt::SolverChoice,
    verbosity: Verbosity,
) -> (Vec<assura_smt::VerificationResult>, f64) {
    let qwarnings = assura_smt::validate_quantifier_bounds(typed);
    if verbosity != Verbosity::Quiet {
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

    if verbosity == Verbosity::Verbose {
        eprintln!(
            "  verify:    {} clause(s) ({verify_ms:.2}ms)",
            results.len()
        );
    }
    if verbosity != Verbosity::Quiet && !results.is_empty() {
        eprintln!();
        eprintln!("Verification ({} clause(s)):", results.len());
        let _ =
            assura_smt::display::write_grouped_verification(&mut std::io::stderr(), &results, "  ");
    }
    (results, verify_ms)
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
        eprintln!("Error: {filename}: {e}");
        process::exit(2);
    });

    // Pipeline + timing
    let output = compile_with_config(&source, filename, &bc.compiler_config);
    crate::timing::print_pipeline_timing(
        &output,
        crate::timing::TimingOptions {
            filename,
            output_mode: OutputMode::Human,
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
        assura_diagnostics::report_diagnostics_human(&diagnostics, filename, &source);
        eprintln!("{filename}: {} error(s) found", diagnostics.len());
        process::exit(1);
    }
    let typed = typed.expect("type check should succeed if has_errors is false");

    // Verify
    let (verification_results, verify_ms) =
        verify_and_print(&typed, filename, bc.solver, verbosity);

    // Auto-implement: call LLM to generate IR implementations
    let ir_bodies = if auto_implement {
        let ai_config = bc
            .project
            .as_ref()
            .map(|(p, _)| p.ai.clone())
            .unwrap_or_default();
        auto_implement_contracts(&typed, filename, &bc.compiler_config, verbosity, &ai_config)
    } else {
        std::collections::HashMap::new()
    };

    // Codegen
    let codegen_start = Instant::now();
    let backend_config = assura_codegen::BackendConfig {
        target: bc.compile_target.clone(),
        runtime_checks,
        ir_bodies,
        ..assura_codegen::BackendConfig::default()
    };
    let project = assura_codegen::codegen_with_config(&typed, &backend_config);

    // Write output
    let out_dir = Path::new(effective_out_dir_str);
    fs::create_dir_all(out_dir).unwrap_or_else(|e| {
        eprintln!(
            "Error: cannot create {}/ directory: {e}",
            effective_out_dir_str
        );
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

    write_generated_project(
        filename,
        out_dir,
        &project,
        &typed,
        &bc.compile_target,
        verbosity,
    );
    write_unresolved_tests(
        out_dir,
        &verification_results,
        parsed_file.as_ref(),
        &typed,
        verbosity,
    );
    run_cargo_build(
        filename,
        effective_out_dir_str,
        out_dir,
        &bc.compile_target,
        no_check,
        verbosity,
    );
}

// ---------------------------------------------------------------------------
// Extracted subfunctions for run_build
// ---------------------------------------------------------------------------

/// Write the generated Rust project to the output directory: Cargo.toml,
/// source files, IR sidecars, metadata JSON, and .cargo/config.toml for WASM.
fn write_generated_project(
    filename: &str,
    out_dir: &Path,
    project: &assura_codegen::GeneratedProject,
    typed: &assura_types::TypedFile,
    compile_target: &assura_codegen::CompileTarget,
    verbosity: Verbosity,
) {
    // Cargo.toml
    let cargo_path = out_dir.join("Cargo.toml");
    fs::write(&cargo_path, &project.cargo_toml).unwrap_or_else(|e| {
        eprintln!("Error: cannot write {}: {e}", cargo_path.display());
        process::exit(1);
    });
    if verbosity != Verbosity::Quiet {
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
            let ir_path = ir_dir.join(format!("{name}.ir"));
            if fs::write(&ir_path, ir_text).is_ok() && verbosity != Verbosity::Quiet {
                println!("  wrote {}", ir_path.display());
            }
        }
    }

    // Source files
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

    // Sidecar metadata JSON
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

    // .cargo/config.toml for WASM target
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
}

/// Auto-generate proptest tests for contracts whose verification was
/// timeout or unknown.
fn write_unresolved_tests(
    out_dir: &Path,
    verification_results: &[assura_smt::VerificationResult],
    parsed_file: Option<&assura_parser::ast::SourceFile>,
    typed: &assura_types::TypedFile,
    verbosity: Verbosity,
) {
    let has_unresolved = verification_results.iter().any(|r| {
        matches!(
            r,
            assura_smt::VerificationResult::Timeout { .. }
                | assura_smt::VerificationResult::Unknown { .. }
        )
    });
    let Some(pf) = parsed_file else { return };
    if !has_unresolved {
        return;
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

/// Run `cargo build` on the generated project and report results.
fn run_cargo_build(
    filename: &str,
    out_dir_str: &str,
    out_dir: &Path,
    compile_target: &assura_codegen::CompileTarget,
    no_check: bool,
    verbosity: Verbosity,
) {
    if no_check {
        if verbosity != Verbosity::Quiet {
            println!("OK  {filename} -> {out_dir_str}/ (check skipped)");
        }
        return;
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
            report_build_success(filename, out_dir, out_dir_str, is_wasm, verbosity);
        }
        Ok(output) => {
            if verbosity != Verbosity::Quiet {
                println!("OK  {filename} -> {out_dir_str}/");
            }
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
        Err(_) => {
            if verbosity != Verbosity::Quiet {
                println!("OK  {filename} -> {out_dir_str}/ (cargo build skipped: cargo not found)");
            }
        }
    }
}

fn report_build_success(
    filename: &str,
    out_dir: &Path,
    out_dir_str: &str,
    is_wasm: bool,
    verbosity: Verbosity,
) {
    if verbosity == Verbosity::Quiet {
        return;
    }
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

/// When the output path is relative, resolve it relative to the input file's
/// parent directory. This ensures `assura build /tmp/project/lib.assura` writes
/// to `/tmp/project/generated/` instead of `./generated/`.
///
/// Returns `None` if the output path is absolute or the input file has no parent.
fn resolve_output_dir_for_file(out_dir_str: &str, filename: &str) -> Option<std::path::PathBuf> {
    let out_path = Path::new(out_dir_str);
    if out_path.is_absolute() {
        return None;
    }
    let input_parent = Path::new(filename).parent()?;
    if input_parent == Path::new("") || input_parent == Path::new(".") {
        return None;
    }
    Some(input_parent.join(out_dir_str))
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
    ai_config: &assura_config::AiConfig,
) -> std::collections::HashMap<String, String> {
    let mut ir_bodies = std::collections::HashMap::new();

    let contexts = assura_smt::ir_prompt_contexts_for_typed(typed, Some(Path::new(source_path)));
    if contexts.is_empty() {
        return ir_bodies;
    }

    for ctx in &contexts {
        if verbosity != Verbosity::Quiet {
            eprint!("  auto-implement {}...", ctx.decl_name);
        }

        let prompt = assura_smt::render_ir_prompt(ctx, assura_smt::IrPromptPattern::Auto);
        // Build a single-contract source for verification.
        // verify_ir validates the IR against the first contract it finds,
        // so we must pass just this contract (not the full multi-contract file).
        let single_contract_source = build_single_contract_source(ctx);
        match call_llm_for_ir(
            &ctx.decl_name,
            &prompt,
            &single_contract_source,
            config,
            verbosity,
            &ctx.params,
            ai_config,
        ) {
            Some(rust_body) => {
                if verbosity != Verbosity::Quiet {
                    eprintln!(" ok");
                }
                ir_bodies.insert(ctx.decl_name.clone(), rust_body);
            }
            None => {
                if verbosity != Verbosity::Quiet {
                    eprintln!(" failed (will use todo!())");
                }
            }
        }
    }

    if verbosity != Verbosity::Quiet {
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
    verbosity: Verbosity,
    output_dir: &str,
    target: assura_codegen::CompileTarget,
    no_check: bool,
    runtime_checks: bool,
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

    for (module_path, resolved) in resolved_files {
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

    // Build the generated code to produce artifacts
    if !no_check && out_dir.join("Cargo.toml").exists() {
        eprintln!("Running cargo build on generated code...");
        let status = std::process::Command::new("cargo")
            .arg("build")
            .current_dir(out_dir)
            .env_remove("RUSTC_WRAPPER")
            .env_remove("CARGO_TARGET_DIR")
            .status();
        match status {
            Ok(s) if s.success() => {
                eprintln!("Generated code compiled successfully");
            }
            Ok(s) => {
                eprintln!(
                    "Generated code failed to compile (exit {})",
                    s.code().unwrap_or(-1)
                );
                process::exit(1);
            }
            Err(e) => {
                eprintln!("Failed to run cargo build: {e}");
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
}
