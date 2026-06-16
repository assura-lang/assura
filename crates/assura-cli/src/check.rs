use super::*;

// `assura check-rust <path> [--json] [--layer 0|1]`
// ---------------------------------------------------------------------------

pub(crate) fn run_check_rust(
    path: &str,
    output_mode: OutputMode,
    verbosity: Verbosity,
    layer: u8,
    solver: Option<assura_smt::SolverChoice>,
) {
    use assura_rust_analyzer::{AnnotatedItem, AnnotatedItemKind};

    let p = Path::new(path);

    // Collect all annotated items from file or directory
    let file_items: Vec<(std::path::PathBuf, Vec<AnnotatedItem>)> = if p.is_dir() {
        match assura_rust_analyzer::scan_directory(p) {
            Ok(results) => results,
            Err(e) => {
                eprintln!("Error scanning directory: {e}");
                process::exit(1);
            }
        }
    } else if p.is_file() {
        match assura_rust_analyzer::parse_rust_file(p) {
            Ok(items) if !items.is_empty() => vec![(p.to_path_buf(), items)],
            Ok(_) => {
                if verbosity != Verbosity::Quiet {
                    println!("{path}: no inline contract annotations found");
                }
                return;
            }
            Err(e) => {
                eprintln!("Error parsing {path}: {e}");
                process::exit(1);
            }
        }
    } else {
        eprintln!("Error: {path} is not a file or directory");
        process::exit(1);
    };

    if file_items.is_empty() {
        if verbosity != Verbosity::Quiet {
            println!("No inline contract annotations found in {path}");
        }
        return;
    }

    let solver_choice = solver.unwrap_or(assura_smt::SolverChoice::Z3);
    let mut total_clauses = 0usize;
    let mut total_verified = 0usize;
    let mut total_errors = 0usize;
    let mut all_results: Vec<serde_json::Value> = Vec::new();

    for (file_path, items) in &file_items {
        let file_display = file_path.display();
        if verbosity == Verbosity::Verbose {
            println!("Checking {file_display} ({} annotated items)", items.len());
        }

        for item in items {
            let (item_name, item_kind_str) = match &item.kind {
                AnnotatedItemKind::Function { name, .. } => (name.clone(), "function"),
                AnnotatedItemKind::Struct { name, .. } => (name.clone(), "struct"),
                AnnotatedItemKind::ImplBlock { self_type, .. } => (self_type.clone(), "impl block"),
            };

            // Build a synthetic .assura contract from the annotations
            let mut contract_source = format!("contract {item_name} {{\n");

            // Add requires clauses
            for clause in &item.contract.requires {
                contract_source.push_str(&format!("  requires {{ {} }}\n", clause.body));
                total_clauses += 1;
            }
            // Add ensures clauses
            for clause in &item.contract.ensures {
                contract_source.push_str(&format!("  ensures {{ {} }}\n", clause.body));
                total_clauses += 1;
            }
            // Add invariant clauses
            for clause in &item.contract.invariants {
                contract_source.push_str(&format!("  invariant {{ {} }}\n", clause.body));
                total_clauses += 1;
            }
            // Add effects clauses
            for clause in &item.contract.effects {
                contract_source.push_str(&format!("  effects {{ {} }}\n", clause.body));
                total_clauses += 1;
            }
            // Add decreases clauses
            for clause in &item.contract.decreases {
                contract_source.push_str(&format!("  decreases {{ {} }}\n", clause.body));
                total_clauses += 1;
            }

            // Add input parameters for functions
            if let AnnotatedItemKind::Function {
                params,
                return_type,
                ..
            } = &item.kind
            {
                // Map Rust types to Assura types for the synthetic contract
                let param_strs: Vec<String> = params
                    .iter()
                    .filter(|p| p.name != "self")
                    .map(|p| format!("{}: {}", p.name, rust_type_to_assura(&p.ty)))
                    .collect();
                if !param_strs.is_empty() {
                    contract_source.push_str(&format!("  requires({})\n", param_strs.join(", ")));
                }
                if let Some(ret) = return_type {
                    let assura_ret = rust_type_to_assura(ret);
                    contract_source.push_str(&format!("  output(result: {assura_ret})\n"));
                }
            }

            contract_source.push_str("}\n");

            // Run the Assura pipeline on the synthetic contract
            let config = assura_config::CompilerConfig::default();
            let output =
                assura_pipeline::compile(&contract_source, &file_display.to_string(), &config);

            // Check for parse/type errors in the synthetic contract
            let parse_ok = !output.has_errors;

            if parse_ok
                && layer >= 1
                && let Some(ref typed) = output.typed
                && let Some(ref file_ast) = output.file
            {
                // Run SMT verification
                let source_for_verify = contract_source.clone();
                let mut diags = Vec::new();
                let mut has_err = false;
                verify_and_report(VerifyContext {
                    filename: &file_display.to_string(),
                    source: &source_for_verify,
                    typed: &Some(typed.clone()),
                    file: &Some(file_ast.clone()),
                    diagnostics: &mut diags,
                    has_errors: &mut has_err,
                    output_mode,
                    verbosity,
                    layer,
                    solver: solver_choice,
                });
                if has_err {
                    total_errors += diags.len();
                } else {
                    total_verified += item.contract.clause_count();
                }
            } else if parse_ok {
                // Layer 0: structural checking only (already done by pipeline)
                total_verified += item.contract.clause_count();
            }

            if output_mode == OutputMode::Json {
                let clauses: Vec<serde_json::Value> = item
                    .contract
                    .requires
                    .iter()
                    .map(|c| clause_to_json(c, "requires"))
                    .chain(
                        item.contract
                            .ensures
                            .iter()
                            .map(|c| clause_to_json(c, "ensures")),
                    )
                    .chain(
                        item.contract
                            .invariants
                            .iter()
                            .map(|c| clause_to_json(c, "invariant")),
                    )
                    .chain(
                        item.contract
                            .effects
                            .iter()
                            .map(|c| clause_to_json(c, "effects")),
                    )
                    .chain(
                        item.contract
                            .decreases
                            .iter()
                            .map(|c| clause_to_json(c, "decreases")),
                    )
                    .collect();

                all_results.push(serde_json::json!({
                    "file": file_display.to_string(),
                    "item": item_name,
                    "kind": item_kind_str,
                    "line": item.line,
                    "clauses": clauses,
                    "status": if total_errors > 0 { "error" } else { "ok" },
                }));
            } else if verbosity != Verbosity::Quiet {
                println!(
                    "  {item_kind_str} `{item_name}` (line {}): {} clause(s)",
                    item.line,
                    item.contract.clause_count()
                );
            }
        }
    }

    // Summary
    if output_mode == OutputMode::Json {
        let summary = serde_json::json!({
            "files": file_items.len(),
            "items": file_items.iter().map(|(_, items)| items.len()).sum::<usize>(),
            "clauses": total_clauses,
            "verified": total_verified,
            "errors": total_errors,
            "results": all_results,
        });
        println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    } else if verbosity != Verbosity::Quiet {
        println!();
        println!(
            "check-rust: {} file(s), {} annotated item(s), {} clause(s)",
            file_items.len(),
            file_items
                .iter()
                .map(|(_, items)| items.len())
                .sum::<usize>(),
            total_clauses
        );
        if total_errors > 0 {
            eprintln!("{total_errors} verification error(s)");
            process::exit(1);
        } else {
            println!("All clauses checked successfully");
        }
    } else if total_errors > 0 {
        process::exit(1);
    }
}

