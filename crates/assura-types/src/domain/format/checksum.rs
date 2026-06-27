// ===========================================================================
// T074: FMT.5 Checksum integrity
// ===========================================================================

use std::collections::HashMap;
use std::ops::Range;

use assura_parser::ast::{ClauseKind, Expr, SpExpr};

use crate::TypeError;
use crate::checkers::*;
use crate::types::*;

/// Validates checksum verification contracts.
///
/// Error codes:
/// - A29001: data used before checksum verification
/// - A29002: checksum algorithm mismatch
/// - A29003: checksum covers wrong byte range
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ChecksumAlgorithm {
    Crc32,
    Adler32,
    Sha256,
    Sha512,
    Md5,
    Custom(String),
}

#[derive(Debug, Clone)]
pub(crate) struct ChecksumChecker {
    /// Data regions and their checksum status
    regions: HashMap<String, ChecksumRegion>,
}

#[derive(Debug, Clone)]
pub(crate) struct ChecksumRegion {
    pub algorithm: ChecksumAlgorithm,
    pub byte_start: usize,
    pub byte_end: usize,
    pub verified: bool,
}

impl ChecksumChecker {
    pub fn new() -> Self {
        Self {
            regions: HashMap::new(),
        }
    }

    pub fn declare_region(
        &mut self,
        name: String,
        algorithm: ChecksumAlgorithm,
        start: usize,
        end: usize,
    ) {
        self.regions.insert(
            name,
            ChecksumRegion {
                algorithm,
                byte_start: start,
                byte_end: end,
                verified: false,
            },
        );
    }

    pub fn mark_verified(&mut self, name: &str) {
        if let Some(region) = self.regions.get_mut(name) {
            region.verified = true;
        }
    }

    /// Look up a region's declared algorithm and byte range.
    pub fn region_info(&self, name: &str) -> Option<(&ChecksumAlgorithm, usize, usize)> {
        self.regions
            .get(name)
            .map(|r| (&r.algorithm, r.byte_start, r.byte_end))
    }

    pub fn check_use_before_verify(&self, name: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some(region) = self.regions.get(name)
            && !region.verified
        {
            return Some(TypeError {
                code: "A29001".into(),
                message: format!("data region `{name}` used before checksum verification"),
                span: span.clone(),
                secondary: None,
                suggestion: None,
            });
        }
        None
    }

    pub fn check_algorithm_match(
        &self,
        name: &str,
        expected: &ChecksumAlgorithm,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if let Some(region) = self.regions.get(name)
            && &region.algorithm != expected
        {
            return Some(TypeError {
                code: "A29002".into(),
                message: format!(
                    "checksum algorithm mismatch for `{name}`: declared {:?}, used {:?}",
                    region.algorithm, expected
                ),
                span: span.clone(),
                secondary: None,
                suggestion: None,
            });
        }
        None
    }

    pub fn check_range_coverage(
        &self,
        name: &str,
        data_start: usize,
        data_end: usize,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if let Some(region) = self.regions.get(name)
            && (region.byte_start > data_start || region.byte_end < data_end)
        {
            return Some(TypeError {
                code: "A29003".into(),
                message: format!(
                    "checksum for `{name}` covers [{},{}] but data range is [{data_start},{data_end}]",
                    region.byte_start, region.byte_end
                ),
                span: span.clone(),
                secondary: None,
                suggestion: None,
            });
        }
        None
    }
}

impl Default for ChecksumChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ChecksumChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = ChecksumChecker::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "checksum" || k == "crc" || k == "hash" {
                        found = true;
                        parse_checksum_decl(&mut checker, &clause.body, &decl.span);
                    }
                    if (k == "verify_checksum" || k == "verified")
                        && let Expr::Ident(name) = &clause.body.node
                    {
                        checker.mark_verified(name);
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let mut errors = Vec::new();
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                    let refs = collect_ident_references(&clause.body);
                    for name in &refs {
                        if let Some(err) = checker.check_use_before_verify(name, &decl.span) {
                            errors.push(err);
                        }
                        if let Some((algo, region_start, region_end)) = checker.region_info(name) {
                            let algo = algo.clone();
                            if let Some(err) =
                                checker.check_algorithm_match(name, &algo, &decl.span)
                            {
                                errors.push(err);
                            }
                            if let Some(err) = checker.check_range_coverage(
                                name,
                                region_start,
                                region_end,
                                &decl.span,
                            ) {
                                errors.push(err);
                            }
                        }
                    }
                }
            }
        }
        errors
    }
}

/// Parse a checksum declaration clause body.
fn parse_checksum_decl(checker: &mut ChecksumChecker, body: &SpExpr, _span: &Range<usize>) {
    match &body.node {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = &func.as_ref().node {
                let algo = args
                    .first()
                    .and_then(extract_ident)
                    .map(parse_checksum_algorithm)
                    .unwrap_or(ChecksumAlgorithm::Crc32);
                let start = args
                    .get(1)
                    .and_then(extract_int_literal)
                    .unwrap_or(DEFAULT_PARAM_ZERO) as usize;
                let end = args
                    .get(2)
                    .and_then(extract_int_literal)
                    .unwrap_or(DEFAULT_REGION_SIZE) as usize;
                checker.declare_region(name.clone(), algo, start, end);
            }
        }
        Expr::Ident(name) => {
            checker.declare_region(name.clone(), ChecksumAlgorithm::Crc32, 0, 0);
        }
        _ => {
            let kvs = extract_kv_pairs(body);
            let name = kvs
                .iter()
                .find(|(k, _)| *k == "name" || *k == "region")
                .and_then(|(_, v)| extract_ident(v))
                .unwrap_or("unnamed")
                .to_string();
            let algo = kvs
                .iter()
                .find(|(k, _)| *k == "algorithm" || *k == "algo")
                .and_then(|(_, v)| extract_ident(v))
                .map(parse_checksum_algorithm)
                .unwrap_or(ChecksumAlgorithm::Crc32);
            let start = kvs
                .iter()
                .find(|(k, _)| *k == "start")
                .and_then(|(_, v)| extract_int_literal(v))
                .unwrap_or(DEFAULT_PARAM_ZERO) as usize;
            let end = kvs
                .iter()
                .find(|(k, _)| *k == "end")
                .and_then(|(_, v)| extract_int_literal(v))
                .unwrap_or(DEFAULT_REGION_SIZE) as usize;
            checker.declare_region(name, algo, start, end);
        }
    }
}

/// Parse a checksum algorithm name to the enum.
fn parse_checksum_algorithm(name: &str) -> ChecksumAlgorithm {
    match name {
        "crc32" | "CRC32" | "crc" => ChecksumAlgorithm::Crc32,
        "adler32" | "ADLER32" | "adler" => ChecksumAlgorithm::Adler32,
        "sha256" | "SHA256" | "sha-256" => ChecksumAlgorithm::Sha256,
        "sha512" | "SHA512" | "sha-512" => ChecksumAlgorithm::Sha512,
        "md5" | "MD5" => ChecksumAlgorithm::Md5,
        _ => ChecksumAlgorithm::Custom(name.to_string()),
    }
}
