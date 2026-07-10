use super::*;

// `assura fmt <file|dir> [--check]` — format .assura source file(s)
// ---------------------------------------------------------------------------

pub(crate) fn run_fmt(filename: &str, check_only: bool) {
    // Match `assura check -`: format stdin to stdout (or --check only).
    if is_stdin_arg(filename) {
        if !fmt_stdin(check_only) {
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
        for file in &files {
            let path_str = file.to_string_lossy();
            if !fmt_one(path_str.as_ref(), check_only) {
                failed = true;
            }
        }
        if failed {
            process::exit(1);
        }
        return;
    }

    if !fmt_one(filename, check_only) {
        process::exit(1);
    }
}

/// Format stdin. Writes formatted source to stdout unless `--check`.
fn fmt_stdin(check_only: bool) -> bool {
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
        if formatted == source {
            true
        } else {
            eprintln!("<stdin>: not formatted");
            false
        }
    } else {
        print!("{formatted}");
        true
    }
}

/// Format one file. Returns `false` if `--check` failed or parse failed.
fn fmt_one(filename: &str, check_only: bool) -> bool {
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
        if formatted == source {
            true
        } else {
            eprintln!("{filename}: not formatted");
            false
        }
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
