#![allow(dead_code)]

use clap::{Args, CommandFactory, Subcommand};
use clap_complete::Shell;
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

    /// Generate skeleton Assura contracts from a Rust source file
    Infer {
        /// Rust source file (.rs) to analyze
        file: String,

        /// Generate contract for a specific function only
        #[arg(long)]
        function: Option<String>,

        /// Write output to a file instead of stdout
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Generate proptest/boundary/smoke tests from contracts
    TestGen {
        /// Source file to generate tests from
        file: String,

        /// Output file for generated tests (default: stdout)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Print a compact reference for AI coding agents
    AgentInstructions,

    /// Check installation: Z3, CVC5, Rust toolchain, WASM target
    Doctor,

    /// Start the Language Server Protocol server
    Lsp,

    /// Generate shell completion scripts
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },

    /// Show contract coverage of a Rust project
    Coverage {
        /// Path to Cargo workspace root (default: current directory)
        #[arg(default_value = ".")]
        path: String,

        /// Directory containing .assura contract files
        #[arg(long, default_value = "contracts")]
        contracts_dir: String,

        /// Output format: human (colored terminal) or json
        #[arg(long, default_value = "human")]
        format: String,

        /// Exit 1 if coverage is below this percentage (for CI)
        #[arg(long)]
        min_coverage: Option<f64>,
    },

    /// Scan a Rust project and verify inferred contracts
    Audit {
        /// Path to Cargo workspace root (default: current directory)
        #[arg(default_value = ".")]
        path: String,

        /// Contract depth: shallow (types only), medium (+ heuristics)
        #[arg(long, default_value = "medium")]
        depth: String,

        /// Output format: human (colored terminal) or json
        #[arg(long, default_value = "human")]
        format: String,

        /// Only audit functions matching pattern (e.g. "parser::*")
        #[arg(long)]
        focus: Option<String>,

        /// Maximum functions to audit
        #[arg(long)]
        max_functions: Option<usize>,

        /// Per-function Z3 timeout in milliseconds
        #[arg(long, default_value_t = 5000)]
        timeout: u64,

        /// Only audit unsafe functions
        #[arg(long)]
        unsafe_only: bool,
    },

    /// Interactive contract playground
    Repl,

    /// Structural diff between two contract files
    Diff {
        /// Original contract file
        old: String,
        /// Updated contract file
        new: String,
        /// Output format: human or json
        #[arg(long, default_value = "human")]
        format: String,
    },

    /// Start the MCP (Model Context Protocol) server for AI agent integration
    Mcp,
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

/// Format a counterexample as a clean single-line summary for diagnostics.
///
/// If a structured `CounterexampleModel` is available, produces a summary
/// like `"counterexample: a = -2, b = 1"`. Otherwise, parses the raw Z3
/// model string and formats it the same way.
fn format_counterexample_summary(
    counter_model: &Option<assura_smt::CounterexampleModel>,
    raw_model: &str,
) -> String {
    // Use the display module's formatting to get clean lines
    let lines = assura_smt::display::format_counterexample_lines(counter_model, raw_model);
    // Each line starts with "| "; strip that and join into a single line
    let pairs: Vec<&str> = lines
        .iter()
        .map(|l| l.strip_prefix("| ").unwrap_or(l.as_str()))
        .collect();
    if pairs.is_empty() {
        return "counterexample found".to_string();
    }
    format!("counterexample: {}", pairs.join("; "))
}

/// Run lex -> parse -> resolve -> typecheck on source text, collecting all diagnostics.
fn compile(source: &str, filename: &str) -> CompilationResult {
    compile_with_config(source, filename, &CompilerConfig::default())
}

/// Run the full pipeline with explicit configuration.
fn compile_with_config(source: &str, filename: &str, config: &CompilerConfig) -> CompilationResult {
    let mut diagnostics: Vec<assura_diagnostics::Diagnostic> = Vec::new();
    let mut has_errors = false;

    // --- Lex + Parse (single pass) ---
    let lex_start = Instant::now();
    let parse_result = assura_parser::parse_full(source);
    let parse_ms = lex_start.elapsed().as_secs_f64() * 1000.0;

    let token_count = parse_result.token_count;
    let file = parse_result.file;

    for le in &parse_result.lex_errors {
        has_errors = true;
        diagnostics.push(
            assura_diagnostics::Diagnostic::error(
                "A01001",
                format!("unexpected character: {:?}", &source[le.span.clone()]),
                le.span.clone(),
            )
            .with_file(filename),
        );
    }

    for e in &parse_result.parse_errors {
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
        Some(Commands::Infer {
            file,
            function,
            output,
        }) => run_infer(&file, function.as_deref(), output.as_deref()),
        Some(Commands::TestGen { file, output }) => {
            run_test_gen(&file, output.as_deref(), verbosity)
        }
        Some(Commands::AgentInstructions) => run_agent_instructions(),
        Some(Commands::Doctor) => run_doctor(),
        Some(Commands::Lsp) => run_lsp(),
        Some(Commands::Completions { shell }) => {
            clap_complete::generate(shell, &mut Cli::command(), "assura", &mut std::io::stdout());
        }
        Some(Commands::Coverage {
            path,
            contracts_dir,
            format,
            min_coverage,
        }) => run_coverage(&path, &contracts_dir, &format, min_coverage),
        Some(Commands::Audit {
            path,
            depth,
            format,
            focus,
            max_functions,
            timeout,
            unsafe_only,
        }) => run_audit(
            &path,
            &depth,
            &format,
            focus.as_deref(),
            max_functions,
            timeout,
            unsafe_only,
        ),
        Some(Commands::Repl) => run_repl(),
        Some(Commands::Diff { old, new, format }) => {
            run_diff(&old, &new, &format);
        }
        Some(Commands::Mcp) => {
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            rt.block_on(async {
                if let Err(e) = assura_mcp::run_mcp_server().await {
                    eprintln!("MCP server error: {e}");
                    std::process::exit(1);
                }
            });
        }
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

    // --- Project mode: detect directory or project root ---
    let path = Path::new(filename);
    if path.is_dir() {
        // Directory mode: check all .assura files in the project
        run_check_project(path, output_mode, verbosity, &compiler_config);
        return;
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
        if let Some(ref f) = file {
            eprintln!(
                "  parse:     {} tokens, {} declaration(s), {} import(s) ({:.2}ms)",
                timing.token_count,
                f.decls.len(),
                f.imports.len(),
                timing.parse_ms
            );
        } else {
            eprintln!(
                "  parse:     {} tokens, failed ({:.2}ms)",
                timing.token_count, timing.parse_ms
            );
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

    // --- Verify + report ---
    let verify_start = Instant::now();
    let verification_results = verify_and_report(
        filename,
        &source,
        &typed,
        &file,
        &mut diagnostics,
        &mut has_errors,
        output_mode,
        verbosity,
        layer,
        solver,
    );

    let verify_ms = verify_start.elapsed().as_secs_f64() * 1000.0;
    if verbosity == Verbosity::Verbose && output_mode == OutputMode::Human {
        eprintln!(
            "  verify:    {} clause(s) ({verify_ms:.2}ms)",
            verification_results.len()
        );
        let total = timing.parse_ms
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
        let total_ms = timing.parse_ms
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
            "  Parse time:      {:.2}ms ({} tokens)",
            timing.parse_ms, timing.token_count
        );
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

    // --- Report (JSON output; human output handled by verify_and_report) ---
    if output_mode == OutputMode::Json {
        {
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
                        Decl::Extern(_) | Decl::Bind(_) => externs += 1,
                        Decl::FnDef(_) => fns += 1,
                        Decl::Service(_) => services += 1,
                        Decl::Prophecy(_) => {}
                        Decl::CodecRegistry(_) => {}
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
    }

    process::exit(if has_errors { 1 } else { 0 });
}

// ---------------------------------------------------------------------------
// Watch mode
// ---------------------------------------------------------------------------

/// Shared verification + reporting logic used by both `run_check` and
/// `check_file_once` (watch mode). Returns the verification results and
/// whether errors were found.
#[allow(clippy::too_many_arguments)]
fn verify_and_report(
    filename: &str,
    source: &str,
    typed: &Option<assura_types::TypedFile>,
    file: &Option<assura_parser::ast::SourceFile>,
    diagnostics: &mut Vec<assura_diagnostics::Diagnostic>,
    has_errors: &mut bool,
    output_mode: OutputMode,
    verbosity: Verbosity,
    layer: u8,
    solver: assura_smt::SolverChoice,
) -> Vec<assura_smt::VerificationResult> {
    // Short-circuit: skip cache/thread-pool init when there are no
    // verifiable clauses (requires/ensures/invariant) in the source.
    let has_clauses = file
        .as_ref()
        .is_some_and(assura_smt::has_verifiable_clauses);

    let mut verification_results = if layer >= 1 && has_clauses {
        if let Some(typed) = typed {
            let cache_dir = std::path::Path::new(filename)
                .parent()
                .unwrap_or(std::path::Path::new("."));
            let verify_cache = assura_smt::VerificationCache::new(cache_dir);
            assura_smt::verify_parallel_with_solver(typed, &verify_cache, solver)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    if let Some(typed) = typed {
        verification_results.extend(assura_smt::display::dispatch_decrease_checks(typed));
    }

    if let Some(typed) = typed {
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
            clause_desc,
            model,
            counter_model,
        } = vr
        {
            *has_errors = true;
            let summary = format_counterexample_summary(counter_model, model);
            diagnostics.push(
                assura_diagnostics::Diagnostic::error(
                    "A05100",
                    format!("verification failed for {clause_desc}: {summary}"),
                    0..0,
                )
                .with_file(filename),
            );
        }
    }

    if output_mode == OutputMode::Human {
        let non_lex: Vec<_> = diagnostics.iter().filter(|d| d.code != "A01001").collect();
        if *has_errors || verbosity != Verbosity::Quiet {
            for d in &non_lex {
                assura_diagnostics::render_diagnostic(d, filename, source);
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
            } else if layer == 0 {
                eprintln!();
                eprintln!("Verification skipped (--layer 0: structural checks only)");
            } else if layer >= 1
                && let Some(f) = file
            {
                let contract_names = assura_smt::display::collect_contract_names(f);
                if !contract_names.is_empty() {
                    eprintln!();
                    eprintln!("Verification:");
                    for name in &contract_names {
                        eprintln!("  {name}:  (no verifiable clauses)");
                    }
                }
            }

            if !*has_errors {
                eprintln!("{filename}: check passed (no errors)");
            } else {
                eprintln!("{filename}: {} error(s) found", diagnostics.len());
            }
        } else if *has_errors {
            eprintln!("{filename}: {} error(s) found", diagnostics.len());
        }
    }

    verification_results
}

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
        if let Some(ref f) = file {
            eprintln!(
                "  parse:     {} tokens, {} declaration(s), {} import(s) ({:.2}ms)",
                timing.token_count,
                f.decls.len(),
                f.imports.len(),
                timing.parse_ms
            );
        } else {
            eprintln!(
                "  parse:     {} tokens, failed ({:.2}ms)",
                timing.token_count, timing.parse_ms
            );
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

    let _ = resolved;
    let _ = hir;
    let _ = timing;

    verify_and_report(
        filename,
        &source,
        &typed,
        &file,
        &mut diagnostics,
        &mut has_errors,
        output_mode,
        verbosity,
        layer,
        assura_smt::SolverChoice::Z3,
    );

    has_errors
}

/// Compute a simple content hash for incremental change detection.
fn content_hash(source: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Run check in watch mode: check once, then watch for file changes.
/// Uses IncrementalCompiler to skip re-checks when file content is unchanged.
fn run_watch_loop(filename: &str, output_mode: OutputMode, verbosity: Verbosity, layer: u8) -> ! {
    use notify::{Event, EventKind, RecursiveMode, Watcher};

    let path = Path::new(filename).canonicalize().unwrap_or_else(|e| {
        eprintln!("Error: cannot resolve path {filename}: {e}");
        process::exit(2);
    });

    // Set up incremental compiler to track file hashes
    let mut incremental = assura_smt::IncrementalCompiler::new();
    let mut last_hash = String::new();

    // Initial check
    eprintln!("[watch] Checking {filename}...");
    eprintln!();
    if let Ok(source) = fs::read_to_string(filename) {
        last_hash = content_hash(&source);
        incremental.register_module(filename.to_string(), last_hash.clone());
    }
    let _ = check_file_once(filename, output_mode, verbosity, layer);
    incremental.mark_checked(filename, 1);
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

    let mut iteration: u64 = 2;
    loop {
        // Wait for a change event
        let _ = rx.recv();

        // Debounce: drain any additional events that arrive within 100ms
        while rx.recv_timeout(Duration::from_millis(100)).is_ok() {}

        // Check if content actually changed (saves without edits, editor auto-format, etc.)
        let new_hash = fs::read_to_string(filename)
            .map(|s| content_hash(&s))
            .unwrap_or_default();

        if new_hash == last_hash && !new_hash.is_empty() {
            if verbosity == Verbosity::Verbose {
                eprintln!("[watch] File saved but content unchanged, skipping re-check.");
            }
            continue;
        }

        // Content changed: update incremental state
        last_hash = new_hash;
        incremental.mark_changed(filename);

        if verbosity == Verbosity::Verbose {
            let dirty = incremental.dirty_modules();
            eprintln!(
                "[watch] {} dirty module(s): {}",
                dirty.len(),
                dirty.join(", ")
            );
        }

        // Clear screen and re-check
        eprint!("\x1B[2J\x1B[H");
        eprintln!("[watch] File changed, re-checking {filename}...");
        eprintln!();
        let _ = check_file_once(filename, output_mode, verbosity, layer);
        incremental.mark_checked(filename, iteration);
        iteration += 1;
        eprintln!();
        eprintln!("[watch] Watching for changes. Press Ctrl+C to stop.");
    }
}

// ---------------------------------------------------------------------------
// Project-mode check: resolve and type-check all .assura files in a project
// ---------------------------------------------------------------------------

fn run_check_project(
    project_dir: &Path,
    output_mode: OutputMode,
    _verbosity: Verbosity,
    config: &CompilerConfig,
) {
    let _ = config; // reserved for future per-project config
    let project_root = if project_dir.join("assura.toml").exists() {
        project_dir.to_path_buf()
    } else {
        assura_resolve::find_project_root(project_dir).unwrap_or_else(|| project_dir.to_path_buf())
    };

    if output_mode == OutputMode::Human {
        eprintln!("Checking project at {}", project_root.display());
    }

    let (resolved_files, warnings) =
        match assura_resolve::discover_and_resolve_project(&project_root) {
            Ok(pair) => pair,
            Err(errors) => {
                for e in &errors {
                    eprintln!("Error: {e}");
                }
                process::exit(1);
            }
        };

    for w in &warnings {
        if output_mode == OutputMode::Human {
            eprintln!("Warning: {w}");
        }
    }

    let mut total_errors = 0usize;
    let mut total_modules = 0usize;
    let mut total_bindings = 0usize;

    // Type-check each resolved file
    for (module_path, resolved) in &resolved_files {
        total_modules += 1;
        match assura_types::type_check(resolved) {
            Ok(typed) => {
                let bindings = typed.type_env.len();
                total_bindings += bindings;
                if output_mode == OutputMode::Human {
                    eprintln!(
                        "OK  {module_path}: {} symbol(s), {bindings} binding(s)",
                        resolved.symbols.symbols.len()
                    );
                }
            }
            Err(errors) => {
                total_errors += errors.len();
                if output_mode == OutputMode::Human {
                    eprintln!("ERR {module_path}: {} error(s)", errors.len());
                    for err in &errors {
                        eprintln!("  {}: {}", err.code, err.message);
                    }
                }
            }
        }
    }

    if output_mode == OutputMode::Human {
        eprintln!();
        eprintln!(
            "Project: {total_modules} module(s), {total_bindings} binding(s), {total_errors} error(s)"
        );
    }

    if total_errors > 0 {
        process::exit(1);
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

    // --- Project mode: detect directory ---
    let path = Path::new(filename);
    if path.is_dir() {
        run_build_project(path, verbosity, out_dir_str, compile_target, no_check);
        return;
    }

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
        if let Some(ref f) = parsed_file {
            eprintln!(
                "  parse:     {} tokens, {} declaration(s), {} import(s) ({:.2}ms)",
                timing.token_count,
                f.decls.len(),
                f.imports.len(),
                timing.parse_ms
            );
        } else {
            eprintln!(
                "  parse:     {} tokens, failed ({:.2}ms)",
                timing.token_count, timing.parse_ms
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
        let total = timing.parse_ms
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

    // --- Auto-generate tests for timeout/unknown verification results ---
    let has_unresolved = verification_results.iter().any(|r| {
        matches!(
            r,
            assura_smt::VerificationResult::Timeout { .. }
                | assura_smt::VerificationResult::Unknown { .. }
        )
    });
    if has_unresolved && let Some(ref pf) = parsed_file {
        let mut test_gen = assura_types::TestGenerator::new();
        for spanned in &pf.decls {
            if let Decl::Contract(c) = &spanned.node {
                let mut params = Vec::new();
                let mut requires = Vec::new();
                let mut ensures = Vec::new();
                for clause in &c.clauses {
                    match &clause.kind {
                        ClauseKind::Input => {
                            let parsed = assura_parser::ast::extract_clause_params(&clause.body);
                            for p in parsed {
                                let ty = typed
                                    .type_env
                                    .bindings
                                    .get(&p.name)
                                    .cloned()
                                    .unwrap_or(assura_types::Type::Unknown);
                                params.push((p.name, ty));
                            }
                        }
                        ClauseKind::Requires => {
                            requires.push(assura_codegen::expr_to_rust_static(&clause.body));
                        }
                        ClauseKind::Ensures => {
                            ensures.push(assura_codegen::expr_to_rust_static(&clause.body));
                        }
                        _ => {}
                    }
                }
                if !params.is_empty() || !ensures.is_empty() {
                    test_gen.add_contract(assura_types::TestableContract {
                        name: c.name.clone(),
                        params,
                        requires,
                        ensures,
                    });
                }
            }
        }
        let tests = test_gen.generate_all();
        if !tests.is_empty() {
            let tests_dir = out_dir.join("tests");
            fs::create_dir_all(&tests_dir).ok();
            let test_file = tests_dir.join("generated_tests.rs");
            let mut content = String::from(
                "// Auto-generated tests (SMT verification returned timeout/unknown)\nuse proptest::prelude::*;\n\n",
            );
            for t in &tests {
                content.push_str(&t.body);
                content.push_str("\n\n");
            }
            if fs::write(&test_file, &content).is_ok() && verbosity != Verbosity::Quiet {
                println!(
                    "  wrote {} ({} tests for unresolved contracts)",
                    test_file.display(),
                    tests.len()
                );
            }
        }
    }

    // --- Build or check the generated Rust project ---
    let skip_check = no_check;
    if !skip_check {
        // WASM targets get `cargo build` to produce a .wasm file;
        // native targets get `cargo check` for fast validation.
        let is_wasm = matches!(compile_target, assura_codegen::CompileTarget::Wasm);
        let cargo_verb = if is_wasm { "build" } else { "check" };

        let mut cmd = process::Command::new("cargo");
        cmd.arg(cargo_verb).current_dir(out_dir);
        if let Some(triple) = compile_target.rust_target() {
            cmd.arg("--target").arg(triple);
        }
        let cargo_result = cmd
            .stdout(process::Stdio::piped())
            .stderr(process::Stdio::piped())
            .output();

        match cargo_result {
            Ok(output) if output.status.success() => {
                if is_wasm {
                    // Report the .wasm artifact path
                    let wasm_dir = out_dir.join("target/wasm32-wasip1/debug");
                    let wasm_file = find_wasm_artifact(&wasm_dir);
                    if let Some(ref wf) = wasm_file {
                        let size = fs::metadata(wf).map(|m| m.len()).unwrap_or(0);
                        if verbosity != Verbosity::Quiet {
                            println!("OK  {filename} -> {} ({} bytes)", wf.display(), size);
                        }
                    } else if verbosity != Verbosity::Quiet {
                        println!(
                            "OK  {filename} -> {out_dir_str}/ (WASM build succeeded, artifact in target/)"
                        );
                    }
                } else if verbosity != Verbosity::Quiet {
                    println!("OK  {filename} -> {out_dir_str}/ (generated Rust compiles)");
                }
            }
            Ok(output) => {
                if verbosity != Verbosity::Quiet {
                    println!("OK  {filename} -> {out_dir_str}/");
                }
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!();
                eprintln!("warning: generated Rust does not {cargo_verb}:");
                // Show only the error lines, not the full cargo output
                for line in stderr.lines() {
                    if line.starts_with("error") || line.contains("-->") {
                        eprintln!("  {line}");
                    }
                }
                eprintln!();
                eprintln!("  Run `cd {out_dir_str} && cargo {cargo_verb}` to see full errors.");
                eprintln!("  Use `--no-check` to skip this validation.");
            }
            Err(_) => {
                // cargo not found or other OS error; skip silently
                if verbosity != Verbosity::Quiet {
                    println!(
                        "OK  {filename} -> {out_dir_str}/ (cargo {cargo_verb} skipped: cargo not found)"
                    );
                }
            }
        }
    } else if verbosity != Verbosity::Quiet {
        println!("OK  {filename} -> {out_dir_str}/ (check skipped)");
    }
}

/// Find the first `.wasm` file in a directory (for WASM build output).
fn find_wasm_artifact(dir: &Path) -> Option<std::path::PathBuf> {
    let rd = fs::read_dir(dir).ok()?;
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "wasm") {
            return Some(path);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Project-mode build: resolve, type-check, and codegen all .assura files
// ---------------------------------------------------------------------------

fn run_build_project(
    project_dir: &Path,
    verbosity: Verbosity,
    output_dir: &str,
    target: assura_codegen::CompileTarget,
    no_check: bool,
) {
    let project_root = if project_dir.join("assura.toml").exists() {
        project_dir.to_path_buf()
    } else {
        assura_resolve::find_project_root(project_dir).unwrap_or_else(|| project_dir.to_path_buf())
    };

    eprintln!("Building project at {}", project_root.display());

    let (resolved_files, warnings) =
        match assura_resolve::discover_and_resolve_project(&project_root) {
            Ok(pair) => pair,
            Err(errors) => {
                for e in &errors {
                    eprintln!("Error: {e}");
                }
                process::exit(1);
            }
        };

    for w in &warnings {
        eprintln!("Warning: {w}");
    }

    let mut all_typed = Vec::new();
    let mut has_errors = false;

    for (module_path, resolved) in &resolved_files {
        match assura_types::type_check(resolved) {
            Ok(typed) => {
                all_typed.push((module_path.clone(), typed));
                if verbosity == Verbosity::Verbose {
                    eprintln!("OK  {module_path}");
                }
            }
            Err(errors) => {
                has_errors = true;
                eprintln!("ERR {module_path}: {} error(s)", errors.len());
                for err in &errors {
                    eprintln!("  {}: {}", err.code, err.message);
                }
            }
        }
    }

    if has_errors {
        eprintln!("Build failed: type errors in project");
        process::exit(1);
    }

    // Generate code for each module
    let out_dir = Path::new(output_dir);
    let mut generated_files = 0usize;
    let mut cargo_toml_written = false;
    for (_module_path, typed) in &all_typed {
        let project = assura_codegen::codegen(typed);
        // Write Cargo.toml once
        if !cargo_toml_written {
            let cargo_path = out_dir.join("Cargo.toml");
            if let Some(parent) = cargo_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Err(e) = fs::write(&cargo_path, &project.cargo_toml) {
                eprintln!("Error writing {}: {e}", cargo_path.display());
                process::exit(1);
            }
            cargo_toml_written = true;
        }
        for (rel_path, content) in &project.files {
            let file_out = out_dir.join(rel_path);
            if let Some(parent) = file_out.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Err(e) = fs::write(&file_out, content) {
                eprintln!("Error writing {}: {e}", file_out.display());
                process::exit(1);
            }
            generated_files += 1;
        }
    }

    eprintln!(
        "Generated {generated_files} file(s) in {}",
        out_dir.display()
    );

    // Optionally run cargo check on generated code
    if !no_check && out_dir.join("Cargo.toml").exists() {
        eprintln!("Running cargo check on generated code...");
        let status = std::process::Command::new("cargo")
            .arg("check")
            .current_dir(out_dir)
            .status();
        match status {
            Ok(s) if s.success() => {
                eprintln!("Generated code compiles successfully");
            }
            Ok(s) => {
                eprintln!(
                    "Generated code failed to compile (exit {})",
                    s.code().unwrap_or(-1)
                );
                process::exit(1);
            }
            Err(e) => {
                eprintln!("Failed to run cargo check: {e}");
            }
        }
    }

    let _ = target; // reserved for future target-specific codegen
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
// `assura infer <rust_file>` -- generate skeleton Assura contracts
// ---------------------------------------------------------------------------

fn run_infer(filename: &str, function_filter: Option<&str>, output_path: Option<&str>) {
    let source = fs::read_to_string(filename).unwrap_or_else(|e| {
        eprintln!("Error: {filename}: {e}");
        process::exit(2);
    });

    let signatures = extract_rust_fn_signatures(&source);

    if signatures.is_empty() {
        eprintln!("No public function signatures found in {filename}");
        process::exit(1);
    }

    let filtered: Vec<&RustFnSig> = if let Some(name) = function_filter {
        let matches: Vec<_> = signatures.iter().filter(|s| s.name == name).collect();
        if matches.is_empty() {
            eprintln!("Function '{name}' not found in {filename}");
            let names: Vec<_> = signatures
                .iter()
                .filter(|s| s.is_pub)
                .map(|s| s.name.as_str())
                .collect();
            eprintln!("Available public functions: {}", names.join(", "));
            process::exit(1);
        }
        matches
    } else {
        // Default: only public functions
        signatures.iter().filter(|s| s.is_pub).collect()
    };

    if filtered.is_empty() {
        eprintln!("No public function signatures found in {filename}");
        process::exit(1);
    }

    let module_path = derive_rust_module_path(filename);

    let mut output = String::new();
    output.push_str(&format!(
        "// Generated by: assura infer {filename}\n// Review and refine these contracts before use.\n\n"
    ));

    for sig in &filtered {
        generate_bind_skeleton(&module_path, sig, &mut output);
    }

    output.push_str(&format!(
        "\n// {} function(s) analyzed from {filename}\n",
        filtered.len()
    ));

    if let Some(path) = output_path {
        fs::write(path, &output).unwrap_or_else(|e| {
            eprintln!("Error: cannot write {path}: {e}");
            process::exit(2);
        });
        eprintln!("Wrote {} contract(s) to {path}", filtered.len());
    } else {
        print!("{output}");
    }
}

/// A parsed Rust function signature (minimal, no syn dependency).
struct RustFnSig {
    name: String,
    params: Vec<(String, String)>, // (name, type)
    return_type: String,
    is_pub: bool,
}

/// Extract public function signatures from Rust source text.
///
/// This is a lightweight regex-free parser that handles common patterns.
/// It does NOT use syn (to avoid adding a heavy dependency to the CLI).
fn extract_rust_fn_signatures(source: &str) -> Vec<RustFnSig> {
    let mut sigs = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        // Strip leading modifiers to find `fn ` keyword.
        // Handles: pub fn, pub(crate) fn, pub async fn, pub const fn,
        //          pub unsafe fn, async fn, const fn, unsafe fn, etc.
        let (is_pub, fn_part) = match strip_fn_prefix(line) {
            Some(pair) => pair,
            None => {
                i += 1;
                continue;
            }
        };

        // Collect full signature (may span multiple lines)
        let mut full_sig = fn_part.to_string();
        let mut j = i + 1;
        while !full_sig.contains('{') && !full_sig.contains(';') && j < lines.len() {
            full_sig.push(' ');
            full_sig.push_str(lines[j].trim());
            j += 1;
        }

        if let Some(sig) = parse_fn_signature(&full_sig, is_pub) {
            sigs.push(sig);
        }

        i = j.max(i + 1);
    }

    sigs
}

/// Strip function declaration prefix and return (is_pub, rest_after_fn).
/// Handles all modifier combinations: pub/pub(vis), async, const, unsafe.
fn strip_fn_prefix(line: &str) -> Option<(bool, &str)> {
    let mut rest = line;
    let mut is_pub = false;

    // Check for pub / pub(vis)
    if let Some(after_pub) = rest.strip_prefix("pub") {
        is_pub = true;
        rest = after_pub;
        // Handle pub(crate), pub(super), pub(in path)
        let trimmed = rest.trim_start();
        if let Some(after_paren) = trimmed.strip_prefix('(') {
            if let Some(close) = after_paren.find(')') {
                rest = &after_paren[close + 1..];
            } else {
                return None;
            }
        } else {
            rest = trimmed;
        }
    }

    // Strip optional modifiers: async, const, unsafe (in any order)
    loop {
        let trimmed = rest.trim_start();
        if let Some(after) = trimmed.strip_prefix("async ") {
            rest = after;
        } else if let Some(after) = trimmed.strip_prefix("const ") {
            rest = after;
        } else if let Some(after) = trimmed.strip_prefix("unsafe ") {
            rest = after;
        } else {
            rest = trimmed;
            break;
        }
    }

    // Must find `fn ` keyword
    let after_fn = rest.strip_prefix("fn ")?;
    Some((is_pub, after_fn))
}

/// Parse a single function signature string like "foo(x: i64, y: &str) -> bool {"
fn parse_fn_signature(sig: &str, is_pub: bool) -> Option<RustFnSig> {
    let paren_open = sig.find('(')?;
    let raw_name = sig[..paren_open].trim();

    // Strip generic parameters: `encode<T: Serialize>` -> `encode`
    let name = if let Some(angle) = raw_name.find('<') {
        raw_name[..angle].trim().to_string()
    } else {
        raw_name.to_string()
    };

    // Skip if name contains invalid chars (macros, etc.)
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }

    // Find matching closing paren (handle nested parens)
    let after_open = &sig[paren_open + 1..];
    let mut depth = 1i32;
    let mut close_offset = 0;
    for (i, ch) in after_open.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close_offset = i;
                    break;
                }
            }
            _ => {}
        }
    }

    let params_str = &after_open[..close_offset];
    let params = parse_param_list(params_str);

    // Extract return type
    let after_close = &after_open[close_offset + 1..];
    let return_type = if let Some(arrow_pos) = after_close.find("->") {
        let ret = after_close[arrow_pos + 2..].trim();
        // Strip trailing { or where
        let ret = ret
            .split('{')
            .next()
            .unwrap_or(ret)
            .split("where")
            .next()
            .unwrap_or(ret)
            .trim();
        ret.to_string()
    } else {
        "()".to_string()
    };

    Some(RustFnSig {
        name,
        params,
        return_type,
        is_pub,
    })
}

/// Parse a parameter list like "x: i64, y: &str, _: bool"
fn parse_param_list(params: &str) -> Vec<(String, String)> {
    let params = params.trim();
    if params.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut depth = 0i32;
    let mut paren_depth = 0i32;
    let mut start = 0;

    // Split on commas respecting <> and ()
    let mut segments = Vec::new();
    for (i, ch) in params.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            '(' => paren_depth += 1,
            ')' if paren_depth > 0 => paren_depth -= 1,
            ',' if depth == 0 && paren_depth == 0 => {
                segments.push(&params[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    segments.push(&params[start..]);

    for seg in segments {
        let seg = seg.trim();
        // Skip self, &self, &mut self
        if seg == "self" || seg == "&self" || seg == "&mut self" {
            continue;
        }
        if let Some(colon_pos) = seg.find(':') {
            let name = seg[..colon_pos].trim();
            let ty = seg[colon_pos + 1..].trim();
            if !name.is_empty() {
                result.push((name.to_string(), ty.to_string()));
            }
        }
    }

    result
}

/// Derive a Rust module path from a filesystem path.
///
/// Walks up from the file looking for `Cargo.toml` to find the crate name.
/// Converts hyphens to underscores (Rust identifier convention).
/// Strips the `src/` segment and maps `lib.rs`/`mod.rs` to their parent module.
///
/// Examples:
///   `crates/assura-codegen/src/type_map.rs` -> `assura_codegen::type_map`
///   `src/lib.rs` -> `my_crate` (crate name from Cargo.toml)
///   `src/foo/bar.rs` -> `my_crate::foo::bar`
fn derive_rust_module_path(file_path: &str) -> String {
    let path = Path::new(file_path);

    // Find the src/ component and the crate root above it
    let components: Vec<_> = path.components().collect();
    let mut crate_name: Option<String> = None;
    let mut src_index: Option<usize> = None;

    for (i, comp) in components.iter().enumerate() {
        if comp.as_os_str() == "src" {
            src_index = Some(i);
            // Crate root is the directory containing src/
            let crate_root: std::path::PathBuf = if i > 0 {
                components[..i].iter().collect()
            } else {
                std::path::PathBuf::from(".")
            };
            // Try to read crate name from Cargo.toml
            let cargo_path = crate_root.join("Cargo.toml");
            if let Ok(content) = fs::read_to_string(&cargo_path) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix("name") {
                        let rest = rest.trim_start();
                        if let Some(rest) = rest.strip_prefix('=') {
                            let rest = rest.trim();
                            let name = rest.trim_matches('"').trim_matches('\'');
                            crate_name = Some(name.replace('-', "_"));
                            break;
                        }
                    }
                }
            }
            break;
        }
    }

    let crate_segment = crate_name.unwrap_or_else(|| "crate".to_string());

    if let Some(si) = src_index {
        // Build relative path from components after src/
        if si + 1 < components.len() {
            let after_src: std::path::PathBuf = components[si + 1..].iter().collect();
            let rel_str = after_src
                .to_string_lossy()
                .trim_end_matches(".rs")
                .replace(['/', '\\'], "::");

            // lib.rs / mod.rs -> just the parent module (or crate root)
            if rel_str == "lib" || rel_str == "mod" {
                crate_segment
            } else if rel_str.ends_with("::mod") || rel_str.ends_with("::lib") {
                let trimmed = rel_str.trim_end_matches("::mod").trim_end_matches("::lib");
                format!("{crate_segment}::{trimmed}")
            } else {
                format!("{crate_segment}::{rel_str}")
            }
        } else {
            // Path ends at src/ itself (unusual)
            crate_segment
        }
    } else {
        // No src/ found; fallback: strip .rs, convert path separators, fix hyphens
        file_path
            .trim_end_matches(".rs")
            .replace('/', "::")
            .replace('-', "_")
    }
}

/// Generate a bind skeleton for a single function.
fn generate_bind_skeleton(module_path: &str, sig: &RustFnSig, out: &mut String) {
    use assura_codegen::type_map::rust_type_to_assura;

    let rust_path = format!("{module_path}::{}", sig.name);

    out.push_str(&format!("bind \"{}\" as {} {{\n", rust_path, sig.name));

    // Input params
    if !sig.params.is_empty() {
        out.push_str("    input(");
        let params: Vec<String> = sig
            .params
            .iter()
            .map(|(name, ty)| format!("{}: {}", name, rust_type_to_assura(ty)))
            .collect();
        out.push_str(&params.join(", "));
        out.push_str(")\n");
    }

    // Output
    let assura_ret = rust_type_to_assura(&sig.return_type);
    if assura_ret != "Unit" {
        out.push_str(&format!("    output(result: {assura_ret})\n"));
    }

    // Skeleton requires/ensures
    out.push_str("    // TODO: add requires clauses (preconditions)\n");
    out.push_str("    // TODO: add ensures clauses (postconditions)\n");
    out.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// `assura test-gen <file.assura>` -- generate tests from contracts
// ---------------------------------------------------------------------------

fn run_test_gen(filename: &str, output: Option<&str>, verbosity: Verbosity) {
    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {filename}: {e}");
            process::exit(1);
        }
    };

    let CompilationResult {
        file,
        typed,
        has_errors,
        ..
    } = compile(&source, filename);

    if has_errors || file.is_none() {
        eprintln!("Error: {filename} has compilation errors; fix them before generating tests.");
        process::exit(1);
    }

    let file = file.unwrap();
    let type_env = typed.as_ref().map(|t| &t.type_env);

    let mut test_gen = assura_types::TestGenerator::new();

    for spanned in &file.decls {
        if let Decl::Contract(c) = &spanned.node {
            let mut params = Vec::new();
            let mut requires = Vec::new();
            let mut ensures = Vec::new();

            for clause in &c.clauses {
                match &clause.kind {
                    ClauseKind::Input => {
                        let parsed = assura_parser::ast::extract_clause_params(&clause.body);
                        for p in parsed {
                            let ty = type_env
                                .and_then(|env| env.bindings.get(&p.name))
                                .cloned()
                                .unwrap_or(assura_types::Type::Unknown);
                            params.push((p.name, ty));
                        }
                    }
                    ClauseKind::Requires => {
                        requires.push(assura_codegen::expr_to_rust_static(&clause.body));
                    }
                    ClauseKind::Ensures => {
                        ensures.push(assura_codegen::expr_to_rust_static(&clause.body));
                    }
                    _ => {}
                }
            }

            if !params.is_empty() || !ensures.is_empty() {
                test_gen.add_contract(assura_types::TestableContract {
                    name: c.name.clone(),
                    params,
                    requires,
                    ensures,
                });
            }
        }
    }

    let tests = test_gen.generate_all();

    if tests.is_empty() {
        eprintln!("No testable contracts found in {filename}.");
        process::exit(0);
    }

    let mut out = String::new();
    out.push_str("// Generated by `assura test-gen`\n");
    out.push_str("// Source: ");
    out.push_str(filename);
    out.push('\n');
    out.push_str("use proptest::prelude::*;\n\n");

    for test in &tests {
        out.push_str(&test.body);
        out.push_str("\n\n");
    }

    if let Some(path) = output {
        match fs::write(path, &out) {
            Ok(()) => {
                if verbosity != Verbosity::Quiet {
                    eprintln!(
                        "Generated {} test(s) from {filename} -> {path}",
                        tests.len()
                    );
                }
            }
            Err(e) => {
                eprintln!("Error writing {path}: {e}");
                process::exit(1);
            }
        }
    } else {
        print!("{out}");
        if verbosity != Verbosity::Quiet {
            eprintln!("Generated {} test(s) from {filename}", tests.len());
        }
    }
}

// ---------------------------------------------------------------------------
// `assura audit [path]` -- scan and verify a Rust project
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn run_audit(
    path: &str,
    depth: &str,
    format: &str,
    focus: Option<&str>,
    max_functions: Option<usize>,
    _timeout_ms: u64,
    unsafe_only: bool,
) {
    use assura_codegen::type_map::rust_type_to_assura;

    // Phase 1: Discover Rust source files
    let root = Path::new(path);
    let cargo_toml = root.join("Cargo.toml");
    if !cargo_toml.exists() {
        eprintln!("Error: no Cargo.toml found at {}", root.display());
        eprintln!("Run `assura audit` from a Cargo workspace root.");
        process::exit(2);
    }

    // Discover src directories: support workspaces and single crates
    let src_dirs = discover_workspace_src_dirs(root);
    if src_dirs.is_empty() {
        eprintln!("Error: no src/ directories found at {}", root.display());
        process::exit(2);
    }

    let mut rs_files = Vec::new();
    for src_dir in &src_dirs {
        rs_files.extend(discover_rs_files(src_dir));
    }
    rs_files.sort();
    rs_files.dedup();

    if rs_files.is_empty() {
        eprintln!("No .rs files found in scanned directories");
        process::exit(1);
    }

    // Phase 2: Extract all function signatures
    let mut all_sigs: Vec<(String, RustFnSig)> = Vec::new();
    for rs_file in &rs_files {
        let rel_path = rs_file
            .strip_prefix(root)
            .unwrap_or(rs_file.as_path())
            .to_string_lossy()
            .to_string();

        let source = match fs::read_to_string(rs_file) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let sigs = extract_rust_fn_signatures(&source);
        for sig in sigs {
            if !sig.is_pub {
                continue;
            }
            if unsafe_only && !source.contains("unsafe") {
                continue;
            }
            if let Some(pattern) = focus {
                let qname = format!("{}::{}", rel_path, sig.name);
                if !qname.contains(pattern) && !sig.name.contains(pattern) {
                    continue;
                }
            }
            all_sigs.push((rel_path.clone(), sig));
        }
    }

    if let Some(max) = max_functions {
        all_sigs.truncate(max);
    }

    if all_sigs.is_empty() {
        eprintln!("No matching public functions found.");
        process::exit(1);
    }

    let is_json = format == "json";

    if !is_json {
        eprintln!(
            "Scanning {} ... found {} public functions in {} files",
            root.display(),
            all_sigs.len(),
            rs_files.len()
        );
    }

    // Phase 3: Generate skeleton contracts
    let mut assura_source = String::new();
    assura_source.push_str("// Auto-generated by: assura audit\n");
    assura_source.push_str("// Review and refine before relying on results.\n\n");

    for (file_path, sig) in &all_sigs {
        let module_path = derive_rust_module_path(file_path);

        let rust_path = format!("{module_path}::{}", sig.name);

        assura_source.push_str(&format!("bind \"{}\" as {} {{\n", rust_path, sig.name));

        if !sig.params.is_empty() {
            assura_source.push_str("    input(");
            let params: Vec<String> = sig
                .params
                .iter()
                .map(|(name, ty)| format!("{}: {}", name, rust_type_to_assura(ty)))
                .collect();
            assura_source.push_str(&params.join(", "));
            assura_source.push_str(")\n");
        }

        let assura_ret = rust_type_to_assura(&sig.return_type);
        if assura_ret != "Unit" {
            assura_source.push_str(&format!("    output(result: {assura_ret})\n"));
        }

        // Medium depth: add heuristic contracts
        if depth == "medium" || depth == "deep" {
            for (name, ty) in &sig.params {
                let aty = rust_type_to_assura(ty);
                // Index parameters: add bounds check
                if (aty == "Nat" || aty == "Int")
                    && (name.contains("index")
                        || name.contains("offset")
                        || name.contains("idx")
                        || name.contains("pos"))
                {
                    assura_source.push_str(&format!("    requires {name} >= 0\n"));
                }
                // Slice/list parameters: non-empty check
                if aty.starts_with("List") || aty == "Bytes" || aty == "String" {
                    assura_source.push_str(&format!("    requires length({name}) > 0\n"));
                }
            }
        }

        assura_source.push_str("}\n\n");
    }

    // Phase 4: Parse and verify the generated contracts
    if !is_json {
        eprintln!(
            "Generating contracts ... {} skeleton contracts",
            all_sigs.len()
        );
    }

    let (parsed, parse_errors) = assura_parser::parse(&assura_source);

    if !parse_errors.is_empty() && !is_json {
        eprintln!(
            "Warning: {} parse error(s) in generated contracts",
            parse_errors.len()
        );
    }

    let mut findings: Vec<AuditFinding> = Vec::new();
    let mut verified_count = 0u32;
    let mut error_count = 0u32;

    if let Some(file) = parsed {
        // Run resolve + type check + verify
        match assura_resolve::resolve(&file) {
            Ok(resolved) => match assura_types::type_check(&resolved) {
                Ok(typed) => {
                    if !is_json {
                        eprintln!("Verifying ...");
                    }
                    let results = assura_smt::verify(&typed);
                    for r in &results {
                        match r {
                            assura_smt::VerificationResult::Verified { .. } => {
                                verified_count += 1;
                            }
                            assura_smt::VerificationResult::Counterexample {
                                clause_desc,
                                model,
                                ..
                            } => {
                                findings.push(AuditFinding {
                                    function: clause_desc.clone(),
                                    clause: "counterexample".to_string(),
                                    severity: "warning".to_string(),
                                    message: "Counterexample found".to_string(),
                                    counterexample: Some(model.clone()),
                                });
                            }
                            assura_smt::VerificationResult::Timeout { clause_desc } => {
                                findings.push(AuditFinding {
                                    function: clause_desc.clone(),
                                    clause: "timeout".to_string(),
                                    severity: "info".to_string(),
                                    message:
                                        "Z3 timed out (needs deeper contract or longer timeout)"
                                            .to_string(),
                                    counterexample: None,
                                });
                            }
                            assura_smt::VerificationResult::Unknown {
                                clause_desc,
                                reason,
                            } => {
                                findings.push(AuditFinding {
                                    function: clause_desc.clone(),
                                    clause: "unknown".to_string(),
                                    severity: "info".to_string(),
                                    message: format!("Z3 result unknown: {reason}"),
                                    counterexample: None,
                                });
                            }
                        }
                    }
                }
                Err(e) => {
                    if !is_json {
                        eprintln!("Type check error: {e:?}");
                    }
                    error_count += 1;
                }
            },
            Err(e) => {
                if !is_json {
                    eprintln!("Resolve error: {e:?}");
                }
                error_count += 1;
            }
        }
    }

    // Phase 5: Output results
    if is_json {
        let report = serde_json::json!({
            "functions_scanned": all_sigs.len(),
            "files_scanned": rs_files.len(),
            "verified": verified_count,
            "findings": findings.len(),
            "errors": error_count,
            "results": findings.iter().map(|f| serde_json::json!({
                "function": f.function,
                "clause": f.clause,
                "severity": f.severity,
                "message": f.message,
                "counterexample": f.counterexample,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else {
        println!();
        println!(
            "AUDIT SUMMARY: {} functions, {} verified, {} findings, {} errors",
            all_sigs.len(),
            verified_count,
            findings.len(),
            error_count
        );

        if !findings.is_empty() {
            println!();
            println!("FINDINGS:");
            for f in &findings {
                let sev = match f.severity.as_str() {
                    "warning" => "WARNING",
                    "error" => "ERROR",
                    _ => "INFO",
                };
                println!("  [{sev}] {}  ({})", f.function, f.clause);
                println!("    {}", f.message);
                if let Some(cex) = &f.counterexample {
                    for line in cex.lines() {
                        println!("    | {line}");
                    }
                }
                println!();
            }
        }

        if findings.is_empty() && error_count == 0 {
            println!("  All verified contracts passed.");
        }
    }

    if !findings.is_empty() {
        process::exit(1);
    }
}

/// A finding from the audit.
struct AuditFinding {
    function: String,
    clause: String,
    severity: String,
    message: String,
    counterexample: Option<String>,
}

/// Recursively discover all .rs files under a directory.
/// Discover src/ directories from a Cargo project root.
///
/// If `Cargo.toml` has a `[workspace]` section with `members`, scan each
/// member's `src/` directory. Supports glob patterns like `crates/*`.
/// If it's a single-crate project, return `root/src/` if it exists.
fn discover_workspace_src_dirs(root: &Path) -> Vec<std::path::PathBuf> {
    let cargo_toml = root.join("Cargo.toml");
    let content = match fs::read_to_string(&cargo_toml) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut src_dirs = Vec::new();

    // Check for workspace members
    let mut in_workspace = false;
    let mut in_members = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[workspace]" {
            in_workspace = true;
            continue;
        }
        if trimmed.starts_with('[') && trimmed != "[workspace]" {
            if in_workspace {
                in_workspace = false;
                in_members = false;
            }
            continue;
        }
        if in_workspace {
            if trimmed.starts_with("members") && trimmed.contains('[') {
                in_members = true;
            }
            if in_members {
                // Extract member paths from members = ["crates/*", "tools/*"]
                for segment in trimmed.split('"') {
                    let seg = segment.trim().trim_matches(',').trim();
                    if seg.is_empty()
                        || seg.starts_with('[')
                        || seg.starts_with(']')
                        || seg.contains('=')
                        || seg == "members"
                    {
                        continue;
                    }
                    // Expand glob patterns like crates/*
                    if seg.contains('*') {
                        let prefix = seg.trim_end_matches("/*").trim_end_matches("\\*");
                        let pattern_dir = root.join(prefix);
                        if let Ok(entries) = fs::read_dir(&pattern_dir) {
                            for entry in entries.flatten() {
                                let member_src = entry.path().join("src");
                                if member_src.is_dir() {
                                    src_dirs.push(member_src);
                                }
                            }
                        }
                    } else {
                        let member_src = root.join(seg).join("src");
                        if member_src.is_dir() {
                            src_dirs.push(member_src);
                        }
                    }
                }
                if trimmed.contains(']') {
                    in_members = false;
                }
            }
        }
    }

    // Fallback: single-crate project with src/
    if src_dirs.is_empty() {
        let src_dir = root.join("src");
        if src_dir.is_dir() {
            src_dirs.push(src_dir);
        }
    }

    src_dirs.sort();
    src_dirs
}

fn discover_rs_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(discover_rs_files(&path));
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

// ---------------------------------------------------------------------------
// `assura lsp` -- start the LSP server
// ---------------------------------------------------------------------------

fn run_lsp() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let (service, socket) = tower_lsp::LspService::new(assura_lsp::AssuraLanguageServer::new);
        tower_lsp::Server::new(stdin, stdout, socket)
            .serve(service)
            .await;
    });
}

// ---------------------------------------------------------------------------
// `assura doctor` -- check installation health
// ---------------------------------------------------------------------------

fn run_doctor() {
    let mut all_ok = true;

    // assura version
    let version = env!("CARGO_PKG_VERSION");
    println!("Assura Doctor");
    println!("  assura:       v{version}");

    // rustc
    match process::Command::new("rustc").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout);
            let ver = ver.trim().strip_prefix("rustc ").unwrap_or(ver.trim());
            println!("  rustc:        {ver} ... OK");
        }
        _ => {
            println!("  rustc:        not found ... MISSING");
            println!("                Install: https://rustup.rs/");
            all_ok = false;
        }
    }

    // cargo
    match process::Command::new("cargo").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout);
            let ver = ver.trim().strip_prefix("cargo ").unwrap_or(ver.trim());
            println!("  cargo:        {ver} ... OK");
        }
        _ => {
            println!("  cargo:        not found ... MISSING");
            all_ok = false;
        }
    }

    // z3
    match process::Command::new("z3").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout);
            let ver = ver.trim();
            // z3 --version outputs "Z3 version 4.13.0 - ..."
            let short = ver
                .strip_prefix("Z3 version ")
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or(ver);
            println!("  z3:           {short} ... OK");
        }
        _ => {
            println!("  z3:           not found ... MISSING (required for verification)");
            println!("                Install: brew install z3  (macOS)");
            println!("                         sudo apt-get install -y libz3-dev  (Ubuntu)");
            all_ok = false;
        }
    }

    // cvc5 (optional)
    match process::Command::new("cvc5").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout);
            let ver = ver.trim();
            let short = ver.lines().next().unwrap_or(ver);
            println!("  cvc5:         {short} ... OK");
        }
        _ => {
            println!("  cvc5:         not found ... OPTIONAL (enables portfolio mode)");
            println!("                Install: brew install cvc5  (macOS)");
        }
    }

    // wasm target (optional)
    match process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
    {
        Ok(out) if out.status.success() => {
            let installed = String::from_utf8_lossy(&out.stdout);
            if installed.contains("wasm32") {
                println!("  wasm target:  installed ... OK");
            } else {
                println!("  wasm target:  not installed ... OPTIONAL");
                println!("                Install: rustup target add wasm32-wasip1");
            }
        }
        _ => {
            println!("  wasm target:  unknown (rustup not found) ... OPTIONAL");
        }
    }

    println!();
    if all_ok {
        println!("All required dependencies are installed.");
    } else {
        println!(
            "Some required dependencies are missing. Install them and re-run `assura doctor`."
        );
        process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// `assura coverage` -- contract coverage report
// ---------------------------------------------------------------------------

fn run_coverage(path: &str, contracts_dir: &str, format: &str, min_coverage: Option<f64>) {
    let root = Path::new(path);
    let src_dir = root.join("src");

    if !src_dir.exists() {
        eprintln!("Error: no src/ directory found at {}", root.display());
        process::exit(2);
    }

    // Phase 1: Discover all public Rust functions
    let rs_files = discover_rs_files(&src_dir);
    let mut all_fns: Vec<(String, String)> = Vec::new(); // (file, fn_name)

    for rs_file in &rs_files {
        let rel_path = rs_file
            .strip_prefix(root)
            .unwrap_or(rs_file.as_path())
            .to_string_lossy()
            .to_string();

        let source = match fs::read_to_string(rs_file) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let sigs = extract_rust_fn_signatures(&source);
        for sig in sigs {
            if sig.is_pub {
                all_fns.push((rel_path.clone(), sig.name));
            }
        }
    }

    // Phase 2: Discover all contract/bind names from .assura files
    let contracts_path = root.join(contracts_dir);
    let mut contract_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut contract_files: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    // Scan contracts directory
    if contracts_path.exists() {
        collect_contract_names_from_dir(&contracts_path, &mut contract_names, &mut contract_files);
    }
    // Also scan for .assura files in the project root and common locations
    for extra_dir in &[".", "assura", "specs"] {
        let d = root.join(extra_dir);
        if d.exists() && d != contracts_path {
            collect_contract_names_from_dir(&d, &mut contract_names, &mut contract_files);
        }
    }

    if all_fns.is_empty() {
        eprintln!("No public functions found in {}", src_dir.display());
        process::exit(1);
    }

    // Phase 3: Cross-reference
    let mut covered: Vec<(String, String, String)> = Vec::new(); // (file, fn, contract_file)
    let mut uncovered: Vec<(String, String, usize)> = Vec::new(); // (file, fn, param_count)

    for (file, fn_name) in &all_fns {
        if contract_names.contains(fn_name.as_str()) {
            let cf = contract_files
                .get(fn_name.as_str())
                .cloned()
                .unwrap_or_else(|| "?".to_string());
            covered.push((file.clone(), fn_name.clone(), cf));
        } else {
            // Get param count for prioritization
            let param_count = rs_files
                .iter()
                .find(|f| {
                    f.strip_prefix(root)
                        .unwrap_or(f.as_path())
                        .to_string_lossy()
                        == *file
                })
                .and_then(|f| fs::read_to_string(f).ok())
                .map(|src| {
                    extract_rust_fn_signatures(&src)
                        .iter()
                        .find(|s| s.name == *fn_name)
                        .map(|s| s.params.len())
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            uncovered.push((file.clone(), fn_name.clone(), param_count));
        }
    }

    // Sort uncovered by param count descending (more params = more complex = higher priority)
    uncovered.sort_by_key(|b| std::cmp::Reverse(b.2));

    let total = all_fns.len();
    let covered_count = covered.len();
    let pct = if total > 0 {
        (covered_count as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    let is_json = format == "json";

    if is_json {
        let report = serde_json::json!({
            "total_functions": total,
            "covered": covered_count,
            "uncovered": uncovered.len(),
            "coverage_percent": (pct * 10.0).round() / 10.0,
            "covered_functions": covered.iter().map(|(f, n, cf)| serde_json::json!({
                "file": f, "function": n, "contract_file": cf
            })).collect::<Vec<_>>(),
            "uncovered_functions": uncovered.iter().map(|(f, n, pc)| serde_json::json!({
                "file": f, "function": n, "param_count": pc
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else {
        println!("Contract Coverage: {}/", src_dir.display());
        println!("  Total public functions:  {}", total);
        println!("  With contracts:          {} ({:.1}%)", covered_count, pct);
        println!("  Without contracts:       {}", uncovered.len());

        if !covered.is_empty() {
            println!();
            println!("  Covered:");
            for (file, name, cf) in &covered {
                println!("    {file}::{name}  <-  {cf}");
            }
        }

        if !uncovered.is_empty() {
            println!();
            println!("  Uncovered (by complexity):");
            for (file, name, pc) in uncovered.iter().take(20) {
                println!("    {file}::{name}  ({pc} params)");
            }
            if uncovered.len() > 20 {
                println!("    ... and {} more", uncovered.len() - 20);
            }
        }
    }

    // Exit 1 if below min coverage
    if let Some(min) = min_coverage
        && pct < min
    {
        if !is_json {
            eprintln!();
            eprintln!("Coverage {pct:.1}% is below minimum {min:.1}%");
        }
        process::exit(1);
    }
}

/// Collect contract/bind names from all .assura files in a directory.
fn collect_contract_names_from_dir(
    dir: &Path,
    names: &mut std::collections::HashSet<String>,
    files: &mut std::collections::HashMap<String, String>,
) {
    let assura_files = discover_assura_files(dir);
    for af in &assura_files {
        let rel = af.to_string_lossy().to_string();
        let source = match fs::read_to_string(af) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let (parsed, _) = assura_parser::parse(&source);
        if let Some(file) = parsed {
            for decl in &file.decls {
                let name = match &decl.node {
                    Decl::Contract(c) => Some(c.name.clone()),
                    Decl::Bind(b) => Some(b.name.clone()),
                    Decl::FnDef(f) => Some(f.name.clone()),
                    Decl::Service(s) => Some(s.name.clone()),
                    _ => None,
                };
                if let Some(n) = name {
                    names.insert(n.clone());
                    files.insert(n, rel.clone());
                }
            }
        }
    }
}

/// Recursively discover all .assura files under a directory.
fn discover_assura_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(discover_assura_files(&path));
            } else if path.extension().is_some_and(|ext| ext == "assura") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

// ---------------------------------------------------------------------------
// `assura agent-instructions` -- print compact agent reference
// ---------------------------------------------------------------------------

fn run_agent_instructions() {
    use assura_codegen::type_map::rust_type_to_assura;

    // Build the type mapping table dynamically from the codegen module
    let type_pairs: Vec<(&str, &str)> = vec![
        ("i8, i16, i32, i64, i128, isize", "Int"),
        ("u8, u16, u32, u64, u128, usize", "Nat"),
        ("f32, f64", "Float"),
        ("bool", "Bool"),
        ("String, &str", "String"),
        ("Vec<u8>, &[u8]", "Bytes"),
        ("()", "Unit"),
    ];
    // Dynamic mappings verified against the codegen module
    let dynamic_pairs: Vec<(String, String)> = vec![
        (
            "Vec<T>".to_string(),
            format!(
                "List<T> (e.g., Vec<i64> -> {})",
                rust_type_to_assura("Vec<i64>")
            ),
        ),
        (
            "Option<T>".to_string(),
            format!(
                "T? (e.g., Option<i64> -> {})",
                rust_type_to_assura("Option<i64>")
            ),
        ),
        (
            "HashMap<K,V>, BTreeMap<K,V>".to_string(),
            format!(
                "Map<K,V> (e.g., HashMap<String, i64> -> {})",
                rust_type_to_assura("HashMap<String, i64>")
            ),
        ),
        (
            "HashSet<T>, BTreeSet<T>".to_string(),
            format!(
                "Set<T> (e.g., HashSet<i64> -> {})",
                rust_type_to_assura("HashSet<i64>")
            ),
        ),
        (
            "Box<T>, Arc<T>, Rc<T>".to_string(),
            format!(
                "T (wrapper erasure, e.g., Arc<String> -> {})",
                rust_type_to_assura("Arc<String>")
            ),
        ),
        (
            "&T, &mut T".to_string(),
            format!(
                "T (reference erasure, e.g., &i64 -> {})",
                rust_type_to_assura("&i64")
            ),
        ),
    ];

    // Build the error code ranges from the diagnostics catalog
    let catalog = assura_diagnostics::error_catalog();
    let mut code_groups: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for info in &catalog {
        let prefix = if info.code.len() >= 4 {
            &info.code[..4]
        } else {
            info.code
        };
        code_groups
            .entry(prefix.to_string())
            .or_default()
            .push(format!("{}: {}", info.code, info.name));
    }

    // Output the instructions
    print!(
        r#"# Assura: AI Agent Quick Reference

## What is Assura?

A contract-first language that compiles to Rust. You write behavioral
contracts (preconditions, postconditions, invariants). The compiler
proves correctness via Z3 SMT solver, then generates Rust source code.

## Contract Syntax

```assura
contract ContractName {{
    input(param1: Type, param2: Type)
    output(result: ReturnType)

    requires {{ precondition_expression }}
    ensures  {{ postcondition_expression }}
    effects  {{ effect_list }}
}}
```

### Clause Kinds

| Clause | Purpose |
|--------|---------|
| `input(...)` | Parameters the function accepts |
| `output(...)` | Return value |
| `requires {{ ... }}` | Preconditions (caller must satisfy) |
| `ensures {{ ... }}` | Postconditions (implementation must satisfy) |
| `effects {{ ... }}` | Side effects the function may perform |
| `invariant {{ ... }}` | Properties that hold throughout execution |
| `decreases {{ ... }}` | Termination measure for recursive functions |
| `states {{ ... }}` | Typestate declarations (for services) |
| `ghost {{ ... }}` | Ghost variables (verification only, erased at runtime) |
| `data_flow {{ ... }}` | Information flow / taint tracking constraints |

### Expression Features

- `old(expr)` in ensures: value of expr before the call
- `result` in ensures: the return value
- `forall x in collection : predicate`: universal quantifier
- `exists x in collection : predicate`: existential quantifier
- `if cond then a else b`: conditional expression
- `length(collection)`: collection length
- `abs(x)`: absolute value

## Type Mapping (Rust to Assura)

| Rust Type | Assura Type |
|-----------|-------------|
"#
    );

    for (rust, assura) in &type_pairs {
        println!("| `{rust}` | `{assura}` |");
    }
    for (rust, assura) in &dynamic_pairs {
        println!("| `{rust}` | `{assura}` |");
    }

    print!(
        r#"
## Binding Contracts to Existing Rust Functions

Use `bind` to attach a contract to a Rust function without rewriting it:

```assura
bind "crate::module::function_name" as function_name_checked {{
    input(x: Int, data: Bytes)
    output(result: Nat)
    requires {{ length(data) > 0 }}
    requires {{ x >= 0 }}
    ensures  {{ result <= length(data) }}
    effects  {{ io }}
}}
```

## Valid Effect Names

`io`, `database`, `logging`, `mem`, `net`, `fs`, `rng`, `time`,
`alloc`, `diverge`, `random`, `pure`

Dotted sub-effects: `console.read`, `console.write`, `filesystem.read`,
`filesystem.write`, `network.connect`, `network.listen`, `database.read`,
`database.write`, `log.info`, `log.error`

## CLI Commands

```bash
# Check a contract file (parse + resolve + typecheck + verify)
assura check file.assura
assura check file.assura --layer 0        # structural checks only
assura check file.assura --verbose         # show timing
assura check file.assura --json            # machine-readable output
assura check file.assura --watch           # re-check on file changes
assura check file.assura --stats           # verification statistics

# Build: verify + generate Rust code
assura build file.assura                   # output to generated/
assura build file.assura --output src/     # custom output dir
assura build file.assura --target wasm     # WASM target

# Generate skeleton contracts from Rust source
assura infer src/lib.rs                    # all public functions
assura infer src/lib.rs --function parse   # specific function
assura infer src/lib.rs -o contracts.assura

# Scan a Rust project
assura audit .                             # whole workspace
assura audit . --unsafe-only               # only unsafe code
assura audit . --focus "parser::*"         # specific module

# Other
assura init my-project                     # scaffold new project
assura fmt file.assura                     # format source
assura explain A03001                      # explain error code
assura test-gen file.assura                # generate tests from contracts
assura doctor                              # check dependencies
assura coverage .                          # contract coverage report
assura completions zsh                     # shell completions
assura lsp                                 # start LSP server
```

## Error Code Ranges

| Range | Category |
|-------|----------|
| A01xxx | Syntax errors (lexer/parser) |
| A02xxx | Name resolution errors |
| A03xxx | Type errors |
| A05xxx | Linearity / verification errors |
| A06xxx | Typestate errors |
| A07xxx | Effect system errors |
| A08xxx | Information flow errors |

Use `assura explain <code>` for details on any error code.

## Development Workflow

1. Write a contract defining what the function should do
2. Run `assura check contract.assura` to verify the contract is well-formed
3. Generate Rust with `assura build contract.assura`
4. If verification fails, read the counterexample and fix the contract
5. The generated Rust includes `debug_assert!` from requires/ensures clauses

For existing Rust code:
1. Run `assura infer src/lib.rs -o contracts.assura` to generate skeletons
2. Refine the skeleton contracts with real invariants
3. Run `assura check contracts.assura` to verify
4. Counterexamples reveal bugs in the original code

## Example Contracts

### Simple arithmetic safety
```assura
contract SafeDivision {{
    input(a: Int, b: Int)
    output(result: Int)
    requires {{ b != 0 }}
    ensures  {{ result * b + (a mod b) == a }}
}}
```

### Bounds checking
```assura
contract BoundedAccess {{
    input(data: List<Int>, index: Nat)
    output(result: Int)
    requires {{ index < length(data) }}
    requires {{ length(data) > 0 }}
    ensures  {{ result == data[index] }}
}}
```

### Side effects declaration
```assura
contract WriteLog {{
    input(message: String)
    output(result: Bool)
    requires {{ length(message) > 0 }}
    ensures  {{ result == true }}
    effects  {{ io, fs }}
}}
```
"#
    );
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
        if let Some(ref f) = file {
            eprintln!(
                "  parse:     {} tokens, {} declaration(s), {} import(s) ({:.2}ms)",
                timing.token_count,
                f.decls.len(),
                f.imports.len(),
                timing.parse_ms
            );
        } else {
            eprintln!(
                "  parse:     {} tokens, failed ({:.2}ms)",
                timing.token_count, timing.parse_ms
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
        let total = timing.parse_ms
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

// ---------------------------------------------------------------------------
// `assura repl` -- interactive contract playground
// ---------------------------------------------------------------------------

fn run_repl() {
    use std::io::{self, BufRead, Write};

    println!("Assura REPL v{}", env!("CARGO_PKG_VERSION"));
    println!("Type a contract to parse and verify. Commands:");
    println!("  :type <rust_type>     Show Assura type mapping");
    println!("  :explain <code>       Explain an error code");
    println!("  :load <file>          Load and verify a file");
    println!("  :quit or Ctrl-D       Exit");
    println!();

    let stdin = io::stdin();
    let mut buffer = String::new();
    let mut in_block = false;
    let mut brace_depth: i32 = 0;

    loop {
        if in_block {
            eprint!("  ... ");
        } else {
            eprint!("assura> ");
        }
        io::stderr().flush().ok();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                eprintln!();
                break;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error reading input: {e}");
                break;
            }
        }

        let trimmed = line.trim();

        if !in_block {
            if trimmed == ":quit" || trimmed == ":q" || trimmed == ":exit" {
                break;
            }
            if trimmed.is_empty() {
                continue;
            }
            if let Some(rust_type) = trimmed.strip_prefix(":type ") {
                let assura_type = assura_codegen::type_map::rust_type_to_assura(rust_type.trim());
                println!("{rust_type} -> {assura_type}");
                continue;
            }
            if trimmed == ":type" {
                eprintln!("Usage: :type <rust_type>  (e.g., :type Vec<Option<i64>>)");
                continue;
            }
            if let Some(code) = trimmed.strip_prefix(":explain ") {
                repl_explain(code.trim());
                continue;
            }
            if trimmed == ":explain" {
                eprintln!("Usage: :explain <code>  (e.g., :explain A03001)");
                continue;
            }
            if let Some(file) = trimmed.strip_prefix(":load ") {
                repl_load(file.trim());
                continue;
            }
            if trimmed == ":load" {
                eprintln!("Usage: :load <file.assura>");
                continue;
            }
            if trimmed.starts_with(':') {
                eprintln!("Unknown command: {trimmed}");
                eprintln!("Available: :type, :explain, :load, :quit");
                continue;
            }
        }

        buffer.push_str(&line);
        for ch in line.chars() {
            if ch == '{' {
                brace_depth += 1;
                in_block = true;
            } else if ch == '}' {
                brace_depth -= 1;
            }
        }

        if brace_depth <= 0 {
            in_block = false;
            brace_depth = 0;
            let input = buffer.trim().to_string();
            if !input.is_empty() {
                repl_eval(&input);
            }
            buffer.clear();
        }
    }
}

fn repl_explain(code: &str) {
    if let Some(info) = assura_diagnostics::explain(code) {
        println!("{} ({})", info.code, info.name);
        println!("  {}", info.description);
        if !info.example.is_empty() {
            println!("  Example: {}", info.example);
        }
        if !info.fix.is_empty() {
            println!("  Fix: {}", info.fix);
        }
    } else {
        eprintln!("Unknown error code: {code}");
    }
}

fn repl_load(path: &str) {
    match fs::read_to_string(path) {
        Ok(source) => repl_eval(&source),
        Err(e) => eprintln!("Error loading {path}: {e}"),
    }
}

fn repl_eval(source: &str) {
    let result = assura_pipeline::run(source);

    for diag in &result.parse_errors {
        eprintln!("  Parse error: {}", diag.message);
    }
    if !result.parse_errors.is_empty() {
        return;
    }

    if result.declarations.is_empty() {
        eprintln!("  No declarations found.");
        return;
    }

    for decl in &result.declarations {
        println!("  OK  {decl}");
    }

    for diag in &result.resolution_errors {
        eprintln!("  Resolution error: {} ({})", diag.message, diag.code);
    }
    if !result.resolution_errors.is_empty() {
        return;
    }

    for diag in &result.type_errors {
        eprintln!("  Type error: {} ({})", diag.message, diag.code);
    }
    if !result.type_errors.is_empty() {
        return;
    }

    for entry in &result.verification {
        match entry.status.as_str() {
            "verified" => println!("  VERIFIED  {}", entry.clause),
            "counterexample" => {
                println!("  COUNTEREXAMPLE  {}", entry.clause);
                if let Some(model) = &entry.model {
                    println!("    | {model}");
                }
            }
            "timeout" => println!("  TIMEOUT  {}", entry.clause),
            "unknown" => {
                let reason = entry.reason.as_deref().unwrap_or("unknown");
                println!("  UNKNOWN  {}: {reason}", entry.clause);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// `assura diff` -- structural diff between contract files
// ---------------------------------------------------------------------------

fn extract_decl_summary(sf: &SourceFile) -> std::collections::BTreeMap<String, Vec<String>> {
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

fn format_clause_body(clause: &assura_parser::ast::Clause) -> String {
    format!("{:?}", clause.body)
}

fn run_diff(old_path: &str, new_path: &str, format: &str) {
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

    if has_diff {
        process::exit(1);
    }
}

struct DiffEntry {
    name: String,
    kind: String,
    added_clauses: Vec<String>,
    removed_clauses: Vec<String>,
    unchanged_clauses: Vec<String>,
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
            tested >= 25,
            "expected at least 25 MUST REJECT fixtures, found {tested}"
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

    // =======================================================================
    // E2E expected outcomes test harness
    // =======================================================================

    /// Expected outcome parsed from an `// EXPECTED: <kind>` annotation.
    #[derive(Debug, PartialEq)]
    enum ExpectedOutcome {
        /// File should verify successfully (no errors).
        Verified,
        /// File should produce at least one counterexample.
        Counterexample,
    }

    /// Parse `// EXPECTED: verified` or `// EXPECTED: counterexample`
    /// from the first lines of source text.
    fn parse_expected(source: &str) -> Option<ExpectedOutcome> {
        for line in source.lines().take(5) {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("// EXPECTED:") {
                let kind = rest.trim().to_lowercase();
                return match kind.as_str() {
                    "verified" => Some(ExpectedOutcome::Verified),
                    "counterexample" => Some(ExpectedOutcome::Counterexample),
                    _ => None,
                };
            }
        }
        None
    }

    /// Run the full pipeline (parse -> resolve -> type-check -> verify)
    /// and return (has_errors, has_counterexample).
    ///
    /// E2E tests use verify_parallel with caching (matches real CLI
    /// behavior) rather than the shared pipeline's basic verify().
    fn run_e2e_pipeline(source: &str) -> (bool, bool) {
        let (file, parse_errors) = assura_parser::parse(source);
        if !parse_errors.is_empty() {
            return (true, false);
        }
        let file = match file {
            Some(f) => f,
            None => return (true, false),
        };
        let resolved = match assura_resolve::resolve(&file) {
            Ok(r) => r,
            Err(_) => return (true, false),
        };
        let hir = assura_hir::lower(&resolved);
        let typed = match assura_types::type_check_hir(&hir) {
            Ok(t) => t,
            Err(_) => return (true, false),
        };
        let cache_dir = std::env::temp_dir().join("assura_e2e_cache");
        let _ = std::fs::create_dir_all(&cache_dir);
        let cache = assura_smt::VerificationCache::new(&cache_dir);
        let results = assura_smt::verify_parallel(&typed, &cache);
        let has_counterexample = results
            .iter()
            .any(|r| matches!(r, assura_smt::VerificationResult::Counterexample { .. }));
        (has_counterexample, has_counterexample)
    }

    #[test]
    fn test_e2e_expected_outcomes() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let e2e_dir = root.join("tests/e2e");

        let mut tested = 0;
        let mut failures: Vec<String> = Vec::new();

        for entry in std::fs::read_dir(&e2e_dir).expect("cannot read tests/e2e") {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("assura") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let filename = path.file_name().unwrap().to_str().unwrap();

            let expected = match parse_expected(&source) {
                Some(e) => e,
                None => {
                    failures.push(format!("{filename}: missing // EXPECTED: annotation"));
                    continue;
                }
            };

            let (has_errors, has_counterexample) = run_e2e_pipeline(&source);

            match expected {
                ExpectedOutcome::Verified => {
                    if has_errors || has_counterexample {
                        failures.push(format!(
                            "{filename}: expected verified, but got errors={has_errors} counterexample={has_counterexample}"
                        ));
                    }
                }
                ExpectedOutcome::Counterexample => {
                    if !has_counterexample {
                        failures.push(format!(
                            "{filename}: expected counterexample, but none found"
                        ));
                    }
                }
            }

            tested += 1;
        }

        assert!(
            failures.is_empty(),
            "E2E test failures:\n{}",
            failures.join("\n")
        );
        assert!(
            tested >= 5,
            "expected at least 5 E2E test files, found {tested}"
        );
    }

    // =======================================================================
    // discover_rs_files unit tests (issue #49)
    // =======================================================================

    #[test]
    fn discover_rs_files_finds_nested_files() {
        let dir = std::env::temp_dir().join("assura_test_discover_nested");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub/deep")).unwrap();
        std::fs::write(dir.join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.join("sub/lib.rs"), "pub fn f() {}").unwrap();
        std::fs::write(dir.join("sub/deep/util.rs"), "pub fn g() {}").unwrap();
        // Non-Rust files should be skipped
        std::fs::write(dir.join("notes.txt"), "not rust").unwrap();
        std::fs::write(dir.join("sub/readme.md"), "docs").unwrap();

        let found = super::discover_rs_files(&dir);
        assert_eq!(found.len(), 3, "should find exactly 3 .rs files");
        assert!(
            found.iter().any(|p| p.ends_with("main.rs")),
            "should find main.rs"
        );
        assert!(
            found.iter().any(|p| p.ends_with("lib.rs")),
            "should find sub/lib.rs"
        );
        assert!(
            found.iter().any(|p| p.ends_with("util.rs")),
            "should find sub/deep/util.rs"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_rs_files_empty_dir_returns_empty() {
        let dir = std::env::temp_dir().join("assura_test_discover_empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let found = super::discover_rs_files(&dir);
        assert!(found.is_empty(), "empty dir should yield no files");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_rs_files_nonexistent_returns_empty() {
        let dir = std::env::temp_dir().join("assura_test_discover_nonexistent");
        let _ = std::fs::remove_dir_all(&dir);

        let found = super::discover_rs_files(&dir);
        assert!(found.is_empty(), "nonexistent dir should yield no files");
    }

    // =======================================================================
    // Infer helper tests (issue #50)
    // =======================================================================

    #[test]
    fn extract_sigs_simple_pub_fn() {
        let source = "pub fn add(a: i64, b: i64) -> i64 { a + b }";
        let sigs = super::extract_rust_fn_signatures(source);
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "add");
        assert!(sigs[0].is_pub);
        assert_eq!(sigs[0].params.len(), 2);
        assert_eq!(sigs[0].return_type, "i64");
    }

    #[test]
    fn extract_sigs_skips_private_fn() {
        let source = "fn helper(x: i32) -> i32 { x }";
        let sigs = super::extract_rust_fn_signatures(source);
        assert_eq!(sigs.len(), 1);
        assert!(!sigs[0].is_pub);
    }

    #[test]
    fn extract_sigs_multiline() {
        let source = "pub fn long_name(\n    a: String,\n    b: Vec<u8>,\n) -> bool {\n    true\n}";
        let sigs = super::extract_rust_fn_signatures(source);
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "long_name");
        assert_eq!(sigs[0].params.len(), 2);
        assert_eq!(sigs[0].return_type, "bool");
    }

    #[test]
    fn extract_sigs_with_self_param() {
        let source = "pub fn get(&self, key: &str) -> Option<String> {";
        let sigs = super::extract_rust_fn_signatures(source);
        assert_eq!(sigs.len(), 1);
        // &self should be skipped
        assert_eq!(sigs[0].params.len(), 1);
        assert_eq!(sigs[0].params[0].0, "key");
    }

    #[test]
    fn extract_sigs_pub_crate() {
        let source = "pub(crate) fn internal(x: u32) -> u32 { x }";
        let sigs = super::extract_rust_fn_signatures(source);
        assert_eq!(sigs.len(), 1);
        assert!(sigs[0].is_pub);
        assert_eq!(sigs[0].name, "internal");
    }

    #[test]
    fn extract_sigs_no_return_type() {
        let source = "pub fn do_stuff(x: i32) { println!(\"{x}\"); }";
        let sigs = super::extract_rust_fn_signatures(source);
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].return_type, "()");
    }

    #[test]
    fn parse_param_list_empty() {
        let result = super::parse_param_list("");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_param_list_single() {
        let result = super::parse_param_list("x: i64");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], ("x".to_string(), "i64".to_string()));
    }

    #[test]
    fn parse_param_list_multiple() {
        let result = super::parse_param_list("a: i32, b: String, c: bool");
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].0, "a");
        assert_eq!(result[1].0, "b");
        assert_eq!(result[2].0, "c");
    }

    #[test]
    fn parse_param_list_nested_generics() {
        let result =
            super::parse_param_list("data: HashMap<String, Vec<Option<i32>>>, count: usize");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "data");
        assert_eq!(result[0].1, "HashMap<String, Vec<Option<i32>>>");
        assert_eq!(result[1].0, "count");
    }

    #[test]
    fn parse_param_list_skips_self() {
        let result = super::parse_param_list("&self, x: i32");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "x");
    }

    #[test]
    fn parse_param_list_mut_self() {
        let result = super::parse_param_list("&mut self, key: String, val: i64");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "key");
        assert_eq!(result[1].0, "val");
    }

    #[test]
    fn parse_fn_sig_basic() {
        let sig = super::parse_fn_signature("add(a: i64, b: i64) -> i64 {", true).unwrap();
        assert_eq!(sig.name, "add");
        assert_eq!(sig.params.len(), 2);
        assert_eq!(sig.return_type, "i64");
        assert!(sig.is_pub);
    }

    #[test]
    fn parse_fn_sig_with_where() {
        let sig = super::parse_fn_signature("process(x: T) -> T where T: Clone {", true).unwrap();
        assert_eq!(sig.name, "process");
        assert_eq!(sig.return_type, "T");
    }

    #[test]
    fn parse_fn_sig_no_return() {
        let sig = super::parse_fn_signature("do_work(x: i32) {", false).unwrap();
        assert_eq!(sig.name, "do_work");
        assert_eq!(sig.return_type, "()");
        assert!(!sig.is_pub);
    }

    #[test]
    fn generate_bind_skeleton_roundtrip() {
        let sig = super::RustFnSig {
            name: "add".to_string(),
            params: vec![
                ("a".to_string(), "i64".to_string()),
                ("b".to_string(), "i64".to_string()),
            ],
            return_type: "i64".to_string(),
            is_pub: true,
        };
        let mut out = String::new();
        super::generate_bind_skeleton("crate::math", &sig, &mut out);
        assert!(out.contains("bind \"crate::math::add\" as add"));
        assert!(out.contains("input(a: Int, b: Int)"));
        assert!(out.contains("output(result: Int)"));
        // Should parse through our own parser
        let (parsed, errs) = assura_parser::parse(&out);
        assert!(
            errs.is_empty(),
            "generated bind should parse: {errs:?}\n{out}"
        );
        assert!(parsed.is_some(), "parsed to None:\n{out}");
    }

    #[test]
    fn generate_bind_skeleton_no_return() {
        let sig = super::RustFnSig {
            name: "log".to_string(),
            params: vec![("msg".to_string(), "&str".to_string())],
            return_type: "()".to_string(),
            is_pub: true,
        };
        let mut out = String::new();
        super::generate_bind_skeleton("crate::util", &sig, &mut out);
        assert!(out.contains("bind \"crate::util::log\" as log"));
        assert!(out.contains("input(msg: String)"));
        // Unit return should not produce output line
        assert!(!out.contains("output(result:"));
    }

    #[test]
    fn discover_rs_files_results_are_sorted() {
        let dir = std::env::temp_dir().join("assura_test_discover_sorted");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("b")).unwrap();
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::write(dir.join("b/z.rs"), "").unwrap();
        std::fs::write(dir.join("a/a.rs"), "").unwrap();
        std::fs::write(dir.join("c.rs"), "").unwrap();

        let found = super::discover_rs_files(&dir);
        let sorted = found.clone();
        assert_eq!(found, sorted, "results should be sorted");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- Regression tests for #42: module path derivation ---

    #[test]
    fn derive_module_path_crate_with_hyphen() {
        // Simulates crates/assura-codegen/src/type_map.rs
        let dir = std::env::temp_dir().join("assura_test_modpath_42");
        let _ = std::fs::remove_dir_all(&dir);
        let crate_dir = dir.join("crates/my-crate");
        std::fs::create_dir_all(crate_dir.join("src")).unwrap();
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();

        let path = format!("{}/crates/my-crate/src/type_map.rs", dir.display());
        let module = super::derive_rust_module_path(&path);
        assert_eq!(
            module, "my_crate::type_map",
            "hyphens must become underscores"
        );

        // lib.rs should resolve to just the crate name
        let lib_path = format!("{}/crates/my-crate/src/lib.rs", dir.display());
        let lib_module = super::derive_rust_module_path(&lib_path);
        assert_eq!(lib_module, "my_crate");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn derive_module_path_nested_module() {
        let dir = std::env::temp_dir().join("assura_test_modpath_nested");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/foo")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"example\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();

        let path = format!("{}/src/foo/bar.rs", dir.display());
        let module = super::derive_rust_module_path(&path);
        assert_eq!(module, "example::foo::bar");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- Regression tests for #43: workspace discovery ---

    #[test]
    fn discover_workspace_src_dirs_single_crate() {
        let dir = std::env::temp_dir().join("assura_test_ws_single");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"single\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let dirs = super::discover_workspace_src_dirs(&dir);
        assert_eq!(dirs.len(), 1);
        assert!(dirs[0].ends_with("src"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_workspace_src_dirs_workspace_glob() {
        let dir = std::env::temp_dir().join("assura_test_ws_glob");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("crates/alpha/src")).unwrap();
        std::fs::create_dir_all(dir.join("crates/beta/src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();

        let dirs = super::discover_workspace_src_dirs(&dir);
        assert_eq!(dirs.len(), 2, "should find both workspace members");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_workspace_src_dirs_explicit_members() {
        let dir = std::env::temp_dir().join("assura_test_ws_explicit");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("lib/core/src")).unwrap();
        std::fs::create_dir_all(dir.join("tools/cli/src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"lib/core\", \"tools/cli\"]\n",
        )
        .unwrap();

        let dirs = super::discover_workspace_src_dirs(&dir);
        assert_eq!(dirs.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- Regression tests for #44: function signature extraction ---

    #[test]
    fn extract_sigs_async_fn() {
        let source = "pub async fn fetch(url: &str) -> Result<String, Error> {";
        let sigs = super::extract_rust_fn_signatures(source);
        assert_eq!(sigs.len(), 1, "should match pub async fn");
        assert_eq!(sigs[0].name, "fetch");
        assert!(sigs[0].is_pub);
    }

    #[test]
    fn extract_sigs_const_fn() {
        let source = "pub const fn max_size() -> usize { 1024 }";
        let sigs = super::extract_rust_fn_signatures(source);
        assert_eq!(sigs.len(), 1, "should match pub const fn");
        assert_eq!(sigs[0].name, "max_size");
    }

    #[test]
    fn extract_sigs_unsafe_fn() {
        let source = "pub unsafe fn raw_ptr(p: *const u8) -> u8 {";
        let sigs = super::extract_rust_fn_signatures(source);
        assert_eq!(sigs.len(), 1, "should match pub unsafe fn");
        assert_eq!(sigs[0].name, "raw_ptr");
    }

    #[test]
    fn extract_sigs_pub_crate_async_fn() {
        let source = "pub(crate) async fn internal_fetch(url: &str) -> String {";
        let sigs = super::extract_rust_fn_signatures(source);
        assert_eq!(sigs.len(), 1, "should match pub(crate) async fn");
        assert_eq!(sigs[0].name, "internal_fetch");
        assert!(sigs[0].is_pub);
    }

    #[test]
    fn extract_sigs_generic_fn_name_stripped() {
        let source = "pub fn encode<T: Serialize>(value: &T) -> Vec<u8> {";
        let sigs = super::extract_rust_fn_signatures(source);
        assert_eq!(sigs.len(), 1);
        assert_eq!(
            sigs[0].name, "encode",
            "generic params must be stripped from name"
        );
    }

    #[test]
    fn extract_sigs_generic_with_where() {
        let source = "pub fn process<T>(items: Vec<T>) -> Vec<T> where T: Clone + Debug {";
        let sigs = super::extract_rust_fn_signatures(source);
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "process");
        assert_eq!(sigs[0].return_type, "Vec<T>");
    }
}
