//! High-level analysis and suggestion orchestration.

use crate::cache::{self, LlmCache};
use crate::provider::LlmProvider;
use crate::types::*;

/// Run Level 1 analysis on a single function.
pub fn analyze_function(
    provider: &dyn LlmProvider,
    cache: &LlmCache,
    request: &AnalysisRequest,
) -> Result<AnalysisResponse, LlmError> {
    let ctx_hash = cache::context_hash(&request.context.called_functions);
    let key = cache::analysis_cache_key(
        &request.function_name,
        &request.function_body,
        &request.contracts,
        &ctx_hash,
        provider.model_id(),
    );

    // Check cache first
    if let Some(cached) = cache.get_analysis(&key) {
        return Ok(cached);
    }

    let response = provider.analyze(request)?;
    let _ = cache.put_analysis(&key, &response);
    Ok(response)
}

/// Run contract suggestion for a single function.
pub fn suggest_contracts(
    provider: &dyn LlmProvider,
    cache: &LlmCache,
    request: &SuggestionRequest,
) -> Result<SuggestionResponse, LlmError> {
    let siblings_hash = cache::context_hash(
        &request
            .context
            .sibling_contracts
            .iter()
            .map(|c| CalledFunctionContract {
                name: c.name.clone(),
                signature: c.signature.clone(),
                requires: c.requires.clone(),
                ensures: c.ensures.clone(),
                source_file: c.source_file.clone(),
            })
            .collect::<Vec<_>>(),
    );
    let key = cache::suggest_cache_key(
        &request.function_name,
        &request.function_body,
        &request.function_signature,
        &request.doc_comments,
        &siblings_hash,
        provider.model_id(),
    );

    if let Some(cached) = cache.get_suggestions(&key) {
        return Ok(cached);
    }

    let response = provider.suggest(request)?;
    let _ = cache.put_suggestions(&key, &response);
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::MockProvider;

    #[test]
    fn analyze_uses_cache() {
        let dir = std::env::temp_dir().join("assura-llm-test-suggest");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = LlmCache::new(&dir);

        let provider = MockProvider {
            analysis_response: AnalysisResponse {
                verdict: Verdict::Pass,
                confidence: 0.99,
                paths: vec![],
                reasoning: "mock".to_string(),
            },
            ..Default::default()
        };

        let req = AnalysisRequest {
            function_name: "test_fn".to_string(),
            function_body: "x + 1".to_string(),
            function_signature: "fn test_fn(x: i32) -> i32".to_string(),
            contracts: vec![ContractClauseInfo {
                kind: "ensures".to_string(),
                expression: "result > x".to_string(),
            }],
            params: vec![],
            return_type: Some("i32".to_string()),
            context: AnalysisContext::default(),
        };

        // First call hits the provider
        let r1 = analyze_function(&provider, &cache, &req).unwrap();
        assert!(matches!(r1.verdict, Verdict::Pass));

        // Second call should hit cache (same result)
        let r2 = analyze_function(&provider, &cache, &req).unwrap();
        assert!(matches!(r2.verdict, Verdict::Pass));

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn make_suggestion_request() -> SuggestionRequest {
        SuggestionRequest {
            function_name: "add".to_string(),
            function_signature: "fn add(a: i32, b: i32) -> i32".to_string(),
            function_body: "a + b".to_string(),
            doc_comments: String::new(),
            impl_type: None,
            visibility: "pub".to_string(),
            is_unsafe: false,
            is_async: false,
            context: SuggestionContext::default(),
        }
    }

    #[test]
    fn suggest_contracts_returns_provider_response() {
        let dir = std::env::temp_dir().join("assura-llm-test-suggest-contracts");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = LlmCache::new(&dir);

        let provider = MockProvider {
            suggestion_response: SuggestionResponse {
                suggestions: vec![RawSuggestion {
                    kind: "requires".to_string(),
                    expression: "a >= 0".to_string(),
                    confidence: 0.85,
                    reasoning: "non-negative input".to_string(),
                    evidence_line: Some(1),
                }],
                skipped_reason: None,
            },
            ..Default::default()
        };

        let req = make_suggestion_request();
        let result = suggest_contracts(&provider, &cache, &req).unwrap();
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].expression, "a >= 0");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn suggest_contracts_uses_cache() {
        let dir = std::env::temp_dir().join("assura-llm-test-suggest-cache-hit");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = LlmCache::new(&dir);

        let provider = MockProvider {
            suggestion_response: SuggestionResponse {
                suggestions: vec![RawSuggestion {
                    kind: "ensures".to_string(),
                    expression: "result == a + b".to_string(),
                    confidence: 0.95,
                    reasoning: "sum".to_string(),
                    evidence_line: None,
                }],
                skipped_reason: None,
            },
            ..Default::default()
        };

        let req = make_suggestion_request();

        // First call hits provider and populates cache
        let r1 = suggest_contracts(&provider, &cache, &req).unwrap();
        assert_eq!(r1.suggestions.len(), 1);

        // Second call should return cached result
        let r2 = suggest_contracts(&provider, &cache, &req).unwrap();
        assert_eq!(r2.suggestions.len(), 1);
        assert_eq!(r2.suggestions[0].expression, "result == a + b");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn suggest_contracts_empty_suggestions() {
        let dir = std::env::temp_dir().join("assura-llm-test-suggest-empty");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = LlmCache::new(&dir);

        let provider = MockProvider::default(); // default has empty suggestions

        let req = make_suggestion_request();
        let result = suggest_contracts(&provider, &cache, &req).unwrap();
        assert!(result.suggestions.is_empty());
        assert!(result.skipped_reason.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn suggest_contracts_with_sibling_context() {
        let dir = std::env::temp_dir().join("assura-llm-test-suggest-siblings");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = LlmCache::new(&dir);

        let provider = MockProvider {
            suggestion_response: SuggestionResponse {
                suggestions: vec![RawSuggestion {
                    kind: "requires".to_string(),
                    expression: "n > 0".to_string(),
                    confidence: 0.9,
                    reasoning: "follows sibling pattern".to_string(),
                    evidence_line: None,
                }],
                skipped_reason: None,
            },
            ..Default::default()
        };

        let req = SuggestionRequest {
            function_name: "divide".to_string(),
            function_signature: "fn divide(a: i32, n: i32) -> i32".to_string(),
            function_body: "a / n".to_string(),
            doc_comments: String::new(),
            impl_type: None,
            visibility: "pub".to_string(),
            is_unsafe: false,
            is_async: false,
            context: SuggestionContext {
                surrounding_types: vec![],
                sibling_contracts: vec![CalledFunctionContract {
                    name: "multiply".to_string(),
                    signature: "fn multiply(a: i32, b: i32) -> i32".to_string(),
                    requires: vec!["a >= 0".to_string()],
                    ensures: vec!["result == a * b".to_string()],
                    source_file: "lib.rs".to_string(),
                }],
            },
        };

        let result = suggest_contracts(&provider, &cache, &req).unwrap();
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].kind, "requires");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
