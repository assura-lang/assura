use super::*;

// ---------------------------------------------------------------------------
// T065: CONC.1 Shared memory protocols
// ---------------------------------------------------------------------------

/// Access mode for a shared object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccessMode {
    /// Exclusive read-write access (no other readers/writers)
    Exclusive,
    /// Shared read-only access (multiple readers, no writers)
    SharedRead,
    /// No access (object is locked by another thread)
    None,
}

impl std::fmt::Display for AccessMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccessMode::Exclusive => write!(f, "exclusive"),
            AccessMode::SharedRead => write!(f, "shared_read"),
            AccessMode::None => write!(f, "none"),
        }
    }
}

/// Error from the shared memory checker.
pub(crate) type SharedMemError = CheckerError;

/// Checker for shared memory protocols.
///
/// Validates that concurrent accesses to shared objects follow
/// the declared protocol: no data races, no concurrent writes.
pub(crate) struct SharedMemChecker {
    /// Per-object access modes
    object_modes: HashMap<String, AccessMode>,
}

impl SharedMemChecker {
    pub fn new() -> Self {
        Self {
            object_modes: HashMap::new(),
        }
    }

    /// Set the current access mode for an object.
    pub fn set_mode(&mut self, object: String, mode: AccessMode) {
        self.object_modes.insert(object, mode);
    }

    /// Check that a read access is valid for the current mode.
    /// - A18001: read without shared_read or exclusive access
    pub fn check_read(&self, object: &str, span: &Range<usize>) -> Vec<SharedMemError> {
        let mut errors = Vec::new();
        match self.object_modes.get(object) {
            Some(AccessMode::Exclusive | AccessMode::SharedRead) => {}
            Some(AccessMode::None) | None => {
                errors.push(SharedMemError {
                    code: "A18001".into(),
                    message: format!(
                        "read access to `{object}` without acquiring shared_read or exclusive mode"
                    ),
                    span: span.clone(),
                });
            }
        }
        errors
    }

    /// Check that a write access is valid for the current mode.
    /// - A18002: write without exclusive access
    pub fn check_write(&self, object: &str, span: &Range<usize>) -> Vec<SharedMemError> {
        let mut errors = Vec::new();
        if self.object_modes.get(object) != Some(&AccessMode::Exclusive) {
            errors.push(SharedMemError {
                code: "A18002".into(),
                message: format!(
                    "write access to `{object}` without exclusive mode; \
                     acquire exclusive access before writing"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check for potential data race: two threads accessing the same object.
    /// - A18003: data race (concurrent write + read or write + write)
    pub fn check_data_race(
        &self,
        object: &str,
        thread_a_mode: AccessMode,
        thread_b_mode: AccessMode,
        span: &Range<usize>,
    ) -> Vec<SharedMemError> {
        let mut errors = Vec::new();
        let is_race = matches!(
            (thread_a_mode, thread_b_mode),
            (
                AccessMode::Exclusive,
                AccessMode::Exclusive | AccessMode::SharedRead
            ) | (AccessMode::SharedRead, AccessMode::Exclusive)
        );
        if is_race {
            errors.push(SharedMemError {
                code: "A18003".into(),
                message: format!(
                    "potential data race on `{object}`: thread A has {thread_a_mode} \
                     while thread B has {thread_b_mode}"
                ),
                span: span.clone(),
            });
        }
        errors
    }
}

impl Default for SharedMemChecker {
    fn default() -> Self {
        Self::new()
    }
}
