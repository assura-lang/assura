/// GitHub release metadata and asset downloads return 403, 429, and 5xx
/// often enough to fail CI. Those statuses are worth another attempt.
/// 200 is success. 401 and 404 are not retryable.
pub fn retryable_github_status(code: u16) -> bool {
    code == 403 || code == 429 || (500..600).contains(&code)
}

#[cfg(test)]
mod tests {
    use super::retryable_github_status;

    #[test]
    fn retries_rate_limit_and_server_errors() {
        for code in [403_u16, 429, 500, 502, 503, 504] {
            assert!(retryable_github_status(code), "{code}");
        }
    }

    #[test]
    fn does_not_retry_success_or_missing() {
        for code in [200_u16, 401, 404] {
            assert!(!retryable_github_status(code), "{code}");
        }
    }
}
