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

// ---- Taint tracking ----

/// A tainted value that cannot be accidentally displayed or logged.
///
/// `Tainted<T>` wraps a value marked as sensitive (e.g., API keys, tokens,
/// passwords). It prevents accidental leaking through:
/// - No `Display` impl (compile error on `println!("{}", val)`)
/// - `Debug` impl prints `[REDACTED]` instead of the actual value
/// - No `Deref` impl (can't accidentally use inner value)
///
/// To access the inner value, use `.declassify()` (explicit opt-in)
/// or `.validate()` (checked access with a predicate).
pub struct Tainted<T> {
    inner: T,
    label: &'static str,
}

impl<T> Tainted<T> {
    /// Wrap a value with a taint label.
    pub fn new(val: T, label: &'static str) -> Self {
        Self { inner: val, label }
    }

    /// Explicitly extract the inner value, acknowledging the taint.
    /// Use sparingly; every call site is an audit point.
    pub fn declassify(self) -> T {
        self.inner
    }

    /// Extract the inner value only if it passes validation.
    /// Returns `None` if the predicate returns `false`.
    pub fn validate<F: FnOnce(&T) -> bool>(self, f: F) -> Option<T> {
        if f(&self.inner) {
            Some(self.inner)
        } else {
            None
        }
    }

    /// Borrow the inner value without consuming the wrapper.
    /// Use when you need read access but want to keep the taint wrapper.
    pub fn as_inner(&self) -> &T {
        &self.inner
    }

    /// Get the taint label (e.g., "secret", "pii").
    pub fn label(&self) -> &'static str {
        self.label
    }

    /// Apply a transformation to the inner value, preserving the taint label.
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Tainted<U> {
        Tainted {
            inner: f(self.inner),
            label: self.label,
        }
    }
}

impl<T> std::fmt::Debug for Tainted<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[REDACTED: {}]", self.label)
    }
}

impl<T: Clone> Clone for Tainted<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            label: self.label,
        }
    }
}

// Deliberately: NO Display, NO Deref, NO AsRef<T>, NO Into<T>
// The only way to get the inner value is .declassify() or .validate()

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

    // ---- Tainted<T> tests ----

    #[test]
    fn tainted_debug_redacts() {
        let t = Tainted::new("my-api-key", "secret");
        let dbg = format!("{t:?}");
        assert_eq!(dbg, "[REDACTED: secret]");
        assert!(!dbg.contains("my-api-key"));
    }

    #[test]
    fn tainted_declassify_returns_inner() {
        let t = Tainted::new("hunter2".to_string(), "password");
        assert_eq!(t.declassify(), "hunter2");
    }

    #[test]
    fn tainted_validate_passes() {
        let t = Tainted::new("sk-abc123".to_string(), "api_key");
        let result = t.validate(|k| k.starts_with("sk-"));
        assert_eq!(result, Some("sk-abc123".to_string()));
    }

    #[test]
    fn tainted_validate_fails() {
        let t = Tainted::new("bad-key".to_string(), "api_key");
        let result = t.validate(|k| k.starts_with("sk-"));
        assert_eq!(result, None);
    }

    #[test]
    fn tainted_as_inner_borrows() {
        let t = Tainted::new(42, "secret_count");
        assert_eq!(*t.as_inner(), 42);
    }

    #[test]
    fn tainted_map_preserves_label() {
        let t = Tainted::new("hello".to_string(), "secret");
        let upper = t.map(|s| s.to_uppercase());
        assert_eq!(upper.label(), "secret");
        assert_eq!(upper.declassify(), "HELLO");
    }

    #[test]
    fn tainted_clone() {
        let t = Tainted::new("key".to_string(), "api");
        let t2 = t.clone();
        assert_eq!(t.declassify(), "key");
        assert_eq!(t2.declassify(), "key");
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
