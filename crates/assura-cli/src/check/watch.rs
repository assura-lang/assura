//! Watch mode: re-check on file change.

use super::super::*;
use super::report::verify_and_report;
use super::types::VerifyContext;

// ---------------------------------------------------------------------------
// Watch mode
// ---------------------------------------------------------------------------

pub(crate) fn check_file_once(
    filename: &str,
    output_mode: OutputMode,
    verbosity: Verbosity,
    layer: u8,
) -> bool {
    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {filename}: {e}");
            return true;
        }
    };

    let output = compile(&source, filename);
    crate::timing::print_pipeline_timing(
        &output,
        crate::timing::TimingOptions {
            filename,
            output_mode,
            verbosity,
            project: None,
            config_line: None,
            verify_ms: None,
            show_total: false,
            show_phase_failures: true,
        },
    );
    let CompilationResult {
        file,
        resolved: _,
        typed,
        mut diagnostics,
        mut has_errors,
        ..
    } = output;

    verify_and_report(VerifyContext {
        filename,
        source: &source,
        typed: &typed,
        file: &file,
        diagnostics: &mut diagnostics,
        has_errors: &mut has_errors,
        output_mode,
        verbosity,
        verify_options: assura_config::VerifyOptions {
            layer,
            solver: assura_smt::SolverChoice::Z3,
            ..Default::default()
        },
        show_cores: false,
    });

    has_errors
}

/// Compute a simple content hash for incremental change detection.
pub(crate) fn content_hash(source: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Run check in watch mode: check once, then watch for file changes.
/// Uses IncrementalCompiler to skip re-checks when file content is unchanged.
pub(crate) fn run_watch_loop(
    filename: &str,
    output_mode: OutputMode,
    verbosity: Verbosity,
    layer: u8,
) -> ! {
    use notify::{Event, EventKind, RecursiveMode, Watcher};

    let path = Path::new(filename).canonicalize().unwrap_or_else(|e| {
        eprintln!("Error: cannot resolve path {filename}: {e}");
        process::exit(2);
    });

    // Set up incremental compiler to track file hashes
    let mut incremental = assura_smt::IncrementalCompiler::new();
    let mut last_hash = String::new();

    // Initial check
    eprintln!("[watch] Checking {filename}...");
    eprintln!();
    if let Ok(source) = fs::read_to_string(filename) {
        last_hash = content_hash(&source);
        incremental.register_module(filename.to_string(), last_hash.clone());
    }
    // In watch mode, we continue regardless of errors (intentionally ignoring result)
    let _had_errors = check_file_once(filename, output_mode, verbosity, layer);
    incremental.mark_checked(filename, 1);
    eprintln!();
    eprintln!("[watch] Watching {filename} for changes. Press Ctrl+C to stop.");

    let (tx, rx) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            // Only trigger on modify/create events
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                let _ = tx.send(());
            }
        }
    })
    .unwrap_or_else(|e| {
        eprintln!("Error: failed to create file watcher: {e}");
        process::exit(2);
    });

    // Watch the file's parent directory to catch renames/replacements
    let watch_dir = path.parent().unwrap_or(&path);
    watcher
        .watch(watch_dir, RecursiveMode::NonRecursive)
        .unwrap_or_else(|e| {
            eprintln!("Error: failed to watch {}: {e}", watch_dir.display());
            process::exit(2);
        });

    let mut iteration: u64 = 2;
    loop {
        // Wait for a change event
        let _ = rx.recv();

        // Debounce: drain any additional events that arrive within 100ms
        while rx.recv_timeout(Duration::from_millis(100)).is_ok() {}

        // Check if content actually changed (saves without edits, editor auto-format, etc.)
        let new_hash = fs::read_to_string(filename)
            .map(|s| content_hash(&s))
            .unwrap_or_default();

        if new_hash == last_hash && !new_hash.is_empty() {
            if verbosity == Verbosity::Verbose {
                eprintln!("[watch] File saved but content unchanged, skipping re-check.");
            }
            continue;
        }

        // Content changed: update incremental state
        last_hash = new_hash;
        incremental.mark_changed(filename);

        if verbosity == Verbosity::Verbose {
            let dirty = incremental.dirty_modules();
            eprintln!(
                "[watch] {} dirty module(s): {}",
                dirty.len(),
                dirty.join(", ")
            );
        }

        // Clear screen and re-check
        eprint!("\x1B[2J\x1B[H");
        eprintln!("[watch] File changed, re-checking {filename}...");
        eprintln!();
        let _had_errors = check_file_once(filename, output_mode, verbosity, layer);
        incremental.mark_checked(filename, iteration);
        iteration += 1;
        eprintln!();
        eprintln!("[watch] Watching for changes. Press Ctrl+C to stop.");
    }
}
