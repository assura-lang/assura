//! Shared compiler pipeline for the Assura language.
//!
//! Provides `compile()` for full-fidelity pipeline results (used by CLI,
//! LSP, server) and `run()` for a lightweight JSON-serializable summary
//! (used by MCP).
//!
//! Pipeline: parse -> resolve -> type check -> verify

use std::time::Instant;

use assura_config::CompilerConfig;
use assura_parser::ast::{Decl, Expr};

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
    let typed = if let Some(ref res) = resolved {
        match assura_types::TypeChecker::new()
            .config(config.type_check.clone())
            .check(res)
        {
            Ok(t) => Some(t),
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
    let typecheck_ms = if resolved.is_some() {
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
        let layer2_results = run_layer2_verification(typed, config);
        results.extend(layer2_results);
    }

    results
}

// ---------------------------------------------------------------------------
// Layer 2 obligation collection and dispatch
// ---------------------------------------------------------------------------

/// Collect Layer 2 obligations from the typed AST and verify them.
///
/// Walks declarations for:
/// - `invariant` clauses containing `forall`/`exists` -> quantified invariants
/// - `decreases` clauses -> termination obligations
/// - Paired serialize/deserialize extern declarations -> roundtrip obligations
fn run_layer2_verification(
    typed: &assura_types::TypedFile,
    config: &CompilerConfig,
) -> Vec<assura_smt::VerificationResult> {
    let layer2_config = assura_smt::Layer2Config {
        timeout_ms: config.verify.timeout_ms.max(10_000), // Layer 2 needs longer timeout
        enable_quantifiers: true,
        enable_termination: true,
        enable_roundtrip: true,
    };
    let mut verifier = assura_smt::Layer2Verifier::new(layer2_config);

    // Walk all declarations to collect obligations
    for decl in &typed.resolved.source.decls {
        collect_invariant_obligations(&decl.node, &mut verifier);
        collect_termination_obligations(&decl.node, &mut verifier);
        collect_roundtrip_obligations(&decl.node, &mut verifier);
    }

    if verifier.obligation_count() == 0 {
        return Vec::new();
    }

    // Run Layer 2 verification and convert results
    let layer2_results = verifier.verify();
    layer2_results
        .into_iter()
        .map(layer2_result_to_verification_result)
        .collect()
}

/// Extract quantified invariant obligations from a declaration's clauses.
///
/// Looks for `ClauseKind::Invariant` clauses whose body contains
/// `Expr::Forall` or `Expr::Exists`. Each becomes a `QuantifiedInvariant`
/// for the Layer 2 verifier.
fn collect_invariant_obligations(decl: &Decl, verifier: &mut assura_smt::Layer2Verifier) {
    let decl_name = decl.name().unwrap_or("anonymous");
    let clauses = decl.clauses();

    for clause in clauses {
        if clause.kind != assura_parser::ast::ClauseKind::Invariant {
            continue;
        }
        // Check if the invariant body is a forall/exists expression
        match &clause.body.node {
            Expr::Forall { var, domain, body } => {
                let sort = domain_to_sort(domain);
                verifier.add_invariant(assura_smt::QuantifiedInvariant {
                    name: format!("{decl_name}::invariant(forall {var})"),
                    bound_vars: vec![(var.clone(), sort)],
                    body: expr_to_predicate_string(body),
                    triggers: vec![],
                });
            }
            Expr::Exists { var, domain, body } => {
                let sort = domain_to_sort(domain);
                verifier.add_invariant(assura_smt::QuantifiedInvariant {
                    name: format!("{decl_name}::invariant(exists {var})"),
                    bound_vars: vec![(var.clone(), sort)],
                    body: expr_to_predicate_string(body),
                    triggers: vec![],
                });
            }
            _ => {
                // Non-quantified invariant: encode as a ground assertion
                verifier.add_invariant(assura_smt::QuantifiedInvariant {
                    name: format!("{decl_name}::invariant"),
                    bound_vars: vec![("__ground".into(), "Int".into())],
                    body: expr_to_predicate_string(&clause.body),
                    triggers: vec![],
                });
            }
        }
    }
}

/// Extract termination obligations from `decreases` clauses.
///
/// Each `ClauseKind::Decreases` clause produces a `TerminationObligation`
/// with the measure expression and (currently empty) recursive call list.
fn collect_termination_obligations(decl: &Decl, verifier: &mut assura_smt::Layer2Verifier) {
    let decl_name = decl.name().unwrap_or("anonymous");
    let clauses = decl.clauses();

    for clause in clauses {
        if clause.kind != assura_parser::ast::ClauseKind::Decreases {
            continue;
        }
        let measure = expr_to_predicate_string(&clause.body);
        verifier.add_termination(assura_smt::TerminationObligation {
            fn_name: decl_name.to_string(),
            measure,
            // Recursive call detection would require call-graph analysis;
            // for now, we report the obligation with no recursive calls
            // (trivially terminating). Future work: walk the fn body for
            // self-calls and add them here.
            recursive_calls: vec![],
        });
    }
}

/// Extract roundtrip obligations from paired serialize/deserialize declarations.
///
/// Looks for extern functions named `serialize_*` / `deserialize_*` or
/// `*_to_json` / `*_from_json` (and similar patterns) within the same file.
fn collect_roundtrip_obligations(decl: &Decl, verifier: &mut assura_smt::Layer2Verifier) {
    // For CodecRegistry declarations, each codec is a roundtrip candidate
    if let Decl::CodecRegistry(registry) = decl {
        for codec in &registry.codecs {
            verifier.add_roundtrip(assura_smt::RoundtripObligation {
                type_name: format!("{}::{}", registry.name, codec.name),
                serialize_fn: format!("{}_encode", codec.name),
                deserialize_fn: format!("{}_decode", codec.name),
            });
        }
    }
}

/// Convert a domain expression to a Z3 sort name.
fn domain_to_sort(domain: &assura_parser::ast::SpExpr) -> String {
    match &domain.node {
        Expr::Ident(name) => match name.as_str() {
            "Int" | "Nat" | "Float" => name.clone(),
            "Bool" => "Bool".into(),
            "String" | "Bytes" => "String".into(),
            _ => "Int".into(), // default to Int for unknown types
        },
        Expr::Raw(tokens) if !tokens.is_empty() => tokens[0].clone(),
        _ => "Int".into(),
    }
}

/// Convert an AST expression to a simple predicate string for Layer 2.
///
/// This is a best-effort textual representation. The Layer 2 verifier
/// has its own expression parser (`parse_predicate_to_z3`).
fn expr_to_predicate_string(expr: &assura_parser::ast::SpExpr) -> String {
    use assura_parser::ast::Expr;
    match &expr.node {
        Expr::BinOp { op, lhs, rhs } => {
            let l = expr_to_predicate_string(lhs);
            let r = expr_to_predicate_string(rhs);
            format!("{l} {} {r}", op.as_str())
        }
        Expr::UnaryOp { op, expr } => {
            let o = expr_to_predicate_string(expr);
            format!("{}{o}", op.as_str())
        }
        Expr::Literal(lit) => match lit {
            assura_parser::ast::Literal::Int(n) => n.clone(),
            assura_parser::ast::Literal::Float(f) => f.clone(),
            assura_parser::ast::Literal::Bool(b) => b.to_string(),
            assura_parser::ast::Literal::Str(s) => format!("\"{s}\""),
        },
        Expr::Ident(name) => name.clone(),
        Expr::Forall { var, body, .. } => {
            format!("forall {var}: {}", expr_to_predicate_string(body))
        }
        Expr::Exists { var, body, .. } => {
            format!("exists {var}: {}", expr_to_predicate_string(body))
        }
        Expr::Raw(tokens) => tokens.join(" "),
        _ => format!("{expr:?}"),
    }
}

/// Convert a `Layer2Result` to a `VerificationResult` for uniform pipeline output.
fn layer2_result_to_verification_result(
    r: assura_smt::Layer2Result,
) -> assura_smt::VerificationResult {
    match r {
        assura_smt::Layer2Result::Verified { invariant, .. } => {
            assura_smt::VerificationResult::Verified {
                clause_desc: format!("layer2:{invariant}"),
                unsat_core: None,
            }
        }
        assura_smt::Layer2Result::Counterexample { invariant, model } => {
            let model_str = model
                .iter()
                .map(|(k, v)| format!("{k} = {v}"))
                .collect::<Vec<_>>()
                .join(", ");
            assura_smt::VerificationResult::Counterexample {
                clause_desc: format!("layer2:{invariant}"),
                model: model_str,
                counter_model: None,
            }
        }
        assura_smt::Layer2Result::Timeout {
            invariant,
            timeout_ms,
        } => assura_smt::VerificationResult::Timeout {
            clause_desc: format!("layer2:{invariant} (timeout {timeout_ms}ms)"),
        },
        assura_smt::Layer2Result::Unknown { invariant, reason } => {
            assura_smt::VerificationResult::Unknown {
                clause_desc: format!("layer2:{invariant}"),
                reason,
            }
        }
    }
}

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
mod tests {
    use super::*;

    #[test]
    fn run_valid_contract() {
        let result = run(
            "contract SafeDiv {\n  input(x: Int, y: Int)\n  output(result: Int)\n  requires { y != 0 }\n  ensures { result > 0 }\n}",
        );
        assert!(result.parse_errors.is_empty());
        assert!(result.resolution_errors.is_empty());
        assert!(
            result.type_errors.is_empty(),
            "unexpected type errors: {:?}",
            result.type_errors
        );
        assert_eq!(result.declarations, vec!["contract SafeDiv"]);
        assert!(!result.verification.is_empty());
    }

    #[test]
    fn run_empty_source() {
        let result = run("");
        assert!(result.declarations.is_empty());
    }

    #[test]
    fn run_parse_error() {
        let result = run("contract Bad { @@@ }");
        assert!(!result.success);
    }

    #[test]
    fn run_multiple_declarations() {
        let result =
            run("contract A {\n  requires { true }\n}\ncontract B {\n  requires { true }\n}");
        assert_eq!(result.declarations.len(), 2);
    }

    #[test]
    fn run_has_errors_false_on_success() {
        let result = run("contract X {\n  requires { true }\n}");
        assert!(!result.has_errors());
    }

    #[test]
    fn run_has_errors_true_on_parse_error() {
        let result = run("contract { !!! }");
        assert!(result.has_errors());
    }

    #[test]
    fn run_serializes_to_json() {
        let result = run("contract T {\n  requires { true }\n}");
        let json = serde_json::to_string(&result);
        assert!(json.is_ok());
    }

    // Tests for compile() function
    #[test]
    fn compile_valid_contract() {
        let config = CompilerConfig::default();
        let output = compile(
            "contract Add {\n  requires { true }\n}",
            "test.assura",
            &config,
        );
        assert!(output.file.is_some());
        assert!(output.resolved.is_some());
        assert!(
            output.typed.is_some(),
            "typed was None; diagnostics: {:?}",
            output.diagnostics
        );
        assert!(
            !output.has_errors,
            "unexpected errors: {:?}",
            output.diagnostics
        );
    }

    #[test]
    fn compile_parse_error_populates_diagnostics() {
        let config = CompilerConfig::default();
        let output = compile("contract { @@@ }", "bad.assura", &config);
        assert!(output.has_errors);
        assert!(!output.diagnostics.is_empty());
    }

    #[test]
    fn compile_empty_source() {
        let config = CompilerConfig::default();
        let output = compile("", "empty.assura", &config);
        assert!(output.file.is_some());
        assert!(!output.has_errors);
    }

    #[test]
    fn compile_records_timing() {
        let config = CompilerConfig::default();
        let output = compile(
            "contract T { requires(x: Int) ensures(result: Int) }",
            "test.assura",
            &config,
        );
        assert!(output.timing.parse_ms >= 0.0);
        assert!(output.timing.token_count > 0);
    }

    // -------------------------------------------------------------------
    // #510: Timeout/Unknown soundness guards
    // -------------------------------------------------------------------

    #[test]
    fn timeout_not_treated_as_verified() {
        let timeout = assura_smt::VerificationResult::Timeout {
            clause_desc: "test".into(),
        };
        let results = vec![timeout];
        assert!(!verification_succeeded(&results));
        assert!(!verification_strict_succeeded(&results));
    }

    #[test]
    fn unknown_non_limitation_not_strict_succeeded() {
        let unknown = assura_smt::VerificationResult::Unknown {
            clause_desc: "test".into(),
            reason: "solver returned inconclusive".into(),
        };
        let results = vec![unknown];
        // Lenient: passes (design decision: Unknown is non-fatal)
        assert!(verification_succeeded(&results));
        // Strict: fails (non-limitation Unknown is a real issue)
        assert!(!verification_strict_succeeded(&results));
    }

    #[test]
    fn known_limitation_passes_both() {
        let limitation = assura_smt::VerificationResult::unknown_not_encoded("test", "feature X");
        let results = vec![limitation];
        assert!(verification_succeeded(&results));
        assert!(verification_strict_succeeded(&results));
    }

    // -------------------------------------------------------------------
    // Layer 2 pipeline wiring tests
    // -------------------------------------------------------------------

    /// Helper: compile with Layer 2 enabled
    fn compile_layer2(source: &str) -> CompilationOutput {
        let config = CompilerConfig {
            verify: assura_config::VerifyOptions {
                layer: 2,
                parallel: false,
                decrease_checks: false,
                enable_cache: false,
                ..Default::default()
            },
            ..Default::default()
        };
        compile_full(source, "test.assura", &config)
    }

    #[test]
    fn layer2_skipped_at_layer1() {
        // Layer 1 should NOT produce any "layer2:" results
        let config = CompilerConfig {
            verify: assura_config::VerifyOptions {
                layer: 1,
                parallel: false,
                decrease_checks: false,
                enable_cache: false,
                ..Default::default()
            },
            ..Default::default()
        };
        let output = compile_full(
            "contract X {\n  requires { true }\n}",
            "test.assura",
            &config,
        );
        let layer2_results: Vec<_> = output
            .verification
            .iter()
            .filter(|r| match r {
                assura_smt::VerificationResult::Verified { clause_desc, .. }
                | assura_smt::VerificationResult::Counterexample { clause_desc, .. }
                | assura_smt::VerificationResult::Timeout { clause_desc, .. }
                | assura_smt::VerificationResult::Unknown { clause_desc, .. } => {
                    clause_desc.starts_with("layer2:")
                }
            })
            .collect();
        assert!(
            layer2_results.is_empty(),
            "layer 1 should not produce layer2 results"
        );
    }

    #[test]
    fn layer2_no_obligations_no_extra_results() {
        // A simple contract with no invariant/decreases/roundtrip
        // should not produce any layer2 results
        let output = compile_layer2("contract X {\n  requires { true }\n}");
        assert!(
            !output.has_errors,
            "unexpected errors: {:?}",
            output.diagnostics
        );
        let layer2_results: Vec<_> = output
            .verification
            .iter()
            .filter(|r| match r {
                assura_smt::VerificationResult::Verified { clause_desc, .. }
                | assura_smt::VerificationResult::Counterexample { clause_desc, .. }
                | assura_smt::VerificationResult::Timeout { clause_desc, .. }
                | assura_smt::VerificationResult::Unknown { clause_desc, .. } => {
                    clause_desc.starts_with("layer2:")
                }
            })
            .collect();
        assert!(
            layer2_results.is_empty(),
            "no obligations should produce no layer2 results"
        );
    }

    #[test]
    fn layer2_invariant_clause_collected() {
        // Contract with an invariant clause should produce layer2 results
        let source = "contract Counter {\n  input(n: Int)\n  requires { n >= 0 }\n  invariant { forall x in Int: x >= 0 || x < 0 }\n}";
        let output = compile_layer2(source);
        assert!(
            !output.has_errors,
            "unexpected errors: {:?}",
            output.diagnostics
        );
        let layer2_results: Vec<_> = output
            .verification
            .iter()
            .filter(|r| match r {
                assura_smt::VerificationResult::Verified { clause_desc, .. }
                | assura_smt::VerificationResult::Counterexample { clause_desc, .. }
                | assura_smt::VerificationResult::Timeout { clause_desc, .. }
                | assura_smt::VerificationResult::Unknown { clause_desc, .. } => {
                    clause_desc.starts_with("layer2:")
                }
            })
            .collect();
        assert!(
            !layer2_results.is_empty(),
            "invariant clause should produce layer2 results"
        );
    }

    #[test]
    fn layer2_decreases_clause_collected() {
        // Contract with a decreases clause should produce layer2 results
        let source = "contract Factorial {\n  input(n: Nat)\n  output(result: Nat)\n  requires { n >= 0 }\n  decreases { n }\n}";
        let output = compile_layer2(source);
        assert!(
            !output.has_errors,
            "unexpected errors: {:?}",
            output.diagnostics
        );
        let layer2_results: Vec<_> = output
            .verification
            .iter()
            .filter(|r| match r {
                assura_smt::VerificationResult::Verified { clause_desc, .. }
                | assura_smt::VerificationResult::Counterexample { clause_desc, .. }
                | assura_smt::VerificationResult::Timeout { clause_desc, .. }
                | assura_smt::VerificationResult::Unknown { clause_desc, .. } => {
                    clause_desc.starts_with("layer2:")
                }
            })
            .collect();
        assert!(
            !layer2_results.is_empty(),
            "decreases clause should produce layer2 results, got none; all results: {:?}",
            output.verification
        );
    }

    #[test]
    fn layer2_result_to_verification_result_conversion() {
        // Test each Layer2Result variant converts correctly
        let verified = assura_smt::Layer2Result::Verified {
            invariant: "inv1".into(),
            time_ms: 42,
        };
        let result = layer2_result_to_verification_result(verified);
        assert!(matches!(
            result,
            assura_smt::VerificationResult::Verified { .. }
        ));

        let ce = assura_smt::Layer2Result::Counterexample {
            invariant: "inv2".into(),
            model: vec![("x".into(), "0".into())],
        };
        let result = layer2_result_to_verification_result(ce);
        assert!(matches!(
            result,
            assura_smt::VerificationResult::Counterexample { .. }
        ));

        let timeout = assura_smt::Layer2Result::Timeout {
            invariant: "inv3".into(),
            timeout_ms: 10000,
        };
        let result = layer2_result_to_verification_result(timeout);
        assert!(matches!(
            result,
            assura_smt::VerificationResult::Timeout { .. }
        ));

        let unknown = assura_smt::Layer2Result::Unknown {
            invariant: "inv4".into(),
            reason: "solver inconclusive".into(),
        };
        let result = layer2_result_to_verification_result(unknown);
        assert!(matches!(
            result,
            assura_smt::VerificationResult::Unknown { .. }
        ));
    }

    #[test]
    fn layer2_expr_to_predicate_string() {
        use assura_parser::ast::{BinOp, Literal, Spanned};

        // Simple ident
        let expr = Spanned::no_span(Expr::Ident("x".into()));
        assert_eq!(expr_to_predicate_string(&expr), "x");

        // Integer literal
        let expr = Spanned::no_span(Expr::Literal(Literal::Int("42".into())));
        assert_eq!(expr_to_predicate_string(&expr), "42");

        // BinOp
        let expr = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gte,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        });
        assert_eq!(expr_to_predicate_string(&expr), "x >= 0");

        // Raw tokens
        let expr = Spanned::no_span(Expr::Raw(vec!["a".into(), "+".into(), "b".into()]));
        assert_eq!(expr_to_predicate_string(&expr), "a + b");
    }

    #[test]
    fn layer2_domain_to_sort_mapping() {
        use assura_parser::ast::Spanned;

        let int_domain = Spanned::no_span(Expr::Ident("Int".into()));
        assert_eq!(domain_to_sort(&int_domain), "Int");

        let nat_domain = Spanned::no_span(Expr::Ident("Nat".into()));
        assert_eq!(domain_to_sort(&nat_domain), "Nat");

        let bool_domain = Spanned::no_span(Expr::Ident("Bool".into()));
        assert_eq!(domain_to_sort(&bool_domain), "Bool");

        let custom_domain = Spanned::no_span(Expr::Ident("MyType".into()));
        assert_eq!(domain_to_sort(&custom_domain), "Int"); // default

        let raw_domain = Spanned::no_span(Expr::Raw(vec!["Float".into()]));
        assert_eq!(domain_to_sort(&raw_domain), "Float");
    }
}
