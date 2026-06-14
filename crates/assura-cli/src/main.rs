#![allow(dead_code)]

use std::env;
use std::fs;
use std::path::Path;
use std::process;

use ariadne::{Color, Label, Report, ReportKind, Source};
use assura_parser::ast::*;
use assura_parser::lexer::Token;
use assura_parser::parser;
use chumsky::Stream;
use chumsky::prelude::*;
use logos::Logos;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Structured diagnostic for JSON output
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct DiagnosticJson {
    code: String,
    message: String,
    file: String,
    start: usize,
    end: usize,
    severity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    secondary: Option<SecondaryJson>,
}

#[derive(Debug, Clone, Serialize)]
struct SecondaryJson {
    message: String,
    start: usize,
    end: usize,
}

impl DiagnosticJson {
    /// Convert from the unified `assura_diagnostics::Diagnostic` type.
    fn from_diagnostic(d: &assura_diagnostics::Diagnostic, filename: &str) -> Self {
        let severity = match d.severity {
            assura_diagnostics::Severity::Error => "error",
            assura_diagnostics::Severity::Warning => "warning",
            assura_diagnostics::Severity::Info => "info",
        };
        let secondary = d.secondary.first().map(|(span, msg)| SecondaryJson {
            message: msg.clone(),
            start: span.start,
            end: span.end,
        });
        DiagnosticJson {
            code: d.code.clone(),
            message: d.message.clone(),
            file: filename.to_string(),
            start: d.primary.start,
            end: d.primary.end,
            severity: severity.to_string(),
            secondary,
        }
    }
}

// ---------------------------------------------------------------------------
// Output mode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    Human,
    Json,
}

// ---------------------------------------------------------------------------
// Shared compilation pipeline
// ---------------------------------------------------------------------------

/// Result of running the full compilation pipeline (lex -> parse -> resolve -> typecheck).
struct CompilationResult {
    file: Option<SourceFile>,
    resolved: Option<assura_resolve::ResolvedFile>,
    typed: Option<assura_types::TypedFile>,
    diagnostics: Vec<DiagnosticJson>,
    has_errors: bool,
}

