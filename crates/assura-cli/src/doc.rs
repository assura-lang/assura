//! `assura doc` command: generate Markdown documentation from contract files.

use std::fmt::Write;
use std::path::Path;

use assura_config::{CompilerConfig, OutputMode, Verbosity};
use assura_parser::ast::*;

/// Run the `doc` command.
pub(crate) fn run_doc(
    file: &str,
    output_dir: Option<&str>,
    verify: bool,
    output_mode: OutputMode,
    verbosity: Verbosity,
) {
    let path = Path::new(file);

    // Collect source files (single file or directory)
    let files: Vec<std::path::PathBuf> = if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .unwrap_or_else(|e| {
                eprintln!("error: cannot read directory {file}: {e}");
                std::process::exit(2);
            })
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "assura"))
            .collect();
        entries.sort();
        entries
    } else {
        vec![path.to_path_buf()]
    };

    if files.is_empty() {
        eprintln!("error: no .assura files found in {file}");
        std::process::exit(2);
    }

    let mut all_docs = String::new();

    for source_path in &files {
        let source = match std::fs::read_to_string(source_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: cannot read {}: {e}", source_path.display());
                std::process::exit(2);
            }
        };

        let filename = source_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();

        let mut config = CompilerConfig::default();
        if verify {
            config.verify.layer = 1;
        }

        let output = assura_pipeline::compile(&source, &filename, &config);

        // Optionally run verification
        let verification = if verify && !output.has_errors {
            if let Some(ref typed) = output.typed {
                let v_out = assura_pipeline::verify_typed(typed, &filename, &config);
                Some(v_out)
            } else {
                None
            }
        } else {
            None
        };

        // Generate documentation from the parsed AST
        let doc = generate_file_doc(&filename, &output.file, &verification);
        all_docs.push_str(&doc);
    }

    // Output
    match output_dir {
        Some(dir) => {
            std::fs::create_dir_all(dir).unwrap_or_else(|e| {
                eprintln!("error: cannot create output directory {dir}: {e}");
                std::process::exit(2);
            });
            let out_path = Path::new(dir).join("contracts.md");
            std::fs::write(&out_path, &all_docs).unwrap_or_else(|e| {
                eprintln!("error: cannot write {}: {e}", out_path.display());
                std::process::exit(2);
            });
            if verbosity != Verbosity::Quiet {
                if output_mode == OutputMode::Json {
                    println!(
                        "{{\"output\":\"{}\",\"status\":\"ok\"}}",
                        out_path.display()
                    );
                } else {
                    println!("Documentation written to {}", out_path.display());
                }
            }
        }
        None => {
            if output_mode == OutputMode::Json {
                let report = serde_json::json!({
                    "markdown": all_docs,
                    "files": files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                print!("{all_docs}");
            }
        }
    }
}

/// Generate Markdown documentation for a single source file.
fn generate_file_doc(
    filename: &str,
    parsed: &Option<SourceFile>,
    verification: &Option<Vec<assura_smt::VerificationResult>>,
) -> String {
    let Some(parsed) = parsed else {
        return format!("# {filename}\n\n*Parse error: could not generate documentation.*\n\n");
    };

    let mut doc = String::new();

    // File header
    let _ = writeln!(doc, "# {filename}\n");

    // Project/module metadata
    if let Some(proj) = &parsed.project {
        let _ = writeln!(doc, "**Project:** {}\n", proj.name);
    }
    if let Some(module) = &parsed.module {
        let path_str = module.path.join(".");
        let _ = writeln!(doc, "**Module:** {path_str}\n");
    }
    if !parsed.imports.is_empty() {
        let _ = writeln!(doc, "**Imports:**\n");
        for imp in &parsed.imports {
            let path_str = imp.path.join(".");
            let _ = writeln!(doc, "- `{path_str}`");
        }
        let _ = writeln!(doc);
    }

    // Document each declaration
    for decl in &parsed.decls {
        let decl_doc = generate_decl_doc(&decl.node, verification);
        doc.push_str(&decl_doc);
    }

    doc
}

/// Generate documentation for a single declaration.
fn generate_decl_doc(
    decl: &Decl,
    verification: &Option<Vec<assura_smt::VerificationResult>>,
) -> String {
    let mut doc = String::new();

    match decl {
        Decl::Contract(c) => write_contract_doc(&mut doc, c, verification),
        Decl::Service(s) => write_service_doc(&mut doc, s),
        Decl::TypeDef(t) => write_typedef_doc(&mut doc, t),
        Decl::EnumDef(e) => write_enum_doc(&mut doc, e),
        Decl::Extern(ext) => write_extern_doc(&mut doc, ext),
        Decl::FnDef(f) => write_fndef_doc(&mut doc, f),
        Decl::Bind(b) => write_bind_doc(&mut doc, b),
        _ => {}
    }

    doc
}

fn write_contract_doc(
    doc: &mut String,
    contract: &ContractDecl,
    verification: &Option<Vec<assura_smt::VerificationResult>>,
) {
    let _ = writeln!(doc, "## Contract: `{}`\n", contract.name);

    // Parameters from fn_params
    if !contract.fn_params.is_empty() {
        let _ = writeln!(doc, "### Parameters\n");
        let _ = writeln!(doc, "| Name | Type |");
        let _ = writeln!(doc, "|------|------|");
        for param in &contract.fn_params {
            let ty_str = param
                .ty
                .as_ref()
                .map(|t| t.to_string())
                .unwrap_or_else(|| "?".to_string());
            let _ = writeln!(doc, "| `{}` | `{ty_str}` |", param.name);
        }
        let _ = writeln!(doc);
    }

    // Clauses
    write_clauses_section(doc, &contract.clauses);

    // Verification status
    if let Some(results) = verification {
        write_verification_status(doc, &contract.name, results);
    }

    let _ = writeln!(doc, "---\n");
}

fn write_service_doc(doc: &mut String, service: &ServiceDecl) {
    let _ = writeln!(doc, "## Service: `{}`\n", service.name);

    if !service.items.is_empty() {
        for item in &service.items {
            match item {
                ServiceItem::Operation { name, clauses } => {
                    let _ = writeln!(doc, "### Operation: `{name}`\n");
                    write_clauses_section(doc, clauses);
                }
                ServiceItem::Query { name, clauses } => {
                    let _ = writeln!(doc, "### Query: `{name}`\n");
                    write_clauses_section(doc, clauses);
                }
                ServiceItem::States(states) => {
                    let _ = writeln!(doc, "### States\n");
                    for s in states {
                        let _ = writeln!(doc, "- `{s}`");
                    }
                    let _ = writeln!(doc);
                }
                ServiceItem::Invariant(expr) => {
                    let _ = writeln!(doc, "### Invariant\n\n`{}`\n", expr_to_string(expr));
                }
                ServiceItem::TypeDef(td) => write_typedef_doc(doc, td),
                ServiceItem::EnumDef(ed) => write_enum_doc(doc, ed),
                ServiceItem::Other { kind, body } => {
                    let _ = writeln!(doc, "### {kind}\n\n`{}`\n", expr_to_string(body));
                }
            }
        }
    }

    let _ = writeln!(doc, "---\n");
}

fn write_typedef_doc(doc: &mut String, typedef: &TypeDef) {
    let _ = writeln!(doc, "## Type: `{}`\n", typedef.name);

    match &typedef.body {
        TypeBody::Struct(fields) => {
            let _ = writeln!(doc, "### Fields\n");
            let _ = writeln!(doc, "| Name | Type |");
            let _ = writeln!(doc, "|------|------|");
            for field in fields {
                let ty_str = field
                    .ty
                    .as_ref()
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "?".to_string());
                let _ = writeln!(doc, "| `{}` | `{ty_str}` |", field.name);
            }
            let _ = writeln!(doc);
        }
        TypeBody::Alias(tokens) => {
            let alias_str = tokens.join(" ");
            let _ = writeln!(doc, "Alias for `{alias_str}`\n");
        }
        TypeBody::Refined(tokens) => {
            let refined_str = tokens.join(" ");
            let _ = writeln!(doc, "Refinement type: `{refined_str}`\n");
        }
        TypeBody::Empty => {}
    }

    let _ = writeln!(doc, "---\n");
}

