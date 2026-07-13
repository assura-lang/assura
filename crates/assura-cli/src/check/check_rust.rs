//! `assura check-rust` — inline contract annotations on Rust sources.

use super::super::*;

// `assura check-rust <path> [--json] [--layer 0|1]`
// ---------------------------------------------------------------------------

/// LLM-related options for `check-rust`.
pub(crate) struct LlmOpts<'a> {
    pub llm: bool,
    pub suggest: bool,
    pub provider: &'a str,
    pub model: Option<&'a str>,
    pub public_only: bool,
    pub unsafe_only: bool,
    pub llm_verify: bool,
}

pub(crate) fn run_check_rust(
    path: &str,
    output_mode: OutputMode,
    verbosity: Verbosity,
    layer: u8,
    solver: Option<assura_smt::SolverChoice>,
    llm_opts: LlmOpts<'_>,
) {
    use assura_rust_analyzer::{AnnotatedItem, AnnotatedItemKind};

    let json = output_mode == OutputMode::Json;
    if layer > 3 {
        if json {
            let report = serde_json::json!({
                "ok": false,
                "error": "invalid_layer",
                "layer": layer,
                "message": format!(
                    "invalid --layer {layer} (expected 0=structural, 1=SMT, 2=quantified/termination, 3=BMC)"
                ),
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!(
                "Error: invalid --layer {layer} (expected 0=structural, 1=SMT, 2=quantified/termination, 3=BMC)"
            );
        }
        process::exit(2);
    }

    let p = Path::new(path);

    // Collect all annotated items from file or directory
    let file_items: Vec<(std::path::PathBuf, Vec<AnnotatedItem>)> = if p.is_dir() {
        match assura_rust_analyzer::scan_directory(p) {
            Ok(results) => results,
            Err(e) => {
                if json {
                    let report = serde_json::json!({
                        "ok": false,
                        "path": path,
                        "error": "scan_failed",
                        "message": format!("Error scanning directory: {e}"),
                    });
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    eprintln!("Error scanning directory: {e}");
                }
                process::exit(1);
            }
        }
    } else if p.is_file() {
        match assura_rust_analyzer::parse_rust_file(p) {
            Ok(items) if !items.is_empty() => vec![(p.to_path_buf(), items)],
            Ok(_) => {
                if json {
                    let report = serde_json::json!({
                        "ok": true,
                        "path": path,
                        "items": 0,
                        "message": format!("{path}: no inline contract annotations found"),
                    });
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else if verbosity != Verbosity::Quiet {
                    println!("{path}: no inline contract annotations found");
                }
                return;
            }
            Err(e) => {
                if json {
                    let report = serde_json::json!({
                        "ok": false,
                        "path": path,
                        "error": "parse_failed",
                        "message": format!("Error parsing {path}: {e}"),
                    });
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    eprintln!("Error parsing {path}: {e}");
                }
                process::exit(1);
            }
        }
    } else {
        if json {
            let report = serde_json::json!({
                "ok": false,
                "path": path,
                "error": "not_found",
                "message": format!("{path} is not a file or directory"),
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!("Error: {path} is not a file or directory");
        }
        process::exit(1);
    };

    if file_items.is_empty() {
        if json {
            let report = serde_json::json!({
                "ok": true,
                "path": path,
                "items": 0,
                "message": format!("No inline contract annotations found in {path}"),
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else if verbosity != Verbosity::Quiet {
            println!("No inline contract annotations found in {path}");
        }
        return;
    }

    let solver_choice = solver.unwrap_or(assura_smt::SolverChoice::Z3);
    let mut total_clauses = 0usize;
    let mut total_verified = 0usize;
    let mut total_errors = 0usize;
    let mut total_body_not_modeled = 0usize;
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

            // Strip trailing `//` comments from doc annotation bodies so
            // `/// @ensures result == x // identity` stays valid Assura.
            let clause_body =
                |raw: &str| -> String { raw.split("//").next().unwrap_or(raw).trim().to_string() };

            // Add requires clauses
            for clause in &item.contract.requires {
                contract_source
                    .push_str(&format!("  requires {{ {} }}\n", clause_body(&clause.body)));
                total_clauses += 1;
            }
            // Machine integer params: constrain to Rust type range so body IR
            // models (e.g. saturating/clamp to i64) match SMT unbounded Int.
            if let AnnotatedItemKind::Function { params, .. } = &item.kind {
                for p in params.iter().filter(|p| p.name != "self") {
                    if let Some((lo, hi)) = rust_int_range_bounds(&p.ty) {
                        contract_source.push_str(&format!(
                            "  requires {{ {} >= {} }}\n  requires {{ {} <= {} }}\n",
                            p.name, lo, p.name, hi
                        ));
                        total_clauses += 2;
                    }
                }
            }
            // Add ensures clauses
            for clause in &item.contract.ensures {
                contract_source
                    .push_str(&format!("  ensures {{ {} }}\n", clause_body(&clause.body)));
                total_clauses += 1;
            }
            // Add invariant clauses
            for clause in &item.contract.invariants {
                contract_source.push_str(&format!(
                    "  invariant {{ {} }}\n",
                    clause_body(&clause.body)
                ));
                total_clauses += 1;
            }
            // Add effects clauses
            for clause in &item.contract.effects {
                contract_source
                    .push_str(&format!("  effects {{ {} }}\n", clause_body(&clause.body)));
                total_clauses += 1;
            }
            // Add decreases clauses
            for clause in &item.contract.decreases {
                contract_source.push_str(&format!(
                    "  decreases {{ {} }}\n",
                    clause_body(&clause.body)
                ));
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
                    .map(|p| {
                        let assura_ty = assura_codegen::type_map::rust_type_to_assura(&p.ty);
                        format!("{}: {assura_ty}", p.name)
                    })
                    .collect();
                // Parameters must be `input(...)` so resolve registers them in
                // scope. `requires(x: Int)` is a boolean clause, not a param list
                // (dogfood: result == x never verified; A02001 undefined `x`).
                if !param_strs.is_empty() {
                    contract_source.push_str(&format!("  input({})\n", param_strs.join(", ")));
                }
                if let Some(ret) = return_type {
                    let assura_ret = assura_codegen::type_map::rust_type_to_assura(ret);
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
            let mut item_status = if parse_ok { "ok" } else { "error" };
            let mut item_verified = 0usize;
            let mut item_skipped = 0usize;
            let mut item_errors = 0usize;

            if parse_ok
                && layer >= 1
                && let Some(ref typed) = output.typed
                && let Some(ref file_ast) = output.file
            {
                // Body model for check-rust (#951 / #975):
                // 1. Co-located `{Name}.ir` beside the Rust file
                // 2. Else simple Rust body → temp `{Name}.ir` next to a temp
                //    contract (co-publish safe: uses disk load, no new APIs)
                // Without either, ensures must not claim verified.
                let has_ensures = !item.contract.ensures.is_empty();
                let colocated = assura_smt::LoadedVerifyExtras::load(file_path.as_path(), typed);
                let mut has_body_ir = colocated.loaded_names().iter().any(|n| n == &item_name);

                // Keep temp dir alive for the verify call.
                let mut body_ir_tmpdir = None;
                let mut verify_filename = file_display.to_string();
                let mut verify_source = contract_source.clone();
                let mut verify_typed = typed.clone();
                let mut verify_file = file_ast.clone();

                if !has_body_ir
                    && let Some((params, ret)) =
                        super::rust_body_ir::function_params_return(&item.kind)
                    && let Ok(rust_src) = fs::read_to_string(file_path)
                    && let Some(body) =
                        super::rust_body_ir::extract_body_return(&rust_src, &item_name)
                    && let Some(ir_text) =
                        super::rust_body_ir::try_ir_from_rust_body(&item_name, params, ret, &body)
                {
                    let dir = std::env::temp_dir().join(format!(
                        "assura-body-ir-{}-{}",
                        std::process::id(),
                        item_name
                    ));
                    let _ = fs::remove_dir_all(&dir);
                    if fs::create_dir_all(&dir).is_ok() {
                        let assura_path = dir.join(format!("{item_name}.assura"));
                        let ir_path = dir.join(format!("{item_name}.ir"));
                        if fs::write(&assura_path, &contract_source).is_ok()
                            && fs::write(&ir_path, &ir_text).is_ok()
                        {
                            let recompiled = assura_pipeline::compile(
                                &contract_source,
                                &assura_path.display().to_string(),
                                &assura_config::CompilerConfig::default(),
                            );
                            if !recompiled.has_errors
                                && let (Some(t), Some(f)) = (recompiled.typed, recompiled.file)
                            {
                                has_body_ir = true;
                                verify_filename = assura_path.display().to_string();
                                verify_source = contract_source.clone();
                                verify_typed = t;
                                verify_file = f;
                                body_ir_tmpdir = Some(dir);
                            }
                        }
                    }
                }
                let expect_body_not_modeled = has_ensures && !has_body_ir;

                let report_verbosity = if expect_body_not_modeled
                    && output_mode == OutputMode::Human
                    && verbosity != Verbosity::Quiet
                {
                    Verbosity::Quiet
                } else {
                    verbosity
                };
                let mut diags = Vec::new();
                let mut has_err = false;
                let vresults = verify_and_report(VerifyContext {
                    filename: &verify_filename,
                    source: &verify_source,
                    typed: &Some(verify_typed),
                    file: &Some(verify_file),
                    diagnostics: &mut diags,
                    has_errors: &mut has_err,
                    output_mode,
                    verbosity: report_verbosity,
                    verify_options: assura_config::VerifyOptions {
                        layer,
                        solver: solver_choice,
                        ..Default::default()
                    },
                    show_cores: false,
                    strict: false,
                });
                for r in &vresults {
                    match r {
                        assura_smt::VerificationResult::Verified { .. } => item_verified += 1,
                        assura_smt::VerificationResult::Counterexample { .. }
                        | assura_smt::VerificationResult::Timeout { .. } => {
                            item_errors += 1;
                        }
                        assura_smt::VerificationResult::Unknown { reason, .. } => {
                            if assura_smt::is_known_smt_limitation(reason) {
                                item_skipped += 1;
                            } else {
                                item_errors += 1;
                            }
                        }
                    }
                }
                // Annotation-only clauses with no SMT job still appear as "checked"
                // at layer 0 semantics when verify produced nothing for them.
                if vresults.is_empty() && !has_err {
                    item_status = "checked";
                } else if item_errors > 0 || has_err {
                    item_status = "error";
                    item_errors = item_errors.max(diags.len().max(1));
                } else if item_skipped > 0 && item_verified == 0 {
                    item_status = "skipped";
                } else if item_skipped > 0 {
                    item_status = "partial";
                } else {
                    item_status = "verified";
                }

                // Drop temp sidecars after verify (co-publish-safe disk IR path).
                if let Some(dir) = body_ir_tmpdir {
                    let _ = fs::remove_dir_all(dir);
                }

                if should_mark_body_not_modeled(
                    has_ensures,
                    has_body_ir,
                    item_status,
                    item_verified,
                    item_errors,
                ) {
                    if verbosity == Verbosity::Verbose && output_mode == OutputMode::Human {
                        eprintln!(
                            "  note: `{item_name}` has no co-located .ir and body is outside the encode surface; \
                             ensures were not proven against the Rust body (status body_not_modeled)"
                        );
                    }
                    item_skipped += item_verified;
                    item_verified = 0;
                    item_status = "body_not_modeled";
                    total_body_not_modeled += 1;
                }

                total_verified += item_verified;
                total_errors += item_errors;
            } else if parse_ok {
                // Layer 0: structural checking only (already done by pipeline)
                item_status = "checked";
                item_verified = 0;
            } else {
                total_errors += 1;
                item_errors = 1;
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
                    "status": item_status,
                    "verified": item_verified,
                    "skipped": item_skipped,
                    "errors": item_errors,
                }));
            } else if verbosity != Verbosity::Quiet {
                println!(
                    "  {item_kind_str} `{item_name}` (line {}): {} clause(s) [{item_status}]",
                    item.line,
                    item.contract.clause_count()
                );
            }
        }
    }

    // LLM analysis (opt-in)
    if llm_opts.llm || llm_opts.suggest || llm_opts.llm_verify {
        run_llm_analysis(&file_items, verbosity, &llm_opts);
    }

    // Summary
    if output_mode == OutputMode::Json {
        let summary = serde_json::json!({
            "files": file_items.len(),
            "items": file_items.iter().map(|(_, items)| items.len()).sum::<usize>(),
            "clauses": total_clauses,
            "verified": total_verified,
            "errors": total_errors,
            "body_not_modeled": total_body_not_modeled,
            "results": all_results,
            "policy": "check-rust proves annotations against co-located .ir or encoded Rust bodies (arith/if/match/wrapping/bitops/checked_*/overflowing_*/rotate/is_power_of_two/ilog/isqrt/next_power_of_two, abs/min/max/clamp/signum/saturating, PartialOrd; see CONTRIBUTING check-rust body proof)",
        });
        println!("{}", serde_json::to_string_pretty(&summary).unwrap());
        if total_errors > 0 || total_body_not_modeled > 0 {
            process::exit(1);
        }
    } else if verbosity != Verbosity::Quiet {
        println!();
        println!(
            "check-rust: {} file(s), {} annotated item(s), {} clause(s), {} verified, {} error(s), {} body_not_modeled",
            file_items.len(),
            file_items
                .iter()
                .map(|(_, items)| items.len())
                .sum::<usize>(),
            total_clauses,
            total_verified,
            total_errors,
            total_body_not_modeled
        );
        if total_errors > 0 {
            eprintln!("{total_errors} verification error(s)");
            process::exit(1);
        } else if total_body_not_modeled > 0 {
            eprintln!(
                "{total_body_not_modeled} item(s) not proven against the Rust body \
                 (simplify body for encode, add co-located {{Name}}.ir, or use assura check + IR)"
            );
            process::exit(1);
        } else if total_verified == 0 {
            println!(
                "No clauses SMT-verified (annotations parsed; simplify body for encode or add co-located IR)"
            );
        } else {
            println!("All hard verification checks passed ({total_verified} verified)");
        }
    } else if total_errors > 0 || total_body_not_modeled > 0 {
        process::exit(1);
    }
}

/// Run LLM-assisted analysis on the scanned items.
fn run_llm_analysis(
    file_items: &[(std::path::PathBuf, Vec<assura_rust_analyzer::AnnotatedItem>)],
    verbosity: Verbosity,
    opts: &LlmOpts<'_>,
) {
    let analyze = opts.llm;
    let suggest = opts.suggest;
    let provider_name = opts.provider;
    let model_override = opts.model;
    let unsafe_only = opts.unsafe_only;
    let public_only = opts.public_only;
    use assura_llm::{
        ContractDatabase,
        cache::LlmCache,
        provider::{HttpProvider, LlmProvider},
        types::*,
    };

    // Build contract database for cross-function propagation
    let contract_db = ContractDatabase::from_scan(file_items);
    if verbosity == Verbosity::Verbose {
        println!(
            "  contract database: {} annotated functions indexed",
            contract_db.len()
        );
    }

    // Configure LLM provider
    let config = LlmConfig::from_provider(provider_name, model_override);

    let cache = LlmCache::new(&config.cache_dir);

    let provider = match HttpProvider::new(config) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("  LLM: {e}");
            eprintln!("  Set the API key environment variable to enable LLM analysis.");
            return;
        }
    };

    // Level 1: analyze annotated functions body vs contracts
    if analyze {
        if verbosity != Verbosity::Quiet {
            println!();
            println!("  AI analysis (model: {}):", provider.model_id());
        }

        for (_file_path, items) in file_items {
            for item in items {
                if let assura_rust_analyzer::AnnotatedItemKind::Function {
                    name,
                    params,
                    return_type,
                    ..
                } = &item.kind
                {
                    if item.contract.requires.is_empty() && item.contract.ensures.is_empty() {
                        continue; // nothing to analyze
                    }

                    let contracts: Vec<ContractClauseInfo> = item
                        .contract
                        .requires
                        .iter()
                        .map(|c| ContractClauseInfo {
                            kind: "requires".to_string(),
                            expression: c.body.clone(),
                        })
                        .chain(item.contract.ensures.iter().map(|c| ContractClauseInfo {
                            kind: "ensures".to_string(),
                            expression: c.body.clone(),
                        }))
                        .collect();

                    let called_fns: Vec<CalledFunctionContract> = contract_db
                        .all_contracts()
                        .into_iter()
                        .filter(|cf| cf.name != *name) // exclude self
                        .collect();

                    let req = AnalysisRequest {
                        function_name: name.clone(),
                        function_body: "(source body not available via scan)".to_string(),
                        function_signature: format!(
                            "fn {}({})",
                            name,
                            params
                                .iter()
                                .map(|p| format!("{}: {}", p.name, p.ty))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        contracts,
                        params: params
                            .iter()
                            .map(|p| ParamEntry {
                                name: p.name.clone(),
                                ty: p.ty.clone(),
                            })
                            .collect(),
                        return_type: return_type.clone(),
                        context: AnalysisContext {
                            surrounding_types: vec![],
                            called_functions: called_fns,
                        },
                    };

                    match assura_llm::suggest::analyze_function(&provider, &cache, &req) {
                        Ok(resp) => {
                            let verdict_str = match &resp.verdict {
                                Verdict::Pass => "pass",
                                Verdict::Fail { .. } => "FAIL",
                                Verdict::Uncertain { .. } => "uncertain",
                            };
                            if verbosity != Verbosity::Quiet {
                                println!(
                                    "    function `{}` (line {}): [{}, confidence: {:.0}%]",
                                    name,
                                    item.line,
                                    verdict_str,
                                    resp.confidence * 100.0,
                                );
                            }
                            if verbosity == Verbosity::Verbose {
                                for path in &resp.paths {
                                    let status = if path.contracts_satisfied {
                                        "ok"
                                    } else {
                                        "FAIL"
                                    };
                                    println!("      path: {} [{}]", path.description, status);
                                }
                                if !resp.reasoning.is_empty() {
                                    println!("      reasoning: {}", resp.reasoning);
                                }
                            }
                        }
                        Err(e) => {
                            if verbosity != Verbosity::Quiet {
                                eprintln!("    function `{name}`: LLM error: {e}");
                            }
                        }
                    }
                }
            }
        }
    }

    // Level 2: LLM-generated lemma chain verification
    if opts.llm_verify {
        if verbosity != Verbosity::Quiet {
            println!();
            println!(
                "  Level 2 lemma verification (model: {}):",
                provider.model_id()
            );
        }

        for (_file_path, items) in file_items {
            for item in items {
                if let assura_rust_analyzer::AnnotatedItemKind::Function { name, params, .. } =
                    &item.kind
                {
                    if item.contract.requires.is_empty() && item.contract.ensures.is_empty() {
                        continue;
                    }

                    let contracts: Vec<ContractClauseInfo> = item
                        .contract
                        .requires
                        .iter()
                        .map(|c| ContractClauseInfo {
                            kind: "requires".to_string(),
                            expression: c.body.clone(),
                        })
                        .chain(item.contract.ensures.iter().map(|c| ContractClauseInfo {
                            kind: "ensures".to_string(),
                            expression: c.body.clone(),
                        }))
                        .collect();

                    let sig = format!(
                        "fn {}({})",
                        name,
                        params
                            .iter()
                            .map(|p| format!("{}: {}", p.name, p.ty))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );

                    // Run Level 1 first if not already done, to get verdict + paths
                    let (verdict_str, paths) = if analyze {
                        // Re-use the Level 1 analysis result via cache (cheap)
                        let called_fns: Vec<CalledFunctionContract> = contract_db
                            .all_contracts()
                            .into_iter()
                            .filter(|cf| cf.name != *name)
                            .collect();
                        let req = AnalysisRequest {
                            function_name: name.clone(),
                            function_body: "(source body not available via scan)".to_string(),
                            function_signature: sig.clone(),
                            contracts: contracts.clone(),
                            params: params
                                .iter()
                                .map(|p| ParamEntry {
                                    name: p.name.clone(),
                                    ty: p.ty.clone(),
                                })
                                .collect(),
                            return_type: None,
                            context: AnalysisContext {
                                surrounding_types: vec![],
                                called_functions: called_fns,
                            },
                        };
                        match assura_llm::suggest::analyze_function(&provider, &cache, &req) {
                            Ok(resp) => {
                                let v = match &resp.verdict {
                                    Verdict::Pass => "pass".to_string(),
                                    Verdict::Fail { .. } => "fail".to_string(),
                                    Verdict::Uncertain { .. } => "uncertain".to_string(),
                                };
                                (v, resp.paths)
                            }
                            Err(_) => ("unknown".to_string(), vec![]),
                        }
                    } else {
                        ("unknown".to_string(), vec![])
                    };

                    match assura_llm::lemma::generate_and_verify_lemmas(
                        &provider,
                        &cache,
                        "(source body not available via scan)",
                        &sig,
                        &contracts,
                        &verdict_str,
                        &paths,
                    ) {
                        Ok((chain, verification)) => {
                            if verbosity != Verbosity::Quiet {
                                let status = if verification.chain_valid {
                                    "VALID"
                                } else {
                                    "INCOMPLETE"
                                };
                                println!(
                                    "    function `{}` (line {}): {} ({}/{} lemmas valid, ensures follows: {})",
                                    name,
                                    item.line,
                                    status,
                                    verification.valid_count,
                                    verification.total_count,
                                    verification.ensures_follows,
                                );
                            }
                            if verbosity == Verbosity::Verbose {
                                for lv in &verification.lemmas {
                                    let r = match &lv.result {
                                        assura_llm::types::LemmaResult::Valid => {
                                            "valid".to_string()
                                        }
                                        assura_llm::types::LemmaResult::Counterexample {
                                            ..
                                        } => "counterexample".to_string(),
                                        assura_llm::types::LemmaResult::Timeout => {
                                            "timeout".to_string()
                                        }
                                        assura_llm::types::LemmaResult::ParseError { message } => {
                                            format!("parse error: {message}")
                                        }
                                    };
                                    println!(
                                        "      lemma `{}`: {} ({}ms)",
                                        lv.label, r, lv.time_ms
                                    );
                                    if verbosity == Verbosity::Verbose {
                                        println!("        assertion: {}", lv.assertion);
                                    }
                                }
                                if chain.chain_complete {
                                    println!("      chain marked complete by LLM");
                                }
                            }
                        }
                        Err(e) => {
                            if verbosity != Verbosity::Quiet {
                                eprintln!("    function `{name}`: lemma error: {e}");
                            }
                        }
                    }
                }
            }
        }
    }

    // Suggestion mode for unannotated functions
    if suggest {
        if verbosity != Verbosity::Quiet {
            println!();
            println!(
                "  AI contract suggestions (model: {}):",
                provider.model_id()
            );
        }

        for (_file_path, items) in file_items {
            for item in items {
                if let assura_rust_analyzer::AnnotatedItemKind::Function {
                    name,
                    params,
                    return_type: _,
                    is_unsafe,
                    is_async,
                    is_public,
                } = &item.kind
                {
                    // Skip already-annotated functions
                    if !item.contract.requires.is_empty()
                        || !item.contract.ensures.is_empty()
                        || !item.contract.invariants.is_empty()
                    {
                        continue;
                    }

                    // Apply filters
                    if unsafe_only && !is_unsafe {
                        continue;
                    }
                    if public_only && !is_public {
                        continue;
                    }

                    let siblings = contract_db.all_contracts();

                    let req = SuggestionRequest {
                        function_name: name.clone(),
                        function_signature: format!(
                            "fn {}({})",
                            name,
                            params
                                .iter()
                                .map(|p| format!("{}: {}", p.name, p.ty))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        function_body: "(source body not available via scan)".to_string(),
                        doc_comments: String::new(),
                        impl_type: None,
                        visibility: String::new(),
                        is_unsafe: *is_unsafe,
                        is_async: *is_async,
                        context: SuggestionContext {
                            surrounding_types: vec![],
                            sibling_contracts: siblings,
                        },
                    };

                    match assura_llm::suggest::suggest_contracts(&provider, &cache, &req) {
                        Ok(resp) if !resp.suggestions.is_empty() => {
                            if verbosity != Verbosity::Quiet {
                                println!(
                                    "    function `{}` (line {}): {} suggestion(s)",
                                    name,
                                    item.line,
                                    resp.suggestions.len(),
                                );
                                for s in &resp.suggestions {
                                    println!(
                                        "      #[{}({})], confidence: {:.0}%",
                                        s.kind,
                                        s.expression,
                                        s.confidence * 100.0,
                                    );
                                    if verbosity == Verbosity::Verbose {
                                        println!("        {}", s.reasoning);
                                    }
                                }
                            }
                        }
                        Ok(_) => {} // no suggestions
                        Err(e) => {
                            if verbosity != Verbosity::Quiet {
                                eprintln!("    function `{name}`: LLM error: {e}");
                            }
                        }
                    }
                }
            }
        }
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

/// Inclusive bounds for fixed-width signed Rust integer types used as params.
fn rust_int_range_bounds(rust_ty: &str) -> Option<(&'static str, &'static str)> {
    // Strip path prefixes / syn spacing: `std :: num :: NonZeroU32` → `NonZeroU32`
    let base = rust_ty
        .rsplit("::")
        .next()
        .unwrap_or(rust_ty)
        .split_whitespace()
        .collect::<String>();
    match base.as_str() {
        "i8" => Some(("-128", "127")),
        "i16" => Some(("-32768", "32767")),
        "i32" => Some(("-2147483648", "2147483647")),
        "i64" | "isize" => Some(("-9223372036854775808", "9223372036854775807")),
        "u8" => Some(("0", "255")),
        "u16" => Some(("0", "65535")),
        "u32" => Some(("0", "4294967295")),
        "NonZeroU8" => Some(("1", "255")),
        "NonZeroU16" => Some(("1", "65535")),
        "NonZeroU32" => Some(("1", "4294967295")),
        // u64 max does not fit i64 IR const path; skip range inject for now
        _ => None,
    }
}

/// Whether check-rust should report `body_not_modeled` instead of a soft pass.
///
/// See #951: without co-located IR or an encoded Rust body, ensures must not
/// look like proof. That includes:
/// - SMT "verified"/"partial" from heuristic IR shapes (false confidence)
/// - SMT "skipped"/"checked" when ensures exist but the body was not modeled
///   (e.g. unconstrained `result` Unknown, nested if not encoded)
pub(crate) fn should_mark_body_not_modeled(
    has_ensures: bool,
    has_body_ir: bool,
    item_status: &str,
    item_verified: usize,
    item_errors: usize,
) -> bool {
    if !has_ensures || has_body_ir || item_errors > 0 {
        return false;
    }
    // False-verified path: synthesis claimed proof without a body model.
    if item_verified > 0 && matches!(item_status, "verified" | "partial") {
        return true;
    }
    // Soft-skip path: ensures present, body unmodeled, no CE — still not proven.
    matches!(item_status, "skipped" | "checked")
}

#[cfg(test)]
mod body_policy_tests {
    #[test]
    fn rust_int_range_i64() {
        assert_eq!(
            super::rust_int_range_bounds("i64"),
            Some(("-9223372036854775808", "9223372036854775807"))
        );
        assert!(super::rust_int_range_bounds("u64").is_none());
    }

    use super::should_mark_body_not_modeled;

    #[test]
    fn marks_synthesized_ensures_without_ir() {
        assert!(should_mark_body_not_modeled(true, false, "verified", 1, 0));
        assert!(should_mark_body_not_modeled(true, false, "partial", 1, 0));
    }

    #[test]
    fn marks_skipped_ensures_without_body_model() {
        assert!(should_mark_body_not_modeled(true, false, "skipped", 0, 0));
        assert!(should_mark_body_not_modeled(true, false, "checked", 0, 0));
    }

    #[test]
    fn keeps_verified_when_colocated_ir_present() {
        assert!(!should_mark_body_not_modeled(true, true, "verified", 1, 0));
        assert!(!should_mark_body_not_modeled(true, true, "skipped", 0, 0));
    }

    #[test]
    fn keeps_requires_only_or_errors() {
        assert!(!should_mark_body_not_modeled(
            false, false, "verified", 1, 0
        ));
        assert!(!should_mark_body_not_modeled(true, false, "error", 0, 1));
        assert!(!should_mark_body_not_modeled(false, false, "skipped", 0, 0));
    }
}
