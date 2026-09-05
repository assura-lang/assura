//! Close-match suggestions for CLI-local vocabularies.
//!
//! Copied here so the published CLI crate does not call unpublished
//! diagnostics / LLM helper APIs (cargo package verifies against
//! crates.io path-dep versions).

/// Known `--llm-provider` values (anthropic, openai, ollama).
pub(crate) const LLM_PROVIDERS: &[&str] = &["anthropic", "openai", "ollama"];

/// Levenshtein edit distance between two strings.
pub(crate) fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for (i, row) in dp.iter_mut().enumerate().take(m + 1) {
        row[0] = i;
    }
    for (j, val) in dp[0].iter_mut().enumerate().take(n + 1) {
        *val = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[m][n]
}

/// Return the closest candidate within a small edit-distance threshold.
///
/// Comparison is case-insensitive. Returns `None` when nothing is close enough.
pub(crate) fn did_you_mean<'a>(input: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let input_l = input.to_ascii_lowercase();
    let input_len = input.chars().count();
    let threshold = match input_len {
        0..=2 => 1,
        3..=5 => 2,
        _ => 3,
    };
    let mut best: Option<(&'a str, usize)> = None;
    for cand in candidates {
        let dist = edit_distance(&input_l, &cand.to_ascii_lowercase());
        if dist == 0 {
            return Some(*cand);
        }
        if dist <= threshold
            && dist < input_len
            && best.is_none_or(|(_, best_dist)| dist < best_dist)
        {
            best = Some((*cand, dist));
        }
    }
    best.map(|(c, _)| c)
}

/// Closest catalog error code, if one is nearby.
pub(crate) fn suggest_error_code(code: &str) -> Option<&'static str> {
    let catalog = assura_diagnostics::error_catalog();
    let codes: Vec<&str> = catalog.iter().map(|i| i.code).collect();
    did_you_mean(code, &codes)
}

/// Human message for an unknown error code, with an optional close match.
pub(crate) fn unknown_error_code_message(code: &str) -> String {
    match suggest_error_code(code) {
        Some(hint) => format!("Unknown error code: {code}\ndid you mean {hint}?"),
        None => format!("Unknown error code: {code}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_identity_and_typo() {
        assert_eq!(edit_distance("openai", "openai"), 0);
        assert_eq!(edit_distance("openaii", "openai"), 1);
        assert_eq!(edit_distance("A0300", "A03001"), 1);
    }

    #[test]
    fn suggests_openai_for_openaii() {
        assert_eq!(
            did_you_mean("openaii", &["anthropic", "openai", "ollama"]),
            Some("openai")
        );
    }

    #[test]
    fn suggests_nearby_error_code() {
        assert_eq!(suggest_error_code("A0300"), Some("A03001"));
    }

    #[test]
    fn unknown_message_includes_hint() {
        let msg = unknown_error_code_message("A0300");
        assert!(msg.contains("Unknown error code: A0300"));
        assert!(msg.contains("did you mean A03001?"));
    }

    #[test]
    fn did_you_mean_returns_none_when_far() {
        assert_eq!(did_you_mean("zzzzzz", LLM_PROVIDERS), None);
    }

    #[test]
    fn suggest_error_code_returns_none_for_unknown() {
        assert_eq!(suggest_error_code("NOTACODE"), None);
    }

    #[test]
    fn unknown_message_omits_hint_when_no_match() {
        let msg = unknown_error_code_message("NOTACODE");
        assert!(msg.contains("Unknown error code: NOTACODE"));
        assert!(
            !msg.contains("did you mean"),
            "no close catalog match should omit a hint: {msg}"
        );
    }
}
