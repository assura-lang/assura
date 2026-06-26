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

impl CryptoConformanceChecker {
    /// AST-walking entry point: scan for crypto conformance annotations and validate.
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        use assura_parser::ast::{ClauseKind, Expr, Literal};

        let mut all_errors = Vec::new();
        let mut checker = CryptoConformanceChecker::new();

        // Pre-register custom algorithm specs from "crypto_spec" clauses
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && k == "crypto_spec"
                    && let Expr::Raw(tokens) = &clause.body.node
                    && !tokens.is_empty()
                {
                    let name = tokens[0].trim_matches('"').to_string();
                    let key_bits: Vec<u32> = tokens
                        .get(1)
                        .and_then(|s| s.parse().ok())
                        .into_iter()
                        .collect();
                    let block_size = tokens.get(2).and_then(|s| s.parse().ok());
                    let nonce_size = tokens.get(3).and_then(|s| s.parse().ok());
                    let tag_size = tokens.get(4).and_then(|s| s.parse().ok());
                    checker.register_spec(CryptoSpec {
                        name,
                        key_size_bits: key_bits,
                        block_size_bytes: block_size,
                        nonce_size_bytes: nonce_size,
                        tag_size_bytes: tag_size,
                    });
                }
            }
        }

        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn_extern(&decl.node) else {
                continue;
            };
            let mut algorithm: Option<String> = None;
            let mut key_size: Option<u32> = None;
            let mut nonce_size: Option<u32> = None;
            let mut has_tag_check = false;
            let mut nonce_source: Option<String> = None;
            let mut is_counter_nonce = false;
            let mut is_random_nonce = false;

            for clause in clauses {
                let kind_name = match &clause.kind {
                    ClauseKind::Other(k) => k.as_str(),
                    _ => continue,
                };
                match kind_name {
                    "conforms" | "spec" | "crypto" => {
                        if let Expr::Literal(Literal::Str(name)) = &clause.body.node {
                            algorithm = Some(name.trim_matches('"').to_string());
                        } else if let Expr::Ident(name) = &clause.body.node {
                            algorithm = Some(name.clone());
                        } else if let Expr::Call { func, .. } = &clause.body.node {
                            if let Expr::Ident(name) = &func.as_ref().node {
                                algorithm = Some(name.clone());
                            }
                        } else if let Expr::Raw(tokens) = &clause.body.node
                            && let Some(t) = tokens.first()
                        {
                            let name = t.trim_matches('"').to_string();
                            if !name.is_empty() {
                                algorithm = Some(name);
                            }
                        }
                    }
                    "key_size" => {
                        if let Expr::Literal(Literal::Int(s)) = &clause.body.node {
                            key_size = s.parse().ok();
                        } else if let Expr::Raw(tokens) = &clause.body.node
                            && let Some(t) = tokens.first()
                        {
                            key_size = t.parse().ok();
                        }
                    }
                    "nonce_size" => {
                        if let Expr::Literal(Literal::Int(s)) = &clause.body.node {
                            nonce_size = s.parse().ok();
                        } else if let Expr::Raw(tokens) = &clause.body.node
                            && let Some(t) = tokens.first()
                        {
                            nonce_size = t.parse().ok();
                        }
                    }
                    "tag_verified" | "tag_check" => {
                        has_tag_check = true;
                    }
                    "nonce" => {
                        if let Expr::Ident(src) = &clause.body.node {
                            nonce_source = Some(src.clone());
                            is_counter_nonce = src.contains("counter") || src.contains("ctr");
                            is_random_nonce = src.contains("random") || src.contains("rng");
                        } else if let Expr::Raw(tokens) = &clause.body.node
                            && let Some(src) = tokens.first()
                        {
                            nonce_source = Some(src.clone());
                            is_counter_nonce = src.contains("counter") || src.contains("ctr");
                            is_random_nonce = src.contains("random") || src.contains("rng");
                        }
                    }
                    _ => {}
                }
            }

            if let Some(ref algo) = algorithm {
                if let Some(ks) = key_size {
                    for err in checker.check_key_size(algo, ks, &decl.span) {
                        all_errors.push(err.into());
                    }
                }
                if let Some(ns) = nonce_size {
                    for err in checker.check_nonce_size(algo, ns, &decl.span) {
                        all_errors.push(err.into());
                    }
                }
                if let Some(ref ns_src) = nonce_source {
                    for err in checker.check_nonce_uniqueness(
                        ns_src,
                        is_counter_nonce,
                        is_random_nonce,
                        &decl.span,
                    ) {
                        all_errors.push(err.into());
                    }
                }
                let has_decrypt_clause = clauses.iter().any(
                    |c| matches!(&c.kind, ClauseKind::Other(k) if k == "decrypt" || k == "decryption"),
                );
                if has_decrypt_clause {
                    for err in checker.check_tag_verification(has_tag_check, &decl.span) {
                        all_errors.push(err.into());
                    }
                }
            }
        }

        all_errors
    }
}
