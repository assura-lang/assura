use super::*;

// `assura fmt <file|dir> [--check]` — format .assura source file(s)
// ---------------------------------------------------------------------------

pub(crate) fn run_fmt(filename: &str, check_only: bool, output_mode: assura_config::OutputMode) {
    let json = output_mode == assura_config::OutputMode::Json;
    // Match `assura check -`: format stdin to stdout (or --check only).
    if is_stdin_arg(filename) {
        if !fmt_stdin(check_only, json) {
            process::exit(1);
        }
        return;
    }

    let path = Path::new(filename);
    if path.is_dir() {
        let mut files = Vec::new();
        collect_assura_files(path, &mut files);
        if files.is_empty() {
            eprintln!("Error: no .assura files found under {filename}");
            process::exit(2);
        }
        let mut failed = false;
        let mut results: Vec<serde_json::Value> = Vec::new();
        for file in &files {
            let path_str = file.to_string_lossy();
            // Suppress per-file JSON; emit one aggregate report below.
            let ok = fmt_one(path_str.as_ref(), check_only, false);
            if json && check_only {
                results.push(serde_json::json!({
                    "file": path_str,
                    "formatted": ok,
                }));
            }
            if !ok {
                failed = true;
            }
        }
        if json && check_only {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "ok": !failed,
                    "files": results,
                }))
                .unwrap()
            );
        }
        if failed {
            process::exit(1);
        }
        return;
    }

    if !fmt_one(filename, check_only, json) {
        process::exit(1);
    }
}

/// Format stdin. Writes formatted source to stdout unless `--check`.
fn fmt_stdin(check_only: bool, json: bool) -> bool {
    let source = match read_source_arg("-") {
        Ok((s, _)) => s,
        Err(e) => {
            eprintln!("Error: reading stdin: {e}");
            process::exit(2);
        }
    };

    let formatted = match assura_fmt::try_format_source(&source) {
        Ok(f) => f,
        Err(errors) => {
            for e in &errors {
                eprintln!("<stdin>: parse error: {}", e.message);
            }
            return false;
        }
    };

    if check_only {
        let ok = formatted == source;
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "file": "<stdin>",
                    "formatted": ok,
                })
            );
        } else if !ok {
            eprintln!("<stdin>: not formatted");
        }
        ok
    } else if json {
        // Still emit source so pipes work; wrap only when check.
        print!("{formatted}");
        true
    } else {
        print!("{formatted}");
        true
    }
}

/// Format one file. Returns `false` if `--check` failed or parse failed.
fn fmt_one(filename: &str, check_only: bool, json: bool) -> bool {
    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {filename}: {e}");
            process::exit(2);
        }
    };

    let formatted = match assura_fmt::try_format_source(&source) {
        Ok(f) => f,
        Err(errors) => {
            for e in &errors {
                eprintln!("{filename}: parse error: {}", e.message);
            }
            return false;
        }
    };

    if check_only {
        let ok = formatted == source;
        if json {
            // Directory path aggregates results; single file prints here.
            // Callers that collect results pass json=true and may skip single print
            // for dirs (handled in run_fmt). For a single file, print now.
            println!(
                "{}",
                serde_json::json!({
                    "file": filename,
                    "formatted": ok,
                })
            );
        } else if !ok {
            eprintln!("{filename}: not formatted");
        }
        ok
    } else {
        if let Err(e) = fs::write(filename, &formatted) {
            eprintln!("Error: cannot write {filename}: {e}");
            process::exit(2);
        }
        true
    }
}

fn collect_assura_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if let Some(name) = p.file_name().and_then(|n| n.to_str())
                && (name == "target" || name == "generated" || name == ".git")
            {
                continue;
            }
            collect_assura_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("assura") {
            out.push(p);
        }
    }
}

// ---------------------------------------------------------------------------
