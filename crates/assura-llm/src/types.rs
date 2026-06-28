//! Core types for LLM-assisted contract analysis.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Request to analyze a function body against its contracts.
#[derive(Debug, Clone, Serialize)]
pub struct AnalysisRequest {
    pub function_name: String,
    pub function_body: String,
    pub function_signature: String,
    pub contracts: Vec<ContractClauseInfo>,
    pub params: Vec<ParamEntry>,
    pub return_type: Option<String>,
    pub context: AnalysisContext,
}

/// A contract clause for the LLM prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractClauseInfo {
    pub kind: String,
    pub expression: String,
}

/// A function parameter for the LLM prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamEntry {
    pub name: String,
    pub ty: String,
}

/// Context about the surrounding code.
#[derive(Debug, Clone, Serialize, Default)]
pub struct AnalysisContext {
    pub surrounding_types: Vec<TypeInfo>,
    pub called_functions: Vec<CalledFunctionContract>,
}

/// A type definition referenced in the analyzed function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeInfo {
    pub name: String,
    pub definition: String,
}

/// Contracts of a function called by the function under analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalledFunctionContract {
    pub name: String,
    pub signature: String,
    pub requires: Vec<String>,
    pub ensures: Vec<String>,
    pub source_file: String,
}

/// LLM analysis response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResponse {
    pub verdict: Verdict,
    pub confidence: f64,
    pub paths: Vec<PathAnalysis>,
    pub reasoning: String,
}

/// Overall verdict from LLM analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    Fail { violations: Vec<Violation> },
    Uncertain { reason: String },
}

/// A specific contract violation found by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Violation {
    pub clause_kind: String,
    pub clause_expression: String,
    pub description: String,
    pub evidence_line: Option<usize>,
}

/// Analysis of a single control-flow path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathAnalysis {
    pub description: String,
    pub reachable_given_preconditions: bool,
    pub contracts_satisfied: bool,
    pub reasoning: String,
}

/// Request to suggest contracts for an unannotated function.
#[derive(Debug, Clone, Serialize)]
pub struct SuggestionRequest {
    pub function_name: String,
    pub function_signature: String,
    pub function_body: String,
    pub doc_comments: String,
    pub impl_type: Option<String>,
    pub visibility: String,
    pub is_unsafe: bool,
    pub is_async: bool,
    pub context: SuggestionContext,
}

/// Context for contract suggestion.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SuggestionContext {
    pub surrounding_types: Vec<TypeInfo>,
    pub sibling_contracts: Vec<CalledFunctionContract>,
}

/// A single contract suggestion from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractSuggestion {
    pub kind: SuggestionKind,
    pub expression: String,
    pub confidence: f64,
    pub reasoning: String,
    pub evidence_line: Option<usize>,
    pub function_name: String,
    pub file: PathBuf,
    pub insert_line: usize,
}

/// Type of suggested contract.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionKind {
    Requires,
    Ensures,
    EnsuresOk,
    EnsuresErr,
    Invariant,
}

/// LLM suggestion response (raw from LLM).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestionResponse {
    pub suggestions: Vec<RawSuggestion>,
    pub skipped_reason: Option<String>,
}

/// A raw suggestion as returned by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawSuggestion {
    pub kind: String,
    pub expression: String,
    pub confidence: f64,
    pub reasoning: String,
    pub evidence_line: Option<usize>,
}

/// LLM provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub model: String,
    pub api_key_env: String,
    pub base_url: Option<String>,
    pub timeout_seconds: u64,
    pub max_tokens: u32,
    pub cache_dir: String,
}

impl LlmConfig {
    /// Build an `LlmConfig` from a provider name and optional model override.
    ///
    /// Fills in provider-specific defaults (API key env var, base URL,
    /// default model) so callers do not need to repeat the match arms.
    pub fn from_provider(provider: &str, model_override: Option<&str>) -> Self {
        let model = model_override
            .map(|s| s.to_string())
            .unwrap_or_else(|| match provider {
                "openai" => "gpt-4o".to_string(),
                "ollama" => "llama3".to_string(),
                _ => "claude-sonnet-4-20250514".to_string(),
            });
        let api_key_env = match provider {
            "openai" => "OPENAI_API_KEY".to_string(),
            "ollama" => "OLLAMA_API_KEY".to_string(),
            _ => "ANTHROPIC_API_KEY".to_string(),
        };
        let base_url = if provider == "ollama" {
            Some("http://localhost:11434/v1".to_string())
        } else {
            None
        };
        Self {
            provider: provider.to_string(),
            model,
            api_key_env,
            base_url,
            ..Default::default()
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
            base_url: None,
            timeout_seconds: 60,
            max_tokens: 4096,
            cache_dir: ".assura-cache/llm".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Level 2: LLM-generated lemma chain
// ---------------------------------------------------------------------------

/// A single intermediate assertion (lemma) generated by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmLemma {
    pub label: String,
    pub assertion: String,
    pub justification: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

/// A chain of lemmas bridging requires to ensures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LemmaChain {
    pub lemmas: Vec<LlmLemma>,
    pub chain_complete: bool,
}

/// Z3 verification result for a single lemma.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LemmaVerification {
    pub label: String,
    pub assertion: String,
    pub result: LemmaResult,
    pub time_ms: u64,
}

/// Result of verifying a single lemma with Z3.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum LemmaResult {
    Valid,
    Counterexample { model: String },
    Timeout,
    ParseError { message: String },
}

