use super::{Diagnostic, Severity};

/// Whether human diagnostics should emit ANSI color.
///
/// `NO_COLOR` (set to any value, including empty) disables color.
/// A non-terminal stderr also disables color, so pipes and CI logs stay plain.
pub(crate) fn diagnostics_use_color(no_color_set: bool, stderr_is_terminal: bool) -> bool {
    !no_color_set && stderr_is_terminal
}

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
    let use_color = diagnostics_use_color(
        std::env::var_os("NO_COLOR").is_some(),
        std::io::IsTerminal::is_terminal(&std::io::stderr()),
    );
    // Ariadne copies label color when the label is added. The config has
    // to be set first, or a later with_color(false) leaves the ANSI codes.
    let mut builder = Report::build(kind, (filename, diag.primary.clone()))
        .with_config(ariadne::Config::default().with_color(use_color))
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
    // Advice-only suggestions may have an empty replacement (no text edit);
    // avoid rendering empty backticks like "Help: message: ``".
    if let Some(ref suggestion) = diag.suggestion {
        let help = if suggestion.replacement.is_empty() {
            suggestion.message.clone()
        } else {
            format!("{}: `{}`", suggestion.message, suggestion.replacement)
        };
        builder = builder.with_help(help);
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

#[cfg(test)]
mod tests {
    use super::diagnostics_use_color;

    #[test]
    fn no_color_or_non_tty_disables_color() {
        assert!(!diagnostics_use_color(true, true));
        assert!(!diagnostics_use_color(true, false));
        assert!(!diagnostics_use_color(false, false));
        assert!(diagnostics_use_color(false, true));
    }
}

/// Render a list of diagnostics to stderr using ariadne.
pub fn report_diagnostics_human(diagnostics: &[Diagnostic], filename: &str, source: &str) {
    for d in diagnostics {
        render_diagnostic(d, filename, source);
    }
}
