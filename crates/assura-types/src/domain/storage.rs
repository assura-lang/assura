//! Storage-related domain checkers.
//!
//! CrashRecoveryChecker, PageCacheChecker, MvccChecker,
//! RollbackChecker, MonotonicStateChecker, StorageFailureChecker.

use assura_parser::ast::{ClauseKind, Expr, SpExpr};

use crate::TypeError;
use crate::checkers::*;
use crate::types::*;

// ===========================================================================
// T086: STOR.1 Crash recovery contracts
// ===========================================================================

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

// ===========================================================================
// T087: STOR.2 Page cache contracts
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct PageCacheChecker {
    pages: std::collections::HashMap<u64, PageInfo>,
    capacity: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct PageInfo {
    pub dirty: bool,
    pub pinned: bool,
    pub pin_count: u32,
}

impl PageCacheChecker {
    pub fn new(capacity: usize) -> Self {
        Self {
            pages: std::collections::HashMap::new(),
            capacity,
        }
    }

    pub fn load_page(&mut self, page_id: u64) {
        self.pages.insert(
            page_id,
            PageInfo {
                dirty: false,
                pinned: false,
                pin_count: 0,
            },
        );
    }

    pub fn pin(&mut self, page_id: u64) {
        if let Some(p) = self.pages.get_mut(&page_id) {
            p.pinned = true;
            p.pin_count += 1;
        }
    }

    pub fn unpin(&mut self, page_id: u64) {
        if let Some(p) = self.pages.get_mut(&page_id) {
            if p.pin_count > 0 {
                p.pin_count -= 1;
            }
            if p.pin_count == 0 {
                p.pinned = false;
            }
        }
    }

    pub fn mark_dirty(&mut self, page_id: u64) {
        if let Some(p) = self.pages.get_mut(&page_id) {
            p.dirty = true;
        }
    }

    pub fn flush(&mut self, page_id: u64) {
        if let Some(p) = self.pages.get_mut(&page_id) {
            p.dirty = false;
        }
    }

    pub fn evict(&mut self, page_id: u64) -> Option<TypeError> {
        if let Some(p) = self.pages.get(&page_id) {
            if p.pinned {
                return Some(TypeError {
                    code: "A34001".into(),
                    message: format!("cannot evict pinned page {page_id}"),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
            if p.dirty {
                return Some(TypeError {
                    code: "A34002".into(),
                    message: format!("evicting dirty page {page_id} without flush"),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        self.pages.remove(&page_id);
        None
    }

    pub fn check_capacity(&self) -> Vec<TypeError> {
        if self.pages.len() > self.capacity {
            vec![TypeError {
                code: "A34003".into(),
                message: format!(
                    "page cache size {} exceeds capacity {}",
                    self.pages.len(),
                    self.capacity
                ),
                span: 0..1,
                secondary: None,
                suggestion: None,
            }]
        } else {
            vec![]
        }
    }
}

impl PageCacheChecker {
    /// Scan an expression for page cache operations.
    fn scan_expr(expr: &SpExpr, checker: &mut PageCacheChecker) {
        if let Some((name, args)) = extract_call(expr) {
            let page_id = args
                .first()
                .and_then(extract_int_literal)
                .unwrap_or(DEFAULT_PARAM_ZERO) as u64;
            match name {
                "load_page" | "load" | "fetch_page" => checker.load_page(page_id),
                "pin" | "pin_page" => checker.pin(page_id),
                "unpin" | "unpin_page" => checker.unpin(page_id),
                "mark_dirty" | "dirty" => checker.mark_dirty(page_id),
                "flush" | "flush_page" => checker.flush(page_id),
                "evict" | "evict_page" => {
                    checker.evict(page_id);
                }
                _ => {}
            }
        }
    }

    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker: Option<PageCacheChecker> = None;
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "page_cache" || k == "buffer_pool" || k == "cache_policy")
                {
                    let capacity = match &clause.body.node {
                        Expr::Call { args, .. } => {
                            args.first()
                                .and_then(extract_int_literal)
                                .unwrap_or(DEFAULT_PAGE_SIZE) as usize
                        }
                        Expr::Literal(assura_parser::ast::Literal::Int(s)) => {
                            s.parse::<usize>().unwrap_or(DEFAULT_PAGE_SIZE as usize)
                        }
                        _ => {
                            let kvs = extract_kv_pairs(&clause.body);
                            kvs.iter()
                                .find(|(k, _)| {
                                    *k == "capacity" || *k == "size" || *k == "max_pages"
                                })
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PAGE_SIZE) as usize
                        }
                    };
                    if checker.is_none() {
                        checker = Some(PageCacheChecker::new(capacity));
                    }
                }
                if let Some(ch) = checker.as_mut()
                    && matches!(
                        clause.kind,
                        ClauseKind::Requires | ClauseKind::Ensures | ClauseKind::Other(_)
                    )
                {
                    Self::scan_expr(&clause.body, ch);
                }
            }
        }
        match checker {
            Some(ch) => ch.check_capacity(),
            None => Vec::new(),
        }
    }
}

impl Default for PageCacheChecker {
    fn default() -> Self {
        Self::new(1024)
    }
}

// ===========================================================================
// T088: STOR.3 MVCC / snapshot isolation
// ===========================================================================

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

// ===========================================================================
// T089: STOR.4 Transactional rollback
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct RollbackChecker {
    savepoints: Vec<String>,
    resources_acquired: Vec<String>,
    rolled_back: bool,
}

impl RollbackChecker {
    pub fn new() -> Self {
        Self {
            savepoints: Vec::new(),
            resources_acquired: Vec::new(),
            rolled_back: false,
        }
    }

    pub fn create_savepoint(&mut self, name: String) {
        self.savepoints.push(name);
    }

    pub fn acquire_resource(&mut self, name: String) {
        self.resources_acquired.push(name);
    }

    pub fn release_resource(&mut self, name: &str) {
        self.resources_acquired.retain(|r| r != name);
    }

    pub fn rollback_to(&mut self, savepoint: &str) -> Option<TypeError> {
        if !self.savepoints.contains(&savepoint.to_string()) {
            return Some(TypeError {
                code: "A36001".into(),
                message: format!("rollback to unknown savepoint `{savepoint}`"),
                span: 0..1,
                secondary: None,
                suggestion: None,
            });
        }
        self.rolled_back = true;
        if let Some(pos) = self.savepoints.iter().position(|s| s == savepoint) {
            self.savepoints.truncate(pos + 1);
        }
        None
    }

    pub fn check_resource_leak(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        if self.rolled_back {
            for r in &self.resources_acquired {
                errors.push(TypeError {
                    code: "A36002".into(),
                    message: format!("resource `{r}` not released after rollback"),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    pub fn check_savepoint_nesting(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for sp in &self.savepoints {
            if !seen.insert(sp.clone()) {
                errors.push(TypeError {
                    code: "A36003".into(),
                    message: format!("duplicate savepoint name `{sp}`"),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }
}

impl RollbackChecker {
    /// Scan an expression for rollback operations.
    fn scan_expr(expr: &SpExpr, checker: &mut RollbackChecker) -> Vec<TypeError> {
        let mut scan_errors = Vec::new();
        if let Some((name, args)) = extract_call(expr) {
            match name {
                "savepoint" | "create_savepoint" => {
                    let sp_name = args
                        .first()
                        .and_then(extract_ident)
                        .unwrap_or("default")
                        .to_string();
                    checker.create_savepoint(sp_name);
                }
                "acquire" | "acquire_resource" | "lock" => {
                    let res_name = args
                        .first()
                        .and_then(extract_ident)
                        .unwrap_or("resource")
                        .to_string();
                    checker.acquire_resource(res_name);
                }
                "release" | "release_resource" | "unlock" => {
                    let res_name = args.first().and_then(extract_ident).unwrap_or("resource");
                    checker.release_resource(res_name);
                }
                "rollback" | "rollback_to" => {
                    let sp_name = args.first().and_then(extract_ident).unwrap_or("default");
                    if let Some(err) = checker.rollback_to(sp_name) {
                        scan_errors.push(err);
                    }
                }
                _ => {}
            }
        }
        if let Expr::Ident(name) = &expr.node {
            checker.create_savepoint(name.clone());
        }
        match &expr.node {
            Expr::Block(exprs) | Expr::List(exprs) => {
                for e in exprs {
                    scan_errors.extend(Self::scan_expr(e, checker));
                }
            }
            Expr::BinOp { lhs, rhs, .. } => {
                scan_errors.extend(Self::scan_expr(lhs, checker));
                scan_errors.extend(Self::scan_expr(rhs, checker));
            }
            Expr::Call { func, args } => {
                scan_errors.extend(Self::scan_expr(func, checker));
                for a in args {
                    scan_errors.extend(Self::scan_expr(a, checker));
                }
            }
            _ => {}
        }
        scan_errors
    }

    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = Self::new();
        let mut scan_errors = Vec::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "rollback" || k == "savepoint" || k == "transactional")
                {
                    found = true;
                    scan_errors.extend(Self::scan_expr(&clause.body, &mut checker));
                }
                if found && matches!(clause.kind, ClauseKind::Requires | ClauseKind::Ensures) {
                    scan_errors.extend(Self::scan_expr(&clause.body, &mut checker));
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let mut errors = scan_errors;
        errors.extend(checker.check_resource_leak());
        errors.extend(checker.check_savepoint_nesting());
        errors
    }
}

impl Default for RollbackChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T090: STOR.5 Monotonic state
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct MonotonicStateChecker {
    monotonic_vars: std::collections::HashMap<String, MonotonicInfo>,
}

#[derive(Debug, Clone)]
pub(crate) struct MonotonicInfo {
    pub current_value: i64,
    pub direction: MonotonicDirection,
    pub span: std::ops::Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MonotonicDirection {
    Increasing,
    StrictlyIncreasing,
    Decreasing,
}

impl MonotonicStateChecker {
    pub fn new() -> Self {
        Self {
            monotonic_vars: std::collections::HashMap::new(),
        }
    }

    pub fn declare(
        &mut self,
        name: String,
        direction: MonotonicDirection,
        initial: i64,
        span: std::ops::Range<usize>,
    ) {
        self.monotonic_vars.insert(
            name,
            MonotonicInfo {
                current_value: initial,
                direction,
                span,
            },
        );
    }

    pub fn update(&mut self, name: &str, new_value: i64) -> Option<TypeError> {
        if let Some(info) = self.monotonic_vars.get_mut(name) {
            let violation = match info.direction {
                MonotonicDirection::Increasing => new_value < info.current_value,
                MonotonicDirection::StrictlyIncreasing => new_value <= info.current_value,
                MonotonicDirection::Decreasing => new_value > info.current_value,
            };
            if violation {
                return Some(TypeError {
                    code: "A37001".into(),
                    message: format!(
                        "monotonicity violation: `{name}` changed from {} to {new_value}",
                        info.current_value
                    ),
                    span: info.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
            info.current_value = new_value;
        }
        None
    }

    pub fn check_reset(&self, name: &str) -> Option<TypeError> {
        if self.monotonic_vars.contains_key(name) {
            Some(TypeError {
                code: "A37002".into(),
                message: format!("illegal reset of monotonic variable `{name}`"),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
        } else {
            None
        }
    }

    pub fn check_access(&self, name: &str) -> Option<TypeError> {
        if !self.monotonic_vars.contains_key(name) {
            Some(TypeError {
                code: "A37003".into(),
                message: format!("access to undeclared monotonic variable `{name}`"),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
        } else {
            None
        }
    }

    pub fn current_value(&self, name: &str) -> Option<i64> {
        self.monotonic_vars.get(name).map(|i| i.current_value)
    }
}

impl MonotonicStateChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = Self::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "monotonic" || k == "monotone" || k == "increasing" {
                        found = true;
                        match &clause.body.node {
                            Expr::Call { func, args } => {
                                if let Expr::Ident(name) = &func.as_ref().node {
                                    let direction = args
                                        .first()
                                        .and_then(extract_ident)
                                        .map(|d| match d {
                                            "strictly_increasing" => {
                                                MonotonicDirection::StrictlyIncreasing
                                            }
                                            "decreasing" => MonotonicDirection::Decreasing,
                                            _ => MonotonicDirection::Increasing,
                                        })
                                        .unwrap_or(MonotonicDirection::Increasing);
                                    let initial = args
                                        .get(1)
                                        .and_then(extract_int_literal)
                                        .unwrap_or(DEFAULT_PARAM_ZERO);
                                    checker.declare(
                                        name.clone(),
                                        direction,
                                        initial,
                                        decl.span.clone(),
                                    );
                                }
                            }
                            Expr::Ident(name) => {
                                checker.declare(
                                    name.clone(),
                                    MonotonicDirection::Increasing,
                                    0,
                                    decl.span.clone(),
                                );
                            }
                            _ => {}
                        }
                    }
                    if (k == "update" || k == "assign" || k == "set")
                        && let Some((name, args)) = extract_call(&clause.body)
                        && let Some(val) = args.first().and_then(extract_int_literal)
                        && let Some(err) = checker.update(name, val)
                    {
                        return vec![err];
                    }
                    if k == "reset"
                        && let Some(name) = extract_ident(&clause.body)
                        && let Some(err) = checker.check_reset(name)
                    {
                        return vec![err];
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let mut errors = Vec::new();
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if clause.kind == ClauseKind::Ensures {
                    let refs = collect_ident_references(&clause.body);
                    for name in &refs {
                        if let Some(mut err) = checker.check_access(name) {
                            if let Some(val) = checker.current_value(name) {
                                err.message.push_str(&format!(" (current value: {val})"));
                            }
                            errors.push(err);
                        }
                    }
                }
            }
        }
        errors
    }
}

impl Default for MonotonicStateChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T091: STOR.6 Storage failure model
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct StorageFailureChecker {
    failure_modes: Vec<FailureMode>,
    handled_modes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FailureMode {
    PartialWrite,
    TornPage,
    BitRot,
    DiskFull,
    IoTimeout,
}

impl FailureMode {
    pub fn name(&self) -> &str {
        match self {
            Self::PartialWrite => "partial_write",
            Self::TornPage => "torn_page",
            Self::BitRot => "bit_rot",
            Self::DiskFull => "disk_full",
            Self::IoTimeout => "io_timeout",
        }
    }
}

impl StorageFailureChecker {
    pub fn new() -> Self {
        Self {
            failure_modes: Vec::new(),
            handled_modes: Vec::new(),
        }
    }

    pub fn declare_failure_mode(&mut self, mode: FailureMode) {
        self.failure_modes.push(mode);
    }

    pub fn mark_handled(&mut self, mode_name: &str) {
        if !self.handled_modes.contains(&mode_name.to_string()) {
            self.handled_modes.push(mode_name.to_string());
        }
    }

    pub fn check_unhandled(&self) -> Vec<TypeError> {
        self.failure_modes
            .iter()
            .filter(|m| !self.handled_modes.contains(&m.name().to_string()))
            .map(|m| TypeError {
                code: "A38001".into(),
                message: format!("storage failure mode `{}` has no handler", m.name()),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn check_spurious_handlers(&self) -> Vec<TypeError> {
        let declared: Vec<_> = self
            .failure_modes
            .iter()
            .map(|m| m.name().to_string())
            .collect();
        self.handled_modes
            .iter()
            .filter(|h| !declared.contains(h))
            .map(|h| TypeError {
                code: "A38002".into(),
                message: format!("handler for undeclared failure mode `{h}`"),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn check_critical_coverage(&self) -> Vec<TypeError> {
        let critical = [FailureMode::PartialWrite, FailureMode::TornPage];
        critical
            .iter()
            .filter(|m| {
                self.failure_modes.contains(m)
                    && !self.handled_modes.contains(&m.name().to_string())
            })
            .map(|m| TypeError {
                code: "A38003".into(),
                message: format!("critical failure mode `{}` must have a handler", m.name()),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
            .collect()
    }
}

impl StorageFailureChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = Self::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "failure_mode" || k == "storage_failure" {
                        found = true;
                        if let Expr::Ident(name) = &clause.body.node {
                            let mode = match name.as_str() {
                                "partial_write" => FailureMode::PartialWrite,
                                "torn_page" => FailureMode::TornPage,
                                "bit_rot" => FailureMode::BitRot,
                                "disk_full" => FailureMode::DiskFull,
                                "io_timeout" => FailureMode::IoTimeout,
                                _ => continue,
                            };
                            checker.declare_failure_mode(mode);
                        }
                    }
                    if (k == "handles" || k == "handles_failure")
                        && let Expr::Ident(name) = &clause.body.node
                    {
                        checker.mark_handled(name);
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let mut errors = checker.check_unhandled();
        errors.extend(checker.check_critical_coverage());
        errors.extend(checker.check_spurious_handlers());
        errors
    }
}

impl Default for StorageFailureChecker {
    fn default() -> Self {
        Self::new()
    }
}
