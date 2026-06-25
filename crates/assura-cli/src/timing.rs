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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_timing(parse: f64, resolve: Option<f64>, typecheck: Option<f64>) -> PhaseTiming {
        PhaseTiming {
            parse_ms: parse,
            resolve_ms: resolve,
            typecheck_ms: typecheck,
            verify_ms: None,
            codegen_ms: None,
            token_count: 0,
        }
    }

    #[test]
    fn total_ms_all_phases() {
        let timing = make_timing(1.5, Some(2.0), Some(3.0));
        let total = total_ms(&timing, Some(4.0));
        assert!((total - 10.5).abs() < f64::EPSILON);
    }

    #[test]
    fn total_ms_missing_phases() {
        let timing = make_timing(5.0, None, None);
        let total = total_ms(&timing, None);
        assert!((total - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn total_ms_verify_from_timing_struct() {
        let mut timing = make_timing(1.0, Some(2.0), Some(3.0));
        timing.verify_ms = Some(4.0);
        // When external verify_ms is None, total_ms ignores timing.verify_ms
        // (the function takes explicit verify_ms, not from timing struct).
        let total = total_ms(&timing, None);
        assert!((total - 6.0).abs() < f64::EPSILON);

        // Pass timing.verify_ms explicitly.
        let total_with = total_ms(&timing, timing.verify_ms);
        assert!((total_with - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn print_pipeline_timing_skips_non_verbose() {
        // Should be a no-op when verbosity is not Verbose.
        let output = assura_pipeline::compile(
            "contract Foo { input(x: Int) }",
            "<test>",
            &assura_config::CompilerConfig::default(),
        );
        let opts = TimingOptions {
            filename: "<test>",
            output_mode: OutputMode::Human,
            verbosity: Verbosity::Normal,
            project: None,
            config_line: None,
            verify_ms: None,
            show_total: false,
            show_phase_failures: false,
        };
        // Should not panic; returns immediately for non-Verbose.
        print_pipeline_timing(&output, opts);
    }
}