/// Full result of Level 2 verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LemmaChainVerification {
    pub lemmas: Vec<LemmaVerification>,
    pub ensures_follows: bool,
    pub chain_valid: bool,
    pub valid_count: usize,
    pub total_count: usize,
}

/// Errors from LLM operations.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("HTTP request failed: {0}")]
    Http(String),
    #[error("API key not set: {env_var}")]
    ApiKeyMissing { env_var: String },
    #[error("LLM response parse error: {0}")]
    Parse(String),
    #[error("LLM request timed out after {seconds}s")]
    Timeout { seconds: u64 },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_pass_roundtrip() {
        let v = Verdict::Pass;
        let json = serde_json::to_string(&v).unwrap();
        let back: Verdict = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, Verdict::Pass));
    }

    #[test]
    fn verdict_fail_roundtrip() {
        let v = Verdict::Fail {
            violations: vec![Violation {
                clause_kind: "ensures".to_string(),
                clause_expression: "result > 0".to_string(),
                description: "returns negative".to_string(),
                evidence_line: Some(10),
            }],
        };
        let json = serde_json::to_string(&v).unwrap();
        let back: Verdict = serde_json::from_str(&json).unwrap();
        match back {
            Verdict::Fail { violations } => {
                assert_eq!(violations.len(), 1);
                assert_eq!(violations[0].clause_expression, "result > 0");
                assert_eq!(violations[0].evidence_line, Some(10));
            }
            _ => panic!("expected Fail variant"),
        }
    }

    #[test]
    fn verdict_uncertain_roundtrip() {
        let v = Verdict::Uncertain {
            reason: "complex control flow".to_string(),
        };
        let json = serde_json::to_string(&v).unwrap();
        let back: Verdict = serde_json::from_str(&json).unwrap();
        match back {
            Verdict::Uncertain { reason } => assert_eq!(reason, "complex control flow"),
            _ => panic!("expected Uncertain variant"),
        }
    }

    #[test]
    fn lemma_result_roundtrip() {
        for (variant, expected_tag) in [
            (LemmaResult::Valid, "valid"),
            (
                LemmaResult::Counterexample {
                    model: "x = -1".to_string(),
                },
                "counterexample",
            ),
            (LemmaResult::Timeout, "timeout"),
            (
                LemmaResult::ParseError {
                    message: "bad syntax".to_string(),
                },
                "parse_error",
            ),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert!(
                json.contains(expected_tag),
                "tag {expected_tag} missing in {json}"
            );
            let _back: LemmaResult = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn suggestion_kind_roundtrip() {
        for kind in [
            SuggestionKind::Requires,
            SuggestionKind::Ensures,
            SuggestionKind::EnsuresOk,
            SuggestionKind::EnsuresErr,
            SuggestionKind::Invariant,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: SuggestionKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn from_provider_anthropic_defaults() {
        let cfg = LlmConfig::from_provider("anthropic", None);
        assert_eq!(cfg.provider, "anthropic");
        assert_eq!(cfg.api_key_env, "ANTHROPIC_API_KEY");
        assert!(cfg.model.contains("claude"));
        assert!(cfg.base_url.is_none());
    }

    #[test]
    fn from_provider_openai() {
        let cfg = LlmConfig::from_provider("openai", Some("gpt-4-turbo"));
        assert_eq!(cfg.provider, "openai");
        assert_eq!(cfg.api_key_env, "OPENAI_API_KEY");
        assert_eq!(cfg.model, "gpt-4-turbo");
        assert!(cfg.base_url.is_none());
    }

    #[test]
    fn from_provider_ollama() {
        let cfg = LlmConfig::from_provider("ollama", None);
        assert_eq!(cfg.provider, "ollama");
        assert_eq!(cfg.api_key_env, "OLLAMA_API_KEY");
        assert_eq!(cfg.model, "llama3");
        assert_eq!(cfg.base_url.as_deref(), Some("http://localhost:11434/v1"));
    }

    #[test]
    fn llm_config_defaults() {
        let cfg = LlmConfig::default();
        assert_eq!(cfg.provider, "anthropic");
        assert_eq!(cfg.api_key_env, "ANTHROPIC_API_KEY");
        assert_eq!(cfg.timeout_seconds, 60);
        assert_eq!(cfg.max_tokens, 4096);
        assert!(cfg.base_url.is_none());
    }
}
