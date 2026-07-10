use clap::{Args, CommandFactory, Subcommand};
use clap_complete::Shell;
use std::process;

use assura_config::{OutputMode, Verbosity};

use super::*;

// ---------------------------------------------------------------------------
// CLI argument definitions (clap 4)
// ---------------------------------------------------------------------------

#[derive(clap::Parser)]
#[command(name = "assura", version, about = "The Assura contract compiler")]
#[command(subcommand_required = true)]
struct Cli {
    #[command(flatten)]
    global: GlobalOpts,

    #[command(subcommand)]
    command: Commands,
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
        /// Source file to check (use `-` to read from stdin)
        file: String,

        /// Verification layer (0=structural, 1=SMT, 2=quantified/termination, 3=BMC)
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

        /// Display unsat cores for verified clauses
        #[arg(long)]
        show_cores: bool,

        /// Treat SMT Unknown (including known limitations) and Timeout as errors
        #[arg(long)]
        strict: bool,

        /// When FILE is a directory, only check demos marked SHOWCASE (must-pass)
        #[arg(long)]
        showcase_only: bool,
    },

    /// Verify inline contract annotations in Rust source files
    CheckRust {
        /// Rust source file or directory to check
        path: String,

        /// Verification layer (0=structural, 1=SMT, 2=quantified/termination, 3=BMC)
        #[arg(long, default_value_t = 1)]
        layer: u8,

        /// SMT solver backend
        #[arg(long, value_parser = parse_solver)]
        solver: Option<assura_smt::SolverChoice>,

        /// Enable LLM-assisted body-vs-contract analysis
        #[arg(long)]
        llm: bool,

        /// Suggest contracts for unannotated functions (combine with --llm for richer analysis)
        #[arg(long)]
        suggest: bool,

        /// LLM provider (anthropic, openai, ollama)
        #[arg(long, default_value = "anthropic")]
        llm_provider: String,

        /// LLM model name
        #[arg(long)]
        llm_model: Option<String>,

        /// Only suggest for public functions (with --suggest)
        #[arg(long)]
        public_only: bool,

        /// Only suggest for unsafe functions (with --suggest)
        #[arg(long)]
        unsafe_only: bool,

        /// Enable Level 2 LLM lemma verification (combine with --llm for richer analysis)
        #[arg(long)]
        llm_verify: bool,
    },

    /// Suggest contracts from cargo-fuzz crash artifacts
    SuggestFromCrash {
        /// Crash artifact file
        #[arg(long, conflicts_with = "crash_dir")]
        crash_input: Option<String>,

        /// Directory of crash artifacts (processes all crash-*/oom-*/timeout-* files)
        #[arg(long, conflicts_with = "crash_input")]
        crash_dir: Option<String>,

        /// Rust source file or directory containing the crashing function
        #[arg(long)]
        target: String,

        /// Stack trace file (from RUST_BACKTRACE=1 output)
        #[arg(long)]
        stacktrace: Option<String>,

        /// LLM provider (anthropic, openai, ollama)
        #[arg(long, default_value = "anthropic")]
        llm_provider: String,

        /// LLM model name
        #[arg(long)]
        llm_model: Option<String>,
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

        /// Generate runtime contract checks that persist in release builds
        #[arg(long)]
        runtime_checks: bool,

        /// Use the configured LLM provider to auto-generate implementations for contracts
        #[arg(long)]
        auto_implement: bool,

        /// Write heuristic IR sidecars next to the source (no LLM); co-located for check/build
        #[arg(long)]
        write_ir: bool,

        /// Emit a binary crate with `fn main` that exercises the primary contract
        #[arg(long)]
        bin: bool,
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
        /// Source file or directory to format (use `-` to read from stdin, write to stdout)
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

        /// Show suggestions without modifying any files
        #[arg(long)]
        dry_run: bool,

        /// Focus on specific risk patterns (comma-separated: unwrap,panic,division,index,unsafe)
        #[arg(long)]
        focus: Option<String>,
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
        /// Run SMT verification of contract evolution (precondition
        /// weakening + postcondition strengthening)
        #[arg(long)]
        verify: bool,
    },

    /// Emit an AI prompt to generate Implementation IR for a contract
    IrPrompt {
        /// Assura source file
        file: String,

        /// Declaration name (required when the file has multiple verification jobs)
        #[arg(long)]
        decl: Option<String>,

        /// List declaration names eligible for IR prompts (one per line)
        #[arg(long)]
        list: bool,

        /// Pattern overlay: auto, identity, arithmetic, length-copy, call-chain, bounds-check, field-access
        #[arg(long, default_value = "auto")]
        pattern: String,
    },

    /// Parse, validate, and codegen an Implementation IR file (Section 4)
    Ir {
        /// IR text file to process
        file: String,

        /// Contract file to validate against
        #[arg(long)]
        contract: Option<String>,

        /// Output directory for generated Rust (default: stdout)
        #[arg(short, long)]
        output: Option<String>,

        /// Run SMT verification of IR against the contract (requires --contract)
        #[arg(long)]
        verify: bool,

        /// Run SMT verification only, skip codegen (requires --contract)
        #[arg(long)]
        verify_only: bool,
    },

    /// Generate documentation from contract files
    Doc {
        /// Source file or directory to document
        file: String,

        /// Output directory (default: stdout)
        #[arg(short, long)]
        output: Option<String>,

        /// Include verification results in documentation
        #[arg(long)]
        verify: bool,
    },

    /// Start the MCP (Model Context Protocol) server for AI agent integration
    Mcp,
}

fn parse_solver(s: &str) -> Result<assura_smt::SolverChoice, String> {
    assura_smt::SolverChoice::from_str_loose(s)
        .ok_or_else(|| format!("invalid solver: {s} (expected z3, cvc5, or portfolio)"))
}