/// Map common Rust types to Assura types for synthetic contracts.
pub(crate) fn rust_type_to_assura(ty: &str) -> &str {
    let trimmed = ty.trim();
    match trimmed {
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" => "Int",
        "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => "Nat",
        "f32" | "f64" => "Float",
        "bool" => "Bool",
        "String" | "&str" | "& str" => "String",
        "()" => "Unit",
        _ => "Int", // Default fallback
    }
}

/// Convert a contract clause to a JSON value.
pub(crate) fn clause_to_json(
    clause: &assura_rust_analyzer::ContractClause,
    kind: &str,
) -> serde_json::Value {
    serde_json::json!({
        "kind": kind,
        "body": clause.body,
        "offset": clause.offset,
    })
}

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
    } = opts;
    // Load project config (assura.toml) if available
    let project = load_project_config(Path::new(filename));
    let config_layer = project.as_ref().map(|(c, _)| c.verify.layer);

    // Verification layer: CLI flag > config file > default (1)
    // 255 is the sentinel for "not specified on CLI"
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
        run_watch_loop(filename, output_mode, verbosity, layer);
        // run_watch_loop never returns (loops until interrupted)
    }

    // --- Project mode: detect directory or project root ---
    let path = Path::new(filename);
    if path.is_dir() {
        // Directory mode: check all .assura files in the project
        run_check_project(path, output_mode, verbosity, &compiler_config);
        return;
    }

    let source = fs::read_to_string(filename).unwrap_or_else(|e| {
        if output_mode == OutputMode::Json {
            let diag = assura_diagnostics::Diagnostic::error("A01000", format!("{e}"), 0..0)
                .with_file(filename);
            println!("{}", serde_json::to_string_pretty(&[diag]).unwrap());
        } else {
            eprintln!("Error: {filename}: {e}");
        }
        process::exit(2);
    });

    // --- Run shared pipeline ---
    let CompilationResult {
        file,
        resolved,
        hir: _,
        typed,
        mut diagnostics,
        mut has_errors,
        timing,
        ..
    } = compile_with_config(&source, filename, &compiler_config);

    if verbosity == Verbosity::Verbose && output_mode == OutputMode::Human {
        if let Some((ref cfg, ref root)) = config {
            eprintln!(
                "Project: {} v{} ({})",
                cfg.package.name,
                cfg.package.version,
                root.display()
            );
            eprintln!(
                "  config: layer={}, solver={}, timeout={}ms, output={}",
                cfg.verify.layer, cfg.verify.smt_solver, cfg.verify.timeout, cfg.build.output
            );
            eprintln!();
        }
        eprintln!("Pipeline timing for {filename}:");
        if let Some(ref f) = file {
            eprintln!(
                "  parse:     {} tokens, {} declaration(s), {} import(s) ({:.2}ms)",
                timing.token_count,
                f.decls.len(),
                f.imports.len(),
                timing.parse_ms
            );
        } else {
            eprintln!(
                "  parse:     {} tokens, failed ({:.2}ms)",
                timing.token_count, timing.parse_ms
            );
        }
        if let Some(resolve_ms) = timing.resolve_ms {
            if let Some(ref r) = resolved {
                let user_symbols = r
                    .symbols
                    .symbols
                    .iter()
                    .filter(|s| s.kind != assura_resolve::SymbolKind::BuiltinType)
                    .count();
                eprintln!("  resolve:   {user_symbols} symbol(s) ({resolve_ms:.2}ms)");
            } else {
                eprintln!("  resolve:   failed ({resolve_ms:.2}ms)");
            }
        }
        if let Some(hir_ms) = timing.hir_ms {
            eprintln!("  hir:       ({hir_ms:.2}ms)");
        }
        if let Some(typecheck_ms) = timing.typecheck_ms {
            if let Some(ref td) = typed {
                eprintln!(
                    "  typecheck: {} binding(s) ({typecheck_ms:.2}ms)",
                    td.type_env.len()
                );
            } else {
                eprintln!("  typecheck: failed ({typecheck_ms:.2}ms)");
            }
        }
        eprintln!();
    }

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
        layer,
        solver,
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
        fs::create_dir_all(dir).unwrap_or_else(|e| {
            eprintln!("Error: cannot create {smt_dir}: {e}");
            process::exit(2);
        });
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
            fs::write(&path, &q.script).unwrap_or_else(|e| {
                eprintln!("Error writing {}: {e}", path.display());
            });
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
            + timing.hir_ms.unwrap_or(0.0)
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
        if let Some(ms) = timing.hir_ms {
            eprintln!("  HIR lower time:  {ms:.2}ms");
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
                .map(|vr| match vr {
                    assura_smt::VerificationResult::Verified { clause_desc } => {
                        serde_json::json!({
                            "status": "verified",
                            "clause": clause_desc,
                        })
                    }
                    assura_smt::VerificationResult::Counterexample {
                        clause_desc,
                        model,
                        counter_model,
                    } => {
                        let mut val = serde_json::json!({
                            "status": "counterexample",
                            "clause": clause_desc,
                            "model": model,
                        });
                        if let Some(cm) = counter_model {
                            let vars: serde_json::Map<String, serde_json::Value> = cm
                                .variables
                                .iter()
                                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                                .collect();
                            val["variables"] = serde_json::Value::Object(vars);
                        }
                        val
                    }
                    assura_smt::VerificationResult::Timeout { clause_desc } => {
                        serde_json::json!({
                            "status": "timeout",
                            "clause": clause_desc,
                        })
                    }
                    assura_smt::VerificationResult::Unknown {
                        clause_desc,
                        reason,
                    } => {
                        serde_json::json!({
                            "status": "unknown",
                            "clause": clause_desc,
                            "reason": reason,
                        })
                    }
                })
                .collect();

            // Build file metadata
            let mut file_info = serde_json::json!({
                "file": filename,
                "success": !has_errors,
            });
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
                let (mut contracts, mut types, mut enums, mut externs, mut fns, mut services) =
                    (0u32, 0, 0, 0, 0, 0);
                for d in &f.decls {
                    match &d.node {
                        Decl::Contract(_) => contracts += 1,
                        Decl::TypeDef(_) => types += 1,
                        Decl::EnumDef(_) => enums += 1,
                        Decl::Extern(_) | Decl::Bind(_) => externs += 1,
                        Decl::FnDef(_) => fns += 1,
                        Decl::Service(_) => services += 1,
                        Decl::Prophecy(_) => {}
                        Decl::CodecRegistry(_) => {}
                        Decl::Block { .. } => {}
                    }
                }
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
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
    }

    process::exit(if has_errors { 1 } else { 0 });
}

