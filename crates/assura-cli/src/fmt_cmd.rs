use super::*;

// `assura fmt <file> [--check]` — format an .assura source file
// ---------------------------------------------------------------------------

pub(crate) fn run_fmt(filename: &str, check_only: bool) {
    let source = fs::read_to_string(filename).unwrap_or_else(|e| {
        eprintln!("Error: {filename}: {e}");
        process::exit(2);
    });

    let formatted = match assura_fmt::try_format_source(&source) {
        Ok(f) => f,
        Err(errors) => {
            for e in &errors {
                eprintln!("{filename}: parse error: {}", e.message);
            }
            process::exit(1);
        }
    };

    if check_only {
        if formatted == source {
            process::exit(0);
        } else {
            eprintln!("{filename}: not formatted");
            process::exit(1);
        }
    } else {
        fs::write(filename, &formatted).unwrap_or_else(|e| {
            eprintln!("Error: cannot write {filename}: {e}");
            process::exit(2);
        });
    }
}

// ---------------------------------------------------------------------------