/// Prefer global `--json` when a command's own `--format` is still the
/// default `"human"`. Explicit `--format json|human` always wins.
fn resolve_format_with_global_json(output_mode: OutputMode, format: String) -> String {
    if output_mode == OutputMode::Json && format == "human" {
        "json".to_string()
    } else {
        format
    }
}

fn parse_target(s: &str) -> Result<assura_codegen::CompileTarget, String> {
    assura_codegen::CompileTarget::from_str_loose(s)
        .ok_or_else(|| format!("invalid target: {s} (expected native or wasm)"))
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run() {
    let cli = <Cli as clap::Parser>::parse();
    let output_mode = cli.global.output_mode();
    let verbosity = cli.global.verbosity();

    match cli.command {
        Commands::Check {
            file,
            layer,
            solver,
            watch,
            stats,
            dump_smt,
            show_cores,
            strict,
            showcase_only,
        } => run_check(CheckOptions {
            filename: &file,
            output_mode,
            verbosity,
            layer,
            solver,
            watch,
            stats,
            dump_smt: dump_smt.as_deref(),
            show_cores,
            strict,
            showcase_only,
        }),
        Commands::CheckRust {
            path,
            layer,
            solver,
            llm,
            suggest,
            llm_provider,
            llm_model,
            public_only,
            unsafe_only,
            llm_verify,
        } => run_check_rust(
            &path,
            output_mode,
            verbosity,
            layer,
            solver,
            LlmOpts {
                llm,
                suggest,
                provider: &llm_provider,
                model: llm_model.as_deref(),
                public_only,
                unsafe_only,
                llm_verify,
            },
        ),
        Commands::SuggestFromCrash {
            crash_input,
            crash_dir,
            target,
            stacktrace,
            llm_provider,
            llm_model,
        } => run_suggest_from_crash(SuggestFromCrashOpts {
            crash_input: crash_input.as_deref(),
            crash_dir: crash_dir.as_deref(),
            target: &target,
            stacktrace_file: stacktrace.as_deref(),
            llm_provider: &llm_provider,
            llm_model: llm_model.as_deref(),
            output_mode,
            verbosity,
        }),
        Commands::Build {
            file,
            output,
            target,
            no_check,
            solver,
            runtime_checks,
            auto_implement,
            write_ir,
            bin,
        } => run_build(BuildOpts {
            filename: &file,
            output_mode,
            verbosity,
            cli_output: &output,
            cli_target: target,
            no_check,
            cli_solver: solver,
            runtime_checks,
            auto_implement,
            write_ir,
            bin,
        }),
        Commands::Init { name } => run_init(&name, output_mode),
        Commands::Explain { code } => run_explain(&code, output_mode),
        Commands::Fmt { file, check } => run_fmt(&file, check, output_mode),
        Commands::Infer {
            file,
            function,
            output,
            dry_run,
            focus,
        } => run_infer(
            &file,
            function.as_deref(),
            output.as_deref(),
            dry_run,
            focus.as_deref(),
            output_mode,
        ),
        Commands::TestGen { file, output } => {
            run_test_gen(&file, output.as_deref(), verbosity, output_mode)
        }
        Commands::AgentInstructions => run_agent_instructions(output_mode),
        Commands::Doctor => run_doctor(output_mode, verbosity),
        Commands::Lsp => run_lsp(),
        Commands::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "assura", &mut std::io::stdout());
        }
        Commands::Coverage {
            path,
            contracts_dir,
            format,
            min_coverage,
        } => {
            let format = resolve_format_with_global_json(output_mode, format);
            run_coverage(&path, &contracts_dir, &format, min_coverage);
        }
        Commands::Audit {
            path,
            depth,
            format,
            focus,
            max_functions,
            timeout,
            unsafe_only,
        } => {
            let format = resolve_format_with_global_json(output_mode, format);
            run_audit(AuditOptions {
                path: &path,
                depth: &depth,
                format: &format,
                focus: focus.as_deref(),
                max_functions,
                timeout_ms: timeout,
                unsafe_only,
            });
        }
        Commands::Repl => run_repl(output_mode),
        Commands::Diff {
            old,
            new,
            format,
            verify,
        } => {
            let format = resolve_format_with_global_json(output_mode, format);
            let (has_diff, structural_json) = run_diff(&old, &new, &format);
            if verify {
                // JSON: single document (structural + evolution). Do not print
                // structural JSON alone first (that produced two JSON objects).
                if format == "json" {
                    run_diff_verify(&old, &new, &format, Some(structural_json));
                } else {
                    run_diff_verify(&old, &new, &format, None);
                }
                // When --verify is used, exit code depends on evolution
                // verification (run_diff_verify exits non-zero on failure).
            } else {
                if format == "json" {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&structural_json).unwrap()
                    );
                }
                if has_diff {
                    process::exit(1);
                }
            }
        }
        Commands::IrPrompt {
            file,
            decl,
            list,
            pattern,
        } => ir_prompt_cmd::run_ir_prompt(
            &file,
            decl.as_deref(),
            list,
            &pattern,
            verbosity,
            output_mode,
        ),
        Commands::Ir {
            file,
            contract,
            output,
            verify,
            verify_only,
        } => run_ir(
            &file,
            contract.as_deref(),
            output.as_deref(),
            verbosity,
            output_mode,
            verify || verify_only,
            verify_only,
        ),
        Commands::Doc {
            file,
            output,
            verify,
        } => run_doc(&file, output.as_deref(), verify, output_mode, verbosity),
        Commands::Mcp => {
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            rt.block_on(async {
                if let Err(e) = assura_mcp::run_mcp_server().await {
                    eprintln!("MCP server error: {e}");
                    std::process::exit(1);
                }
            });
        }
    }
}