// ---------------------------------------------------------------------------
// Watch mode
// ---------------------------------------------------------------------------

/// Shared verification + reporting logic used by both `run_check` and
/// `check_file_once` (watch mode). Returns the verification results and
/// whether errors were found.
pub(crate) fn verify_and_report(ctx: VerifyContext<'_>) -> Vec<assura_smt::VerificationResult> {
    let VerifyContext {
        filename,
        source,
        typed,
        file,
        diagnostics,
        has_errors,
        output_mode,
        verbosity,
        layer,
        solver,
    } = ctx;
    // Short-circuit: skip cache/thread-pool init when there are no
    // verifiable clauses (requires/ensures/invariant) in the source.
    let has_clauses = file
        .as_ref()
        .is_some_and(assura_smt::has_verifiable_clauses);

    let mut verification_results = if layer >= 1 && has_clauses {
        if let Some(typed) = typed {
            let cache_dir = std::path::Path::new(filename)
                .parent()
                .unwrap_or(std::path::Path::new("."));
            let verify_cache = assura_smt::VerificationCache::new(cache_dir);
            assura_smt::verify_parallel_with_solver(typed, &verify_cache, solver)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    if let Some(typed) = typed {
        verification_results.extend(assura_smt::display::dispatch_decrease_checks(typed));
    }

    if let Some(typed) = typed {
        let qwarnings = assura_smt::validate_quantifier_bounds(typed);
        for w in &qwarnings {
            diagnostics.push(
                assura_diagnostics::Diagnostic::warning(
                    "A05200",
                    format!(
                        "unbounded quantifier in {}: {} ({})",
                        w.context, w.domain_desc, w.reason
                    ),
                    0..0,
                )
                .with_file(filename),
            );
        }
    }

    for vr in &verification_results {
        match vr {
            assura_smt::VerificationResult::Counterexample {
                clause_desc,
                model,
                counter_model,
            } => {
                *has_errors = true;
                let summary = format_counterexample_summary(counter_model, model);
                diagnostics.push(
                    assura_diagnostics::Diagnostic::error(
                        "A05100",
                        format!("verification failed for {clause_desc}: {summary}"),
                        0..0,
                    )
                    .with_file(filename),
                );
            }
            assura_smt::VerificationResult::Timeout { clause_desc } => {
                *has_errors = true;
                diagnostics.push(
                    assura_diagnostics::Diagnostic::error(
                        "A05100",
                        format!("verification timeout for {clause_desc}"),
                        0..0,
                    )
                    .with_file(filename),
                );
            }
            assura_smt::VerificationResult::Unknown {
                clause_desc,
                reason,
            } => {
                if is_known_smt_limitation(reason) {
                    diagnostics.push(
                        assura_diagnostics::Diagnostic::warning(
                            "A05100",
                            format!("verification skipped for {clause_desc}: {reason}"),
                            0..0,
                        )
                        .with_file(filename),
                    );
                } else {
                    *has_errors = true;
                    diagnostics.push(
                        assura_diagnostics::Diagnostic::error(
                            "A05100",
                            format!("verification inconclusive for {clause_desc}: {reason}"),
                            0..0,
                        )
                        .with_file(filename),
                    );
                }
            }
            assura_smt::VerificationResult::Verified { .. } => {}
        }
    }

    if output_mode == OutputMode::Human {
        let non_lex: Vec<_> = diagnostics.iter().filter(|d| d.code != "A01001").collect();
        if *has_errors || verbosity != Verbosity::Quiet {
            for d in &non_lex {
                assura_diagnostics::render_diagnostic(d, filename, source);
            }
        }

        if verbosity != Verbosity::Quiet {
            if !verification_results.is_empty() {
                eprintln!();
                eprintln!("Verification ({} clause(s)):", verification_results.len());
                let _ = assura_smt::display::write_grouped_verification(
                    &mut std::io::stderr(),
                    &verification_results,
                    "  ",
                );
            } else if layer == 0 {
                eprintln!();
                eprintln!("Verification skipped (--layer 0: structural checks only)");
            } else if layer >= 1
                && let Some(f) = file
            {
                let contract_names = assura_smt::display::collect_contract_names(f);
                if !contract_names.is_empty() {
                    eprintln!();
                    eprintln!("Verification:");
                    for name in &contract_names {
                        eprintln!("  {name}:  (no verifiable clauses)");
                    }
                }
            }

            if !*has_errors {
                eprintln!("{filename}: check passed (no errors)");
            } else {
                eprintln!("{filename}: {} error(s) found", diagnostics.len());
            }
        } else if *has_errors {
            eprintln!("{filename}: {} error(s) found", diagnostics.len());
        }
    }

    verification_results
}

/// Check a single file and print results. Returns true if there were errors.
pub(crate) fn check_file_once(
    filename: &str,
    output_mode: OutputMode,
    verbosity: Verbosity,
    layer: u8,
) -> bool {
    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {filename}: {e}");
            return true;
        }
    };

    let CompilationResult {
        file,
        resolved,
        hir,
        typed,
        mut diagnostics,
        mut has_errors,
        timing,
        ..
    } = compile(&source, filename);

    if verbosity == Verbosity::Verbose && output_mode == OutputMode::Human {
        eprintln!("Pipeline timing for {filename}:");
        if let Some(ref f) = file {
            eprintln!(
                "  parse:     {} tokens, {} declaration(s), {} import(s) ({:.2}ms)",
                timing.token_count,
                f.decls.len(),
                f.imports.len(),
                timing.parse_ms
            );
        } else {
            eprintln!(
                "  parse:     {} tokens, failed ({:.2}ms)",
                timing.token_count, timing.parse_ms
            );
        }
        if let Some(resolve_ms) = timing.resolve_ms {
            if let Some(ref r) = resolved {
                let user_symbols = r
                    .symbols
                    .symbols
                    .iter()
                    .filter(|s| s.kind != assura_resolve::SymbolKind::BuiltinType)
                    .count();
                eprintln!("  resolve:   {user_symbols} symbol(s) ({resolve_ms:.2}ms)");
            } else {
                eprintln!("  resolve:   failed ({resolve_ms:.2}ms)");
            }
        }
        if let Some(hir_ms) = timing.hir_ms {
            if let Some(ref h) = hir {
                eprintln!("  hir:       {} decl(s) ({hir_ms:.2}ms)", h.decls.len());
            } else {
                eprintln!("  hir:       skipped ({hir_ms:.2}ms)");
            }
        }
        if let Some(typecheck_ms) = timing.typecheck_ms {
            if let Some(ref td) = typed {
                eprintln!(
                    "  typecheck: {} binding(s) ({typecheck_ms:.2}ms)",
                    td.type_env.len()
                );
            } else {
                eprintln!("  typecheck: failed ({typecheck_ms:.2}ms)");
            }
        }
        eprintln!();
    }

    // These variables are used conditionally in verbose mode above.
    // Explicitly mark as intentionally unused after that point.
    let _resolved = resolved;
    let _hir = hir;
    let _timing = timing;

    verify_and_report(VerifyContext {
        filename,
        source: &source,
        typed: &typed,
        file: &file,
        diagnostics: &mut diagnostics,
        has_errors: &mut has_errors,
        output_mode,
        verbosity,
        layer,
        solver: assura_smt::SolverChoice::Z3,
    });

    has_errors
}

