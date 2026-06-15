use super::*;

// `assura fmt <file> [--check]` — format an .assura source file
// ---------------------------------------------------------------------------

pub(crate) fn run_fmt(filename: &str, check_only: bool) {
    let source = fs::read_to_string(filename).unwrap_or_else(|e| {
        eprintln!("Error: {filename}: {e}");
        process::exit(2);
    });

    let (file, errors) = assura_parser::parse(&source);

    if !errors.is_empty() {
        eprintln!(
            "Error: cannot format {filename}: {} parse error(s)",
            errors.len()
        );
        for e in &errors {
            eprintln!("  {e}");
        }
        process::exit(1);
    }

    let file = match file {
        Some(f) => f,
        None => {
            eprintln!("Error: cannot format {filename}: parse returned no AST");
            process::exit(1);
        }
    };

    let formatted = assura_fmt::format_source_file(&file);

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
