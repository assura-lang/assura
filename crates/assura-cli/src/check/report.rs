//! Verification result reporting (human/JSON) and SMT Unknown classification.

use super::super::*;
use super::types::VerifyContext;

/// Co-located and in-memory heuristic IR for check UX (human `-v` + JSON).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IrSurfaceListing {
    pub colocated: Vec<String>,
    pub synthesized: Vec<String>,
    /// `(contract_name, "body from …")` multi-ensures driver notes.
    pub notes: Vec<(String, String)>,
}

/// Disk sidecars once + one `stub_ir_sidecars` pass (co-publish-safe APIs only).
pub(crate) fn collect_ir_surface_listing(
    filename: &str,
    typed: &assura_types::TypedFile,
) -> IrSurfaceListing {
    let path = std::path::Path::new(filename);
    let disk = assura_smt::LoadedVerifyExtras::load(path, typed);
    let colocated = disk.loaded_names();
    let colocated_set: std::collections::HashSet<&str> =
        colocated.iter().map(String::as_str).collect();
    let texts = assura_smt::stub_ir_sidecars_for_typed(typed);
    let mut synthesized: Vec<String> = texts
        .iter()
        .filter(|(name, text)| !colocated_set.contains(name.as_str()) && !text.contains("Stub IR"))
        .map(|(name, _)| name.clone())
        .collect();
    synthesized.sort();
    let mut notes = Vec::new();
    for name in &synthesized {
        if let Some(text) = texts.get(name)
            && let Some(detail) = synth_note_detail_from_ir(text)
        {
            notes.push((name.clone(), detail));
        }
    }
    IrSurfaceListing {
        colocated,
        synthesized,
        notes,
    }
}

fn eprint_ir_surface_listing(listing: &IrSurfaceListing) {
    if listing.colocated.is_empty() && listing.synthesized.is_empty() {
        eprintln!("  ir:        no co-located sidecars and no synthesizable ensures");
        return;
    }
    if !listing.colocated.is_empty() {
        eprintln!(
            "  ir:        {} co-located sidecar(s): {}",
            listing.colocated.len(),
            listing.colocated.join(", ")
        );
    }
    if !listing.synthesized.is_empty() {
        eprintln!(
            "  ir:        synthesized in-memory: {}",
            listing.synthesized.join(", ")
        );
        for (name, detail) in &listing.notes {
            eprintln!("  ir:          {name} {detail}");
        }
    }
}

