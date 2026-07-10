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
            if json {
                let report = serde_json::json!({
                    "ok": false,
                    "error": "no_assura_files",
                    "path": filename,
                    "message": format!("no .assura files found under {filename}"),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                eprintln!("Error: no .assura files found under {filename}");
            }
            process::exit(2);
        }
        let mut failed = false;
        let mut results: Vec<serde_json::Value> = Vec::new();
        for file in &files {
            let path_str = file.to_string_lossy();
            // Quiet per-file reporting when emitting aggregate JSON (avoids
            // human "not formatted" lines on stderr alongside the JSON doc).
            let ok = fmt_one(path_str.as_ref(), check_only, json, /*aggregate*/ json);
            if json {
                if check_only {
                    results.push(serde_json::json!({
                        "file": path_str,
                        "formatted": ok,
                    }));
                } else {
                    results.push(serde_json::json!({
                        "file": path_str,
                        "ok": ok,
                        "wrote": ok,
                    }));
                }
            }
            if !ok {
                failed = true;
            }
        }
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "ok": !failed,
                    "check": check_only,
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

    if !fmt_one(filename, check_only, json, /*aggregate*/ false) {
        process::exit(1);
    }
}

/// Format stdin. Writes formatted source to stdout unless `--check`.
fn fmt_stdin(check_only: bool, json: bool) -> bool {
    let source = match read_source_arg("-") {
        Ok((s, _)) => s,
        Err(e) => {
            if json {
                let report = serde_json::json!({
                    "ok": false,
                    "file": "<stdin>",
                    "error": format!("reading stdin: {e}"),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                eprintln!("Error: reading stdin: {e}");
            }
            process::exit(2);
        }
    };

    let formatted = match assura_fmt::try_format_source(&source) {
        Ok(f) => f,
        Err(errors) => {
            if json {
                let report = serde_json::json!({
                    "ok": false,
                    "file": "<stdin>",
                    "error": "parse_error",
                    "messages": errors.iter().map(|e| e.message.clone()).collect::<Vec<_>>(),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                for e in &errors {
                    eprintln!("<stdin>: parse error: {}", e.message);
                }
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
                    "ok": ok,
                    "file": "<stdin>",
                    "formatted": ok,
                })
            );
        } else if !ok {
            eprintln!("<stdin>: not formatted");
        }
        ok
    } else if json {
        // Pipe-friendly: emit formatted source (not a status wrapper).
        print!("{formatted}");
        true
    } else {
        print!("{formatted}");
        true
    }
}

/// Format one file. Returns `false` if `--check` failed or parse failed.
///
/// When `aggregate` is true (directory + `--json`), do not print per-file
/// JSON or human "not formatted" lines; the caller emits one report.
fn fmt_one(filename: &str, check_only: bool, json: bool, aggregate: bool) -> bool {
    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            if json {
                let report = serde_json::json!({
                    "ok": false,
                    "file": filename,
                    "error": format!("{e}"),
                    "message": format!("{filename}: {e}"),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                eprintln!("Error: {filename}: {e}");
            }
            process::exit(2);
        }
    };

    let formatted = match assura_fmt::try_format_source(&source) {
        Ok(f) => f,
        Err(errors) => {
            if aggregate {
                // Caller reports failure in aggregate JSON.
            } else if json {
                let report = serde_json::json!({
                    "ok": false,
                    "file": filename,
                    "error": "parse_error",
                    "messages": errors.iter().map(|e| e.message.clone()).collect::<Vec<_>>(),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                for e in &errors {
                    eprintln!("{filename}: parse error: {}", e.message);
                }
            }
            return false;
        }
    };

    if check_only {
        let ok = formatted == source;
        if aggregate {
            // Caller builds the aggregate JSON document.
        } else if json {
            println!(
                "{}",
                serde_json::json!({
                    "ok": ok,
                    "file": filename,
                    "formatted": ok,
                })
            );
        } else if !ok {
            eprintln!("{filename}: not formatted");
        }
        ok
    } else {
        let changed = formatted != source;
        if let Err(e) = fs::write(filename, &formatted) {
            if json && !aggregate {
                let report = serde_json::json!({
                    "ok": false,
                    "file": filename,
                    "error": format!("cannot write {filename}: {e}"),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else if !json {
                eprintln!("Error: cannot write {filename}: {e}");
            }
            process::exit(2);
        }
        if !aggregate && json {
            println!(
                "{}",
                serde_json::json!({
                    "ok": true,
                    "file": filename,
                    "wrote": true,
                    "changed": changed,
                })
            );
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