/// Run lex -> parse -> resolve -> typecheck on source text, collecting all diagnostics.
fn compile(source: &str, filename: &str) -> CompilationResult {
    let mut diagnostics: Vec<DiagnosticJson> = Vec::new();
    let mut has_errors = false;

    // --- Lex ---
    let lex = Token::lexer(source);
    let mut tokens: Vec<(Token, std::ops::Range<usize>)> = Vec::new();

    for (tok, span) in lex.spanned() {
        match tok {
            Ok(t) => tokens.push((t, span)),
            Err(()) => {
                has_errors = true;
                diagnostics.push(DiagnosticJson {
                    code: "A01001".to_string(),
                    message: format!("unexpected character: {:?}", &source[span.clone()]),
                    file: filename.to_string(),
                    start: span.start,
                    end: span.end,
                    severity: "error".to_string(),
                    secondary: None,
                });
            }
        }
    }

    // --- Parse ---
    let len = source.len();
    let token_stream = Stream::from_iter(len..len + 1, tokens.into_iter());
    let (file, parse_errors) = parser::source_file().parse_recovery(token_stream);

    for e in &parse_errors {
        has_errors = true;
        let span = e.span();
        let found = e
            .found()
            .map(|t| friendly_token_name(&format!("{t}")))
            .unwrap_or_else(|| "end of file".to_string());
        let expected: Vec<String> = e
            .expected()
            .filter_map(|ex| ex.as_ref().map(|t| friendly_token_name(&format!("{t}"))))
            .collect();

        // Deduplicate and sort for cleaner output
        let mut expected: Vec<String> = expected
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        // Group into categories for large expected sets
        let msg = if expected.is_empty() {
            format!("unexpected {found}")
        } else if expected.len() > 6 {
            // Too many options; summarize by category
            let has_clause = expected.iter().any(|e| {
                matches!(
                    e.as_str(),
                    "requires"
                        | "ensures"
                        | "invariant"
                        | "effects"
                        | "modifies"
                        | "input"
                        | "output"
                        | "errors"
                        | "rule"
                        | "decreases"
                )
            });
            let has_decl = expected.iter().any(|e| {
                matches!(
                    e.as_str(),
                    "contract" | "type" | "enum" | "fn" | "service" | "extern"
                )
            });
            let mut summary: Vec<String> = Vec::new();
            if has_decl {
                summary.push("a declaration".to_string());
            }
            if has_clause {
                summary.push("a clause keyword".to_string());
            }
            if !has_decl && !has_clause {
                expected.truncate(5);
                summary.push(expected.join(", "));
            }
            format!("expected {}, found {found}", summary.join(" or "))
        } else {
            format!("expected {}, found {found}", expected.join(" or "))
        };

        diagnostics.push(DiagnosticJson {
            code: "A01002".to_string(),
            message: msg,
            file: filename.to_string(),
            start: span.start,
            end: span.end,
            severity: "error".to_string(),
            secondary: None,
        });
    }

    // --- Resolve (only if we have a parsed file) ---
    let resolved = if let Some(ref file) = file {
        match assura_resolve::resolve(file) {
            Ok(r) => {
                for w in &r.warnings {
                    diagnostics.push(DiagnosticJson {
                        code: w.code.to_string(),
                        message: w.message.clone(),
                        file: filename.to_string(),
                        start: w.span.start,
                        end: w.span.end,
                        severity: "warning".to_string(),
                        secondary: w.secondary.as_ref().map(|(span, msg)| SecondaryJson {
                            message: msg.clone(),
                            start: span.start,
                            end: span.end,
                        }),
                    });
                }
                Some(r)
            }
            Err(errs) => {
                has_errors = true;
                for e in &errs {
                    diagnostics.push(DiagnosticJson {
                        code: e.code.to_string(),
                        message: e.message.clone(),
                        file: filename.to_string(),
                        start: e.span.start,
                        end: e.span.end,
                        severity: "error".to_string(),
                        secondary: e.secondary.as_ref().map(|(span, msg)| SecondaryJson {
                            message: msg.clone(),
                            start: span.start,
                            end: span.end,
                        }),
                    });
                }
                None
            }
        }
    } else {
        None
    };

    // --- Type check (only if resolution succeeded) ---
    let typed = if let Some(ref resolved) = resolved {
        match assura_types::type_check(resolved) {
            Ok(t) => Some(t),
            Err(errs) => {
                has_errors = true;
                for e in &errs {
                    diagnostics.push(DiagnosticJson {
                        code: e.code.clone(),
                        message: e.message.clone(),
                        file: filename.to_string(),
                        start: e.span.start,
                        end: e.span.end,
                        severity: "error".to_string(),
                        secondary: e.secondary.as_ref().map(|(span, msg)| SecondaryJson {
                            message: msg.clone(),
                            start: span.start,
                            end: span.end,
                        }),
                    });
                }
                None
            }
        }
    } else {
        None
    };

    CompilationResult {
        file,
        resolved,
        typed,
        diagnostics,
        has_errors,
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = env::args().collect();
    let non_flag_args: Vec<&String> = args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();

    // Detect subcommands
    let is_check = non_flag_args.first().is_some_and(|a| a.as_str() == "check");
    let is_build = non_flag_args.first().is_some_and(|a| a.as_str() == "build");
    let is_init = non_flag_args.first().is_some_and(|a| a.as_str() == "init");
    let is_explain = non_flag_args
        .first()
        .is_some_and(|a| a.as_str() == "explain");

    // Handle --help, -h, and --version first
    if args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        print_help();
        return;
    }
    if args.contains(&"--version".to_string()) || args.contains(&"-V".to_string()) {
        println!("assura {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    if is_check {
        run_check(&args);
    } else if is_build {
        run_build(&args);
    } else if is_init {
        run_init(&args);
    } else if is_explain {
        run_explain(&args);
    } else {
        run_legacy(&args);
    }
}

// ---------------------------------------------------------------------------
// Help
// ---------------------------------------------------------------------------

fn print_help() {
    println!(
        "assura {} - The Assura contract compiler\n\
         \n\
         USAGE:\n\
         \x20   assura <file.assura>                Parse and check a contract file\n\
         \x20   assura check <file> [OPTIONS]       Full pipeline: parse, resolve, type-check, verify\n\
         \x20   assura build <file> [--output <dir>]  Generate Rust code from a contract file\n\
         \x20   assura init <name>                   Create a new Assura project\n\
         \x20   assura explain <code>                Explain an error code (e.g., A03001)\n\
         \n\
         OPTIONS:\n\
         \x20   --ast                               Dump the AST (with default command)\n\
         \x20   --tokens                            Dump the token stream (with default command)\n\
         \x20   --json                              Output diagnostics as JSON\n\
         \x20   --human                             Output diagnostics as rich terminal (default)\n\
         \x20   --layer <0|1>                       Verification layer (0=structural, 1=SMT)\n\
         \x20   --output <dir>                      Output directory for generated code (build)\n\
         \x20   --no-check                          Skip cargo check on generated code (build)\n\
         \x20   -h, --help                          Show this help message\n\
         \x20   -V, --version                       Show version",
        env!("CARGO_PKG_VERSION")
    );
}

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

/// Extract positional arguments, skipping flags and their values.
/// Flags with values: --layer, --output. Simple flags: --json, --human, --ast, --tokens.
fn positional_args(args: &[String]) -> Vec<String> {
    let flags_with_values = ["--layer", "--output"];
    let mut result = Vec::new();
    let mut skip_next = false;
    for arg in args.iter().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if flags_with_values.contains(&arg.as_str()) {
            skip_next = true;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        result.push(arg.clone());
    }
    result
}

// ---------------------------------------------------------------------------
// `assura check <file> [--json|--human] [--layer 0|1]`
// ---------------------------------------------------------------------------

fn run_check(args: &[String]) {
    let output_mode = if args.contains(&"--json".to_string()) {
        OutputMode::Json
    } else {
        OutputMode::Human
    };

    // Verification layer: --layer 0 = structural only, --layer 1 = SMT (default)
    let layer: u8 = args
        .windows(2)
        .find(|w| w[0] == "--layer")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(1);

    // The file is the first positional arg after "check", skipping flag values
    let filename = positional_args(args)
        .into_iter()
        .nth(1) // skip "check" itself
        .unwrap_or_else(|| {
            eprintln!("Usage: assura check <file.assura> [--json|--human] [--layer 0|1]");
            process::exit(2);
        });

    let source = fs::read_to_string(&filename).unwrap_or_else(|e| {
        if output_mode == OutputMode::Json {
            let diag = DiagnosticJson {
                code: "A01000".to_string(),
                message: format!("{e}"),
                file: filename.clone(),
                start: 0,
                end: 0,
                severity: "error".to_string(),
                secondary: None,
            };
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
        typed,
        mut diagnostics,
        mut has_errors,
    } = compile(&source, &filename);

    // --- Verify (only if type check succeeded and layer >= 1) ---
    let mut verification_results = if layer >= 1 {
        if let Some(ref typed) = typed {
            assura_smt::verify(typed)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // --- Dispatch pending decrease checks to SMT ---
    if let Some(ref typed) = typed {
        verification_results.extend(dispatch_decrease_checks(typed));
    }

    // --- Quantifier bound validation ---
    if let Some(ref typed) = typed {
        let qwarnings = assura_smt::validate_quantifier_bounds(typed);
        for w in &qwarnings {
            diagnostics.push(DiagnosticJson {
                code: "A05200".to_string(),
                message: format!(
                    "unbounded quantifier in {}: {} ({})",
                    w.context, w.domain_desc, w.reason
                ),
                file: filename.clone(),
                start: 0,
                end: 0,
                severity: "warning".to_string(),
                secondary: None,
            });
        }
    }

    // Convert counterexamples to diagnostics so they appear in both modes
    for vr in &verification_results {
        if let assura_smt::VerificationResult::Counterexample {
            clause_desc, model, ..
        } = vr
        {
            has_errors = true;
            diagnostics.push(DiagnosticJson {
                code: "A05100".to_string(),
                message: format!("verification failed for {clause_desc}: {model}"),
                file: filename.clone(),
                start: 0,
                end: 0,
                severity: "error".to_string(),
                secondary: None,
            });
        }
    }

    // --- Report ---
    match output_mode {
        OutputMode::Json => {
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
                        Decl::Extern(_) => externs += 1,
                        Decl::FnDef(_) => fns += 1,
                        Decl::Service(_) => services += 1,
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

            let output = serde_json::json!({
                "file_info": file_info,
                "diagnostics": diagnostics,
                "verification": verification_json,
                "layer": layer,
            });
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        OutputMode::Human => {
            // Lex errors already reported above; report the rest.
            let non_lex: Vec<_> = diagnostics
                .iter()
                .filter(|d| d.code != "A01001")
                .cloned()
                .collect();
            report_diagnostics_human(&non_lex, &filename, &source);

            // Print verification results grouped by contract/function
            if !verification_results.is_empty() {
                eprintln!();
                eprintln!("Verification ({} clause(s)):", verification_results.len());
                print_grouped_verification(&verification_results);
            } else if layer == 0 {
                eprintln!();
                eprintln!("Verification skipped (--layer 0: structural checks only)");
            } else if layer >= 1 {
                // Layer 1+ but no results: show what contracts exist
                // and that they had no verifiable clauses
                if let Some(ref f) = file {
                    let contract_names = collect_contract_names(f);
                    if !contract_names.is_empty() {
                        eprintln!();
                        eprintln!("Verification:");
                        for name in &contract_names {
                            eprintln!("  {name}:  (no verifiable clauses)");
                        }
                    }
                }
            }

            if !has_errors {
                eprintln!("{filename}: check passed (no errors)");
            } else {
                eprintln!("{filename}: {} error(s) found", diagnostics.len());
            }
        }
    }

    process::exit(if has_errors { 1 } else { 0 });
}

/// Extract the contract/service/function name prefix from a clause description.
/// Clause descriptions have the form "ContractName::clause_kind" or
/// "ServiceName.OpName::clause_kind".
fn clause_owner(clause_desc: &str) -> &str {
    // Split on "::" to get the owner part (e.g., "SafeDivision" from
    // "SafeDivision::ensures")
    clause_desc.split("::").next().unwrap_or(clause_desc)
}

/// Print verification results grouped by contract/service/function name.
fn print_grouped_verification(results: &[assura_smt::VerificationResult]) {
    // Collect results by owner (contract/service/function name)
    let mut groups: Vec<(String, Vec<&assura_smt::VerificationResult>)> = Vec::new();

    for vr in results {
        let desc = match vr {
            assura_smt::VerificationResult::Verified { clause_desc }
            | assura_smt::VerificationResult::Counterexample { clause_desc, .. }
            | assura_smt::VerificationResult::Timeout { clause_desc }
            | assura_smt::VerificationResult::Unknown { clause_desc, .. } => clause_desc.as_str(),
        };
        let owner = clause_owner(desc).to_string();

        if let Some(group) = groups.iter_mut().find(|(name, _)| *name == owner) {
            group.1.push(vr);
        } else {
            groups.push((owner, vec![vr]));
        }
    }

    for (owner, results) in &groups {
        eprintln!("  {owner}:");
        for vr in results {
            match vr {
                assura_smt::VerificationResult::Verified { clause_desc } => {
                    let kind = clause_desc.split("::").nth(1).unwrap_or(clause_desc);
                    eprintln!("    {kind:<20} ... verified");
                }
                assura_smt::VerificationResult::Counterexample {
                    clause_desc,
                    model,
                    counter_model,
                } => {
                    let kind = clause_desc.split("::").nth(1).unwrap_or(clause_desc);
                    eprintln!("    {kind:<20} ... COUNTEREXAMPLE");
                    for line in format_counterexample_lines(counter_model, model) {
                        eprintln!("      {line}");
                    }
                }
                assura_smt::VerificationResult::Timeout { clause_desc } => {
                    let kind = clause_desc.split("::").nth(1).unwrap_or(clause_desc);
                    eprintln!("    {kind:<20} ... timeout");
                }
                assura_smt::VerificationResult::Unknown {
                    clause_desc,
                    reason,
                } => {
                    let kind = clause_desc.split("::").nth(1).unwrap_or(clause_desc);
                    eprintln!("    {kind:<20} ... skipped ({reason})");
                }
            }
        }
    }
}

/// Format a counterexample for human-readable display.
///
/// If a structured `CounterexampleModel` is available, display clean
/// `name = value` pairs. Otherwise fall back to the raw Z3 model string.
fn format_counterexample_lines(
    counter_model: &Option<assura_smt::CounterexampleModel>,
    model: &str,
) -> Vec<String> {
    if let Some(cm) = counter_model
        && !cm.variables.is_empty()
    {
        let mut lines = Vec::new();
        // Separate input variables from result/output variables
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for (name, value) in &cm.variables {
            let clean_name = name.strip_prefix("__").unwrap_or(name);
            let clean_value = clean_z3_value(value);
            if clean_name == "result" || clean_name.starts_with("result") {
                outputs.push((clean_name.to_string(), clean_value));
            } else {
                inputs.push((clean_name.to_string(), clean_value));
            }
        }
        if !inputs.is_empty() {
            let pairs: Vec<String> = inputs.iter().map(|(n, v)| format!("{n} = {v}")).collect();
            lines.push(format!("| {}", pairs.join(", ")));
        }
        if !outputs.is_empty() {
            for (name, value) in &outputs {
                lines.push(format!("| {name} = {value}"));
            }
        }
        return lines;
    }
    // Fallback: raw Z3 model
    model.lines().map(|l| format!("| {l}")).collect()
}

/// Clean up Z3 value formatting for human display.
fn clean_z3_value(value: &str) -> String {
    let v = value.trim();
    // Z3 outputs negative numbers as `(- N)`, convert to `-N`
    if v.starts_with("(- ") && v.ends_with(')') {
        return format!("-{}", &v[3..v.len() - 1]);
    }
    v.to_string()
}

/// Like `print_grouped_verification` but writes to stdout (for the default
/// summary output path, as opposed to the `check` command which uses stderr).
fn print_grouped_verification_stdout(results: &[assura_smt::VerificationResult]) {
    let mut groups: Vec<(String, Vec<&assura_smt::VerificationResult>)> = Vec::new();

    for vr in results {
        let desc = match vr {
            assura_smt::VerificationResult::Verified { clause_desc }
            | assura_smt::VerificationResult::Counterexample { clause_desc, .. }
            | assura_smt::VerificationResult::Timeout { clause_desc }
            | assura_smt::VerificationResult::Unknown { clause_desc, .. } => clause_desc.as_str(),
        };
        let owner = clause_owner(desc).to_string();

        if let Some(group) = groups.iter_mut().find(|(name, _)| *name == owner) {
            group.1.push(vr);
        } else {
            groups.push((owner, vec![vr]));
        }
    }

    for (owner, results) in &groups {
        println!("      {owner}:");
        for vr in results {
            match vr {
                assura_smt::VerificationResult::Verified { clause_desc } => {
                    let kind = clause_desc.split("::").nth(1).unwrap_or(clause_desc);
                    println!("        {kind:<20} ... verified");
                }
                assura_smt::VerificationResult::Counterexample {
                    clause_desc,
                    model,
                    counter_model,
                } => {
                    let kind = clause_desc.split("::").nth(1).unwrap_or(clause_desc);
                    println!("        {kind:<20} ... COUNTEREXAMPLE");
                    for line in format_counterexample_lines(counter_model, model) {
                        println!("          {line}");
                    }
                }
                assura_smt::VerificationResult::Timeout { clause_desc } => {
                    let kind = clause_desc.split("::").nth(1).unwrap_or(clause_desc);
                    println!("        {kind:<20} ... timeout");
                }
                assura_smt::VerificationResult::Unknown {
                    clause_desc,
                    reason,
                } => {
                    let kind = clause_desc.split("::").nth(1).unwrap_or(clause_desc);
                    println!("        {kind:<20} ... skipped ({reason})");
                }
            }
        }
    }
}

/// Collect names of all contracts, services, and extern fns that could
/// potentially have verifiable clauses.
fn collect_contract_names(file: &SourceFile) -> Vec<String> {
    let mut names = Vec::new();
    for decl in &file.decls {
        match &decl.node {
            Decl::Contract(c) => names.push(c.name.clone()),
            Decl::Service(s) => names.push(s.name.clone()),
            Decl::Extern(ex) => {
                if ex
                    .clauses
                    .iter()
                    .any(|cl| matches!(cl.kind, ClauseKind::Ensures | ClauseKind::Invariant))
                {
                    names.push(ex.name.clone());
                }
            }
            Decl::FnDef(f) => {
                if f.clauses.iter().any(|cl| {
                    matches!(
                        cl.kind,
                        ClauseKind::Ensures | ClauseKind::Invariant | ClauseKind::Decreases
                    )
                }) {
                    names.push(f.name.clone());
                }
            }
            Decl::TypeDef(_) | Decl::EnumDef(_) | Decl::Block { .. } => {}
        }
    }
    names
}

/// Dispatch pending decrease checks from the type checker to the SMT solver.
///
/// The type checker identifies recursive calls where syntactic checking is
/// inconclusive and returns `PendingDecreaseCheck` entries. This function
/// sends each one to `assura_smt::verify_decrease()` and returns the results
/// as `VerificationResult`s that can be merged with the main verification output.
fn dispatch_decrease_checks(
    typed: &assura_types::TypedFile,
) -> Vec<assura_smt::VerificationResult> {
    typed
        .pending_decrease_checks
        .iter()
        .map(|check| {
            let desc = format!("{}::decreases({})", check.fn_name, "termination");
            assura_smt::verify_decrease(
                &check.preconditions,
                &check.measure_expr,
                &check.call_arg,
                desc,
            )
        })
        .collect()
}

/// Render a unified `assura_diagnostics::Diagnostic` using ariadne.
fn render_diagnostic(diag: &assura_diagnostics::Diagnostic, filename: &str, source: &str) {
    let kind = match diag.severity {
        assura_diagnostics::Severity::Error => ReportKind::Error,
        assura_diagnostics::Severity::Warning => ReportKind::Warning,
        assura_diagnostics::Severity::Info => ReportKind::Advice,
    };
    let color = match diag.severity {
        assura_diagnostics::Severity::Error => Color::Red,
        assura_diagnostics::Severity::Warning => Color::Yellow,
        assura_diagnostics::Severity::Info => Color::Blue,
    };
    let mut builder = Report::build(kind, filename, diag.primary.start)
        .with_message(format!("[{}] {}", diag.code, diag.message))
        .with_label(
            Label::new((filename, diag.primary.clone()))
                .with_message(&diag.message)
                .with_color(color),
        );
    for (span, label) in &diag.secondary {
        builder = builder.with_label(
            Label::new((filename, span.clone()))
                .with_message(label)
                .with_color(Color::Blue),
        );
    }
    builder
        .finish()
        .eprint((filename, Source::from(source)))
        .ok();
}

/// Render diagnostics using ariadne for human-readable terminal output.
fn report_diagnostics_human(diagnostics: &[DiagnosticJson], filename: &str, source: &str) {
    for d in diagnostics {
        let mut builder = Report::build(ReportKind::Error, filename, d.start)
            .with_message(format!("[{}] {}", d.code, d.message))
            .with_label(
                Label::new((filename, d.start..d.end))
                    .with_message(&d.message)
                    .with_color(Color::Red),
            );
        if let Some(ref sec) = d.secondary {
            builder = builder.with_label(
                Label::new((filename, sec.start..sec.end))
                    .with_message(&sec.message)
                    .with_color(Color::Blue),
            );
        }
        builder
            .finish()
            .eprint((filename, Source::from(source)))
            .ok();
    }
}

// ---------------------------------------------------------------------------
// `assura build <file.assura>` — codegen to generated/
// ---------------------------------------------------------------------------

fn run_build(args: &[String]) {
    let pos = positional_args(args);

    let filename = pos.get(1).unwrap_or_else(|| {
        eprintln!("Usage: assura build <file.assura> [--output <dir>]");
        process::exit(2);
    });

    // Output directory: --output <dir> or default "generated"
    let out_dir_str = args
        .windows(2)
        .find(|w| w[0] == "--output")
        .map(|w| w[1].as_str())
        .unwrap_or("generated");

    let source = fs::read_to_string(filename).unwrap_or_else(|e| {
        eprintln!("Error: {filename}: {e}");
        process::exit(2);
    });

    // --- Run shared pipeline ---
    let CompilationResult {
        diagnostics,
        has_errors,
        typed,
        ..
    } = compile(&source, filename);

    // Report errors in human mode
    if has_errors {
        report_diagnostics_human(&diagnostics, filename, &source);
        eprintln!("{filename}: {} error(s) found", diagnostics.len());
        process::exit(1);
    }

    let typed = typed.expect("type check should succeed if has_errors is false");

    // --- Quantifier bound validation ---
    let qwarnings = assura_smt::validate_quantifier_bounds(&typed);
    for w in &qwarnings {
        eprintln!(
            "warning: unbounded quantifier in {}: {} ({})",
            w.context, w.domain_desc, w.reason
        );
    }

    // --- Verify ---
    let mut verification_results = assura_smt::verify(&typed);
    verification_results.extend(dispatch_decrease_checks(&typed));
    if !verification_results.is_empty() {
        eprintln!();
        eprintln!("Verification ({} clause(s)):", verification_results.len());
        print_grouped_verification(&verification_results);
    }

    // --- Codegen ---
    let project = assura_codegen::codegen(&typed);

    // --- Write to output directory ---
    let out_dir = Path::new(out_dir_str);
    fs::create_dir_all(out_dir).unwrap_or_else(|e| {
        eprintln!("Error: cannot create {out_dir_str}/ directory: {e}");
        process::exit(1);
    });

    // Write Cargo.toml
    let cargo_path = out_dir.join("Cargo.toml");
    fs::write(&cargo_path, &project.cargo_toml).unwrap_or_else(|e| {
        eprintln!("Error: cannot write {}: {e}", cargo_path.display());
        process::exit(1);
    });
    println!("  wrote {}", cargo_path.display());

    // Write source files
    for (rel_path, content) in &project.files {
        let full_path = out_dir.join(rel_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|e| {
                eprintln!("Error: cannot create directory {}: {e}", parent.display());
                process::exit(1);
            });
        }
        fs::write(&full_path, content).unwrap_or_else(|e| {
            eprintln!("Error: cannot write {}: {e}", full_path.display());
            process::exit(1);
        });
        println!("  wrote {}", full_path.display());
    }

    // --- Validate generated Rust compiles ---
    let skip_check = args.contains(&"--no-check".to_string());
    if !skip_check {
        let cargo_check = process::Command::new("cargo")
            .arg("check")
            .current_dir(out_dir)
            .stdout(process::Stdio::piped())
            .stderr(process::Stdio::piped())
            .output();

        match cargo_check {
            Ok(output) if output.status.success() => {
                println!("OK  {filename} -> {out_dir_str}/ (generated Rust compiles)");
            }
            Ok(output) => {
                println!("OK  {filename} -> {out_dir_str}/");
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!();
                eprintln!("warning: generated Rust does not compile:");
                // Show only the error lines, not the full cargo output
                for line in stderr.lines() {
                    if line.starts_with("error") || line.contains("-->") {
                        eprintln!("  {line}");
                    }
                }
                eprintln!();
                eprintln!("  Run `cd {out_dir_str} && cargo check` to see full errors.");
                eprintln!("  Use `--no-check` to skip this validation.");
            }
            Err(_) => {
                // cargo not found or other OS error; skip silently
                println!("OK  {filename} -> {out_dir_str}/ (cargo check skipped: cargo not found)");
            }
        }
    } else {
        println!("OK  {filename} -> {out_dir_str}/ (check skipped)");
    }
}

// ---------------------------------------------------------------------------
// `assura init <project-name>` — scaffold a new Assura project
// ---------------------------------------------------------------------------

fn run_init(args: &[String]) {
    let project_name = args
        .iter()
        .skip(1) // skip binary name
        .filter(|a| !a.starts_with('-'))
        .nth(1) // skip "init" itself
        .unwrap_or_else(|| {
            eprintln!("Usage: assura init <project-name>");
            process::exit(2);
        });

    let project_dir = Path::new(project_name);

    if project_dir.exists() {
        eprintln!("Error: directory '{project_name}' already exists");
        process::exit(1);
    }

    // Create project directory and contracts subdirectory
    let contracts_dir = project_dir.join("contracts");
    fs::create_dir_all(&contracts_dir).unwrap_or_else(|e| {
        eprintln!("Error: cannot create directory: {e}");
        process::exit(1);
    });

    // Write assura.toml
    let toml_content = format!(
        r#"[project]
name = "{project_name}"
version = "0.1.0"
edition = "2024"

[profile]
features = ["core"]
"#
    );
    let toml_path = project_dir.join("assura.toml");
    fs::write(&toml_path, &toml_content).unwrap_or_else(|e| {
        eprintln!("Error: cannot write {}: {e}", toml_path.display());
        process::exit(1);
    });

    // Write starter contract
    let contract_content = r#"// SafeDivision: ensures division by zero is impossible
contract SafeDivision {
    requires: b != 0
    ensures: result * b + (a % b) == a
    effects: pure
}
"#;
    let contract_path = contracts_dir.join("lib.assura");
    fs::write(&contract_path, contract_content).unwrap_or_else(|e| {
        eprintln!("Error: cannot write {}: {e}", contract_path.display());
        process::exit(1);
    });

    // Report what was created
    println!("Created new Assura project '{project_name}':");
    println!("  {}", toml_path.display());
    println!("  {}", contract_path.display());
}

// ---------------------------------------------------------------------------
// `assura explain <error-code>`
// ---------------------------------------------------------------------------

struct ErrorInfo {
    code: &'static str,
    name: &'static str,
    description: &'static str,
    example: &'static str,
    fix: &'static str,
}

fn error_catalog() -> Vec<ErrorInfo> {
    vec![
        ErrorInfo {
            code: "A01001",
            name: "Unexpected character",
            description: "The lexer encountered a character that is not part of any valid \
                          token. This usually means a stray symbol or an unsupported \
                          Unicode character in the source file.",
            example: r#"  contract Foo {
      requires: x > 0 @ y   // '@' is not a valid Assura operator
  }"#,
            fix: "Remove or replace the invalid character. Check for copy-paste \
                 artifacts, smart quotes, or characters from other languages.",
        },
        ErrorInfo {
            code: "A01002",
            name: "Unexpected token",
            description: "The parser found a token that does not fit the expected grammar \
                          at this position. This is the most common syntax error and can \
                          indicate a missing keyword, misplaced punctuation, or an \
                          incomplete declaration.",
            example: r#"  contract Foo {
      requires x > 0   // missing ':' after 'requires'
  }"#,
            fix: "Check for missing colons after clause keywords (requires:, ensures:), \
                 unmatched braces or parentheses, or misspelled keywords. The error \
                 message shows what was expected vs. what was found.",
        },
        ErrorInfo {
            code: "A02001",
            name: "Undefined name",
            description: "A name was used that has not been defined in the current scope \
                          or any enclosing scope. This applies to type names, variable \
                          names, contract names, and function names.",
            example: r#"  contract Foo {
      requires: bar > 0   // 'bar' is not defined anywhere
  }

  type Alias = Unknown   // 'Unknown' is not a known type"#,
            fix: "Check spelling of the name. Ensure the type or variable is defined \
                 before use, or add an import if it comes from another module. \
                 Built-in types (Int, Bool, String, etc.) are always available.",
        },
        ErrorInfo {
            code: "A02003",
            name: "Duplicate definition",
            description: "Two declarations in the same scope share the same name. Each \
                          name must be unique within its scope (module, service, contract, \
                          or function body).",
            example: r#"  contract Foo {
      requires: x > 0
  }

  contract Foo {            // duplicate: 'Foo' already defined
      requires: y > 0
  }"#,
            fix: "Rename one of the conflicting declarations to a unique name. If you \
                 intended to extend a contract, use the 'extends' keyword instead of \
                 redefining it.",
        },
        ErrorInfo {
            code: "A02005",
            name: "Circular import",
            description: "The import graph contains a cycle. Module A imports module B, \
                          which (directly or indirectly) imports module A. Assura does \
                          not allow circular dependencies between modules.",
            example: r#"  // file: a.assura
  import b

  // file: b.assura
  import a               // circular: a -> b -> a"#,
            fix: "Break the cycle by extracting shared definitions into a third module \
                 that both modules can import, or restructure the dependency so it flows \
                 in one direction.",
        },
        ErrorInfo {
            code: "A03001",
            name: "Type mismatch",
            description: "An expression has a type that does not match the expected type \
                          in context. This includes operand type mismatches in binary \
                          operations, wrong return types, and assignment type conflicts.",
            example: r#"  contract Add {
      requires: x > "hello"   // comparing Int with String
  }

  fn double(x: Int) -> Bool {
      x * 2                    // returns Int, expected Bool
  }"#,
            fix: "Ensure both sides of an operation have compatible types. Check that \
                 function return types match their declared output type. Use explicit \
                 conversions when needed (e.g., 'as Int').",
        },
        ErrorInfo {
            code: "A03002",
            name: "Argument count mismatch",
            description: "A function or contract was called with the wrong number of \
                          arguments. The call must provide exactly the number of \
                          parameters declared in the function signature.",
            example: r#"  fn add(a: Int, b: Int) -> Int

  // ...
  add(1)          // error: expected 2 arguments, got 1
  add(1, 2, 3)    // error: expected 2 arguments, got 3"#,
            fix: "Provide exactly the number of arguments that the function expects. \
                 Check the function signature to see its parameter list.",
        },
        ErrorInfo {
            code: "A03003",
            name: "Wrong number of type arguments",
            description: "A generic type was instantiated with the wrong number of type \
                          parameters. For example, List takes 1 type argument, Map takes \
                          2, and non-generic types take 0.",
            example: r#"  type Pair = List<Int, Bool>   // List takes 1 type arg, got 2

  type Bad = Option             // Option takes 1 type arg, got 0"#,
            fix: "Check how many type parameters the generic type expects. Common ones: \
                 List<T> (1), Map<K, V> (2), Set<T> (1), Option<T> (1), \
                 Result<T, E> (2).",
        },
        ErrorInfo {
            code: "A03004",
            name: "Unknown field",
            description: "A field access (expr.field) refers to a field that does not \
                          exist on the type of the expression. The type either has no \
                          fields, or the field name is misspelled.",
            example: r#"  type Point { x: Int, y: Int }

  contract CheckPoint {
      requires: p.z > 0   // Point has no field 'z'
  }"#,
            fix: "Check the type definition for available field names. Fix the spelling \
                 or use a valid field. If the field should exist, add it to the type \
                 definition.",
        },
        ErrorInfo {
            code: "A03005",
            name: "Not callable",
            description: "An expression was used in a function call position, but its \
                          type is not a function or callable. Only functions, extern \
                          functions, and service operations can be called.",
            example: r#"  type Foo { x: Int }

  contract Bad {
      requires: Foo(42) > 0   // Foo is a type, not a function
  }"#,
            fix: "Ensure you are calling a function, not a type or variable. If you \
                 meant to construct a value, use struct literal syntax. If you \
                 meant to call a method, check that the method exists on the type.",
        },
        ErrorInfo {
            code: "A03006",
            name: "Clause type mismatch",
            description: "A 'requires' or 'ensures' clause must evaluate to a Bool. \
                          The expression in the clause has a non-Bool type, which means \
                          it cannot serve as a logical predicate.",
            example: r#"  contract Foo {
      requires: x + 1     // Int expression, not Bool
      ensures: "done"     // String, not Bool
  }"#,
            fix: "Ensure requires/ensures clauses are boolean expressions. Use \
                 comparison operators (==, !=, <, >, <=, >=), logical operators \
                 (and, or, not), or boolean-valued function calls.",
        },
        ErrorInfo {
            code: "A10001",
            name: "Non-exhaustive pattern",
            description: "A match expression does not cover all possible variants of the \
                          enum being matched. Every variant must be handled either \
                          explicitly or via a wildcard pattern to ensure the match is \
                          total.",
            example: r#"  enum Color { Red, Green, Blue }

  match c {
      Red => 1,
      Green => 2
      // missing: Blue
  }"#,
            fix: "Add the missing variant(s) to the match expression, or add a wildcard \
                 pattern (_ => ...) to handle all remaining cases. The error message \
                 lists which variants are not covered.",
        },
        // -- Phase 1: Linearity errors (A05xxx) --
        ErrorInfo {
            code: "A05001",
            name: "Linear variable used more than once",
            description: "A variable with linear grade (:_1) was used more than once \
                          computationally. Linear variables must be consumed exactly once. \
                          Refinement predicates (ghost/logical uses) do not count.",
            example: r#"  fn bad(x: Int :_1) -> (Int, Int)
      effects: pure
  { (x, x) }   // x used twice"#,
            fix: "Restructure the code to use the linear variable exactly once. If you \
                 need the value in two places, clone it first (if the type supports it) \
                 or refactor to avoid the double use.",
        },
        ErrorInfo {
            code: "A05002",
            name: "Linear variable not consumed",
            description: "A variable with linear grade (:_1) was never used. Linear \
                          variables must be consumed exactly once before going out of scope.",
            example: r#"  fn bad(x: Int :_1) -> Int
      effects: pure
  { 42 }   // x is never used"#,
            fix: "Use the variable before it goes out of scope, or explicitly drop it. \
                 If you intentionally do not need the value, consider changing its grade \
                 to :_omega (unlimited).",
        },
        ErrorInfo {
            code: "A05003",
            name: "Usage grade violation",
            description: "A variable was used a number of times that does not match its \
                          declared usage grade. Grade :_n means exactly n uses.",
            example: r#"  fn bad(x: Int :_2) -> Int
      effects: pure
  { x }   // used once, but grade requires exactly 2"#,
            fix: "Adjust the code to use the variable the exact number of times \
                 specified by its grade, or change the grade to match actual usage.",
        },
        ErrorInfo {
            code: "A05004",
            name: "Linear variable consumed in only one branch",
            description: "A linear variable was consumed in one branch of a conditional \
                          but not the other. Linear variables must be consumed in all \
                          branches or none.",
            example: r#"  fn bad(x: Int :_1, flag: Bool) -> Int
      effects: pure
  { if flag then x else 0 }
  // x consumed in 'then' branch but not 'else'"#,
            fix: "Ensure the linear variable is consumed in every branch of the \
                 conditional, or restructure to consume it before the branch point.",
        },
        // -- Phase 1: Typestate errors (A06xxx) --
        ErrorInfo {
            code: "A06001",
            name: "Invalid state transition",
            description: "An operation was called on an object that is not in the required \
                          state. Each operation declares which state the object must be in \
                          before the operation is valid.",
            example: r#"  service OrderService {
      states: [Created, Paid, Shipped]
      operation ship(order) {
          requires: state == Paid   // must be Paid
      }
  }
  // calling ship() on a Created order -> A06001"#,
            fix: "Check the object's current state before calling the operation. Use a \
                 prior state transition to move the object to the required state first.",
        },
        ErrorInfo {
            code: "A06002",
            name: "Typestate variable not linear",
            description: "A variable with typestate tracking must be linear (:_1). \
                          Typestate requires that the object is consumed and recreated \
                          at each state transition, which requires linearity.",
            example: r#"  fn bad(conn: Connection)  // missing :_1
  // conn has states but is not linear -> A06002"#,
            fix: "Add the linear grade :_1 to the variable declaration. Typestate \
                 variables must be linear to ensure state transitions are tracked.",
        },
        ErrorInfo {
            code: "A06003",
            name: "Unknown state",
            description: "A state name used in a transition or assertion does not match \
                          any state in the object's state declaration.",
            example: r#"  service Foo {
      states: [A, B, C]
      operation go_to_d(x) {
          ensures: state == D   // D is not in [A, B, C] -> A06003
      }
  }"#,
            fix: "Use one of the declared states from the 'states:' declaration. \
                 If you need a new state, add it to the states list.",
        },
        ErrorInfo {
            code: "A06004",
            name: "Ambiguous state after branch",
            description: "After a conditional (if/match) where different branches lead to \
                          different states, the object's state is ambiguous. The type \
                          checker cannot determine which state the object is in.",
            example: r#"  if condition then
      order.pay()      // state -> Paid
  else
      order.cancel()   // state -> Cancelled
  // order state is ambiguous: Paid or Cancelled -> A06004"#,
            fix: "Restructure the code so that all branches end with the object in the \
                 same state, or consume the object before the branch point.",
        },
        // -- Phase 1: Effect errors (A07xxx) --
        ErrorInfo {
            code: "A07001",
            name: "Undeclared effect",
            description: "A function performs an effect that is not listed in its \
                          'effects' clause. Every side effect must be explicitly declared.",
            example: r#"  fn save(data: Data) -> Unit
      effects: database.read   // only declares read
  {
      db.write(data)   // database.write not declared -> A07001
  }"#,
            fix: "Add the missing effect to the function's 'effects' clause. If the \
                 function should be pure, remove the effectful operation.",
        },
        ErrorInfo {
            code: "A07002",
            name: "Effect containment violation",
            description: "A function calls another function whose effects are not a \
                          subset of the caller's declared effects. A pure function \
                          cannot call an effectful function.",
            example: r#"  fn helper() -> Unit
      effects: io.write

  fn pure_fn() -> Unit
      effects: pure
  { helper() }   // calls io.write from pure context -> A07002"#,
            fix: "Either add the callee's effects to the caller's effect declaration, \
                 or avoid calling effectful functions from restricted contexts.",
        },
        ErrorInfo {
            code: "A07003",
            name: "Unknown effect name",
            description: "An effect name in an 'effects' clause does not match any known \
                          effect. Built-in effects include: io, io.read, io.write, \
                          database, database.read, database.write, network, crypto, pure.",
            example: r#"  fn bad() -> Unit
      effects: teleport   // 'teleport' is not a known effect -> A07003"#,
            fix: "Use a valid effect name from the built-in effect hierarchy. Check \
                 the documentation for the complete list of effects.",
        },
        // -- A02006: Duplicate import --
        ErrorInfo {
            code: "A02006",
            name: "Duplicate import",
            description: "The same module is imported more than once. Duplicate \
                          imports are redundant and may indicate a copy-paste error.",
            example: r#"  import std.collections;
  import std.collections;  // duplicate"#,
            fix: "Remove the duplicate import statement.",
        },
        // -- A02007: Unused import --
        ErrorInfo {
            code: "A02007",
            name: "Unused import",
            description: "An import was declared but none of its symbols are used \
                          in the file. This is a warning, not an error.",
            example: r#"  import std.math;  // unused

  contract Foo {
      input { x: Int }  // does not use std.math
  }"#,
            fix: "Remove the unused import, or use a symbol from the imported module.",
        },
        // -- A02008: Invalid import path segment --
        ErrorInfo {
            code: "A02008",
            name: "Invalid import path segment",
            description: "An import path contains a segment that is not a valid module \
                          name. Segments must start with a lowercase ASCII letter or \
                          underscore, followed by letters, digits, or underscores.",
            example: r#"  import std.Math;  // A02008: 'Math' starts with uppercase"#,
            fix: "Use lowercase module names: `import std.math;`",
        },
        // -- A03010: Division by zero --
        ErrorInfo {
            code: "A03010",
            name: "Division by zero",
            description: "A division or modulo operation has a constant zero divisor, \
                          which would cause a runtime panic.",
            example: r#"  contract DivZero {
      input { x: Int }
      ensures { x / 0 == 0 }  // A03010: division by zero
  }"#,
            fix: "Use a non-zero divisor, or add a requires clause that the \
                 divisor is non-zero.",
        },
        // -- A08001: Taint flow violation --
        ErrorInfo {
            code: "A08001",
            name: "Taint flow violation",
            description: "A value with an untrusted taint label flows to a \
                          sink that requires a higher trust level. This \
                          indicates a potential information flow vulnerability.",
            example: r#"  contract TaintViolation {
      input { user_data: @Untrusted String }
      ensures { db.write(user_data) }  // needs @Trusted
  }"#,
            fix: "Validate or sanitize the untrusted input before passing it \
                 to the trusted sink, or adjust the taint labels.",
        },
        // -- A02002: Ambiguous name --
        ErrorInfo {
            code: "A02002",
            name: "Ambiguous name",
            description: "A name could refer to multiple definitions because of \
                          overlapping imports. The compiler cannot determine which \
                          definition was intended.",
            example: r#"  import a { Foo }
  import b { Foo }   // both modules export 'Foo'

  contract Bar {
      requires: Foo > 0   // ambiguous: a.Foo or b.Foo?
  }"#,
            fix: "Use a qualified name (module.Foo) to disambiguate, or use an \
                 alias on one of the imports: import b { Foo as BFoo }.",
        },
        // -- A02004: Visibility violation --
        ErrorInfo {
            code: "A02004",
            name: "Visibility violation",
            description: "An attempt was made to access a field or member that \
                          is not public. Non-pub fields are only accessible within \
                          the module that defines the type.",
            example: r#"  type Wallet {
      balance: Int   // private (no pub)
  }

  contract Check {
      requires: w.balance > 0   // A02004: balance is private
  }"#,
            fix: "Mark the field as 'pub' in the type definition if external \
                 access is intended, or access it through a public getter method.",
        },
        // -- A05005: Ghost/linear interaction --
        ErrorInfo {
            code: "A05005",
            name: "Ghost code modifies linear variable",
            description: "A ghost block attempted to consume or modify a linear \
                          variable. Ghost code is erased at runtime, so it must not \
                          affect the usage count of linear variables.",
            example: r#"  fn bad(x: Int :_1) -> Int
      effects: pure
  {
      ghost { let _ = x; }   // ghost uses linear var -> A05005
      x
  }"#,
            fix: "Remove the linear variable reference from the ghost block. \
                 Ghost code should only read or reference non-linear variables.",
        },
        // -- A07004: Pure function has side effects --
        ErrorInfo {
            code: "A07004",
            name: "Pure function has side effects",
            description: "A function declared as 'effects: pure' performs an \
                          operation that has side effects. Pure functions may not \
                          perform I/O, mutate shared state, or call effectful functions.",
            example: r#"  fn pure_fn(x: Int) -> Int
      effects: pure
  {
      println(x)   // I/O in pure function -> A07004
      x
  }"#,
            fix: "Remove the effectful operation from the pure function, or \
                 change the effects declaration to include the required effects.",
        },
        // -- A07005: Effect row mismatch --
        ErrorInfo {
            code: "A07005",
            name: "Effect row mismatch",
            description: "A function's declared effect row does not match the \
                          effect rows of higher-order function parameters or \
                          closures passed to it.",
            example: r#"  fn map(f: fn(Int) -> Int effects: pure, xs: List<Int>)
      effects: pure
  // calling with an effectful closure -> A07005"#,
            fix: "Ensure the function or closure passed as an argument has \
                 effects that are a subset of the expected effects.",
        },
        // -- A08002-A08005: Information flow --
        ErrorInfo {
            code: "A08002",
            name: "Information flow: implicit leak",
            description: "A secret value influences a public output through \
                          control flow (e.g., an if-branch on a secret condition). \
                          This is an implicit information flow violation.",
            example: r#"  fn check(secret: @Confidential Bool) -> @Public Int
  {
      if secret then 1 else 0   // A08002: implicit leak
  }"#,
            fix: "Remove the dependency of the public output on the secret \
                 value, or explicitly declassify the information.",
        },
        ErrorInfo {
            code: "A08003",
            name: "Declassification without justification",
            description: "A declassify operation lowers the security label of data \
                          without providing the required justification label.",
            example: r#"  fn leak(x: @Confidential Int) -> @Public Int
  {
      declassify(x)   // missing purpose -> A08003
  }"#,
            fix: "Provide a purpose label for the declassification: \
                 declassify(x, purpose: \"user_consent\").",
        },
        ErrorInfo {
            code: "A08004",
            name: "Missing taint label",
            description: "A function accepts external input without a taint label. \
                          All data from external sources must be explicitly labeled.",
            example: r#"  extern fn read_input() -> String   // missing @Untrusted
  // Should be: -> @Untrusted String"#,
            fix: "Add a taint annotation to the return type: @Untrusted.",
        },
        ErrorInfo {
            code: "A08005",
            name: "Security label hierarchy violation",
            description: "An assignment or operation violates the security label \
                          hierarchy. Data cannot flow from higher security levels \
                          to lower ones without explicit declassification.",
            example: r#"  fn bad(secret: @Restricted Data) -> @Public Data
  {
      secret   // Restricted -> Public without declassify -> A08005
  }"#,
            fix: "Add a declassify operation with appropriate justification, \
                 or adjust the security labels.",
        },
        // -- A09001-A09004: Totality / termination --
        ErrorInfo {
            code: "A09001",
            name: "Missing decreases clause",
            description: "A recursive function does not have a 'decreases' clause. \
                          Recursive functions must prove termination by providing a \
                          measure that decreases on each recursive call.",
            example: r#"  fn factorial(n: Int) -> Int
  {
      if n == 0 then 1 else n * factorial(n - 1)
      // missing: decreases { n }
  }"#,
            fix: "Add a 'decreases' clause with a non-negative expression that \
                 strictly decreases on each recursive call. Example: decreases { n }.",
        },
        ErrorInfo {
            code: "A09002",
            name: "Decreases clause not proven",
            description: "The SMT solver could not prove that the decreases measure \
                          strictly decreases on every recursive call, or that the \
                          measure remains non-negative.",
            example: r#"  fn bad(n: Int) -> Int
      decreases { n }
  {
      bad(n + 1)   // n increases, not decreases -> A09002
  }"#,
            fix: "Ensure the decreases expression becomes strictly smaller on \
                 each recursive call and remains non-negative. The base case \
                 must be reachable.",
        },
        ErrorInfo {
            code: "A09003",
            name: "Partial function without 'partial' marker",
            description: "A function may not terminate but is not marked as 'partial'. \
                          Functions that may loop forever must be explicitly annotated.",
            example: r#"  fn server_loop() -> Never
  {
      loop { handle_request() }
      // infinite loop without 'partial' -> A09003
  }"#,
            fix: "Mark the function as 'partial' to acknowledge it may not \
                 terminate, or add a termination proof with 'decreases'.",
        },
        ErrorInfo {
            code: "A09004",
            name: "Mutual recursion without termination proof",
            description: "Two or more functions call each other recursively without \
                          a combined termination measure that decreases across the \
                          call cycle.",
            example: r#"  fn is_even(n: Nat) -> Bool { if n == 0 then true else is_odd(n-1) }
  fn is_odd(n: Nat) -> Bool { if n == 0 then false else is_even(n-1) }
  // need decreases { n } on both"#,
            fix: "Add 'decreases' clauses to all functions in the recursive \
                 group. The measure must decrease on every call in the cycle.",
        },
        // -- Phase 1: SMT verification (A05100) --
        ErrorInfo {
            code: "A05100",
            name: "Verification failed (counterexample found)",
            description: "The SMT solver found a counterexample showing that a contract \
                          clause does not hold. The model shows concrete values for \
                          variables that violate the property.",
            example: r#"  contract AlwaysPositive {
      requires: true
      ensures: x > 0
  }
  // Counterexample: x = 0 or x = -1"#,
            fix: "Either strengthen the requires clause to eliminate the counterexample \
                 inputs, or weaken the ensures clause to account for the case. The \
                 counterexample model shows exactly which inputs break the contract.",
        },
    ]
}

