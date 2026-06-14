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
fn hash_clauses(contract_name: &str, clauses: &[assura_parser::ast::Clause]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    contract_name.hash(&mut hasher);
    for clause in clauses {
        format!("{:?}", clause.kind).hash(&mut hasher);
        format!("{:?}", clause.body).hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
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

/// Verify a contract's clauses, using the cache if available.
///
/// Checks the cache first. On miss, runs Z3 and stores the result.
pub fn verify_contract_cached(
    contract_name: &str,
    clauses: &[assura_parser::ast::Clause],
    cache: &VerificationCache,
) -> Vec<VerificationResult> {
    if let Some(results) = cache.get(contract_name, clauses) {
        return results;
    }
    let results = verify_contract(contract_name, clauses);
    cache.put(contract_name, clauses, &results);
    results
}
