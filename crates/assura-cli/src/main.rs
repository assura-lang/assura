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

// ---------------------------------------------------------------------------
// Output mode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    Human,
    Json,
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
    let is_explain = non_flag_args.first().is_some_and(|a| a.as_str() == "explain");

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
// `assura check <file> [--json|--human]`
// ---------------------------------------------------------------------------

fn run_check(args: &[String]) {
    let output_mode = if args.contains(&"--json".to_string()) {
        OutputMode::Json
    } else {
        OutputMode::Human
    };

    // The file is the first non-flag arg after "check"
    let filename = args
        .iter()
        .skip(1) // skip binary name
        .filter(|a| !a.starts_with('-'))
        .nth(1) // skip "check" itself
        .unwrap_or_else(|| {
            eprintln!("Usage: assura check <file.assura> [--json|--human]");
            process::exit(2);
        });

    let source = fs::read_to_string(filename).unwrap_or_else(|e| {
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

    let mut diagnostics: Vec<DiagnosticJson> = Vec::new();
    let mut has_errors = false;

    // --- Lex ---
    let lex = Token::lexer(&source);
    let mut tokens: Vec<(Token, std::ops::Range<usize>)> = Vec::new();

    for (tok, span) in lex.spanned() {
        match tok {
            Ok(t) => tokens.push((t, span)),
            Err(()) => {
                has_errors = true;
                diagnostics.push(DiagnosticJson {
                    code: "A01001".to_string(),
                    message: format!("unexpected character: {:?}", &source[span.clone()]),
                    file: filename.clone(),
                    start: span.start,
                    end: span.end,
                    severity: "error".to_string(),
                    secondary: None,
                });
            }
        }
    }

    // If lex errors, still try to continue for maximum diagnostics,
    // but we'll exit 1 at the end.
    if has_errors && output_mode == OutputMode::Human {
        report_diagnostics_human(&diagnostics, filename, &source);
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
            .map(|t| format!("{t}"))
            .unwrap_or_else(|| "end of file".to_string());
        let expected: Vec<String> = e
            .expected()
            .map(|ex| match ex {
                Some(t) => format!("{t}"),
                None => "end of input".to_string(),
            })
            .collect();

        let msg = if expected.is_empty() {
            format!("unexpected {found}")
        } else {
            format!("expected {}, found {found}", expected.join(" or "))
        };

        diagnostics.push(DiagnosticJson {
            code: "A01002".to_string(),
            message: msg,
            file: filename.clone(),
            start: span.start,
            end: span.end,
            severity: "error".to_string(),
            secondary: None,
        });
    }

    // --- Resolve (only if we have a parsed file) ---
    let resolved = if let Some(ref file) = file {
        match assura_resolve::resolve(file) {
            Ok(r) => Some(r),
            Err(errs) => {
                has_errors = true;
                for e in &errs {
                    diagnostics.push(DiagnosticJson {
                        code: e.code.to_string(),
                        message: e.message.clone(),
                        file: filename.clone(),
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
    if let Some(ref resolved) = resolved
        && let Err(errs) = assura_types::type_check(resolved)
    {
        has_errors = true;
        for e in &errs {
            diagnostics.push(DiagnosticJson {
                code: e.code.clone(),
                message: e.message.clone(),
                file: filename.clone(),
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
    }

    // --- Report ---
    match output_mode {
        OutputMode::Json => {
            println!("{}", serde_json::to_string_pretty(&diagnostics).unwrap());
        }
        OutputMode::Human => {
            // Lex errors already reported above; report the rest.
            let non_lex: Vec<_> = diagnostics
                .iter()
                .filter(|d| d.code != "A01001")
                .cloned()
                .collect();
            report_diagnostics_human(&non_lex, filename, &source);

            if !has_errors {
                eprintln!("{filename}: check passed (no errors)");
            } else {
                eprintln!("{filename}: {} error(s) found", diagnostics.len());
            }
        }
    }

    process::exit(if has_errors { 1 } else { 0 });
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
    // The file is the first non-flag arg after "build"
    let filename = args
        .iter()
        .skip(1) // skip binary name
        .filter(|a| !a.starts_with('-'))
        .nth(1) // skip "build" itself
        .unwrap_or_else(|| {
            eprintln!("Usage: assura build <file.assura>");
            process::exit(2);
        });

    let source = fs::read_to_string(filename).unwrap_or_else(|e| {
        eprintln!("Error: {filename}: {e}");
        process::exit(2);
    });

    // --- Lex ---
    let lex = Token::lexer(&source);
    let mut tokens: Vec<(Token, std::ops::Range<usize>)> = Vec::new();
    let mut lex_errors = Vec::new();

    for (tok, span) in lex.spanned() {
        match tok {
            Ok(t) => tokens.push((t, span)),
            Err(()) => lex_errors.push(span),
        }
    }

    for span in &lex_errors {
        let snippet = &source[span.clone()];
        Report::build(ReportKind::Error, filename.as_str(), span.start)
            .with_message(format!("unexpected character: {snippet:?}"))
            .with_label(
                Label::new((filename.as_str(), span.clone()))
                    .with_message("invalid token")
                    .with_color(Color::Red),
            )
            .finish()
            .eprint((filename.as_str(), Source::from(&source)))
            .ok();
    }

    if !lex_errors.is_empty() {
        process::exit(1);
    }

    // --- Parse ---
    let len = source.len();
    let token_stream = Stream::from_iter(len..len + 1, tokens.into_iter());
    let (file, parse_errors) = parser::source_file().parse_recovery(token_stream);

    for e in &parse_errors {
        let span = e.span();
        let found = e
            .found()
            .map(|t| format!("{t}"))
            .unwrap_or_else(|| "end of file".to_string());
        let expected: Vec<String> = e
            .expected()
            .map(|ex| match ex {
                Some(t) => format!("{t}"),
                None => "end of input".to_string(),
            })
            .collect();
        let msg = if expected.is_empty() {
            format!("unexpected {found}")
        } else {
            format!("expected {}, found {found}", expected.join(" or "))
        };
        Report::build(ReportKind::Error, filename.as_str(), span.start)
            .with_message(&msg)
            .with_label(
                Label::new((filename.as_str(), span.clone()))
                    .with_message(&msg)
                    .with_color(Color::Red),
            )
            .finish()
            .eprint((filename.as_str(), Source::from(&source)))
            .ok();
    }

    let Some(file) = file else {
        eprintln!("{filename}: parse failed");
        process::exit(1);
    };

    if !parse_errors.is_empty() {
        process::exit(1);
    }

    // --- Resolve ---
    let resolved = match assura_resolve::resolve(&file) {
        Ok(r) => r,
        Err(errs) => {
            for e in &errs {
                let mut builder = Report::build(ReportKind::Error, filename.as_str(), e.span.start)
                    .with_message(format!("[{}] {}", e.code, e.message))
                    .with_label(
                        Label::new((filename.as_str(), e.span.clone()))
                            .with_message(&e.message)
                            .with_color(Color::Red),
                    );
                if let Some((ref sec_span, ref sec_msg)) = e.secondary {
                    builder = builder.with_label(
                        Label::new((filename.as_str(), sec_span.clone()))
                            .with_message(sec_msg)
                            .with_color(Color::Blue),
                    );
                }
                builder
                    .finish()
                    .eprint((filename.as_str(), Source::from(&source)))
                    .ok();
            }
            eprintln!("{filename}: {} resolution error(s)", errs.len());
            process::exit(1);
        }
    };

    // --- Type check ---
    let typed = match assura_types::type_check(&resolved) {
        Ok(t) => t,
        Err(errs) => {
            for e in &errs {
                let mut builder = Report::build(ReportKind::Error, filename.as_str(), e.span.start)
                    .with_message(format!("[{}] {}", e.code, e.message))
                    .with_label(
                        Label::new((filename.as_str(), e.span.clone()))
                            .with_message(&e.message)
                            .with_color(Color::Red),
                    );
                if let Some((ref sec_span, ref sec_msg)) = e.secondary {
                    builder = builder.with_label(
                        Label::new((filename.as_str(), sec_span.clone()))
                            .with_message(sec_msg)
                            .with_color(Color::Blue),
                    );
                }
                builder
                    .finish()
                    .eprint((filename.as_str(), Source::from(&source)))
                    .ok();
            }
            eprintln!("{filename}: {} type error(s)", errs.len());
            process::exit(1);
        }
    };

    // --- Codegen ---
    let project = assura_codegen::codegen(&typed);

    // --- Write to generated/ ---
    let out_dir = Path::new("generated");
    fs::create_dir_all(out_dir).unwrap_or_else(|e| {
        eprintln!("Error: cannot create generated/ directory: {e}");
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

    println!("OK  {filename} -> generated/");
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
            eprintln!("       assura build <file.assura>");
            eprintln!("       assura init <project-name>");
            process::exit(2);
        });

    let source = fs::read_to_string(filename).unwrap_or_else(|e| {
        eprintln!("Error: {filename}: {e}");
        process::exit(2);
    });

    // --- Lex ---
    let lex = Token::lexer(&source);
    let mut tokens: Vec<(Token, std::ops::Range<usize>)> = Vec::new();
    let mut lex_errors = Vec::new();

    for (tok, span) in lex.spanned() {
        match tok {
            Ok(t) => tokens.push((t, span)),
            Err(()) => lex_errors.push(span),
        }
    }

    if show_tokens {
        for (tok, span) in &tokens {
            let line = source[..span.start].lines().count();
            let col = span.start - source[..span.start].rfind('\n').map_or(0, |p| p + 1) + 1;
            println!("{line}:{col}  {tok:?}");
        }
        return;
    }

    for span in &lex_errors {
        let snippet = &source[span.clone()];
        Report::build(ReportKind::Error, filename.as_str(), span.start)
            .with_message(format!("unexpected character: {snippet:?}"))
            .with_label(
                Label::new((filename.as_str(), span.clone()))
                    .with_message("invalid token")
                    .with_color(Color::Red),
            )
            .finish()
            .eprint((filename.as_str(), Source::from(&source)))
            .ok();
    }

    if !lex_errors.is_empty() {
        process::exit(1);
    }

    // --- Parse ---
    let len = source.len();
    let token_stream = Stream::from_iter(len..len + 1, tokens.into_iter());

    let (file, parse_errors) = parser::source_file().parse_recovery(token_stream);

    for e in &parse_errors {
        let span = e.span();
        let found = e
            .found()
            .map(|t| format!("{t}"))
            .unwrap_or_else(|| "end of file".to_string());
        let expected: Vec<String> = e
            .expected()
            .map(|ex| match ex {
                Some(t) => format!("{t}"),
                None => "end of input".to_string(),
            })
            .collect();

        let msg = if expected.is_empty() {
            format!("unexpected {found}")
        } else {
            format!("expected {}, found {found}", expected.join(" or "))
        };

        Report::build(ReportKind::Error, filename.as_str(), span.start)
            .with_message(&msg)
            .with_label(
                Label::new((filename.as_str(), span.clone()))
                    .with_message(&msg)
                    .with_color(Color::Red),
            )
            .finish()
            .eprint((filename.as_str(), Source::from(&source)))
            .ok();
    }

    let Some(file) = file else {
        eprintln!("{filename}: parse failed");
        process::exit(1);
    };

    if !parse_errors.is_empty() {
        process::exit(1);
    }

    // --- Resolve ---
    let resolved = match assura_resolve::resolve(&file) {
        Ok(r) => r,
        Err(errs) => {
            for e in &errs {
                let mut builder = Report::build(ReportKind::Error, filename.as_str(), e.span.start)
                    .with_message(format!("[{}] {}", e.code, e.message))
                    .with_label(
                        Label::new((filename.as_str(), e.span.clone()))
                            .with_message(&e.message)
                            .with_color(Color::Red),
                    );
                if let Some((ref sec_span, ref sec_msg)) = e.secondary {
                    builder = builder.with_label(
                        Label::new((filename.as_str(), sec_span.clone()))
                            .with_message(sec_msg)
                            .with_color(Color::Blue),
                    );
                }
                builder
                    .finish()
                    .eprint((filename.as_str(), Source::from(&source)))
                    .ok();
            }
            eprintln!("{filename}: {} resolution error(s)", errs.len());
            process::exit(1);
        }
    };

    // --- Type check ---
    let typed = match assura_types::type_check(&resolved) {
        Ok(t) => t,
        Err(errs) => {
            for e in &errs {
                let mut builder = Report::build(ReportKind::Error, filename.as_str(), e.span.start)
                    .with_message(format!("[{}] {}", e.code, e.message))
                    .with_label(
                        Label::new((filename.as_str(), e.span.clone()))
                            .with_message(&e.message)
                            .with_color(Color::Red),
                    );
                if let Some((ref sec_span, ref sec_msg)) = e.secondary {
                    builder = builder.with_label(
                        Label::new((filename.as_str(), sec_span.clone()))
                            .with_message(sec_msg)
                            .with_color(Color::Blue),
                    );
                }
                builder
                    .finish()
                    .eprint((filename.as_str(), Source::from(&source)))
                    .ok();
            }
            eprintln!("{filename}: {} type error(s)", errs.len());
            process::exit(1);
        }
    };

    // --- Output ---
    if show_ast {
        print_ast(&file);
    } else {
        print_summary(filename, &file, &resolved.symbols, &typed.type_env);
    }
}

fn print_summary(
    filename: &str,
    file: &SourceFile,
    symbols: &assura_resolve::SymbolTable,
    type_env: &assura_types::TypeEnv,
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
        Decl::Block { kind, name, body } => {
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
        Expr::Raw(tokens) => tokens.join(" "),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max])
    } else {
        s.to_string()
    }
}