fn write_enum_doc(doc: &mut String, enum_def: &EnumDef) {
    let _ = writeln!(doc, "## Enum: `{}`\n", enum_def.name);

    if !enum_def.variants.is_empty() {
        let _ = writeln!(doc, "### Variants\n");
        for variant in &enum_def.variants {
            if variant.fields.is_empty() {
                let _ = writeln!(doc, "- `{}`", variant.name);
            } else {
                // Field strings may be space-joined multi-token types.
                let types: Vec<String> = variant
                    .fields
                    .iter()
                    .map(|f| f.split_whitespace().collect::<Vec<_>>().join(""))
                    .collect();
                let _ = writeln!(doc, "- `{}({})`", variant.name, types.join(", "));
            }
        }
        let _ = writeln!(doc);
    }

    let _ = writeln!(doc, "---\n");
}

fn write_extern_doc(doc: &mut String, ext: &ExternDecl) {
    let _ = writeln!(doc, "## Extern: `{}`\n", ext.name);
    if !ext.params.is_empty() {
        let _ = writeln!(doc, "### Parameters\n");
        let _ = writeln!(doc, "| Name | Type |");
        let _ = writeln!(doc, "|------|------|");
        for param in &ext.params {
            let ty_str = param
                .ty
                .as_ref()
                .map(|t| t.to_string())
                .unwrap_or_else(|| "?".to_string());
            let _ = writeln!(doc, "| `{}` | `{ty_str}` |", param.name);
        }
        let _ = writeln!(doc);
    }
    write_clauses_section(doc, &ext.clauses);
    let _ = writeln!(doc, "---\n");
}

