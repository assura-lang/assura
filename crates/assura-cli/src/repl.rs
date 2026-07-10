use super::*;

// `assura repl` -- interactive contract playground
// ---------------------------------------------------------------------------

pub(crate) fn run_repl(output_mode: OutputMode) {
    use std::io::{self, BufRead, Write};

    let json = output_mode == OutputMode::Json;
    if !json {
        println!("Assura REPL v{}", env!("CARGO_PKG_VERSION"));
        println!("Type a contract to parse and verify. Commands:");
        println!("  :type <rust_type>     Show Assura type mapping");
        println!("  :explain <code>       Explain an error code");
        println!("  :load <file>          Load and verify a file");
        println!("  :quit or Ctrl-D       Exit");
        println!();
    }

    let stdin = io::stdin();
    let mut buffer = String::new();
    let mut in_block = false;
    let mut brace_depth: i32 = 0;

    loop {
        if !json {
            if in_block {
                eprint!("  ... ");
            } else {
                eprint!("assura> ");
            }
            io::stderr().flush().ok();
        }

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                if !json {
                    eprintln!();
                }
                break;
            }
            Ok(_) => {}
            Err(e) => {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ok": false,
                            "error": format!("Error reading input: {e}"),
                        })
                    );
                } else {
                    eprintln!("Error reading input: {e}");
                }
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
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ok": true,
                            "command": "type",
                            "rust": rust_type.trim(),
                            "assura": assura_type,
                        })
                    );
                } else {
                    println!("{rust_type} -> {assura_type}");
                }
                continue;
            }
            if trimmed == ":type" {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ok": false,
                            "error": "usage",
                            "usage": ":type <rust_type>",
                        })
                    );
                } else {
                    eprintln!("Usage: :type <rust_type>  (e.g., :type Vec<Option<i64>>)");
                }
                continue;
            }
            if let Some(code) = trimmed.strip_prefix(":explain ") {
                repl_explain(code.trim(), json);
                continue;
            }
            if trimmed == ":explain" {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ok": false,
                            "error": "usage",
                            "usage": ":explain <code>",
                        })
                    );
                } else {
                    eprintln!("Usage: :explain <code>  (e.g., :explain A03001)");
                }
                continue;
            }
            if let Some(file) = trimmed.strip_prefix(":load ") {
                repl_load(file.trim(), json);
                continue;
            }
            if trimmed == ":load" {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ok": false,
                            "error": "usage",
                            "usage": ":load <file.assura>",
                        })
                    );
                } else {
                    eprintln!("Usage: :load <file.assura>");
                }
                continue;
            }
            if trimmed.starts_with(':') {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ok": false,
                            "error": "unknown_command",
                            "command": trimmed,
                            "available": [":type", ":explain", ":load", ":quit"],
                        })
                    );
                } else {
                    eprintln!("Unknown command: {trimmed}");
                    eprintln!("Available: :type, :explain, :load, :quit");
                }
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
                repl_eval(&input, json);
            }
            buffer.clear();
        }
    }
}

pub(crate) fn repl_explain(code: &str, json: bool) {
    if let Some(info) = assura_diagnostics::explain(code) {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "ok": true,
                    "command": "explain",
                    "code": info.code,
                    "name": info.name,
                    "description": info.description,
                    "example": info.example,
                    "fix": info.fix,
                })
            );
        } else {
            println!("{} ({})", info.code, info.name);
            println!("  {}", info.description);
            if !info.example.is_empty() {
                println!("  Example: {}", info.example);
            }
            if !info.fix.is_empty() {
                println!("  Fix: {}", info.fix);
            }
        }
    } else if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": false,
                "command": "explain",
                "error": format!("Unknown error code: {code}"),
                "code": code,
            })
        );
    } else {
        eprintln!("Unknown error code: {code}");
    }
}

pub(crate) fn repl_load(path: &str, json: bool) {
    match fs::read_to_string(path) {
        Ok(source) => repl_eval(&source, json),
        Err(e) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": false,
                        "command": "load",
                        "file": path,
                        "error": format!("{e}"),
                    })
                );
            } else {
                eprintln!("Error loading {path}: {e}");
            }
        }
    }
}

pub(crate) fn repl_eval(source: &str, json: bool) {
    let result = assura_pipeline::run(source);

    if json {
        let parse_errors: Vec<_> = result
            .parse_errors
            .iter()
            .map(|d| serde_json::json!({"message": d.message}))
            .collect();
        if !parse_errors.is_empty() {
            println!(
                "{}",
                serde_json::json!({
                    "ok": false,
                    "phase": "parse",
                    "errors": parse_errors,
                })
            );
            return;
        }
        if result.declarations.is_empty() {
            println!(
                "{}",
                serde_json::json!({
                    "ok": false,
                    "error": "no_declarations",
                    "message": "No declarations found.",
                })
            );
            return;
        }
        let resolution_errors: Vec<_> = result
            .resolution_errors
            .iter()
            .map(|d| serde_json::json!({"code": d.code, "message": d.message}))
            .collect();
        let type_errors: Vec<_> = result
            .type_errors
            .iter()
            .map(|d| serde_json::json!({"code": d.code, "message": d.message}))
            .collect();
        let verification: Vec<_> = result
            .verification
            .iter()
            .map(|entry| {
                let mut v = serde_json::json!({
                    "clause": entry.clause,
                    "status": entry.status,
                });
                if let Some(model) = &entry.model {
                    v["model"] = serde_json::json!(model);
                }
                if let Some(reason) = &entry.reason {
                    v["reason"] = serde_json::json!(reason);
                }
                v
            })
            .collect();
        let ok = resolution_errors.is_empty() && type_errors.is_empty();
        println!(
            "{}",
            serde_json::json!({
                "ok": ok,
                "declarations": result.declarations,
                "resolution_errors": resolution_errors,
                "type_errors": type_errors,
                "verification": verification,
            })
        );
        return;
    }

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

#[cfg(test)]
mod tests {
    #[test]
    fn repl_eval_valid_contract_does_not_panic() {
        // repl_eval prints to stdout/stderr; verify it does not panic on valid input.
        let source = r#"
contract SafeDiv {
    input(a: Int, b: Int)
    output(result: Int)
    requires { b != 0 }
}
"#;
        super::repl_eval(source, false);
        super::repl_eval(source, true);
    }

    #[test]
    fn repl_eval_empty_source_does_not_panic() {
        // Empty source should produce "No declarations found." on stderr, not panic.
        super::repl_eval("", false);
        super::repl_eval("", true);
    }

    #[test]
    fn repl_explain_known_code() {
        // A01001 is the "Unexpected character" error; explain should not panic.
        super::repl_explain("A01001", false);
        super::repl_explain("A01001", true);
    }

    #[test]
    fn repl_explain_unknown_code() {
        // Unknown codes should print an error message, not panic.
        super::repl_explain("Z99999", false);
        super::repl_explain("Z99999", true);
    }
}
