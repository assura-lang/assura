//! Shared solver timeout policy (solver-neutral).
//!
//! All backends use these constants so Z3, CVC5 native, and CVC5 shell
//! have identical timeout behavior.

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
}
