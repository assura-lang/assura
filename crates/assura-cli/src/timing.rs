//! Shared pipeline timing output for CLI commands.

use assura_config::{OutputMode, Verbosity};
use assura_pipeline::{CompilationOutput, PhaseTiming};
use assura_resolve::SymbolKind;
use std::path::Path;

/// Options controlling verbose pipeline timing output.
pub struct TimingOptions<'a> {
    pub filename: &'a str,
    pub output_mode: OutputMode,
    pub verbosity: Verbosity,
    /// Project name/version/root for commands that discover a project config.
    pub project: Option<(&'a str, &'a str, &'a Path)>,
    /// Config summary line (solver, layer, etc.).
    pub config_line: Option<String>,
    /// Extra verify phase timing not yet in `output.timing`.
    pub verify_ms: Option<f64>,
    /// Include total line (parse + resolve + typecheck + verify).
    pub show_total: bool,
    /// Show failure messages for skipped phases.
    pub show_phase_failures: bool,
}

/// Print pipeline phase timing to stderr when verbose human mode is active.
pub fn print_pipeline_timing(output: &CompilationOutput, opts: TimingOptions<'_>) {
    if opts.verbosity != Verbosity::Verbose || opts.output_mode != OutputMode::Human {
        return;
    }

    if let Some((name, version, root)) = opts.project {
        eprintln!("Project: {name} v{version} ({})", root.display());
        if let Some(ref line) = opts.config_line {
            eprintln!("  {line}");
        }
        eprintln!();
    }

    eprintln!("Pipeline timing for {}:", opts.filename);
    print_core_phases(output, opts.show_phase_failures);

    if let Some(verify_ms) = opts.verify_ms {
        eprintln!(
            "  verify:    {} clause(s) ({verify_ms:.2}ms)",
            output.verification.len()
        );
    } else if let Some(verify_ms) = output.timing.verify_ms {
        eprintln!(
            "  verify:    {} clause(s) ({verify_ms:.2}ms)",
            output.verification.len()
        );
    }

    if opts.show_total {
        let total = total_ms(&output.timing, opts.verify_ms.or(output.timing.verify_ms));
        eprintln!("  total:     {total:.2}ms");
    }

    eprintln!();
}

fn print_core_phases(output: &CompilationOutput, show_failures: bool) {
    let timing = output.timing;
    if let Some(ref f) = output.file {
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

    if let Some(resolve_ms) = timing.resolve_ms {
        if let Some(ref r) = output.resolved {
            let user_symbols = r
                .symbols
                .symbols
                .iter()
                .filter(|s| s.kind != SymbolKind::BuiltinType)
                .count();
            eprintln!("  resolve:   {user_symbols} symbol(s) ({resolve_ms:.2}ms)");
        } else if show_failures {
            eprintln!("  resolve:   failed ({resolve_ms:.2}ms)");
        }
    }

    if let Some(typecheck_ms) = timing.typecheck_ms {
        if let Some(ref td) = output.typed {
            eprintln!(
                "  typecheck: {} binding(s) ({typecheck_ms:.2}ms)",
                td.type_env.len()
            );
        } else if show_failures {
            eprintln!("  typecheck: failed ({typecheck_ms:.2}ms)");
        }
    }
}

fn total_ms(timing: &PhaseTiming, verify_ms: Option<f64>) -> f64 {
    timing.parse_ms
        + timing.resolve_ms.unwrap_or(0.0)
        + timing.typecheck_ms.unwrap_or(0.0)
        + verify_ms.unwrap_or(0.0)
}
