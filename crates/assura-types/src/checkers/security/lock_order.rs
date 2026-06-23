use super::*;

// ---------------------------------------------------------------------------
// T068: CONC.4 Lock ordering
// ---------------------------------------------------------------------------

/// Error from the lock ordering checker.
pub(crate) type LockOrderError = CheckerError;

/// Checker for static lock ordering.
///
/// Prevents deadlocks by enforcing a total order on lock acquisitions.
pub(crate) struct LockOrderChecker {
    /// Lock hierarchy: name -> priority (lower = acquire first)
    lock_order: HashMap<String, u32>,
    /// Currently held locks (name, priority)
    held: Vec<(String, u32)>,
}

impl LockOrderChecker {
    pub fn new() -> Self {
        Self {
            lock_order: HashMap::new(),
            held: Vec::new(),
        }
    }

    /// Define the lock hierarchy. Locks with lower priority must be acquired first.
    pub fn define_order(&mut self, lock_name: String, priority: u32) {
        self.lock_order.insert(lock_name, priority);
    }

    /// Record acquiring a lock. Check ordering.
    /// - A21001: lock acquired out of order (deadlock risk)
    pub fn acquire(&mut self, lock_name: &str, span: &Range<usize>) -> Vec<LockOrderError> {
        let mut errors = Vec::new();
        let priority = self.lock_order.get(lock_name).copied().unwrap_or(u32::MAX);

        // Check that we're not acquiring a lower-priority lock while holding higher
        if let Some((held_name, held_priority)) = self.held.last().filter(|(_, hp)| priority <= *hp)
        {
            errors.push(LockOrderError {
                code: "A21001".into(),
                message: format!(
                    "lock `{lock_name}` (priority {priority}) acquired while holding \
                     `{held_name}` (priority {held_priority}); violates lock ordering"
                ),
                span: span.clone(),
            });
        }

        self.held.push((lock_name.to_string(), priority));
        errors
    }

    /// Record releasing a lock.
    /// - A21002: lock released out of order (must release in reverse acquisition order)
    pub fn release(&mut self, lock_name: &str, span: &Range<usize>) -> Vec<LockOrderError> {
        let mut errors = Vec::new();
        if let Some((top_name, _)) = self.held.last().filter(|(n, _)| n != lock_name) {
            errors.push(LockOrderError {
                code: "A21002".into(),
                message: format!(
                    "lock `{lock_name}` released while `{top_name}` is still held; \
                     release in reverse acquisition order"
                ),
                span: span.clone(),
            });
        }
        self.held.retain(|(n, _)| n != lock_name);
        errors
    }

    /// Check that no lock is known but unordered.
    /// - A21003: lock used without defined order
    pub fn check_ordering_defined(
        &self,
        lock_name: &str,
        span: &Range<usize>,
    ) -> Vec<LockOrderError> {
        let mut errors = Vec::new();
        if !self.lock_order.contains_key(lock_name) {
            errors.push(LockOrderError {
                code: "A21003".into(),
                message: format!(
                    "lock `{lock_name}` used without a defined ordering; \
                     add it to the lock hierarchy"
                ),
                span: span.clone(),
            });
        }
        errors
    }
}

impl Default for LockOrderChecker {
    fn default() -> Self {
        Self::new()
    }
}
