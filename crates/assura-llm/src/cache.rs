//! Content-hash cache for LLM analysis results.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::types::*;

/// Cache for LLM analysis results, keyed by content hash.
pub struct LlmCache {
    cache_dir: PathBuf,
}

impl LlmCache {
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            cache_dir: cache_dir.into(),
        }
    }

    /// Get the cache directory path.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Look up a cached analysis result.
    pub fn get_analysis(&self, key: &str) -> Option<AnalysisResponse> {
        let path = self.cache_dir.join(format!("{key}.json"));
        let data = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Store an analysis result in the cache.
    pub fn put_analysis(&self, key: &str, response: &AnalysisResponse) -> Result<(), LlmError> {
        std::fs::create_dir_all(&self.cache_dir)?;
        let path = self.cache_dir.join(format!("{key}.json"));
        let data = serde_json::to_string_pretty(response)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    /// Look up cached suggestions.
    pub fn get_suggestions(&self, key: &str) -> Option<SuggestionResponse> {
        let sub = self.cache_dir.join("suggest");
        let path = sub.join(format!("{key}.json"));
        let data = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Store suggestions in the cache.
    pub fn put_suggestions(
        &self,
        key: &str,
        response: &SuggestionResponse,
    ) -> Result<(), LlmError> {
        let sub = self.cache_dir.join("suggest");
        std::fs::create_dir_all(&sub)?;
        let path = sub.join(format!("{key}.json"));
        let data = serde_json::to_string_pretty(response)?;
        std::fs::write(path, data)?;
        Ok(())
    }
}

/// Compute cache key for analysis.
pub fn analysis_cache_key(
    function_name: &str,
    function_body: &str,
    contracts: &[ContractClauseInfo],
    context_hash: &str,
    model: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"analysis-v1:");
    hasher.update(function_name.as_bytes());
    hasher.update(function_body.as_bytes());
    for c in contracts {
        hasher.update(c.kind.as_bytes());
        hasher.update(c.expression.as_bytes());
    }
    hasher.update(context_hash.as_bytes());
    hasher.update(model.as_bytes());
    hasher.update(crate::prompt::prompt_version().as_bytes());
    hex::encode(hasher.finalize())
}

/// Compute cache key for suggestions.
pub fn suggest_cache_key(
    function_name: &str,
    function_body: &str,
    function_signature: &str,
    doc_comments: &str,
    siblings_hash: &str,
    model: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"suggest-v1:");
    hasher.update(function_name.as_bytes());
    hasher.update(function_body.as_bytes());
    hasher.update(function_signature.as_bytes());
    hasher.update(doc_comments.as_bytes());
    hasher.update(siblings_hash.as_bytes());
    hasher.update(model.as_bytes());
    hasher.update(crate::prompt::prompt_version().as_bytes());
    hex::encode(hasher.finalize())
}

/// Compute a hash of the analysis context (called functions' contracts).
pub fn context_hash(called: &[CalledFunctionContract]) -> String {
    let mut hasher = Sha256::new();
    for cf in called {
        hasher.update(cf.name.as_bytes());
        for r in &cf.requires {
            hasher.update(r.as_bytes());
        }
        for e in &cf.ensures {
            hasher.update(e.as_bytes());
        }
    }
    hex::encode(hasher.finalize())
}

/// Hex encoding helper.
pub(crate) mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Get the default cache directory, resolved relative to a project root.
pub fn default_cache_dir(project_root: &Path) -> PathBuf {
    project_root.join(".assura-cache").join("llm")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_deterministic() {
        let k1 = analysis_cache_key("foo", "x + 1", &[], "", "mock");
        let k2 = analysis_cache_key("foo", "x + 1", &[], "", "mock");
        assert_eq!(k1, k2);
    }

    #[test]
    fn cache_key_changes_with_body() {
        let k1 = analysis_cache_key("foo", "x + 1", &[], "", "mock");
        let k2 = analysis_cache_key("foo", "x + 2", &[], "", "mock");
        assert_ne!(k1, k2);
    }

    #[test]
    fn roundtrip_cache() {
        let dir = std::env::temp_dir().join("assura-llm-test-cache");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = LlmCache::new(&dir);

        let resp = AnalysisResponse {
            verdict: Verdict::Pass,
            confidence: 0.99,
            paths: vec![],
            reasoning: "test".to_string(),
        };

        cache.put_analysis("testkey", &resp).unwrap();
        let got = cache.get_analysis("testkey").unwrap();
        assert!((got.confidence - 0.99).abs() < 0.01);
        assert!(matches!(got.verdict, Verdict::Pass));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
