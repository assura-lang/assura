use super::{Diagnostic, Severity};

/// Render a single `Diagnostic` to stderr using ariadne.
pub fn render_diagnostic(diag: &Diagnostic, filename: &str, source: &str) {
    use ariadne::{Color, Label, Report, ReportKind, Source};

    let kind = match diag.severity {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
        Severity::Info => ReportKind::Advice,
    };
    let color = match diag.severity {
        Severity::Error => Color::Red,
        Severity::Warning => Color::Yellow,
        Severity::Info => Color::Blue,
    };
    let mut builder = Report::build(kind, (filename, diag.primary.clone()))
        .with_message(format!("[{}] {}", diag.code, diag.message))
        .with_label(
            Label::new((filename, diag.primary.clone()))
                .with_message(&diag.message)
                .with_color(color),
        );
    for sec in &diag.secondary {
        builder = builder.with_label(
            Label::new((filename, sec.span.clone()))
                .with_message(&sec.message)
                .with_color(Color::Blue),
        );
    }
    // Render suggestion as a help note when present.
    if let Some(ref suggestion) = diag.suggestion {
        builder = builder.with_help(format!(
            "{}: `{}`",
            suggestion.message, suggestion.replacement
        ));
    }
    // Add an explain hint for errors so users know how to get more detail.
    if diag.severity == Severity::Error {
        builder = builder.with_note(format!(
            "for more information, run `assura explain {}`",
            diag.code
        ));
    }
    builder
        .finish()
        .eprint((filename, Source::from(source)))
        .ok();
}

/// Render a list of diagnostics to stderr using ariadne.
pub fn report_diagnostics_human(diagnostics: &[Diagnostic], filename: &str, source: &str) {
    for d in diagnostics {
        render_diagnostic(d, filename, source);
    }
}
