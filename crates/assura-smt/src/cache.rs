use super::*;

// ===========================================================================
// T113: Verification caching
// ===========================================================================

// ---------------------------------------------------------------------------
// Session cache (in-memory, per-session deduplication for verify_clauses)
// ---------------------------------------------------------------------------

/// Entry in the per-session in-memory verification cache.
#[derive(Debug, Clone)]
pub struct SessionCacheEntry {
    pub result: String,
}

/// In-memory cache used within a single verification session to avoid
/// re-verifying the same clause twice. Not persisted across runs.
#[derive(Debug)]
pub struct SessionCache {
    entries: std::collections::HashMap<String, SessionCacheEntry>,
    hits: usize,
    misses: usize,
}

impl SessionCache {
    pub fn new() -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            hits: 0,
            misses: 0,
        }
    }

    pub fn insert(&mut self, key: String, result: String, _cost: usize) {
        self.entries.insert(key, SessionCacheEntry { result });
    }

    pub fn lookup(&mut self, key: &str) -> Option<&SessionCacheEntry> {
        if self.entries.contains_key(key) {
            self.hits += 1;
            self.entries.get(key)
        } else {
            self.misses += 1;
            None
        }
    }

    pub fn invalidate(&mut self, key: &str) {
        self.entries.remove(key);
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.hits = 0;
        self.misses = 0;
    }
}

impl Default for SessionCache {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Verification caching (P006)
// ---------------------------------------------------------------------------

/// Serializable representation of a cached verification result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum CachedResult {
    Verified { clause_desc: String },
    Counterexample { clause_desc: String, model: String },
    Timeout { clause_desc: String },
    Unknown { clause_desc: String, reason: String },
}

impl From<&VerificationResult> for CachedResult {
    fn from(r: &VerificationResult) -> Self {
        match r {
            VerificationResult::Verified { clause_desc } => CachedResult::Verified {
                clause_desc: clause_desc.clone(),
            },
            VerificationResult::Counterexample {
                clause_desc, model, ..
            } => CachedResult::Counterexample {
                clause_desc: clause_desc.clone(),
                model: model.clone(),
            },
            VerificationResult::Timeout { clause_desc } => CachedResult::Timeout {
                clause_desc: clause_desc.clone(),
            },
            VerificationResult::Unknown {
                clause_desc,
                reason,
            } => CachedResult::Unknown {
                clause_desc: clause_desc.clone(),
                reason: reason.clone(),
            },
        }
    }
}

impl From<CachedResult> for VerificationResult {
    fn from(c: CachedResult) -> Self {
        match c {
            CachedResult::Verified { clause_desc } => VerificationResult::Verified { clause_desc },
            CachedResult::Counterexample { clause_desc, model } => {
                VerificationResult::Counterexample {
                    clause_desc,
                    model,
                    counter_model: None,
                }
            }
            CachedResult::Timeout { clause_desc } => VerificationResult::Timeout { clause_desc },
            CachedResult::Unknown {
                clause_desc,
                reason,
            } => VerificationResult::Unknown {
                clause_desc,
                reason,
            },
        }
    }
}

