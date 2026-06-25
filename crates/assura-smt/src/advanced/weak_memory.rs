// ===========================================================================
// T092: CONC.6 Weak memory ordering
// ===========================================================================

/// Models C++ memory ordering semantics for verification.
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryOrdering {
    Relaxed,
    Acquire,
    Release,
    AcqRel,
    SeqCst,
}

/// A memory access with its ordering constraint.
#[derive(Debug, Clone)]
pub struct MemoryAccess {
    pub thread_id: u64,
    pub variable: String,
    pub is_write: bool,
    pub ordering: MemoryOrdering,
    pub sequence_num: u64,
}

/// Verifies weak memory ordering contracts.
#[derive(Debug, Clone)]
pub struct WeakMemoryChecker {
    accesses: Vec<MemoryAccess>,
    happens_before: Vec<(u64, u64)>,
    next_seq: u64,
}

impl WeakMemoryChecker {
    pub fn new() -> Self {
        Self {
            accesses: Vec::new(),
            happens_before: Vec::new(),
            next_seq: 0,
        }
    }

    pub fn record_access(
        &mut self,
        thread_id: u64,
        variable: String,
        is_write: bool,
        ordering: MemoryOrdering,
    ) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.accesses.push(MemoryAccess {
            thread_id,
            variable,
            is_write,
            ordering,
            sequence_num: seq,
        });
        seq
    }

    pub fn add_happens_before(&mut self, before: u64, after: u64) {
        self.happens_before.push((before, after));
    }

    fn is_ordered(&self, a: u64, b: u64) -> bool {
        self.happens_before.iter().any(|&(x, y)| x == a && y == b)
    }

    /// Check for data races: concurrent accesses to same variable with at least one write
    /// and no happens-before relationship.
    pub fn check_data_races(&self) -> Vec<String> {
        let mut races = Vec::new();
        for i in 0..self.accesses.len() {
            for j in (i + 1)..self.accesses.len() {
                let a = &self.accesses[i];
                let b = &self.accesses[j];
                if a.variable == b.variable
                    && a.thread_id != b.thread_id
                    && (a.is_write || b.is_write)
                    && !self.is_ordered(a.sequence_num, b.sequence_num)
                    && !self.is_ordered(b.sequence_num, a.sequence_num)
                {
                    races.push(format!(
                        "data race on `{}` between thread {} and thread {}",
                        a.variable, a.thread_id, b.thread_id
                    ));
                }
            }
        }
        races
    }

    /// Check that release-acquire pairs are consistent.
    pub fn check_release_acquire(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        for a in &self.accesses {
            if a.ordering == MemoryOrdering::Release && a.is_write {
                let has_acquire = self.accesses.iter().any(|b| {
                    b.variable == a.variable
                        && !b.is_write
                        && b.thread_id != a.thread_id
                        && b.ordering == MemoryOrdering::Acquire
                });
                if !has_acquire {
                    warnings.push(format!(
                        "release write on `{}` (thread {}) has no matching acquire read",
                        a.variable, a.thread_id
                    ));
                }
            }
        }
        warnings
    }

    /// Check for relaxed accesses that should be stronger.
    pub fn check_ordering_strength(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        for a in &self.accesses {
            if a.ordering == MemoryOrdering::Relaxed && a.is_write {
                let read_by_other = self
                    .accesses
                    .iter()
                    .any(|b| b.variable == a.variable && b.thread_id != a.thread_id && !b.is_write);
                if read_by_other {
                    warnings.push(format!("relaxed write on `{}` (thread {}) is read by another thread; consider Release ordering", a.variable, a.thread_id));
                }
            }
        }
        warnings
    }

    pub fn access_count(&self) -> usize {
        self.accesses.len()
    }
}

impl Default for WeakMemoryChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wmc_new_is_empty() {
        let wmc = WeakMemoryChecker::new();
        assert_eq!(wmc.access_count(), 0);
    }

    #[test]
    fn wmc_default_is_empty() {
        let wmc = WeakMemoryChecker::default();
        assert_eq!(wmc.access_count(), 0);
    }

    #[test]
    fn wmc_record_access_increments_count() {
        let mut wmc = WeakMemoryChecker::new();
        wmc.record_access(0, "x".into(), true, MemoryOrdering::SeqCst);
        assert_eq!(wmc.access_count(), 1);
    }

    #[test]
    fn wmc_record_access_returns_sequence_numbers() {
        let mut wmc = WeakMemoryChecker::new();
        let s0 = wmc.record_access(0, "x".into(), true, MemoryOrdering::Relaxed);
        let s1 = wmc.record_access(0, "y".into(), false, MemoryOrdering::Relaxed);
        assert_eq!(s0, 0);
        assert_eq!(s1, 1);
    }

    #[test]
    fn wmc_data_race_detected() {
        let mut wmc = WeakMemoryChecker::new();
        wmc.record_access(0, "x".into(), true, MemoryOrdering::Relaxed);
        wmc.record_access(1, "x".into(), false, MemoryOrdering::Relaxed);
        let races = wmc.check_data_races();
        assert_eq!(races.len(), 1);
        assert!(races[0].contains("data race on `x`"));
    }

    #[test]
    fn wmc_no_race_same_thread() {
        let mut wmc = WeakMemoryChecker::new();
        wmc.record_access(0, "x".into(), true, MemoryOrdering::Relaxed);
        wmc.record_access(0, "x".into(), false, MemoryOrdering::Relaxed);
        assert!(wmc.check_data_races().is_empty());
    }

    #[test]
    fn wmc_no_race_both_reads() {
        let mut wmc = WeakMemoryChecker::new();
        wmc.record_access(0, "x".into(), false, MemoryOrdering::Relaxed);
        wmc.record_access(1, "x".into(), false, MemoryOrdering::Relaxed);
        assert!(wmc.check_data_races().is_empty());
    }

    #[test]
    fn wmc_no_race_with_happens_before() {
        let mut wmc = WeakMemoryChecker::new();
        let s0 = wmc.record_access(0, "x".into(), true, MemoryOrdering::Release);
        let s1 = wmc.record_access(1, "x".into(), false, MemoryOrdering::Acquire);
        wmc.add_happens_before(s0, s1);
        assert!(wmc.check_data_races().is_empty());
    }

    #[test]
    fn wmc_release_without_acquire() {
        let mut wmc = WeakMemoryChecker::new();
        wmc.record_access(0, "flag".into(), true, MemoryOrdering::Release);
        let warnings = wmc.check_release_acquire();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("no matching acquire"));
    }

    #[test]
    fn wmc_release_with_acquire_ok() {
        let mut wmc = WeakMemoryChecker::new();
        wmc.record_access(0, "flag".into(), true, MemoryOrdering::Release);
        wmc.record_access(1, "flag".into(), false, MemoryOrdering::Acquire);
        assert!(wmc.check_release_acquire().is_empty());
    }

    #[test]
    fn wmc_relaxed_write_read_by_other_thread() {
        let mut wmc = WeakMemoryChecker::new();
        wmc.record_access(0, "data".into(), true, MemoryOrdering::Relaxed);
        wmc.record_access(1, "data".into(), false, MemoryOrdering::Relaxed);
        let warnings = wmc.check_ordering_strength();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("consider Release ordering"));
    }

    #[test]
    fn wmc_seqcst_no_ordering_warning() {
        let mut wmc = WeakMemoryChecker::new();
        wmc.record_access(0, "data".into(), true, MemoryOrdering::SeqCst);
        wmc.record_access(1, "data".into(), false, MemoryOrdering::SeqCst);
        assert!(wmc.check_ordering_strength().is_empty());
    }
}
