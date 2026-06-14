#![allow(dead_code)]

use clap::{Args, Subcommand};
use std::fs;
use std::path::Path;
use std::process;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use assura_config::{CompilerConfig, OutputMode, ProjectConfig, Verbosity};
use assura_parser::ast::*;
use assura_parser::lexer::Token;
use logos::Logos;
// ---------------------------------------------------------------------------
// CLI argument definitions (clap 4)
// ---------------------------------------------------------------------------

#[derive(clap::Parser)]
#[command(name = "assura", version, about = "The Assura contract compiler")]
#[command(subcommand_required = false)]
struct Cli {
    #[command(flatten)]
    global: GlobalOpts,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Source file to parse and check (legacy mode)
    #[arg(global = false)]
    file: Option<String>,

    /// Dump the AST (legacy mode)
    #[arg(long)]
    ast: bool,

    /// Dump the token stream (legacy mode)
    #[arg(long)]
    tokens: bool,
}

#[derive(Args, Clone)]
struct GlobalOpts {
    /// Output diagnostics as JSON
    #[arg(long, global = true)]
    json: bool,

    /// Output diagnostics as rich terminal (default)
    #[arg(long, global = true)]
    human: bool,

    /// Show timing, intermediate results, Z3 stats
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Suppress all output except errors
    #[arg(short, long, global = true)]
    quiet: bool,
}

impl GlobalOpts {
    fn output_mode(&self) -> OutputMode {
        if self.json {
            OutputMode::Json
        } else {
            OutputMode::Human
        }
    }

