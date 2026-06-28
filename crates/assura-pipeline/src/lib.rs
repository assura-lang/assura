//! Shared compiler pipeline for the Assura language.
//!
//! Provides `compile()` for full-fidelity pipeline results (used by CLI,
//! LSP, server) and `run()` for a lightweight JSON-serializable summary
//! (used by MCP).
//!
//! Pipeline: parse -> resolve -> type check -> verify

use std::time::Instant;

use assura_config::CompilerConfig;
use assura_parser::ast::Decl;

// ---------------------------------------------------------------------------
// CompilationOutput: the canonical pipeline result
// ---------------------------------------------------------------------------

/// Full-fidelity result of running the compiler pipeline.
///
/// Contains all intermediate artifacts and diagnostics. Every entry point
/// (CLI, LSP, server, MCP) should use this instead of re-implementing the
/// pipeline chain.
pub struct CompilationOutput {
    /// Parsed AST (`None` if parsing failed entirely).
    pub file: Option<assura_parser::ast::SourceFile>,
    /// Resolved file (`None` if resolution was not attempted or failed).
    pub resolved: Option<assura_resolve::ResolvedFile>,
    /// Type-checked file (`None` if type checking was not attempted or failed).
    pub typed: Option<assura_types::TypedFile>,
    /// SMT verification results (empty if verification was not run).
    pub verification: Vec<assura_smt::VerificationResult>,
    /// Generated Rust project (`None` if codegen was not run).
    pub generated: Option<assura_codegen::GeneratedProject>,
    /// All diagnostics from every phase (parse, resolve, type check).
    pub diagnostics: Vec<assura_diagnostics::Diagnostic>,
    /// Whether any errors were found (parse, resolve, or type check only).
    ///
    /// **Important:** SMT counterexamples live in [`verification`](Self::verification)
    /// and do NOT set `has_errors`. Use [`verification_succeeded`] or
    /// [`verification_strict_succeeded`] to check verify outcomes.
    pub has_errors: bool,
    /// Timing information for each phase.
    pub timing: PhaseTiming,
}

/// Timing information for each pipeline phase.
#[derive(Clone, Copy)]
pub struct PhaseTiming {
    /// Time to lex and parse, in milliseconds.
    pub parse_ms: f64,
    /// Time to resolve names (None if skipped).
    pub resolve_ms: Option<f64>,
    /// Time to type-check (None if skipped).
    pub typecheck_ms: Option<f64>,
    /// Time to run SMT verification (None if skipped).
    pub verify_ms: Option<f64>,
    /// Time to generate Rust code (None if skipped).
    pub codegen_ms: Option<f64>,
    /// Number of tokens produced by the lexer.
    pub token_count: usize,
}