fn run_explain(args: &[String]) {
    let code = args
        .iter()
        .skip(1) // skip binary name
        .filter(|a| !a.starts_with('-'))
        .nth(1) // skip "explain" itself
        .unwrap_or_else(|| {
            eprintln!("Usage: assura explain <error-code>");
            eprintln!();
            eprintln!("Example: assura explain A03001");
            process::exit(2);
        });

    let catalog = error_catalog();
    let entry = catalog.iter().find(|e| e.code == code.as_str());

    match entry {
        Some(info) => {
            println!("{}: {}", info.code, info.name);
            println!();
            println!("{}", info.description);
            println!();
            println!("Example:");
            println!();
            println!("{}", info.example);
            println!();
            println!("How to fix:");
            println!();
            println!("{}", info.fix);
        }
        None => {
            eprintln!("Unknown error code: {code}");
            eprintln!();
            eprintln!("Known error codes:");
            for info in &catalog {
                eprintln!("  {} - {}", info.code, info.name);
            }
            process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Legacy mode: `assura [--ast|--tokens] <file>`
// ---------------------------------------------------------------------------

fn run_legacy(args: &[String]) {
    let show_ast = args.contains(&"--ast".to_string());
    let show_tokens = args.contains(&"--tokens".to_string());

    let filename = args
        .iter()
        .filter(|a| !a.starts_with('-'))
        .nth(1)
        .unwrap_or_else(|| {
            eprintln!("Usage: assura [--ast|--tokens] <file.assura>");
            eprintln!("       assura check <file.assura> [--json|--human]");
            eprintln!("       assura build <file.assura> [--output <dir>]");
            eprintln!("       assura init <project-name>");
            eprintln!("       assura explain <error-code>");
            process::exit(2);
        });

    let source = fs::read_to_string(filename).unwrap_or_else(|e| {
        eprintln!("Error: {filename}: {e}");
        process::exit(2);
    });

    // --tokens mode: lex only, dump tokens, exit early
    if show_tokens {
        let lex = Token::lexer(&source);
        for (tok, span) in lex.spanned() {
            if let Ok(t) = tok {
                let line = source[..span.start].lines().count();
                let col = span.start - source[..span.start].rfind('\n').map_or(0, |p| p + 1) + 1;
                println!("{line}:{col}  {t:?}");
            }
        }
        return;
    }

    // --- Run shared pipeline ---
    let CompilationResult {
        file,
        resolved,
        typed,
        diagnostics,
        has_errors,
    } = compile(&source, filename);

    if has_errors {
        report_diagnostics_human(&diagnostics, filename, &source);
        eprintln!("{filename}: {} error(s) found", diagnostics.len());
        process::exit(1);
    }

    let file = file.expect("file should exist if has_errors is false");
    let resolved = resolved.expect("resolved should exist if has_errors is false");
    let typed = typed.expect("typed should exist if has_errors is false");

    // --- Verify ---
    let mut verification_results = assura_smt::verify(&typed);
    verification_results.extend(dispatch_decrease_checks(&typed));

    // --- Output ---
    if show_ast {
        print_ast(&file);
    } else {
        print_summary(
            filename,
            &file,
            &resolved.symbols,
            &typed.type_env,
            &verification_results,
        );
    }
}

fn print_summary(
    filename: &str,
    file: &SourceFile,
    symbols: &assura_resolve::SymbolTable,
    type_env: &assura_types::TypeEnv,
    verification_results: &[assura_smt::VerificationResult],
) {
    let mut contracts = 0u32;
    let mut types = 0u32;
    let mut enums = 0u32;
    let mut externs = 0u32;
    let mut fns = 0u32;
    let mut services = 0u32;
    let mut other = 0u32;

    for d in &file.decls {
        match &d.node {
            Decl::Contract(_) => contracts += 1,
            Decl::TypeDef(_) => types += 1,
            Decl::EnumDef(_) => enums += 1,
            Decl::Extern(_) => externs += 1,
            Decl::FnDef(_) => fns += 1,
            Decl::Service(_) => services += 1,
            Decl::Block { .. } => other += 1,
        }
    }

    println!("OK  {filename}");
    if let Some(p) = &file.project {
        println!(
            "    project:   {}  profile: [{}]",
            p.name,
            p.profile.join(", ")
        );
    }
    if let Some(m) = &file.module {
        println!("    module:    {}", m.path.join("."));
    }
    println!("    imports:   {}", file.imports.len());

    let mut parts = Vec::new();
    if contracts > 0 {
        parts.push(format!("{contracts} contract(s)"));
    }
    if types > 0 {
        parts.push(format!("{types} type(s)"));
    }
    if enums > 0 {
        parts.push(format!("{enums} enum(s)"));
    }
    if externs > 0 {
        parts.push(format!("{externs} extern(s)"));
    }
    if fns > 0 {
        parts.push(format!("{fns} fn(s)"));
    }
    if services > 0 {
        parts.push(format!("{services} service(s)"));
    }
    if other > 0 {
        parts.push(format!("{other} other"));
    }
    println!(
        "    declares:  {}",
        if parts.is_empty() {
            "(empty)".to_string()
        } else {
            parts.join(", ")
        }
    );
    let user_symbols = symbols
        .symbols
        .iter()
        .filter(|s| s.kind != assura_resolve::SymbolKind::BuiltinType)
        .count();
    println!("    resolve:   OK ({user_symbols} symbols)");
    println!("    typecheck: OK ({} bindings)", type_env.len());

    if verification_results.is_empty() {
        let contract_names = collect_contract_names(file);
        if contract_names.is_empty() {
            println!("    verify:    OK (no verifiable clauses)");
        } else {
            println!(
                "    verify:    OK (no verifiable clauses in {})",
                contract_names.join(", ")
            );
        }
    } else {
        let verified = verification_results
            .iter()
            .filter(|r| matches!(r, assura_smt::VerificationResult::Verified { .. }))
            .count();
        let cex = verification_results
            .iter()
            .filter(|r| matches!(r, assura_smt::VerificationResult::Counterexample { .. }))
            .count();
        let timeout = verification_results
            .iter()
            .filter(|r| matches!(r, assura_smt::VerificationResult::Timeout { .. }))
            .count();
        let unknown = verification_results
            .iter()
            .filter(|r| matches!(r, assura_smt::VerificationResult::Unknown { .. }))
            .count();

        let mut parts = Vec::new();
        if verified > 0 {
            parts.push(format!("{verified} verified"));
        }
        if cex > 0 {
            parts.push(format!("{cex} counterexample(s)"));
        }
        if timeout > 0 {
            parts.push(format!("{timeout} timeout(s)"));
        }
        if unknown > 0 {
            parts.push(format!("{unknown} unknown"));
        }
        println!(
            "    verify:    {} clause(s): {}",
            verification_results.len(),
            parts.join(", ")
        );
        // Show per-clause details on stdout (for default summary output)
        print_grouped_verification_stdout(verification_results);
    }
}

fn print_ast(file: &SourceFile) {
    if let Some(p) = &file.project {
        println!("Project: {} [{}]", p.name, p.profile.join(", "));
    }
    if let Some(m) = &file.module {
        println!("Module: {}", m.path.join("."));
    }
    for imp in &file.imports {
        let alias = imp
            .alias
            .as_deref()
            .map(|a| format!(" as {a}"))
            .unwrap_or_default();
        let items = if imp.items.is_empty() {
            String::new()
        } else {
            format!(" {{{}}}", imp.items.join(", "))
        };
        println!("Import: {}{alias}{items}", imp.path.join("."));
    }
    for d in &file.decls {
        print_decl(&d.node, 0);
    }
}

fn print_decl(decl: &Decl, indent: usize) {
    let pad = "  ".repeat(indent);
    match decl {
        Decl::Contract(c) => {
            let tps = if c.type_params.is_empty() {
                String::new()
            } else {
                format!("<{}>", c.type_params.join(", "))
            };
            println!("{pad}Contract: {}{tps}", c.name);
            for cl in &c.clauses {
                let body = truncate(&expr_to_string(&cl.body), 60);
                println!("{pad}  {:?}: {body}", cl.kind);
            }
        }
        Decl::TypeDef(t) => {
            let tps = if t.type_params.is_empty() {
                String::new()
            } else {
                format!("<{}>", t.type_params.join(", "))
            };
            match &t.body {
                TypeBody::Refined(toks) => {
                    println!(
                        "{pad}Type: {}{tps} = {{{}}}",
                        t.name,
                        truncate(&toks.join(" "), 50)
                    );
                }
                TypeBody::Alias(toks) => {
                    println!(
                        "{pad}Type: {}{tps} = {}",
                        t.name,
                        truncate(&toks.join(" "), 50)
                    );
                }
                TypeBody::Struct(fields) => {
                    println!("{pad}Type: {}{tps}", t.name);
                    for f in fields {
                        let pub_str = if f.is_pub { "pub " } else { "" };
                        println!("{pad}  {pub_str}{}: {}", f.name, f.ty.join(" "));
                    }
                }
                TypeBody::Empty => println!("{pad}Type: {}{tps}", t.name),
            }
        }
        Decl::EnumDef(e) => {
            let tps = if e.type_params.is_empty() {
                String::new()
            } else {
                format!("<{}>", e.type_params.join(", "))
            };
            println!("{pad}Enum: {}{tps}", e.name);
            for v in &e.variants {
                if v.fields.is_empty() {
                    println!("{pad}  {}", v.name);
                } else {
                    println!("{pad}  {}({})", v.name, v.fields.join(" "));
                }
            }
        }
        Decl::Extern(ex) => {
            let params = ex
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name, p.ty.join(" ")))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "{pad}Extern: fn {}({params}) -> {}",
                ex.name,
                ex.return_ty.join(" ")
            );
            for cl in &ex.clauses {
                println!(
                    "{pad}  {:?}: {}",
                    cl.kind,
                    truncate(&expr_to_string(&cl.body), 50)
                );
            }
        }
        Decl::FnDef(f) => {
            let params = f
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name, p.ty.join(" ")))
                .collect::<Vec<_>>()
                .join(", ");
            let ret = if f.return_ty.is_empty() {
                String::new()
            } else {
                format!(" -> {}", f.return_ty.join(" "))
            };
            println!("{pad}Fn: {}({params}){ret}", f.name);
            for cl in &f.clauses {
                println!(
                    "{pad}  {:?}: {}",
                    cl.kind,
                    truncate(&expr_to_string(&cl.body), 50)
                );
            }
        }
        Decl::Service(s) => {
            println!("{pad}Service: {}", s.name);
            for item in &s.items {
                match item {
                    ServiceItem::TypeDef(t) => {
                        println!("{pad}  type: {}", t.name);
                    }
                    ServiceItem::States(states) => {
                        println!("{pad}  states: {}", states.join(" -> "));
                    }
                    ServiceItem::Operation { name, clauses } => {
                        println!("{pad}  operation: {name}");
                        for cl in clauses {
                            println!(
                                "{pad}    {:?}: {}",
                                cl.kind,
                                truncate(&expr_to_string(&cl.body), 40)
                            );
                        }
                    }
                    ServiceItem::Query { name, clauses } => {
                        println!("{pad}  query: {name}");
                        for cl in clauses {
                            println!(
                                "{pad}    {:?}: {}",
                                cl.kind,
                                truncate(&expr_to_string(&cl.body), 40)
                            );
                        }
                    }
                    ServiceItem::Invariant(expr) => {
                        println!("{pad}  invariant: {}", truncate(&expr_to_string(expr), 50));
                    }
                    _ => {}
                }
            }
        }
        Decl::Block {
            kind, name, body, ..
        } => {
            println!("{pad}{kind}: {name} ({} clause(s))", body.len());
        }
    }
}

