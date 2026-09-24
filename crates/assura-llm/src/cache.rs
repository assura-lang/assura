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

fn update_len_prefixed(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

fn update_count(hasher: &mut Sha256, count: usize) {
    hasher.update((count as u64).to_le_bytes());
}

fn update_bool(hasher: &mut Sha256, value: bool) {
    update_len_prefixed(hasher, if value { "1" } else { "0" });
}

/// Compute cache key for analysis.
pub fn analysis_cache_key(
    function_name: &str,
    function_body: &str,
    contracts: &[ContractClauseInfo],
    context_hash: &str,
    model: &str,
    function_signature: &str,
    surrounding_types: &[TypeInfo],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"analysis-v3:");
    update_len_prefixed(&mut hasher, function_name);
    update_len_prefixed(&mut hasher, function_body);
    for c in contracts {
        update_len_prefixed(&mut hasher, &c.kind);
        update_len_prefixed(&mut hasher, &c.expression);
    }
    update_len_prefixed(&mut hasher, context_hash);
    update_len_prefixed(&mut hasher, model);
    update_len_prefixed(&mut hasher, crate::prompt::prompt_version());
    update_len_prefixed(&mut hasher, function_signature);
    update_count(&mut hasher, surrounding_types.len());
    for ty in surrounding_types {
        update_len_prefixed(&mut hasher, &ty.name);
        update_len_prefixed(&mut hasher, &ty.definition);
    }
    hex::encode(hasher.finalize())
}

/// Compute cache key for suggestions.
#[allow(clippy::too_many_arguments)]
pub fn suggest_cache_key(
    function_name: &str,
    function_body: &str,
    function_signature: &str,
    doc_comments: &str,
    siblings_hash: &str,
    model: &str,
    impl_type: Option<&str>,
    visibility: &str,
    is_unsafe: bool,
    is_async: bool,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"suggest-v3:");
    update_len_prefixed(&mut hasher, function_name);
    update_len_prefixed(&mut hasher, function_body);
    update_len_prefixed(&mut hasher, function_signature);
    update_len_prefixed(&mut hasher, doc_comments);
    update_len_prefixed(&mut hasher, siblings_hash);
    update_len_prefixed(&mut hasher, model);
    update_len_prefixed(&mut hasher, crate::prompt::prompt_version());
    update_len_prefixed(&mut hasher, impl_type.unwrap_or(""));
    update_len_prefixed(&mut hasher, visibility);
    update_bool(&mut hasher, is_unsafe);
    update_bool(&mut hasher, is_async);
    hex::encode(hasher.finalize())
}

