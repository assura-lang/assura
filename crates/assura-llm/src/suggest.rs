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
}
