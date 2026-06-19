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

    // --- Run shared pipeline ---
    let CompilationResult {
        file,
        resolved,
        hir: _,
        typed,
        diagnostics,
        has_errors,
        timing,
        ..
    } = compile(&source, filename);

    if verbosity == Verbosity::Verbose {
        eprintln!("Pipeline timing for {filename}:");
        if let Some(ref f) = file {
            eprintln!(
                "  parse:     {} tokens, {} declaration(s), {} import(s) ({:.2}ms)",
                timing.token_count,
                f.decls.len(),
                f.imports.len(),
                timing.parse_ms
            );
        } else {
            eprintln!(
                "  parse:     {} tokens, failed ({:.2}ms)",
                timing.token_count, timing.parse_ms
            );
        }
        if let Some(resolve_ms) = timing.resolve_ms
            && let Some(ref r) = resolved
        {
            let user_symbols = r
                .symbols
                .symbols
                .iter()
                .filter(|s| s.kind != assura_resolve::SymbolKind::BuiltinType)
                .count();
            eprintln!("  resolve:   {user_symbols} symbol(s) ({resolve_ms:.2}ms)");
        }
        if let Some(hir_ms) = timing.hir_ms {
            eprintln!("  hir:       ({hir_ms:.2}ms)");
        }
        if let Some(typecheck_ms) = timing.typecheck_ms
            && let Some(ref td) = typed
        {
            eprintln!(
                "  typecheck: {} binding(s) ({typecheck_ms:.2}ms)",
                td.type_env.len()
            );
        }
        eprintln!();
    }

    if has_errors {
        assura_diagnostics::report_diagnostics_human(&diagnostics, filename, &source);
        if verbosity != Verbosity::Quiet {
            eprintln!("{filename}: {} error(s) found", diagnostics.len());
        }
        process::exit(1);
    }

    let file = file.expect("file should exist if has_errors is false");
    let resolved = resolved.expect("resolved should exist if has_errors is false");
    let typed = typed.expect("typed should exist if has_errors is false");

    // --- Verify ---
    let verify_start = Instant::now();
    let explain_cache_dir = std::path::Path::new(filename)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let explain_verify_cache = assura_smt::VerificationCache::new(explain_cache_dir);
    let ir_map = assura_smt::load_ir_bodies_for_typed(std::path::Path::new(filename), &typed);
    let verify_extras = (!ir_map.is_empty()).then_some(assura_smt::VerifyFileExtras {
        ir_bodies: Some(&ir_map),
    });
    let mut verification_results = assura_smt::verify_parallel_with_solver(
        &typed,
        &explain_verify_cache,
        assura_smt::SolverChoice::Z3,
        verify_extras.as_ref(),
    );
    verification_results.extend(assura_smt::display::dispatch_decrease_checks(&typed));
    let verify_ms = verify_start.elapsed().as_secs_f64() * 1000.0;

    if verbosity == Verbosity::Verbose {
        eprintln!(
            "  verify:    {} clause(s) ({verify_ms:.2}ms)",
            verification_results.len()
        );
        let total = timing.parse_ms
            + timing.resolve_ms.unwrap_or(0.0)
            + timing.typecheck_ms.unwrap_or(0.0)
            + verify_ms;
        eprintln!("  total:     {total:.2}ms");
        eprintln!();
    }

    // --- Output ---
    if verbosity == Verbosity::Quiet {
        // Quiet mode: no output for success
    } else if show_ast {
        assura_parser::display::print_ast(&file);
    } else {
        let _ = assura_smt::display::write_summary(
            &mut std::io::stdout(),
            filename,
            &file,
            &resolved.symbols,
            &typed.type_env,
            &verification_results,
        );
    }
}

// ---------------------------------------------------------------------------
