use super::*;

// `assura diff` -- structural diff between contract files
// ---------------------------------------------------------------------------

pub(crate) fn extract_decl_summary(
    sf: &SourceFile,
) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut result = std::collections::BTreeMap::new();
    for spanned_decl in &sf.decls {
        let decl = &spanned_decl.node;
        let name = match decl {
            Decl::Contract(c) => c.name.clone(),
            Decl::Bind(b) => b.name.clone(),
            Decl::FnDef(f) => f.name.clone(),
            Decl::Service(s) => s.name.clone(),
            Decl::TypeDef(t) => t.name.clone(),
            Decl::EnumDef(e) => e.name.clone(),
            Decl::Extern(e) => e.name.clone(),
            Decl::Prophecy(p) => p.name.clone(),
            Decl::CodecRegistry(c) => c.name.clone(),
            Decl::Block { name, .. } => name.clone(),
        };
        let clauses: Vec<String> = match decl {
            Decl::Contract(c) => c
                .clauses
                .iter()
                .map(|cl| format!("{:?}: {}", cl.kind, format_clause_body(cl)))
                .collect(),
            Decl::Bind(b) => b
                .clauses
                .iter()
                .map(|cl| format!("{:?}: {}", cl.kind, format_clause_body(cl)))
                .collect(),
            _ => Vec::new(),
        };
        result.insert(name, clauses);
    }
    result
}

pub(crate) fn format_clause_body(clause: &assura_parser::ast::Clause) -> String {
    format!("{:?}", clause.body)
}

