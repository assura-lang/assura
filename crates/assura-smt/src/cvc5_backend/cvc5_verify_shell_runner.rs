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

pub(crate) fn run_cvc5_binary(script: &str, tlimit_ms: u32) -> Cvc5Result {
    run_cvc5_binary_mode(script, tlimit_ms, false)
}

/// `QF_BV` scripts for machine division. Eager bitblasting is rejected on
/// `(set-logic ALL)`, so the integer shell scripts do not pass this flag.
pub(crate) fn run_cvc5_binary_eager(script: &str, tlimit_ms: u32) -> Cvc5Result {
    run_cvc5_binary_mode(script, tlimit_ms, true)
}

fn run_cvc5_binary_mode(script: &str, tlimit_ms: u32, eager_bitblast: bool) -> Cvc5Result {
    match execute_cvc5(script, tlimit_ms, eager_bitblast) {
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
            cvc5_interpret_clause_check_result(desc, kind, Cvc5ClauseSatOutcome::unsat())
        }
        Cvc5Result::Sat(model_str) => cvc5_interpret_clause_check_result(
            desc,
            kind,
            cvc5_sat_outcome_from_smtlib_model(model_str),
        ),
        Cvc5Result::Timeout => {
            cvc5_interpret_clause_check_result(desc, kind, Cvc5ClauseSatOutcome::timeout())
        }
        Cvc5Result::Error(reason) => {
            // Route through shared policy for consistent outcome classification (#466).
            cvc5_interpret_clause_check_result(desc, kind, Cvc5ClauseSatOutcome::unknown(reason))
        }
    }
}

pub(crate) fn run_cvc5_binary_queries(
    script: &str,
    tlimit_ms: u32,
) -> Result<Vec<Cvc5Result>, String> {
    run_cvc5_binary_queries_mode(script, tlimit_ms, false)
}

pub(crate) fn run_cvc5_binary_queries_eager(
    script: &str,
    tlimit_ms: u32,
) -> Result<Vec<Cvc5Result>, String> {
    run_cvc5_binary_queries_mode(script, tlimit_ms, true)
}

fn run_cvc5_binary_queries_mode(
    script: &str,
    tlimit_ms: u32,
    eager_bitblast: bool,
) -> Result<Vec<Cvc5Result>, String> {
    let stdout = execute_cvc5(script, tlimit_ms, eager_bitblast)?;
    parse_cvc5_stdout_all(&stdout)
}

/// Stop reading solver stdout past this size. A partial `get-model` is
/// discarded rather than parsed as a counterexample.
pub(crate) const CVC5_STDOUT_CAP: usize = 4 * 1024 * 1024;

/// Read `reader` until EOF and discard every byte. Unlike [`read_capped`],
/// this does not stop early: a capped drain would fill the pipe and stall
/// the child before the parent can kill it.
pub(crate) fn discard_until_eof(mut reader: impl std::io::Read) -> usize {
    let mut total = 0usize;
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => return total,
            Ok(n) => total = total.saturating_add(n),
        }
    }
}