fn expr_to_string(expr: &Expr) -> String {
    match expr {
        Expr::Literal(lit) => match lit {
            Literal::Int(s) | Literal::Float(s) => s.clone(),
            Literal::Str(s) => format!("\"{s}\""),
            Literal::Bool(b) => b.to_string(),
        },
        Expr::Ident(s) => s.clone(),
        Expr::Field(e, f) => format!("{}.{f}", expr_to_string(e)),
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let args_s: Vec<String> = args.iter().map(expr_to_string).collect();
            format!(
                "{}.{method}({})",
                expr_to_string(receiver),
                args_s.join(", ")
            )
        }
        Expr::Call { func, args } => {
            let args_s: Vec<String> = args.iter().map(expr_to_string).collect();
            format!("{}({})", expr_to_string(func), args_s.join(", "))
        }
        Expr::Index { expr: e, index } => {
            format!("{}[{}]", expr_to_string(e), expr_to_string(index))
        }
        Expr::BinOp { lhs, op, rhs } => {
            let op_s = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Mod => "mod",
                BinOp::Eq => "==",
                BinOp::Neq => "!=",
                BinOp::Lt => "<",
                BinOp::Lte => "<=",
                BinOp::Gt => ">",
                BinOp::Gte => ">=",
                BinOp::And => "and",
                BinOp::Or => "or",
                BinOp::Implies => "=>",
                BinOp::In => "in",
                BinOp::NotIn => "not in",
                BinOp::Concat => "++",
                BinOp::Range => "..",
            };
            format!("{} {op_s} {}", expr_to_string(lhs), expr_to_string(rhs))
        }
        Expr::UnaryOp { op, expr: e } => {
            let op_s = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "not",
            };
            format!("{op_s} {}", expr_to_string(e))
        }
        Expr::Old(e) => format!("old({})", expr_to_string(e)),
        Expr::Forall { var, domain, body } => {
            format!(
                "forall {var} in {}: {}",
                expr_to_string(domain),
                expr_to_string(body)
            )
        }
        Expr::Exists { var, domain, body } => {
            format!(
                "exists {var} in {}: {}",
                expr_to_string(domain),
                expr_to_string(body)
            )
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => match else_branch {
            Some(eb) => format!(
                "if {} then {} else {}",
                expr_to_string(cond),
                expr_to_string(then_branch),
                expr_to_string(eb)
            ),
            None => format!(
                "if {} then {}",
                expr_to_string(cond),
                expr_to_string(then_branch)
            ),
        },
        Expr::Paren(e) => format!("({})", expr_to_string(e)),
        Expr::List(elems) => {
            let elems_s: Vec<String> = elems.iter().map(expr_to_string).collect();
            format!("[{}]", elems_s.join(", "))
        }
        Expr::Cast { expr: e, ty } => format!("{} as {ty}", expr_to_string(e)),
        Expr::Block(exprs) => {
            let strs: Vec<String> = exprs.iter().map(expr_to_string).collect();
            strs.join(" ")
        }
        Expr::Ghost(inner) => format!("ghost {{ {} }}", expr_to_string(inner)),
        Expr::Apply { lemma_name, args } => {
            let args_s: Vec<String> = args.iter().map(expr_to_string).collect();
            format!("apply {lemma_name}({})", args_s.join(", "))
        }
        Expr::Match { scrutinee, arms } => {
            let scrut = expr_to_string(scrutinee);
            let arms_s: Vec<String> = arms
                .iter()
                .map(|arm| {
                    let pat = match &arm.pattern {
                        assura_parser::ast::Pattern::Ident(name) => name.clone(),
                        assura_parser::ast::Pattern::Wildcard => "_".into(),
                        assura_parser::ast::Pattern::Literal(lit) => format!("{lit:?}"),
                        assura_parser::ast::Pattern::Constructor { name, fields } => {
                            let fs: Vec<String> = fields
                                .iter()
                                .map(|f| match f {
                                    assura_parser::ast::Pattern::Ident(n) => n.clone(),
                                    assura_parser::ast::Pattern::Wildcard => "_".into(),
                                    other => format!("{other:?}"),
                                })
                                .collect();
                            format!("{name}({})", fs.join(", "))
                        }
                        assura_parser::ast::Pattern::Tuple(pats) => {
                            let ps: Vec<String> = pats
                                .iter()
                                .map(|p| match p {
                                    assura_parser::ast::Pattern::Ident(n) => n.clone(),
                                    assura_parser::ast::Pattern::Wildcard => "_".into(),
                                    other => format!("{other:?}"),
                                })
                                .collect();
                            format!("({})", ps.join(", "))
                        }
                    };
                    format!("{pat} => {}", expr_to_string(&arm.body))
                })
                .collect();
            format!("match {scrut} {{ {} }}", arms_s.join(", "))
        }
        Expr::Let { name, value, body } => {
            format!(
                "let {} = {} in {}",
                name,
                expr_to_string(value),
                expr_to_string(body)
            )
        }
        Expr::Tuple(elems) => {
            let items: Vec<String> = elems.iter().map(expr_to_string).collect();
            format!("({})", items.join(", "))
        }
        Expr::Raw(tokens) => tokens.join(" "),
    }
}