pub(crate) fn run_diff(old_path: &str, new_path: &str, format: &str) -> bool {
    let old_src = match fs::read_to_string(old_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {old_path}: {e}");
            process::exit(1);
        }
    };
    let new_src = match fs::read_to_string(new_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {new_path}: {e}");
            process::exit(1);
        }
    };

    let (old_ast, old_errs) = assura_parser::parse(&old_src);
    let (new_ast, new_errs) = assura_parser::parse(&new_src);

    if !old_errs.is_empty() {
        eprintln!("Warning: {old_path} has {} parse error(s)", old_errs.len());
    }
    if !new_errs.is_empty() {
        eprintln!("Warning: {new_path} has {} parse error(s)", new_errs.len());
    }

    let old_decls = old_ast
        .as_ref()
        .map(extract_decl_summary)
        .unwrap_or_default();
    let new_decls = new_ast
        .as_ref()
        .map(extract_decl_summary)
        .unwrap_or_default();

    let mut changes = Vec::new();
    let mut has_diff = false;

    for (name, old_clauses) in &old_decls {
        if !new_decls.contains_key(name) {
            has_diff = true;
            changes.push(DiffEntry {
                name: name.clone(),
                kind: "removed".to_string(),
                added_clauses: Vec::new(),
                removed_clauses: old_clauses.clone(),
                unchanged_clauses: Vec::new(),
            });
        }
    }

    for (name, new_clauses) in &new_decls {
        match old_decls.get(name) {
            None => {
                has_diff = true;
                changes.push(DiffEntry {
                    name: name.clone(),
                    kind: "added".to_string(),
                    added_clauses: new_clauses.clone(),
                    removed_clauses: Vec::new(),
                    unchanged_clauses: Vec::new(),
                });
            }
            Some(old_clauses) => {
                let added: Vec<String> = new_clauses
                    .iter()
                    .filter(|c| !old_clauses.contains(c))
                    .cloned()
                    .collect();
                let removed: Vec<String> = old_clauses
                    .iter()
                    .filter(|c| !new_clauses.contains(c))
                    .cloned()
                    .collect();
                let unchanged: Vec<String> = new_clauses
                    .iter()
                    .filter(|c| old_clauses.contains(c))
                    .cloned()
                    .collect();
                if !added.is_empty() || !removed.is_empty() {
                    has_diff = true;
                    changes.push(DiffEntry {
                        name: name.clone(),
                        kind: "modified".to_string(),
                        added_clauses: added,
                        removed_clauses: removed,
                        unchanged_clauses: unchanged,
                    });
                }
            }
        }
    }

    if format == "json" {
        let json = serde_json::json!({
            "identical": !has_diff,
            "changes": changes.iter().map(|c| serde_json::json!({
                "name": c.name,
                "kind": c.kind,
                "added_clauses": c.added_clauses,
                "removed_clauses": c.removed_clauses,
                "unchanged_clauses": c.unchanged_clauses,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&json).unwrap());
    } else {
        if !has_diff {
            println!("No structural differences.");
        }
        for entry in &changes {
            match entry.kind.as_str() {
                "added" => println!("{}:  (new)", entry.name),
                "removed" => println!("{}:  (removed)", entry.name),
                _ => println!("{}:", entry.name),
            }
            for c in &entry.removed_clauses {
                println!("  - {c}");
            }
            for c in &entry.added_clauses {
                println!("  + {c}");
            }
            for c in &entry.unchanged_clauses {
                println!("    {c}");
            }
            println!();
        }
    }

    has_diff
}

/// Run SMT-based evolution verification on two contract files.
///
/// Parses both files and checks backward compatibility:
/// - Precondition weakening: old_requires => new_requires
/// - Postcondition strengthening: new_ensures => old_ensures
pub(crate) fn run_diff_verify(old_path: &str, new_path: &str, format: &str) {
    let old_src = match fs::read_to_string(old_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {old_path}: {e}");
            process::exit(1);
        }
    };
    let new_src = match fs::read_to_string(new_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {new_path}: {e}");
            process::exit(1);
        }
    };

    let (old_ast, old_errs) = assura_parser::parse(&old_src);
    let (new_ast, new_errs) = assura_parser::parse(&new_src);

    if !old_errs.is_empty() || old_ast.is_none() {
        eprintln!("Cannot verify evolution: {old_path} has parse errors");
        process::exit(1);
    }
    if !new_errs.is_empty() || new_ast.is_none() {
        eprintln!("Cannot verify evolution: {new_path} has parse errors");
        process::exit(1);
    }

    let old_ast = old_ast.unwrap();
    let new_ast = new_ast.unwrap();

    let results = assura_smt::verify_file_evolution(&old_ast, &new_ast);

    if results.is_empty() {
        if format == "json" {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "evolution": [],
                    "compatible": true,
                }))
                .unwrap()
            );
        } else {
            println!("No matching contracts to verify evolution.");
        }
        return;
    }

    let mut all_pass = true;
    if format == "json" {
        let json_results: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                let pre_ok = matches!(
                    r.precondition_weakening,
                    assura_smt::VerificationResult::Verified { .. }
                );
                let post_ok = matches!(
                    r.postcondition_strengthening,
                    assura_smt::VerificationResult::Verified { .. }
                );
                if !pre_ok || !post_ok {
                    all_pass = false;
                }
                serde_json::json!({
                    "contract": r.contract_name,
                    "precondition_weakening": format!("{:?}", r.precondition_weakening),
                    "postcondition_strengthening": format!("{:?}", r.postcondition_strengthening),
                    "compatible": pre_ok && post_ok,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "evolution": json_results,
                "compatible": all_pass,
            }))
            .unwrap()
        );
    } else {
        println!("\nContract evolution verification:");
        for r in &results {
            println!("  {}:", r.contract_name);
            let pre_status = match &r.precondition_weakening {
                assura_smt::VerificationResult::Verified { .. } => "verified",
                assura_smt::VerificationResult::Counterexample { .. } => {
                    all_pass = false;
                    "FAILED (preconditions strengthened)"
                }
                assura_smt::VerificationResult::Unknown { reason, .. } => {
                    eprintln!("    warning: {reason}");
                    "unknown"
                }
                assura_smt::VerificationResult::Timeout { .. } => {
                    all_pass = false;
                    "timeout"
                }
            };
            println!("    precondition weakening  ... {pre_status}");

            let post_status = match &r.postcondition_strengthening {
                assura_smt::VerificationResult::Verified { .. } => "verified",
                assura_smt::VerificationResult::Counterexample { .. } => {
                    all_pass = false;
                    "FAILED (postconditions weakened)"
                }
                assura_smt::VerificationResult::Unknown { reason, .. } => {
                    eprintln!("    warning: {reason}");
                    "unknown"
                }
                assura_smt::VerificationResult::Timeout { .. } => {
                    all_pass = false;
                    "timeout"
                }
            };
            println!("    postcondition strength. ... {post_status}");
        }
    }

    if !all_pass {
        process::exit(1);
    }
}

pub(crate) struct DiffEntry {
    name: String,
    kind: String,
    added_clauses: Vec<String>,
    removed_clauses: Vec<String>,
    unchanged_clauses: Vec<String>,
}

// ===========================================================================
// Integration tests: full pipeline from source text through all passes
// ===========================================================================
