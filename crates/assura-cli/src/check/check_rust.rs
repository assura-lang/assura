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
                    .map(|p| {
                        let assura_ty = assura_codegen::type_map::rust_type_to_assura(&p.ty);
                        format!("{}: {assura_ty}", p.name)
                    })
                    .collect();
                if !param_strs.is_empty() {
                    contract_source.push_str(&format!("  requires({})\n", param_strs.join(", ")));
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
                    verify_options: assura_config::VerifyOptions {
                        layer,
                        solver: solver_choice,
                        ..Default::default()
                    },
                    show_cores: false,
                    strict: false,
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
                if let assura_rust_analyzer::AnnotatedItemKind::Function {
                    name,
                    params,
                    return_type: _,
                    ..
                } = &item.kind
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