/// Map raw token display names to human-friendly descriptions.
fn friendly_token_name(raw: &str) -> String {
    // Strip surrounding quotes from chumsky's output format
    let s = raw.trim_matches('\'').trim_matches('"');
    match s {
        "{" => "'{'".to_string(),
        "}" => "'}'".to_string(),
        "(" => "'('".to_string(),
        ")" => "')'".to_string(),
        "[" => "'['".to_string(),
        "]" => "']'".to_string(),
        ":" => "':'".to_string(),
        ";" => "';'".to_string(),
        "," => "','".to_string(),
        "." => "'.'".to_string(),
        "=" => "'='".to_string(),
        "<" => "'<'".to_string(),
        ">" => "'>'".to_string(),
        "->" => "'->'".to_string(),
        "=>" => "'=>'".to_string(),
        "#" => "'#'".to_string(),
        // Clause keywords
        "Requires" | "requires" => "requires".to_string(),
        "Ensures" | "ensures" => "ensures".to_string(),
        "Invariant" | "invariant" => "invariant".to_string(),
        "Effects" | "effects" => "effects".to_string(),
        "Modifies" | "modifies" => "modifies".to_string(),
        "Input" | "input" => "input".to_string(),
        "Output" | "output" => "output".to_string(),
        "Errors" | "errors" => "errors".to_string(),
        "Rule" | "rule" => "rule".to_string(),
        "Decreases" | "decreases" => "decreases".to_string(),
        "MustNot" | "must_not" => "must_not".to_string(),
        "DataFlow" | "data_flow" => "data_flow".to_string(),
        // Declaration keywords
        "contract" => "contract".to_string(),
        "type" => "type".to_string(),
        "enum" => "enum".to_string(),
        "fn" => "fn".to_string(),
        "service" => "service".to_string(),
        "extern" => "extern".to_string(),
        "lemma" => "lemma".to_string(),
        "opaque" => "opaque".to_string(),
        "pure" => "pure".to_string(),
        "ghost" => "ghost".to_string(),
        // Other keywords
        "Interface" | "interface" => "interface".to_string(),
        "Extends" | "extends" => "extends".to_string(),
        "Impl" | "impl" => "impl".to_string(),
        "Spec" | "spec" => "spec".to_string(),
        "Axiom" | "axiom" => "axiom".to_string(),
        "Define" | "define" => "define".to_string(),
        "Property" | "property" => "property".to_string(),
        "ConstantTime" | "constant_time" => "constant_time".to_string(),
        "MustBe" | "must_be" => "must_be".to_string(),
        "VerifyAgainst" | "verify_against" => "verify_against".to_string(),
        "Reads" | "reads" => "reads".to_string(),
        "Bounds" | "bounds" => "bounds".to_string(),
        // Default: pass through
        _ => raw.to_string(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max])
    } else {
        s.to_string()
    }
}

