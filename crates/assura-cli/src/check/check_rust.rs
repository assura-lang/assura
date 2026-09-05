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
                "success": false,
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

    // Annotated items only (crates.io rust-analyzer APIs). `--suggest` then
    // walks with syn locally so unannotated fns are candidates without
    // calling unpublished ScanOptions / *_with_options helpers.
    let mut file_items: Vec<(std::path::PathBuf, Vec<AnnotatedItem>)> = if p.is_dir() {
        match assura_rust_analyzer::scan_directory(p) {
            Ok(results) => results,
            Err(e) => {
                if json {
                    let report = serde_json::json!({
                        "ok": false,
                        "success": false,
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
            Ok(_) => Vec::new(),
            Err(e) => {
                if json {
                    let report = serde_json::json!({
                        "ok": false,
                        "success": false,
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
                "success": false,
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

    if llm_opts.suggest {
        add_unannotated_fn_stubs(p, &mut file_items);
    }

    if file_items.is_empty() {
        if llm_opts.suggest {
            report_nothing_to_suggest(path, json);
            process::exit(1);
        }
        if json {
            let report = serde_json::json!({
                "ok": true,
                "success": true,
                "path": path,
                "items": 0,
                "message": format!("No inline contract annotations found in {path}"),
                "vacuous": true,
                "vacuous_reason": "no inline contract annotations",
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else if verbosity != Verbosity::Quiet {
            println!("No inline contract annotations found in {path}");
        }
        return;
    }

    let suggest_candidates = if llm_opts.suggest {
        let candidates = collect_suggest_candidates(&file_items, &llm_opts);
        if candidates.is_empty() {
            report_nothing_to_suggest(path, json);
            process::exit(1);
        }
        if !json && verbosity != Verbosity::Quiet {
            println!("  AI contract suggest candidates:");
            for c in &candidates {
                println!("    function `{}` (line {})", c.name, c.line);
            }
        }
        candidates
    } else {
        Vec::new()
    };

    let solver_choice = solver.unwrap_or(assura_smt::SolverChoice::Z3);
    let mut total_clauses = 0usize;
    let mut total_verified = 0usize;
    let mut total_errors = 0usize;
    let mut total_body_not_modeled = 0usize;
    let mut last_bnm_reason: Option<String> = None;
    let mut all_results: Vec<serde_json::Value> = Vec::new();

    for (file_path, items) in &file_items {
        let file_display = file_path.display();
        if verbosity == Verbosity::Verbose {
            println!("Checking {file_display} ({} annotated items)", items.len());
        }

        for item in items {
            // Unannotated functions are suggest candidates only.
            if item.contract.is_empty() {
                continue;
            }
            let (item_name, item_kind_str) = match &item.kind {
                AnnotatedItemKind::Function { name, .. } => (name.clone(), "function"),
                AnnotatedItemKind::Struct { name, .. } => (name.clone(), "struct"),
                AnnotatedItemKind::ImplBlock { self_type, .. } => (self_type.clone(), "impl block"),
            };

            let (contract_source, n_clauses) = synthesize_inline_contract(&item_name, item);
            total_clauses += n_clauses;

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
                    && let Some(body) = super::rust_body_ir::extract_body_return_at(
                        &rust_src, &item_name, item.line,
                    )
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
                    let fold_reason = super::rust_body_ir::take_fold_residual();
                    if verbosity == Verbosity::Verbose && output_mode == OutputMode::Human {
                        if let Some(ref reason) = fold_reason {
                            eprintln!("  note: `{item_name}` body_not_modeled: {reason}");
                        } else {
                            eprintln!(
                                "  note: `{item_name}` has no co-located .ir and body is outside the encode surface; \
                                 ensures were not proven against the Rust body (status body_not_modeled)"
                            );
                        }
                    }
                    item_skipped += item_verified;
                    item_verified = 0;
                    item_status = "body_not_modeled";
                    total_body_not_modeled += 1;
                    // Stash last reason for the summary rewrite hints (if any).
                    if let Some(reason) = fold_reason {
                        last_bnm_reason = Some(format!("{item_name}: {reason}"));
                    }
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
                    .chain(modifies_clauses_json(item))
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
        let ok = total_errors == 0 && total_body_not_modeled == 0;
        let mut summary = serde_json::json!({
            "ok": ok,
            "success": ok,
            "vacuous": false,
            "files": file_items.len(),
            "items": file_items.iter().map(|(_, items)| items.len()).sum::<usize>(),
            "clauses": total_clauses,
            "verified": total_verified,
            "errors": total_errors,
            "body_not_modeled": total_body_not_modeled,
            "results": all_results,
            "policy": "check-rust proves annotations against co-located .ir or encoded Rust bodies (arith/if/match/wrapping/bitops/checked_*/overflowing_*/rotate/is_power_of_two/ilog/isqrt/next_power_of_two, abs/min/max/clamp/signum/saturating, PartialOrd; see CONTRIBUTING check-rust body proof)",
        });
        if llm_opts.suggest {
            summary["suggest_candidates"] = serde_json::Value::Array(
                suggest_candidates
                    .iter()
                    .map(|c| {
                        serde_json::json!({
                            "name": c.name,
                            "line": c.line,
                            "file": c.file,
                        })
                    })
                    .collect(),
            );
        }
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
            if let Some(ref reason) = last_bnm_reason {
                eprintln!(
                    "{total_body_not_modeled} item(s) not proven against the Rust body \
                     (body_not_modeled). Last reason: {reason}. Loops stay residual; \
                     if/match mutation joins are encoded. Peel checked_*/overflowing_* \
                     with .unwrap_or / .is_some() / .0; avoid panic div/mod. Or add \
                     co-located {{Name}}.ir. Surface map: docs/CHECK-RUST-SURFACE.md"
                );
            } else {
                eprintln!(
                    "{total_body_not_modeled} item(s) not proven against the Rust body \
                     (body_not_modeled). Rewrite hints: peel checked_*/overflowing_* \
                     with .unwrap_or / .is_some() / .0; avoid panic div/mod; loops with \
                     mutation stay residual. Or add co-located {{Name}}.ir. Surface map: \
                     docs/CHECK-RUST-SURFACE.md"
                );
            }
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

struct SuggestCandidate {
    name: String,
    line: usize,
    file: String,
}

fn collect_suggest_candidates(
    file_items: &[(std::path::PathBuf, Vec<assura_rust_analyzer::AnnotatedItem>)],
    opts: &LlmOpts<'_>,
) -> Vec<SuggestCandidate> {
    let mut candidates = Vec::new();
    for (file_path, items) in file_items {
        for item in items {
            let assura_rust_analyzer::AnnotatedItemKind::Function {
                name,
                is_unsafe,
                is_public,
                ..
            } = &item.kind
            else {
                continue;
            };
            if !item.contract.is_empty() {
                continue;
            }
            if opts.unsafe_only && !is_unsafe {
                continue;
            }
            if opts.public_only && !is_public {
                continue;
            }
            candidates.push(SuggestCandidate {
                name: name.clone(),
                line: item.line,
                file: file_path.display().to_string(),
            });
        }
    }
    candidates
}

/// CLI-local syn walk: add empty-contract stubs for functions not already
/// present from the published annotated-only scan.
fn add_unannotated_fn_stubs(
    root: &Path,
    file_items: &mut Vec<(std::path::PathBuf, Vec<assura_rust_analyzer::AnnotatedItem>)>,
) {
    if root.is_file() {
        add_stubs_for_rust_file(root, file_items);
        return;
    }
    if root.is_dir() {
        let mut rs_files = Vec::new();
        collect_rs_files(root, &mut rs_files);
        for path in rs_files {
            add_stubs_for_rust_file(&path, file_items);
        }
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            continue;
        }
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" || name == "generated" {
                continue;
            }
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn add_stubs_for_rust_file(
    path: &Path,
    file_items: &mut Vec<(std::path::PathBuf, Vec<assura_rust_analyzer::AnnotatedItem>)>,
) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let file = match syn::parse_file(&source) {
        Ok(f) => f,
        Err(_) => return,
    };
    let existing_idx = file_items.iter().position(|(p, _)| p == path);
    let existing_keys: std::collections::HashSet<(String, usize)> = existing_idx
        .map(|i| {
            file_items[i]
                .1
                .iter()
                .filter_map(|item| match &item.kind {
                    assura_rust_analyzer::AnnotatedItemKind::Function { name, .. } => {
                        Some((name.clone(), item.line))
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();

    let mut stubs = Vec::new();
    collect_fn_stubs(&file.items, &existing_keys, &mut stubs);
    if stubs.is_empty() {
        return;
    }
    if let Some(i) = existing_idx {
        file_items[i].1.extend(stubs);
    } else {
        file_items.push((path.to_path_buf(), stubs));
    }
}

/// Annotated `item.line` can be the doc/`@ensures` line or one off the
/// `fn` keyword (span 0-based vs 1-based). Treat nearby lines as the same item.
fn stub_already_annotated(
    existing_keys: &std::collections::HashSet<(String, usize)>,
    name: &str,
    fn_line: usize,
) -> bool {
    existing_keys
        .iter()
        .any(|(n, line)| n == name && line.abs_diff(fn_line) <= 1)
}

fn collect_fn_stubs(
    items: &[syn::Item],
    existing_keys: &std::collections::HashSet<(String, usize)>,
    stubs: &mut Vec<assura_rust_analyzer::AnnotatedItem>,
) {
    for item in items {
        match item {
            syn::Item::Fn(func) => {
                let name = func.sig.ident.to_string();
                let line = func.sig.fn_token.span.start().line;
                if stub_already_annotated(existing_keys, &name, line) {
                    continue;
                }
                stubs.push(fn_stub(&func.sig, &func.vis));
            }
            syn::Item::Impl(imp) => {
                for impl_item in &imp.items {
                    if let syn::ImplItem::Fn(method) = impl_item {
                        let name = method.sig.ident.to_string();
                        let line = method.sig.fn_token.span.start().line;
                        if stub_already_annotated(existing_keys, &name, line) {
                            continue;
                        }
                        stubs.push(fn_stub(&method.sig, &method.vis));
                    }
                }
            }
            syn::Item::Mod(module) => {
                if let Some((_, inner)) = &module.content {
                    collect_fn_stubs(inner, existing_keys, stubs);
                }
            }
            _ => {}
        }
    }
}

fn fn_stub(sig: &syn::Signature, vis: &syn::Visibility) -> assura_rust_analyzer::AnnotatedItem {
    let line = sig.fn_token.span.start().line;
    assura_rust_analyzer::AnnotatedItem {
        contract: assura_rust_analyzer::InlineContract::default(),
        kind: assura_rust_analyzer::AnnotatedItemKind::Function {
            name: sig.ident.to_string(),
            params: Vec::new(),
            return_type: None,
            is_unsafe: matches!(sig.safety, syn::Safety::Unsafe(_)),
            is_async: sig.asyncness.is_some(),
            is_public: matches!(vis, syn::Visibility::Public(_)),
        },
        offset: 0,
        line,
    }
}

fn report_nothing_to_suggest(path: &str, json: bool) {
    let message = format!("nothing to suggest: no unannotated functions in {path}");
    if json {
        let report = serde_json::json!({
            "ok": false,
            "success": false,
            "path": path,
            "error": "nothing_to_suggest",
            "message": message,
        });
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else {
        eprintln!("Error: {message}");
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
    if !crate::suggest::LLM_PROVIDERS
        .iter()
        .any(|p| *p == provider_name.to_ascii_lowercase())
    {
        eprintln!("  LLM: unknown provider '{provider_name}'");
        if let Some(hint) =
            crate::suggest::did_you_mean(provider_name, crate::suggest::LLM_PROVIDERS)
        {
            eprintln!("  did you mean {hint}?");
        }
        return;
    }
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
                    if !item.contract.is_empty() {
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

/// Strip trailing `//` comments from doc annotation bodies so
/// `/// @ensures result == x // identity` stays valid Assura.
fn clause_body(raw: &str) -> String {
    raw.split("//").next().unwrap_or(raw).trim().to_string()
}

/// Body to emit for `@modifies`. `None` when empty, comment-only, or
/// only `{}` / whitespace / commas (those become A14001).
fn modifies_clause_body(raw: &str) -> Option<String> {
    let body = clause_body(raw);
    let has_name = body
        .chars()
        .any(|c| !c.is_whitespace() && c != '{' && c != '}' && c != ',');
    has_name.then_some(body)
}

/// JSON clauses for `@modifies` annotations that survive empty-body skip.
fn modifies_clauses_json(item: &assura_rust_analyzer::AnnotatedItem) -> Vec<serde_json::Value> {
    item.contract
        .annotations_of(assura_rust_analyzer::InlineClauseKind::Modifies)
        .into_iter()
        .filter(|c| modifies_clause_body(&c.body).is_some())
        .map(|c| clause_to_json(c, "modifies"))
        .collect()
}

/// Build a synthetic Assura contract from inline Rust annotations.
/// Returns `(source, clause_count)`.
fn synthesize_inline_contract(
    item_name: &str,
    item: &assura_rust_analyzer::AnnotatedItem,
) -> (String, usize) {
    use assura_rust_analyzer::{AnnotatedItemKind, InlineClauseKind};

    let mut contract_source = format!("contract {item_name} {{\n");
    let mut total_clauses = 0usize;

    for clause in &item.contract.requires {
        contract_source.push_str(&format!("  requires {{ {} }}\n", clause_body(&clause.body)));
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
    for clause in &item.contract.ensures {
        contract_source.push_str(&format!("  ensures {{ {} }}\n", clause_body(&clause.body)));
        total_clauses += 1;
    }
    for clause in &item.contract.invariants {
        contract_source.push_str(&format!(
            "  invariant {{ {} }}\n",
            clause_body(&clause.body)
        ));
        total_clauses += 1;
    }
    for clause in &item.contract.effects {
        contract_source.push_str(&format!("  effects {{ {} }}\n", clause_body(&clause.body)));
        total_clauses += 1;
    }
    for clause in &item.contract.decreases {
        contract_source.push_str(&format!(
            "  decreases {{ {} }}\n",
            clause_body(&clause.body)
        ));
        total_clauses += 1;
    }
    for clause in item.contract.annotations_of(InlineClauseKind::Modifies) {
        let Some(body) = modifies_clause_body(&clause.body) else {
            continue;
        };
        contract_source.push_str(&format!("  modifies {{ {body} }}\n"));
        total_clauses += 1;
    }

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
    (contract_source, total_clauses)
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

#[cfg(test)]
mod modifies_emit_tests {
    use assura_rust_analyzer::{AnnotatedItemKind, InlineClauseKind, parse_rust_source};

    #[test]
    fn synthesizes_modifies_clause() {
        let src = "\
/// @requires x > 0
/// @ensures result >= x
/// @modifies xs
fn bump(x: i32, xs: i32) -> i32 { x }
";
        let items = parse_rust_source(src).expect("parse rust snippet");
        assert!(!items.is_empty(), "expected an annotated function");
        let item = &items[0];
        assert!(
            !item
                .contract
                .annotations_of(InlineClauseKind::Modifies)
                .is_empty(),
            "parser must store @modifies on InlineContract.annotations"
        );
        let name = match &item.kind {
            AnnotatedItemKind::Function { name, .. } => name.as_str(),
            other => panic!("expected function, got {other:?}"),
        };
        let (source, count) = super::synthesize_inline_contract(name, item);
        assert!(
            source.contains("modifies { xs }"),
            "synthesized contract must emit @modifies, got:\n{source}"
        );
        assert!(count >= 3, "requires + ensures + modifies, got {count}");
    }

    fn synth_from_rust(src: &str) -> String {
        let items = parse_rust_source(src).expect("parse rust snippet");
        assert!(!items.is_empty(), "expected an annotated function");
        let item = &items[0];
        let name = match &item.kind {
            AnnotatedItemKind::Function { name, .. } => name.as_str(),
            other => panic!("expected function, got {other:?}"),
        };
        super::synthesize_inline_contract(name, item).0
    }

    #[test]
    fn skips_empty_and_comment_only_modifies() {
        let empty = synth_from_rust(
            "\
/// @requires x > 0
/// @ensures result >= x
/// @modifies
fn bump(x: i32, xs: i32) -> i32 { x }
",
        );
        assert!(
            !empty.contains("modifies {"),
            "empty @modifies must not emit, got:\n{empty}"
        );

        let comment_only = synth_from_rust(
            "\
/// @requires x > 0
/// @ensures result >= x
/// @modifies // note
fn bump(x: i32, xs: i32) -> i32 { x }
",
        );
        assert!(
            !comment_only.contains("modifies {"),
            "comment-only @modifies must not emit, got:\n{comment_only}"
        );
    }

    #[test]
    fn skips_empty_braces_modifies_without_a14001() {
        let source = synth_from_rust(
            "\
/// @requires x > 0
/// @ensures result >= x
/// @modifies {}
fn bump(x: i32, xs: i32) -> i32 { x }
",
        );
        assert!(
            !source.contains("modifies {"),
            "empty @modifies {{}} must not emit, got:\n{source}"
        );
        let file = assura_parser::parse_unwrap(&source);
        let resolved = assura_resolve::resolve(&file).expect("resolve synth");
        match assura_types::type_check(resolved) {
            Ok(typed) => {
                assert!(
                    typed.warnings.iter().all(|w| w.code != "A14001"),
                    "A14001 in warnings: {:?}",
                    typed.warnings
                );
            }
            Err(errors) => {
                assert!(
                    errors.iter().all(|e| e.code != "A14001"),
                    "empty @modifies {{}} must not A14001-fail, got: {errors:?}"
                );
            }
        }
    }

    #[test]
    fn modifies_clause_to_json_kind_and_body() {
        let src = "\
/// @requires x > 0
/// @ensures result >= x
/// @modifies xs
fn bump(x: i32, xs: i32) -> i32 { x }
";
        let items = parse_rust_source(src).expect("parse rust snippet");
        assert!(!items.is_empty(), "expected an annotated function");
        let jsons = super::modifies_clauses_json(&items[0]);
        assert_eq!(
            jsons.len(),
            1,
            "expected one modifies JSON clause, got: {jsons:?}"
        );
        assert_eq!(jsons[0]["kind"], "modifies");
        let body = jsons[0]["body"].as_str().unwrap_or("");
        assert!(
            body.contains("xs"),
            "JSON modifies body must contain xs, got: {body:?}"
        );
    }

    #[test]
    fn empty_modifies_produces_no_json_clause() {
        let src = "\
/// @modifies
/// @modifies // note
/// @modifies {}
fn bump(xs: i32) -> i32 { xs }
";
        let items = parse_rust_source(src).expect("parse rust snippet");
        assert!(!items.is_empty(), "expected an annotated function");
        let jsons = super::modifies_clauses_json(&items[0]);
        assert!(
            jsons.is_empty(),
            "empty @modifies must produce no JSON clause, got: {jsons:?}"
        );
    }
}
