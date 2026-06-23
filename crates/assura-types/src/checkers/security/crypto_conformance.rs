use super::*;

// ---------------------------------------------------------------------------
// T061: SEC.5 Cryptographic conformance
// ---------------------------------------------------------------------------

/// Error from the cryptographic conformance checker.
pub(crate) type CryptoConformanceError = CheckerError;

/// A cryptographic algorithm specification.
#[derive(Debug, Clone)]
pub(crate) struct CryptoSpec {
    pub name: String,
    pub key_size_bits: Vec<u32>,
    pub block_size_bytes: Option<u32>,
    pub nonce_size_bytes: Option<u32>,
    pub tag_size_bytes: Option<u32>,
}

/// Checker for cryptographic conformance.
///
/// Validates that cryptographic implementations match their mathematical
/// specifications: correct key sizes, nonce handling, tag verification.
pub(crate) struct CryptoConformanceChecker {
    /// Known algorithm specs
    specs: HashMap<String, CryptoSpec>,
}

impl CryptoConformanceChecker {
    pub fn new() -> Self {
        let mut specs = HashMap::new();
        // Register common algorithms
        specs.insert(
            "AES-128-GCM".into(),
            CryptoSpec {
                name: "AES-128-GCM".into(),
                key_size_bits: vec![128],
                block_size_bytes: Some(16),
                nonce_size_bytes: Some(12),
                tag_size_bytes: Some(16),
            },
        );
        specs.insert(
            "AES-256-GCM".into(),
            CryptoSpec {
                name: "AES-256-GCM".into(),
                key_size_bits: vec![256],
                block_size_bytes: Some(16),
                nonce_size_bytes: Some(12),
                tag_size_bytes: Some(16),
            },
        );
        specs.insert(
            "ChaCha20-Poly1305".into(),
            CryptoSpec {
                name: "ChaCha20-Poly1305".into(),
                key_size_bits: vec![256],
                block_size_bytes: None,
                nonce_size_bytes: Some(12),
                tag_size_bytes: Some(16),
            },
        );
        Self { specs }
    }

    /// Register a custom algorithm specification.
    pub fn register_spec(&mut self, spec: CryptoSpec) {
        self.specs.insert(spec.name.clone(), spec);
    }

    /// Check that a key size matches the algorithm spec.
    /// - A17001: wrong key size for algorithm
    pub fn check_key_size(
        &self,
        algorithm: &str,
        key_size_bits: u32,
        span: &Range<usize>,
    ) -> Vec<CryptoConformanceError> {
        let mut errors = Vec::new();
        if let Some(spec) = self
            .specs
            .get(algorithm)
            .filter(|s| !s.key_size_bits.contains(&key_size_bits))
        {
            let mut msg = format!(
                "key size {key_size_bits} bits does not match `{}` \
                 which requires {:?} bits",
                spec.name, spec.key_size_bits
            );
            if let Some(bs) = spec.block_size_bytes {
                msg.push_str(&format!(" (block size: {bs} bytes)"));
            }
            if let Some(ts) = spec.tag_size_bytes {
                msg.push_str(&format!(" (tag size: {ts} bytes)"));
            }
            errors.push(CryptoConformanceError {
                code: "A17001".into(),
                message: msg,
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that a nonce size matches the algorithm spec.
    /// - A17002: wrong nonce size for algorithm
    pub fn check_nonce_size(
        &self,
        algorithm: &str,
        nonce_size_bytes: u32,
        span: &Range<usize>,
    ) -> Vec<CryptoConformanceError> {
        let mut errors = Vec::new();
        let mismatch = self
            .specs
            .get(algorithm)
            .and_then(|s| s.nonce_size_bytes)
            .filter(|&expected| nonce_size_bytes != expected);
        if let Some(expected) = mismatch {
            errors.push(CryptoConformanceError {
                code: "A17002".into(),
                message: format!(
                    "nonce size {nonce_size_bytes} bytes does not match `{algorithm}` \
                     which requires {expected} bytes"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that nonce reuse is prevented.
    /// - A17003: potential nonce reuse detected
    pub fn check_nonce_uniqueness(
        &self,
        nonce_source: &str,
        is_counter: bool,
        is_random: bool,
        span: &Range<usize>,
    ) -> Vec<CryptoConformanceError> {
        let mut errors = Vec::new();
        if !is_counter && !is_random {
            errors.push(CryptoConformanceError {
                code: "A17003".into(),
                message: format!(
                    "nonce `{nonce_source}` is neither counter-based nor random; \
                     potential nonce reuse"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that authentication tag is verified before using decrypted data.
    /// - A17004: decrypted data used before tag verification
    pub fn check_tag_verification(
        &self,
        has_tag_check: bool,
        span: &Range<usize>,
    ) -> Vec<CryptoConformanceError> {
        let mut errors = Vec::new();
        if !has_tag_check {
            errors.push(CryptoConformanceError {
                code: "A17004".into(),
                message: "decrypted data used before authentication tag verification; \
                          verify the tag before processing plaintext"
                    .into(),
                span: span.clone(),
            });
        }
        errors
    }
}

impl Default for CryptoConformanceChecker {
    fn default() -> Self {
        Self::new()
    }
}