/// Shared verification + reporting logic used by both `run_check` and
/// `check_file_once` (watch mode). Returns the verification results and
/// whether errors were found.
pub(crate) fn verify_and_report(ctx: VerifyContext<'_>) -> Vec<assura_smt::VerificationResult> {
    let VerifyContext {
        filename,
        source,
        typed,
        file,
        diagnostics,
        has_errors,
        output_mode,
        verbosity,
        verify_options,
        show_cores,
        strict,
    } = ctx;
    let layer = verify_options.layer;
    // Short-circuit: skip cache/thread-pool init when there are no
    // verifiable clauses (requires/ensures/invariant) in the source.
    let has_clauses = file
        .as_ref()
        .is_some_and(assura_smt::has_verifiable_clauses);

    let verification_results = if layer >= 1 && has_clauses {
        typed.as_ref().map_or_else(Vec::new, |typed| {
            // Human -v: list IR before verify so agents/users see bodies used.
            if verbosity == Verbosity::Verbose && output_mode == OutputMode::Human {
                eprint_ir_surface_listing(&collect_ir_surface_listing(filename, typed));
            }
            let config = assura_config::CompilerConfig {
                verify: verify_options,
                ..Default::default()
            };
            assura_pipeline::verify_typed(typed, filename, &config)
        })
    } else {
        Vec::new()
    };

    // Build a lookup from contract/decl name to source span so SMT
    // diagnostics point to the originating declaration, not 0..0.
    let decl_spans = build_decl_span_map(file);

    if let Some(typed) = typed {
        let qwarnings = assura_smt::validate_quantifier_bounds(typed);
        for w in &qwarnings {
            let span = lookup_clause_span(&w.context, &decl_spans);
            diagnostics.push(
                assura_diagnostics::Diagnostic::warning(
                    "A05200",
                    format!(
                        "unbounded quantifier in {}: {} ({})",
                        w.context, w.domain_desc, w.reason
                    ),
                    span,
                )
                .with_file(filename),
            );
        }
    }

    // #703: Suppress A04008 "result unconstrained" warnings when the
    // corresponding ensures clause actually verified (IR sidecar loaded).
    let has_verified_ensures = verification_results.iter().any(|r| {
        matches!(
            r,
            assura_smt::VerificationResult::Verified { clause_desc, .. }
                if clause_desc.ends_with("::ensures")
        )
    });
    if has_verified_ensures {
        diagnostics.retain(|d| d.code != "A04008");
    }

    for vr in &verification_results {
        let clause_desc = match vr {
            assura_smt::VerificationResult::Counterexample { clause_desc, .. }
            | assura_smt::VerificationResult::Timeout { clause_desc }
            | assura_smt::VerificationResult::Unknown { clause_desc, .. }
            | assura_smt::VerificationResult::Verified { clause_desc, .. } => clause_desc,
        };
        let span = lookup_clause_span(clause_desc, &decl_spans);

        match vr {
            assura_smt::VerificationResult::Counterexample {
                clause_desc,
                model,
                counter_model,
            } => {
                *has_errors = true;
                let summary = format_counterexample_summary(counter_model, model);
                diagnostics.push(
                    assura_diagnostics::Diagnostic::error(
                        "A05100",
                        format!("verification failed for {clause_desc}: {summary}"),
                        span.clone(),
                    )
                    .with_file(filename),
                );
            }
            assura_smt::VerificationResult::Timeout { clause_desc } => {
                *has_errors = true;
                diagnostics.push(
                    assura_diagnostics::Diagnostic::error(
                        "A05101",
                        format!(
                            "verification timeout for {clause_desc} (consider increasing --timeout)"
                        ),
                        span.clone(),
                    )
                    .with_file(filename),
                );
            }
            assura_smt::VerificationResult::Unknown {
                clause_desc,
                reason,
            } => {
                if is_known_smt_limitation(reason) && !strict {
                    // #865: unconstrained-result path gets a dedicated help suggestion.
                    let mut diag = assura_diagnostics::Diagnostic::warning(
                        "A05102",
                        format!("verification skipped for {clause_desc}: {reason}"),
                        span.clone(),
                    )
                    .with_file(filename);
                    if reason.contains("result is unconstrained")
                        || reason.contains("`result` stays unconstrained")
                        || reason.contains("not auto-synthesizable")
                    {
                        diag = diag.with_suggestion(
                            "simplify ensures (result == expr / bounds), or materialize IR offline, \
                             or use LLM auto-implement",
                            span.clone(),
                            "assura build path.assura --write-ir  (offline); \
                             assura build path.assura --auto-implement  (LLM residuals)",
                        );
                    }
                    diagnostics.push(diag);
                } else if is_known_smt_limitation(reason) && strict {
                    *has_errors = true;
                    diagnostics.push(
                        assura_diagnostics::Diagnostic::error(
                            "A05102",
                            format!("verification skipped for {clause_desc} (--strict): {reason}"),
                            span.clone(),
                        )
                        .with_file(filename),
                    );
                } else {
                    *has_errors = true;
                    diagnostics.push(
                        assura_diagnostics::Diagnostic::error(
                            "A05103",
                            format!("verification inconclusive for {clause_desc}: {reason}"),
                            span.clone(),
                        )
                        .with_file(filename),
                    );
                }
            }
            assura_smt::VerificationResult::Verified { .. } => {}
        }
    }

    if output_mode == OutputMode::Human {
        // Render all diagnostics, including A01001 (unexpected character).
        // Filtering A01001 hid real errors (e.g. non-ASCII identifiers) while
        // JSON still reported them (dogfood: `contract Café`).
        if *has_errors || verbosity != Verbosity::Quiet {
            for d in diagnostics.iter() {
                assura_diagnostics::render_diagnostic(d, filename, source);
            }
        }

        if verbosity != Verbosity::Quiet {
            if !verification_results.is_empty() {
                eprintln!();
                eprintln!("Verification ({} clause(s)):", verification_results.len());
                let _ = assura_smt::display::write_grouped_verification_with_cores(
                    &mut std::io::stderr(),
                    &verification_results,
                    "  ",
                    show_cores,
                );
            } else if layer == 0 {
                eprintln!();
                eprintln!("Verification skipped (--layer 0: structural checks only)");
            } else if layer >= 1
                && !*has_errors
                && let Some(f) = file
            {
                // Only when parse/resolve/type already clean. On syntax errors the
                // "no verifiable clauses" block confuses users (Adversarial/UX).
                let contract_names = assura_smt::display::collect_contract_names(f);
                if !contract_names.is_empty() {
                    eprintln!();
                    eprintln!("Verification:");
                    for name in &contract_names {
                        // Hostile/oversized names must not flood the terminal
                        // (Adversarial: 10k-char contract id).
                        let display = assura_smt::display::truncate_display_name(name, 64);
                        // has_clauses means requires/ensures/invariant exist, but
                        // the SMT job collector may still emit nothing (e.g.
                        // requires-only: preconditions are assumed, not proved).
                        if has_clauses {
                            eprintln!("  {display}:  (no SMT proof obligations)");
                        } else {
                            eprintln!("  {display}:  (no verifiable clauses)");
                        }
                    }
                    eprintln!();
                    if has_clauses {
                        eprintln!(
                            "  hint: `requires` alone is assumed; add `ensures` or `invariant` to prove postconditions"
                        );
                    } else {
                        eprintln!(
                            "  hint: add `requires`, `ensures`, or `invariant` clauses to enable verification"
                        );
                    }
                }
            }

            let error_count = diagnostics
                .iter()
                .filter(|d| d.severity == assura_diagnostics::Severity::Error)
                .count();
            let warning_count = diagnostics
                .iter()
                .filter(|d| d.severity == assura_diagnostics::Severity::Warning)
                .count();
            if !*has_errors {
                // Vacuous success: empty sources, or contracts present but no
                // SMT-checkable clauses, pass every phase without proving
                // anything. Surface that so users/agents do not treat
                // "check passed" as proof of coverage (PM lesson, MPI).
                let no_decls = file.as_ref().is_some_and(|f| f.decls.is_empty());
                let contracts_without_results = layer >= 1
                    && verification_results.is_empty()
                    && file.as_ref().is_some_and(|f| {
                        !assura_smt::display::collect_contract_names(f).is_empty()
                    });
                let summary = success_summary_message(
                    no_decls,
                    contracts_without_results,
                    has_clauses,
                    warning_count,
                );
                eprintln!("{filename}: {summary}");
            } else if warning_count > 0 {
                eprintln!(
                    "{filename}: {error_count} error{}, {warning_count} warning{}",
                    if error_count == 1 { "" } else { "s" },
                    if warning_count == 1 { "" } else { "s" }
                );
            } else {
                eprintln!(
                    "{filename}: {error_count} error{}",
                    if error_count == 1 { "" } else { "s" }
                );
            }
        } else if *has_errors {
            let error_count = diagnostics
                .iter()
                .filter(|d| d.severity == assura_diagnostics::Severity::Error)
                .count();
            eprintln!(
                "{filename}: {error_count} error{}",
                if error_count == 1 { "" } else { "s" }
            );
        }
    }

    verification_results
}

