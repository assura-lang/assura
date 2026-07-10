//! `assura suggest-from-crash` — propose contracts from cargo-fuzz crash artifacts.

use super::super::*;

/// Options for `assura suggest-from-crash`.
pub(crate) struct SuggestFromCrashOpts<'a> {
    pub crash_input: Option<&'a str>,
    pub crash_dir: Option<&'a str>,
    pub target: &'a str,
    pub stacktrace_file: Option<&'a str>,
    pub llm_provider: &'a str,
    pub llm_model: Option<&'a str>,
    pub output_mode: OutputMode,
    pub verbosity: Verbosity,
}

pub(crate) fn run_suggest_from_crash(opts: SuggestFromCrashOpts<'_>) {
    use assura_llm::fuzz::*;
    use assura_llm::{cache::LlmCache, types::LlmConfig};

    let SuggestFromCrashOpts {
        crash_input,
        crash_dir,
        target,
        stacktrace_file,
        llm_provider,
        llm_model,
        output_mode,
        verbosity,
    } = opts;

    // Load crash artifacts
    let artifacts: Vec<CrashArtifact> = if let Some(input) = crash_input {
        match CrashArtifact::from_file(Path::new(input)) {
            Ok(a) => vec![a],
            Err(e) => {
                eprintln!("Error reading crash artifact {input}: {e}");
                process::exit(1);
            }
        }
    } else if let Some(dir) = crash_dir {
        match CrashArtifact::from_directory(Path::new(dir)) {
            Ok(a) if a.is_empty() => {
                if output_mode == OutputMode::Json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "artifacts": 0,
                            "function": null,
                            "total_suggestions": 0,
                            "crashes": [],
                            "message": format!("No crash artifacts found in {dir}"),
                        })
                    );
                } else if verbosity != Verbosity::Quiet {
                    println!("No crash artifacts found in {dir}");
                }
                return;
            }
            Ok(a) => a,
            Err(e) => {
                eprintln!("Error reading crash directory {dir}: {e}");
                process::exit(1);
            }
        }
    } else {
        eprintln!("Error: specify --crash-input or --crash-dir");
        process::exit(1);
    };

    // Parse stack trace (if provided)
    let stack_trace = stacktrace_file.and_then(|f| {
        std::fs::read_to_string(f)
            .map(|text| StackTrace::parse(&text))
            .ok()
    });

    // Scan target Rust source for annotated items
    let target_path = Path::new(target);
    let file_items = if target_path.is_dir() {
        assura_rust_analyzer::scan_directory(target_path).unwrap_or_default()
    } else if target_path.is_file() {
        assura_rust_analyzer::parse_rust_file(target_path)
            .map(|items| vec![(target_path.to_path_buf(), items)])
            .unwrap_or_default()
    } else {
        eprintln!("Error: {target} is not a file or directory");
        process::exit(1);
    };

    // Build contract database to find existing contracts
    let contract_db = assura_llm::ContractDatabase::from_scan(&file_items);

    // Configure LLM provider
    let config = LlmConfig::from_provider(llm_provider, llm_model);

    let cache = LlmCache::new(&config.cache_dir);

    let provider = match assura_llm::HttpProvider::new(config) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("LLM provider error: {e}");
            eprintln!("Set the API key environment variable to enable LLM analysis.");
            process::exit(1);
        }
    };

    if verbosity != Verbosity::Quiet && output_mode != OutputMode::Json {
        println!(
            "suggest-from-crash: {} artifact(s), target: {target}",
            artifacts.len()
        );
    }

    // Determine the target function name from stack trace (if available)
    let crash_fn_name = stack_trace
        .as_ref()
        .and_then(|t| t.crash_function())
        .map(|f| {
            // Extract the function name from the fully qualified path
            f.function_name
                .rsplit("::")
                .next()
                .unwrap_or(&f.function_name)
                .to_string()
        });

    // Read the target file source for LLM context
    let target_source = if target_path.is_file() {
        std::fs::read_to_string(target_path).unwrap_or_default()
    } else {
        // If it's a directory, use stack trace to find the specific file
        stack_trace
            .as_ref()
            .and_then(|t| t.crash_function())
            .and_then(|f| f.file.as_ref())
            .and_then(|file| std::fs::read_to_string(file).ok())
            .unwrap_or_else(|| "(source not available)".to_string())
    };

    let fn_name = crash_fn_name.as_deref().unwrap_or("(unknown function)");

    // Get existing contracts for the function
    let existing: Vec<String> = contract_db
        .lookup_function(fn_name)
        .map(|c| {
            c.requires
                .iter()
                .map(|r| format!("#[requires({r})]"))
                .chain(c.ensures.iter().map(|e| format!("#[ensures({e})]")))
                .collect()
        })
        .unwrap_or_default();

    let mut all_results: Vec<serde_json::Value> = Vec::new();
    let mut total_suggestions = 0usize;
    let mut llm_errors: Vec<serde_json::Value> = Vec::new();

    // Track seen crashes for deduplication
    let mut seen_keys = std::collections::HashSet::new();

    for artifact in &artifacts {
        let dedup_key = format!(
            "{}:{}",
            fn_name,
            stack_trace
                .as_ref()
                .and_then(|t| t.panic_message.as_deref())
                .unwrap_or("unknown")
        );

        if !seen_keys.insert(dedup_key) {
            if verbosity == Verbosity::Verbose && output_mode != OutputMode::Json {
                println!(
                    "  skipping {} (duplicate crash class)",
                    artifact.path.display()
                );
            }
            continue;
        }

        if verbosity == Verbosity::Verbose && output_mode != OutputMode::Json {
            println!(
                "  analyzing {} ({}, {})",
                artifact.path.display(),
                artifact.crash_kind,
                artifact.input_summary,
            );
        }

        match suggest_from_crash(
            &provider,
            &cache,
            &target_source,
            fn_name,
            artifact,
            stack_trace.as_ref(),
            &existing,
        ) {
            Ok(resp) if !resp.suggestions.is_empty() => {
                total_suggestions += resp.suggestions.len();

                if output_mode == OutputMode::Json {
                    all_results.push(serde_json::json!({
                        "artifact": artifact.path.display().to_string(),
                        "crash_kind": artifact.crash_kind.to_string(),
                        "function": fn_name,
                        "suggestions": resp.suggestions,
                    }));
                } else if verbosity != Verbosity::Quiet {
                    println!("\n  {} ({}):", artifact.path.display(), artifact.crash_kind,);
                    for s in &resp.suggestions {
                        println!(
                            "    #[{}({})]  confidence: {:.0}%",
                            s.kind,
                            s.expression,
                            s.confidence * 100.0,
                        );
                        println!("      prevents: {}", s.prevents);
                        if verbosity == Verbosity::Verbose {
                            println!("      reasoning: {}", s.reasoning);
                        }
                    }
                }
            }
            Ok(_) => {
                if verbosity == Verbosity::Verbose && output_mode != OutputMode::Json {
                    println!("  {} no suggestions", artifact.path.display());
                }
            }
            Err(e) => {
                let msg = e.to_string();
                llm_errors.push(serde_json::json!({
                    "artifact": artifact.path.display().to_string(),
                    "error": msg,
                }));
                // Human diagnostics on stderr; JSON carries errors in the body.
                if output_mode != OutputMode::Json {
                    eprintln!("  {}: LLM error: {e}", artifact.path.display(),);
                }
            }
        }
    }

    // Output
    if output_mode == OutputMode::Json {
        let summary = serde_json::json!({
            "artifacts": artifacts.len(),
            "function": fn_name,
            "total_suggestions": total_suggestions,
            "crashes": all_results,
            "errors": llm_errors,
            "success": llm_errors.is_empty(),
        });
        println!("{}", serde_json::to_string_pretty(&summary).unwrap());
        if !llm_errors.is_empty() {
            process::exit(1);
        }
    } else if verbosity != Verbosity::Quiet {
        println!(
            "\nsuggest-from-crash: {} artifact(s) analyzed, {} suggestion(s)",
            artifacts.len(),
            total_suggestions,
        );
        if !llm_errors.is_empty() {
            eprintln!("{} LLM error(s)", llm_errors.len());
            process::exit(1);
        }
    } else if !llm_errors.is_empty() {
        process::exit(1);
    }
}