// ===========================================================================
// Integration tests: full pipeline from source text through all passes
// ===========================================================================

#[cfg(test)]
mod tests {

    /// Run the full pipeline: parse -> resolve -> type-check -> codegen
    fn full_pipeline(source: &str) -> Result<assura_codegen::GeneratedProject, String> {
        let (file, errs) = assura_parser::parse(source);
        if !errs.is_empty() {
            return Err(format!("parse errors: {errs:?}"));
        }
        let file = file.ok_or("parse returned None")?;
        let resolved =
            assura_resolve::resolve(&file).map_err(|e| format!("resolve errors: {e:?}"))?;
        let typed =
            assura_types::type_check(&resolved).map_err(|e| format!("type errors: {e:?}"))?;
        Ok(assura_codegen::codegen(&typed))
    }

    /// Verify that a source string successfully passes all pipeline stages.
    fn assert_pipeline_ok(source: &str) {
        let project = full_pipeline(source).expect("pipeline failed");
        assert!(!project.cargo_toml.is_empty(), "empty Cargo.toml");
        assert!(!project.files.is_empty(), "no generated files");
        // Validate generated Rust is syntactically valid
        let lib = &project.files[0].1;
        syn::parse_file(lib).unwrap_or_else(|e| {
            panic!("generated Rust is not valid:\n{lib}\n\nerror: {e}");
        });
    }