// ---------------------------------------------------------------------------
// Span lookup + SMT limitation helper
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------

/// Human summary after a successful check (no hard errors).
/// Kept pure so unit tests lock the MPI vacuous-success wording.
pub(crate) fn success_summary_message(
    no_decls: bool,
    contracts_without_results: bool,
    has_clause_kinds: bool,
    warning_count: usize,
) -> String {
    if no_decls {
        "check passed (no contracts or functions to verify)".into()
    } else if contracts_without_results {
        if has_clause_kinds {
            "check passed (no SMT proof obligations; add ensures or invariant)".into()
        } else {
            "check passed (no verifiable clauses)".into()
        }
    } else if warning_count > 0 {
        format!(
            "check passed ({warning_count} warning{})",
            if warning_count == 1 { "" } else { "s" }
        )
    } else {
        "check passed (no errors)".into()
    }
}

/// Build a map from declaration name to source span.
/// Used to give SMT diagnostics real source locations instead of 0..0.
fn build_decl_span_map(
    file: &Option<assura_parser::ast::SourceFile>,
) -> std::collections::HashMap<String, std::ops::Range<usize>> {
    let mut map = std::collections::HashMap::new();
    if let Some(f) = file {
        for spanned in &f.decls {
            // Contracts, services, functions, and blocks are the names that
            // appear as clause_desc prefixes in SMT diagnostics.
            let include = matches!(
                &spanned.node,
                assura_parser::ast::Decl::Contract(_)
                    | assura_parser::ast::Decl::FnDef(_)
                    | assura_parser::ast::Decl::Block { .. }
                    | assura_parser::ast::Decl::Service(_)
            );
            if include && let Some(n) = spanned.node.name() {
                map.insert(n.to_string(), spanned.span.clone());
            }
        }
    }
    map
}

