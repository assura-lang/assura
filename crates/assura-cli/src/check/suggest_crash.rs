//! `assura suggest-from-crash` — propose contracts from cargo-fuzz crash artifacts.

use super::super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_suggest_from_crash(
    crash_input: Option<&str>,
    crash_dir: Option<&str>,
    target: &str,
    stacktrace_file: Option<&str>,
    llm_provider: &str,
    llm_model: Option<&str>,
    output_mode: OutputMode,
    verbosity: Verbosity,
) {
    use assura_llm::fuzz::*;
    use assura_llm::{cache::LlmCache, types::LlmConfig};

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
                if verbosity != Verbosity::Quiet {
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
    let config = LlmConfig {
        provider: llm_provider.to_string(),
        model: llm_model
            .map(|s| s.to_string())
            .unwrap_or_else(|| match llm_provider {
                "openai" => "gpt-4o".to_string(),
                "ollama" => "llama3".to_string(),
                _ => "claude-sonnet-4-20250514".to_string(),
            }),
        api_key_env: match llm_provider {
            "openai" => "OPENAI_API_KEY".to_string(),
            "ollama" => "OLLAMA_API_KEY".to_string(),
            _ => "ANTHROPIC_API_KEY".to_string(),
        },
        base_url: if llm_provider == "ollama" {
            Some("http://localhost:11434/v1".to_string())
        } else {
            None
        },
        ..Default::default()
    };

    let cache = LlmCache::new(&config.cache_dir);

    let provider = match assura_llm::HttpProvider::new(config) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("LLM provider error: {e}");
            eprintln!("Set the API key environment variable to enable LLM analysis.");
            process::exit(1);
        }
    };

    if verbosity != Verbosity::Quiet {
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
            if verbosity == Verbosity::Verbose {
                println!(
                    "  skipping {} (duplicate crash class)",
                    artifact.path.display()
                );
            }
            continue;
        }

        if verbosity == Verbosity::Verbose {
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
                if verbosity == Verbosity::Verbose {
                    println!("  {} no suggestions", artifact.path.display());
                }
            }
            Err(e) => {
                eprintln!("  {}: LLM error: {e}", artifact.path.display(),);
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
        });
        println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    } else if verbosity != Verbosity::Quiet {
        println!(
            "\nsuggest-from-crash: {} artifact(s) analyzed, {} suggestion(s)",
            artifacts.len(),
            total_suggestions,
        );
    }
}