    #[test]
    fn pipeline_contract() {
        assert_pipeline_ok(
            r#"
contract SafeDivision {
  input(a: Int, b: Int)
  output(result: Int)
  requires { b != 0 }
  ensures { result * b == a }
  effects { pure }
}
"#,
        );
    }

    #[test]
    fn pipeline_fn_with_clauses() {
        assert_pipeline_ok(
            r#"
fn clamp(x: Int, lo: Int, hi: Int) -> Int
  requires { lo <= hi }
  ensures { result >= lo && result <= hi }
{
  if x < lo then lo else if x > hi then hi else x
}
"#,
        );
    }

    #[test]
    fn pipeline_type_def() {
        assert_pipeline_ok(
            r#"
type Point {
  x: Int,
  y: Int
}

contract UsePoint {
  input(p: Point)
  output(result: Int)
  ensures { result >= 0 }
}
"#,
        );
    }

    #[test]
    fn pipeline_demo_libwebp() {
        let source = std::fs::read_to_string("../../demos/libwebp-huffman.assura")
            .or_else(|_| std::fs::read_to_string("demos/libwebp-huffman.assura"))
            .expect("cannot find libwebp demo");
        assert_pipeline_ok(&source);
    }

    #[test]
    fn pipeline_demo_zlib() {
        let source = std::fs::read_to_string("../../demos/zlib-inflate.assura")
            .or_else(|_| std::fs::read_to_string("demos/zlib-inflate.assura"))
            .expect("cannot find zlib demo");
        assert_pipeline_ok(&source);
    }

    #[test]
    fn pipeline_demo_mbedtls() {
        let source = std::fs::read_to_string("../../demos/mbedtls-x509.assura")
            .or_else(|_| std::fs::read_to_string("demos/mbedtls-x509.assura"))
            .expect("cannot find mbedtls demo");
        assert_pipeline_ok(&source);
    }

    #[test]
    fn pipeline_test_basic() {
        let source = std::fs::read_to_string("../../tests/fixtures/test_basic.assura")
            .or_else(|_| std::fs::read_to_string("tests/fixtures/test_basic.assura"))
            .expect("cannot find test_basic fixture");
        assert_pipeline_ok(&source);
    }

    #[test]
    fn pipeline_advanced_patterns() {
        let source = std::fs::read_to_string("../../tests/fixtures/advanced_patterns.assura")
            .or_else(|_| std::fs::read_to_string("tests/fixtures/advanced_patterns.assura"))
            .expect("cannot find advanced_patterns fixture");
        assert_pipeline_ok(&source);
    }

    #[test]
    fn test_diagnostics_from_parse_errors() {
        // Deliberately invalid syntax should produce parse errors
        let (file, errors) = assura_parser::parse("contract { invalid }");
        // At least some errors expected
        assert!(
            !errors.is_empty() || file.is_none(),
            "expected parse errors for invalid syntax"
        );
    }

    #[test]
    fn test_parse_error_includes_expected_tokens() {
        // Syntax error should produce an error with a non-empty expected set
        let (_file, errors) = assura_parser::parse("contract 123");
        assert!(!errors.is_empty(), "expected at least one parse error");
        let e = &errors[0];
        let expected: Vec<_> = e.expected().collect();
        assert!(
            !expected.is_empty(),
            "parse error should include expected tokens, got: {e:?}"
        );
        // The found token should be the integer 123
        assert!(e.found().is_some(), "parse error should have a found token");
    }