/// Read `reader` until EOF. If another byte would pass `limit`, return an
/// error and do not yield a truncated buffer.
pub(crate) fn read_capped(mut reader: impl std::io::Read, limit: usize) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = reader
            .read(&mut chunk)
            .map_err(|e| format!("cvc5 stdout read failed: {e}"))?;
        if n == 0 {
            return Ok(buf);
        }
        if buf.len().saturating_add(n) > limit {
            return Err(format!(
                "cvc5 stdout exceeded {limit} bytes; output discarded so a partial model is not parsed"
            ));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

fn execute_cvc5(script: &str, tlimit_ms: u32, eager_bitblast: bool) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Re-resolve so short values still hit the shared floor (same helper as
    // native `tlimit` and Z3 clause timeout).
    let tlimit = crate::encode_timeout_policy::clause_timeout_tlimit(tlimit_ms as u64);
    let mut cmd = Command::new("cvc5");
    cmd.arg("--lang")
        .arg("smt2")
        .arg("--produce-models")
        // Multi-clause scripts use `(push 1)` / `(pop 1)`. cvc5 rejects
        // `set-option` for `:incremental` after `(set-logic ALL)` plus any
        // declaration, so the flag has to be on the command line. A single
        // `(check-sat)` still answers under this flag.
        .arg("--incremental")
        // 64-bit wrapping division (vault share formulas) makes the default
        // nonlinear solver hit the clause budget. Splitting on zero finds
        // the same counterexample Z3 finds, and linear unsat stays unsat.
        .arg("--nl-ext-split-zero");
    if eager_bitblast {
        // Nested 64-bit `bvsdiv` (vault rounding) stays lazy-bitblast until
        // the clause budget. Eager bitblasting finishes the same query.
        // Only legal for `QF_BV`; `ALL` errors with ackermann.
        cmd.arg("--bitblast=eager");
    }
    cmd.arg("--tlimit")
        .arg(tlimit)
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

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "cvc5 stdout pipe missing".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "cvc5 stderr pipe missing".to_string())?;
    // Drain stderr to EOF. Stopping at the stdout cap would leave the
    // pipe full and stall cvc5 inside write, before kill() runs.
    let stderr_thread = std::thread::spawn(move || drain_keeping_prefix(stderr, 4096));
    let stdout_result = read_capped(stdout, CVC5_STDOUT_CAP);
    if stdout_result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let stderr_bytes = stderr_thread.join().unwrap_or_default();
    let bytes = stdout_result?;
    let stdout_text = String::from_utf8_lossy(&bytes).into_owned();
    let stderr_text = String::from_utf8_lossy(&stderr_bytes);
    Ok(promote_solver_timeout(stdout_text, &stderr_text))
}

/// cvc5 prints `cvc5 interrupted by timeout.` on stderr and may leave
/// stdout empty. That is a timeout, not a script with no `(check-sat)`.
fn promote_solver_timeout(stdout: String, stderr: &str) -> String {
    if stdout.lines().any(|line| is_query_line(line.trim())) {
        return stdout;
    }
    if stderr.contains("interrupted by timeout") {
        return "timeout\n".to_string();
    }
    stdout
}