/// Compute a simple content hash for incremental change detection.
pub(crate) fn content_hash(source: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Run check in watch mode: check once, then watch for file changes.
/// Uses IncrementalCompiler to skip re-checks when file content is unchanged.
pub(crate) fn run_watch_loop(
    filename: &str,
    output_mode: OutputMode,
    verbosity: Verbosity,
    layer: u8,
) -> ! {
    use notify::{Event, EventKind, RecursiveMode, Watcher};

    let path = Path::new(filename).canonicalize().unwrap_or_else(|e| {
        eprintln!("Error: cannot resolve path {filename}: {e}");
        process::exit(2);
    });

    // Set up incremental compiler to track file hashes
    let mut incremental = assura_smt::IncrementalCompiler::new();
    let mut last_hash = String::new();

    // Initial check
    eprintln!("[watch] Checking {filename}...");
    eprintln!();
    if let Ok(source) = fs::read_to_string(filename) {
        last_hash = content_hash(&source);
        incremental.register_module(filename.to_string(), last_hash.clone());
    }
    // In watch mode, we continue regardless of errors (intentionally ignoring result)
    let _had_errors = check_file_once(filename, output_mode, verbosity, layer);
    incremental.mark_checked(filename, 1);
    eprintln!();
    eprintln!("[watch] Watching {filename} for changes. Press Ctrl+C to stop.");

    let (tx, rx) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            // Only trigger on modify/create events
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                let _ = tx.send(());
            }
        }
    })
    .unwrap_or_else(|e| {
        eprintln!("Error: failed to create file watcher: {e}");
        process::exit(2);
    });

    // Watch the file's parent directory to catch renames/replacements
    let watch_dir = path.parent().unwrap_or(&path);
    watcher
        .watch(watch_dir, RecursiveMode::NonRecursive)
        .unwrap_or_else(|e| {
            eprintln!("Error: failed to watch {}: {e}", watch_dir.display());
            process::exit(2);
        });

    let mut iteration: u64 = 2;
    loop {
        // Wait for a change event
        let _ = rx.recv();

        // Debounce: drain any additional events that arrive within 100ms
        while rx.recv_timeout(Duration::from_millis(100)).is_ok() {}

        // Check if content actually changed (saves without edits, editor auto-format, etc.)
        let new_hash = fs::read_to_string(filename)
            .map(|s| content_hash(&s))
            .unwrap_or_default();

        if new_hash == last_hash && !new_hash.is_empty() {
            if verbosity == Verbosity::Verbose {
                eprintln!("[watch] File saved but content unchanged, skipping re-check.");
            }
            continue;
        }

        // Content changed: update incremental state
        last_hash = new_hash;
        incremental.mark_changed(filename);

        if verbosity == Verbosity::Verbose {
            let dirty = incremental.dirty_modules();
            eprintln!(
                "[watch] {} dirty module(s): {}",
                dirty.len(),
                dirty.join(", ")
            );
        }

        // Clear screen and re-check
        eprint!("\x1B[2J\x1B[H");
        eprintln!("[watch] File changed, re-checking {filename}...");
        eprintln!();
        let _had_errors = check_file_once(filename, output_mode, verbosity, layer);
        incremental.mark_checked(filename, iteration);
        iteration += 1;
        eprintln!();
        eprintln!("[watch] Watching for changes. Press Ctrl+C to stop.");
    }
}

