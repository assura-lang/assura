//! CVC5 binary invocation for shell-out verification.

use assura_ast::ClauseKind;

use crate::VerificationResult;
use crate::cvc5_verify_shared::{
    Cvc5ClauseSatOutcome, cvc5_interpret_clause_check_result, cvc5_sat_outcome_from_smtlib_model,
};

/// Result of running CVC5 binary on an SMT-LIB2 script.
pub(crate) enum Cvc5Result {
    Unsat,
    Sat(String),
    Timeout,
    Error(String),
}

pub(crate) fn run_cvc5_binary(script: &str) -> Cvc5Result {
    match execute_cvc5(script) {
        Ok(stdout) => parse_cvc5_stdout_first(&stdout),
        Err(reason) => Cvc5Result::Error(reason),
    }
}

pub(crate) fn cvc5_shell_query_to_verification_result(
    desc: &str,
    kind: ClauseKind,
    query: Cvc5Result,
) -> VerificationResult {
    match query {
        Cvc5Result::Unsat => {
            cvc5_interpret_clause_check_result(desc, kind, Cvc5ClauseSatOutcome::Unsat)
        }
        Cvc5Result::Sat(model_str) => cvc5_interpret_clause_check_result(
            desc,
            kind,
            cvc5_sat_outcome_from_smtlib_model(model_str),
        ),
        Cvc5Result::Timeout => {
            cvc5_interpret_clause_check_result(desc, kind, Cvc5ClauseSatOutcome::Timeout)
        }
        Cvc5Result::Error(reason) => VerificationResult::Unknown {
            clause_desc: desc.to_string(),
            reason,
        },
    }
}

pub(crate) fn run_cvc5_binary_queries(script: &str) -> Result<Vec<Cvc5Result>, String> {
    let stdout = execute_cvc5(script)?;
    parse_cvc5_stdout_all(&stdout)
}

fn execute_cvc5(script: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut cmd = Command::new("cvc5");
    cmd.arg("--lang")
        .arg("smt2")
        .arg("--tlimit")
        .arg("1000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cvc5 not found on PATH: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .map_err(|e| format!("Failed to write SMT script to CVC5 stdin: {e}"))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("cvc5 execution failed: {e}"))?;

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn is_query_line(line: &str) -> bool {
    matches!(
        line,
        "sat" | "unsat" | "timeout" | "resourceout" | "unknown"
    )
}

fn parse_cvc5_stdout_first(stdout: &str) -> Cvc5Result {
    match parse_cvc5_stdout_all(stdout) {
        Ok(mut results) if !results.is_empty() => results.remove(0),
        Ok(_) => Cvc5Result::Error("cvc5 produced no check-sat results".into()),
        Err(reason) => Cvc5Result::Error(reason),
    }
}

fn parse_cvc5_stdout_all(stdout: &str) -> Result<Vec<Cvc5Result>, String> {
    let lines: Vec<&str> = stdout.lines().collect();
    let mut results = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        if line.is_empty() {
            i += 1;
            continue;
        }

        match line {
            "unsat" => {
                results.push(Cvc5Result::Unsat);
                i += 1;
            }
            "sat" => {
                i += 1;
                let mut model_lines = Vec::new();
                while i < lines.len() && !is_query_line(lines[i].trim()) {
                    model_lines.push(lines[i]);
                    i += 1;
                }
                results.push(Cvc5Result::Sat(model_lines.join("\n")));
            }
            "timeout" | "resourceout" | "unknown" => {
                results.push(Cvc5Result::Timeout);
                i += 1;
            }
            _ => {
                if results.is_empty() {
                    return Err(format!("unexpected cvc5 output: {line}"));
                }
                i += 1;
            }
        }
    }

    if results.is_empty() {
        return Err("cvc5 produced no check-sat results".into());
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VerificationResult;
    use assura_ast::ClauseKind;

    #[test]
    fn shell_query_helper_maps_unsat_to_verified_ensures() {
        let result = cvc5_shell_query_to_verification_result(
            "T::Ensures",
            ClauseKind::Ensures,
            Cvc5Result::Unsat,
        );
        assert!(matches!(
            result,
                VerificationResult::Verified { clause_desc, .. } if clause_desc == "T::Ensures"
        ));
    }

    #[test]
    fn shell_query_helper_maps_sat_to_counterexample() {
        let result = cvc5_shell_query_to_verification_result(
            "T::Ensures",
            ClauseKind::Ensures,
            Cvc5Result::Sat("(define-fun x () Int 0)".into()),
        );
        match result {
            VerificationResult::Counterexample {
                clause_desc, model, ..
            } => {
                assert_eq!(clause_desc, "T::Ensures");
                assert!(model.contains("x = 0"), "model should name x: {model}");
            }
            other => panic!("expected Counterexample, got {other:?}"),
        }
    }

    #[test]
    fn parse_multi_query_stdout() {
        let stdout = "unsat\nsat\n(define-fun x () Int 1)\nunsat\n";
        let results = parse_cvc5_stdout_all(stdout).unwrap();
        assert_eq!(results.len(), 3);
        assert!(matches!(results[0], Cvc5Result::Unsat));
        assert!(matches!(results[1], Cvc5Result::Sat(_)));
        assert!(matches!(results[2], Cvc5Result::Unsat));
    }
}
