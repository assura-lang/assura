//! `assura check-rust` — inline contract annotations on Rust sources.

use super::super::*;

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
                    verify_options: assura_config::VerifyOptions {
                        layer,
                        solver: solver_choice,
                        ..Default::default()
                    },
                    show_cores: false,
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