// ---------------------------------------------------------------------------
// Project-mode check: resolve and type-check all .assura files in a project
// ---------------------------------------------------------------------------

pub(crate) fn run_check_project(
    project_dir: &Path,
    output_mode: OutputMode,
    _verbosity: Verbosity,
    config: &CompilerConfig,
) {
    let project_root = if project_dir.join("assura.toml").exists() {
        project_dir.to_path_buf()
    } else {
        assura_resolve::find_project_root(project_dir).unwrap_or_else(|| project_dir.to_path_buf())
    };

    if output_mode == OutputMode::Human {
        eprintln!("Checking project at {}", project_root.display());
    }

    let (resolved_files, warnings) =
        match assura_resolve::discover_and_resolve_project(&project_root) {
            Ok(pair) => pair,
            Err(errors) => {
                for e in &errors {
                    eprintln!("Error: {e}");
                }
                process::exit(1);
            }
        };

    for w in &warnings {
        if output_mode == OutputMode::Human {
            eprintln!("Warning: {w}");
        }
    }

    let mut total_errors = 0usize;
    let mut total_modules = 0usize;
    let mut total_bindings = 0usize;

    // Type-check each resolved file with cross-module type information
    for (module_path, resolved) in &resolved_files {
        total_modules += 1;
        match assura_types::type_check_with_modules(resolved, &resolved_files, &config.type_check) {
            Ok(typed) => {
                let bindings = typed.type_env.len();
                total_bindings += bindings;
                if output_mode == OutputMode::Human {
                    eprintln!(
                        "OK  {module_path}: {} symbol(s), {bindings} binding(s)",
                        resolved.symbols.symbols.len()
                    );
                }
            }
            Err(errors) => {
                total_errors += errors.len();
                if output_mode == OutputMode::Human {
                    eprintln!("ERR {module_path}: {} error(s)", errors.len());
                    for err in &errors {
                        eprintln!("  {}: {}", err.code, err.message);
                    }
                }
            }
        }
    }

    if output_mode == OutputMode::Human {
        eprintln!();
        eprintln!(
            "Project: {total_modules} module(s), {total_bindings} binding(s), {total_errors} error(s)"
        );
    }

    if total_errors > 0 {
        process::exit(1);
    }
}

