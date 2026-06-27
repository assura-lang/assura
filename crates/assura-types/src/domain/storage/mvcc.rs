//! T088: STOR.3 MVCC / snapshot isolation.

use assura_parser::ast::{ClauseKind, Expr, SpExpr};

use crate::TypeError;
use crate::checkers::*;
use crate::types::*;

#[derive(Debug, Clone)]
pub(crate) struct MvccChecker {
    versions: std::collections::HashMap<String, Vec<MvccVersion>>,
    active_snapshots: Vec<u64>,
    next_txn_id: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct MvccVersion {
    pub txn_id: u64,
    pub committed: bool,
}

impl MvccChecker {
    pub fn new() -> Self {
        Self {
            versions: std::collections::HashMap::new(),
            active_snapshots: Vec::new(),
            next_txn_id: 1,
        }
    }

    pub fn begin_txn(&mut self) -> u64 {
        let id = self.next_txn_id;
        self.next_txn_id += 1;
        self.active_snapshots.push(id);
        id
    }

    pub fn write_version(&mut self, key: String, txn_id: u64) {
        self.versions.entry(key).or_default().push(MvccVersion {
            txn_id,
            committed: false,
        });
    }

    pub fn commit_txn(&mut self, txn_id: u64) {
        self.active_snapshots.retain(|&id| id != txn_id);
        for versions in self.versions.values_mut() {
            for v in versions.iter_mut() {
                if v.txn_id == txn_id {
                    v.committed = true;
                }
            }
        }
    }

    pub fn check_write_conflicts(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (key, versions) in &self.versions {
            let uncommitted: Vec<_> = versions.iter().filter(|v| !v.committed).collect();
            if uncommitted.len() > 1 {
                errors.push(TypeError {
                    code: "A35001".into(),
                    message: format!(
                        "write-write conflict on key `{key}`: {} uncommitted versions",
                        uncommitted.len()
                    ),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    pub fn check_snapshot_read(&self, key: &str, reader_txn: u64) -> Option<TypeError> {
        if let Some(versions) = self.versions.get(key) {
            for v in versions {
                if v.txn_id != reader_txn
                    && !v.committed
                    && self.active_snapshots.contains(&v.txn_id)
                {
                    return Some(TypeError {
                        code: "A35002".into(),
                        message: format!(
                            "snapshot isolation violation: txn {reader_txn} reads uncommitted from txn {} on `{key}`",
                            v.txn_id
                        ),
                        span: 0..1,
                        secondary: None,
                        suggestion: None,
                    });
                }
            }
        }
        None
    }

    pub fn check_phantom(&self, txn_id: u64) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (key, versions) in &self.versions {
            for v in versions {
                if v.txn_id > txn_id && v.committed {
                    errors.push(TypeError { code: "A35003".into(), message: format!("phantom read: txn {txn_id} sees committed version from later txn {} on `{key}`", v.txn_id), span: 0..1, secondary: None, suggestion: None });
                }
            }
        }
        errors
    }
}

impl MvccChecker {
    /// Scan an expression for MVCC operations.
    fn scan_expr(expr: &SpExpr, checker: &mut MvccChecker) {
        if let Some((name, args)) = extract_call(expr) {
            match name {
                "begin_txn" | "begin" | "start_transaction" => {
                    checker.begin_txn();
                }
                "write" | "write_version" | "put" => {
                    let key = args
                        .first()
                        .and_then(extract_ident)
                        .unwrap_or("default")
                        .to_string();
                    let txn_id = args
                        .get(1)
                        .and_then(extract_int_literal)
                        .unwrap_or(DEFAULT_PARAM_ONE) as u64;
                    checker.write_version(key, txn_id);
                }
                "commit" | "commit_txn" => {
                    let txn_id = args
                        .first()
                        .and_then(extract_int_literal)
                        .unwrap_or(DEFAULT_PARAM_ONE) as u64;
                    checker.commit_txn(txn_id);
                }
                _ => {}
            }
        }
        match &expr.node {
            Expr::Block(exprs) | Expr::List(exprs) => {
                for e in exprs {
                    Self::scan_expr(e, checker);
                }
            }
            Expr::BinOp { lhs, rhs, .. } => {
                Self::scan_expr(lhs, checker);
                Self::scan_expr(rhs, checker);
            }
            _ => {}
        }
    }

    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = Self::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "mvcc" || k == "snapshot_isolation" || k == "serializable")
                {
                    found = true;
                    Self::scan_expr(&clause.body, &mut checker);
                }
                if found && matches!(clause.kind, ClauseKind::Requires | ClauseKind::Ensures) {
                    Self::scan_expr(&clause.body, &mut checker);
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let mut errors = checker.check_write_conflicts();
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if clause.kind == ClauseKind::Ensures {
                    let refs = collect_ident_references(&clause.body);
                    for name in &refs {
                        if let Some(err) = checker.check_snapshot_read(name, 1) {
                            errors.push(err);
                        }
                    }
                }
            }
        }
        errors.extend(checker.check_phantom(1));
        errors
    }
}

impl Default for MvccChecker {
    fn default() -> Self {
        Self::new()
    }
}