fn write_fndef_doc(doc: &mut String, fndef: &FnDef) {
    let kind = if fndef.is_ghost {
        "Ghost Function"
    } else if fndef.is_lemma {
        "Lemma"
    } else {
        "Function"
    };
    let _ = writeln!(doc, "## {kind}: `{}`\n", fndef.name);
    if !fndef.params.is_empty() {
        let _ = writeln!(doc, "### Parameters\n");
        let _ = writeln!(doc, "| Name | Type |");
        let _ = writeln!(doc, "|------|------|");
        for param in &fndef.params {
            let ty_str = param
                .ty
                .as_ref()
                .map(|t| t.to_string())
                .unwrap_or_else(|| "?".to_string());
            let _ = writeln!(doc, "| `{}` | `{ty_str}` |", param.name);
        }
        let _ = writeln!(doc);
    }
    write_clauses_section(doc, &fndef.clauses);
    let _ = writeln!(doc, "---\n");
}

fn write_bind_doc(doc: &mut String, bind: &BindDecl) {
    let _ = writeln!(doc, "## Bind: `{}`\n", bind.name);
    let _ = writeln!(doc, "Target: `{}`\n", bind.target_path);
    write_clauses_section(doc, &bind.clauses);
    let _ = writeln!(doc, "---\n");
}

// ---- Helpers ----

fn write_clauses_section(doc: &mut String, clauses: &[Clause]) {
    let requires: Vec<_> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .collect();
    let ensures: Vec<_> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .collect();
    let invariants: Vec<_> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Invariant)
        .collect();
    let effects: Vec<_> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Effects)
        .collect();

    if !requires.is_empty() {
        let _ = writeln!(doc, "### Preconditions (requires)\n");
        for clause in &requires {
            let _ = writeln!(doc, "- `{}`", expr_to_string(&clause.body));
        }
        let _ = writeln!(doc);
    }

    if !ensures.is_empty() {
        let _ = writeln!(doc, "### Postconditions (ensures)\n");
        for clause in &ensures {
            let _ = writeln!(doc, "- `{}`", expr_to_string(&clause.body));
        }
        let _ = writeln!(doc);
    }

    if !invariants.is_empty() {
        let _ = writeln!(doc, "### Invariants\n");
        for clause in &invariants {
            let _ = writeln!(doc, "- `{}`", expr_to_string(&clause.body));
        }
        let _ = writeln!(doc);
    }

    if !effects.is_empty() {
        let _ = writeln!(doc, "### Effects\n");
        for clause in &effects {
            let _ = writeln!(doc, "- `{}`", expr_to_string(&clause.body));
        }
        let _ = writeln!(doc);
    }

    // Other clauses (feature-specific)
    let others: Vec<_> = clauses
        .iter()
        .filter(|c| {
            !matches!(
                c.kind,
                ClauseKind::Requires
                    | ClauseKind::Ensures
                    | ClauseKind::Invariant
                    | ClauseKind::Effects
                    | ClauseKind::Input
                    | ClauseKind::Output
            )
        })
        .collect();

    if !others.is_empty() {
        let _ = writeln!(doc, "### Specification Clauses\n");
        for clause in &others {
            let kind = match &clause.kind {
                ClauseKind::Other(k) => k.as_str(),
                ClauseKind::Decreases => "decreases",
                ClauseKind::Modifies => "modifies",
                ClauseKind::Errors => "errors",
                ClauseKind::Rule => "rule",
                ClauseKind::DataFlow => "data_flow",
                ClauseKind::MustNot => "must_not",
                ClauseKind::Ordering => "ordering",
                _ => "clause",
            };
            let _ = writeln!(doc, "- **{kind}**: `{}`", expr_to_string(&clause.body));
        }
        let _ = writeln!(doc);
    }
}

