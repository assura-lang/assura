//! Shared solver timeout policy (solver-neutral).
//!
//! All backends use these constants so Z3, CVC5 native, and CVC5 shell
//! have identical timeout behavior.

/// Default solver timeout for contract clause checks (milliseconds).
pub(crate) const DEFAULT_SOLVER_TIMEOUT_MS: u32 = 10_000;

/// Timeout string for CVC5's `--tlimit` flag (shell-out path).
pub(crate) const DEFAULT_SOLVER_TIMEOUT_TLIMIT: &str = "10000";

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
}