fn drain_keeping_prefix(mut reader: impl std::io::Read, keep: usize) -> Vec<u8> {
    let mut kept = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => return kept,
            Ok(n) => {
                if kept.len() >= keep {
                    let _ = discard_until_eof(reader);
                    return kept;
                }
                let room = keep - kept.len();
                let take = n.min(room);
                kept.extend_from_slice(&chunk[..take]);
                if take < n {
                    let _ = discard_until_eof(reader);
                    return kept;
                }
            }
        }
    }
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
            "timeout" | "resourceout" => {
                results.push(Cvc5Result::Timeout);
                i += 1;
            }
            // `unknown` is not a time limit. Calling it a timeout tells the
            // user to raise `--timeout` when the solver already finished.
            "unknown" => {
                results.push(Cvc5Result::Error("cvc5 returned unknown".into()));
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
    fn discard_until_eof_reads_past_the_stdout_cap() {
        let data = vec![7u8; CVC5_STDOUT_CAP + 32];
        let n = discard_until_eof(std::io::Cursor::new(data.clone()));
        assert_eq!(n, data.len());
        assert!(read_capped(std::io::Cursor::new(data), CVC5_STDOUT_CAP).is_err());
    }

    #[test]
    fn read_capped_rejects_output_past_the_limit() {
        let data = b"sat\n(partial model that must not be parsed)";
        let err = read_capped(&data[..], 4).expect_err("over the cap");
        assert!(
            err.contains("exceeded 4 bytes"),
            "partial stdout must not become a model: {err}"
        );
        let exact = read_capped(&data[..4], 4).expect("exact cap is complete");
        assert_eq!(exact, b"sat\n");
    }

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
    fn parse_unknown_is_not_a_timeout() {
        let parsed = parse_cvc5_stdout_first("unknown\n");
        match parsed {
            Cvc5Result::Error(reason) => {
                assert!(reason.contains("unknown"), "{reason}");
            }
            Cvc5Result::Timeout => panic!("unknown must not be reported as a timeout"),
            Cvc5Result::Unsat | Cvc5Result::Sat(_) => {
                panic!("unknown must not be sat or unsat")
            }
        }
    }

    #[test]
    fn empty_stdout_with_timeout_stderr_is_a_timeout() {
        let text = promote_solver_timeout(String::new(), "cvc5 interrupted by timeout.\n");
        assert!(matches!(
            parse_cvc5_stdout_first(&text),
            Cvc5Result::Timeout
        ));
    }

    #[test]
    fn cvc5_push_pop_returns_one_result_per_check_sat() {
        let Ok(out) = std::process::Command::new("cvc5").arg("--version").output() else {
            return;
        };
        if !out.status.success() {
            return;
        }
        // Declarations sit between set-logic and the pushes, which is where
        // an in-script `(set-option :incremental true)` makes cvc5 abort
        // before any check-sat.
        let script = "\
(set-logic ALL)
(declare-const x Int)
(push 1)
(assert (> x 0))
(check-sat)
(pop 1)
(push 1)
(assert (and (> x 0) (< x 0)))
(check-sat)
(pop 1)
";
        match run_cvc5_binary_queries(script, 5_000) {
            Ok(results) => {
                assert_eq!(results.len(), 2, "expected one result per check-sat");
                assert!(
                    matches!(results[0], Cvc5Result::Sat(_)),
                    "first query should be sat"
                );
                assert!(
                    matches!(results[1], Cvc5Result::Unsat),
                    "contradiction should be unsat"
                );
            }
            Err(reason) => panic!("cvc5 error: {reason}"),
        }
    }

    #[test]
    fn cvc5_wrapped_div_counterexample_is_sat() {
        let Ok(out) = std::process::Command::new("cvc5").arg("--version").output() else {
            return;
        };
        if !out.status.success() {
            return;
        }
        // VaultShareInflation: victim shares use 64-bit wrapping division.
        // The default nonlinear solver times out; `--nl-ext-split-zero` on
        // the cvc5 command line finds the counterexample.
        let script = "\
(set-logic ALL)
(declare-const total_shares_before Int)
(declare-const total_assets_before Int)
(declare-const donation Int)
(declare-const victim_deposit Int)
(declare-const attacker_deposit Int)
(assert (>= attacker_deposit 1))
(assert (>= donation 0))
(assert (>= victim_deposit 1))
(assert (>= total_shares_before 1))
(assert (>= total_assets_before 1))
(assert (= total_shares_before attacker_deposit))
(assert (= total_assets_before (- (mod (+ (+ attacker_deposit donation) 9223372036854775808) 18446744073709551616) 9223372036854775808)))
(assert (not (>= (div (* victim_deposit total_shares_before) total_assets_before) 1)))
(check-sat)
";
        match run_cvc5_binary(script, 8_000) {
            Cvc5Result::Sat(_) => {}
            Cvc5Result::Unsat => panic!("share inflation must have a counterexample"),
            Cvc5Result::Timeout => panic!("share inflation must not time out"),
            Cvc5Result::Error(reason) => panic!("cvc5 error: {reason}"),
        }
    }

    #[test]
    fn cvc5_false_ensures_without_adt_prelude_is_sat() {
        let Ok(out) = std::process::Command::new("cvc5").arg("--version").output() else {
            return;
        };
        if !out.status.success() {
            return;
        }
        let script = "\
(set-logic ALL)
(set-option :produce-models true)
(declare-const n Int)
(assert (and (>= n 0) (<= n 18446744073709551615)))
(assert (<= n 10))
(assert (not (> n 100)))
(check-sat)
(get-model)
";
        match run_cvc5_binary(script, 5_000) {
            Cvc5Result::Sat(model) => {
                assert!(
                    model.contains('0'),
                    "counterexample model should name a value: {model}"
                );
            }
            Cvc5Result::Timeout => panic!("false ensures must not time out"),
            Cvc5Result::Unsat => panic!("false ensures must not verify"),
            Cvc5Result::Error(reason) => panic!("cvc5 error: {reason}"),
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

    #[test]
    fn shell_tlimit_uses_resolved_clause_budget() {
        // File-level verify resolves via clause_timeout_ms; shell runner
        // receives the already-floored u32 and must not re-hardcode 10000.
        let short = crate::encode_timeout_policy::clause_timeout_ms(1_000);
        let long = crate::encode_timeout_policy::clause_timeout_ms(60_000);
        assert_eq!(
            short,
            crate::encode_timeout_policy::DEFAULT_SOLVER_TIMEOUT_MS
        );
        assert_eq!(long, 60_000);
        assert_ne!(
            long.to_string(),
            crate::encode_timeout_policy::DEFAULT_SOLVER_TIMEOUT_TLIMIT
        );
    }
}