/// Run the full pipeline: lex -> parse -> resolve -> type check.
///
/// Collects all diagnostics and intermediate artifacts. Does NOT run SMT
/// verification (that is caller-controlled since it needs solver choice,
/// caching, etc.).
pub fn compile(source: &str, filename: &str, config: &CompilerConfig) -> CompilationOutput {
    let mut diagnostics: Vec<assura_diagnostics::Diagnostic> = Vec::new();
    let mut has_errors = false;

    // --- Lex + Parse ---
    let lex_start = Instant::now();
    let parse_result = assura_parser::parse_full(source);
    let parse_ms = lex_start.elapsed().as_secs_f64() * 1000.0;

    let token_count = parse_result.token_count;
    let file = parse_result.file;

    for le in &parse_result.lex_errors {
        has_errors = true;
        diagnostics.push(le.to_diagnostic(source).with_file(filename));
    }

    for e in &parse_result.parse_errors {
        has_errors = true;
        let d: assura_diagnostics::Diagnostic = e.clone().into();
        diagnostics.push(d.with_file(filename));
    }

    // --- Resolve ---
    let resolve_start = Instant::now();
    let resolved = if let Some(ref f) = file {
        match assura_resolve::resolve(f) {
            Ok(r) => {
                for w in &r.warnings {
                    let mut d: assura_diagnostics::Diagnostic = w.clone().into();
                    d.severity = assura_diagnostics::Severity::Warning;
                    diagnostics.push(d.with_file(filename));
                }
                Some(r)
            }
            Err(errs) => {
                has_errors = true;
                for e in &errs {
                    let d: assura_diagnostics::Diagnostic = e.clone().into();
                    diagnostics.push(d.with_file(filename));
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

    // --- Type check ---
    let typecheck_start = Instant::now();
    let had_resolved = resolved.is_some();
    let (typed, resolved) = if let Some(res) = resolved {
        match assura_types::TypeChecker::new()
            .config(config.type_check.clone())
            .check(res)
        {
            Ok(t) => {
                for w in &t.warnings {
                    let mut d: assura_diagnostics::Diagnostic = w.clone().into();
                    d.severity = assura_diagnostics::Severity::Warning;
                    diagnostics.push(d.with_file(filename));
                }
                // resolved is now inside typed.resolved (Arc)
                (Some(t), None)
            }
            Err((errs, returned_resolved)) => {
                has_errors = true;
                for e in &errs {
                    let d: assura_diagnostics::Diagnostic = e.clone().into();
                    diagnostics.push(d.with_file(filename));
                }
                (None, Some(returned_resolved))
            }
        }
    } else {
        (None, None)
    };
    let typecheck_ms = if had_resolved {
        Some(typecheck_start.elapsed().as_secs_f64() * 1000.0)
    } else {
        None
    };

    CompilationOutput {
        file,
        resolved,
        typed,
        verification: vec![],
        generated: None,
        diagnostics,
        has_errors,
        timing: PhaseTiming {
            parse_ms,
            resolve_ms,
            typecheck_ms,
            verify_ms: None,
            codegen_ms: None,
            token_count,
        },
    }
}

/// Run SMT verification on an already type-checked file using options from
/// `CompilerConfig.verify` (solver, timeout, parallel, decrease checks).
///
/// This is the canonical verify entry point for pipeline consumers. Prefer it
/// over constructing `Verifier::new(...).parallel().with_decrease_checks()` by
/// hand so CLI, server, MCP, and tests stay behaviorally aligned.
///
/// # Agent / caller invariants
///
/// - **Layer 0**: `config.verify.layer < 1` returns an empty vec (no solver).
/// - **Options**: always pass full `CompilerConfig.verify` (or
///   `Verifier::apply_options`); do not re-chain `.parallel()` ad hoc.
/// - **`has_errors`**: this function does **not** mutate any error flag.
///   Counterexamples live only in the returned `Vec<VerificationResult>`.
/// - **Unknown reasons** containing [`assura_smt::KNOWN_SMT_LIMITATION_MARKER`]
///   are compiler limitations (warnings in CLI), not necessarily failures.
/// - **Contracts are spec-only**: `result` / output vars may be unconstrained;
///   `ensures` that mention only outputs often counterexample legitimately.
pub fn verify_typed(
    typed: &assura_types::TypedFile,
    filename: &str,
    config: &CompilerConfig,
) -> Vec<assura_smt::VerificationResult> {
    if config.verify.layer < 1 {
        return Vec::new();
    }

    // --- Layer 1: standard SMT verification ---
    let mut results = assura_smt::Verifier::new(typed)
        .source(std::path::Path::new(filename))
        .apply_options(config.verify.clone())
        .verify();

    // --- Layer 2: quantified invariants, termination, roundtrips ---
    if config.verify.layer >= 2 {
        results.extend(assura_smt::verify_layer2(typed, config));
    }

    // --- Layer 3: BMC + k-induction ---
    if config.verify.layer >= 3 {
        results.extend(assura_smt::verify_layer3(typed, config));
    }

    results
}

// Layer 2/3 verification logic moved to assura_smt::verify_layer2/verify_layer3.
// See crates/assura-smt/src/entry/layer_dispatch.rs.

/// Run the full pipeline: lex -> parse -> resolve -> type check -> verify -> codegen.
///
/// Unlike `compile()`, this also runs SMT verification and code generation.
/// Verification is skipped if type checking failed or `config.verify.layer < 1`.
/// Codegen runs whenever type checking succeeded (SMT counterexamples are
/// recorded in `verification` but do not block codegen; callers decide how
/// to treat them via `verification` results or [`verification_succeeded`]).
///
/// # Agent / caller invariants
///
/// - Early return when `output.has_errors` after `compile()` (parse/resolve/type only).
/// - SMT results are stored in `output.verification`; they do **not** set `has_errors`.
/// - For “did verify succeed?” use [`verification_succeeded`], not `!has_errors` alone.
/// - Defaults in `CompilerConfig::default().verify` enable parallel + decrease checks
///   (CLI parity). Tests should use `VerifyOptions::for_tests()` via `test_config`.
pub fn compile_full(source: &str, filename: &str, config: &CompilerConfig) -> CompilationOutput {
    let mut output = compile(source, filename, config);

    if output.has_errors {
        return output;
    }

    // --- SMT verification (options from CompilerConfig.verify) ---
    let verify_start = Instant::now();
    if let Some(ref typed) = output.typed {
        output.verification = verify_typed(typed, filename, config);
    }
    output.timing.verify_ms = Some(verify_start.elapsed().as_secs_f64() * 1000.0);

    // #703: Suppress A04008 "result unconstrained" warnings when the
    // corresponding ensures clause actually verified (IR sidecar loaded).
    if !output.verification.is_empty() {
        let has_verified_ensures = output.verification.iter().any(|r| {
            matches!(
                r,
                assura_smt::VerificationResult::Verified { clause_desc, .. }
                    if clause_desc.contains("ensures")
            )
        });
        if has_verified_ensures {
            output.diagnostics.retain(|d| d.code != "A04008");
        }
    }

    // --- Codegen ---
    let codegen_start = Instant::now();
    if let Some(ref typed) = output.typed {
        output.generated = Some(assura_codegen::codegen(typed));
    }
    output.timing.codegen_ms = Some(codegen_start.elapsed().as_secs_f64() * 1000.0);

    output
}

/// True when no verification result is a counterexample or timeout.
///
/// `Unknown` (including [`assura_smt::KNOWN_SMT_LIMITATION_MARKER`]) is treated
/// as non-fatal here, matching lightweight test / MCP success heuristics.
/// Callers that need stricter policy should inspect `verification` directly,
/// or use [`verification_strict_succeeded`].
pub fn verification_succeeded(results: &[assura_smt::VerificationResult]) -> bool {
    !results.iter().any(|r| {
        matches!(
            r,
            assura_smt::VerificationResult::Counterexample { .. }
                | assura_smt::VerificationResult::Timeout { .. }
        )
    })
}

/// Like [`verification_succeeded`], but also fails on any `Unknown` whose
/// reason is **not** a known compiler limitation (`is_known_smt_limitation`).
///
/// Use for tests annotated conceptually as `// MUST VERIFY` (solver must
/// decide Verified or only limitation-Unknowns).
pub fn verification_strict_succeeded(results: &[assura_smt::VerificationResult]) -> bool {
    if !verification_succeeded(results) {
        return false;
    }
    !results.iter().any(|r| match r {
        assura_smt::VerificationResult::Unknown { reason, .. } => {
            !assura_smt::is_known_smt_limitation(reason)
        }
        _ => false,
    })
}

// ---------------------------------------------------------------------------
// IR Verification (AI verification loop, task 12.01)
// ---------------------------------------------------------------------------

/// Result of verifying IR implementation against a contract.
///
/// Contains per-clause verification results with progress tracking.
/// This is the core output type for the AI verification loop.
#[derive(Debug, Clone, serde::Serialize)]
pub struct IrVerifyResult {
    /// Overall status: "verified", "failed", "error".
    pub status: String,
    /// Human-readable progress: "3/4 clauses verified (75%)".
    pub progress: String,
    /// Per-clause verification results.
    pub clauses: Vec<IrClauseResult>,
    /// Aggregate counts.
    pub summary: IrVerifySummary,
    /// Contract compilation errors (empty when contract compiles cleanly).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub compile_errors: Vec<String>,
    /// IR parse errors (empty when IR parses cleanly).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ir_errors: Vec<String>,
    /// Structural validation errors (empty when IR matches contract).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub validation_errors: Vec<String>,
}

/// Per-clause verification result for the AI verification loop.
#[derive(Debug, Clone, serde::Serialize)]
pub struct IrClauseResult {
    /// Clause descriptor: "ContractName::ensures[0]".
    pub name: String,
    /// Status: "verified", "counterexample", "timeout", "unknown".
    pub status: String,
    /// Counterexample model (for counterexample status).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterexample: Option<serde_json::Value>,
    /// Reason (for unknown status).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Summary counts for IR verification.
#[derive(Debug, Clone, serde::Serialize)]
pub struct IrVerifySummary {
    pub verified: usize,
    pub counterexample: usize,
    pub timeout: usize,
    pub unknown: usize,
    pub total: usize,
}

impl IrVerifyResult {
    fn from_results(results: Vec<assura_smt::VerificationResult>) -> Self {
        let mut verified = 0usize;
        let mut counterexample = 0usize;
        let mut timeout = 0usize;
        let mut unknown = 0usize;

        let clauses: Vec<IrClauseResult> = results
            .iter()
            .map(|r| match r {
                assura_smt::VerificationResult::Verified { clause_desc, .. } => {
                    verified += 1;
                    IrClauseResult {
                        name: clause_desc.clone(),
                        status: "verified".into(),
                        counterexample: None,
                        reason: None,
                    }
                }
                assura_smt::VerificationResult::Counterexample {
                    clause_desc,
                    model,
                    counter_model,
                    ..
                } => {
                    counterexample += 1;
                    let cex = counter_model.as_ref().map(|cm| {
                        let vars: serde_json::Map<String, serde_json::Value> = cm
                            .variables
                            .iter()
                            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                            .collect();
                        serde_json::json!({ "variables": vars, "raw_model": model })
                    });
                    IrClauseResult {
                        name: clause_desc.clone(),
                        status: "counterexample".into(),
                        counterexample: cex,
                        reason: None,
                    }
                }
                assura_smt::VerificationResult::Timeout { clause_desc } => {
                    timeout += 1;
                    IrClauseResult {
                        name: clause_desc.clone(),
                        status: "timeout".into(),
                        counterexample: None,
                        reason: None,
                    }
                }
                assura_smt::VerificationResult::Unknown {
                    clause_desc,
                    reason,
                } => {
                    unknown += 1;
                    IrClauseResult {
                        name: clause_desc.clone(),
                        status: "unknown".into(),
                        counterexample: None,
                        reason: Some(reason.clone()),
                    }
                }
            })
            .collect();

        let total = clauses.len();
        let has_hard_failure = counterexample > 0 || timeout > 0;
        let has_unknown_failure = unknown > 0
            && !results.iter().all(|r| {
                r.is_known_limitation()
                    || matches!(r, assura_smt::VerificationResult::Verified { .. })
            });
        let status = if has_hard_failure || has_unknown_failure {
            "failed"
        } else {
            "verified"
        };

        let pct = (verified * 100).checked_div(total).unwrap_or(0);
        let progress = format!("{verified}/{total} clauses verified ({pct}%)");

        Self {
            status: status.into(),
            progress,
            clauses,
            summary: IrVerifySummary {
                verified,
                counterexample,
                timeout,
                unknown,
                total,
            },
            compile_errors: vec![],
            ir_errors: vec![],
            validation_errors: vec![],
        }
    }

    fn compile_error(diagnostics: Vec<assura_diagnostics::Diagnostic>) -> Self {
        Self {
            status: "error".into(),
            progress: "0/0 clauses verified (0%)".into(),
            clauses: vec![],
            summary: IrVerifySummary {
                verified: 0,
                counterexample: 0,
                timeout: 0,
                unknown: 0,
                total: 0,
            },
            compile_errors: diagnostics.iter().map(|d| d.to_string()).collect(),
            ir_errors: vec![],
            validation_errors: vec![],
        }
    }

    fn ir_parse_error(errors: Vec<String>) -> Self {
        Self {
            status: "error".into(),
            progress: "0/0 clauses verified (0%)".into(),
            clauses: vec![],
            summary: IrVerifySummary {
                verified: 0,
                counterexample: 0,
                timeout: 0,
                unknown: 0,
                total: 0,
            },
            compile_errors: vec![],
            ir_errors: errors,
            validation_errors: vec![],
        }
    }

    fn validation_error(errors: Vec<String>) -> Self {
        Self {
            status: "error".into(),
            progress: "0/0 clauses verified (0%)".into(),
            clauses: vec![],
            summary: IrVerifySummary {
                verified: 0,
                counterexample: 0,
                timeout: 0,
                unknown: 0,
                total: 0,
            },
            compile_errors: vec![],
            ir_errors: vec![],
            validation_errors: errors,
        }
    }
}

/// Verify IR implementation text against an Assura contract source.
///
/// This is the core of the AI verification loop (task 12.01). It:
/// 1. Compiles the contract (parse + resolve + typecheck)
/// 2. Parses the IR text
/// 3. Validates IR structure against the contract
/// 4. Runs SMT verification with the IR body constraints
/// 5. Returns per-clause results with progress tracking
pub fn verify_ir(
    contract_source: &str,
    ir_source: &str,
    config: &CompilerConfig,
) -> IrVerifyResult {
    // 1. Compile contract through the pipeline
    let output = compile(contract_source, "<contract>", config);
    if output.has_errors {
        return IrVerifyResult::compile_error(output.diagnostics);
    }
    let typed = match output.typed {
        Some(t) => t,
        None => {
            return IrVerifyResult::compile_error(vec![assura_diagnostics::Diagnostic::error(
                "A01000",
                "contract produced no typed output",
                0..0,
            )]);
        }
    };

    // 2. Parse the IR text
    let ir_module = match assura_smt::parse_ir_module(ir_source) {
        Ok(m) => m,
        Err(errors) => return IrVerifyResult::ir_parse_error(errors),
    };

    // 3. Structural validation: find the first contract decl and validate
    let contract_decl = typed
        .resolved
        .source
        .decls
        .iter()
        .find_map(|d| match &d.node {
            Decl::Contract(c) => Some(c),
            _ => None,
        });

    if let Some(contract) = contract_decl {
        let validation = assura_smt::validate_ir_against_contract(&ir_module, contract);
        if !validation.valid {
            return IrVerifyResult::validation_error(validation.errors);
        }
    }

    // 4. Build VerifyFileExtras from in-memory IR (mapped to the contract name)
    let contract_name = contract_decl
        .map(|c| c.name.as_str())
        .unwrap_or(&ir_module.name);
    let loaded = match assura_smt::LoadedVerifyExtras::from_ir_text(ir_source, contract_name) {
        Ok(l) => l,
        Err(errors) => return IrVerifyResult::ir_parse_error(errors),
    };

    // 5. Run SMT verification with the inline extras
    if config.verify.layer < 1 {
        // Layer 0 only: no SMT verification, just structural validation
        return IrVerifyResult {
            status: "verified".into(),
            progress: "0/0 clauses verified (structural only)".into(),
            clauses: vec![],
            summary: IrVerifySummary {
                verified: 0,
                counterexample: 0,
                timeout: 0,
                unknown: 0,
                total: 0,
            },
            compile_errors: vec![],
            ir_errors: vec![],
            validation_errors: vec![],
        };
    }

    let results = assura_smt::Verifier::new(&typed)
        .with_extras(&loaded)
        .apply_options(config.verify.clone())
        .verify();

    IrVerifyResult::from_results(results)
}

// ---------------------------------------------------------------------------
// PipelineResult: lightweight JSON-serializable summary (MCP compat)
// ---------------------------------------------------------------------------

/// A diagnostic from any pipeline phase.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PipelineDiagnostic {
    pub code: assura_diagnostics::ErrorCode,
    pub message: String,
}

/// A verification result entry (alias for [`assura_smt::VerificationSummary`]).
pub type VerificationEntry = assura_smt::VerificationSummary;

/// The result of running the full compiler pipeline on a source string.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PipelineResult {
    pub success: bool,
    pub declarations: Vec<String>,
    pub parse_errors: Vec<PipelineDiagnostic>,
    pub resolution_errors: Vec<PipelineDiagnostic>,
    pub type_errors: Vec<PipelineDiagnostic>,
    pub verification: Vec<VerificationEntry>,
}

impl PipelineResult {
    /// True if the pipeline produced any errors.
    pub fn has_errors(&self) -> bool {
        !self.parse_errors.is_empty()
            || !self.resolution_errors.is_empty()
            || !self.type_errors.is_empty()
    }
}

/// Extract a human-readable summary name from a declaration.
fn decl_summary(decl: &Decl) -> String {
    decl.summary_label()
}

/// Run the full compiler pipeline: parse -> resolve -> HIR -> typecheck -> verify.
///
/// Returns a lightweight JSON-serializable summary. For full-fidelity results
/// with intermediate artifacts, use `compile()` or `compile_full()` instead.
///
/// Uses `"<inline>"` as the source path (no IR sidecar discovery). Prefer
/// [`run_at`] when verifying a file on disk.
pub fn run(source: &str) -> PipelineResult {
    run_at(source, "<inline>")
}

/// Like [`run`], but uses `filename` for IR sidecar discovery (`{dir}/{Name}.ir`,
/// `{dir}/generated/{Name}.ir`).
pub fn run_at(source: &str, filename: &str) -> PipelineResult {
    let output = compile_full(source, filename, &CompilerConfig::default());

    let declarations: Vec<String> = output
        .file
        .as_ref()
        .map(|f| f.decls.iter().map(|d| decl_summary(&d.node)).collect())
        .unwrap_or_default();

    // Classify diagnostics by phase (based on error code prefix)
    let mut parse_errors = Vec::new();
    let mut resolution_errors = Vec::new();
    let mut type_errors = Vec::new();
    for d in &output.diagnostics {
        let pd = PipelineDiagnostic {
            code: d.code.clone(),
            message: d.message.clone(),
        };
        if d.code.as_str().starts_with("A01") {
            parse_errors.push(pd);
        } else if d.code.as_str().starts_with("A02") {
            resolution_errors.push(pd);
        } else {
            type_errors.push(pd);
        }
    }

    if output.has_errors {
        return PipelineResult {
            success: false,
            declarations,
            parse_errors,
            resolution_errors,
            type_errors,
            verification: vec![],
        };
    }

    let verification: Vec<VerificationEntry> = output
        .verification
        .iter()
        .map(assura_smt::VerificationSummary::from)
        .collect();

    let success = verification_succeeded(&output.verification);

    PipelineResult {
        success,
        parse_errors: vec![],
        declarations,
        resolution_errors: vec![],
        type_errors: vec![],
        verification,
    }
}
#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
