//! CVC5 binary invocation for shell-out verification.

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
