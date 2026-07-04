//! LLM provider trait and implementations.

use crate::types::*;

/// Trait for LLM backends.
pub trait LlmProvider: Send + Sync {
    /// Analyze a function body against its contracts (Level 1).
    fn analyze(&self, request: &AnalysisRequest) -> Result<AnalysisResponse, LlmError>;

    /// Suggest contracts for an unannotated function.
    fn suggest(&self, request: &SuggestionRequest) -> Result<SuggestionResponse, LlmError>;

    /// Raw LLM call with system + user prompts (for custom prompt flows).
    fn call_raw(&self, system_prompt: &str, user_prompt: &str) -> Result<String, LlmError>;

    /// Return the model identifier (for cache keys).
    fn model_id(&self) -> &str;
}

// ---------------------------------------------------------------------------
// HTTP provider (OpenAI / Anthropic compatible)
// ---------------------------------------------------------------------------

/// HTTP-based LLM provider for OpenAI and Anthropic APIs.
pub struct HttpProvider {
    config: LlmConfig,
    client: reqwest::blocking::Client,
    api_key: String,
}

impl HttpProvider {
    /// Create a new HTTP provider from config.
    ///
    /// Reads the API key from the environment variable specified in config.
    pub fn new(config: LlmConfig) -> Result<Self, LlmError> {
        let api_key = std::env::var(&config.api_key_env).map_err(|_| LlmError::ApiKeyMissing {
            env_var: config.api_key_env.clone(),
        })?;

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_seconds))
            .build()
            .map_err(|e| LlmError::Http(e.to_string()))?;

        Ok(Self {
            config,
            client,
            api_key,
        })
    }

    fn call_api(&self, system_prompt: &str, user_prompt: &str) -> Result<String, LlmError> {
        let is_anthropic = self.config.provider == "anthropic"
            || self
                .config
                .base_url
                .as_deref()
                .unwrap_or("")
                .contains("anthropic");

        if is_anthropic {
            self.call_anthropic(system_prompt, user_prompt)
        } else {
            self.call_openai_compat(system_prompt, user_prompt)
        }
    }

    fn call_anthropic(&self, system_prompt: &str, user_prompt: &str) -> Result<String, LlmError> {
        let url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com/v1/messages");

        let body = serde_json::json!({
            "model": self.config.model,
            "max_tokens": self.config.max_tokens,
            "system": system_prompt,
            "messages": [
                {"role": "user", "content": user_prompt}
            ]
        });

        let resp = self
            .client
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| LlmError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(LlmError::Http(format!("{status}: {text}")));
        }

        let json: serde_json::Value = resp.json().map_err(|e| LlmError::Http(e.to_string()))?;

        json["content"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| LlmError::Parse("missing content[0].text in response".to_string()))
    }

    fn call_openai_compat(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, LlmError> {
        let url = format!(
            "{}/chat/completions",
            self.config
                .base_url
                .as_deref()
                .unwrap_or("https://api.openai.com/v1")
        );

        let body = serde_json::json!({
            "model": self.config.model,
            "max_tokens": self.config.max_tokens,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ]
        });

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| LlmError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(LlmError::Http(format!("{status}: {text}")));
        }

        let json: serde_json::Value = resp.json().map_err(|e| LlmError::Http(e.to_string()))?;

        json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                LlmError::Parse("missing choices[0].message.content in response".to_string())
            })
    }
}

impl LlmProvider for HttpProvider {
    fn analyze(&self, request: &AnalysisRequest) -> Result<AnalysisResponse, LlmError> {
        let system_prompt = crate::prompt::analysis_system_prompt();
        let user_prompt = crate::prompt::analysis_user_prompt(request);
        let raw = self.call_api(&system_prompt, &user_prompt)?;
        crate::prompt::parse_analysis_response(&raw)
    }

    fn suggest(&self, request: &SuggestionRequest) -> Result<SuggestionResponse, LlmError> {
        let system_prompt = crate::prompt::suggestion_system_prompt();
        let user_prompt = crate::prompt::suggestion_user_prompt(request);
        let raw = self.call_api(&system_prompt, &user_prompt)?;
        crate::prompt::parse_suggestion_response(&raw)
    }

    fn call_raw(&self, system_prompt: &str, user_prompt: &str) -> Result<String, LlmError> {
        self.call_api(system_prompt, user_prompt)
    }

    fn model_id(&self) -> &str {
        &self.config.model
    }
}

// ---------------------------------------------------------------------------
// Mock provider (for tests)
// ---------------------------------------------------------------------------

/// Mock LLM provider that returns configurable responses.
pub struct MockProvider {
    pub analysis_response: AnalysisResponse,
    pub suggestion_response: SuggestionResponse,
}

