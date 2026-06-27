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