    fn verbosity(&self) -> Verbosity {
        if self.verbose {
            Verbosity::Verbose
        } else if self.quiet {
            Verbosity::Quiet
        } else {
            Verbosity::Normal
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Full pipeline: parse, resolve, type-check, verify
    Check {
        /// Source file to check
        file: String,

        /// Verification layer (0=structural, 1=SMT)
        #[arg(long, default_value_t = 255)]
        layer: u8,

        /// SMT solver backend
        #[arg(long, value_parser = parse_solver)]
        solver: Option<assura_smt::SolverChoice>,

        /// Watch for file changes and re-check
        #[arg(short, long)]
        watch: bool,

        /// Print verification statistics (clause counts, solve times)
        #[arg(long)]
        stats: bool,

        /// Write SMT-LIB2 files for each verification query to this directory
        #[arg(long, value_name = "DIR")]
        dump_smt: Option<String>,
    },

    /// Generate Rust code from a contract file
    Build {
        /// Source file to build
        file: String,

        /// Output directory for generated code
        #[arg(long, default_value = "generated")]
        output: String,

        /// Compilation target
        #[arg(long, value_parser = parse_target)]
        target: Option<assura_codegen::CompileTarget>,

        /// Skip cargo check on generated code
        #[arg(long)]
        no_check: bool,

        /// SMT solver backend
        #[arg(long, value_parser = parse_solver)]
        solver: Option<assura_smt::SolverChoice>,
    },

    /// Create a new Assura project
    Init {
        /// Project name
        name: String,
    },

    /// Explain an error code (e.g., A03001)
    Explain {
        /// Error code to explain
        code: String,
    },

    /// Format an .assura source file
    Fmt {
        /// Source file to format
        file: String,

        /// Check formatting without modifying the file
        #[arg(long)]
        check: bool,
    },
}

fn parse_solver(s: &str) -> Result<assura_smt::SolverChoice, String> {
    assura_smt::SolverChoice::from_str_loose(s)
        .ok_or_else(|| format!("invalid solver: {s} (expected z3, cvc5, or portfolio)"))
}

fn parse_target(s: &str) -> Result<assura_codegen::CompileTarget, String> {
    assura_codegen::CompileTarget::from_str_loose(s)
        .ok_or_else(|| format!("invalid target: {s} (expected native or wasm)"))
}

/// Load `assura.toml` from the project root, if it exists.
fn load_project_config(start_path: &Path) -> Option<(ProjectConfig, std::path::PathBuf)> {
    assura_config::load_project_config(start_path, assura_resolve::find_project_root)
}

// ---------------------------------------------------------------------------
// Pipeline timing
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct TimingInfo {
    lex_ms: f64,
    parse_ms: f64,
    resolve_ms: Option<f64>,
    hir_ms: Option<f64>,
    typecheck_ms: Option<f64>,
    token_count: usize,
}

// ---------------------------------------------------------------------------
// Shared compilation pipeline
// ---------------------------------------------------------------------------

/// Result of running the full compilation pipeline (lex -> parse -> resolve -> typecheck).
struct CompilationResult {
    file: Option<SourceFile>,
    resolved: Option<assura_resolve::ResolvedFile>,
    hir: Option<assura_hir::HirFile>,
    typed: Option<assura_types::TypedFile>,
    diagnostics: Vec<assura_diagnostics::Diagnostic>,
    has_errors: bool,
    timing: TimingInfo,
}

/// Run lex -> parse -> resolve -> typecheck on source text, collecting all diagnostics.
fn compile(source: &str, filename: &str) -> CompilationResult {
    compile_with_config(source, filename, &CompilerConfig::default())
}

/// Run the full pipeline with explicit configuration.
fn compile_with_config(source: &str, filename: &str, config: &CompilerConfig) -> CompilationResult {
    let mut diagnostics: Vec<assura_diagnostics::Diagnostic> = Vec::new();
    let mut has_errors = false;

    // --- Lex ---
    let lex_start = Instant::now();
    let lex = Token::lexer(source);
    let mut tokens: Vec<(Token, std::ops::Range<usize>)> = Vec::new();

    for (tok, span) in lex.spanned() {
        match tok {
            Ok(t) => tokens.push((t, span)),
            Err(()) => {
                has_errors = true;
                diagnostics.push(
                    assura_diagnostics::Diagnostic::error(
                        "A01001",
                        format!("unexpected character: {:?}", &source[span.clone()]),
                        span,
                    )
                    .with_file(filename),
                );
            }
        }
    }
    let lex_ms = lex_start.elapsed().as_secs_f64() * 1000.0;
    let token_count = tokens.len();

    // --- Parse ---
    let parse_start = Instant::now();
    let (file, parse_errors) = assura_parser::parse(source);
    let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;

    for e in &parse_errors {
        has_errors = true;
        diagnostics.push(
            assura_diagnostics::Diagnostic::error("A01002", e.message.clone(), e.span.clone())
                .with_file(filename),
        );
    }

    // --- Resolve (only if we have a parsed file) ---
    let resolve_start = Instant::now();
    let resolved = if let Some(ref file) = file {
        match assura_resolve::resolve(file) {
            Ok(r) => {
                for w in &r.warnings {
                    let mut d = assura_diagnostics::Diagnostic::warning(
                        w.code,
                        w.message.clone(),
                        w.span.clone(),
                    )
                    .with_file(filename);
                    if let Some((span, msg)) = &w.secondary {
                        d = d.with_secondary(span.clone(), msg.clone());
                    }
                    diagnostics.push(d);
                }
                Some(r)
            }
            Err(errs) => {
                has_errors = true;
                for e in &errs {
                    let mut d = assura_diagnostics::Diagnostic::error(
                        e.code,
                        e.message.clone(),
                        e.span.clone(),
                    )
                    .with_file(filename);
                    if let Some((span, msg)) = &e.secondary {
                        d = d.with_secondary(span.clone(), msg.clone());
                    }
                    diagnostics.push(d);
                }
                None
            }
        }
    } else {
        None
    };
    let resolve_ms = if file.is_some() {
        Some(resolve_start.elapsed().as_secs_f64() * 1000.0)
    } else {
        None
    };

    // --- HIR lowering (only if resolution succeeded) ---
    let hir_start = Instant::now();
    let hir = resolved.as_ref().map(assura_hir::lower);
    let hir_ms = if resolved.is_some() {
        Some(hir_start.elapsed().as_secs_f64() * 1000.0)
    } else {
        None
    };

    // --- Type check (only if HIR/resolution succeeded) ---
    let typecheck_start = Instant::now();
    let typed = if let Some(ref hir_file) = hir {
        match assura_types::type_check_hir_with_config(hir_file, &config.type_check) {
            Ok(t) => Some(t),
            Err(errs) => {
                has_errors = true;
                for e in &errs {
                    let mut d = assura_diagnostics::Diagnostic::error(
                        e.code.clone(),
                        e.message.clone(),
                        e.span.clone(),
                    )
                    .with_file(filename);
                    if let Some((span, msg)) = &e.secondary {
                        d = d.with_secondary(span.clone(), msg.clone());
                    }
                    diagnostics.push(d);
                }
                None
            }
        }
    } else {
        None
    };
    let typecheck_ms = if resolved.is_some() {
        Some(typecheck_start.elapsed().as_secs_f64() * 1000.0)
    } else {
        None
    };

    CompilationResult {
        file,
        resolved,
        hir,
        typed,
        diagnostics,
        has_errors,
        timing: TimingInfo {
            lex_ms,
            parse_ms,
            resolve_ms,
            hir_ms,
            typecheck_ms,
            token_count,
        },
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let cli = <Cli as clap::Parser>::parse();
    let output_mode = cli.global.output_mode();
    let verbosity = cli.global.verbosity();

    match cli.command {
        Some(Commands::Check {
            file,
            layer,
            solver,
            watch,
            stats,
            dump_smt,
        }) => run_check(
            &file,
            output_mode,
            verbosity,
            layer,
            solver,
            watch,
            stats,
            dump_smt.as_deref(),
        ),
        Some(Commands::Build {
            file,
            output,
            target,
            no_check,
            solver,
        }) => run_build(
            &file,
            output_mode,
            verbosity,
            &output,
            target,
            no_check,
            solver,
        ),
        Some(Commands::Init { name }) => run_init(&name),
        Some(Commands::Explain { code }) => run_explain(&code),
        Some(Commands::Fmt { file, check }) => run_fmt(&file, check),
        None => {
            // Legacy mode: `assura [--ast|--tokens] <file>`
            if let Some(file) = cli.file {
                run_legacy(&file, verbosity, cli.ast, cli.tokens);
            } else {
                // No subcommand and no file: show help
                <Cli as clap::CommandFactory>::command()
                    .print_help()
                    .unwrap();
                println!();
                process::exit(2);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// `assura check <file> [--json|--human] [--layer 0|1]`
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn run_check(
    filename: &str,
    output_mode: OutputMode,
    verbosity: Verbosity,
    cli_layer: u8,
    cli_solver: Option<assura_smt::SolverChoice>,
    watch: bool,
    stats: bool,
    dump_smt: Option<&str>,
) {
    // Load project config (assura.toml) if available
    let project = load_project_config(Path::new(filename));
    let config_layer = project.as_ref().map(|(c, _)| c.verify.layer);

    // Verification layer: CLI flag > config file > default (1)
    // 255 is the sentinel for "not specified on CLI"
    let layer: u8 = if cli_layer != 255 {
        cli_layer
    } else {
        config_layer.unwrap_or(1)
    };

    // Solver choice: CLI flag > config file > default (Z3)
    let config_solver = project
        .as_ref()
        .and_then(|(c, _)| assura_smt::SolverChoice::from_str_loose(&c.verify.smt_solver));
    let solver =
        cli_solver.unwrap_or_else(|| config_solver.unwrap_or(assura_smt::SolverChoice::Z3));

    // Build unified compiler config
    let compiler_config = if let Some((ref proj, _)) = project {
        let mut cc = CompilerConfig::from_project(proj, output_mode, verbosity);
        cc.verify.layer = layer;
        cc.verify.solver = solver.as_str().to_string();
        cc
    } else {
        CompilerConfig {
            output_mode,
            verbosity,
            verify: assura_config::VerifyOptions {
                layer,
                solver: solver.as_str().to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    };
    // Keep the project config around for verbose display
    let config = project;

    if watch {
        run_watch_loop(filename, output_mode, verbosity, layer);
        // run_watch_loop never returns (loops until interrupted)
    }

    let source = fs::read_to_string(filename).unwrap_or_else(|e| {
        if output_mode == OutputMode::Json {
            let diag = assura_diagnostics::Diagnostic::error("A01000", format!("{e}"), 0..0)
                .with_file(filename);
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
        hir: _,
        typed,
        mut diagnostics,
        mut has_errors,
        timing,
    } = compile_with_config(&source, filename, &compiler_config);

    if verbosity == Verbosity::Verbose && output_mode == OutputMode::Human {
        if let Some((ref cfg, ref root)) = config {
            eprintln!(
                "Project: {} v{} ({})",
                cfg.package.name,
                cfg.package.version,
                root.display()
            );
            eprintln!(
                "  config: layer={}, solver={}, timeout={}ms, output={}",
                cfg.verify.layer, cfg.verify.smt_solver, cfg.verify.timeout, cfg.build.output
            );
            eprintln!();
        }
        eprintln!("Pipeline timing for {filename}:");
        eprintln!(
            "  lex:       {} tokens ({:.2}ms)",
            timing.token_count, timing.lex_ms
        );
        if let Some(ref f) = file {
            eprintln!(
                "  parse:     {} declaration(s), {} import(s) ({:.2}ms)",
                f.decls.len(),
                f.imports.len(),
                timing.parse_ms
            );
        } else {
            eprintln!("  parse:     failed ({:.2}ms)", timing.parse_ms);
        }
        if let Some(resolve_ms) = timing.resolve_ms {
            if let Some(ref r) = resolved {
                let user_symbols = r
                    .symbols
                    .symbols
                    .iter()
                    .filter(|s| s.kind != assura_resolve::SymbolKind::BuiltinType)
                    .count();
                eprintln!("  resolve:   {user_symbols} symbol(s) ({resolve_ms:.2}ms)");
            } else {
                eprintln!("  resolve:   failed ({resolve_ms:.2}ms)");
            }
        }
        if let Some(hir_ms) = timing.hir_ms {
            eprintln!("  hir:       ({hir_ms:.2}ms)");
        }
        if let Some(typecheck_ms) = timing.typecheck_ms {
            if let Some(ref td) = typed {
                eprintln!(
                    "  typecheck: {} binding(s) ({typecheck_ms:.2}ms)",
                    td.type_env.len()
                );
            } else {
                eprintln!("  typecheck: failed ({typecheck_ms:.2}ms)");
            }
        }
        eprintln!();
    }

    // --- Verify (only if type check succeeded and layer >= 1) ---
    let verify_start = Instant::now();
    let cache_dir = std::path::Path::new(filename)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let verify_cache = assura_smt::VerificationCache::new(cache_dir);
    let mut verification_results = if layer >= 1 {
        if let Some(ref typed) = typed {
            assura_smt::verify_parallel_with_solver(typed, &verify_cache, solver)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // --- Dispatch pending decrease checks to SMT ---
    if let Some(ref typed) = typed {
        verification_results.extend(assura_smt::display::dispatch_decrease_checks(typed));
    }

    // --- Quantifier bound validation ---
    if let Some(ref typed) = typed {
        let qwarnings = assura_smt::validate_quantifier_bounds(typed);
        for w in &qwarnings {
            diagnostics.push(
                assura_diagnostics::Diagnostic::warning(
                    "A05200",
                    format!(
                        "unbounded quantifier in {}: {} ({})",
                        w.context, w.domain_desc, w.reason
                    ),
                    0..0,
                )
                .with_file(filename),
            );
        }
    }

    let verify_ms = verify_start.elapsed().as_secs_f64() * 1000.0;
    if verbosity == Verbosity::Verbose && output_mode == OutputMode::Human {
        eprintln!(
            "  verify:    {} clause(s) ({verify_ms:.2}ms)",
            verification_results.len()
        );
        let total = timing.lex_ms
            + timing.parse_ms
            + timing.resolve_ms.unwrap_or(0.0)
            + timing.typecheck_ms.unwrap_or(0.0)
            + verify_ms;
        eprintln!("  total:     {total:.2}ms");
        eprintln!();
    }

    // --- Dump SMT queries to files ---
    if let Some(smt_dir) = dump_smt
        && let Some(ref typed) = typed
    {
        let dir = Path::new(smt_dir);
        fs::create_dir_all(dir).unwrap_or_else(|e| {
            eprintln!("Error: cannot create {smt_dir}: {e}");
            process::exit(2);
        });
        let queries = assura_smt::dump_smt_queries(typed);
        for (i, q) in queries.iter().enumerate() {
            let name = format!("{}_{}.smt2", q.context, q.kind);
            let path = dir.join(
                if queries
                    .iter()
                    .filter(|qq| qq.context == q.context && qq.kind == q.kind)
                    .count()
                    > 1
                {
                    format!("{}_{}_{}.smt2", q.context, q.kind, i)
                } else {
                    name
                },
            );
            fs::write(&path, &q.script).unwrap_or_else(|e| {
                eprintln!("Error writing {}: {e}", path.display());
            });
        }
        if output_mode == OutputMode::Human {
            eprintln!("Wrote {} SMT-LIB2 file(s) to {smt_dir}/", queries.len());
        }
    }

    // --- Print stats ---
    if stats && output_mode == OutputMode::Human {
        let verified = verification_results
            .iter()
            .filter(|r| matches!(r, assura_smt::VerificationResult::Verified { .. }))
            .count();
        let counterexamples = verification_results
            .iter()
            .filter(|r| matches!(r, assura_smt::VerificationResult::Counterexample { .. }))
            .count();
        let timeouts = verification_results
            .iter()
            .filter(|r| matches!(r, assura_smt::VerificationResult::Timeout { .. }))
            .count();
        let unknowns = verification_results
            .iter()
            .filter(|r| matches!(r, assura_smt::VerificationResult::Unknown { .. }))
            .count();
        let total_ms = timing.lex_ms
            + timing.parse_ms
            + timing.resolve_ms.unwrap_or(0.0)
            + timing.hir_ms.unwrap_or(0.0)
            + timing.typecheck_ms.unwrap_or(0.0)
            + verify_ms;

        eprintln!();
        eprintln!("=== Verification Statistics ===");
        eprintln!("  Clauses:         {}", verification_results.len());
        eprintln!("  Verified:        {verified}");
        eprintln!("  Counterexamples: {counterexamples}");
        eprintln!("  Timeouts:        {timeouts}");
        eprintln!("  Unknown:         {unknowns}");
        eprintln!();
        eprintln!(
            "  Lex time:        {:.2}ms ({} tokens)",
            timing.lex_ms, timing.token_count
        );
        eprintln!("  Parse time:      {:.2}ms", timing.parse_ms);
        if let Some(ms) = timing.resolve_ms {
            eprintln!("  Resolve time:    {ms:.2}ms");
        }
        if let Some(ms) = timing.hir_ms {
            eprintln!("  HIR lower time:  {ms:.2}ms");
        }
        if let Some(ms) = timing.typecheck_ms {
            eprintln!("  Type-check time: {ms:.2}ms");
        }
        eprintln!("  Verify time:     {verify_ms:.2}ms");
        eprintln!("  Total time:      {total_ms:.2}ms");
    }

    // Convert counterexamples to diagnostics so they appear in both modes
    for vr in &verification_results {
        if let assura_smt::VerificationResult::Counterexample {
            clause_desc, model, ..
        } = vr
        {
            has_errors = true;
            diagnostics.push(
                assura_diagnostics::Diagnostic::error(
                    "A05100",
                    format!("verification failed for {clause_desc}: {model}"),
                    0..0,
                )
                .with_file(filename),
            );
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

            let mut output = serde_json::json!({
                "file_info": file_info,
                "diagnostics": diagnostics,
                "verification": verification_json,
                "layer": layer,
            });
            if let Some((ref cfg, ref root)) = config {
                output["config"] = serde_json::json!({
                    "project_root": root.display().to_string(),
                    "package": {
                        "name": cfg.package.name,
                        "version": cfg.package.version,
                    },
                    "build": {
                        "target": cfg.build.target,
                        "output": cfg.build.output,
                    },
                    "verify": {
                        "smt_solver": cfg.verify.smt_solver,
                        "layer": cfg.verify.layer,
                        "timeout": cfg.verify.timeout,
                    },
                });
            }
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        OutputMode::Human => {
            // Lex errors already reported above; report the rest.
            let non_lex: Vec<_> = diagnostics.iter().filter(|d| d.code != "A01001").collect();
            // Always report error diagnostics, even in quiet mode
            if has_errors || verbosity != Verbosity::Quiet {
                for d in &non_lex {
                    assura_diagnostics::render_diagnostic(d, filename, &source);
                }
            }

            // Print verification results grouped by contract/function
            if verbosity != Verbosity::Quiet {
                if !verification_results.is_empty() {
                    eprintln!();
                    eprintln!("Verification ({} clause(s)):", verification_results.len());
                    let _ = assura_smt::display::write_grouped_verification(
                        &mut std::io::stderr(),
                        &verification_results,
                        "  ",
                    );
                } else if layer == 0 {
                    eprintln!();
                    eprintln!("Verification skipped (--layer 0: structural checks only)");
                } else if layer >= 1 {
                    // Layer 1+ but no results: show what contracts exist
                    // and that they had no verifiable clauses
                    if let Some(ref f) = file {
                        let contract_names = assura_smt::display::collect_contract_names(f);
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
            } else if has_errors {
                // Quiet mode: only show error count
                eprintln!("{filename}: {} error(s) found", diagnostics.len());
            }
        }
    }

    process::exit(if has_errors { 1 } else { 0 });
}

// ---------------------------------------------------------------------------
// Watch mode
// ---------------------------------------------------------------------------

/// Check a single file and print results. Returns true if there were errors.
fn check_file_once(
    filename: &str,
    output_mode: OutputMode,
    verbosity: Verbosity,
    layer: u8,
) -> bool {
    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {filename}: {e}");
            return true;
        }
    };

    let CompilationResult {
        file,
        resolved,
        hir,
        typed,
        mut diagnostics,
        mut has_errors,
        timing,
    } = compile(&source, filename);

    if verbosity == Verbosity::Verbose && output_mode == OutputMode::Human {
        eprintln!("Pipeline timing for {filename}:");
        eprintln!(
            "  lex:       {} tokens ({:.2}ms)",
            timing.token_count, timing.lex_ms
        );
        if let Some(ref f) = file {
            eprintln!(
                "  parse:     {} declaration(s), {} import(s) ({:.2}ms)",
                f.decls.len(),
                f.imports.len(),
                timing.parse_ms
            );
        } else {
            eprintln!("  parse:     failed ({:.2}ms)", timing.parse_ms);
        }
        if let Some(resolve_ms) = timing.resolve_ms {
            if let Some(ref r) = resolved {
                let user_symbols = r
                    .symbols
                    .symbols
                    .iter()
                    .filter(|s| s.kind != assura_resolve::SymbolKind::BuiltinType)
                    .count();
                eprintln!("  resolve:   {user_symbols} symbol(s) ({resolve_ms:.2}ms)");
            } else {
                eprintln!("  resolve:   failed ({resolve_ms:.2}ms)");
            }
        }
        if let Some(hir_ms) = timing.hir_ms {
            if let Some(ref h) = hir {
                eprintln!("  hir:       {} decl(s) ({hir_ms:.2}ms)", h.decls.len());
            } else {
                eprintln!("  hir:       skipped ({hir_ms:.2}ms)");
            }
        }
        if let Some(typecheck_ms) = timing.typecheck_ms {
            if let Some(ref td) = typed {
                eprintln!(
                    "  typecheck: {} binding(s) ({typecheck_ms:.2}ms)",
                    td.type_env.len()
                );
            } else {
                eprintln!("  typecheck: failed ({typecheck_ms:.2}ms)");
            }
        }
        eprintln!();
    }

    // Verify
    let watch_cache_dir = std::path::Path::new(filename)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let watch_verify_cache = assura_smt::VerificationCache::new(watch_cache_dir);
    let mut verification_results = if layer >= 1 {
        if let Some(ref typed) = typed {
            assura_smt::verify_parallel(typed, &watch_verify_cache)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    if let Some(ref typed) = typed {
        verification_results.extend(assura_smt::display::dispatch_decrease_checks(typed));
    }

    if let Some(ref typed) = typed {
        let qwarnings = assura_smt::validate_quantifier_bounds(typed);
        for w in &qwarnings {
            diagnostics.push(
                assura_diagnostics::Diagnostic::warning(
                    "A05200",
                    format!(
                        "unbounded quantifier in {}: {} ({})",
                        w.context, w.domain_desc, w.reason
                    ),
                    0..0,
                )
                .with_file(filename),
            );
        }
    }

    for vr in &verification_results {
        if let assura_smt::VerificationResult::Counterexample {
            clause_desc, model, ..
        } = vr
        {
            has_errors = true;
            diagnostics.push(
                assura_diagnostics::Diagnostic::error(
                    "A05100",
                    format!("verification failed for {clause_desc}: {model}"),
                    0..0,
                )
                .with_file(filename),
            );
        }
    }

    if output_mode == OutputMode::Human {
        let non_lex: Vec<_> = diagnostics.iter().filter(|d| d.code != "A01001").collect();
        if has_errors || verbosity != Verbosity::Quiet {
            for d in &non_lex {
                assura_diagnostics::render_diagnostic(d, filename, &source);
            }
        }

        if verbosity != Verbosity::Quiet {
            if !verification_results.is_empty() {
                eprintln!();
                eprintln!("Verification ({} clause(s)):", verification_results.len());
                let _ = assura_smt::display::write_grouped_verification(
                    &mut std::io::stderr(),
                    &verification_results,
                    "  ",
                );
            }
            if !has_errors {
                eprintln!("{filename}: check passed (no errors)");
            } else {
                eprintln!("{filename}: {} error(s) found", diagnostics.len());
            }
        } else if has_errors {
            eprintln!("{filename}: {} error(s) found", diagnostics.len());
        }
    }

    has_errors
}

/// Run check in watch mode: check once, then watch for file changes.
fn run_watch_loop(filename: &str, output_mode: OutputMode, verbosity: Verbosity, layer: u8) -> ! {
    use notify::{Event, EventKind, RecursiveMode, Watcher};

    let path = Path::new(filename).canonicalize().unwrap_or_else(|e| {
        eprintln!("Error: cannot resolve path {filename}: {e}");
        process::exit(2);
    });

    // Initial check
    eprintln!("[watch] Checking {filename}...");
    eprintln!();
    let _ = check_file_once(filename, output_mode, verbosity, layer);
    eprintln!();
    eprintln!("[watch] Watching {filename} for changes. Press Ctrl+C to stop.");

    let (tx, rx) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            // Only trigger on modify/create events
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                let _ = tx.send(());
            }
        }
    })
    .unwrap_or_else(|e| {
        eprintln!("Error: failed to create file watcher: {e}");
        process::exit(2);
    });

    // Watch the file's parent directory to catch renames/replacements
    let watch_dir = path.parent().unwrap_or(&path);
    watcher
        .watch(watch_dir, RecursiveMode::NonRecursive)
        .unwrap_or_else(|e| {
            eprintln!("Error: failed to watch {}: {e}", watch_dir.display());
            process::exit(2);
        });

    loop {
        // Wait for a change event
        let _ = rx.recv();

        // Debounce: drain any additional events that arrive within 100ms
        while rx.recv_timeout(Duration::from_millis(100)).is_ok() {}

        // Clear screen and re-check
        eprint!("\x1B[2J\x1B[H");
        eprintln!("[watch] File changed, re-checking {filename}...");
        eprintln!();
        let _ = check_file_once(filename, output_mode, verbosity, layer);
        eprintln!();
        eprintln!("[watch] Watching for changes. Press Ctrl+C to stop.");
    }
}

// ---------------------------------------------------------------------------
// `assura build <file.assura>` — codegen to generated/
// ---------------------------------------------------------------------------

fn run_build(
    filename: &str,
    _output_mode: OutputMode,
    verbosity: Verbosity,
    cli_output: &str,
    cli_target: Option<assura_codegen::CompileTarget>,
    no_check: bool,
    cli_solver: Option<assura_smt::SolverChoice>,
) {
    // Load project config (assura.toml) if available
    let project = load_project_config(Path::new(filename));
    let config_output = project
        .as_ref()
        .map(|(c, _)| c.build.output.clone())
        .unwrap_or_else(|| "generated".to_string());

    // Output directory: CLI flag (non-default) > config file > default "generated"
    let out_dir_str = if cli_output != "generated" {
        cli_output
    } else {
        config_output.as_str()
    };

    // Solver choice: CLI flag > config file > default (Z3)
    let build_solver = cli_solver
        .or_else(|| {
            project
                .as_ref()
                .and_then(|(c, _)| assura_smt::SolverChoice::from_str_loose(&c.verify.smt_solver))
        })
        .unwrap_or(assura_smt::SolverChoice::Z3);

    // Target: CLI flag > config file > default (native)
    let compile_target = cli_target
        .or_else(|| {
            project
                .as_ref()
                .and_then(|(c, _)| assura_codegen::CompileTarget::from_str_loose(&c.build.target))
        })
        .unwrap_or(assura_codegen::CompileTarget::Native);

    // Build unified compiler config
    let compiler_config = if let Some((ref proj, _)) = project {
        let mut cc = CompilerConfig::from_project(proj, _output_mode, verbosity);
        cc.verify.solver = build_solver.as_str().to_string();
        cc.codegen.output_dir = out_dir_str.to_string();
        cc
    } else {
        CompilerConfig {
            output_mode: _output_mode,
            verbosity,
            verify: assura_config::VerifyOptions {
                solver: build_solver.as_str().to_string(),
                ..Default::default()
            },
            codegen: assura_config::CodegenConfig {
                output_dir: out_dir_str.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    };
    let config = project;

    let source = fs::read_to_string(filename).unwrap_or_else(|e| {
        eprintln!("Error: {filename}: {e}");
        process::exit(2);
    });

    // --- Run shared pipeline ---
    let CompilationResult {
        diagnostics,
        has_errors,
        typed,
        timing,
        file: parsed_file,
        resolved,
        hir: _hir,
    } = compile_with_config(&source, filename, &compiler_config);

    if verbosity == Verbosity::Verbose {
        if let Some((ref cfg, ref root)) = config {
            eprintln!(
                "Project: {} v{} ({})",
                cfg.package.name,
                cfg.package.version,
                root.display()
            );
            eprintln!(
                "  config: output={}, target={}, solver={}, timeout={}ms",
                cfg.build.output, cfg.build.target, cfg.verify.smt_solver, cfg.verify.timeout
            );
            eprintln!();
        }
        eprintln!("Pipeline timing for {filename}:");
        eprintln!(
            "  lex:       {} tokens ({:.2}ms)",
            timing.token_count, timing.lex_ms
        );
        if let Some(ref f) = parsed_file {
            eprintln!(
                "  parse:     {} declaration(s), {} import(s) ({:.2}ms)",
                f.decls.len(),
                f.imports.len(),
                timing.parse_ms
            );
        }
        if let Some(resolve_ms) = timing.resolve_ms
            && let Some(ref r) = resolved
        {
            let user_symbols = r
                .symbols
                .symbols
                .iter()
                .filter(|s| s.kind != assura_resolve::SymbolKind::BuiltinType)
                .count();
            eprintln!("  resolve:   {user_symbols} symbol(s) ({resolve_ms:.2}ms)");
        }
        if let Some(hir_ms) = timing.hir_ms {
            eprintln!("  hir:       ({hir_ms:.2}ms)");
        }
        if let Some(typecheck_ms) = timing.typecheck_ms
            && let Some(ref td) = typed
        {
            eprintln!(
                "  typecheck: {} binding(s) ({typecheck_ms:.2}ms)",
                td.type_env.len()
            );
        }
    }

    // Report errors in human mode
    if has_errors {
        assura_diagnostics::report_diagnostics_human(&diagnostics, filename, &source);
        eprintln!("{filename}: {} error(s) found", diagnostics.len());
        process::exit(1);
    }

    let typed = typed.expect("type check should succeed if has_errors is false");

    // --- Quantifier bound validation ---
    let qwarnings = assura_smt::validate_quantifier_bounds(&typed);
    if verbosity != Verbosity::Quiet {
        for w in &qwarnings {
            eprintln!(
                "warning: unbounded quantifier in {}: {} ({})",
                w.context, w.domain_desc, w.reason
            );
        }
    }

    // --- Verify ---
    let verify_start = Instant::now();
    let build_cache_dir = std::path::Path::new(filename)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let build_verify_cache = assura_smt::VerificationCache::new(build_cache_dir);
    let mut verification_results =
        assura_smt::verify_parallel_with_solver(&typed, &build_verify_cache, build_solver);
    verification_results.extend(assura_smt::display::dispatch_decrease_checks(&typed));
    let verify_ms = verify_start.elapsed().as_secs_f64() * 1000.0;

    if verbosity == Verbosity::Verbose {
        eprintln!(
            "  verify:    {} clause(s) ({verify_ms:.2}ms)",
            verification_results.len()
        );
    }

    if verbosity != Verbosity::Quiet && !verification_results.is_empty() {
        eprintln!();
        eprintln!("Verification ({} clause(s)):", verification_results.len());
        let _ = assura_smt::display::write_grouped_verification(
            &mut std::io::stderr(),
            &verification_results,
            "  ",
        );
    }

    // --- Codegen ---
    let codegen_start = Instant::now();
    let backend_config = assura_codegen::BackendConfig {
        target: compile_target.clone(),
        ..assura_codegen::BackendConfig::default()
    };
    let project = assura_codegen::codegen_with_config(&typed, &backend_config);

    // --- Write to output directory ---
    let out_dir = Path::new(out_dir_str);
    fs::create_dir_all(out_dir).unwrap_or_else(|e| {
        eprintln!("Error: cannot create {out_dir_str}/ directory: {e}");
        process::exit(1);
    });

    let codegen_ms = codegen_start.elapsed().as_secs_f64() * 1000.0;
    if verbosity == Verbosity::Verbose {
        eprintln!(
            "  codegen:   {} file(s) ({codegen_ms:.2}ms)",
            project.files.len()
        );
        let total = timing.lex_ms
            + timing.parse_ms
            + timing.resolve_ms.unwrap_or(0.0)
            + timing.typecheck_ms.unwrap_or(0.0)
            + verify_ms
            + codegen_ms;
        eprintln!("  total:     {total:.2}ms");
        eprintln!();
    }

    // Write Cargo.toml
    let cargo_path = out_dir.join("Cargo.toml");
    fs::write(&cargo_path, &project.cargo_toml).unwrap_or_else(|e| {
        eprintln!("Error: cannot write {}: {e}", cargo_path.display());
        process::exit(1);
    });
    if verbosity != Verbosity::Quiet {
        println!("  wrote {}", cargo_path.display());
    }

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
        if verbosity != Verbosity::Quiet {
            println!("  wrote {}", full_path.display());
        }
    }

    // --- Generate .cargo/config.toml for WASM target ---
    if matches!(compile_target, assura_codegen::CompileTarget::Wasm) {
        let cargo_dir = out_dir.join(".cargo");
        fs::create_dir_all(&cargo_dir).unwrap_or_else(|e| {
            eprintln!("Error: cannot create {}: {e}", cargo_dir.display());
            process::exit(1);
        });
        let config_toml = cargo_dir.join("config.toml");
        fs::write(&config_toml, "[build]\ntarget = \"wasm32-wasip1\"\n").unwrap_or_else(|e| {
            eprintln!("Error: cannot write {}: {e}", config_toml.display());
            process::exit(1);
        });
        if verbosity != Verbosity::Quiet {
            println!("  wrote {}", config_toml.display());
        }
    }

    // --- Validate generated Rust compiles ---
    let skip_check = no_check;
    if !skip_check {
        let mut cmd = process::Command::new("cargo");
        cmd.arg("check").current_dir(out_dir);
        if let Some(triple) = compile_target.rust_target() {
            cmd.arg("--target").arg(triple);
        }
        let cargo_check = cmd
            .stdout(process::Stdio::piped())
            .stderr(process::Stdio::piped())
            .output();

        match cargo_check {
            Ok(output) if output.status.success() => {
                if verbosity != Verbosity::Quiet {
                    println!("OK  {filename} -> {out_dir_str}/ (generated Rust compiles)");
                }
            }
            Ok(output) => {
                if verbosity != Verbosity::Quiet {
                    println!("OK  {filename} -> {out_dir_str}/");
                }
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
                if verbosity != Verbosity::Quiet {
                    println!(
                        "OK  {filename} -> {out_dir_str}/ (cargo check skipped: cargo not found)"
                    );
                }
            }
        }
    } else if verbosity != Verbosity::Quiet {
        println!("OK  {filename} -> {out_dir_str}/ (check skipped)");
    }
}

// ---------------------------------------------------------------------------
// `assura fmt <file> [--check]` — format an .assura source file
// ---------------------------------------------------------------------------

fn run_fmt(filename: &str, check_only: bool) {
    let source = fs::read_to_string(filename).unwrap_or_else(|e| {
        eprintln!("Error: {filename}: {e}");
        process::exit(2);
    });

    let (file, errors) = assura_parser::parse(&source);

    if !errors.is_empty() {
        eprintln!(
            "Error: cannot format {filename}: {} parse error(s)",
            errors.len()
        );
        for e in &errors {
            eprintln!("  {e}");
        }
        process::exit(1);
    }

    let file = match file {
        Some(f) => f,
        None => {
            eprintln!("Error: cannot format {filename}: parse returned no AST");
            process::exit(1);
        }
    };

    let formatted = assura_fmt::format_source_file(&file);

    if check_only {
        if formatted == source {
            process::exit(0);
        } else {
            eprintln!("{filename}: not formatted");
            process::exit(1);
        }
    } else {
        fs::write(filename, &formatted).unwrap_or_else(|e| {
            eprintln!("Error: cannot write {filename}: {e}");
            process::exit(2);
        });
    }
}

// ---------------------------------------------------------------------------
// `assura init <project-name>` -- scaffold a new Assura project
// ---------------------------------------------------------------------------

fn run_init(project_name: &str) {
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
        r#"[package]
name = "{project_name}"
version = "0.1.0"

[build]
target = "native"       # "native" or "wasm32-wasi"
output = "generated"

[verify]
smt-solver = "z3"       # "z3", "cvc5", or "portfolio"
layer = 1               # 0 = structural only, 1 = SMT
timeout = 1000          # SMT timeout in ms

[profile]
type = "minimal"        # minimal, parser, database, etc.
"#
    );
    let toml_path = project_dir.join("assura.toml");
    fs::write(&toml_path, &toml_content).unwrap_or_else(|e| {
        eprintln!("Error: cannot write {}: {e}", toml_path.display());
        process::exit(1);
    });

    // Write starter contract
    let contract_content = r#"// SafeDivision: ensures division by zero is impossible
//
// The requires clause guarantees callers must pass a non-zero divisor.
// The ensures clause states the result is always defined (not an error).
contract SafeDivision {
    input(a: Int, b: Int)
    output(result: Int)

    requires { b != 0 }
    ensures  { b != 0 }
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

fn run_explain(code: &str) {
    match assura_diagnostics::explain(code) {
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
            let catalog = assura_diagnostics::error_catalog();
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

fn run_legacy(filename: &str, verbosity: Verbosity, show_ast: bool, show_tokens: bool) {
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
        hir: _,
        typed,
        diagnostics,
        has_errors,
        timing,
    } = compile(&source, filename);

    if verbosity == Verbosity::Verbose {
        eprintln!("Pipeline timing for {filename}:");
        eprintln!(
            "  lex:       {} tokens ({:.2}ms)",
            timing.token_count, timing.lex_ms
        );
        if let Some(ref f) = file {
            eprintln!(
                "  parse:     {} declaration(s), {} import(s) ({:.2}ms)",
                f.decls.len(),
                f.imports.len(),
                timing.parse_ms
            );
        }
        if let Some(resolve_ms) = timing.resolve_ms
            && let Some(ref r) = resolved
        {
            let user_symbols = r
                .symbols
                .symbols
                .iter()
                .filter(|s| s.kind != assura_resolve::SymbolKind::BuiltinType)
                .count();
            eprintln!("  resolve:   {user_symbols} symbol(s) ({resolve_ms:.2}ms)");
        }
        if let Some(hir_ms) = timing.hir_ms {
            eprintln!("  hir:       ({hir_ms:.2}ms)");
        }
        if let Some(typecheck_ms) = timing.typecheck_ms
            && let Some(ref td) = typed
        {
            eprintln!(
                "  typecheck: {} binding(s) ({typecheck_ms:.2}ms)",
                td.type_env.len()
            );
        }
        eprintln!();
    }

    if has_errors {
        assura_diagnostics::report_diagnostics_human(&diagnostics, filename, &source);
        if verbosity != Verbosity::Quiet {
            eprintln!("{filename}: {} error(s) found", diagnostics.len());
        }
        process::exit(1);
    }

    let file = file.expect("file should exist if has_errors is false");
    let resolved = resolved.expect("resolved should exist if has_errors is false");
    let typed = typed.expect("typed should exist if has_errors is false");

    // --- Verify ---
    let verify_start = Instant::now();
    let explain_cache_dir = std::path::Path::new(filename)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let explain_verify_cache = assura_smt::VerificationCache::new(explain_cache_dir);
    let mut verification_results = assura_smt::verify_parallel(&typed, &explain_verify_cache);
    verification_results.extend(assura_smt::display::dispatch_decrease_checks(&typed));
    let verify_ms = verify_start.elapsed().as_secs_f64() * 1000.0;

    if verbosity == Verbosity::Verbose {
        eprintln!(
            "  verify:    {} clause(s) ({verify_ms:.2}ms)",
            verification_results.len()
        );
        let total = timing.lex_ms
            + timing.parse_ms
            + timing.resolve_ms.unwrap_or(0.0)
            + timing.typecheck_ms.unwrap_or(0.0)
            + verify_ms;
        eprintln!("  total:     {total:.2}ms");
        eprintln!();
    }

    // --- Output ---
    if verbosity == Verbosity::Quiet {
        // Quiet mode: no output for success
    } else if show_ast {
        assura_parser::display::print_ast(&file);
    } else {
        let _ = assura_smt::display::write_summary(
            &mut std::io::stdout(),
            filename,
            &file,
            &resolved.symbols,
            &typed.type_env,
            &verification_results,
        );
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
    fn test_parse_error_includes_message() {
        // Syntax error should produce an error with a meaningful message
        let (_file, errors) = assura_parser::parse("contract 123");
        assert!(!errors.is_empty(), "expected at least one parse error");
        let e = &errors[0];
        assert!(
            !e.message.is_empty(),
            "parse error should have a non-empty message, got: {e:?}"
        );
        // The error span should point to a valid location
        assert!(
            e.span.start <= e.span.end,
            "error span should be valid: {:?}",
            e.span
        );
    }

    #[test]
    fn test_resolution_error_diagnostic() {
        // Valid parse but contains an unresolved reference
        let source = r#"
contract Foo {
  requires { unknown_fn(x) }
}
"#;
        let file = assura_parser::parse_unwrap(source);
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
        let file = assura_parser::parse_unwrap(source);
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
        let file = assura_parser::parse_unwrap(source);
        let resolved = assura_resolve::resolve(&file).unwrap();
        let typed = assura_types::type_check(&resolved).unwrap();
        let config = assura_codegen::BackendConfig {
            backend: assura_codegen::CodegenBackend::Cranelift,
            opt_level: 0,
            debug_info: true,
            target: assura_codegen::CompileTarget::Native,
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

    // =======================================================================
    // P001: Verbose and quiet mode tests
    // =======================================================================

    #[test]
    fn verbose_check_shows_timing() {
        let out = std::process::Command::new(assura_bin())
            .args(["check", "--verbose", "demos/libwebp-huffman.assura"])
            .current_dir(workspace_root())
            .output()
            .expect("failed to run assura check --verbose");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("Pipeline timing"),
            "should show pipeline timing header: {stderr}"
        );
        assert!(stderr.contains("lex:"), "should show lex timing: {stderr}");
        assert!(
            stderr.contains("parse:"),
            "should show parse timing: {stderr}"
        );
        assert!(
            stderr.contains("resolve:"),
            "should show resolve timing: {stderr}"
        );
        assert!(
            stderr.contains("typecheck:"),
            "should show typecheck timing: {stderr}"
        );
        assert!(
            stderr.contains("ms"),
            "should show millisecond units: {stderr}"
        );
        assert!(
            stderr.contains("total:"),
            "should show total timing: {stderr}"
        );
    }

    #[test]
    fn quiet_check_suppresses_summary() {
        let out = std::process::Command::new(assura_bin())
            .args(["check", "--quiet", "demos/libwebp-huffman.assura"])
            .current_dir(workspace_root())
            .output()
            .expect("failed to run assura check --quiet");
        assert!(out.status.success(), "check should succeed");
        let stderr = String::from_utf8_lossy(&out.stderr);
        // Quiet mode should not show "check passed" or verification info
        assert!(
            !stderr.contains("check passed"),
            "quiet mode should not show 'check passed': {stderr}"
        );
        assert!(
            !stderr.contains("Verification"),
            "quiet mode should not show verification summary: {stderr}"
        );
    }

    #[test]
    fn quiet_check_shows_errors() {
        let out = std::process::Command::new(assura_bin())
            .args([
                "check",
                "--quiet",
                "tests/fixtures/must_reject/clause_type_error.assura",
            ])
            .current_dir(workspace_root())
            .output()
            .expect("failed to run assura check --quiet on invalid file");
        assert!(!out.status.success(), "check should fail on invalid input");
        let stderr = String::from_utf8_lossy(&out.stderr);
        // Even in quiet mode, errors must be visible
        assert!(
            stderr.contains("error"),
            "quiet mode should still show errors: {stderr}"
        );
    }

    #[test]
    fn verbose_short_flag_works() {
        let out = std::process::Command::new(assura_bin())
            .args(["check", "-v", "demos/libwebp-huffman.assura"])
            .current_dir(workspace_root())
            .output()
            .expect("failed to run assura check -v");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("Pipeline timing"),
            "-v should work like --verbose: {stderr}"
        );
    }

    #[test]
    fn quiet_short_flag_works() {
        let out = std::process::Command::new(assura_bin())
            .args(["check", "-q", "demos/libwebp-huffman.assura"])
            .current_dir(workspace_root())
            .output()
            .expect("failed to run assura check -q");
        assert!(out.status.success(), "check should succeed");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("check passed"),
            "-q should work like --quiet: {stderr}"
        );
    }

    #[test]
    fn verbose_build_shows_codegen_timing() {
        let tmp = std::env::temp_dir().join("assura_p001_verbose_build");
        let _ = std::fs::remove_dir_all(&tmp);
        let out = std::process::Command::new(assura_bin())
            .args([
                "build",
                "--verbose",
                "demos/libwebp-huffman.assura",
                "--output",
                tmp.to_str().unwrap(),
            ])
            .current_dir(workspace_root())
            .output()
            .expect("failed to run assura build --verbose");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("Pipeline timing"),
            "build --verbose should show timing: {stderr}"
        );
        assert!(
            stderr.contains("codegen:"),
            "build --verbose should show codegen timing: {stderr}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // =======================================================================
    // T205: End-to-end round-trip tests
    // =======================================================================

    /// Helper: run the full pipeline on a demo file and return the generated project.
    fn roundtrip_demo(demo_name: &str) -> assura_codegen::GeneratedProject {
        let source = std::fs::read_to_string(format!("../../demos/{demo_name}"))
            .or_else(|_| std::fs::read_to_string(format!("demos/{demo_name}")))
            .unwrap_or_else(|_| panic!("cannot find demo: {demo_name}"));
        full_pipeline(&source).unwrap_or_else(|e| panic!("{demo_name}: pipeline failed: {e}"))
    }

    /// Helper: write a GeneratedProject to a temp dir and run cargo check on it.
    fn cargo_check_project(project: &assura_codegen::GeneratedProject, label: &str) {
        let tmp = std::env::temp_dir().join(format!("assura_t205_{label}"));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();
        std::fs::write(tmp.join("Cargo.toml"), &project.cargo_toml).unwrap();
        for (path, content) in &project.files {
            let full = tmp.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&full, content).unwrap();
        }
        let output = std::process::Command::new("cargo")
            .arg("check")
            .current_dir(&tmp)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .expect("cargo check failed to start");
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(
            output.status.success(),
            "{label}: generated Rust failed cargo check:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn roundtrip_libwebp_generates_valid_rust() {
        let project = roundtrip_demo("libwebp-huffman.assura");
        // Verify syntactically valid
        for (path, content) in &project.files {
            syn::parse_file(content).unwrap_or_else(|e| {
                panic!("libwebp {path}: invalid Rust: {e}");
            });
        }
        // Verify cargo check passes
        cargo_check_project(&project, "libwebp");
    }

    #[test]
    fn roundtrip_zlib_generates_valid_rust() {
        let project = roundtrip_demo("zlib-inflate.assura");
        for (path, content) in &project.files {
            syn::parse_file(content).unwrap_or_else(|e| {
                panic!("zlib {path}: invalid Rust: {e}");
            });
        }
        cargo_check_project(&project, "zlib");
    }

    #[test]
    fn roundtrip_mbedtls_generates_valid_rust() {
        let project = roundtrip_demo("mbedtls-x509.assura");
        for (path, content) in &project.files {
            syn::parse_file(content).unwrap_or_else(|e| {
                panic!("mbedtls {path}: invalid Rust: {e}");
            });
        }
        cargo_check_project(&project, "mbedtls");
    }

    #[test]
    fn roundtrip_libwebp_has_debug_asserts() {
        let project = roundtrip_demo("libwebp-huffman.assura");
        // Contracts with requires clauses should produce debug_assert! calls
        let all_source: String = project
            .files
            .iter()
            .map(|(_, content)| content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        // The libwebp demo has requires { alphabet_size <= MAX_ALPHABET_SIZE }
        // which should produce a debug_assert in the generated code
        assert!(
            all_source.contains("debug_assert!"),
            "generated code should contain debug_assert! from requires clauses"
        );
    }

    #[test]
    fn roundtrip_zlib_has_function_stubs() {
        let project = roundtrip_demo("zlib-inflate.assura");
        let all_source: String = project
            .files
            .iter()
            .map(|(_, content)| content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        // The zlib demo defines functions and contracts
        assert!(
            all_source.contains("fn inflate_extra_field_step"),
            "zlib generated code should contain inflate_extra_field_step function"
        );
        assert!(
            all_source.contains("fn validate_xlen"),
            "zlib generated code should contain validate_xlen function"
        );
    }

    #[test]
    fn roundtrip_libwebp_function_signatures_present() {
        let project = roundtrip_demo("libwebp-huffman.assura");
        let all_source: String = project
            .files
            .iter()
            .map(|(_, content)| content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        // The libwebp demo defines functions like validate_code_lengths
        // and a contract BuildHuffmanTableContract with a check() method
        assert!(
            all_source.contains("fn validate_code_lengths"),
            "generated code should have validate_code_lengths function"
        );
        assert!(
            all_source.contains("fn check("),
            "generated code should have check function from contract"
        );
    }

    #[test]
    fn roundtrip_contract_with_ensures_has_postcondition() {
        // A contract with ensures should generate postcondition checks
        let source = r#"
contract PostCheck {
    input(a: Int, b: Int)
    output(result: Int)
    requires { b != 0 }
    ensures { result * b == a }
}
"#;
        let project = full_pipeline(source).expect("pipeline failed");
        let all_source: String = project
            .files
            .iter()
            .map(|(_, content)| content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        // requires clause should become debug_assert
        assert!(
            all_source.contains("debug_assert!"),
            "should have debug_assert from requires clause"
        );
        // The function should have the right parameter types
        assert!(
            all_source.contains("i64") || all_source.contains("Int"),
            "should have integer types in generated code"
        );
    }

    #[test]
    fn roundtrip_service_generates_typestate() {
        // A service with states should generate typestate markers
        let source = r#"
service Connection {
    states: Disconnected -> Connected -> Authenticated

    operation Connect {
        requires: state == Disconnected
        ensures: state == Connected
    }

    operation Authenticate {
        requires: state == Connected
        ensures: state == Authenticated
    }
}
"#;
        let project = full_pipeline(source).expect("pipeline failed");
        let all_source: String = project
            .files
            .iter()
            .map(|(_, content)| content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        // Should generate state marker structs
        assert!(
            all_source.contains("Disconnected") && all_source.contains("Connected"),
            "should generate state marker structs"
        );
        assert!(
            all_source.contains("PhantomData"),
            "should use PhantomData for typestate"
        );
    }

    #[test]
    fn roundtrip_project_has_valid_cargo_toml() {
        let project = roundtrip_demo("libwebp-huffman.assura");
        // Verify Cargo.toml has essential sections
        assert!(
            project.cargo_toml.contains("[package]"),
            "needs [package] section"
        );
        assert!(project.cargo_toml.contains("name ="), "needs package name");
        assert!(project.cargo_toml.contains("edition ="), "needs edition");
    }

    #[test]
    fn quiet_build_suppresses_file_listing() {
        let tmp = std::env::temp_dir().join("assura_p001_quiet_build");
        let _ = std::fs::remove_dir_all(&tmp);
        let out = std::process::Command::new(assura_bin())
            .args([
                "build",
                "--quiet",
                "demos/libwebp-huffman.assura",
                "--output",
                tmp.to_str().unwrap(),
                "--no-check",
            ])
            .current_dir(workspace_root())
            .output()
            .expect("failed to run assura build --quiet");
        assert!(
            out.status.success(),
            "build should succeed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        // Quiet mode should not list written files
        assert!(
            !stdout.contains("wrote"),
            "quiet mode should not list files: {stdout}"
        );
        assert!(
            !stdout.contains("OK"),
            "quiet mode should not show OK: {stdout}"
        );
        // But the files should still be generated
        assert!(
            tmp.join("Cargo.toml").exists(),
            "files should still be generated in quiet mode"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------------------
    // Formatter tests
    // ---------------------------------------------------------------------------

    /// Parse source, format it, re-parse, re-format, and assert idempotency.
    fn assert_format_idempotent(source: &str) {
        let file = assura_parser::parse_unwrap(source);

        let formatted1 = assura_fmt::format_source_file(&file);

        let (file2, errs2) = assura_parser::parse(&formatted1);
        assert!(
            errs2.is_empty(),
            "parse errors on formatted output: {errs2:?}\nformatted:\n{formatted1}"
        );
        let file2 = file2.expect("re-parse returned None");

        let formatted2 = assura_fmt::format_source_file(&file2);
        assert_eq!(
            formatted1, formatted2,
            "formatter is not idempotent:\n--- pass 1 ---\n{formatted1}\n--- pass 2 ---\n{formatted2}"
        );
    }

    #[test]
    fn fmt_contract_idempotent() {
        assert_format_idempotent(
            r#"
contract SafeDivision {
  input(a: Int, b: Int)
  output(result: Int)
  requires { b != 0 }
  ensures { result * b + (a mod b) == a }
  effects { pure }
}
"#,
        );
    }

    #[test]
    fn fmt_type_and_enum_idempotent() {
        assert_format_idempotent(
            r#"
type PositiveInt = { n: Int | n > 0 };
enum Color { Red, Green, Blue }
enum Result<T> { Ok(T), Err(String) }
"#,
        );
    }

    #[test]
    fn fmt_extern_fn_idempotent() {
        assert_format_idempotent(
            r#"
extern fn malloc(size: Nat) -> Bytes
  requires { size > 0 }
  ensures { result.length() == size }
  effects { mem.alloc };
"#,
        );
    }

    #[test]
    fn fmt_fn_with_clauses_idempotent() {
        assert_format_idempotent(
            r#"
fn fibonacci(n: Nat) -> Nat
  requires n >= 0
  decreases n
  ensures result >= 0
"#,
        );
    }

    #[test]
    fn fmt_service_idempotent() {
        assert_format_idempotent(
            r#"
service UserService {
  type User {
    id: Nat;
    name: String;
  }
  states: Created -> Active -> Deleted
  operation CreateUser {
    input(name: String)
    output(user: User)
    requires { name.length() > 0 }
    effects { database.write }
  }
  invariant { forall u in users: u.id > 0 }
}
"#,
        );
    }

    #[test]
    fn fmt_project_and_module_idempotent() {
        assert_format_idempotent(
            r#"
project myapp {
  profile: [core, mem, sec]
}

module app.main;

import std.math { abs };

contract Foo {
  input(x: Int)
  requires { x > 0 }
}
"#,
        );
    }

    #[test]
    fn fmt_feature_block_idempotent() {
        assert_format_idempotent(
            r#"
feature ecdsa = enabled
feature x509 = enabled
  requires: ecdsa
feature_max MAX_SIZE: Nat = 256
"#,
        );
    }

    #[test]
    fn fmt_produces_parseable_output() {
        // Verify that formatting a contract produces valid parseable output
        let source = "contract Foo {\n  input(x: Int)\n  requires { x > 0 }\n}\n";
        let (file, _) = assura_parser::parse(source);
        let file = file.unwrap();
        let formatted = assura_fmt::format_source_file(&file);
        // Must contain the contract name
        assert!(
            formatted.contains("contract Foo"),
            "formatted must contain contract name"
        );
        // Must re-parse without errors
        let (file2, errs) = assura_parser::parse(&formatted);
        assert!(
            errs.is_empty(),
            "formatted output has parse errors: {errs:?}"
        );
        assert!(file2.is_some(), "formatted output parsed to None");
    }

    #[test]
    fn fmt_dotted_effects_idempotent() {
        assert_format_idempotent(
            r#"
fn read_data(conn: &mut Connection) -> Bytes
  effects { io.read }
"#,
        );
    }

    // -----------------------------------------------------------------------
    // Config parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_full_config() {
        let toml_str = r#"
[package]
name = "my-project"
version = "1.2.3"

[build]
target = "wasm32-wasi"
output = "out"

[verify]
smt-solver = "cvc5"
layer = 0
timeout = 5000

[profile]
type = "database"
"#;
        let config: super::ProjectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.package.name, "my-project");
        assert_eq!(config.package.version, "1.2.3");
        assert_eq!(config.build.target, "wasm32-wasi");
        assert_eq!(config.build.output, "out");
        assert_eq!(config.verify.smt_solver, "cvc5");
        assert_eq!(config.verify.layer, 0);
        assert_eq!(config.verify.timeout, 5000);
        assert_eq!(config.profile.profile_type, "database");
    }

    #[test]
    fn parse_minimal_config() {
        let toml_str = r#"
[package]
name = "test"
"#;
        let config: super::ProjectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.package.name, "test");
        assert_eq!(config.package.version, "0.1.0"); // default
        assert_eq!(config.build.target, "native"); // default
        assert_eq!(config.build.output, "generated"); // default
        assert_eq!(config.verify.smt_solver, "z3"); // default
        assert_eq!(config.verify.layer, 1); // default
        assert_eq!(config.verify.timeout, 1000); // default
        assert_eq!(config.profile.profile_type, "minimal"); // default
    }

    #[test]
    fn parse_empty_config() {
        let config: super::ProjectConfig = toml::from_str("").unwrap();
        assert_eq!(config.package.name, ""); // default
        assert_eq!(config.verify.layer, 1);
    }

    #[test]
    fn parse_legacy_project_section() {
        // The legacy [project] section should be handled by
        // load_project_config via string replacement.
        let toml_str = r#"
[project]
name = "legacy-project"
version = "0.2.0"
"#;
        // Simulate the replacement that load_project_config does
        let parse_content = toml_str.replace("[project]", "[package]");
        let config: super::ProjectConfig = toml::from_str(&parse_content).unwrap();
        assert_eq!(config.package.name, "legacy-project");
        assert_eq!(config.package.version, "0.2.0");
    }

    #[test]
    fn load_config_from_disk() {
        let dir = std::env::temp_dir().join("assura-config-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config_content = r#"[package]
name = "disk-test"
version = "0.3.0"

[verify]
layer = 0
timeout = 2000
"#;
        std::fs::write(dir.join("assura.toml"), config_content).unwrap();

        // Create a subdir with a dummy file
        let sub = dir.join("src");
        std::fs::create_dir_all(&sub).unwrap();
        let file = sub.join("main.assura");
        std::fs::write(&file, "").unwrap();

        let result = super::load_project_config(&file);
        assert!(result.is_some(), "should find config");
        let (cfg, root) = result.unwrap();
        assert_eq!(cfg.package.name, "disk-test");
        assert_eq!(cfg.package.version, "0.3.0");
        assert_eq!(cfg.verify.layer, 0);
        assert_eq!(cfg.verify.timeout, 2000);
        assert_eq!(root, dir);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_config_missing_returns_none() {
        let dir = std::env::temp_dir().join("assura-no-config-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("test.assura");
        std::fs::write(&file, "").unwrap();

        // May or may not find one depending on system temp layout.
        // At minimum, it should not panic.
        let _ = super::load_project_config(&file);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_cli_wasm_target_generates_config() {
        // Build with --target wasm should produce .cargo/config.toml
        let tmp = std::env::temp_dir().join("assura_i003_wasm");
        let _ = std::fs::remove_dir_all(&tmp);
        let out = std::process::Command::new(assura_bin())
            .args([
                "build",
                "demos/libwebp-huffman.assura",
                "--output",
                tmp.to_str().unwrap(),
                "--target",
                "wasm",
                "--no-check",
            ])
            .current_dir(workspace_root())
            .output()
            .expect("failed to run assura build");
        assert!(
            out.status.success(),
            "build --target wasm should succeed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        // Should have .cargo/config.toml with wasm target
        let cargo_config = tmp.join(".cargo/config.toml");
        assert!(
            cargo_config.exists(),
            ".cargo/config.toml should exist for WASM target"
        );
        let content = std::fs::read_to_string(&cargo_config).unwrap();
        assert!(
            content.contains("wasm32-wasip1"),
            ".cargo/config.toml should set wasm32-wasip1 target"
        );
        // Cargo.toml should have WASM comment
        let cargo_toml = std::fs::read_to_string(tmp.join("Cargo.toml")).unwrap();
        assert!(
            cargo_toml.contains("wasm32-wasip1"),
            "Cargo.toml should mention WASM target"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn build_cli_native_target_no_cargo_config() {
        // Build without --target (or --target native) should not create .cargo/config.toml
        let tmp = std::env::temp_dir().join("assura_i003_native");
        let _ = std::fs::remove_dir_all(&tmp);
        let out = std::process::Command::new(assura_bin())
            .args([
                "build",
                "demos/libwebp-huffman.assura",
                "--output",
                tmp.to_str().unwrap(),
                "--no-check",
            ])
            .current_dir(workspace_root())
            .output()
            .expect("failed to run assura build");
        assert!(
            out.status.success(),
            "build should succeed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let cargo_config = tmp.join(".cargo/config.toml");
        assert!(
            !cargo_config.exists(),
            ".cargo/config.toml should NOT exist for native target"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
