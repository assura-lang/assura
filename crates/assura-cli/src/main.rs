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
    let is_explain = non_flag_args
        .first()
        .is_some_and(|a| a.as_str() == "explain");

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
        report_diagnostics_human(&diagnostics, &filename, &source);
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
    let typed = if let Some(ref resolved) = resolved {
        match assura_types::type_check(resolved) {
            Ok(t) => Some(t),
            Err(errs) => {
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
                None
            }
        }
    } else {
        None
    };

    // --- Verify (only if type check succeeded and layer >= 1) ---
    let verification_results = if layer >= 1 {
        if let Some(ref typed) = typed {
            assura_smt::verify(typed)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

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

            let output = serde_json::json!({
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

            // Print verification results
            if !verification_results.is_empty() {
                eprintln!();
                eprintln!(
                    "Verification layer {layer} ({} clause(s)):",
                    verification_results.len()
                );
                for vr in &verification_results {
                    match vr {
                        assura_smt::VerificationResult::Verified { clause_desc } => {
                            eprintln!("  VERIFIED        {clause_desc}");
                        }
                        assura_smt::VerificationResult::Counterexample {
                            clause_desc,
                            model,
                            ..
                        } => {
                            eprintln!("  COUNTEREXAMPLE  {clause_desc}");
                            eprintln!("    model: {model}");
                        }
                        assura_smt::VerificationResult::Timeout { clause_desc } => {
                            eprintln!("  TIMEOUT         {clause_desc}");
                        }
                        assura_smt::VerificationResult::Unknown {
                            clause_desc,
                            reason,
                        } => {
                            eprintln!("  UNKNOWN         {clause_desc} ({reason})");
                        }
                    }
                }
            } else if layer == 0 {
                eprintln!();
                eprintln!("Verification skipped (--layer 0: structural checks only)");
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

    // --- Verify ---
    let verification_results = assura_smt::verify(&typed);
    if !verification_results.is_empty() {
        eprintln!();
        eprintln!("Verification ({} clause(s)):", verification_results.len());
        for vr in &verification_results {
            match vr {
                assura_smt::VerificationResult::Verified { clause_desc } => {
                    eprintln!("  VERIFIED    {clause_desc}");
                }
                assura_smt::VerificationResult::Counterexample {
                    clause_desc, model, ..
                } => {
                    eprintln!("  COUNTEREXAMPLE  {clause_desc}");
                    eprintln!("    model: {model}");
                }
                assura_smt::VerificationResult::Timeout { clause_desc } => {
                    eprintln!("  TIMEOUT     {clause_desc}");
                }
                assura_smt::VerificationResult::Unknown {
                    clause_desc,
                    reason,
                } => {
                    eprintln!("  UNKNOWN     {clause_desc} ({reason})");
                }
            }
        }
    }

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
            eprintln!("       assura build <file.assura>");
            eprintln!("       assura init <project-name>");
            eprintln!("       assura explain <error-code>");
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

    // --- Verify ---
    let verification_results = assura_smt::verify(&typed);

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
        println!("    verify:    OK (no verifiable clauses)");
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
        Expr::Ghost(inner) => format!("ghost {{ {} }}", expr_to_string(inner)),
        Expr::Apply { lemma_name, args } => {
            let args_s: Vec<String> = args.iter().map(expr_to_string).collect();
            format!("apply {lemma_name}({})", args_s.join(", "))
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
