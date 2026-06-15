use super::*;

// `assura repl` -- interactive contract playground
// ---------------------------------------------------------------------------

pub(crate) fn run_repl() {
    use std::io::{self, BufRead, Write};

    println!("Assura REPL v{}", env!("CARGO_PKG_VERSION"));
    println!("Type a contract to parse and verify. Commands:");
    println!("  :type <rust_type>     Show Assura type mapping");
    println!("  :explain <code>       Explain an error code");
    println!("  :load <file>          Load and verify a file");
    println!("  :quit or Ctrl-D       Exit");
    println!();

    let stdin = io::stdin();
    let mut buffer = String::new();
    let mut in_block = false;
    let mut brace_depth: i32 = 0;

    loop {
        if in_block {
            eprint!("  ... ");
        } else {
            eprint!("assura> ");
        }
        io::stderr().flush().ok();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                eprintln!();
                break;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error reading input: {e}");
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
                println!("{rust_type} -> {assura_type}");
                continue;
            }
            if trimmed == ":type" {
                eprintln!("Usage: :type <rust_type>  (e.g., :type Vec<Option<i64>>)");
                continue;
            }
            if let Some(code) = trimmed.strip_prefix(":explain ") {
                repl_explain(code.trim());
                continue;
            }
            if trimmed == ":explain" {
                eprintln!("Usage: :explain <code>  (e.g., :explain A03001)");
                continue;
            }
            if let Some(file) = trimmed.strip_prefix(":load ") {
                repl_load(file.trim());
                continue;
            }
            if trimmed == ":load" {
                eprintln!("Usage: :load <file.assura>");
                continue;
            }
            if trimmed.starts_with(':') {
                eprintln!("Unknown command: {trimmed}");
                eprintln!("Available: :type, :explain, :load, :quit");
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
                repl_eval(&input);
            }
            buffer.clear();
        }
    }
}

pub(crate) fn repl_explain(code: &str) {
    if let Some(info) = assura_diagnostics::explain(code) {
        println!("{} ({})", info.code, info.name);
        println!("  {}", info.description);
        if !info.example.is_empty() {
            println!("  Example: {}", info.example);
        }
        if !info.fix.is_empty() {
            println!("  Fix: {}", info.fix);
        }
    } else {
        eprintln!("Unknown error code: {code}");
    }
}

pub(crate) fn repl_load(path: &str) {
    match fs::read_to_string(path) {
        Ok(source) => repl_eval(&source),
        Err(e) => eprintln!("Error loading {path}: {e}"),
    }
}

pub(crate) fn repl_eval(source: &str) {
    let result = assura_pipeline::run(source);

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
