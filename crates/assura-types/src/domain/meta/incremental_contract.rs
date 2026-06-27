use assura_parser::ast::{ClauseKind, Expr};

use crate::checkers::{extract_ident, extract_int_literal, extract_kv_pairs};
use crate::checks::clauses_contract_fn_block;
use crate::types::{DEFAULT_PARAM_ONE, DEFAULT_PARAM_ZERO};
use crate::TypeError;

// ===========================================================================
// T104: MISC.1 Incremental contracts
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct IncrementalContractChecker {
    contracts: std::collections::HashMap<String, ContractHistoryEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct ContractHistoryEntry {
    pub versions: Vec<ContractVersionEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct ContractVersionEntry {
    pub version: u32,
    pub requires_count: usize,
    pub ensures_count: usize,
    pub span: std::ops::Range<usize>,
}

impl IncrementalContractChecker {
    pub fn new() -> Self {
        Self {
            contracts: std::collections::HashMap::new(),
        }
    }

    pub fn add_version(
        &mut self,
        name: String,
        version: u32,
        requires_count: usize,
        ensures_count: usize,
        span: std::ops::Range<usize>,
    ) {
        let history = self
            .contracts
            .entry(name)
            .or_insert_with(|| ContractHistoryEntry {
                versions: Vec::new(),
            });
        history.versions.push(ContractVersionEntry {
            version,
            requires_count,
            ensures_count,
            span,
        });
    }

    pub fn check_precondition_weakening(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, history) in &self.contracts {
            for i in 1..history.versions.len() {
                if history.versions[i].requires_count > history.versions[i - 1].requires_count {
                    errors.push(TypeError {
                        code: "A51001".into(),
                        message: format!(
                            "contract `{name}` v{} strengthens preconditions",
                            history.versions[i].version
                        ),
                        span: history.versions[i].span.clone(),
                        secondary: Some((
                            history.versions[i - 1].span.clone(),
                            format!("previous version v{}", history.versions[i - 1].version),
                        )),
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_postcondition_strengthening(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, history) in &self.contracts {
            for i in 1..history.versions.len() {
                if history.versions[i].ensures_count < history.versions[i - 1].ensures_count {
                    errors.push(TypeError {
                        code: "A51002".into(),
                        message: format!(
                            "contract `{name}` v{} weakens postconditions",
                            history.versions[i].version
                        ),
                        span: history.versions[i].span.clone(),
                        secondary: Some((
                            history.versions[i - 1].span.clone(),
                            format!("previous version v{}", history.versions[i - 1].version),
                        )),
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_version_continuity(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, history) in &self.contracts {
            for i in 1..history.versions.len() {
                if history.versions[i].version != history.versions[i - 1].version + 1 {
                    errors.push(TypeError {
                        code: "A51003".into(),
                        message: format!(
                            "contract `{name}` has version gap: v{} to v{}",
                            history.versions[i - 1].version,
                            history.versions[i].version
                        ),
                        span: history.versions[i].span.clone(),
                        secondary: Some((
                            history.versions[i - 1].span.clone(),
                            format!("v{}", history.versions[i - 1].version),
                        )),
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }
}

impl Default for IncrementalContractChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Incremental contracts source walking
// ===========================================================================

impl IncrementalContractChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = IncrementalContractChecker::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            let requires_count = clauses
                .iter()
                .filter(|c| matches!(c.kind, ClauseKind::Requires))
                .count();
            let ensures_count = clauses
                .iter()
                .filter(|c| matches!(c.kind, ClauseKind::Ensures))
                .count();
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "version" || k == "incremental" || k == "contract_version")
                {
                    found = true;
                    match &clause.body.node {
                        Expr::Call { func, args } => {
                            if let Expr::Ident(name) = &func.as_ref().node {
                                let major = args
                                    .first()
                                    .and_then(extract_int_literal)
                                    .unwrap_or(DEFAULT_PARAM_ONE)
                                    as u32;
                                let minor = args
                                    .get(1)
                                    .and_then(extract_int_literal)
                                    .unwrap_or(DEFAULT_PARAM_ZERO)
                                    as u32;
                                let patch = args
                                    .get(2)
                                    .and_then(extract_int_literal)
                                    .unwrap_or(DEFAULT_PARAM_ZERO)
                                    as u32;
                                let version = major * 10000 + minor * 100 + patch;
                                checker.add_version(
                                    name.clone(),
                                    version,
                                    requires_count,
                                    ensures_count,
                                    decl.span.clone(),
                                );
                            }
                        }
                        Expr::Ident(name) => {
                            checker.add_version(
                                name.clone(),
                                10000,
                                requires_count,
                                ensures_count,
                                decl.span.clone(),
                            );
                        }
                        _ => {
                            let kvs = extract_kv_pairs(&clause.body);
                            let name = kvs
                                .iter()
                                .find(|(k, _)| *k == "name" || *k == "contract")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("unnamed")
                                .to_string();
                            let major = kvs
                                .iter()
                                .find(|(k, _)| *k == "major")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ONE)
                                as u32;
                            let minor = kvs
                                .iter()
                                .find(|(k, _)| *k == "minor")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ZERO)
                                as u32;
                            let patch = kvs
                                .iter()
                                .find(|(k, _)| *k == "patch")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ZERO)
                                as u32;
                            let version = major * 10000 + minor * 100 + patch;
                            checker.add_version(
                                name,
                                version,
                                requires_count,
                                ensures_count,
                                decl.span.clone(),
                            );
                        }
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let mut errors = checker.check_precondition_weakening();
        errors.extend(checker.check_postcondition_strengthening());
        errors.extend(checker.check_version_continuity());
        errors
    }
}