// ---------------------------------------------------------------------------

/// Returns `true` if the given `VerificationResult::Unknown` reason represents
/// a known compiler limitation (warning, exit 0) rather than a genuine solver
/// inconclusive result (error, exit 1).
fn is_known_smt_limitation(reason: &str) -> bool {
    reason.contains("not yet encoded in SMT")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_classification_known_limitation_is_warning() {
        assert!(is_known_smt_limitation(
            "clause uses features not yet encoded in SMT (method call, deep field chain)"
        ));
    }

    #[test]
    fn unknown_classification_solver_reason_is_error() {
        assert!(!is_known_smt_limitation("non-linear arithmetic"));
        assert!(!is_known_smt_limitation(
            "Z3 not available (compiled without z3-verify feature)"
        ));
        assert!(!is_known_smt_limitation(
            "could not encode clause to SMT-LIB2"
        ));
        assert!(!is_known_smt_limitation("no result from solver"));
    }

    #[test]
    fn unknown_classification_boundary_near_miss() {
        assert!(!is_known_smt_limitation("clause not encoded in SMT yet"));
        assert!(!is_known_smt_limitation("not yet supported in SMT"));
        assert!(!is_known_smt_limitation("features not encoded"));
    }

    #[test]
    fn unknown_classification_diagnostic_output() {
        let filename = "test.assura";
        let clause_desc = "TestContract: ensures";

        // Warning path: known limitation
        let reason = "clause uses features not yet encoded in SMT (method call)";
        let mut has_errors = false;
        let diag = if is_known_smt_limitation(reason) {
            assura_diagnostics::Diagnostic::warning(
                "A05100",
                format!("verification skipped for {clause_desc}: {reason}"),
                0..0,
            )
            .with_file(filename)
        } else {
            has_errors = true;
            assura_diagnostics::Diagnostic::error(
                "A05100",
                format!("verification inconclusive for {clause_desc}: {reason}"),
                0..0,
            )
            .with_file(filename)
        };
        assert!(!has_errors, "known limitation should not set has_errors");
        assert!(diag.message.starts_with("verification skipped"));

        // Error path: solver inconclusive
        let reason2 = "non-linear arithmetic";
        let mut has_errors2 = false;
        let diag2 = if is_known_smt_limitation(reason2) {
            assura_diagnostics::Diagnostic::warning(
                "A05100",
                format!("verification skipped for {clause_desc}: {reason2}"),
                0..0,
            )
            .with_file(filename)
        } else {
            has_errors2 = true;
            assura_diagnostics::Diagnostic::error(
                "A05100",
                format!("verification inconclusive for {clause_desc}: {reason2}"),
                0..0,
            )
            .with_file(filename)
        };
        assert!(has_errors2, "solver inconclusive should set has_errors");
        assert!(diag2.message.starts_with("verification inconclusive"));
    }
}