/// Compute a stable content hash of a contract's clauses for cache keying.
///
/// Uses SHA-256 for deterministic hashing across Rust versions and platforms.
fn hash_clauses(contract_name: &str, clauses: &[assura_parser::ast::Clause]) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(contract_name.as_bytes());
    for clause in clauses {
        hasher.update(format!("{:?}", clause.kind).as_bytes());
        hasher.update(format!("{:?}", clause.body).as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// Verification cache backed by the filesystem.
///
/// Each contract's results are stored as a JSON file in `.assura-cache/verify/`
/// keyed by the content hash of its clauses. When the contract changes, the
/// hash changes, and the old cache entry is naturally invalidated.
pub struct VerificationCache {
    cache_dir: std::path::PathBuf,
}

impl VerificationCache {
    /// Create a cache rooted at the given project directory.
    ///
    /// Cache files are stored in `<base_dir>/.assura-cache/verify/`.
    pub fn new(base_dir: &std::path::Path) -> Self {
        let cache_dir = base_dir.join(".assura-cache").join("verify");
        let _ = std::fs::create_dir_all(&cache_dir);
        Self { cache_dir }
    }

    /// Look up cached verification results for a contract.
    pub fn get(
        &self,
        contract_name: &str,
        clauses: &[assura_parser::ast::Clause],
    ) -> Option<Vec<VerificationResult>> {
        let hash = hash_clauses(contract_name, clauses);
        let path = self.cache_dir.join(format!("{hash}.json"));
        let data = std::fs::read_to_string(&path).ok()?;
        let cached: Vec<CachedResult> = serde_json::from_str(&data).ok()?;
        Some(cached.into_iter().map(VerificationResult::from).collect())
    }

    /// Store verification results for a contract.
    pub fn put(
        &self,
        contract_name: &str,
        clauses: &[assura_parser::ast::Clause],
        results: &[VerificationResult],
    ) {
        let hash = hash_clauses(contract_name, clauses);
        let path = self.cache_dir.join(format!("{hash}.json"));
        let cached: Vec<CachedResult> = results.iter().map(CachedResult::from).collect();
        if let Ok(json) = serde_json::to_string(&cached) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// Remove all cached verification results.
    pub fn clear(&self) {
        let _ = std::fs::remove_dir_all(&self.cache_dir);
        let _ = std::fs::create_dir_all(&self.cache_dir);
    }

    /// Number of cached entries.
    pub fn entry_count(&self) -> usize {
        std::fs::read_dir(&self.cache_dir)
            .map(|rd| rd.filter(|e| e.is_ok()).count())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------
    // SessionCache tests
    // -------------------------------------------------------------------

    #[test]
    fn test_session_cache_insert_and_lookup() {
        let mut cache = SessionCache::new();
        cache.insert("key1".into(), "verified".into(), 0);
        let entry = cache.lookup("key1").unwrap();
        assert_eq!(entry.result, "verified");
    }

    #[test]
    fn test_session_cache_miss() {
        let mut cache = SessionCache::new();
        assert!(cache.lookup("nonexistent").is_none());
    }

    #[test]
    fn test_session_cache_hit_rate() {
        let mut cache = SessionCache::new();
        cache.insert("k".into(), "v".into(), 0);
        cache.lookup("k"); // hit
        cache.lookup("k"); // hit
        cache.lookup("miss"); // miss
        // 2 hits, 1 miss = 2/3
        let rate = cache.hit_rate();
        assert!((rate - 2.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn test_session_cache_hit_rate_empty() {
        let cache = SessionCache::new();
        assert_eq!(cache.hit_rate(), 0.0);
    }

    #[test]
    fn test_session_cache_invalidate() {
        let mut cache = SessionCache::new();
        cache.insert("k".into(), "v".into(), 0);
        cache.invalidate("k");
        assert!(cache.lookup("k").is_none());
    }

    #[test]
    fn test_session_cache_clear() {
        let mut cache = SessionCache::new();
        cache.insert("a".into(), "1".into(), 0);
        cache.insert("b".into(), "2".into(), 0);
        cache.lookup("a"); // hit
        cache.clear();
        assert_eq!(cache.entry_count(), 0);
        assert_eq!(cache.hit_rate(), 0.0);
    }

    #[test]
    fn test_session_cache_entry_count() {
        let mut cache = SessionCache::new();
        assert_eq!(cache.entry_count(), 0);
        cache.insert("a".into(), "1".into(), 0);
        assert_eq!(cache.entry_count(), 1);
        cache.insert("b".into(), "2".into(), 0);
        assert_eq!(cache.entry_count(), 2);
    }

    #[test]
    fn test_session_cache_overwrite() {
        let mut cache = SessionCache::new();
        cache.insert("k".into(), "old".into(), 0);
        cache.insert("k".into(), "new".into(), 0);
        let entry = cache.lookup("k").unwrap();
        assert_eq!(entry.result, "new");
        assert_eq!(cache.entry_count(), 1);
    }

    // -------------------------------------------------------------------
    // VerificationCache (filesystem) tests
    // -------------------------------------------------------------------

    #[test]
    fn test_verification_cache_miss_on_empty() {
        let dir = tempfile::tempdir().unwrap();
        let cache = VerificationCache::new(dir.path());
        let clauses: Vec<assura_parser::ast::Clause> = vec![];
        assert!(cache.get("test_contract", &clauses).is_none());
    }

    #[test]
    fn test_verification_cache_put_and_get() {
        let dir = tempfile::tempdir().unwrap();
        let cache = VerificationCache::new(dir.path());
        let clauses: Vec<assura_parser::ast::Clause> = vec![];
        let results = vec![VerificationResult::Verified {
            clause_desc: "test::ensures".into(),
        }];
        cache.put("my_contract", &clauses, &results);
        let got = cache.get("my_contract", &clauses).unwrap();
        assert_eq!(got.len(), 1);
        assert!(matches!(got[0], VerificationResult::Verified { .. }));
    }

    #[test]
    fn test_verification_cache_clear() {
        let dir = tempfile::tempdir().unwrap();
        let cache = VerificationCache::new(dir.path());
        let clauses: Vec<assura_parser::ast::Clause> = vec![];
        cache.put("c", &clauses, &[]);
        assert!(cache.entry_count() > 0);
        cache.clear();
        assert_eq!(cache.entry_count(), 0);
    }

    #[test]
    fn test_verification_cache_different_contracts() {
        let dir = tempfile::tempdir().unwrap();
        let cache = VerificationCache::new(dir.path());
        let clauses: Vec<assura_parser::ast::Clause> = vec![];
        let r1 = vec![VerificationResult::Verified {
            clause_desc: "a::ensures".into(),
        }];
        let r2 = vec![VerificationResult::Timeout {
            clause_desc: "b::ensures".into(),
        }];
        cache.put("a", &clauses, &r1);
        cache.put("b", &clauses, &r2);
        let got_a = cache.get("a", &clauses).unwrap();
        let got_b = cache.get("b", &clauses).unwrap();
        assert!(matches!(got_a[0], VerificationResult::Verified { .. }));
        assert!(matches!(got_b[0], VerificationResult::Timeout { .. }));
    }

    #[test]
    fn test_cached_result_roundtrip_counterexample() {
        let dir = tempfile::tempdir().unwrap();
        let cache = VerificationCache::new(dir.path());
        let clauses: Vec<assura_parser::ast::Clause> = vec![];
        let results = vec![VerificationResult::Counterexample {
            clause_desc: "c::ensures".into(),
            model: "x -> 5".into(),
            counter_model: None,
        }];
        cache.put("c", &clauses, &results);
        let got = cache.get("c", &clauses).unwrap();
        assert!(matches!(got[0], VerificationResult::Counterexample { .. }));
    }

    #[test]
    fn test_hash_clauses_deterministic() {
        // Regression test for #56: hash must be stable across runs
        let clauses: Vec<assura_parser::ast::Clause> = vec![];
        let h1 = super::hash_clauses("contract_a", &clauses);
        let h2 = super::hash_clauses("contract_a", &clauses);
        assert_eq!(h1, h2, "same input must produce same hash");
        // SHA-256 produces 64 hex chars
        assert_eq!(h1.len(), 64, "SHA-256 hex output is 64 chars");
    }

    #[test]
    fn test_hash_clauses_different_contracts() {
        let clauses: Vec<assura_parser::ast::Clause> = vec![];
        let h1 = super::hash_clauses("alpha", &clauses);
        let h2 = super::hash_clauses("beta", &clauses);
        assert_ne!(
            h1, h2,
            "different contract names must produce different hashes"
        );
    }

    #[test]
    fn test_cached_result_roundtrip_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let cache = VerificationCache::new(dir.path());
        let clauses: Vec<assura_parser::ast::Clause> = vec![];
        let results = vec![VerificationResult::Unknown {
            clause_desc: "c::ensures".into(),
            reason: "solver error".into(),
        }];
        cache.put("c", &clauses, &results);
        let got = cache.get("c", &clauses).unwrap();
        match &got[0] {
            VerificationResult::Unknown { reason, .. } => {
                assert_eq!(reason, "solver error");
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }
}
