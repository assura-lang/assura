//! `assura check` single-file entry (pipeline + verify dispatch).

use super::super::*;
use super::report::{collect_ir_surface_listing, verify_and_report};
use super::types::{CheckOptions, VerifyContext};

// ---------------------------------------------------------------------------
// `assura check <file> [--json|--human] [--layer 0|1]`
// ---------------------------------------------------------------------------

pub(crate) fn run_check(opts: CheckOptions<'_>) {
    let CheckOptions {
        filename,
        output_mode,
        verbosity,
        layer: cli_layer,
        solver: cli_solver,
        watch,
        stats,
        dump_smt,
        show_cores,
        strict,
        showcase_only,
    } = opts;
    // Load project config (assura.toml) if available
    let project = load_project_config(Path::new(filename));
    let config_layer = project.as_ref().map(|(c, _)| c.verify.layer);

    // Verification layer: CLI flag > config file > default (1)
    // 255 is the sentinel for "not specified on CLI"
    if cli_layer != 255 && cli_layer > 3 {
        eprintln!(
            "Error: invalid --layer {cli_layer} (expected 0=structural, 1=SMT, 2=quantified/termination, 3=BMC)"
        );
        process::exit(2);
    }
    let layer: u8 = if cli_layer != 255 {
        cli_layer
    } else {
        config_layer.unwrap_or(1)
    };

    // Solver choice: CLI flag > config file > default (Z3)
    let config_solver = project.as_ref().map(|(c, _)| c.verify.smt_solver);
    let solver =
        cli_solver.unwrap_or_else(|| config_solver.unwrap_or(assura_smt::SolverChoice::Z3));

    // Build unified compiler config
    let compiler_config = if let Some((ref proj, _)) = project {
        let mut cc = CompilerConfig::from_project(proj, output_mode, verbosity);
        cc.verify.layer = layer;
        cc.verify.solver = solver;
        cc
    } else {
        CompilerConfig {
            output_mode,
            verbosity,
            verify: assura_config::VerifyOptions {
                layer,
                solver,
                ..Default::default()
            },
            ..Default::default()
        }
    };
    // Keep the project config around for verbose display
    let config = project;

    if watch {
        if is_stdin_arg(filename) {
            if output_mode == OutputMode::Json {
                let report = serde_json::json!({
                    "ok": false,
                    "command": "check",
                    "watch": true,
                    "error": "watch_stdin_unsupported",
                    "message": "--watch cannot be used with stdin (-)",
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                eprintln!("Error: --watch cannot be used with stdin (-)");
            }
            process::exit(2);
        }
        run_watch_loop(filename, output_mode, verbosity, layer);
        // run_watch_loop never returns (loops until interrupted)
    }

    // --- Project mode: detect directory or project root ---
    let path = Path::new(filename);
    if !is_stdin_arg(filename) && path.is_dir() {
        // Directory mode: check all .assura files in the project
        run_check_project(
            path,
            output_mode,
            verbosity,
            &compiler_config,
            showcase_only,
            strict,
        );
        return;
    }

    let (source, display_name) = read_source_arg(filename).unwrap_or_else(|e| {
        if output_mode == OutputMode::Json {
            let diag = assura_diagnostics::Diagnostic::error("A01000", format!("{e}"), 0..0)
                .with_file(filename);
            println!("{}", serde_json::to_string_pretty(&[diag]).unwrap());
        } else {
            eprintln!("Error: {filename}: {e}");
        }
        process::exit(2);
    });
    // Use a stable display path for diagnostics (filename stays "-" for CLI args).
    let filename = display_name.as_str();

    // --- Run shared pipeline ---
    let output = compile_with_config(&source, filename, &compiler_config);
    crate::timing::print_pipeline_timing(
        &output,
        crate::timing::TimingOptions {
            filename,
            output_mode,
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
                    "config: layer={}, solver={}, timeout={}ms, output={}",
                    cfg.verify.layer, cfg.verify.smt_solver, cfg.verify.timeout, cfg.build.output
                )
            }),
            verify_ms: None,
            show_total: false,
            show_phase_failures: true,
        },
    );
    let CompilationResult {
        file,
        resolved,
        typed,
        mut diagnostics,
        mut has_errors,
        timing,
        ..
    } = output;

    // --- Verify + report ---
    let verify_start = Instant::now();
    let verification_results = verify_and_report(VerifyContext {
        filename,
        source: &source,
        typed: &typed,
        file: &file,
        diagnostics: &mut diagnostics,
        has_errors: &mut has_errors,
        output_mode,
        verbosity,
        verify_options: compiler_config.verify.clone(),
        show_cores,
        strict,
    });

    let verify_ms = verify_start.elapsed().as_secs_f64() * 1000.0;
    if verbosity == Verbosity::Verbose && output_mode == OutputMode::Human {
        eprintln!(
            "  verify:    {} clause(s) ({verify_ms:.2}ms)",
            verification_results.len()
        );
        let total = timing.parse_ms
            + timing.resolve_ms.unwrap_or(0.0)
            + timing.typecheck_ms.unwrap_or(0.0)
            + verify_ms;
        eprintln!("  total:     {total:.2}ms");
        eprintln!();
    }

    // --- Dump SMT queries to files ---
    if let Some(smt_dir) = dump_smt
        && let Some(ref typed) = typed
    {
        let dir = Path::new(smt_dir);
        if let Err(e) = fs::create_dir_all(dir) {
            if output_mode == OutputMode::Json {
                let report = serde_json::json!({
                    "ok": false,
                    "command": "check",
                    "error": "dump_smt_mkdir_failed",
                    "path": smt_dir,
                    "message": format!("cannot create {smt_dir}: {e}"),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                eprintln!("Error: cannot create {smt_dir}: {e}");
            }
            process::exit(2);
        }
        let queries = assura_smt::dump_smt_queries(typed);
        for (i, q) in queries.iter().enumerate() {
            let name = format!("{}_{}.smt2", q.context, q.kind);
            let path = dir.join(
                if queries
                    .iter()
                    .filter(|qq| qq.context == q.context && qq.kind == q.kind)
                    .count()
                    > 1
                {
                    format!("{}_{}_{}.smt2", q.context, q.kind, i)
                } else {
                    name
                },
            );
            if let Err(e) = fs::write(&path, &q.script) {
                if output_mode == OutputMode::Json {
                    let report = serde_json::json!({
                        "ok": false,
                        "command": "check",
                        "error": "dump_smt_write_failed",
                        "path": path.display().to_string(),
                        "message": format!("Error writing {}: {e}", path.display()),
                    });
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    eprintln!("Error writing {}: {e}", path.display());
                }
                process::exit(2);
            }
        }
        if output_mode == OutputMode::Human {
            eprintln!("Wrote {} SMT-LIB2 file(s) to {smt_dir}/", queries.len());
        }
    }

    // --- Print stats ---
    if stats && output_mode == OutputMode::Human {
        let verified = verification_results
            .iter()
            .filter(|r| matches!(r, assura_smt::VerificationResult::Verified { .. }))
            .count();
        let counterexamples = verification_results
            .iter()
            .filter(|r| matches!(r, assura_smt::VerificationResult::Counterexample { .. }))
            .count();
        let timeouts = verification_results
            .iter()
            .filter(|r| matches!(r, assura_smt::VerificationResult::Timeout { .. }))
            .count();
        let unknowns = verification_results
            .iter()
            .filter(|r| matches!(r, assura_smt::VerificationResult::Unknown { .. }))
            .count();
        let total_ms = timing.parse_ms
            + timing.resolve_ms.unwrap_or(0.0)
            + timing.typecheck_ms.unwrap_or(0.0)
            + verify_ms;

        eprintln!();
        eprintln!("=== Verification Statistics ===");
        eprintln!("  Clauses:         {}", verification_results.len());
        eprintln!("  Verified:        {verified}");
        eprintln!("  Counterexamples: {counterexamples}");
        eprintln!("  Timeouts:        {timeouts}");
        eprintln!("  Unknown:         {unknowns}");
        eprintln!();
        eprintln!(
            "  Parse time:      {:.2}ms ({} tokens)",
            timing.parse_ms, timing.token_count
        );
        if let Some(ms) = timing.resolve_ms {
            eprintln!("  Resolve time:    {ms:.2}ms");
        }
        if let Some(ms) = timing.typecheck_ms {
            eprintln!("  Type-check time: {ms:.2}ms");
        }
        eprintln!("  Verify time:     {verify_ms:.2}ms");
        eprintln!("  Total time:      {total_ms:.2}ms");
    }

    // --- Report (JSON output; human output handled by verify_and_report) ---
    if output_mode == OutputMode::Json {
        {
            // Build verification summary for JSON output
            let verification_json: Vec<serde_json::Value> = verification_results
                .iter()
                .map(assura_smt::VerificationResult::to_json_value)
                .collect();

            // Build file metadata
            let mut file_info = serde_json::json!({
                "file": filename,
                "success": !has_errors,
            });
            // Vacuous success for agents/automation (mirrors human-mode wording).
            // Empty sources and contracts with zero SMT results look like
            // "success" without proving anything.
            if !has_errors {
                let no_decls = file.as_ref().is_some_and(|f| f.decls.is_empty());
                let has_clause_kinds = file
                    .as_ref()
                    .is_some_and(assura_smt::has_verifiable_clauses);
                let has_contracts = file
                    .as_ref()
                    .is_some_and(|f| !assura_smt::display::collect_contract_names(f).is_empty());
                let contracts_without_results =
                    layer >= 1 && verification_results.is_empty() && has_contracts;
                if no_decls {
                    file_info["vacuous"] = serde_json::json!(true);
                    file_info["vacuous_reason"] =
                        serde_json::json!("no contracts or functions to verify");
                } else if contracts_without_results {
                    file_info["vacuous"] = serde_json::json!(true);
                    file_info["vacuous_reason"] = if has_clause_kinds {
                        serde_json::json!("no SMT proof obligations; add ensures or invariant")
                    } else {
                        serde_json::json!("no verifiable clauses")
                    };
                }
            }
            if let Some(ref f) = file {
                if let Some(ref p) = f.project {
                    file_info["project"] = serde_json::json!({
                        "name": p.name,
                        "profile": p.profile,
                    });
                }
                if let Some(ref m) = f.module {
                    file_info["module"] = serde_json::json!(m.path.join("."));
                }
                file_info["imports"] = serde_json::json!(f.imports.len());
                let mut decl_counts = serde_json::Map::new();
                use assura_parser::ast::{
                    BindDecl, ContractDecl, DeclVisitor, EnumDef, ExternDecl, FnDef, ServiceDecl,
                    TypeDef, walk_decls,
                };
                struct DeclCounts {
                    contracts: u32,
                    types: u32,
                    enums: u32,
                    externs: u32,
                    fns: u32,
                    services: u32,
                }
                impl DeclVisitor for DeclCounts {
                    fn visit_contract(&mut self, _: &ContractDecl) {
                        self.contracts += 1;
                    }
                    fn visit_type_def(&mut self, _: &TypeDef) {
                        self.types += 1;
                    }
                    fn visit_enum_def(&mut self, _: &EnumDef) {
                        self.enums += 1;
                    }
                    fn visit_extern(&mut self, _: &ExternDecl) {
                        self.externs += 1;
                    }
                    fn visit_bind(&mut self, _: &BindDecl) {
                        self.externs += 1;
                    }
                    fn visit_fn_def(&mut self, _: &FnDef) {
                        self.fns += 1;
                    }
                    fn visit_service(&mut self, _: &ServiceDecl) {
                        self.services += 1;
                    }
                }
                let mut counts = DeclCounts {
                    contracts: 0,
                    types: 0,
                    enums: 0,
                    externs: 0,
                    fns: 0,
                    services: 0,
                };
                walk_decls(&mut counts, &f.decls);
                let (contracts, types, enums, externs, fns, services) = (
                    counts.contracts,
                    counts.types,
                    counts.enums,
                    counts.externs,
                    counts.fns,
                    counts.services,
                );
                if contracts > 0 {
                    decl_counts.insert("contracts".into(), contracts.into());
                }
                if types > 0 {
                    decl_counts.insert("types".into(), types.into());
                }
                if enums > 0 {
                    decl_counts.insert("enums".into(), enums.into());
                }
                if externs > 0 {
                    decl_counts.insert("externs".into(), externs.into());
                }
                if fns > 0 {
                    decl_counts.insert("functions".into(), fns.into());
                }
                if services > 0 {
                    decl_counts.insert("services".into(), services.into());
                }
                file_info["declarations"] = serde_json::Value::Object(decl_counts);
            }
            if let Some(ref r) = resolved {
                let user_symbols = r
                    .symbols
                    .symbols
                    .iter()
                    .filter(|s| s.kind != assura_resolve::SymbolKind::BuiltinType)
                    .count();
                file_info["resolve"] = serde_json::json!({
                    "status": "ok",
                    "symbols": user_symbols,
                });
            }
            if let Some(ref t) = typed {
                file_info["typecheck"] = serde_json::json!({
                    "status": "ok",
                    "bindings": t.type_env.len(),
                });
                // IR surface for agents (mirrors human `check -v` ir: lines).
                if layer >= 1 {
                    let listing = collect_ir_surface_listing(filename, t);
                    let notes: serde_json::Map<String, serde_json::Value> = listing
                        .notes
                        .into_iter()
                        .map(|(k, v)| (k, serde_json::Value::String(v)))
                        .collect();
                    file_info["ir"] = serde_json::json!({
                        "colocated": listing.colocated,
                        "synthesized": listing.synthesized,
                        "synth_notes": notes,
                    });
                }
            }

            let mut output = serde_json::json!({
                "file_info": file_info,
                "diagnostics": diagnostics,
                "verification": verification_json,
                "layer": layer,
            });
            if let Some((ref cfg, ref root)) = config {
                output["config"] = serde_json::json!({
                    "project_root": root.display().to_string(),
                    "package": {
                        "name": cfg.package.name,
                        "version": cfg.package.version,
                    },
                    "build": {
                        "target": cfg.build.target,
                        "output": cfg.build.output,
                    },
                    "verify": {
                        "smt_solver": cfg.verify.smt_solver,
                        "layer": cfg.verify.layer,
                        "timeout": cfg.verify.timeout,
                    },
                });
            }
            if stats {
                let verified = verification_results
                    .iter()
                    .filter(|r| matches!(r, assura_smt::VerificationResult::Verified { .. }))
                    .count();
                let counterexamples = verification_results
                    .iter()
                    .filter(|r| matches!(r, assura_smt::VerificationResult::Counterexample { .. }))
                    .count();
                let timeouts = verification_results
                    .iter()
                    .filter(|r| matches!(r, assura_smt::VerificationResult::Timeout { .. }))
                    .count();
                let unknowns = verification_results
                    .iter()
                    .filter(|r| matches!(r, assura_smt::VerificationResult::Unknown { .. }))
                    .count();
                let total_ms = timing.parse_ms
                    + timing.resolve_ms.unwrap_or(0.0)
                    + timing.typecheck_ms.unwrap_or(0.0)
                    + verify_ms;
                output["stats"] = serde_json::json!({
                    "clauses": verification_results.len(),
                    "verified": verified,
                    "counterexamples": counterexamples,
                    "timeouts": timeouts,
                    "unknowns": unknowns,
                    "timing_ms": {
                        "parse": timing.parse_ms,
                        "resolve": timing.resolve_ms,
                        "typecheck": timing.typecheck_ms,
                        "verify": verify_ms,
                        "total": total_ms,
                    },
                    "tokens": timing.token_count,
                });
            }
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
    }

    process::exit(if has_errors { 1 } else { 0 });
}
