//! Shared solver timeout policy (solver-neutral).
//!
//! All backends use these constants so Z3, CVC5 native, and CVC5 shell
//! have identical timeout behavior.

use std::cell::Cell;

/// Default solver timeout for contract clause checks (milliseconds).
///
/// Also the **floor** for file-level verify: config defaults are often 1s
/// (`VerifyOptions::default`), which is too short for multi-clause demos.
/// Callers that want a longer budget (e.g. `timeout = 60000` in assura.toml)
/// get it via [`clause_timeout_ms`].
pub(crate) const DEFAULT_SOLVER_TIMEOUT_MS: u32 = 10_000;

/// Timeout string for CVC5's `--tlimit` flag (shell-out path).
pub(crate) const DEFAULT_SOLVER_TIMEOUT_TLIMIT: &str = "10000";

/// Resolve the per-clause solver timeout from a caller request.
///
/// Floors at [`DEFAULT_SOLVER_TIMEOUT_MS`] so short config defaults do not
/// flip production verifies to 1s timeouts. Longer requests (above the
/// floor) pass through, capped at `u32::MAX`.
pub(crate) fn clause_timeout_ms(requested_ms: u64) -> u32 {
    let requested = u32::try_from(requested_ms).unwrap_or(u32::MAX);
    requested.max(DEFAULT_SOLVER_TIMEOUT_MS)
}

// Per-thread budget for solvers that are not the main clause solver.
// Feature clauses and decrease checks run on the caller thread. The
// budget is set for one dispatch instead of a parameter on every arm.
thread_local! {
    static FEATURE_SOLVER_TIMEOUT_MS: Cell<u32> = const { Cell::new(DEFAULT_SOLVER_TIMEOUT_MS) };
}

struct RestoreFeatureTimeout(u32);

impl Drop for RestoreFeatureTimeout {
    fn drop(&mut self) {
        FEATURE_SOLVER_TIMEOUT_MS.with(|cell| cell.set(self.0));
    }
}

/// Timeout in milliseconds for side solvers on this thread.
pub(crate) fn feature_solver_timeout_ms() -> u32 {
    FEATURE_SOLVER_TIMEOUT_MS.with(Cell::get)
}

/// Run `f` with the clause timeout. Short requests floor at 10s.
/// Longer requests pass through. The previous budget is restored,
/// including when `f` panics.
pub(crate) fn with_feature_solver_timeout<R>(requested_ms: u64, f: impl FnOnce() -> R) -> R {
    let timeout = clause_timeout_ms(requested_ms);
    let prev = FEATURE_SOLVER_TIMEOUT_MS.with(|cell| cell.replace(timeout));
    let _restore = RestoreFeatureTimeout(prev);
    f()
}

/// Resolve per-clause timeout as a CVC5 `--tlimit` / `tlimit` option string.
///
/// Same floor/resolve rules as [`clause_timeout_ms`] so shell and native
/// CVC5 paths match Z3 when `assura.toml` raises `verify.timeout`.
pub(crate) fn clause_timeout_tlimit(requested_ms: u64) -> String {
    let ms = clause_timeout_ms(requested_ms);
    if ms == DEFAULT_SOLVER_TIMEOUT_MS {
        DEFAULT_SOLVER_TIMEOUT_TLIMIT.to_string()
    } else {
        ms.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tlimit_string_matches_constant() {
        assert_eq!(
            DEFAULT_SOLVER_TIMEOUT_TLIMIT,
            DEFAULT_SOLVER_TIMEOUT_MS.to_string()
        );
    }

    #[test]
    fn clause_timeout_floors_short_requests() {
        assert_eq!(clause_timeout_ms(0), DEFAULT_SOLVER_TIMEOUT_MS);
        assert_eq!(clause_timeout_ms(1_000), DEFAULT_SOLVER_TIMEOUT_MS);
        assert_eq!(
            clause_timeout_ms(DEFAULT_SOLVER_TIMEOUT_MS as u64),
            DEFAULT_SOLVER_TIMEOUT_MS
        );
    }

    #[test]
    fn clause_timeout_allows_longer_requests() {
        assert_eq!(clause_timeout_ms(30_000), 30_000);
        assert_eq!(clause_timeout_ms(u64::MAX), u32::MAX);
    }

    #[test]
    fn clause_timeout_tlimit_matches_resolved_ms() {
        assert_eq!(
            clause_timeout_tlimit(1_000),
            DEFAULT_SOLVER_TIMEOUT_MS.to_string()
        );
        assert_eq!(clause_timeout_tlimit(30_000), "30000");
        assert_ne!(clause_timeout_tlimit(60_000), DEFAULT_SOLVER_TIMEOUT_TLIMIT);
    }
}