impl Default for MockProvider {
    fn default() -> Self {
        Self {
            analysis_response: AnalysisResponse {
                verdict: Verdict::Pass,
                confidence: 1.0,
                paths: vec![],
                reasoning: "mock analysis".to_string(),
            },
            suggestion_response: SuggestionResponse {
                suggestions: vec![],
                skipped_reason: None,
            },
        }
    }
}

impl LlmProvider for MockProvider {
    fn analyze(&self, _request: &AnalysisRequest) -> Result<AnalysisResponse, LlmError> {
        Ok(self.analysis_response.clone())
    }

    fn suggest(&self, _request: &SuggestionRequest) -> Result<SuggestionResponse, LlmError> {
        Ok(self.suggestion_response.clone())
    }

    fn call_raw(&self, _system_prompt: &str, _user_prompt: &str) -> Result<String, LlmError> {
        // Return a default crash suggestion response for testing
        Ok(serde_json::to_string(&serde_json::json!({
            "suggestions": []
        }))
        .unwrap())
    }

    fn model_id(&self) -> &str {
        "mock"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_provider_new_missing_api_key() {
        // Use an env var name that is guaranteed not to be set
        let env_var = "ASSURA_TEST_NONEXISTENT_KEY_XYZ_12345";

        let config = LlmConfig {
            api_key_env: env_var.to_string(),
            ..LlmConfig::default()
        };
        let result = HttpProvider::new(config);
        assert!(result.is_err(), "should fail when env var is not set");
        let err = result.err().unwrap();
        let msg = err.to_string();
        assert!(
            msg.contains(env_var),
            "error should mention env var name: {msg}"
        );
        assert!(
            matches!(err, LlmError::ApiKeyMissing { .. }),
            "should be ApiKeyMissing variant"
        );
    }

    #[test]
    fn http_provider_new_with_api_key_succeeds() {
        let env_var = "ASSURA_TEST_PROVIDER_KEY_SUCCESS";
        // SAFETY: test-only env var, no concurrent access
        unsafe { std::env::set_var(env_var, "test-key-value") };

        let config = LlmConfig {
            api_key_env: env_var.to_string(),
            ..LlmConfig::default()
        };
        let _provider = HttpProvider::new(config)
            .expect("HttpProvider::new should succeed when env var is set");

        // SAFETY: cleaning up test env var
        unsafe { std::env::remove_var(env_var) };
    }

    #[test]
    fn mock_provider_returns_configured_analysis() {
        let provider = MockProvider {
            analysis_response: AnalysisResponse {
                verdict: Verdict::Fail {
                    violations: vec![Violation {
                        clause_kind: "ensures".to_string(),
                        clause_expression: "result > 0".to_string(),
                        description: "can return 0".to_string(),
                        evidence_line: Some(5),
                    }],
                },
                confidence: 0.8,
                paths: vec![],
                reasoning: "test fail".to_string(),
            },
            ..Default::default()
        };

        let req = AnalysisRequest {
            function_name: "f".to_string(),
            function_body: "0".to_string(),
            function_signature: "fn f() -> i32".to_string(),
            contracts: vec![],
            params: vec![],
            return_type: Some("i32".to_string()),
            context: AnalysisContext::default(),
        };

        let result = provider.analyze(&req).unwrap();
        assert!(matches!(result.verdict, Verdict::Fail { .. }));
    }

    #[test]
    fn mock_provider_returns_configured_suggestions() {
        let provider = MockProvider {
            suggestion_response: SuggestionResponse {
                suggestions: vec![RawSuggestion {
                    kind: "requires".to_string(),
                    expression: "x > 0".to_string(),
                    confidence: 0.9,
                    reasoning: "guard".to_string(),
                    evidence_line: None,
                }],
                skipped_reason: None,
            },
            ..Default::default()
        };

        let req = SuggestionRequest {
            function_name: "g".to_string(),
            function_signature: "fn g(x: i32)".to_string(),
            function_body: "x + 1".to_string(),
            doc_comments: String::new(),
            impl_type: None,
            visibility: "pub".to_string(),
            is_unsafe: false,
            is_async: false,
            context: SuggestionContext::default(),
        };

        let result = provider.suggest(&req).unwrap();
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].expression, "x > 0");
    }

    #[test]
    fn mock_provider_model_id() {
        let provider = MockProvider::default();
        assert_eq!(provider.model_id(), "mock");
    }

    #[test]
    fn mock_provider_call_raw_returns_json() {
        let provider = MockProvider::default();
        let result = provider.call_raw("system", "user").unwrap();
        // Should be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["suggestions"].is_array());
    }
}
