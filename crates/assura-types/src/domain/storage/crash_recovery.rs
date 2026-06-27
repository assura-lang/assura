//! T086: STOR.1 Crash recovery contracts.

use assura_parser::ast::ClauseKind;

use crate::TypeError;
use crate::checkers::*;

/// Tracks write-ahead log (WAL) discipline and crash-safe commit sequences.
#[derive(Debug, Clone)]
pub(crate) struct CrashRecoveryChecker {
    wal_entries: Vec<WalEntry>,
    committed: Vec<String>,
    fsynced: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct WalEntry {
    pub id: String,
    pub data_written: bool,
    pub wal_written: bool,
    pub fsynced: bool,
}

impl CrashRecoveryChecker {
    pub fn new() -> Self {
        Self {
            wal_entries: Vec::new(),
            committed: Vec::new(),
            fsynced: Vec::new(),
        }
    }

    pub fn begin_write(&mut self, id: String) {
        self.wal_entries.push(WalEntry {
            id,
            data_written: false,
            wal_written: false,
            fsynced: false,
        });
    }

    pub fn write_wal(&mut self, id: &str) {
        if let Some(e) = self.wal_entries.iter_mut().find(|e| e.id == id) {
            e.wal_written = true;
        }
    }

    pub fn write_data(&mut self, id: &str) {
        if let Some(e) = self.wal_entries.iter_mut().find(|e| e.id == id) {
            e.data_written = true;
        }
    }

    pub fn fsync(&mut self, id: &str) {
        if let Some(e) = self.wal_entries.iter_mut().find(|e| e.id == id) {
            e.fsynced = true;
        }
        self.fsynced.push(id.to_string());
    }

    pub fn commit(&mut self, id: &str) {
        self.committed.push(id.to_string());
    }

    pub fn check_write_ahead(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for e in &self.wal_entries {
            if e.data_written && !e.wal_written {
                errors.push(TypeError {
                    code: "A33001".into(),
                    message: format!("data write for `{}` without preceding WAL entry", e.id),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    pub fn check_commit_durability(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for id in &self.committed {
            if !self.fsynced.contains(id) {
                errors.push(TypeError {
                    code: "A33002".into(),
                    message: format!("commit for `{id}` without fsync"),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    pub fn check_ordering(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for e in &self.wal_entries {
            if e.fsynced && !e.data_written {
                errors.push(TypeError {
                    code: "A33003".into(),
                    message: format!("fsync for `{}` before data write", e.id),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    pub fn check_all(&self) -> Vec<TypeError> {
        let mut errs = self.check_write_ahead();
        errs.extend(self.check_commit_durability());
        errs.extend(self.check_ordering());
        errs
    }
}

impl CrashRecoveryChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = Self::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "wal" || k == "crash_recovery" || k == "write_ahead" {
                        found = true;
                        if let Some(id) = extract_ident(&clause.body) {
                            checker.begin_write(id.to_string());
                        }
                    }
                    if (k == "write_data" || k == "data_write")
                        && let Some(id) = extract_ident(&clause.body)
                    {
                        checker.write_data(id);
                    }
                    if (k == "write_wal" || k == "wal_write")
                        && let Some(id) = extract_ident(&clause.body)
                    {
                        checker.write_wal(id);
                    }
                    if (k == "fsync" || k == "flush")
                        && let Some(id) = extract_ident(&clause.body)
                    {
                        checker.fsync(id);
                    }
                    if k == "commit"
                        && let Some(id) = extract_ident(&clause.body)
                    {
                        checker.commit(id);
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        checker.check_all()
    }
}

impl Default for CrashRecoveryChecker {
    fn default() -> Self {
        Self::new()
    }
}