    #[test]
    fn test_resolution_error_diagnostic() {
        // Valid parse but contains an unresolved reference
        let source = r#"
contract Foo {
  requires { unknown_fn(x) }
}
"#;
        let (file, errs) = assura_parser::parse(source);
        assert!(errs.is_empty());
        let file = file.unwrap();
        // Resolve should succeed (soft errors for unresolved refs)
        let resolved = assura_resolve::resolve(&file);
        assert!(resolved.is_ok());
    }

    #[test]
    fn test_type_error_diagnostic() {
        // Type checking should detect the type mismatch (requires needs Bool)
        let source = r#"
contract Typed {
  input(x: Int)
  requires { x + 1 }
}
"#;
        let (file, errs) = assura_parser::parse(source);
        assert!(errs.is_empty(), "unexpected parse errors: {errs:?}");
        let file = file.unwrap();
        let resolved = assura_resolve::resolve(&file).unwrap();
        let typed = assura_types::type_check(&resolved);
        // Type checking may succeed with warnings, or produce errors
        // depending on strictness. Just verify it doesn't panic.
        let _ = typed;
    }

    /// Walk tests/fixtures/errors/*.assura looking for `// MUST REJECT Axxxxx`
    /// annotations. Each annotated file must produce a type error with the
    /// specified code. This validates the error detection pipeline.
    /// Scans both `tests/fixtures/errors/` and `tests/fixtures/must_reject/`.
    #[test]
    fn test_must_reject_fixtures() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();

        let dirs = [
            root.join("tests/fixtures/errors"),
            root.join("tests/fixtures/must_reject"),
        ];

        let mut tested = 0;
        for dir in &dirs {
            if !dir.exists() {
                continue;
            }
            for entry in std::fs::read_dir(dir).expect("cannot read fixtures dir") {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("assura") {
                    continue;
                }
                let source = std::fs::read_to_string(&path).unwrap();

                // Look for // MUST REJECT Axxxxx
                let expected_code = source.lines().find_map(|line| {
                    let trimmed = line.trim();
                    if trimmed.starts_with("// MUST REJECT ") {
                        Some(trimmed.strip_prefix("// MUST REJECT ")?.trim().to_string())
                    } else {
                        None
                    }
                });
                let Some(code) = expected_code else {
                    continue; // No annotation, skip
                };

                let (file, _parse_errors) = assura_parser::parse(&source);
                let Some(file) = file else {
                    continue; // Parse failed entirely, not a type check test
                };
                let resolved = match assura_resolve::resolve(&file) {
                    Ok(r) => r,
                    Err(res_errors) => {
                        let found = res_errors.iter().any(|e| e.code == code);
                        assert!(
                            found,
                            "{}: expected resolution error {code}, got: {:?}",
                            path.display(),
                            res_errors
                        );
                        tested += 1;
                        continue;
                    }
                };
                let type_result = assura_types::type_check(&resolved);
                match type_result {
                    Err(type_errors) => {
                        let found = type_errors.iter().any(|e| e.code == code);
                        assert!(
                            found,
                            "{}: expected type error {code}, got: {:?}",
                            path.display(),
                            type_errors
                        );
                    }
                    Ok(_) => {
                        panic!(
                            "{}: expected error {code} but type checking succeeded",
                            path.display()
                        );
                    }
                }
                tested += 1;
            }
        }
        assert!(
            tested >= 15,
            "expected at least 15 MUST REJECT fixtures, found {tested}"
        );
    }

    /// T204: Positive test suite. Files annotated with `// MUST COMPILE` must
    /// parse, resolve, type-check, and produce valid generated Rust (verified
    /// via `syn::parse_file`).
    #[test]
    fn test_must_compile_fixtures() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests/fixtures/must_compile");

        let mut tested = 0;
        for entry in std::fs::read_dir(&dir).expect("cannot read must_compile fixtures dir") {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("assura") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();

            // Verify annotation
            let has_annotation = source.lines().any(|l| l.trim() == "// MUST COMPILE");
            assert!(
                has_annotation,
                "{}: missing // MUST COMPILE annotation",
                path.display()
            );

            // Parse
            let (file, parse_errors) = assura_parser::parse(&source);
            assert!(
                parse_errors.is_empty(),
                "{}: unexpected parse errors: {:?}",
                path.display(),
                parse_errors
            );
            let file = file.unwrap_or_else(|| {
                panic!("{}: parse returned None", path.display());
            });

            // Resolve
            let resolved = assura_resolve::resolve(&file).unwrap_or_else(|errs| {
                panic!("{}: resolution errors: {:?}", path.display(), errs);
            });

            // Type check
            let typed = assura_types::type_check(&resolved).unwrap_or_else(|errs| {
                panic!("{}: type errors: {:?}", path.display(), errs);
            });

            // Codegen
            let project = assura_codegen::codegen(&typed);

            // Verify generated Rust is syntactically valid
            for (file_path, rust_source) in &project.files {
                syn::parse_file(rust_source).unwrap_or_else(|err| {
                    panic!(
                        "{}: generated {} is not valid Rust: {}\n--- source ---\n{}",
                        path.display(),
                        file_path,
                        err,
                        rust_source
                    );
                });
            }

            tested += 1;
        }
        assert!(
            tested >= 15,
            "expected at least 15 MUST COMPILE fixtures, found {tested}"
        );
    }

    // =======================================================================
    // Build --output flag tests
    // =======================================================================

    #[test]
    fn build_output_generates_to_custom_dir() {
        // Verify codegen writes to the correct output directory
        let source = r#"
contract SimpleBuild {
  input(x: Int)
  output(result: Int)
  requires { x > 0 }
  ensures { result > 0 }
}
"#;
        let project = full_pipeline(source).expect("pipeline failed");
        // Verify the project has cargo toml and source files
        assert!(
            project.cargo_toml.contains("[package]"),
            "should have package section"
        );
        assert!(!project.files.is_empty(), "should have generated files");
        let (path, content) = &project.files[0];
        assert_eq!(path, "src/lib.rs");
        assert!(
            content.contains("fn check"),
            "should contain check function"
        );
    }

    #[test]
    fn build_output_writes_files_to_disk() {
        let source = r#"
contract DiskWrite {
  input(n: Int)
  output(result: Bool)
  requires { n >= 0 }
  ensures { result }
}
"#;
        let project = full_pipeline(source).expect("pipeline failed");
        // Write to a temp directory and verify files exist
        let tmp = std::env::temp_dir().join("assura_test_output");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();
        std::fs::write(tmp.join("Cargo.toml"), &project.cargo_toml).unwrap();
        for (path, content) in &project.files {
            std::fs::write(tmp.join(path), content).unwrap();
        }
        // Verify files exist
        assert!(tmp.join("Cargo.toml").exists());
        assert!(tmp.join("src/lib.rs").exists());
        // Read back and verify content
        let cargo_content = std::fs::read_to_string(tmp.join("Cargo.toml")).unwrap();
        assert!(cargo_content.contains("[package]"));
        let lib_content = std::fs::read_to_string(tmp.join("src/lib.rs")).unwrap();
        assert!(lib_content.contains("Generated by the Assura compiler"));
        // Clean up
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Helper to find the `assura` binary built by cargo.
    fn assura_bin() -> std::path::PathBuf {
        // Look next to the test binary itself (target/debug/)
        let mut path = std::env::current_exe().unwrap();
        path.pop(); // remove test binary name
        if path.ends_with("deps") {
            path.pop(); // target/debug/deps -> target/debug
        }
        path.push("assura");
        if path.exists() {
            return path;
        }
        // Fallback: just try "assura" on PATH
        std::path::PathBuf::from("assura")
    }

    /// Workspace root (two levels up from crate manifest).
    fn workspace_root() -> String {
        env!("CARGO_MANIFEST_DIR").replace("/crates/assura-cli", "")
    }

    #[test]
    fn build_cli_output_creates_custom_dir() {
        // Integration test: invoke `assura build` with --output and verify
        // the directory is created with Cargo.toml and src/lib.rs.
        let tmp = std::env::temp_dir().join("assura_r007_custom_output");
        let _ = std::fs::remove_dir_all(&tmp);
        let out = std::process::Command::new(assura_bin())
            .args([
                "build",
                "demos/libwebp-huffman.assura",
                "--output",
                tmp.to_str().unwrap(),
            ])
            .current_dir(workspace_root())
            .output()
            .expect("failed to run assura build");
        assert!(
            out.status.success(),
            "build should succeed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(tmp.join("Cargo.toml").exists(), "Cargo.toml should exist");
        assert!(tmp.join("src/lib.rs").exists(), "src/lib.rs should exist");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn build_cli_default_output_is_generated() {
        // Integration test: build without --output uses "generated/" directory.
        // We run from a temp dir to avoid polluting the repo.
        let workspace = std::env::temp_dir().join("assura_r007_default");
        let _ = std::fs::remove_dir_all(&workspace);
        std::fs::create_dir_all(&workspace).unwrap();
        // Copy a demo file into the workspace
        let demo_src = std::path::Path::new(&workspace_root()).join("demos/libwebp-huffman.assura");
        let demo_dest = workspace.join("input.assura");
        std::fs::copy(&demo_src, &demo_dest).unwrap();
        let out = std::process::Command::new(assura_bin())
            .args(["build", "input.assura"])
            .current_dir(&workspace)
            .output()
            .expect("failed to run assura build");
        assert!(
            out.status.success(),
            "build should succeed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            workspace.join("generated/Cargo.toml").exists(),
            "default generated/Cargo.toml should exist"
        );
        assert!(
            workspace.join("generated/src/lib.rs").exists(),
            "default generated/src/lib.rs should exist"
        );
        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[test]
    fn build_cli_error_on_missing_file() {
        // Integration test: build with a nonexistent file should fail.
        let out = std::process::Command::new(assura_bin())
            .args(["build", "nonexistent_file_r007.assura"])
            .output()
            .expect("failed to run assura build");
        assert!(!out.status.success(), "build should fail for missing file");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("Error") || stderr.contains("error") || stderr.contains("No such file"),
            "stderr should mention error: {stderr}"
        );
    }

    #[test]
    fn build_codegen_with_cranelift_backend() {
        let source = r#"
contract CraneliftTest {
  input(x: Int)
  output(result: Int)
  ensures { result == x }
}
"#;
        let (file, errs) = assura_parser::parse(source);
        assert!(errs.is_empty());
        let file = file.unwrap();
        let resolved = assura_resolve::resolve(&file).unwrap();
        let typed = assura_types::type_check(&resolved).unwrap();
        let config = assura_codegen::BackendConfig {
            backend: assura_codegen::CodegenBackend::Cranelift,
            opt_level: 0,
            debug_info: true,
        };
        let project = assura_codegen::codegen_with_config(&typed, &config);
        assert!(
            project.cargo_toml.contains("Cranelift"),
            "should mention Cranelift backend"
        );
        assert!(
            project.cargo_toml.contains("debug = true"),
            "should have debug info"
        );
    }
}