/// Extract a source span for a verification result's clause_desc.
/// clause_desc format: "ContractName::ClauseKind" or "ContractName: kind".
fn lookup_clause_span(
    clause_desc: &str,
    decl_spans: &std::collections::HashMap<String, std::ops::Range<usize>>,
) -> std::ops::Range<usize> {
    // Extract the name before "::" or ":"
    let name = clause_desc
        .split("::")
        .next()
        .or_else(|| clause_desc.split(':').next())
        .unwrap_or(clause_desc)
        .trim();
    decl_spans.get(name).cloned().unwrap_or(0..0)
}

/// Returns `true` if the given `VerificationResult::Unknown` reason represents
/// a known compiler limitation (warning, exit 0) rather than a genuine solver
/// inconclusive result (error, exit 1).
fn is_known_smt_limitation(reason: &str) -> bool {
    assura_smt::is_known_smt_limitation(reason)
}

/// Human detail from `// assura-synth-body` / residual comments in heuristic IR
/// (#1370). Co-publish safe: parses comment text only (no new smt crate APIs).
pub(crate) fn synth_note_detail_from_ir(ir_text: &str) -> Option<String> {
    let mut body: Option<String> = None;
    let mut residual: Option<String> = None;
    for line in ir_text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("// assura-synth-body:") {
            body = Some(rest.trim().to_string());
        } else if let Some(rest) = t.strip_prefix("// assura-synth-residual:") {
            residual = Some(rest.trim().to_string());
        }
    }
    let body = body?;
    Some(match residual {
        Some(r) if !r.is_empty() => {
            format!("body from {body}; residual {r} (not body driver)")
        }
        _ => format!("body from {body}"),
    })
}

#[cfg(test)]
mod synth_note_parse_tests {
    use super::synth_note_detail_from_ir;

    #[test]
    fn parses_body_and_residual() {
        let ir = r#"// Generated IR
// assura-synth-body: ensures#2 result_eq
// assura-synth-residual: ensures#1
module X {
}
"#;
        let d = synth_note_detail_from_ir(ir).expect("detail");
        assert!(
            d.contains("ensures#2 result_eq")
                && d.contains("ensures#1")
                && d.contains("not body driver"),
            "{d}"
        );
    }

    #[test]
    fn parses_combined_bounds_without_residual() {
        let ir = "// assura-synth-body: combined_bounds\nmodule Y {}\n";
        let d = synth_note_detail_from_ir(ir).expect("detail");
        assert_eq!(d, "body from combined_bounds");
    }

    #[test]
    fn missing_body_comment_returns_none() {
        assert!(synth_note_detail_from_ir("// Generated IR\nmodule Z {}\n").is_none());
    }
}
