use super::*;

// T059: SEC.3 Constant-time execution
// ---------------------------------------------------------------------------

/// Error from the constant-time checker.
pub(crate) type ConstantTimeError = CheckerError;

/// Checker for constant-time execution properties.
///
/// Ensures secret-dependent code does not branch on secrets,
/// preventing timing side-channel attacks.
pub(crate) struct ConstantTimeChecker {
    /// Variables classified as secret
    secrets: HashMap<String, bool>,
}

impl ConstantTimeChecker {
    pub fn new() -> Self {
        Self {
            secrets: HashMap::new(),
        }
    }

    /// Mark a variable as secret (timing-sensitive).
    pub fn mark_secret(&mut self, name: String) {
        self.secrets.insert(name, true);
    }

    /// Check if an expression references any secret variable.
    pub fn references_secret(&self, expr: &Expr) -> bool {
        struct SecretChecker<'a> {
            secrets: &'a HashMap<String, bool>,
            found: bool,
        }
        impl ExprVisitor for SecretChecker<'_> {
            fn visit_ident(&mut self, name: &str) {
                if self.secrets.contains_key(name) {
                    self.found = true;
                }
            }
        }
        let mut c = SecretChecker {
            secrets: &self.secrets,
            found: false,
        };
        c.visit_expr(expr);
        c.found
    }

    /// Check that branches do not depend on secret data.
    /// - A14001: branch condition depends on secret data (timing leak)
    pub fn check_branch(&self, condition: &Expr, span: &Range<usize>) -> Vec<ConstantTimeError> {
        let mut errors = Vec::new();
        if self.references_secret(condition) {
            errors.push(ConstantTimeError {
                code: "A14001".into(),
                message: "branch condition depends on secret data; \
                          this creates a timing side-channel"
                    .into(),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that array indexing does not depend on secret data.
    /// - A14002: secret-dependent array index (cache timing leak)
    pub fn check_index(&self, index_expr: &Expr, span: &Range<usize>) -> Vec<ConstantTimeError> {
        let mut errors = Vec::new();
        if self.references_secret(index_expr) {
            errors.push(ConstantTimeError {
                code: "A14002".into(),
                message: "array index depends on secret data; \
                          this creates a cache timing side-channel"
                    .into(),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check a full expression for constant-time violations.
    pub fn check_expr(&self, expr: &Expr, span: &Range<usize>) -> Vec<ConstantTimeError> {
        let mut errors = Vec::new();
        match expr {
            Expr::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                errors.extend(self.check_branch(cond, span));
                errors.extend(self.check_expr(then_branch, span));
                if let Some(e) = else_branch {
                    errors.extend(self.check_expr(e, span));
                }
            }
            Expr::Index { index, .. } => {
                errors.extend(self.check_index(index, span));
            }
            Expr::BinOp { lhs, rhs, .. } => {
                errors.extend(self.check_expr(lhs, span));
                errors.extend(self.check_expr(rhs, span));
            }
            Expr::Call { args, .. } => {
                for a in args {
                    errors.extend(self.check_expr(a, span));
                }
            }
            _ => {}
        }
        errors
    }
}

impl Default for ConstantTimeChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T063: TYPE.2 Recursive structural invariants
// ---------------------------------------------------------------------------

/// A structural invariant on a recursive data structure.
#[derive(Debug, Clone)]
pub(crate) struct StructuralInvariant {
    pub name: String,
    /// The type this invariant applies to
    pub type_name: String,
    /// Kind of structural property
    pub kind: InvariantKind,
}

/// Kinds of structural invariants for recursive types.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InvariantKind {
    /// Tree balance: left depth and right depth differ by at most k
    TreeBalance { max_diff: u32 },
    /// List sortedness: elements in non-decreasing order
    Sorted { descending: bool },
    /// Graph acyclicity: no cycles in the structure
    Acyclic,
    /// Binary search tree: left < node < right
    BstOrdering,
    /// Heap property: parent <= children (or >=)
    HeapProperty { min_heap: bool },
    /// Custom invariant expressed as a predicate string
    Custom(String),
}

impl std::fmt::Display for InvariantKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvariantKind::TreeBalance { max_diff } => {
                write!(f, "tree_balance(max_diff={max_diff})")
            }
            InvariantKind::Sorted { descending } => {
                if *descending {
                    write!(f, "sorted(desc)")
                } else {
                    write!(f, "sorted(asc)")
                }
            }
            InvariantKind::Acyclic => write!(f, "acyclic"),
            InvariantKind::BstOrdering => write!(f, "bst_ordering"),
            InvariantKind::HeapProperty { min_heap } => {
                if *min_heap {
                    write!(f, "min_heap")
                } else {
                    write!(f, "max_heap")
                }
            }
            InvariantKind::Custom(pred) => write!(f, "custom({pred})"),
        }
    }
}

/// Error from the structural invariant checker.
pub(crate) type StructuralInvariantError = CheckerError;

/// Checker for recursive structural invariants.
pub(crate) struct StructuralInvariantChecker {
    /// Registered invariants per type
    invariants: HashMap<String, Vec<StructuralInvariant>>,
    /// Known recursive types (type name -> list of recursive field names)
    recursive_types: HashMap<String, Vec<String>>,
}

impl StructuralInvariantChecker {
    pub fn new() -> Self {
        Self {
            invariants: HashMap::new(),
            recursive_types: HashMap::new(),
        }
    }

    /// Register a type as recursive, listing its self-referencing fields.
    pub fn register_recursive_type(&mut self, type_name: String, recursive_fields: Vec<String>) {
        self.recursive_types.insert(type_name, recursive_fields);
    }

    /// Register a structural invariant on a type.
    pub fn register_invariant(&mut self, inv: StructuralInvariant) {
        self.invariants
            .entry(inv.type_name.clone())
            .or_default()
            .push(inv);
    }

    /// Check that a structural invariant is applicable to the type.
    /// - A15001: invariant on non-recursive type
    /// - A15002: tree invariant on non-tree structure
    /// - A15003: sort invariant on non-sequence structure
    pub fn check_invariant_applicability(
        &self,
        type_name: &str,
        kind: &InvariantKind,
        span: &Range<usize>,
    ) -> Vec<StructuralInvariantError> {
        let mut errors = Vec::new();
        if !self.recursive_types.contains_key(type_name) {
            errors.push(StructuralInvariantError {
                code: "A15001".into(),
                message: format!(
                    "structural invariant `{kind}` applied to non-recursive type `{type_name}`"
                ),
                span: span.clone(),
            });
            return errors;
        }

        let fields = &self.recursive_types[type_name];
        match kind {
            InvariantKind::TreeBalance { .. }
            | InvariantKind::BstOrdering
            | InvariantKind::HeapProperty { .. } => {
                // Tree invariants need at least 2 recursive fields (left, right)
                if fields.len() < 2 {
                    errors.push(StructuralInvariantError {
                        code: "A15002".into(),
                        message: format!(
                            "tree invariant `{kind}` requires at least 2 recursive fields, \
                             but `{type_name}` has {}",
                            fields.len()
                        ),
                        span: span.clone(),
                    });
                }
            }
            InvariantKind::Sorted { .. } => {
                // Sort invariant needs exactly 1 recursive field (next pointer)
                if fields.len() != 1 {
                    errors.push(StructuralInvariantError {
                        code: "A15003".into(),
                        message: format!(
                            "sort invariant requires exactly 1 recursive field (next pointer), \
                             but `{type_name}` has {}",
                            fields.len()
                        ),
                        span: span.clone(),
                    });
                }
            }
            InvariantKind::Acyclic | InvariantKind::Custom(_) => {
                // These are valid for any recursive type
            }
        }
        errors
    }

    /// Check that an operation preserves the structural invariant.
    /// - A15004: operation may violate structural invariant
    pub fn check_operation_preserves(
        &self,
        type_name: &str,
        operation: &str,
        modifies_structure: bool,
        has_preservation_proof: bool,
        span: &Range<usize>,
    ) -> Vec<StructuralInvariantError> {
        let mut errors = Vec::new();
        if !modifies_structure {
            return errors; // Read-only operations preserve invariants trivially
        }
        if let Some(invs) = self.invariants.get(type_name) {
            for inv in invs {
                if !has_preservation_proof {
                    errors.push(StructuralInvariantError {
                        code: "A15004".into(),
                        message: format!(
                            "operation `{operation}` modifies `{type_name}` \
                             but has no proof preserving invariant `{}` ({})",
                            inv.name, inv.kind
                        ),
                        span: span.clone(),
                    });
                }
            }
        }
        errors
    }

    /// Get all invariants for a type (test-only).
    #[cfg(test)]
    pub fn get_invariants(&self, type_name: &str) -> Vec<&StructuralInvariant> {
        self.invariants
            .get(type_name)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }
}

impl Default for StructuralInvariantChecker {
    fn default() -> Self {
        Self::new()
    }
}

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

// ---------------------------------------------------------------------------
// T067: CONC.3 Determinism contracts
// ---------------------------------------------------------------------------

/// Error from the determinism checker.
pub(crate) type DeterminismError = CheckerError;

/// Checker for determinism contracts.
///
/// Ensures functions marked as `deterministic` do not use
/// non-deterministic constructs (HashMap iteration, random,
/// thread-dependent ordering).
pub(crate) struct DeterminismChecker {
    /// Functions marked as deterministic
    deterministic_fns: HashMap<String, bool>,
    /// Known non-deterministic types/functions
    non_det_sources: Vec<String>,
}

impl DeterminismChecker {
    pub fn new() -> Self {
        Self {
            deterministic_fns: HashMap::new(),
            non_det_sources: vec![
                "HashMap".into(),
                "HashSet".into(),
                "random".into(),
                "rand".into(),
                "thread_rng".into(),
                "SystemTime::now".into(),
                "Instant::now".into(),
            ],
        }
    }

    /// Mark a function as requiring deterministic execution.
    pub fn mark_deterministic(&mut self, fn_name: String) {
        self.deterministic_fns.insert(fn_name, true);
    }

    /// Add a custom non-deterministic source.
    pub fn add_non_det_source(&mut self, source: String) {
        self.non_det_sources.push(source);
    }

    /// Check if a type/function name is non-deterministic.
    pub fn is_non_deterministic(&self, name: &str) -> bool {
        self.non_det_sources
            .iter()
            .any(|s| name.contains(s.as_str()))
    }

    /// Check that a deterministic function does not use non-deterministic constructs.
    /// - A20001: deterministic function uses non-deterministic type/call
    pub fn check_fn_body(
        &self,
        fn_name: &str,
        used_names: &[String],
        span: &Range<usize>,
    ) -> Vec<DeterminismError> {
        let mut errors = Vec::new();
        if !self.deterministic_fns.contains_key(fn_name) {
            return errors; // Not marked deterministic, skip
        }
        for name in used_names {
            if self.is_non_deterministic(name) {
                errors.push(DeterminismError {
                    code: "A20001".into(),
                    message: format!(
                        "deterministic function `{fn_name}` uses non-deterministic `{name}`; \
                         use BTreeMap/BTreeSet or a seeded RNG instead"
                    ),
                    span: span.clone(),
                });
            }
        }
        errors
    }

    /// Check that iteration order is deterministic.
    /// - A20002: iterating over HashMap/HashSet in deterministic context
    pub fn check_iteration(
        &self,
        fn_name: &str,
        iterated_type: &str,
        span: &Range<usize>,
    ) -> Vec<DeterminismError> {
        let mut errors = Vec::new();
        if self.deterministic_fns.contains_key(fn_name)
            && (iterated_type.contains("HashMap") || iterated_type.contains("HashSet"))
        {
            errors.push(DeterminismError {
                code: "A20002".into(),
                message: format!(
                    "deterministic function `{fn_name}` iterates over `{iterated_type}` \
                     which has non-deterministic ordering"
                ),
                span: span.clone(),
            });
        }
        errors
    }
}

impl Default for DeterminismChecker {
    fn default() -> Self {
        Self::new()
    }
}

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

// ---------------------------------------------------------------------------
// T060: SEC.4 Secure erasure
// ---------------------------------------------------------------------------

/// Error from the secure erasure checker.
pub(crate) type SecureErasureError = CheckerError;

/// Checker for secure erasure of sensitive data.
///
/// Ensures that linear types marked as sensitive are consumed
/// via zeroize before being dropped, preventing sensitive data
/// from lingering in memory.
pub(crate) struct SecureErasureChecker {
    /// Variables that hold sensitive data and must be zeroized
    sensitive_vars: HashMap<String, bool>,
    /// Variables that have been properly zeroized
    zeroized: HashMap<String, bool>,
}

impl SecureErasureChecker {
    pub fn new() -> Self {
        Self {
            sensitive_vars: HashMap::new(),
            zeroized: HashMap::new(),
        }
    }

    /// Returns the names of all sensitive variables.
    pub fn sensitive_names(&self) -> Vec<String> {
        self.sensitive_vars.keys().cloned().collect()
    }

    /// Mark a variable as holding sensitive data.
    pub fn mark_sensitive(&mut self, name: String) {
        self.sensitive_vars.insert(name, true);
    }

    /// Record that a variable has been zeroized.
    pub fn mark_zeroized(&mut self, name: String) {
        self.zeroized.insert(name, true);
    }

    /// Check that a sensitive variable was zeroized before going out of scope.
    /// - A16001: sensitive variable dropped without zeroization
    pub fn check_scope_exit(&self, var_name: &str, span: &Range<usize>) -> Vec<SecureErasureError> {
        let mut errors = Vec::new();
        if self.sensitive_vars.contains_key(var_name) && !self.zeroized.contains_key(var_name) {
            errors.push(SecureErasureError {
                code: "A16001".into(),
                message: format!(
                    "sensitive variable `{var_name}` dropped without secure erasure; \
                     call zeroize() before the variable goes out of scope"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that a copy of sensitive data is also marked sensitive.
    /// - A16002: sensitive data copied to non-sensitive variable
    pub fn check_copy(
        &self,
        source: &str,
        target: &str,
        target_is_sensitive: bool,
        span: &Range<usize>,
    ) -> Vec<SecureErasureError> {
        let mut errors = Vec::new();
        if self.sensitive_vars.contains_key(source) && !target_is_sensitive {
            errors.push(SecureErasureError {
                code: "A16002".into(),
                message: format!(
                    "sensitive data from `{source}` copied to `{target}` \
                     which is not marked as sensitive; the copy will not be zeroized"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that sensitive data is not leaked through return values.
    /// - A16003: function returns sensitive data without @sensitive annotation
    pub fn check_return(
        &self,
        returned_var: &str,
        fn_return_is_sensitive: bool,
        span: &Range<usize>,
    ) -> Vec<SecureErasureError> {
        let mut errors = Vec::new();
        if self.sensitive_vars.contains_key(returned_var) && !fn_return_is_sensitive {
            errors.push(SecureErasureError {
                code: "A16003".into(),
                message: format!(
                    "function returns sensitive variable `{returned_var}` \
                     but return type is not marked @sensitive"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check all sensitive variables at end of scope.
    pub fn check_all_erased(&self, span: &Range<usize>) -> Vec<SecureErasureError> {
        let mut errors = Vec::new();
        for name in self.sensitive_vars.keys() {
            if !self.zeroized.contains_key(name) {
                errors.push(SecureErasureError {
                    code: "A16001".into(),
                    message: format!("sensitive variable `{name}` dropped without secure erasure"),
                    span: span.clone(),
                });
            }
        }
        errors
    }
}

impl Default for SecureErasureChecker {
    fn default() -> Self {
        Self::new()
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Range<usize> {
        0..10
    }

    fn ident(s: &str) -> Expr {
        Expr::Ident(s.to_string())
    }

    fn int_lit(n: i64) -> Expr {
        Expr::Literal(Literal::Int(n.to_string()))
    }

    // ---- ConstantTimeChecker ----

    #[test]
    fn ct_no_secret_no_error() {
        let checker = ConstantTimeChecker::new();
        let errs = checker.check_branch(&ident("x"), &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn ct_branch_on_secret() {
        let mut checker = ConstantTimeChecker::new();
        checker.mark_secret("key".into());
        let errs = checker.check_branch(&ident("key"), &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A14001");
    }

    #[test]
    fn ct_index_on_secret() {
        let mut checker = ConstantTimeChecker::new();
        checker.mark_secret("secret_idx".into());
        let errs = checker.check_index(&ident("secret_idx"), &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A14002");
    }

    #[test]
    fn ct_check_expr_if_with_secret_condition() {
        let mut checker = ConstantTimeChecker::new();
        checker.mark_secret("s".into());
        let expr = Expr::If {
            cond: Box::new(ident("s")),
            then_branch: Box::new(int_lit(1)),
            else_branch: Some(Box::new(int_lit(0))),
        };
        let errs = checker.check_expr(&expr, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A14001");
    }

    #[test]
    fn ct_references_secret_in_binop() {
        let mut checker = ConstantTimeChecker::new();
        checker.mark_secret("pw".into());
        let expr = Expr::BinOp {
            lhs: Box::new(ident("pw")),
            op: BinOp::Add,
            rhs: Box::new(int_lit(1)),
        };
        assert!(checker.references_secret(&expr));
    }

    #[test]
    fn ct_no_secret_reference() {
        let checker = ConstantTimeChecker::new();
        assert!(!checker.references_secret(&ident("x")));
    }

    // ---- StructuralInvariantChecker ----

    #[test]
    fn si_invariant_on_non_recursive_type() {
        let checker = StructuralInvariantChecker::new();
        let errs = checker.check_invariant_applicability("Flat", &InvariantKind::Acyclic, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A15001");
    }

    #[test]
    fn si_tree_invariant_needs_two_fields() {
        let mut checker = StructuralInvariantChecker::new();
        checker.register_recursive_type("LinkedList".into(), vec!["next".into()]);
        let errs = checker.check_invariant_applicability(
            "LinkedList",
            &InvariantKind::TreeBalance { max_diff: 1 },
            &span(),
        );
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A15002");
    }

    #[test]
    fn si_tree_invariant_ok_with_two_fields() {
        let mut checker = StructuralInvariantChecker::new();
        checker.register_recursive_type("Tree".into(), vec!["left".into(), "right".into()]);
        let errs =
            checker.check_invariant_applicability("Tree", &InvariantKind::BstOrdering, &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn si_sort_invariant_needs_one_field() {
        let mut checker = StructuralInvariantChecker::new();
        checker.register_recursive_type("Tree".into(), vec!["left".into(), "right".into()]);
        let errs = checker.check_invariant_applicability(
            "Tree",
            &InvariantKind::Sorted { descending: false },
            &span(),
        );
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A15003");
    }

    #[test]
    fn si_operation_preserves_without_proof() {
        let mut checker = StructuralInvariantChecker::new();
        checker.register_recursive_type("Tree".into(), vec!["left".into(), "right".into()]);
        checker.register_invariant(StructuralInvariant {
            name: "balanced".into(),
            type_name: "Tree".into(),
            kind: InvariantKind::TreeBalance { max_diff: 1 },
        });
        let errs = checker.check_operation_preserves("Tree", "insert", true, false, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A15004");
    }

    #[test]
    fn si_readonly_operation_no_error() {
        let mut checker = StructuralInvariantChecker::new();
        checker.register_recursive_type("Tree".into(), vec!["left".into(), "right".into()]);
        checker.register_invariant(StructuralInvariant {
            name: "balanced".into(),
            type_name: "Tree".into(),
            kind: InvariantKind::TreeBalance { max_diff: 1 },
        });
        let errs = checker.check_operation_preserves("Tree", "lookup", false, false, &span());
        assert!(errs.is_empty());
    }

    // ---- SharedMemChecker ----

    #[test]
    fn sm_read_without_access() {
        let checker = SharedMemChecker::new();
        let errs = checker.check_read("buf", &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A18001");
    }

    #[test]
    fn sm_read_with_shared_read() {
        let mut checker = SharedMemChecker::new();
        checker.set_mode("buf".into(), AccessMode::SharedRead);
        let errs = checker.check_read("buf", &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn sm_write_without_exclusive() {
        let mut checker = SharedMemChecker::new();
        checker.set_mode("buf".into(), AccessMode::SharedRead);
        let errs = checker.check_write("buf", &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A18002");
    }

    #[test]
    fn sm_write_with_exclusive() {
        let mut checker = SharedMemChecker::new();
        checker.set_mode("buf".into(), AccessMode::Exclusive);
        let errs = checker.check_write("buf", &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn sm_data_race_exclusive_exclusive() {
        let checker = SharedMemChecker::new();
        let errs = checker.check_data_race(
            "shared",
            AccessMode::Exclusive,
            AccessMode::Exclusive,
            &span(),
        );
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A18003");
    }

    #[test]
    fn sm_no_race_shared_read() {
        let checker = SharedMemChecker::new();
        let errs = checker.check_data_race(
            "shared",
            AccessMode::SharedRead,
            AccessMode::SharedRead,
            &span(),
        );
        assert!(errs.is_empty());
    }

    // ---- DeterminismChecker ----

    #[test]
    fn det_non_deterministic_sources() {
        let checker = DeterminismChecker::new();
        assert!(checker.is_non_deterministic("HashMap"));
        assert!(checker.is_non_deterministic("HashSet"));
        assert!(checker.is_non_deterministic("random"));
        assert!(!checker.is_non_deterministic("BTreeMap"));
    }

    #[test]
    fn det_fn_body_with_hash_map() {
        let mut checker = DeterminismChecker::new();
        checker.mark_deterministic("pure_fn".into());
        let errs =
            checker.check_fn_body("pure_fn", &["HashMap".into(), "BTreeMap".into()], &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A20001");
    }

    #[test]
    fn det_unmarked_fn_no_error() {
        let checker = DeterminismChecker::new();
        let errs = checker.check_fn_body("any_fn", &["HashMap".into()], &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn det_iteration_over_hashset() {
        let mut checker = DeterminismChecker::new();
        checker.mark_deterministic("pure_fn".into());
        let errs = checker.check_iteration("pure_fn", "HashSet<i32>", &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A20002");
    }

    #[test]
    fn det_custom_non_det_source() {
        let mut checker = DeterminismChecker::new();
        checker.add_non_det_source("getrandom".into());
        assert!(checker.is_non_deterministic("getrandom"));
    }

    // ---- LockOrderChecker ----

    #[test]
    fn lock_acquire_in_order() {
        let mut checker = LockOrderChecker::new();
        checker.define_order("A".into(), 1);
        checker.define_order("B".into(), 2);
        let errs = checker.acquire("A", &span());
        assert!(errs.is_empty());
        let errs = checker.acquire("B", &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn lock_acquire_out_of_order() {
        let mut checker = LockOrderChecker::new();
        checker.define_order("A".into(), 1);
        checker.define_order("B".into(), 2);
        let errs = checker.acquire("B", &span());
        assert!(errs.is_empty());
        let errs = checker.acquire("A", &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A21001");
    }

    #[test]
    fn lock_release_out_of_order() {
        let mut checker = LockOrderChecker::new();
        checker.define_order("A".into(), 1);
        checker.define_order("B".into(), 2);
        let _ = checker.acquire("A", &span());
        let _ = checker.acquire("B", &span());
        let errs = checker.release("A", &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A21002");
    }

    #[test]
    fn lock_release_correct_order() {
        let mut checker = LockOrderChecker::new();
        checker.define_order("A".into(), 1);
        checker.define_order("B".into(), 2);
        let _ = checker.acquire("A", &span());
        let _ = checker.acquire("B", &span());
        let errs = checker.release("B", &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn lock_ordering_undefined() {
        let checker = LockOrderChecker::new();
        let errs = checker.check_ordering_defined("unknown_lock", &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A21003");
    }

    // ---- SecureErasureChecker ----

    #[test]
    fn se_sensitive_not_zeroized() {
        let mut checker = SecureErasureChecker::new();
        checker.mark_sensitive("key".into());
        let errs = checker.check_scope_exit("key", &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A16001");
    }

    #[test]
    fn se_sensitive_zeroized() {
        let mut checker = SecureErasureChecker::new();
        checker.mark_sensitive("key".into());
        checker.mark_zeroized("key".into());
        let errs = checker.check_scope_exit("key", &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn se_copy_to_non_sensitive() {
        let mut checker = SecureErasureChecker::new();
        checker.mark_sensitive("key".into());
        let errs = checker.check_copy("key", "buf", false, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A16002");
    }

    #[test]
    fn se_copy_to_sensitive_ok() {
        let mut checker = SecureErasureChecker::new();
        checker.mark_sensitive("key".into());
        let errs = checker.check_copy("key", "key_copy", true, &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn se_return_without_annotation() {
        let mut checker = SecureErasureChecker::new();
        checker.mark_sensitive("key".into());
        let errs = checker.check_return("key", false, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A16003");
    }

    #[test]
    fn se_check_all_erased() {
        let mut checker = SecureErasureChecker::new();
        checker.mark_sensitive("a".into());
        checker.mark_sensitive("b".into());
        checker.mark_zeroized("a".into());
        let errs = checker.check_all_erased(&span());
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("b"));
    }

    #[test]
    fn se_sensitive_names() {
        let mut checker = SecureErasureChecker::new();
        checker.mark_sensitive("x".into());
        checker.mark_sensitive("y".into());
        let names = checker.sensitive_names();
        assert_eq!(names.len(), 2);
    }

    // ---- CryptoConformanceChecker ----

    #[test]
    fn crypto_correct_key_size() {
        let checker = CryptoConformanceChecker::new();
        let errs = checker.check_key_size("AES-128-GCM", 128, &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn crypto_wrong_key_size() {
        let checker = CryptoConformanceChecker::new();
        let errs = checker.check_key_size("AES-128-GCM", 256, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A17001");
    }

    #[test]
    fn crypto_correct_nonce_size() {
        let checker = CryptoConformanceChecker::new();
        let errs = checker.check_nonce_size("AES-256-GCM", 12, &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn crypto_wrong_nonce_size() {
        let checker = CryptoConformanceChecker::new();
        let errs = checker.check_nonce_size("AES-256-GCM", 16, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A17002");
    }

    #[test]
    fn crypto_nonce_not_unique() {
        let checker = CryptoConformanceChecker::new();
        let errs = checker.check_nonce_uniqueness("static_nonce", false, false, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A17003");
    }

    #[test]
    fn crypto_nonce_counter_ok() {
        let checker = CryptoConformanceChecker::new();
        let errs = checker.check_nonce_uniqueness("counter", true, false, &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn crypto_tag_not_verified() {
        let checker = CryptoConformanceChecker::new();
        let errs = checker.check_tag_verification(false, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A17004");
    }

    #[test]
    fn crypto_tag_verified_ok() {
        let checker = CryptoConformanceChecker::new();
        let errs = checker.check_tag_verification(true, &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn crypto_custom_spec() {
        let mut checker = CryptoConformanceChecker::new();
        checker.register_spec(CryptoSpec {
            name: "MyAlgo".into(),
            key_size_bits: vec![512],
            block_size_bytes: Some(64),
            nonce_size_bytes: Some(24),
            tag_size_bytes: Some(32),
        });
        let errs = checker.check_key_size("MyAlgo", 512, &span());
        assert!(errs.is_empty());
        let errs = checker.check_key_size("MyAlgo", 256, &span());
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn crypto_unknown_algorithm_no_error() {
        let checker = CryptoConformanceChecker::new();
        let errs = checker.check_key_size("UnknownAlgo", 42, &span());
        assert!(errs.is_empty());
    }
}
