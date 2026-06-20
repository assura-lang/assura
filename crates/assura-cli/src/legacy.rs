use super::*;

// Legacy mode: `assura [--ast|--tokens] <file>`
// ---------------------------------------------------------------------------

pub(crate) fn run_legacy(filename: &str, verbosity: Verbosity, show_ast: bool, show_tokens: bool) {
    let source = fs::read_to_string(filename).unwrap_or_else(|e| {
        eprintln!("Error: {filename}: {e}");
        process::exit(2);
    });

    // --tokens mode: lex only, dump tokens, exit early
    if show_tokens {
        let lex = Token::lexer(&source);
        for (tok, span) in lex.spanned() {
            if let Ok(t) = tok {
                let line = source[..span.start].lines().count();
                let col = span.start - source[..span.start].rfind('\n').map_or(0, |p| p + 1) + 1;
                println!("{line}:{col}  {t:?}");
            }
        }
        return;
    }

    // --ast mode: parse only, dump AST
    if show_ast {
        let file = assura_parser::parse_unwrap(&source);
        assura_parser::display::print_ast(&file);
        return;
    }

    // Default legacy path: delegate to `assura check` (full pipeline + verify)
    if verbosity != Verbosity::Quiet {
        eprintln!("note: `assura <file>` is deprecated; use `assura check <file>`");
    }
    run_check(CheckOptions {
        filename,
        output_mode: OutputMode::Human,
        verbosity,
        layer: 255,
        solver: None,
        watch: false,
        stats: false,
        dump_smt: None,
        show_cores: false,
    });
}