fn write_verification_status(
    doc: &mut String,
    contract_name: &str,
    results: &[assura_smt::VerificationResult],
) {
    // Filter results relevant to this contract
    let relevant: Vec<_> = results
        .iter()
        .filter(|r| {
            let desc = match r {
                assura_smt::VerificationResult::Verified { clause_desc, .. } => clause_desc,
                assura_smt::VerificationResult::Counterexample { clause_desc, .. } => clause_desc,
                assura_smt::VerificationResult::Timeout { clause_desc, .. } => clause_desc,
                assura_smt::VerificationResult::Unknown { clause_desc, .. } => clause_desc,
            };
            desc.starts_with(contract_name)
        })
        .collect();

    if relevant.is_empty() {
        return;
    }

    let _ = writeln!(doc, "### Verification Status\n");
    let _ = writeln!(doc, "| Clause | Status |");
    let _ = writeln!(doc, "|--------|--------|");

    for result in &relevant {
        match result {
            assura_smt::VerificationResult::Verified { clause_desc, .. } => {
                let _ = writeln!(doc, "| `{clause_desc}` | Verified |");
            }
            assura_smt::VerificationResult::Counterexample { clause_desc, .. } => {
                let _ = writeln!(doc, "| `{clause_desc}` | Counterexample found |");
            }
            assura_smt::VerificationResult::Timeout { clause_desc, .. } => {
                let _ = writeln!(doc, "| `{clause_desc}` | Timeout |");
            }
            assura_smt::VerificationResult::Unknown {
                clause_desc,
                reason,
                ..
            } => {
                if assura_smt::is_known_smt_limitation(reason) {
                    let _ = writeln!(doc, "| `{clause_desc}` | Not yet encoded |");
                } else {
                    let _ = writeln!(doc, "| `{clause_desc}` | Unknown |");
                }
            }
        }
    }
    let _ = writeln!(doc);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_generates_markdown_for_demo() {
        let source = std::fs::read_to_string("../../demos/heartbleed.assura")
            .expect("demo file should exist");
        let output = assura_pipeline::compile(&source, "heartbleed.assura", &Default::default());
        let doc = generate_file_doc("heartbleed.assura", &output.file, &None);

        assert!(
            doc.contains("# heartbleed.assura"),
            "should have file header"
        );
        assert!(
            doc.contains("HeartbeatSafeResponse"),
            "should document the contract"
        );
        assert!(doc.contains("requires"), "should have requires section");
    }

    #[test]
    fn doc_generates_params_table() {
        let source = r#"
contract Div {
    input(a: Int, b: Int)
    requires { b != 0 }
    ensures { result == a / b }
}
"#;
        let output = assura_pipeline::compile(source, "test.assura", &Default::default());
        let doc = generate_file_doc("test.assura", &output.file, &None);

        assert!(doc.contains("b != 0"), "should show requires clause");
        assert!(
            doc.contains("result == a / b"),
            "should show ensures clause"
        );
    }

    #[test]
    fn doc_handles_service_decls() {
        let source = r#"
service Counter {
    state { count: Int }
    transition idle -> active via start {}
}
"#;
        let output = assura_pipeline::compile(source, "test.assura", &Default::default());
        let doc = generate_file_doc("test.assura", &output.file, &None);

        assert!(
            doc.contains("Service: `Counter`"),
            "should document service"
        );
    }

    #[test]
    fn doc_handles_empty_source() {
        let doc = generate_file_doc("empty.assura", &None, &None);
        assert!(doc.contains("Parse error"), "should indicate parse error");
    }

    #[test]
    fn doc_generates_type_and_enum_sections() {
        let source = r#"
type Point {
    x: Int
    y: Int
}

enum Color {
    Red
    Green
    Blue
}
"#;
        let output = assura_pipeline::compile(source, "test.assura", &Default::default());
        let doc = generate_file_doc("test.assura", &output.file, &None);

        assert!(doc.contains("Type: `Point`"), "should document type");
        assert!(doc.contains("Enum: `Color`"), "should document enum");
    }

    #[test]
    fn doc_generates_extern_fn_bind_sections() {
        let source = r#"
extern sha256(data: Bytes) -> Bytes

fn add(a: Int, b: Int) -> Int {
    requires { a >= 0 }
    ensures { result == a + b }
}

bind Logger {
    input(msg: String)
    output(ok: Bool)
}
"#;
        let output = assura_pipeline::compile(source, "test.assura", &Default::default());
        let doc = generate_file_doc("test.assura", &output.file, &None);

        assert!(
            doc.contains("Extern: `sha256`"),
            "should document extern, got: {doc}"
        );
        assert!(
            doc.contains("Function: `add`"),
            "should document fn, got: {doc}"
        );
        assert!(
            doc.contains("Bind: `Logger`"),
            "should document bind, got: {doc}"
        );
    }

    #[test]
    fn doc_uses_dot_separated_paths() {
        let source = "module std.math\nimport std.core\ncontract X { input(x: Int) }\n";
        let output = assura_pipeline::compile(source, "test.assura", &Default::default());
        let doc = generate_file_doc("test.assura", &output.file, &None);

        assert!(
            doc.contains("std.math"),
            "module path should use dots, got: {doc}"
        );
        assert!(
            !doc.contains("std::math"),
            "should NOT use Rust :: syntax for module paths"
        );
        assert!(
            doc.contains("std.core"),
            "import path should use dots, got: {doc}"
        );
        assert!(
            !doc.contains("std::core"),
            "should NOT use Rust :: syntax for import paths"
        );
    }
}
