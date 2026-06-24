//! Verification output formatting and display helpers.
//!
//! Provides functions for printing verification results grouped by
//! contract/service/function, formatting counterexamples, and printing
//! declaration summaries. All output functions write to a `&mut dyn Write`
//! so callers can direct output to stdout, stderr, or any other sink.

use std::io::Write;

use assura_ast::{
    BindDecl, ClauseKind, ContractDecl, DeclVisitor, ExternDecl, FnDef, ServiceDecl, SourceFile,
    walk_decls,
};
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
    write_grouped_verification_with_cores(w, results, indent, false)
}

/// Like [`write_grouped_verification`], but optionally prints unsat cores.
pub fn write_grouped_verification_with_cores(
    w: &mut dyn Write,
    results: &[VerificationResult],
    indent: &str,
    show_cores: bool,
) -> std::io::Result<()> {
    let mut groups: Vec<(String, Vec<&VerificationResult>)> = Vec::new();

    for vr in results {
        let desc = match vr {
            VerificationResult::Verified { clause_desc, .. }
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
                VerificationResult::Verified {
                    clause_desc,
                    unsat_core,
                } => {
                    let kind = clause_desc.split("::").nth(1).unwrap_or(clause_desc);
                    writeln!(w, "{indent}  {kind:<20} ... verified")?;
                    if show_cores
                        && let Some(core) = unsat_core
                        && !core.is_empty()
                    {
                        writeln!(w, "{indent}    unsat core: {}", core.join(", "))?;
                    }
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
    // Fallback: parse raw Z3 model into variable assignments.
    // The raw model contains lines like `name -> value` and multi-line
    // `name -> {\n  value\n}` blocks. Parse them into clean pairs.
    let mut lines = Vec::new();
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut current_name: Option<String> = None;
    let mut block_lines: Vec<String> = Vec::new();
    let mut in_block = false;

    for line in model.lines() {
        let trimmed = line.trim();
        if in_block {
            if trimmed == "}" {
                // End of block: join inner lines as the value
                let value = block_lines.join(" ");
                if let Some(name) = current_name.take() {
                    // Skip internal Z3 variables
                    if !name.starts_with("__") || name == crate::encode_atom_policy::RESULT_VAR_NAME
                    {
                        let clean_name = name.strip_prefix("__field_").unwrap_or(&name).to_string();
                        pairs.push((clean_name, clean_z3_value(&value)));
                    }
                }
                block_lines.clear();
                in_block = false;
            } else {
                block_lines.push(trimmed.to_string());
            }
        } else if let Some((name, rest)) = trimmed.split_once(" -> ") {
            let rest = rest.trim();
            if rest == "{" {
                // Start of multi-line block
                current_name = Some(name.to_string());
                in_block = true;
            } else {
                // Single-line assignment
                let name = name.trim();
                if !name.starts_with("__") || name == crate::encode_atom_policy::RESULT_VAR_NAME {
                    let clean_name = name.strip_prefix("__field_").unwrap_or(name).to_string();
                    pairs.push((clean_name, clean_z3_value(rest)));
                }
            }
        }
    }

    if pairs.is_empty() {
        // Could not parse; show raw model lines
        return model.lines().map(|l| format!("| {l}")).collect();
    }

    // Format parsed pairs as clean counterexample
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    for (name, value) in &pairs {
        if name == "result" || name.starts_with("result") {
            outputs.push((name.clone(), value.clone()));
        } else {
            inputs.push((name.clone(), value.clone()));
        }
    }
    if !inputs.is_empty() {
        let formatted: Vec<String> = inputs.iter().map(|(n, v)| format!("{n} = {v}")).collect();
        lines.push(format!("| {}", formatted.join(", ")));
    }
    for (name, value) in &outputs {
        lines.push(format!("| {name} = {value}"));
    }
    lines
}

/// Clean up Z3 value formatting for human display.
///
/// Handles several Z3 output patterns:
/// - `(- N)` negative numbers -> `-N`
/// - Multi-line `{\n  value\n}` blocks -> just the inner value
/// - `(/ p q)` rationals -> `p/q`
pub fn clean_z3_value(value: &str) -> String {
    let v = value.trim();
    // Z3 outputs negative numbers as `(- N)`, convert to `-N`
    if v.starts_with("(- ") && v.ends_with(')') {
        let inner = v[3..v.len() - 1].trim();
        return format!("-{inner}");
    }
    // Z3 rational output: `(/ p q)` -> `p/q`
    if v.starts_with("(/ ") && v.ends_with(')') {
        let inner = &v[3..v.len() - 1];
        if let Some((p, q)) = inner.split_once(' ') {
            return format!("{}/{}", p.trim(), q.trim());
        }
    }
    // Multi-line block: `{\n  value\n}` -> extract the value
    if v.starts_with('{') && v.ends_with('}') {
        let inner = v[1..v.len() - 1].trim();
        if !inner.is_empty() {
            return clean_z3_value(inner);
        }
    }
    v.to_string()
}

/// Collect names of all contracts, services, and extern fns that could
/// potentially have verifiable clauses.
///
/// Uses [`DeclVisitor`] so new `Decl` variants only need an arm in `walk_decl`,
/// not another open-coded match here.
pub fn collect_contract_names(file: &SourceFile) -> Vec<String> {
    struct VerifiableNames(Vec<String>);

    impl DeclVisitor for VerifiableNames {
        fn visit_contract(&mut self, c: &ContractDecl) {
            self.0.push(c.name.clone());
        }
        fn visit_service(&mut self, s: &ServiceDecl) {
            self.0.push(s.name.clone());
        }
        fn visit_extern(&mut self, ex: &ExternDecl) {
            if ex
                .clauses
                .iter()
                .any(|cl| matches!(cl.kind, ClauseKind::Ensures | ClauseKind::Invariant))
            {
                self.0.push(ex.name.clone());
            }
        }
        fn visit_fn_def(&mut self, f: &FnDef) {
            if f.clauses.iter().any(|cl| {
                matches!(
                    cl.kind,
                    ClauseKind::Ensures | ClauseKind::Invariant | ClauseKind::Decreases
                )
            }) {
                self.0.push(f.name.clone());
            }
        }
        fn visit_bind(&mut self, b: &BindDecl) {
            if b.clauses
                .iter()
                .any(|cl| matches!(cl.kind, ClauseKind::Ensures | ClauseKind::Invariant))
            {
                self.0.push(b.name.clone());
            }
        }
    }

    let mut v = VerifiableNames(Vec::new());
    walk_decls(&mut v, &file.decls);
    v.0
}

/// Dispatch pending decrease checks from the type checker to the SMT solver.
///
/// The type checker identifies recursive calls where syntactic checking is
/// inconclusive and returns `PendingDecreaseCheck` entries. This function
/// sends each one to `verify_decrease()` and returns the results as
/// `VerificationResult`s that can be merged with the main verification output.
pub fn dispatch_decrease_checks(typed: &TypedFile) -> Vec<VerificationResult> {
    use assura_ast::Spanned;
    typed
        .pending_decrease_checks
        .iter()
        .map(|check| {
            let desc = format!("{}::decreases({})", check.fn_name, "termination");
            let preconditions: Vec<_> = check
                .preconditions
                .iter()
                .map(|e| Spanned::no_span(e.clone()))
                .collect();
            let measure = Spanned::no_span(check.measure_expr.clone());
            let call_arg = Spanned::no_span(check.call_arg.clone());
            crate::verify_decrease(&preconditions, &measure, &call_arg, desc)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CounterexampleModel;

    #[test]
    fn test_clean_z3_value_negative_number() {
        assert_eq!(clean_z3_value("(- 5)"), "-5");
    }

    #[test]
    fn test_clean_z3_value_large_negative() {
        assert_eq!(clean_z3_value("(- 42)"), "-42");
    }

    #[test]
    fn test_clean_z3_value_positive_number() {
        assert_eq!(clean_z3_value("5"), "5");
    }

    #[test]
    fn test_clean_z3_value_zero() {
        assert_eq!(clean_z3_value("0"), "0");
    }

    #[test]
    fn test_clean_z3_value_with_whitespace() {
        assert_eq!(clean_z3_value("  42  "), "42");
    }

    #[test]
    fn test_clean_z3_value_negative_with_whitespace() {
        assert_eq!(clean_z3_value("  (- 7)  "), "-7");
    }

    #[test]
    fn test_clean_z3_value_non_numeric() {
        assert_eq!(clean_z3_value("true"), "true");
    }

    #[test]
    fn test_counterexample_with_inputs() {
        let model = CounterexampleModel {
            variables: vec![
                ("x".to_string(), "42".to_string()),
                ("y".to_string(), "(- 3)".to_string()),
            ],
        };
        let lines = format_counterexample_lines(&Some(model), "raw model");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("x = 42"));
        assert!(lines[0].contains("y = -3"));
    }

    #[test]
    fn test_counterexample_with_result() {
        let model = CounterexampleModel {
            variables: vec![
                ("a".to_string(), "10".to_string()),
                ("result".to_string(), "(- 1)".to_string()),
            ],
        };
        let lines = format_counterexample_lines(&Some(model), "raw model");
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("a = 10"));
        assert!(lines[1].contains("result = -1"));
    }

    #[test]
    fn test_counterexample_empty_model_fallback() {
        let model = CounterexampleModel { variables: vec![] };
        let raw = "x = 0\ny = -1";
        let lines = format_counterexample_lines(&Some(model), raw);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("x = 0"));
        assert!(lines[1].contains("y = -1"));
    }

    #[test]
    fn test_counterexample_none_model() {
        let raw = "some raw model output";
        let lines = format_counterexample_lines(&None, raw);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("some raw model output"));
    }

    #[test]
    fn test_counterexample_multiline_raw() {
        let raw = "line 1\nline 2\nline 3";
        let lines = format_counterexample_lines(&None, raw);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "| line 1");
        assert_eq!(lines[1], "| line 2");
        assert_eq!(lines[2], "| line 3");
    }

    #[test]
    fn test_counterexample_with_dunder_prefix() {
        let model = CounterexampleModel {
            variables: vec![("__x".to_string(), "99".to_string())],
        };
        let lines = format_counterexample_lines(&Some(model), "");
        assert!(lines[0].contains("x = 99"));
        assert!(!lines[0].contains("__x"));
    }

    #[test]
    fn test_clause_owner_simple() {
        assert_eq!(clause_owner("SafeDivide::ensures"), "SafeDivide");
    }

    #[test]
    fn test_clause_owner_service_op() {
        assert_eq!(
            clause_owner("OrderService.pay::requires"),
            "OrderService.pay"
        );
    }

    #[test]
    fn test_clause_owner_no_separator() {
        assert_eq!(clause_owner("standalone"), "standalone");
    }

    #[test]
    fn test_write_grouped_verified() {
        let results = vec![VerificationResult::verified("Foo::ensures")];
        let mut buf = Vec::new();
        write_grouped_verification(&mut buf, &results, "  ").unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Foo:"));
        assert!(output.contains("ensures"));
        assert!(output.contains("verified"));
    }

    #[test]
    fn test_write_grouped_counterexample() {
        let results = vec![VerificationResult::Counterexample {
            clause_desc: "Bar::invariant".to_string(),
            model: "x = 0".to_string(),
            counter_model: None,
        }];
        let mut buf = Vec::new();
        write_grouped_verification(&mut buf, &results, "").unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Bar:"));
        assert!(output.contains("COUNTEREXAMPLE"));
    }

    #[test]
    fn test_write_grouped_timeout() {
        let results = vec![VerificationResult::Timeout {
            clause_desc: "Baz::ensures".to_string(),
        }];
        let mut buf = Vec::new();
        write_grouped_verification(&mut buf, &results, "").unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("timeout"));
    }

    #[test]
    fn test_write_grouped_unknown() {
        let results = vec![VerificationResult::Unknown {
            clause_desc: "Qux::requires".to_string(),
            reason: "non-linear arithmetic".to_string(),
        }];
        let mut buf = Vec::new();
        write_grouped_verification(&mut buf, &results, "").unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("skipped"));
        assert!(output.contains("non-linear arithmetic"));
    }

    #[test]
    fn test_write_grouped_multiple_same_owner() {
        let results = vec![
            VerificationResult::verified("C::requires"),
            VerificationResult::verified("C::ensures"),
        ];
        let mut buf = Vec::new();
        write_grouped_verification(&mut buf, &results, "").unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output.matches("C:").count(), 1);
        assert!(output.contains("requires"));
        assert!(output.contains("ensures"));
    }
}
