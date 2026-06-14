//! Verification output formatting and display helpers.
//!
//! Provides functions for printing verification results grouped by
//! contract/service/function, formatting counterexamples, and printing
//! declaration summaries. All output functions write to a `&mut dyn Write`
//! so callers can direct output to stdout, stderr, or any other sink.

use std::io::Write;

use assura_parser::ast::{ClauseKind, Decl, SourceFile};
use assura_types::TypedFile;

use crate::VerificationResult;

// ---------------------------------------------------------------------------
// Verification output helpers
// ---------------------------------------------------------------------------

/// Extract the contract/service/function name prefix from a clause description.
///
/// Clause descriptions have the form `"ContractName::clause_kind"` or
/// `"ServiceName.OpName::clause_kind"`.
pub fn clause_owner(clause_desc: &str) -> &str {
    clause_desc.split("::").next().unwrap_or(clause_desc)
}

/// Write verification results grouped by contract/service/function name.
///
/// The `indent` parameter controls the base indentation level. Each nested
/// level adds two more spaces. For example, with `indent = "  "`:
///
/// ```text
///   ContractName:
///     ensures              ... verified
///     invariant            ... COUNTEREXAMPLE
///       | x = 42
/// ```
pub fn write_grouped_verification(
    w: &mut dyn Write,
    results: &[VerificationResult],
    indent: &str,
) -> std::io::Result<()> {
    let mut groups: Vec<(String, Vec<&VerificationResult>)> = Vec::new();

    for vr in results {
        let desc = match vr {
            VerificationResult::Verified { clause_desc }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc }
            | VerificationResult::Unknown { clause_desc, .. } => clause_desc.as_str(),
        };
        let owner = clause_owner(desc).to_string();

        if let Some(group) = groups.iter_mut().find(|(name, _)| *name == owner) {
            group.1.push(vr);
        } else {
            groups.push((owner, vec![vr]));
        }
    }

    for (owner, results) in &groups {
        writeln!(w, "{indent}{owner}:")?;
        for vr in results {
            match vr {
                VerificationResult::Verified { clause_desc } => {
                    let kind = clause_desc.split("::").nth(1).unwrap_or(clause_desc);
                    writeln!(w, "{indent}  {kind:<20} ... verified")?;
                }
                VerificationResult::Counterexample {
                    clause_desc,
                    model,
                    counter_model,
                } => {
                    let kind = clause_desc.split("::").nth(1).unwrap_or(clause_desc);
                    writeln!(w, "{indent}  {kind:<20} ... COUNTEREXAMPLE")?;
                    for line in format_counterexample_lines(counter_model, model) {
                        writeln!(w, "{indent}    {line}")?;
                    }
                }
                VerificationResult::Timeout { clause_desc } => {
                    let kind = clause_desc.split("::").nth(1).unwrap_or(clause_desc);
                    writeln!(w, "{indent}  {kind:<20} ... timeout")?;
                }
                VerificationResult::Unknown {
                    clause_desc,
                    reason,
                } => {
                    let kind = clause_desc.split("::").nth(1).unwrap_or(clause_desc);
                    writeln!(w, "{indent}  {kind:<20} ... skipped ({reason})")?;
                }
            }
        }
    }

    Ok(())
}

/// Format a counterexample for human-readable display.
///
/// If a structured `CounterexampleModel` is available, display clean
/// `name = value` pairs. Otherwise fall back to the raw Z3 model string.
pub fn format_counterexample_lines(
    counter_model: &Option<crate::CounterexampleModel>,
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
///
/// Converts Z3's `(- N)` negative number format to `-N`.
pub fn clean_z3_value(value: &str) -> String {
    let v = value.trim();
    // Z3 outputs negative numbers as `(- N)`, convert to `-N`
    if v.starts_with("(- ") && v.ends_with(')') {
        return format!("-{}", &v[3..v.len() - 1]);
    }
    v.to_string()
}

/// Collect names of all contracts, services, and extern fns that could
/// potentially have verifiable clauses.
pub fn collect_contract_names(file: &SourceFile) -> Vec<String> {
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
/// sends each one to `verify_decrease()` and returns the results as
/// `VerificationResult`s that can be merged with the main verification output.
pub fn dispatch_decrease_checks(typed: &TypedFile) -> Vec<VerificationResult> {
    typed
        .pending_decrease_checks
        .iter()
        .map(|check| {
            let desc = format!("{}::decreases({})", check.fn_name, "termination");
            crate::verify_decrease(
                &check.preconditions,
                &check.measure_expr,
                &check.call_arg,
                desc,
            )
        })
        .collect()
}

/// Print a declaration summary to the given writer.
///
/// Displays file info (project, module, imports, declarations), resolution
/// and type-check status, and verification results grouped by contract.
pub fn write_summary(
    w: &mut dyn Write,
    filename: &str,
    file: &SourceFile,
    symbols: &assura_resolve::SymbolTable,
    type_env: &assura_types::TypeEnv,
    verification_results: &[VerificationResult],
) -> std::io::Result<()> {
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

    writeln!(w, "OK  {filename}")?;
    if let Some(p) = &file.project {
        writeln!(
            w,
            "    project:   {}  profile: [{}]",
            p.name,
            p.profile.join(", ")
        )?;
    }
    if let Some(m) = &file.module {
        writeln!(w, "    module:    {}", m.path.join("."))?;
    }
    writeln!(w, "    imports:   {}", file.imports.len())?;

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
    writeln!(
        w,
        "    declares:  {}",
        if parts.is_empty() {
            "(empty)".to_string()
        } else {
            parts.join(", ")
        }
    )?;
    let user_symbols = symbols
        .symbols
        .iter()
        .filter(|s| s.kind != assura_resolve::SymbolKind::BuiltinType)
        .count();
    writeln!(w, "    resolve:   OK ({user_symbols} symbols)")?;
    writeln!(w, "    typecheck: OK ({} bindings)", type_env.len())?;

    if verification_results.is_empty() {
        let contract_names = collect_contract_names(file);
        if contract_names.is_empty() {
            writeln!(w, "    verify:    OK (no verifiable clauses)")?;
        } else {
            writeln!(
                w,
                "    verify:    OK (no verifiable clauses in {})",
                contract_names.join(", ")
            )?;
        }
    } else {
        let verified = verification_results
            .iter()
            .filter(|r| matches!(r, VerificationResult::Verified { .. }))
            .count();
        let cex = verification_results
            .iter()
            .filter(|r| matches!(r, VerificationResult::Counterexample { .. }))
            .count();
        let timeout = verification_results
            .iter()
            .filter(|r| matches!(r, VerificationResult::Timeout { .. }))
            .count();
        let unknown = verification_results
            .iter()
            .filter(|r| matches!(r, VerificationResult::Unknown { .. }))
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
        writeln!(
            w,
            "    verify:    {} clause(s): {}",
            verification_results.len(),
            parts.join(", ")
        )?;
        // Show per-clause details
        write_grouped_verification(w, verification_results, "      ")?;
    }

    Ok(())
}
