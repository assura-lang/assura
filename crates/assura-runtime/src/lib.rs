//! Runtime contract monitoring for Assura-generated Rust code.
//!
//! When codegen is run with `--runtime-checks`, contract clauses
//! (requires, ensures, invariant) are emitted as runtime checks that
//! persist in release builds. Violations are routed through a
//! pluggable handler that can log, panic, or send telemetry.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Tracks the total number of contract violations observed.
static VIOLATION_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Information about a contract violation.
#[derive(Debug, Clone)]
pub struct Violation<'a> {
    /// Name of the contract (e.g. `"SafeDivision"`).
    pub contract: &'a str,
    /// Kind of clause that was violated: `"requires"`, `"ensures"`, or `"invariant"`.
    pub clause: &'a str,
    /// The condition expression that evaluated to false.
    pub condition: &'a str,
    /// Source file where the contract is defined.
    pub file: &'a str,
    /// Line number in the generated code.
    pub line: u32,
}

/// Handler function type for contract violations.
pub type ViolationHandler = fn(&Violation<'_>);

/// The default handler: panics with a descriptive message.
fn default_handler(v: &Violation<'_>) {
    panic!(
        "contract violation: {} {} failed: {} ({}:{})",
        v.contract, v.clause, v.condition, v.file, v.line
    );
}

/// Global handler slot. Uses a function pointer for zero-cost dispatch.
static HANDLER: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(default_handler as *mut ());

/// Set a custom violation handler. Call once at program startup.
///
/// # Example
///
/// ```
/// assura_runtime::set_handler(|v| {
///     eprintln!("[ASSURA] {} {} violated: {}", v.contract, v.clause, v.condition);
/// });
/// ```
pub fn set_handler(handler: ViolationHandler) {
    HANDLER.store(handler as *mut (), Ordering::Release);
}

/// Report a contract violation. Called by generated code when a
/// runtime check fails.
///
/// This function is `#[inline(never)]` to keep the hot path (the
/// check itself) small and branch-predictor-friendly.
#[inline(never)]
#[cold]
pub fn contract_violation(contract: &str, clause: &str, condition: &str, file: &str, line: u32) {
    VIOLATION_COUNT.fetch_add(1, Ordering::Relaxed);
    let v = Violation {
        contract,
        clause,
        condition,
        file,
        line,
    };
    let handler: ViolationHandler = unsafe { std::mem::transmute(HANDLER.load(Ordering::Acquire)) };
    handler(&v);
}

/// Returns the total number of violations observed since program start.
pub fn violation_count() -> usize {
    VIOLATION_COUNT.load(Ordering::Relaxed)
}

// ---- Convenience handlers ----

/// A handler that logs to stderr but does not panic.
pub fn log_handler(v: &Violation<'_>) {
    eprintln!(
        "[assura] contract violation: {} {} failed: {} ({}:{})",
        v.contract, v.clause, v.condition, v.file, v.line
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn violation_struct_fields() {
        let v = Violation {
            contract: "Foo",
            clause: "requires",
            condition: "x > 0",
            file: "test.rs",
            line: 42,
        };
        assert_eq!(v.contract, "Foo");
        assert_eq!(v.clause, "requires");
        assert_eq!(v.condition, "x > 0");
        assert_eq!(v.file, "test.rs");
        assert_eq!(v.line, 42);
    }

    #[test]
    fn log_handler_does_not_panic() {
        let v = Violation {
            contract: "Bar",
            clause: "ensures",
            condition: "result >= 0",
            file: "test.rs",
            line: 10,
        };
        log_handler(&v);
    }

    #[test]
    fn violation_count_increments() {
        let before = violation_count();
        // Use log_handler to avoid panicking
        set_handler(log_handler);
        contract_violation("Test", "requires", "x > 0", "test.rs", 1);
        assert!(violation_count() > before);
        // Reset to default handler
        set_handler(default_handler);
    }
}
