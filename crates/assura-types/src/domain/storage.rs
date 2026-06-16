//! Storage-related domain checkers.
//!
//! CrashRecoveryChecker, PageCacheChecker, MvccChecker,
//! RollbackChecker, MonotonicStateChecker, StorageFailureChecker.

use crate::TypeError;

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
                });
            }
            if p.dirty {
                return Some(TypeError {
                    code: "A34002".into(),
                    message: format!("evicting dirty page {page_id} without flush"),
                    span: 0..1,
                    secondary: None,
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
            }]
        } else {
            vec![]
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
                    errors.push(TypeError { code: "A35003".into(), message: format!("phantom read: txn {txn_id} sees committed version from later txn {} on `{key}`", v.txn_id), span: 0..1, secondary: None });
                }
            }
        }
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
                });
            }
        }
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
            })
        } else {
            None
        }
    }

    pub fn current_value(&self, name: &str) -> Option<i64> {
        self.monotonic_vars.get(name).map(|i| i.current_value)
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
            })
            .collect()
    }
}

impl Default for StorageFailureChecker {
    fn default() -> Self {
        Self::new()
    }
}