/// Compute a hash of the analysis context (called functions' contracts).
pub fn context_hash(called: &[CalledFunctionContract]) -> String {
    let mut hasher = Sha256::new();
    for cf in called {
        update_len_prefixed(&mut hasher, &cf.name);
        update_len_prefixed(&mut hasher, &cf.signature);
        update_len_prefixed(&mut hasher, &cf.source_file);
        update_count(&mut hasher, cf.requires.len());
        for r in &cf.requires {
            update_len_prefixed(&mut hasher, r);
        }
        update_count(&mut hasher, cf.ensures.len());
        for e in &cf.ensures {
            update_len_prefixed(&mut hasher, e);
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
        let k1 = analysis_cache_key("foo", "x + 1", &[], "", "mock", "", &[]);
        let k2 = analysis_cache_key("foo", "x + 1", &[], "", "mock", "", &[]);
        assert_eq!(k1, k2);
    }

    #[test]
    fn cache_key_changes_with_body() {
        let k1 = analysis_cache_key("foo", "x + 1", &[], "", "mock", "", &[]);
        let k2 = analysis_cache_key("foo", "x + 2", &[], "", "mock", "", &[]);
        assert_ne!(k1, k2);
    }

    #[test]
    fn analysis_cache_key_name_body_boundary() {
        let left = analysis_cache_key("ab", "c", &[], "", "mock", "", &[]);
        let right = analysis_cache_key("a", "bc", &[], "", "mock", "", &[]);
        assert_ne!(left, right);
    }

    #[test]
    fn analysis_cache_key_context_model_boundary() {
        let left = analysis_cache_key("foo", "body", &[], "x", "m", "", &[]);
        let right = analysis_cache_key("foo", "body", &[], "", "xm", "", &[]);
        assert_ne!(left, right);
    }

    #[test]
    fn analysis_cache_key_clause_boundary() {
        let split_kind = [ContractClauseInfo {
            kind: "ab".to_string(),
            expression: "c".to_string(),
        }];
        let split_expr = [ContractClauseInfo {
            kind: "a".to_string(),
            expression: "bc".to_string(),
        }];
        let left = analysis_cache_key("foo", "body", &split_kind, "", "mock", "", &[]);
        let right = analysis_cache_key("foo", "body", &split_expr, "", "mock", "", &[]);
        assert_ne!(left, right);
    }

    #[test]
    fn suggest_cache_key_name_body_boundary() {
        let left = suggest_cache_key(
            "ab", "c", "sig", "doc", "sib", "mock", None, "", false, false,
        );
        let right = suggest_cache_key(
            "a", "bc", "sig", "doc", "sib", "mock", None, "", false, false,
        );
        assert_ne!(left, right);
    }

    #[test]
    fn context_hash_name_requires_boundary() {
        let name_longer = vec![CalledFunctionContract {
            name: "ab".to_string(),
            signature: String::new(),
            requires: vec!["c".to_string()],
            ensures: vec![],
            source_file: String::new(),
        }];
        let requires_longer = vec![CalledFunctionContract {
            name: "a".to_string(),
            signature: String::new(),
            requires: vec!["bc".to_string()],
            ensures: vec![],
            source_file: String::new(),
        }];
        assert_ne!(context_hash(&name_longer), context_hash(&requires_longer));
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

    #[test]
    fn roundtrip_suggestions_cache() {
        let dir = std::env::temp_dir().join("assura-llm-test-suggest-cache");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = LlmCache::new(&dir);

        let resp = SuggestionResponse {
            suggestions: vec![RawSuggestion {
                kind: "requires".to_string(),
                expression: "x > 0".to_string(),
                confidence: 0.9,
                reasoning: "guard clause".to_string(),
                evidence_line: Some(5),
            }],
            skipped_reason: None,
        };

        cache.put_suggestions("skey", &resp).unwrap();
        let got = cache.get_suggestions("skey").unwrap();
        assert_eq!(got.suggestions.len(), 1);
        assert_eq!(got.suggestions[0].expression, "x > 0");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn context_hash_separates_requires_and_ensures() {
        let requires_both = vec![CalledFunctionContract {
            name: "f".to_string(),
            signature: String::new(),
            requires: vec!["a".to_string(), "b".to_string()],
            ensures: vec![],
            source_file: String::new(),
        }];
        let split = vec![CalledFunctionContract {
            name: "f".to_string(),
            signature: String::new(),
            requires: vec!["a".to_string()],
            ensures: vec!["b".to_string()],
            source_file: String::new(),
        }];
        assert_ne!(context_hash(&requires_both), context_hash(&split));
    }

    #[test]
    fn context_hash_changes_with_signature() {
        let base = CalledFunctionContract {
            name: "f".to_string(),
            signature: "fn f()".to_string(),
            requires: vec![],
            ensures: vec![],
            source_file: "a.rs".to_string(),
        };
        let changed = CalledFunctionContract {
            signature: "fn f(x: i32)".to_string(),
            ..base.clone()
        };
        assert_ne!(context_hash(&[base]), context_hash(&[changed]));
    }

    #[test]
    fn analysis_cache_key_changes_with_signature() {
        let left = analysis_cache_key("foo", "body", &[], "", "mock", "fn foo()", &[]);
        let right = analysis_cache_key("foo", "body", &[], "", "mock", "fn foo(x: i32)", &[]);
        assert_ne!(left, right);
    }

    #[test]
    fn analysis_cache_key_changes_with_surrounding_type() {
        let original = [TypeInfo {
            name: "Point".to_string(),
            definition: "struct Point { x: i32 }".to_string(),
        }];
        let changed = [TypeInfo {
            name: "Point".to_string(),
            definition: "struct Point { x: i64 }".to_string(),
        }];
        let left = analysis_cache_key("foo", "body", &[], "", "mock", "fn foo()", &original);
        let right = analysis_cache_key("foo", "body", &[], "", "mock", "fn foo()", &changed);
        assert_ne!(left, right);
    }

    #[test]
    fn suggest_cache_key_changes_with_unsafe() {
        let safe = suggest_cache_key(
            "foo", "body", "fn foo()", "", "", "mock", None, "pub", false, false,
        );
        let unsafe_fn = suggest_cache_key(
            "foo", "body", "fn foo()", "", "", "mock", None, "pub", true, false,
        );
        assert_ne!(safe, unsafe_fn);
    }

    #[test]
    fn suggest_cache_key_changes_with_impl_type() {
        let free = suggest_cache_key(
            "foo", "body", "fn foo()", "", "", "mock", None, "pub", false, false,
        );
        let method = suggest_cache_key(
            "foo",
            "body",
            "fn foo()",
            "",
            "",
            "mock",
            Some("Foo"),
            "pub",
            false,
            false,
        );
        assert_ne!(free, method);
    }

    #[test]
    fn context_hash_deterministic() {
        let called = vec![CalledFunctionContract {
            name: "helper".to_string(),
            signature: "fn helper(x: i32) -> i32".to_string(),
            requires: vec!["x > 0".to_string()],
            ensures: vec!["result >= x".to_string()],
            source_file: "lib.rs".to_string(),
        }];
        let h1 = context_hash(&called);
        let h2 = context_hash(&called);
        assert_eq!(h1, h2);
    }

    #[test]
    fn context_hash_changes_with_contract() {
        let c1 = vec![CalledFunctionContract {
            name: "f".to_string(),
            signature: "fn f()".to_string(),
            requires: vec!["true".to_string()],
            ensures: vec![],
            source_file: "a.rs".to_string(),
        }];
        let c2 = vec![CalledFunctionContract {
            name: "f".to_string(),
            signature: "fn f()".to_string(),
            requires: vec!["false".to_string()],
            ensures: vec![],
            source_file: "a.rs".to_string(),
        }];
        assert_ne!(context_hash(&c1), context_hash(&c2));
    }

    #[test]
    fn context_hash_empty() {
        let h = context_hash(&[]);
        assert!(!h.is_empty());
    }

    #[test]
    fn hex_encode_works() {
        assert_eq!(hex::encode([0xde, 0xad, 0xbe, 0xef]), "deadbeef");
        assert_eq!(hex::encode([]), "");
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let dir = std::env::temp_dir().join("assura-llm-test-miss");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = LlmCache::new(&dir);
        assert!(cache.get_analysis("nonexistent").is_none());
        assert!(cache.get_suggestions("nonexistent").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
